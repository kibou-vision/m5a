//! 画面に出すアシスタントの顔。
//!
//! 描画そのものはハードウェア層が行い、ここでは「いまどんな顔か」を
//! 時刻から決める。実機なしで検証できるよう乱数は持たず、
//! まばたきの間隔だけを内部の擬似乱数で散らす。

use crate::state::AppState;

/// まばたき1回にかける時間。
const BLINK_DURATION_MS: u64 = 140;
/// まばたきの最短間隔。
const BLINK_MIN_INTERVAL_MS: u64 = 2_500;
/// まばたき間隔の振れ幅。一定間隔だと機械的に見えるため散らす。
const BLINK_INTERVAL_SPREAD_MS: u64 = 2_500;
/// 失敗が続いても「読み込み中」として見せる回数。
///
/// Wi-Fi は最初の数回つながらないことが普通にあり、そのたびに困り顔を
/// 見せると子どもを不安にさせる。何度も駄目なときだけ親に伝える。
pub const PATIENT_ATTEMPTS: u32 = 5;

/// 考えているときに視線が左右に往復する周期。
const GAZE_SWAY_PERIOD_MS: f32 = 1_800.0;

/// 顔の基本の表情。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expression {
    /// 準備中。接続待ち。
    Waiting,
    /// 待機中。ふつうの笑顔。
    Idle,
    /// 聞いている。目を見開いている。
    Listening,
    /// 考えている。視線が泳ぐ。
    Thinking,
    /// 話している。口が動く。
    Talking,
    /// 困っている。設定不備や失敗のとき。
    Trouble,
}

impl Expression {
    /// アプリの状態に対応する表情。
    ///
    /// 立ち上げ中の失敗は `failed_attempts` が少ないうちは待ち扱いにする。
    /// 設定不備は親が直すまで直らないので、最初から困り顔で伝える。
    pub fn from_state(state: &AppState, failed_attempts: u32) -> Self {
        match state {
            AppState::Booting | AppState::Connecting | AppState::Opening => Self::Waiting,
            AppState::Ready => Self::Idle,
            AppState::Listening => Self::Listening,
            AppState::Thinking => Self::Thinking,
            AppState::Speaking => Self::Talking,
            AppState::SetupRequired => Self::Trouble,
            AppState::Recovering(_) if failed_attempts < PATIENT_ATTEMPTS => Self::Waiting,
            AppState::Recovering(_) => Self::Trouble,
        }
    }

    /// 立ち上げ中で、顔ではなく読み込みの印を見せる場面か。
    pub fn is_loading(self) -> bool {
        self == Self::Waiting
    }

    /// まばたきしていないときの目の開き具合。
    fn resting_eye_openness(self) -> u8 {
        match self {
            Self::Waiting => 70,
            Self::Idle => 90,
            Self::Listening => 100,
            Self::Thinking => 80,
            Self::Talking => 90,
            Self::Trouble => 60,
        }
    }

    /// 声を出していないときの口の開き具合。
    fn resting_mouth_openness(self) -> u8 {
        match self {
            Self::Talking => 10,
            _ => 5,
        }
    }

}

/// ある瞬間の顔のかたち。描画側はこれだけを見て絵を組み立てる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceFrame {
    pub expression: Expression,
    /// 目の開き具合。0 が閉じ、100 が全開。
    pub eye_openness: u8,
    /// 口の開き具合。0 が閉じ、100 が全開。
    pub mouth_openness: u8,
    /// 視線の左右。-100 が左、100 が右。
    pub gaze_x: i8,
}

/// 時刻を与えると顔のかたちを返す。
#[derive(Debug, Clone)]
pub struct FaceAnimator {
    expression: Expression,
    /// 再生中の音量。話している口の開きに使う。
    voice_level: u8,
    next_blink_at_ms: u64,
    blink_started_at_ms: Option<u64>,
    /// まばたき間隔を散らすための擬似乱数の種。
    seed: u32,
}

impl Default for FaceAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl FaceAnimator {
    pub fn new() -> Self {
        Self {
            // 最初の一コマは状態が決まる前なので、読み込み中として始める。
            expression: Expression::Waiting,
            voice_level: 0,
            next_blink_at_ms: BLINK_MIN_INTERVAL_MS,
            blink_started_at_ms: None,
            seed: 0x5A5A_1234,
        }
    }

    pub fn set_expression(&mut self, expression: Expression) {
        self.expression = expression;
    }

    /// 再生中の音量を伝える。話している口の開きに反映される。
    pub fn set_voice_level(&mut self, level: u8) {
        self.voice_level = level.min(100);
    }

    /// その時刻の顔を返す。
    pub fn frame_at(&mut self, now_ms: u64) -> FaceFrame {
        self.advance_blink(now_ms);

        FaceFrame {
            expression: self.expression,
            eye_openness: self.eye_openness_at(now_ms),
            mouth_openness: self.mouth_openness(),
            gaze_x: self.gaze_x_at(now_ms),
        }
    }

    fn advance_blink(&mut self, now_ms: u64) {
        match self.blink_started_at_ms {
            Some(started) if now_ms >= started + BLINK_DURATION_MS => {
                self.blink_started_at_ms = None;
                // 観測した時刻ではなく予定時刻を基準にすることで、
                // 描画間隔が変わってもまばたきの間隔が揺れない。
                self.next_blink_at_ms = started + BLINK_DURATION_MS + self.pick_blink_interval();
            }
            None if now_ms >= self.next_blink_at_ms => {
                self.blink_started_at_ms = Some(self.next_blink_at_ms);
            }
            _ => {}
        }
    }

