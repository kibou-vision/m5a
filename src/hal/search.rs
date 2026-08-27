//! Tavily への web 検索リクエストの実行。
//!
//! 電文の組み立てと結果の解析は [`m5a_core::search`] に任せ、ここでは
//! 実際の HTTPS 通信だけを行う。

use std::sync::mpsc::{channel, Receiver};
use std::thread;

use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::io::Write;
use embedded_svc::utils::io::try_read_full;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use m5a_core::search::{self, SearchRequest};

/// 検索スレッドに割く作業領域。TLS ハンドシェイクとJSONの組み立てに足りる
/// 範囲で切り詰める。
const TASK_STACK: usize = 12 * 1024;
/// 応答本文の読み取り上限。1件の要約だけを求めているので大きくしない。
const RESPONSE_BUFFER: usize = 4 * 1024;

/// 別スレッドで問い合わせ、結果が出たらチャンネルに1件だけ流す。
///
/// メインループはノンブロッキングなポーリングが前提のため、
/// [`crate::hal::session`] の受信と同じ「重い通信はスレッドへ、結果は
/// チャンネルで受け取る」構成に揃える。通信や解析に失敗しても `None` を
/// 流し、呼び出し側が「わからなかった」で会話を続けられるようにする。
pub fn spawn(request: SearchRequest) -> Receiver<Option<String>> {
    let (sender, receiver) = channel();

    let spawned = thread::Builder::new()
        .stack_size(TASK_STACK)
        .spawn(move || {
            let outcome = fetch(&request).unwrap_or_else(|error| {
                log::warn!("web検索に失敗しました: {error:#}");
                None
            });
            let _ = sender.send(outcome);
        });

    if let Err(error) = spawned {
        log::warn!("web検索の仕事を始められません: {error}");
    }

    receiver
}

/// Tavily に問い合わせ、要約を取り出す。
fn fetch(request: &SearchRequest) -> anyhow::Result<Option<String>> {
    let config = HttpConfiguration {
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut client = HttpClient::wrap(EspHttpConnection::new(&config)?);

    let headers: Vec<(&str, &str)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    let mut http_request = client.post(&request.url, &headers)?;
    http_request.write_all(request.body.as_bytes())?;
    http_request.flush()?;
    let mut response = http_request.submit()?;

    let mut buf = [0u8; RESPONSE_BUFFER];
    let bytes_read = try_read_full(&mut response, &mut buf).map_err(|(error, _)| error)?;
    let body = std::str::from_utf8(&buf[..bytes_read])?;

    Ok(search::parse_response(body))
}
