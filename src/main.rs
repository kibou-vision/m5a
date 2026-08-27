//! 子供用音声チャットアシスタント m5a。

mod hal;

use anyhow::Result;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sntp::EspSntp;

use m5a_core::config::{self, Config, ConfigError};
use m5a_core::face::{Expression, FaceAnimator};
use m5a_core::guardrail::{Guardrail, Verdict};
use m5a_core::layout;
use m5a_core::logbook::{self, LogEntry, Speaker};
use m5a_core::ports::StorageError;
use m5a_core::realtime::{self, ServerEvent, SessionSetup};
use m5a_core::state::{transition, AppAction, AppEvent, AppState, Failure};

use hal::board::{self, DisplayLock};
use hal::face::FaceView;
use hal::session::Session;
use hal::storage::SdStorage;
use hal::touch::{TouchChange, TouchReader};
use hal::wifi;

/// 顔を描き直す間隔。まばたきが滑らかに見える程度に保つ。
const FRAME_INTERVAL_MS: u32 = 40;
/// ひとつのきっかけから連鎖する処理の上限。取り違えで回り続けるのを防ぐ。
const MAX_FOLLOW_UPS: usize = 8;
/// 失敗してからやり直すまでの待ち。
/// 子どもが操作しなくても自力で戻れるように、放っておいても再試行する。
const RETRY_DELAY_MS: u64 = 10_000;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().expect("周辺機器を取得できません");

    board::init_bus()?;
    board::start_display()?;
    board::set_brightness(100)?;
    board::report_memory("画面の準備後");

    let settings = read_settings();
    report_settings(&settings);

    run(peripherals.modem, settings)
}

/// SD カードから設定を読む。
fn read_settings() -> Result<Config, ConfigError> {
    board::mount_sd_card()
        .map_err(|error| ConfigError::Unreadable(StorageError::Io(error.to_string())))?;

    config::load_config(&mut SdStorage::new())
}

/// 設定の読み取り結果を、親が対処できる形でログに出す。APIキーは出さない。
fn report_settings(settings: &Result<Config, ConfigError>) {
    match settings {
        Ok(config) => log::info!(
            "せっていを よみました: なまえ={} モデル={} こえ={}",
            config.child.name,
            config.openai.model,
            config.openai.voice
        ),
        Err(error) => {
            log::warn!("{}", error.describe());
            log::warn!("→ {}", error.remedy());
        }
    }
}

/// 実機の持ち物。状態遷移が依頼した処理をここで実行する。
struct Runtime {
    config: Option<Config>,
    guardrail: Option<Guardrail>,
    modem: Option<Modem<'static>>,
    wifi: Option<wifi::Connection>,
    clock: Option<EspSntp<'static>>,
    session: Option<Session>,
    /// 接続が確立したら送る設定。
    setup: Option<SessionSetup>,
    storage: SdStorage,
}

impl Runtime {
    fn new(modem: Modem<'static>, config: Option<Config>) -> Self {
        let guardrail = config
            .as_ref()
            .map(|config| Guardrail::new(&config.child.name, config.child.age));

        Self {
            config,
            guardrail,
            modem: Some(modem),
            wifi: None,
            clock: None,
            session: None,
            setup: None,
            storage: SdStorage::new(),
        }
    }

    /// 依頼された処理を行い、その結果として起きたきっかけを返す。
    fn perform(&mut self, action: &AppAction) -> Option<AppEvent> {
        match action {
            AppAction::ConnectNetwork => self.connect_network(),
            AppAction::OpenSession => self.open_session(),
            AppAction::CloseSession => {
                self.session = None;
                None
            }
            AppAction::CancelResponse => {
                self.tell(&realtime::build_response_cancel());
                None
            }
            AppAction::ShowSetupGuide => {
                log::warn!("せっていを かきこんで ください");
                None
            }
            AppAction::ShowFailure(failure) => {
                log::warn!("{}", failure.describe());
                log::warn!("→ {}", failure.remedy());
                self.record(Speaker::System, failure.describe());
                None
            }
            // 音声の取り込みと再生は次の段階で繋ぐ。
            other => {
                log::info!("依頼: {other:?}");
                None
            }
        }
    }

    fn connect_network(&mut self) -> Option<AppEvent> {
        if self.wifi.is_none() {
            let credentials = &self.config.as_ref()?.wifi;
            let modem = self.modem.take()?;

            match wifi::prepare(modem, credentials) {
                Ok(connection) => self.wifi = Some(connection),
                Err(error) => {
                    log::warn!("{error:#}");
                    return Some(AppEvent::Failed(Failure::Network));
                }
            }
        }

        if let Err(error) = wifi::attach(self.wifi.as_mut()?) {
            log::warn!("{error:#}");
            return Some(AppEvent::Failed(Failure::Network));
        }

        log::info!(
            "WiFi に つながりました: {}",
            wifi::describe_address(self.wifi.as_ref()?)
        );
        // 時刻合わせは一度だけでよい。
        if self.clock.is_none() {
            self.clock = wifi::sync_clock();
        }

        Some(AppEvent::NetworkReady)
    }

    fn open_session(&mut self) -> Option<AppEvent> {
        let config = self.config.as_ref()?;
        let guardrail = self.guardrail.as_ref()?;

        let setup = SessionSetup {
            model: config.openai.model.clone(),
            voice: config.openai.voice.clone(),
            audio_format: config.openai.audio_format,
            instructions: guardrail.build_instructions(),
        };

        board::report_memory("接続の直前");

        let api_key = config.openai.api_key.clone();

        match Session::open(&setup, &api_key) {
            Ok(session) => {
                self.session = Some(session);
                self.setup = Some(setup);
                // 設定を送るのは接続が確立してから。合図はサーバから届く。
                None
            }
            Err(error) => {
                log::warn!("{error:#}");
                Some(AppEvent::Failed(Failure::Session))
            }
        }
    }

