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
use m5a_core::guardrail::{Guardrail, Verdict};
use m5a_core::layout;
use m5a_core::logbook::{self, LogEntry, Speaker};
use m5a_core::ports::StorageError;
use m5a_core::realtime::{self, ServerEvent, SessionSetup};
use m5a_core::search;
use m5a_core::state::{transition, AppAction, AppEvent, AppState, Failure};
use m5a_core::turn_detector::{TurnDetector, TurnOutcome};

use hal::audio::Audio;
use hal::board::{self, DisplayLock};
use hal::face::FaceView;
use hal::session::Session;
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

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().expect("周辺機器を取得できません");

    board::init_bus()?;
    board::start_display()?;
    board::set_brightness(SCREEN_BRIGHTNESS)?;
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
            "せっていを よみました: なまえ={} アシスタント名={} モデル={} こえ={}",
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
}

impl Runtime {
    fn new(modem: Modem<'static>, config: Option<Config>) -> Self {
        let guardrail = config
            .as_ref()
            .map(|config| Guardrail::new(&config.child.name, config.child.age, &config.assistant.name));

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
                log::warn!("せっていを かきこんで ください");
                None
            }
            AppAction::ShowFailure(failure) => {
                log::warn!("{}", failure.describe());
                log::warn!("→ {}", failure.remedy());
                self.record(Speaker::System, failure.describe());
                None
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
            Ok(audio) => self.audio = Some(audio),
            Err(error) => log::warn!("音を使えません: {error:#}"),
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
                None
            }
            Err(error) => {
                log::warn!("{error:#}");
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
        self.turn.as_ref()?;
        let level = self.audio.as_ref().map_or(0, Audio::input_level);
        // しきい値が実機に合っているか、後で確かめられるように残す。
        log::debug!("マイクの音量: {level}");
        let outcome = self.turn.as_mut()?.observe(level, elapsed_ms)?;

        Some(match outcome {
            TurnOutcome::SpeechEnded => AppEvent::SpeechEnded,
            TurnOutcome::NothingSaid => AppEvent::SpeechNotDetected,
        })
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
        log::warn!("気になる はなし: {concern:?}");

        if concern.should_notify_parent() {
            self.record(Speaker::System, format!("気になる発言がありました: {text}"));
        }
        self.record(Speaker::System, reply);
    }

    /// モデルからの function tool 呼び出しを受け、検索を始めるか、
    /// その場で「わからない」を返す。
    fn handle_tool_call(&mut self, call_id: &str, name: &str, arguments: &str) {
        if name != search::TOOL_NAME {
            self.finish_tool_call(call_id, "それは できません");
            return;
        }

        let Some(api_key) = self.config.as_ref().and_then(|config| config.search.api_key())
        else {
            self.finish_tool_call(call_id, "いまは しらべられません");
            return;
        };

        let Some(query) = search::extract_query(arguments) else {
            self.finish_tool_call(call_id, "なにを しらべるか わかりませんでした");
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
                return;
            }
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
    // 立ち上げ中の失敗を数える。数回は読み込み中として見せ、
    // それでも駄目なときだけ困り顔で親に伝える。
    let mut failed_attempts = 0_u32;
    let mut last_now_ms = uptime_ms();

    runtime.open_audio();
    advance(&mut state, &mut runtime, startup);
    log::info!("画面の準備ができました");

    loop {
        let now_ms = uptime_ms();
        let elapsed_ms = (now_ms - last_now_ms) as u32;
        last_now_ms = now_ms;

        if let Some(change) = touch.poll() {
            if let Some(event) = to_event(change) {
                advance(&mut state, &mut runtime, event);
            }
        }

        runtime.push_captured();
        runtime.poll_search();

        while let Some(event) = runtime.receive() {
            advance(&mut state, &mut runtime, event);
        }
        if let Some(event) = runtime.poll_playback() {
            advance(&mut state, &mut runtime, event);
        }
        if let Some(event) = runtime.poll_turn(elapsed_ms) {
            advance(&mut state, &mut runtime, event);
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
        }
        if state == AppState::Ready {
            failed_attempts = 0;
        }

        animator.set_expression(Expression::from_state(&state, failed_attempts));
        animator.set_voice_level(runtime.voice_level());
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
            layout::is_talk_target(at).then_some(AppEvent::TalkPressed)
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
