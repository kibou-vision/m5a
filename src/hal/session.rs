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

/// 受信バッファ。ここを超える電文は組み立て直せないため、
/// 音声の断片が余裕をもって収まる大きさにする。
const RECEIVE_BUFFER: usize = 8 * 1024;
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

        let client = EspWebSocketClient::new(&url, &config, CONNECT_TIMEOUT, move |event| {
            forward(event, &sender);
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

/// 電文の頭だけを取り出す。全文を出すと画面外に流れて読めなくなる。
fn head_of(payload: &str) -> String {
    payload.chars().take(120).collect()
}

/// 受信した電文を解釈して待ち行列へ流す。
///
/// この関数は受信用の仕事から呼ばれる。重い処理はせず、渡すだけにする。
fn forward(event: &Result<WebSocketEvent<'_>, esp_idf_svc::io::EspIOError>, sender: &Sender<ServerEvent>) {
    let Ok(event) = event else {
        // 接続の途中経過でも失敗として届くため、記録に留める。
        log::debug!("受信に失敗しました");
        return;
    };

    match event.event_type {
        WebSocketEventType::Text(payload) => {
            // 音声の断片が毎秒いくつも届くため、中身は必要なときだけ見る。
            log::debug!("受信 {} バイト: {}", payload.len(), head_of(payload));

            match realtime::parse_server_event(payload) {
                Ok(ServerEvent::Ignored) => {}
                Ok(parsed) => {
                    let _ = sender.send(parsed);
                }
                // 電文を1つ読み違えても対話は続けられる。
                Err(error) => log::warn!("電文を解釈できません: {}", error.detail),
            }
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
