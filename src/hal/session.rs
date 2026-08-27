//! Realtime API との WebSocket 接続。
//!
//! 受け取った電文の解釈は [`m5a_core::realtime`] に任せ、ここでは
//! 繋ぐことと、届いた出来事を待ち行列へ流すことだけを行う。

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, Result};
use esp_idf_svc::sys::esp_crt_bundle_attach;
use esp_idf_svc::ws::client::{
    EspWebSocketClient, EspWebSocketClientConfig, WebSocketEvent, WebSocketEventType,
};
use esp_idf_svc::ws::FrameType;
use m5a_core::realtime::{self, ServerEvent, SessionSetup};

/// 受信バッファ。
///
/// これを超える電文は分割されて届く。組み立て直しはこちらで行うため、
/// 内部メモリを空けられる大きさに留める。
const RECEIVE_BUFFER: usize = 8 * 1024;
/// 組み立て中の電文が膨らみすぎたら諦める大きさ。
/// 断片を取りこぼすと永久に完成しないため、上限を設けて捨てる。
const MAX_ASSEMBLED: usize = 128 * 1024;
/// 接続を待つ上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// 通信が滞ったとみなすまでの時間。指定しないと警告とともに既定値が使われる。
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);
/// 受信を捌く仕事の作業領域。
/// 内部メモリは LVGL の描画バッファと Wi-Fi で逼迫しているため、
/// TLS と JSON の解析に足りる範囲で切り詰める。
const TASK_STACK: usize = 6 * 1024;

/// 開いている対話セッション。落とすと切れる。
pub struct Session {
    client: EspWebSocketClient<'static>,
    events: Receiver<ServerEvent>,
}

impl Session {
    /// 接続を始める。
    ///
    /// 設定を送るのは接続が確立してからにする。`new` は繋ぎ終える前に
    /// 戻ってくるため、ここで送ると「まだ繋がっていない」と拒まれる。
    /// サーバは接続時に `session.created` を寄越すので、それを合図にする。
    pub fn open(setup: &SessionSetup, api_key: &str) -> Result<Self> {
        let (sender, events) = channel();

        // ヘッダは接続時に C 側へ写し取られるため、ここで借りるだけでよい。
        let headers = realtime::build_auth_header(api_key);
        let url = realtime::build_endpoint_url(&setup.model);

        let config = EspWebSocketClientConfig {
            headers: Some(&headers),
            crt_bundle_attach: Some(esp_crt_bundle_attach),
            buffer_size: RECEIVE_BUFFER,
            task_stack: TASK_STACK,
            reconnect_timeout_ms: NETWORK_TIMEOUT,
            network_timeout_ms: NETWORK_TIMEOUT,
            ..Default::default()
        };

        // 分割された電文を継ぎ足す場所。受信を捌く仕事の中だけで使う。
        let mut assembling = String::new();

        let client = EspWebSocketClient::new(&url, &config, CONNECT_TIMEOUT, move |event| {
            forward(event, &sender, &mut assembling);
        })
        .context("OpenAI に繋がりません。APIキーとネットワークを確かめてください")?;

        Ok(Session { client, events })
    }

    /// こちらの設定を送る。接続が確立してから呼ぶこと。
    pub fn configure(&mut self, setup: &SessionSetup) -> Result<()> {
        self.send(&realtime::build_session_update(setup))
            .context("対話の設定を送れません")
    }

    /// 組み立て済みの電文を送る。
    pub fn send(&mut self, message: &str) -> Result<()> {
        self.client
            .send(FrameType::Text(false), message.as_bytes())
            .context("OpenAI へ送れません")?;
        Ok(())
    }

    /// 届いている出来事を1件取り出す。無ければ `None`。
    pub fn poll(&mut self) -> Option<ServerEvent> {
        self.events.try_recv().ok()
    }

}

/// 届いた断片を継ぎ足し、電文として読めたら流す。
///
/// 受信バッファを超える電文は複数回に分けて届く。使っている
/// WebSocket の実装は分割の情報を渡してくれないため、
/// 「読めるようになるまで継ぎ足す」ことで組み立て直す。
fn assemble(payload: &str, sender: &Sender<ServerEvent>, assembling: &mut String) {
    assembling.push_str(payload);

    match realtime::parse_server_event(assembling) {
        Ok(ServerEvent::Ignored) => assembling.clear(),
        Ok(parsed) => {
            assembling.clear();
            let _ = sender.send(parsed);
        }
        Err(_) if assembling.len() >= MAX_ASSEMBLED => {
            // 断片を取りこぼすと永久に完成しない。捨てて次の電文に備える。
            log::warn!("電文を組み立てられないので捨てました（{} バイト）", assembling.len());
            assembling.clear();
        }
        // まだ途中。次の断片を待つ。
        Err(_) => {}
    }
}

/// 受信した電文を解釈して待ち行列へ流す。
///
/// この関数は受信用の仕事から呼ばれる。重い処理はせず、渡すだけにする。
fn forward(
    event: &Result<WebSocketEvent<'_>, esp_idf_svc::io::EspIOError>,
    sender: &Sender<ServerEvent>,
    assembling: &mut String,
) {
    let Ok(event) = event else {
        // 接続の途中経過でも失敗として届くため、記録に留める。
        log::debug!("受信に失敗しました");
        return;
    };

    match event.event_type {
        WebSocketEventType::Text(payload) => {
            assemble(payload, sender, assembling);
        }
        WebSocketEventType::Binary(payload) => {
            log::info!("二進の受信 {} バイト", payload.len());
        }
        WebSocketEventType::Connected => log::info!("OpenAI に繋がりました"),
        WebSocketEventType::Disconnected => log::warn!("OpenAI との接続が切れました"),
        WebSocketEventType::Closed => log::warn!("OpenAI が接続を閉じました"),
        WebSocketEventType::Close(reason) => log::warn!("閉じる知らせ: {reason:?}"),
        WebSocketEventType::Ping | WebSocketEventType::Pong => {}
        WebSocketEventType::BeforeConnect => log::debug!("接続を試みます"),
    }
}
