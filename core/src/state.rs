//! アプリ全体の状態遷移。
//!
//! ボタンは押した瞬間に録音を始めるきっかけにすぎず、押し続ける必要はない。
//! 実際にいつ録音を終えて送るかは端末側の
//! [`crate::turn_detector::TurnDetector`] が声と沈黙から決め、
//! `SpeechEnded` / `SpeechNotDetected` として届く。

/// 画面と音声のふるまいを決める状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// 起動直後。設定の読み込み待ち。
    Booting,
    /// 設定が未記入または不正。親が直すまで先へ進めない。
    SetupRequired,
    /// WiFi に接続中。
    Connecting,
    /// 対話セッションを確立中。
    Opening,
    /// 待機中。話しかけられるのを待っている。
    Ready,
    /// 録音中。ボタンが押されている間。
    Listening,
    /// 送信済みで応答を待っている。
    Thinking,
    /// 応答音声を再生中。
    Speaking,
    /// 失敗から復帰する前の待機。
    Recovering(Failure),
    /// 無操作が続いたため、電源を落とす直前。
    ShuttingDown,
}

/// 復帰可能な失敗の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// WiFi に繋がらない。
    Network,
    /// 対話セッションが切れた。
    Session,
    /// SDカードを読み書きできない。
    Storage,
}

impl Failure {
    /// 何が起きたかを親に示す文。
    pub fn describe(self) -> &'static str {
        match self {
            Self::Network => "WiFiに繋がりません",
            Self::Session => "対話の準備ができません",
            Self::Storage => "SDカードを読めません",
        }
    }

    /// どう直せばよいかを示す文。
    pub fn remedy(self) -> &'static str {
        match self {
            Self::Network => {
                "config.toml のWiFi名とパスワードを確かめてください。2.4GHzのWiFiが必要です"
            }
            Self::Session => {
                "config.toml のAPIキーと、OpenAIの利用残高を確かめてください"
            }
            Self::Storage => "SDカードが入っているか確かめてください",
        }
    }
}

/// 状態を動かすきっかけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// 設定を読み込めた。
    ConfigLoaded,
    /// 設定が未記入・不正だった。
    ConfigRejected,
    /// WiFi に接続できた。
    NetworkReady,
    /// 対話セッションが確立した。
    SessionOpened,
    /// ボタンが押された。
    TalkPressed,
    /// ボタンが離された。
    TalkReleased,
    /// 声のあとに沈黙が区切りの長さまで続き、話し終わったとみなせる。
    SpeechEnded,
    /// 声が一度も無いまま沈黙が区切りの長さまで続いた。
    SpeechNotDetected,
    /// 応答音声が届き始めた。
    ResponseStarted,
    /// 応答が終わった。
    ResponseFinished,
    /// 失敗した。
    Failed(Failure),
    /// やり直す。
    RetryRequested,
    /// 一定時間、操作が無かった。
    Idle,
}

/// 状態遷移に伴ってハードウェア層に依頼する処理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    /// WiFi 接続を開始する。
    ConnectNetwork,
    /// 対話セッションを開く。
    OpenSession,
    /// 対話セッションを閉じる。
    CloseSession,
    /// マイクの取り込みを始める。
    StartCapture,
    /// マイクの取り込みを止める。
    StopCapture,
    /// 録音を送信し、応答を要求する。
    RequestResponse,
    /// 生成中の応答を打ち切る。
    CancelResponse,
    /// 応答音声の再生を始める。
    StartPlayback,
    /// 応答音声の再生を止める。
    StopPlayback,
    /// 設定の記入方法を親に向けて表示する。
    ShowSetupGuide,
    /// 失敗の内容と対処法を表示する。
    ShowFailure(Failure),
    /// 電源を切る。
    PowerOff,
}

/// 遷移の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub next: AppState,
    pub actions: Vec<AppAction>,
}

impl Transition {
    fn to(next: AppState, actions: Vec<AppAction>) -> Self {
        Self { next, actions }
    }

    /// 現在の状態では意味を持たないきっかけ。取りこぼしても害がないので何もしない。
    fn ignore(current: &AppState) -> Self {
        Self {
            next: current.clone(),
            actions: Vec::new(),
        }
    }
}

