//! 子供用音声チャットアシスタント m5a。

mod hal;

use anyhow::Result;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sntp::EspSntp;

use m5a_core::config::{self, Config, ConfigError};
use m5a_core::greeting::TimeOfDay;
use m5a_core::face::{Expression, FaceAnimator};
use m5a_core::gesture::{self, SwipeDirection};
use m5a_core::guardrail::{Guardrail, Verdict};
use m5a_core::layout::{self, Point};
use m5a_core::logbook::{self, LogEntry, Speaker};
use m5a_core::module_status::ModuleStatus;
use m5a_core::ports::StorageError;
use m5a_core::realtime::{self, ServerEvent, SessionSetup};
use m5a_core::screen::{self, Screen, ScreenEvent};
use m5a_core::search;
use m5a_core::settings_layout;
use m5a_core::state::{transition, AppAction, AppEvent, AppState, Failure};
use m5a_core::turn_detector::{TurnDetector, TurnOutcome};

use hal::audio::Audio;
use hal::board::{self, DisplayLock};
use hal::face::FaceView;
use hal::session::Session;
use hal::settings_view::SettingsView;
use hal::storage::SdStorage;
use hal::touch::{TouchChange, TouchReader};
use hal::wifi;

use std::sync::mpsc::Receiver;

/// 画面の明るさ。暗い部屋でもまぶしくない程度に抑える。
const SCREEN_BRIGHTNESS: u8 = 50;

/// 顔を描き直す間隔。まばたきが滑らかに見える程度に保つ。
const FRAME_INTERVAL_MS: u32 = 40;
/// ひとつのきっかけから連鎖する処理の上限。取り違えで回り続けるのを防ぐ。
const MAX_FOLLOW_UPS: usize = 8;
/// ためておける会話ログの数。これを超えたら古いものから捨てる。
/// 対話を続けることを優先し、記録のために memory を食いつぶさない。
const MAX_PENDING_LOGS: usize = 64;

/// SD カードへ書く前に必要な DMA の空き。
///
/// SPI は書き込みのたびに DMA バッファを確保する。足りないまま呼ぶと
/// ドライバが NULL を返したまま進み、内部で落ちてしまう。
const SD_WRITE_HEADROOM: usize = 12 * 1_024;

/// 失敗してからやり直すまでの待ち。
/// 子どもが操作しなくても自力で戻れるように、放っておいても再試行する。
const RETRY_DELAY_MS: u64 = 3_000;

/// この時間タッチ操作が無ければ電源を落とす。
/// 置き忘れて電池を消耗させないための既定値。
const IDLE_SHUTDOWN_MS: u64 = 3 * 60 * 1_000;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().expect("周辺機器を取得できません");

    board::init_bus()?;
    board::start_display()?;
    board::set_brightness(SCREEN_BRIGHTNESS)?;
    board::report_memory("画面の準備後");

    // 顔・設定画面の部品は表示できるようになった直後に作り、SDカードの
    // マウントなど時間のかかる処理より前に設定画面を一度出す。
    // 起動直後、何も映らない時間をできるだけ短くするため。
    let mut view = {
        let _lock = DisplayLock::acquire();
        FaceView::create()
    };
    let mut settings_view = {
        let _lock = DisplayLock::acquire();
        SettingsView::create()
    };
    show_booting_screen(&mut view, &mut settings_view);

    let settings = read_settings();
    report_settings(&settings);

    run(peripherals.modem, settings, view, settings_view)
}

