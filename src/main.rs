//! 子供用音声チャットアシスタント m5a。
//!
//! 起動の順序には理由がある。LCD の DC と microSD の MISO が同じ端子に
//! 繋がっているため、先に設定を読み終えてカードを外してから画面を初期化する。

mod hal;

use anyhow::{Context, Result};
use esp_idf_svc::hal::delay::{Delay, FreeRtos};
use esp_idf_svc::hal::gpio::{AnyIOPin, Gpio3, Gpio35, Gpio36, Gpio37, Gpio4, PinDriver, Pins};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::sd::spi::SdSpiHostDriver;
use esp_idf_svc::hal::spi::config::Config as SpiConfig;
use esp_idf_svc::hal::spi::{Dma, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI2};
use esp_idf_svc::hal::units::{FromValueType as _, Hertz};

use m5a_core::config::{self, Config, ConfigError};
use m5a_core::face::{Expression, FaceAnimator, FaceFrame};
use m5a_core::ports::StorageError;
use m5a_core::render;
use m5a_core::state::{transition, AppAction, AppEvent, AppState};

use hal::display::{self, TRANSFER_BUFFER_LEN};
use hal::pins;
use hal::power;
use hal::storage::{self, SdStorage};
use hal::touch::{TouchChange, TouchReader};

/// 画面を描き直す間隔。まばたきが滑らかに見える程度に保つ。
const FRAME_INTERVAL_MS: u32 = 40;
/// 電源を入れてから周辺デバイスが応答するまでの待ち。
const POWER_SETTLE_MS: u32 = 50;
/// SPI の速度。CoreS3 の配線で安定して出せる範囲に留める。
const SPI_MHZ: u32 = 40;
/// DMA で一度に運べる大きさ。画面の1行分に余裕を持たせている。
const SPI_DMA_BYTES: usize = 4_096;

/// LCD と microSD が分け合う SPI の端子。
///
/// GPIO35 は LCD では DC、microSD では MISO として働く。ひとつの型に
/// まとめておくことで、両者を同時に使えないことを取り違えにくくする。
struct SharedSpi<'d> {
    bus: SPI2<'d>,
    sclk: Gpio36<'d>,
    mosi: Gpio37<'d>,
    shared: Gpio35<'d>,
    sd_cs: Gpio4<'d>,
    lcd_cs: Gpio3<'d>,
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let Peripherals { pins, i2c0, spi2, .. } =
        Peripherals::take().context("周辺機器を取得できません")?;
    let Pins { gpio11, gpio12, gpio3, gpio4, gpio35, gpio36, gpio37, .. } = pins;

    let mut i2c = I2cDriver::new(
        i2c0,
        gpio12,
        gpio11,
        &I2cConfig::new().baudrate(Hertz(pins::I2C_HZ)),
    )
    .context("内部 I2C を初期化できません")?;

    let mut spi = SharedSpi {
        bus: spi2,
        sclk: gpio36,
        mosi: gpio37,
        shared: gpio35,
        sd_cs: gpio4,
        lcd_cs: gpio3,
    };

    power::enable_peripherals(&mut i2c)?;
    Delay::new_default().delay_ms(POWER_SETTLE_MS);

    let settings = read_settings(&mut spi, &mut i2c);
    report_settings(&settings);

    power::set_backlight(&mut i2c, 100)?;

    run(spi, i2c, settings)
}

/// SD カードから設定を読む。読み終えたらカードを外して端子を LCD に譲る。
fn read_settings(spi: &mut SharedSpi, i2c: &mut I2cDriver) -> Result<Config, ConfigError> {
    let unreadable =
        |error: anyhow::Error| ConfigError::Unreadable(StorageError::Io(error.to_string()));

    power::set_sd_power(i2c, true).map_err(unreadable)?;
    Delay::new_default().delay_ms(POWER_SETTLE_MS);

    let outcome = load_from_card(spi);

    // マウントを解いたうえで給電も止め、GPIO35 を LCD の DC として使えるようにする。
    let _ = power::set_sd_power(i2c, false);

    outcome
}

fn load_from_card(spi: &mut SharedSpi) -> Result<Config, ConfigError> {
    let unreadable =
        |error: anyhow::Error| ConfigError::Unreadable(StorageError::Io(error.to_string()));

    let bus = SpiDriver::new(
        unsafe { spi.bus.reborrow() },
        unsafe { spi.sclk.reborrow() },
        unsafe { spi.mosi.reborrow() },
        Some(unsafe { spi.shared.reborrow() }),
        &SpiDriverConfig::new().dma(Dma::Auto(SPI_DMA_BYTES)),
    )
    .map_err(|error| unreadable(error.into()))?;

    let host = SdSpiHostDriver::new(
        bus,
        Some(unsafe { spi.sd_cs.reborrow() }),
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        None,
    )
    .map_err(|error| unreadable(error.into()))?;

    let _mounted = storage::mount_sd_card(host).map_err(unreadable)?;

    config::load_config(&mut SdStorage::new())
}