    /// 接続が確立したので、こちらの設定を送る。
    fn configure_session(&mut self) {
        let Some(setup) = self.setup.take() else {
            return;
        };
        let Some(session) = self.session.as_mut() else {
            return;
        };

        if let Err(error) = session.configure(&setup) {
            log::warn!("{error:#}");
        }
    }

    /// サーバへ電文を送る。切れていれば黙って捨てる。
    fn tell(&mut self, message: &str) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Err(error) = session.send(message) {
            log::warn!("{error:#}");
        }
    }

    /// サーバからの知らせを、状態遷移のきっかけに変える。
    fn receive(&mut self) -> Option<AppEvent> {
        let event = self.session.as_mut()?.poll()?;

        match event {
            ServerEvent::SessionCreated => {
                self.configure_session();
                None
            }
            ServerEvent::SessionConfigured => Some(AppEvent::SessionOpened),
            ServerEvent::AudioDelta(_audio) => {
                // 次の段階で再生に繋ぐ。
                Some(AppEvent::ResponseStarted)
            }
            ServerEvent::AssistantSaid(text) => {
                self.record(Speaker::Assistant, text);
                None
            }
            ServerEvent::ChildSaid(text) => {
                self.watch_over(&text);
                self.record(Speaker::Child, text);
                None
            }
            ServerEvent::ResponseFinished => Some(AppEvent::ResponseFinished),
            // 多くは回復できるので、繋いだまま記録に留める。
            ServerEvent::Reported { code, message } => {
                log::warn!("OpenAI からの知らせ: {message} ({code:?})");
                None
            }
            ServerEvent::Ignored => None,
        }
    }

    /// 子どもの言葉に気づかいが要るか調べ、要るなら親にも残す。
    fn watch_over(&mut self, text: &str) {
        let Some(guardrail) = self.guardrail.as_ref() else {
            return;
        };
        let Verdict::Intervene(concern) = guardrail.inspect(text) else {
            return;
        };

        let reply = guardrail.safe_reply(concern);
        log::warn!("気になる はなし: {concern:?}");

        if concern.should_notify_parent() {
            self.record(Speaker::System, format!("気になる発言がありました: {text}"));
        }
        self.record(Speaker::System, reply);
    }

    /// 会話ログに1行残す。失敗しても対話は止めない。
    fn record(&mut self, speaker: Speaker, text: impl AsRef<str>) {
        let entry = LogEntry::new(wifi::now_unix(), speaker, text.as_ref());

        if let Err(error) = logbook::append_entry(&mut self.storage, &entry) {
            log::debug!("ログを残せません: {error}");
        }
    }
}

fn run(modem: Modem<'static>, settings: Result<Config, ConfigError>) -> Result<()> {
    let mut view = {
        let _lock = DisplayLock::acquire();
        FaceView::create()
    };

    let startup = if settings.is_ok() {
        AppEvent::ConfigLoaded
    } else {
        AppEvent::ConfigRejected
    };
    let mut runtime = Runtime::new(modem, settings.ok());

    let mut state = AppState::Booting;
    let mut animator = FaceAnimator::new();
    let mut touch = TouchReader::new(board::touch_device());
    let mut retry_at: Option<u64> = None;

    advance(&mut state, &mut runtime, startup);
    log::info!("画面の準備ができました");

    loop {
        let now_ms = uptime_ms();

        if let Some(change) = touch.poll() {
            if let Some(event) = to_event(change) {
                advance(&mut state, &mut runtime, event);
            }
        }

        while let Some(event) = runtime.receive() {
            advance(&mut state, &mut runtime, event);
        }

        retry_at = schedule_retry(&state, retry_at, now_ms);
        if retry_at.is_some_and(|due| now_ms >= due) {
            retry_at = None;
            log::info!("やりなおします");
            advance(&mut state, &mut runtime, AppEvent::RetryRequested);
        }

        animator.set_expression(Expression::from_state(&state));
        let frame = animator.frame_at(now_ms);
        let placement = layout::lay_out_face(&frame);

        {
            let _lock = DisplayLock::acquire();
            view.apply(&placement);
        }

        FreeRtos::delay_ms(FRAME_INTERVAL_MS);
    }
}

/// 失敗している間だけ、やり直しの時刻を決める。
///
/// 設定不備は親が直すまで直らないため、ここでは再試行しない。
fn schedule_retry(state: &AppState, scheduled: Option<u64>, now_ms: u64) -> Option<u64> {
    match state {
        AppState::Recovering(_) => scheduled.or(Some(now_ms + RETRY_DELAY_MS)),
        _ => None,
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

/// きっかけを与え、依頼された処理と、そこから生まれたきっかけを処理しきる。
fn advance(state: &mut AppState, runtime: &mut Runtime, event: AppEvent) {
    let mut pending = Some(event);

    for _ in 0..MAX_FOLLOW_UPS {
        let Some(event) = pending.take() else {
            return;
        };

        let step = transition(state, event);
        *state = step.next;

        for action in &step.actions {
            if let Some(follow_up) = runtime.perform(action) {
                pending = Some(follow_up);
            }
        }
    }

    log::warn!("処理が続きすぎたので打ち切りました");
}