/// SDカードの読み込みなどが終わる前に、まず設定画面を一度描いておく。
fn show_booting_screen(view: &mut FaceView, settings_view: &mut SettingsView) {
    let statuses = m5a_core::module_status::ModuleStatuses::booting();
    let placement = settings_layout::lay_out_settings(&statuses, "");

    let _lock = DisplayLock::acquire();
    view.hide();
    settings_view.show();
    settings_view.apply(&placement);
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
            "設定を読み込みました: 名前={} アシスタント名={} モデル={} 声={}",
            config.child.name,
            config.assistant.name,
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
    audio: Option<Audio>,
    /// 組み立て途中のアシスタントの発話。
    /// 文字起こしは細切れに届くため、言い終えてからまとめて記録する。
    spoken: String,
    /// 書き出しを待っている会話ログ。
    /// 会話中は SD の DMA バッファを確保できないため、待機に戻ってから書く。
    pending_logs: Vec<LogEntry>,
    storage: SdStorage,
    /// 進行中のweb検索。呼び出しのcall_idと、結果を待つ受け口。
    pending_search: Option<(String, Receiver<Option<String>>)>,
    /// サーバーからは応答終了の知らせが届いたが、まだ再生しきっていない分がある。
    /// 実際に鳴らし終わってから状態遷移のきっかけを起こす。
    finishing_response: bool,
    /// 録音中だけ持つ、声と沈黙の追跡。`Listening` の間だけ `Some`。
    turn: Option<TurnDetector>,
    /// 声を検出した直後の一コマだけ立つ、うなずきの合図。
    nod_pending: bool,
    /// 各モジュール（画面・SD・マイク・WiFi・話す相手・検索）の準備状況。
    /// 設定画面に一覧で出す。
    module_statuses: m5a_core::module_status::ModuleStatuses,
}

