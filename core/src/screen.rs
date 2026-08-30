//! アシスタント画面と設定画面の切り替え。
//!
//! [`crate::state::AppState`] は対話のフェーズだけを表す状態機械であり、
//! どちらの画面を出すかとは別軸で決まる。混ぜると組み合わせが爆発するため、
//! 独立した小さな状態機械としてここに分ける。

/// 表示中の画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// 顔で応答するふだんの画面。
    Assistant,
    /// 各モジュールの準備状況を並べる画面。
    Settings,
}

/// 画面を切り替えるきっかけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenEvent {
    /// 指で左スワイプし、設定画面へ切り替えた。
    SwipedToSettings,
    /// 設定画面の閉じるボタンを押した。
    ///
    /// 設定画面からアシスタント画面へは、スワイプでは戻れない
    /// （閉じるボタン・[`Self::AllModulesReady`] のいずれかでのみ戻る）。
    CloseButtonPressed,
    /// 設定不備や失敗が起き、親に対処してもらう必要が生じた。
    ProblemDetected,
    /// 監視対象の全モジュールが準備できた。
    AllModulesReady,
}

/// 画面ときっかけから次に出す画面を決める。
///
/// 自動遷移（`ProblemDetected` / `AllModulesReady`）と手動切り替えは
/// 対等に扱い、直近に届いたきっかけが画面を決める。
pub fn transition_screen(current: Screen, event: ScreenEvent) -> Screen {
    let _ = current;
    match event {
        ScreenEvent::CloseButtonPressed => Screen::Assistant,
        ScreenEvent::SwipedToSettings => Screen::Settings,
        ScreenEvent::ProblemDetected => Screen::Settings,
        ScreenEvent::AllModulesReady => Screen::Assistant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swiping_opens_settings() {
        assert_eq!(
            transition_screen(Screen::Assistant, ScreenEvent::SwipedToSettings),
            Screen::Settings
        );
    }

    #[test]
    fn close_button_returns_to_assistant() {
        assert_eq!(
            transition_screen(Screen::Settings, ScreenEvent::CloseButtonPressed),
            Screen::Assistant
        );
    }

    #[test]
    fn problem_forces_settings_screen_even_from_assistant() {
        assert_eq!(
            transition_screen(Screen::Assistant, ScreenEvent::ProblemDetected),
            Screen::Settings
        );
        assert_eq!(
            transition_screen(Screen::Settings, ScreenEvent::ProblemDetected),
            Screen::Settings
        );
    }

    #[test]
    fn all_modules_ready_returns_to_assistant_screen() {
        assert_eq!(
            transition_screen(Screen::Settings, ScreenEvent::AllModulesReady),
            Screen::Assistant
        );
        assert_eq!(
            transition_screen(Screen::Assistant, ScreenEvent::AllModulesReady),
            Screen::Assistant
        );
    }
}