/// 状態ときっかけから次の状態と実行すべき処理を決める。
pub fn transition(current: &AppState, event: AppEvent) -> Transition {
    // 失敗はどの状態からでも起こりうるので先に処理する。
    if let AppEvent::Failed(failure) = event {
        return Transition::to(
            AppState::Recovering(failure),
            vec![
                AppAction::StopCapture,
                AppAction::StopPlayback,
                AppAction::CloseSession,
                AppAction::ShowFailure(failure),
            ],
        );
    }

    // 無操作もどの状態からでも起こりうる。すでに電源を落とす途中なら何もしない。
    if event == AppEvent::Idle {
        return match current {
            AppState::ShuttingDown => Transition::ignore(current),
            _ => Transition::to(
                AppState::ShuttingDown,
                vec![
                    AppAction::StopCapture,
                    AppAction::StopPlayback,
                    AppAction::CloseSession,
                    AppAction::PowerOff,
                ],
            ),
        };
    }

    match (current, event) {
        (AppState::Booting, AppEvent::ConfigLoaded) => {
            Transition::to(AppState::Connecting, vec![AppAction::ConnectNetwork])
        }
        (AppState::Booting, AppEvent::ConfigRejected) => {
            Transition::to(AppState::SetupRequired, vec![AppAction::ShowSetupGuide])
        }

        (AppState::Connecting, AppEvent::NetworkReady) => {
            Transition::to(AppState::Opening, vec![AppAction::OpenSession])
        }

        (AppState::Opening, AppEvent::SessionOpened) => {
            Transition::to(AppState::Ready, Vec::new())
        }

        (AppState::Ready, AppEvent::TalkPressed) => {
            Transition::to(AppState::Listening, vec![AppAction::StartCapture])
        }

        // 応答中に話しかけられたら応答を止めて聞く側に回る。
        // 子供は相手の話し終わりを待たないので、割り込みを許さないと会話が成立しない。
        (AppState::Speaking, AppEvent::TalkPressed) => Transition::to(
            AppState::Listening,
            vec![
                AppAction::CancelResponse,
                AppAction::StopPlayback,
                AppAction::StartCapture,
            ],
        ),

        // 応答待ちの最中に押し直された場合も、待っていた応答は捨てる。
        (AppState::Thinking, AppEvent::TalkPressed) => Transition::to(
            AppState::Listening,
            vec![AppAction::CancelResponse, AppAction::StartCapture],
        ),

        // 声のあとの沈黙が区切りに達したら、録音を止めて応答を求める。
        // ボタンを離すタイミングは見ない。押し続ける必要が無いため。
        (AppState::Listening, AppEvent::SpeechEnded) => Transition::to(
            AppState::Thinking,
            vec![AppAction::StopCapture, AppAction::RequestResponse],
        ),

        // 声が一度も無いまま沈黙が続いたら、何も送らず静かに待機へ戻る。
        (AppState::Listening, AppEvent::SpeechNotDetected) => {
            Transition::to(AppState::Ready, vec![AppAction::StopCapture])
        }

        (AppState::Thinking, AppEvent::ResponseStarted) => {
            Transition::to(AppState::Speaking, vec![AppAction::StartPlayback])
        }

        (AppState::Speaking, AppEvent::ResponseFinished) => {
            Transition::to(AppState::Ready, vec![AppAction::StopPlayback])
        }

        // 音声が一度も届かないまま応答が終わることがある。
        (AppState::Thinking, AppEvent::ResponseFinished) => {
            Transition::to(AppState::Ready, Vec::new())
        }

        (AppState::Recovering(_), AppEvent::RetryRequested) => {
            Transition::to(AppState::Connecting, vec![AppAction::ConnectNetwork])
        }

        _ => Transition::ignore(current),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一連のきっかけを順に与えて最終状態と全処理を得る。
    fn run(start: AppState, events: &[AppEvent]) -> (AppState, Vec<AppAction>) {
        let mut state = start;
        let mut actions = Vec::new();
        for event in events {
            let step = transition(&state, event.clone());
            state = step.next;
            actions.extend(step.actions);
        }
        (state, actions)
    }

    #[test]
    fn walks_from_boot_to_ready() {
        let (state, actions) = run(
            AppState::Booting,
            &[
                AppEvent::ConfigLoaded,
                AppEvent::NetworkReady,
                AppEvent::SessionOpened,
            ],
        );

        assert_eq!(state, AppState::Ready);
        assert_eq!(
            actions,
            vec![AppAction::ConnectNetwork, AppAction::OpenSession]
        );
    }

    #[test]
    fn stops_at_setup_when_config_is_rejected() {
        let (state, actions) = run(AppState::Booting, &[AppEvent::ConfigRejected]);

        assert_eq!(state, AppState::SetupRequired);
        assert_eq!(actions, vec![AppAction::ShowSetupGuide]);
    }

    #[test]
    fn setup_required_ignores_button() {
        let (state, actions) = run(
            AppState::SetupRequired,
            &[AppEvent::TalkPressed, AppEvent::TalkReleased],
        );

        assert_eq!(state, AppState::SetupRequired);
        assert!(actions.is_empty());
    }

    #[test]
    fn press_and_release_completes_one_turn() {
        let (state, actions) = run(
            AppState::Ready,
            &[
                AppEvent::TalkPressed,
                AppEvent::SpeechEnded,
                AppEvent::ResponseStarted,
                AppEvent::ResponseFinished,
            ],
        );

        assert_eq!(state, AppState::Ready);
        assert_eq!(
            actions,
            vec![
                AppAction::StartCapture,
                AppAction::StopCapture,
                AppAction::RequestResponse,
                AppAction::StartPlayback,
                AppAction::StopPlayback,
            ]
        );
    }

    #[test]
    fn silence_without_any_speech_returns_to_ready_quietly() {
        let step = transition(&AppState::Listening, AppEvent::SpeechNotDetected);

        assert_eq!(step.next, AppState::Ready);
        assert_eq!(step.actions, vec![AppAction::StopCapture]);
    }

    #[test]
    fn releasing_the_button_no_longer_ends_the_turn() {
        let step = transition(&AppState::Listening, AppEvent::TalkReleased);

        assert_eq!(step.next, AppState::Listening);
        assert!(step.actions.is_empty());
    }

    #[test]
    fn pressing_while_speaking_interrupts_the_answer() {
        let step = transition(&AppState::Speaking, AppEvent::TalkPressed);

        assert_eq!(step.next, AppState::Listening);
        assert_eq!(
            step.actions,
            vec![
                AppAction::CancelResponse,
                AppAction::StopPlayback,
                AppAction::StartCapture,
            ]
        );
    }

    #[test]
    fn pressing_while_thinking_discards_the_pending_answer() {
        let step = transition(&AppState::Thinking, AppEvent::TalkPressed);

        assert_eq!(step.next, AppState::Listening);
        assert_eq!(
            step.actions,
            vec![AppAction::CancelResponse, AppAction::StartCapture]
        );
    }

    #[test]
    fn returns_to_ready_when_answer_has_no_audio() {
        let step = transition(&AppState::Thinking, AppEvent::ResponseFinished);

        assert_eq!(step.next, AppState::Ready);
        assert!(step.actions.is_empty());
    }

    #[test]
    fn failure_from_any_state_stops_everything() {
        for start in [
            AppState::Connecting,
            AppState::Ready,
            AppState::Listening,
            AppState::Thinking,
            AppState::Speaking,
        ] {
            let step = transition(&start, AppEvent::Failed(Failure::Session));

            assert_eq!(step.next, AppState::Recovering(Failure::Session));
            assert!(step.actions.contains(&AppAction::CloseSession));
            assert!(step
                .actions
                .contains(&AppAction::ShowFailure(Failure::Session)));
        }
    }

    #[test]
    fn idle_from_any_state_shuts_down() {
        for start in [
            AppState::Booting,
            AppState::SetupRequired,
            AppState::Connecting,
            AppState::Ready,
            AppState::Listening,
            AppState::Thinking,
            AppState::Speaking,
            AppState::Recovering(Failure::Network),
        ] {
            let step = transition(&start, AppEvent::Idle);

            assert_eq!(step.next, AppState::ShuttingDown);
            assert_eq!(
                step.actions,
                vec![
                    AppAction::StopCapture,
                    AppAction::StopPlayback,
                    AppAction::CloseSession,
                    AppAction::PowerOff,
                ]
            );
        }
    }

    #[test]
    fn idle_while_already_shutting_down_is_ignored() {
        let step = transition(&AppState::ShuttingDown, AppEvent::Idle);

        assert_eq!(step.next, AppState::ShuttingDown);
        assert!(step.actions.is_empty());
    }

    #[test]
    fn retry_reconnects_from_recovering() {
        let step = transition(&AppState::Recovering(Failure::Network), AppEvent::RetryRequested);

        assert_eq!(step.next, AppState::Connecting);
        assert_eq!(step.actions, vec![AppAction::ConnectNetwork]);
    }

    #[test]
    fn release_without_press_is_ignored() {
        let step = transition(&AppState::Ready, AppEvent::TalkReleased);

        assert_eq!(step.next, AppState::Ready);
        assert!(step.actions.is_empty());
    }

    #[test]
    fn every_failure_offers_a_remedy() {
        for failure in [Failure::Network, Failure::Session, Failure::Storage] {
            assert!(!failure.describe().is_empty());
            assert!(!failure.remedy().is_empty());
        }
    }
}