impl Runtime {
    fn new(modem: Modem<'static>, config: Option<Config>, sd_card: ModuleStatus) -> Self {
        let guardrail = config
            .as_ref()
            .map(|config| Guardrail::new(&config.child.name, config.child.age, &config.assistant.name));

        let mut module_statuses = m5a_core::module_status::ModuleStatuses::booting();
        module_statuses.sd_card = sd_card;
        if let Some(config) = config.as_ref() {
            if config.search.api_key().is_some() {
                module_statuses.web_search = Some(ModuleStatus::Ready);
            }
        }

        Self {
            config,
            guardrail,
            modem: Some(modem),
            wifi: None,
            clock: None,
            session: None,
            setup: None,
            audio: None,
            spoken: String::new(),
            pending_logs: Vec::new(),
            storage: SdStorage::new(),
            pending_search: None,
            finishing_response: false,
            turn: None,
            nod_pending: false,
            module_statuses,
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
                self.interrupt_audio();
                // 途中で遮っても、そこまで言った分は残す。
                self.flush_spoken();
                // 検索中に遮られたら、遅れて届く結果は捨てる。
                self.pending_search = None;
                // 鳴らしきるのを待つ必要はなくなった。
                self.finishing_response = false;
                None
            }
            AppAction::StartCapture => {
                if let Some(audio) = self.audio.as_ref() {
                    // 前回の録音の残りが新しい発話の頭に混ざらないようにする。
                    audio.discard_captured();
                    audio.begin_capture();
                }
                self.turn = Some(TurnDetector::new());
                None
            }
            AppAction::StopCapture => {
                if let Some(audio) = self.audio.as_ref() {
                    audio.end_capture();
                }
                self.turn = None;
                None
            }
            AppAction::RequestResponse => {
                // 録音を確定してから応答を求める。順序を違えると空のまま返る。
                self.tell(&realtime::build_audio_commit());
                self.tell(&realtime::build_response_create());
                None
            }
            AppAction::StartPlayback => None,
            // 口を閉じるのは再生の仕事自身に任せる。ここで閉じると、
            // まだキューに残っている分の音より先に口が閉じてしまう。
            AppAction::StopPlayback => None,
            AppAction::ShowSetupGuide => {
                log::warn!("設定を書き込んでください");
                None
            }
            AppAction::ShowFailure(failure) => {
                log::warn!("{}", failure.describe());
                log::warn!("→ {}", failure.remedy());
                self.record(Speaker::System, failure.describe());
                None
            }
            AppAction::PowerOff => {
                log::info!("しばらく操作が無かったため電源を落とします");
                // 電源が切れる前に、たまっているログを失わないようにする。
                self.flush_logs();
                board::power_off();
            }
        }
    }

    /// マイクとスピーカーを用意する。失敗しても画面と設定は使えるようにする。
    fn open_audio(&mut self) {
        if self.audio.is_some() {
            return;
        }
        let Some(config) = self.config.as_ref() else {
            return;
        };

        match Audio::start(config.openai.audio_format) {
            Ok(audio) => {
                self.audio = Some(audio);
                self.module_statuses.microphone = ModuleStatus::Ready;
            }
            Err(error) => {
                log::warn!("音を使えません: {error:#}");
                self.module_statuses.microphone = ModuleStatus::Error;
            }
        }
    }

    fn connect_network(&mut self) -> Option<AppEvent> {
        self.module_statuses.wifi = ModuleStatus::Checking;

        if self.wifi.is_none() {
            let credentials = &self.config.as_ref()?.wifi;
            let modem = self.modem.take()?;

            match wifi::prepare(modem, credentials) {
                Ok(connection) => self.wifi = Some(connection),
                Err(error) => {
                    log::warn!("{error:#}");
                    // 失敗しても schedule_retry() が数秒後に自動でやり直し
                    // 続けるため、Failedにはせず Checking のままにする。
                    return Some(AppEvent::Failed(Failure::Network));
                }
            }
        }

        if let Err(error) = wifi::attach(self.wifi.as_mut()?) {
            log::warn!("{error:#}");
            return Some(AppEvent::Failed(Failure::Network));
        }

        log::info!(
            "WiFi に繋がりました: {}",
            wifi::describe_address(self.wifi.as_ref()?)
        );
        self.module_statuses.wifi = ModuleStatus::Ready;
        // 時刻合わせは一度だけでよい。
        if self.clock.is_none() {
            self.clock = wifi::sync_clock();
        }

        Some(AppEvent::NetworkReady)
    }

    fn open_session(&mut self) -> Option<AppEvent> {
        self.module_statuses.realtime_session = ModuleStatus::Checking;

        let config = self.config.as_ref()?;
        let guardrail = self.guardrail.as_ref()?;

        let tools = if config.search.api_key().is_some() {
            vec![search::tool_definition()]
        } else {
            Vec::new()
        };

        let setup = SessionSetup {
            model: config.openai.model.clone(),
            voice: config.openai.voice.clone(),
            audio_format: config.openai.audio_format,
            instructions: guardrail.build_instructions(),
            tools,
        };

        board::report_memory("接続の直前");

        let api_key = config.openai.api_key.clone();

        match Session::open(&setup, &api_key) {
            Ok(session) => {
                self.session = Some(session);
                self.setup = Some(setup);
                // 設定を送るのは接続が確立してから。合図はサーバから届く。
                // 「使える」への切り替えは SessionConfigured の受信時。
                None
            }
            Err(error) => {
                log::warn!("{error:#}");
                // WiFiと同様、schedule_retry()が自動でやり直すため
                // Checkingのままにする。
                Some(AppEvent::Failed(Failure::Session))
            }
        }
    }

    /// 話せるようになった最初に、こちらからあいさつする。
    ///
    /// 子どもが最初の一言を考えなくてよいようにする。会話ログの記録は
    /// 通常のやり取りと同じ経路（AssistantSaid の集約）で行われる。
    fn greet(&mut self) {
        let Some(guardrail) = self.guardrail.as_ref() else {
            return;
        };
        let time_of_day = TimeOfDay::at(wifi::now_unix());
        let prompt = guardrail.build_greeting_prompt(time_of_day);

        self.tell(&realtime::build_response_create_with_instructions(&prompt));
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

    /// 言い終えたアシスタントの発話を記録する。
    fn flush_spoken(&mut self) {
        if self.spoken.is_empty() {
            return;
        }

        let spoken = std::mem::take(&mut self.spoken);
        log::info!("アシスタント: {spoken}");
        self.record(Speaker::Assistant, spoken);
    }

    /// 録音を送り出す。溜まっている分をまとめて流す。
    ///
    /// 声を検出したかにかかわらず、待ち行列は毎回空にする。空けずに残すと
    /// 録音の仕事側の待ち行列が満杯のまま数秒続き、そちら側の送信待ちが
    /// 詰まってタスクウォッチドッグに落ちたことがある。声を一度も
    /// 検出していない間は、取り出した分をサーバーへは送らず捨てるだけにする。
    fn push_captured(&mut self) {
        let Some(audio) = self.audio.as_ref() else {
            return;
        };

        let mut pending = Vec::new();
        while let Some(chunk) = audio.take_captured() {
            pending.push(chunk);
        }

        if !self.turn.as_ref().is_some_and(TurnDetector::has_spoken) {
            return;
        }

        for chunk in pending {
            self.tell(&realtime::build_audio_append(&chunk));
        }
    }

    /// いま鳴っている音の大きさ。口の開きに使う。
    fn voice_level(&self) -> u8 {
        self.audio.as_ref().map_or(0, Audio::level)
    }

    /// 録音中の沈黙を追跡し、話し終わり（または声が無いまま）を判定する。
    fn poll_turn(&mut self, elapsed_ms: u32) -> Option<AppEvent> {
        let detector = self.turn.as_mut()?;
        let had_spoken = detector.has_spoken();
        let level = self.audio.as_ref().map_or(0, Audio::input_level);
        // しきい値が実機に合っているか、後で確かめられるように残す。
        log::debug!("マイクの音量: {level}");
        let outcome = detector.observe(level, elapsed_ms);

        // 声を検出した瞬間だけ、うなずきの合図を立てる。
        if !had_spoken && self.turn.as_ref().is_some_and(TurnDetector::has_spoken) {
            self.nod_pending = true;
        }

        outcome.map(|outcome| match outcome {
            TurnOutcome::SpeechEnded => AppEvent::SpeechEnded,
            TurnOutcome::NothingSaid => AppEvent::SpeechNotDetected,
        })
    }

    /// うなずきの合図が立っていれば、それを消費して伝える。
    fn take_nod_pending(&mut self) -> bool {
        std::mem::take(&mut self.nod_pending)
    }

    /// 応答終了の知らせを待たせていたら、実際に鳴らし終わったか確かめる。
    fn poll_playback(&mut self) -> Option<AppEvent> {
        if !self.finishing_response {
            return None;
        }
        if self.audio.as_ref().is_some_and(Audio::is_speaking) {
            return None;
        }

        self.finishing_response = false;
        Some(AppEvent::ResponseFinished)
    }

    /// 割り込みで応答を打ち切る。溜めていた音声も鳴らさず捨てる。
    fn interrupt_audio(&mut self) {
        if let Some(audio) = self.audio.as_ref() {
            audio.interrupt();
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
            ServerEvent::SessionConfigured => {
                self.module_statuses.realtime_session = ModuleStatus::Ready;
                self.greet();
                Some(AppEvent::SessionOpened)
            }
            ServerEvent::AudioDelta(audio) => {
                if let Some(player) = self.audio.as_ref() {
                    player.play(audio);
                }
                Some(AppEvent::ResponseStarted)
            }
            // 断片ごとに書くと、ひとつの発話で何十回も SD に書きに行くことになる。
            ServerEvent::AssistantSaid(text) => {
                self.spoken.push_str(&text);
                None
            }
            ServerEvent::ChildSaid(text) => {
                log::info!("こども: {text}");
                self.watch_over(&text);
                self.record(Speaker::Child, text);
                None
            }
            // サーバーは音声をリアルタイムより速く送ってくるため、この知らせが
            // 届いた時点でもまだ再生しきっていない分が残っていることがある。
            // 状態遷移は鳴らし終わってから起こす（poll_playback を参照）。
            ServerEvent::ResponseFinished => {
                self.flush_spoken();
                if self.audio.as_ref().is_some_and(Audio::is_speaking) {
                    self.finishing_response = true;
                    None
                } else {
                    Some(AppEvent::ResponseFinished)
                }
            }
            ServerEvent::ToolCallRequested { call_id, name, arguments } => {
                self.handle_tool_call(&call_id, &name, &arguments);
                None
            }
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
        log::warn!("気になる話: {concern:?}");

        if concern.should_notify_parent() {
            self.record(Speaker::System, format!("気になる発言がありました: {text}"));
        }
        self.record(Speaker::System, reply);
    }

    /// モデルからの function tool 呼び出しを受け、検索を始めるか、
    /// その場で「わからない」を返す。
    fn handle_tool_call(&mut self, call_id: &str, name: &str, arguments: &str) {
        if name != search::TOOL_NAME {
            self.finish_tool_call(call_id, "それはできません");
            return;
        }

        let Some(api_key) = self.config.as_ref().and_then(|config| config.search.api_key())
        else {
            self.finish_tool_call(call_id, "今は調べられません");
            return;
        };

        let Some(query) = search::extract_query(arguments) else {
            self.finish_tool_call(call_id, "何を調べるか分かりませんでした");
            return;
        };

        log::info!("web検索: {query}");
        // 検索が動かない不具合の調査用。TLS の仕事を新たに始められる
        // 内部メモリが残っているか、その場で確かめる。
        board::report_memory("web検索の直前");
        let request = search::build_request(&query, api_key);
        self.pending_search = Some((call_id.to_string(), hal::search::spawn(request)));
    }

    /// 検索スレッドの結果が届いていれば、会話へ差し戻す。
    ///
    /// スレッドが答えを送らずに終わった（例えば途中で落ちた）場合も
    /// 「わからなかった」で会話を進め、Thinking のまま止まらないようにする。
    fn poll_search(&mut self) {
        use std::sync::mpsc::TryRecvError;

        let Some((_, receiver)) = self.pending_search.as_ref() else {
            return;
        };

        let output = match receiver.try_recv() {
            Ok(result) => result.unwrap_or_else(|| "わかりませんでした".to_string()),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => "わかりませんでした".to_string(),
        };

        let (call_id, _) = self.pending_search.take().expect("直前に確かめたはず");
        self.finish_tool_call(&call_id, &output);
    }

    /// function tool の実行結果を送り、応答の続きを求める。
    fn finish_tool_call(&mut self, call_id: &str, output: &str) {
        self.tell(&realtime::build_function_call_output(call_id, output));
        self.tell(&realtime::build_response_create());
    }

    /// 会話ログを1行ためる。実際に書くのは待機に戻ってから。
    fn record(&mut self, speaker: Speaker, text: impl AsRef<str>) {
        if self.pending_logs.len() >= MAX_PENDING_LOGS {
            self.pending_logs.remove(0);
        }

        self.pending_logs
            .push(LogEntry::new(wifi::now_unix(), speaker, text.as_ref()));
    }

    /// たまった会話ログを SD カードへ書き出す。
    ///
    /// 会話中は音声と通信が内部メモリを使い切っており、SPI が書き込み用の
    /// DMA バッファを確保できない。待機に戻って空きが戻ってから書く。
    fn flush_logs(&mut self) {
        if self.pending_logs.is_empty() {
            return;
        }

        let headroom = board::dma_headroom();
        if headroom < SD_WRITE_HEADROOM {
            return;
        }

        for entry in std::mem::take(&mut self.pending_logs) {
            if let Err(error) = logbook::append_entry(&mut self.storage, &entry) {
                log::warn!("ログを残せません: {error}");
                self.module_statuses.sd_card = ModuleStatus::Error;
                return;
            }
        }
    }

    /// 設定画面で選ばれた声を反映し、SDカードへ書き戻す。
    ///
    /// 確立済みの対話セッションへは即座に反映されない
    /// （Realtime APIはセッション確立後に声を変えられない）。
    /// 次にセッションを開くとき（次回起動、または再接続時）から使われる。
    fn select_voice(&mut self, voice: &str) {
        let Some(config) = self.config.as_mut() else {
            return;
        };
        if config.openai.voice == voice {
            return;
        }
        config.openai.voice = voice.to_string();

        if let Err(error) = config::save_voice(&mut self.storage, voice) {
            log::warn!("声を保存できません: {}", error.describe());
        } else {
            log::info!("声を保存しました: {voice}");
        }
    }
}

/// SDカードの読み書きに関わる設定エラーだけを、SDカードの不調として扱う。
/// それ以外（記入漏れ・書式誤り）はカード自体は読めているので `Ready`。
/// 何が起きたかは `report_settings()` がすでにシリアルログへ出している。
fn sd_card_status(settings: &Result<Config, ConfigError>) -> ModuleStatus {
    match settings {
        Err(ConfigError::Unreadable(_) | ConfigError::Unwritable) => ModuleStatus::Error,
        _ => ModuleStatus::Ready,
    }
}

fn run(
    modem: Modem<'static>,
    settings: Result<Config, ConfigError>,
    mut view: FaceView,
    mut settings_view: SettingsView,
) -> Result<()> {
    let sd_card = sd_card_status(&settings);

    let startup = if settings.is_ok() {
        AppEvent::ConfigLoaded
    } else {
        AppEvent::ConfigRejected
    };
    let mut runtime = Runtime::new(modem, settings.ok(), sd_card);

    let mut state = AppState::Booting;
    // 起動直後は必ず設定画面から見せ、モジュールの準備状況を親が確かめられるようにする。
    let mut screen = Screen::Settings;
    let mut animator = FaceAnimator::new();
    let mut touch = TouchReader::new(board::touch_device());
    let mut retry_at: Option<u64> = None;
    // 立ち上げ中の失敗を数える。数回は読み込み中として見せ、
    // それでも駄目なときだけ困り顔で親に伝える。
    let mut failed_attempts = 0_u32;
    let mut last_now_ms = uptime_ms();
    // スワイプ判定用に、指を置いた瞬間の座標を覚えておく。
    // スワイプと判った時点で `None` に戻し、指を離したときに二重に
    // 判定しないようにする。
    let mut swipe_start: Option<Point> = None;
    // 全モジュールが揃ったときの設定画面からの自動復帰は、起動直後や
    // 失敗からの回復時だけ働かせる。スワイプで自分から設定画面を
    // 開いたときにまで働くと、開いた直後に押し戻されて操作できない。
    let mut auto_return_to_assistant = true;
    // 最後にタッチ操作があった時刻。これが `IDLE_SHUTDOWN_MS` 経つと電源を落とす。
    let mut idle_since_ms = uptime_ms();

    runtime.open_audio();
    advance(&mut state, &mut runtime, startup);
    sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
    log::info!("画面の準備ができました");

    loop {
        let now_ms = uptime_ms();
        let elapsed_ms = (now_ms - last_now_ms) as u32;
        last_now_ms = now_ms;

        let current_voice = runtime
            .config
            .as_ref()
            .map(|config| config.openai.voice.as_str())
            .unwrap_or_default();
        let settings_snapshot =
            settings_layout::lay_out_settings(&runtime.module_statuses, current_voice);

        if let Some(change) = touch.poll() {
            idle_since_ms = now_ms;
            match change {
                TouchChange::Pressed(at) => {
                    swipe_start = Some(at);
                    // 設定画面ではおはなしボタンの意味を持たせない。
                    if screen == Screen::Assistant {
                        if let Some(event) = to_event(TouchChange::Pressed(at)) {
                            advance(&mut state, &mut runtime, event);
                            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
                        }
                    }
                }
                // 指を離すのを待たず、動いた時点でスワイプと分かり次第すぐに
                // 切り替える。画面全体がおはなしボタンの当たり判定を
                // 兼ねているため、指を離すまで待つと「録音が始まった
                // つもりのまま」に見えて切り替わらないように感じる。
                TouchChange::Moved(at) => {
                    if let Some(start) = swipe_start {
                        if let Some(direction) = gesture::detect_swipe(start, at) {
                            swipe_start = None;
                            handle_swipe(
                                direction,
                                &mut screen,
                                &mut state,
                                &mut runtime,
                                &mut auto_return_to_assistant,
                            );
                        }
                    }
                }
                TouchChange::Released(at) => {
                    let swipe = swipe_start.take().and_then(|start| gesture::detect_swipe(start, at));

                    if let Some(direction) = swipe {
                        handle_swipe(
                            direction,
                            &mut screen,
                            &mut state,
                            &mut runtime,
                            &mut auto_return_to_assistant,
                        );
                    } else if screen == Screen::Assistant {
                        if let Some(event) = to_event(TouchChange::Released(at)) {
                            advance(&mut state, &mut runtime, event);
                            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
                        }
                    } else if let Some(voice) = settings_layout::voice_at(&settings_snapshot, at) {
                        runtime.select_voice(voice);
                    }
                }
            }
        }

        runtime.push_captured();
        runtime.poll_search();

        while let Some(event) = runtime.receive() {
            advance(&mut state, &mut runtime, event);
            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
        }
        if let Some(event) = runtime.poll_playback() {
            advance(&mut state, &mut runtime, event);
            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
        }
        if let Some(event) = runtime.poll_turn(elapsed_ms) {
            advance(&mut state, &mut runtime, event);
            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
        }

        // 待機に戻ったときだけ、たまったログを書き出す余裕がある。
        if state == AppState::Ready {
            runtime.flush_logs();
        }

        retry_at = schedule_retry(&state, retry_at, now_ms);
        if retry_at.is_some_and(|due| now_ms >= due) {
            retry_at = None;
            failed_attempts += 1;
            log::info!("やりなおします（{failed_attempts}回目）");
            advance(&mut state, &mut runtime, AppEvent::RetryRequested);
            sync_screen_for_state(&mut screen, &state, &mut auto_return_to_assistant);
        }
        if state == AppState::Ready {
            failed_attempts = 0;
        }

        if now_ms.saturating_sub(idle_since_ms) >= IDLE_SHUTDOWN_MS {
            advance(&mut state, &mut runtime, AppEvent::Idle);
        }

        // 監視対象の全モジュールが整ったら、設定画面から自動的に戻る。
        // ただし自分でスワイプして開いた設定画面まで押し戻さない。
        if screen == Screen::Settings
            && auto_return_to_assistant
            && runtime.module_statuses.all_ready()
        {
            screen = screen::transition_screen(screen, ScreenEvent::AllModulesReady);
        }

        let nod_pending = runtime.take_nod_pending();

        match screen {
            Screen::Assistant => {
                animator.set_expression(Expression::from_state(&state, failed_attempts));
                animator.set_voice_level(runtime.voice_level());
                if nod_pending {
                    animator.trigger_nod(now_ms);
                }
                let frame = animator.frame_at(now_ms);
                let placement = layout::lay_out_face(&frame);

                let _lock = DisplayLock::acquire();
                settings_view.hide();
                view.show();
                view.apply(&placement);
            }
            Screen::Settings => {
                let _lock = DisplayLock::acquire();
                view.hide();
                settings_view.show();
                settings_view.apply(&settings_snapshot);
            }
        }

        FreeRtos::delay_ms(FRAME_INTERVAL_MS);
    }
}

/// 画面ときっかけから次の画面を決め、`screen` を更新する。
fn screen_event_of(direction: SwipeDirection) -> ScreenEvent {
    match direction {
        // 右から左へのスワイプで設定画面へ、逆で戻る。
        SwipeDirection::Left => ScreenEvent::SwipedToSettings,
        SwipeDirection::Right => ScreenEvent::SwipedToAssistant,
    }
}

/// スワイプが分かった時点で呼ぶ。画面を切り替え、押した瞬間に
/// アシスタント画面だったせいで始まってしまっていた録音があれば、
/// 何も言わずに終えたことにして静かに片付ける。
fn handle_swipe(
    direction: SwipeDirection,
    screen: &mut Screen,
    state: &mut AppState,
    runtime: &mut Runtime,
    auto_return_to_assistant: &mut bool,
) {
    if *state == AppState::Listening {
        advance(state, runtime, AppEvent::SpeechNotDetected);
    }

    let event = screen_event_of(direction);
    if event == ScreenEvent::SwipedToSettings {
        // 自分で設定画面を開いたのだから、全モジュールが揃ったからと
        // いって勝手に押し戻さない。
        *auto_return_to_assistant = false;
    }
    *screen = screen::transition_screen(*screen, event);
}

/// 設定不備や失敗に入った瞬間、問答無用で設定画面へ切り替える。
fn sync_screen_for_state(screen: &mut Screen, state: &AppState, auto_return_to_assistant: &mut bool) {
    if matches!(state, AppState::SetupRequired | AppState::Recovering(_)) {
        *screen = screen::transition_screen(*screen, ScreenEvent::ProblemDetected);
        // 直しさえすれば自動的にアシスタント画面へ戻ってよい状況なので、
        // 手動スワイプによる抑止は解除する。
        *auto_return_to_assistant = true;
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
            layout::is_talk_target(at).then_some(AppEvent::TalkPressed)
        }
        TouchChange::Released(_) => Some(AppEvent::TalkReleased),
        TouchChange::Moved(_) => None,
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
