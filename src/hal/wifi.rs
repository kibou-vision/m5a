//! Wi-Fi 接続と時刻合わせ。
//!
//! 会話ログに日付を入れるために時刻を取りに行くが、取れなくても対話は
//! 成り立つので、時刻合わせの失敗では止めない。

use anyhow::{anyhow, Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use m5a_core::config::WifiConfig;

/// 時刻合わせを待つ上限。これを過ぎたら日付なしでログを書く。
const TIME_SYNC_ATTEMPTS: u32 = 20;
const TIME_SYNC_INTERVAL_MS: u32 = 500;

/// 接続済みの Wi-Fi。落とすと切れる。
pub type Connection = BlockingWifi<EspWifi<'static>>;

/// Wi-Fi を使える状態にする。接続はまだしない。
///
/// modem は一度しか取り出せないため、繋ぎ直すときに作り直さずに済むよう
/// 準備と接続を分けている。
pub fn prepare(modem: Modem<'static>, credentials: &WifiConfig) -> Result<Connection> {
    let event_loop = EspSystemEventLoop::take().context("イベントループを取得できません")?;
    let storage = EspDefaultNvsPartition::take().context("NVS を取得できません")?;

    let driver = EspWifi::new(modem, event_loop.clone(), Some(storage))
        .context("Wi-Fi を初期化できません")?;
    let mut wifi = BlockingWifi::wrap(driver, event_loop).context("Wi-Fi を準備できません")?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: credentials
            .ssid
            .as_str()
            .try_into()
            .map_err(|_| anyhow!("WiFiの名前が長すぎます: {}", credentials.ssid))?,
        password: credentials
            .password
            .as_str()
            .try_into()
            .map_err(|_| anyhow!("WiFiのパスワードが長すぎます"))?,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))
    .context("Wi-Fi の設定を反映できません")?;

    wifi.start().context("Wi-Fi を開始できません")?;

    Ok(wifi)
}

/// 接続する。切れているときは繋ぎ直しにも使える。
pub fn attach(wifi: &mut Connection) -> Result<()> {
    // 前の接続が残っていると繋ぎ直せないことがある。
    let _ = wifi.disconnect();

    wifi.connect().context(
        "WiFi に接続できません。名前とパスワードを確かめてください。2.4GHz のみ使えます",
    )?;
    wifi.wait_netif_up()
        .context("WiFi のアドレスを取得できません")?;

    Ok(())
}

/// 割り当てられたアドレス。ルータ側で見分けるときに使う。
pub fn describe_address(wifi: &Connection) -> String {
    match wifi.wifi().sta_netif().get_ip_info() {
        Ok(info) => format!("IP {} / MAC {}", info.ip, describe_mac(wifi)),
        Err(_) => format!("MAC {}", describe_mac(wifi)),
    }
}

fn describe_mac(wifi: &Connection) -> String {
    match wifi.wifi().sta_netif().get_mac() {
        Ok(mac) => mac
            .iter()
            .map(|part| format!("{part:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
        Err(_) => "不明".to_string(),
    }
}

/// 時刻を取りに行く。取れなければ `None` を返す。
///
/// 返り値を持ち続けている間だけ時刻合わせが続く。
pub fn sync_clock() -> Option<EspSntp<'static>> {
    let sntp = EspSntp::new_default().ok()?;

    for _ in 0..TIME_SYNC_ATTEMPTS {
        if sntp.get_sync_status() == SyncStatus::Completed {
            return Some(sntp);
        }
        FreeRtos::delay_ms(TIME_SYNC_INTERVAL_MS);
    }

    log::warn!("時刻を取得できませんでした。ログに日付が入りません");
    None
}

/// 日本時間との時差（秒）。設定は SD カードに置かない。
/// この端末はいまのところ日本国内でしか使わないため。
const JST_OFFSET_SECONDS: i64 = 9 * 3_600;

/// いまの日本時間を UNIX 時刻として返す。取得できていなければ 0 以下になる。
///
/// SNTP は UTC を返すため、時差を足してから使う。あいさつの朝昼晩の判定や
/// 会話ログの時刻はすべてこれを基準にする。
pub fn now_unix() -> i64 {
    let mut now: esp_idf_svc::sys::time_t = 0;
    unsafe { esp_idf_svc::sys::time(&mut now) };

    let utc = now as i64;
    if utc <= 0 {
        return utc;
    }
    utc + JST_OFFSET_SECONDS
}
