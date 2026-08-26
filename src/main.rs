//! 子供用音声チャットアシスタント m5a。

mod hal;

use anyhow::Result;

use m5a_core::config::{self, Config, ConfigError};
use m5a_core::face::{Expression, FaceAnimator};
use m5a_core::layout::{self, FaceLayout};
use m5a_core::ports::StorageError;
use m5a_core::state::{transition, AppAction, AppEvent, AppState};

use hal::board::{self, DisplayLock};
use hal::face::FaceView;
use hal::storage::SdStorage;
use hal::touch::{TouchChange, TouchReader};

/// 顔を描き直す間隔。まばたきが滑らかに見える程度に保つ。
const FRAME_INTERVAL_MS: u32 = 40;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    board::init_bus()?;
    board::start_display()?;
    board::set_brightness(100)?;

    let settings = read_settings();
    report_settings(&settings);

    run(settings)
}

/// SD カードから設定を読む。
fn read_settings() -> Result<Config, ConfigError> {
    board::mount_sd_card()
        .map_err(|error| ConfigError::Unreadable(StorageError::Io(error.to_string())))?;

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

fn run(settings: Result<Config, ConfigError>) -> Result<()> {
    let mut view = {
        let _lock = DisplayLock::acquire();
        FaceView::create()
    };

    let mut state = AppState::Booting;
    let mut animator = FaceAnimator::new();
    let mut touch = TouchReader::new(board::touch_device());

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
        if let Some(change) = touch.poll() {
            if let Some(event) = to_event(change) {
                advance(&mut state, event);
            }
        }

        animator.set_expression(Expression::from_state(&state));
        let frame = animator.frame_at(uptime_ms());
        let placement: FaceLayout = layout::lay_out_face(&frame);

        {
            let _lock = DisplayLock::acquire();
            view.apply(&placement);
        }

        esp_idf_svc::hal::delay::FreeRtos::delay_ms(FRAME_INTERVAL_MS);
    }
}

/// 起動からの経過時間。表情の位相を決めるのに使う。
fn uptime_ms() -> u64 {
    (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000) as u64
}

/// 指の動きを、おはなしボタンの上でだけ状態遷移のきっかけに変える。
fn to_event(change: TouchChange) -> Option<AppEvent> {
    match change {
        TouchChange::Pressed(at) => {
            // ボタンを狙えているかは実機でしか分からないため、座標を残す。
            log::info!("さわった: ({}, {})", at.x, at.y);
            layout::contains_talk_button(at).then_some(AppEvent::TalkPressed)
        }
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