    /// 線形合同法。実機と試験で同じ並びになれば十分なので簡素なもので足りる。
    fn pick_blink_interval(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let spread = u64::from(self.seed >> 16) % BLINK_INTERVAL_SPREAD_MS;
        BLINK_MIN_INTERVAL_MS + spread
    }

    fn eye_openness_at(&self, now_ms: u64) -> u8 {
        let resting = self.expression.resting_eye_openness();

        let Some(started) = self.blink_started_at_ms else {
            return resting;
        };

        // まばたきは閉じてから開くまでを直線で近似する。
        let elapsed = (now_ms - started) as f32 / BLINK_DURATION_MS as f32;
        let openness_ratio = (1.0 - 2.0 * elapsed).abs().clamp(0.0, 1.0);

        (f32::from(resting) * openness_ratio) as u8
    }

    fn mouth_openness(&self) -> u8 {
        if self.expression == Expression::Talking {
            self.voice_level.max(self.expression.resting_mouth_openness())
        } else {
            self.expression.resting_mouth_openness()
        }
    }

    fn gaze_x_at(&self, now_ms: u64) -> i8 {
        if self.expression != Expression::Thinking {
            return 0;
        }

        let phase = now_ms as f32 / GAZE_SWAY_PERIOD_MS * core::f32::consts::TAU;
        (phase.sin() * 60.0) as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Failure;

    #[test]
    fn maps_every_state_to_an_expression() {
        let pairs = [
            (AppState::Booting, Expression::Waiting),
            (AppState::Connecting, Expression::Waiting),
            (AppState::Opening, Expression::Waiting),
            (AppState::Ready, Expression::Idle),
            (AppState::Listening, Expression::Listening),
            (AppState::Thinking, Expression::Thinking),
            (AppState::Speaking, Expression::Talking),
            (AppState::SetupRequired, Expression::Trouble),
        ];

        for (state, expected) in pairs {
            assert_eq!(Expression::from_state(&state, 0), expected, "state={state:?}");
        }
    }

    #[test]
    fn early_failures_look_like_loading() {
        let recovering = AppState::Recovering(Failure::Network);

        // 起動直後の数回は普通に失敗する。困り顔を見せない。
        for attempts in 0..PATIENT_ATTEMPTS {
            assert_eq!(
                Expression::from_state(&recovering, attempts),
                Expression::Waiting,
                "{attempts}回目"
            );
        }
    }

    #[test]
    fn persistent_failures_ask_for_help() {
        let recovering = AppState::Recovering(Failure::Network);

        assert_eq!(
            Expression::from_state(&recovering, PATIENT_ATTEMPTS),
            Expression::Trouble
        );
    }

    #[test]
    fn setup_trouble_shows_immediately() {
        // 設定不備は待っても直らないので、最初から親に伝える。
        assert_eq!(
            Expression::from_state(&AppState::SetupRequired, 0),
            Expression::Trouble
        );
    }

    #[test]
    fn loading_states_hide_the_face() {
        assert!(Expression::Waiting.is_loading());
        assert!(!Expression::Idle.is_loading());
        assert!(!Expression::Trouble.is_loading());
    }

    #[test]
    fn eyes_close_and_reopen_during_a_blink() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        let before = animator.frame_at(BLINK_MIN_INTERVAL_MS - 1).eye_openness;
        let midway = animator
            .frame_at(BLINK_MIN_INTERVAL_MS + BLINK_DURATION_MS / 2)
            .eye_openness;
        let after = animator
            .frame_at(BLINK_MIN_INTERVAL_MS + BLINK_DURATION_MS)
            .eye_openness;

        assert_eq!(before, Expression::Idle.resting_eye_openness());
        assert_eq!(midway, 0);
        assert_eq!(after, Expression::Idle.resting_eye_openness());
    }

    #[test]
    fn blinks_repeatedly_over_time() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        // 20ms 間隔の観測では完全に閉じた瞬間を踏むとは限らないため、
        // 十分に閉じている状態への変わり目を数える。
        let mut blink_count = 0;
        let mut was_closing = false;
        for now_ms in (0..60_000).step_by(20) {
            let closing = animator.frame_at(now_ms).eye_openness < 30;
            if closing && !was_closing {
                blink_count += 1;
            }
            was_closing = closing;
        }

        assert!(
            (12..=24).contains(&blink_count),
            "60秒で12〜24回まばたきするはず: {blink_count}回"
        );
    }

    #[test]
    fn mouth_follows_voice_level_only_while_talking() {
        let mut animator = FaceAnimator::new();
        animator.set_voice_level(80);

        animator.set_expression(Expression::Talking);
        assert_eq!(animator.frame_at(0).mouth_openness, 80);

        animator.set_expression(Expression::Listening);
        assert_eq!(animator.frame_at(0).mouth_openness, 5);
    }

    #[test]
    fn gaze_sways_only_while_thinking() {
        let mut animator = FaceAnimator::new();

        animator.set_expression(Expression::Idle);
        assert_eq!(animator.frame_at(500).gaze_x, 0);

        animator.set_expression(Expression::Thinking);
        let sway: Vec<i8> = (0..2_000).step_by(100).map(|t| animator.frame_at(t).gaze_x).collect();
        assert!(sway.iter().any(|&x| x > 20), "右に振れるはず: {sway:?}");
        assert!(sway.iter().any(|&x| x < -20), "左に振れるはず: {sway:?}");
    }

    #[test]
    fn is_reproducible_for_the_same_time_sequence() {
        let frames_of = || {
            let mut animator = FaceAnimator::new();
            animator.set_expression(Expression::Idle);
            (0..20_000)
                .step_by(37)
                .map(|t| animator.frame_at(t))
                .collect::<Vec<_>>()
        };

        assert_eq!(frames_of(), frames_of());
    }
}