/// 設定の読み取り結果を、親が対処できる形でログに出す。
fn report_settings(settings: &Result<Config, ConfigError>) {
    match settings {
        Ok(config) => log::info!(
            "せっていを よみました: なまえ={} モデル={}",
            config.child.name,
            config.openai.model
        ),
        Err(error) => {
            log::warn!("{}", error.describe());
            log::warn!("→ {}", error.remedy());
        }
    }
}

fn run(
    spi: SharedSpi<'static>,
    mut i2c: I2cDriver,
    settings: Result<Config, ConfigError>,
) -> Result<()> {
    let bus = SpiDriver::new(
        spi.bus,
        spi.sclk,
        spi.mosi,
        // SD の MISO と同じ端子を DC に使うため、ここでは受信線を持たせない。
        Option::<AnyIOPin>::None,
        &SpiDriverConfig::new().dma(Dma::Auto(SPI_DMA_BYTES)),
    )
    .context("LCD 用の SPI を初期化できません")?;

    let device = SpiDeviceDriver::new(
        bus,
        Some(spi.lcd_cs),
        &SpiConfig::new().baudrate(SPI_MHZ.MHz().into()),
    )
    .context("LCD を SPI に接続できません")?;

    let data_command =
        PinDriver::output(spi.shared).context("LCD の DC 端子を設定できません")?;

    let buffer = vec![0_u8; TRANSFER_BUFFER_LEN].leak();
    let mut lcd = display::build_display(device, data_command, buffer)?;

    let mut state = AppState::Booting;
    let mut animator = FaceAnimator::new();
    let mut touch = TouchReader::new();
    let mut last_drawn: Option<FaceFrame> = None;

    let startup = if settings.is_ok() {
        AppEvent::ConfigLoaded
    } else {
        AppEvent::ConfigRejected
    };
    advance(&mut state, startup);

    // 第2段階で WiFi 接続と対話セッションに置き換える。
    // それまでは画面とタッチを確かめるために待機状態まで進めておく。
    if settings.is_ok() {
        advance(&mut state, AppEvent::NetworkReady);
        advance(&mut state, AppEvent::SessionOpened);
    }

    log::info!("画面の準備ができました");

    loop {
        match touch.poll(&mut i2c) {
            Ok(Some(change)) => {
                if let Some(event) = to_event(change) {
                    advance(&mut state, event);
                }
            }
            Ok(None) => {}
            // 一度読めなくても次の周回で拾える。会話を止めるほどではない。
            Err(error) => log::debug!("タッチを読めません: {error}"),
        }

        animator.set_expression(Expression::from_state(&state));
        let frame = animator.frame_at(uptime_ms());

        if last_drawn != Some(frame) {
            render::draw_face(&mut lcd, &frame)
                .map_err(|error| anyhow::anyhow!("顔を描けません: {error:?}"))?;
            last_drawn = Some(frame);
        }

        FreeRtos::delay_ms(FRAME_INTERVAL_MS);
    }
}

/// 起動からの経過時間。表情の位相を決めるのに使う。
fn uptime_ms() -> u64 {
    (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000) as u64
}

/// 指の動きを、おはなしボタンの上でだけ状態遷移のきっかけに変える。
fn to_event(change: TouchChange) -> Option<AppEvent> {
    match change {
        TouchChange::Pressed(at) if render::contains_talk_button(at) => Some(AppEvent::TalkPressed),
        TouchChange::Pressed(_) => None,
        TouchChange::Released => Some(AppEvent::TalkReleased),
    }
}

fn advance(state: &mut AppState, event: AppEvent) {
    let step = transition(state, event);

    for action in &step.actions {
        perform(action);
    }
    *state = step.next;
}

/// 第1段階では音声とネットワークが未接続のため、依頼された処理を記録に留める。
fn perform(action: &AppAction) {
    match action {
        AppAction::ShowSetupGuide => log::warn!("せっていを かきこんで ください"),
        AppAction::ShowFailure(failure) => {
            log::warn!("{}", failure.describe());
            log::warn!("→ {}", failure.remedy());
        }
        other => log::info!("依頼: {other:?}"),
    }
}
