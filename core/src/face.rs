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

/// 視線が正面から向きへ移り、また正面へ戻るのにかける長さ。
/// 途中の向きを増やし、瞬間的に切り替わらずなめらかに動くようにする。
const IDLE_GLANCE_TRANSITION_MS: u64 = 100;
/// 向きへ着いてから戻り始めるまで留める長さ。
const IDLE_GLANCE_PLATEAU_MS: u64 = 500;
/// 待機中、流し目を保つ長さ。行き（遷移）・留まり・帰り（遷移）の合計。
const IDLE_GLANCE_HOLD_MS: u64 =
    IDLE_GLANCE_TRANSITION_MS + IDLE_GLANCE_PLATEAU_MS + IDLE_GLANCE_TRANSITION_MS;
/// 流し目の最短間隔。
const IDLE_GLANCE_MIN_INTERVAL_MS: u64 = 4_000;
/// 流し目の間隔の振れ幅。
const IDLE_GLANCE_INTERVAL_SPREAD_MS: u64 = 5_000;
/// 流し目で視線を振る大きさ。
const IDLE_GLANCE_MAGNITUDE: i8 = 70;

/// 二連続まばたきの1回目と2回目の間隔。
const DOUBLE_BLINK_GAP_MS: u64 = 220;
/// 待機中のまばたきが二連続になる確率の分母（1/N の確率で起こる）。
const DOUBLE_BLINK_CHANCE_DENOM: u32 = 4;

/// 待機中の流し目の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glance {
    Center,
    Left,
    Right,
    Up,
    Down,
}

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

    /// 表情ごとの目の開き具合。まばたきとは独立で、常に真円の大きさを表す。
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
    /// 表情ごとの目の開き具合。0 が閉じ、100 が全開。
    /// まばたきの影響を受けない、常に真円の大きさ。
    pub eye_openness: u8,
    /// まばたきによる目の細まり。0 が完全に閉じ、100 がまばたきしていない
    /// 状態。真円の高さだけを縮めるのに使う（幅には使わない）。
    pub blink_openness: u8,
    /// 口の開き具合。0 が閉じ、100 が全開。
    pub mouth_openness: u8,
    /// 視線の左右。-100 が左、100 が右。
    pub gaze_x: i8,
    /// 視線の上下。-100 が上、100 が下。
    pub gaze_y: i8,
}

/// 時刻を与えると顔のかたちを返す。
#[derive(Debug, Clone)]
pub struct FaceAnimator {
    expression: Expression,
    /// 再生中の音量。話している口の開きに使う。
    voice_level: u8,
    next_blink_at_ms: u64,
    blink_started_at_ms: Option<u64>,
    /// 現在のまばたきの直後に、もう一度まばたきさせるか。
    queued_makeup_blink: bool,
    /// 現在のまばたきが、二連続の2回目であるか。
    /// 2回目のあとにさらに続けて仕込まないための印。
    in_makeup_blink: bool,
    /// 待機中の流し目の向きと予定。
    idle_glance: Glance,
    next_idle_glance_at_ms: u64,
    idle_glance_started_at_ms: Option<u64>,
    /// まばたき間隔などを散らすための擬似乱数の種。
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
            queued_makeup_blink: false,
            in_makeup_blink: false,
            idle_glance: Glance::Center,
            next_idle_glance_at_ms: IDLE_GLANCE_MIN_INTERVAL_MS,
            idle_glance_started_at_ms: None,
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
        self.advance_idle_glance(now_ms);

        FaceFrame {
            expression: self.expression,
            eye_openness: self.expression.resting_eye_openness(),
            blink_openness: self.blink_openness_at(now_ms),
            mouth_openness: self.mouth_openness(),
            gaze_x: self.gaze_x_at(now_ms),
            gaze_y: self.gaze_y_at(now_ms),
        }
    }

    fn advance_blink(&mut self, now_ms: u64) {
        match self.blink_started_at_ms {
            Some(started) if now_ms >= started + BLINK_DURATION_MS => {
                self.blink_started_at_ms = None;
                // 観測した時刻ではなく予定時刻を基準にすることで、
                // 描画間隔が変わってもまばたきの間隔が揺れない。
                if self.queued_makeup_blink {
                    self.queued_makeup_blink = false;
                    self.in_makeup_blink = true;
                    self.next_blink_at_ms = started + BLINK_DURATION_MS + DOUBLE_BLINK_GAP_MS;
                } else {
                    self.in_makeup_blink = false;
                    self.next_blink_at_ms =
                        started + BLINK_DURATION_MS + self.pick_blink_interval();
                }
            }
            None if now_ms >= self.next_blink_at_ms => {
                self.blink_started_at_ms = Some(self.next_blink_at_ms);
                // 2回目のまばたき自体をさらに二連続にはしない。
                if !self.in_makeup_blink {
                    self.queued_makeup_blink =
                        self.expression == Expression::Idle && self.roll_double_blink();
                }
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

    fn roll_double_blink(&mut self) -> bool {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 24) % DOUBLE_BLINK_CHANCE_DENOM == 0
    }

    /// 待機中だけ、視線を左右上下へ流してはまた正面へ戻す。
    fn advance_idle_glance(&mut self, now_ms: u64) {
        if self.expression != Expression::Idle {
            self.idle_glance = Glance::Center;
            self.idle_glance_started_at_ms = None;
            return;
        }

        match self.idle_glance_started_at_ms {
            Some(started) if now_ms >= started + IDLE_GLANCE_HOLD_MS => {
                self.idle_glance_started_at_ms = None;
                self.idle_glance = Glance::Center;
                self.next_idle_glance_at_ms =
                    started + IDLE_GLANCE_HOLD_MS + self.pick_idle_glance_interval();
            }
            None if now_ms >= self.next_idle_glance_at_ms => {
                self.idle_glance_started_at_ms = Some(now_ms);
                self.idle_glance = self.pick_glance_direction();
            }
            _ => {}
        }
    }

    fn pick_idle_glance_interval(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let spread = u64::from(self.seed >> 16) % IDLE_GLANCE_INTERVAL_SPREAD_MS;
        IDLE_GLANCE_MIN_INTERVAL_MS + spread
    }

    /// 左・右・上・下の4パターンから流し目の向きを選ぶ。
    fn pick_glance_direction(&mut self) -> Glance {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        match (self.seed >> 20) % 4 {
            0 => Glance::Left,
            1 => Glance::Right,
            2 => Glance::Up,
            _ => Glance::Down,
        }
    }

    /// まばたきによる目の細まり。0 が完全に閉じ、100 がまばたきしていない状態。
    fn blink_openness_at(&self, now_ms: u64) -> u8 {
        let Some(started) = self.blink_started_at_ms else {
            return 100;
        };

        // まばたきは閉じてから開くまでを直線で近似する。
        let elapsed = (now_ms - started) as f32 / BLINK_DURATION_MS as f32;
        let openness_ratio = (1.0 - 2.0 * elapsed).abs().clamp(0.0, 1.0);

        (100.0 * openness_ratio) as u8
    }

    fn mouth_openness(&self) -> u8 {
        if self.expression == Expression::Talking {
            self.voice_level.max(self.expression.resting_mouth_openness())
        } else {
            self.expression.resting_mouth_openness()
        }
    }

    fn gaze_x_at(&self, now_ms: u64) -> i8 {
        match self.expression {
            Expression::Thinking => {
                let phase = now_ms as f32 / GAZE_SWAY_PERIOD_MS * core::f32::consts::TAU;
                (phase.sin() * 60.0) as i8
            }
            Expression::Idle => match self.idle_glance {
                Glance::Left => -self.idle_glance_magnitude(now_ms),
                Glance::Right => self.idle_glance_magnitude(now_ms),
                _ => 0,
            },
            _ => 0,
        }
    }

    fn gaze_y_at(&self, now_ms: u64) -> i8 {
        if self.expression != Expression::Idle {
            return 0;
        }

        match self.idle_glance {
            Glance::Up => -self.idle_glance_magnitude(now_ms),
            Glance::Down => self.idle_glance_magnitude(now_ms),
            _ => 0,
        }
    }

    /// 流し目の大きさ。正面から向きへ、向きから正面へは瞬時に切り替えず、
    /// 途中の向きを挟んでなめらかに動かす。
    fn idle_glance_magnitude(&self, now_ms: u64) -> i8 {
        let Some(started) = self.idle_glance_started_at_ms else {
            return 0;
        };

        let elapsed = now_ms.saturating_sub(started) as f32;
        let hold = IDLE_GLANCE_HOLD_MS as f32;
        let transition = IDLE_GLANCE_TRANSITION_MS as f32;

        let ratio = if elapsed < transition {
            elapsed / transition
        } else if elapsed < hold - transition {
            1.0
        } else {
            ((hold - elapsed) / transition).clamp(0.0, 1.0)
        };

        (f32::from(IDLE_GLANCE_MAGNITUDE) * ratio) as i8
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

        let before = animator.frame_at(BLINK_MIN_INTERVAL_MS - 1).blink_openness;
        let midway = animator
            .frame_at(BLINK_MIN_INTERVAL_MS + BLINK_DURATION_MS / 2)
            .blink_openness;
        let after = animator
            .frame_at(BLINK_MIN_INTERVAL_MS + BLINK_DURATION_MS)
            .blink_openness;

        assert_eq!(before, 100);
        assert_eq!(midway, 0);
        assert_eq!(after, 100);
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
            let closing = animator.frame_at(now_ms).blink_openness < 30;
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
    fn idle_glances_visit_all_four_directions() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        let frames: Vec<(i8, i8)> = (0..120_000)
            .step_by(50)
            .map(|t| {
                let frame = animator.frame_at(t);
                (frame.gaze_x, frame.gaze_y)
            })
            .collect();

        assert!(frames.iter().any(|&(x, _)| x > 0), "右を見るはず: {frames:?}");
        assert!(frames.iter().any(|&(x, _)| x < 0), "左を見るはず: {frames:?}");
        assert!(frames.iter().any(|&(_, y)| y > 0), "下を見るはず: {frames:?}");
        assert!(frames.iter().any(|&(_, y)| y < 0), "上を見るはず: {frames:?}");
    }

    #[test]
    fn idle_glances_return_to_centre_between_looks() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        let frames: Vec<(i8, i8)> = (0..120_000)
            .step_by(50)
            .map(|t| {
                let frame = animator.frame_at(t);
                (frame.gaze_x, frame.gaze_y)
            })
            .collect();

        assert!(
            frames.iter().any(|&(x, y)| x == 0 && y == 0),
            "見ていないときは正面に戻るはず: {frames:?}"
        );
    }

    #[test]
    fn eyes_do_not_glance_outside_idle() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Listening);

        for t in (0..120_000).step_by(50) {
            let frame = animator.frame_at(t);
            assert_eq!(frame.gaze_y, 0, "Idle以外では上下を見ないはず");
        }
    }

    #[test]
    fn blinks_sometimes_come_in_pairs() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        // 閉じている瞬間どうしの間隔を数え、既定のまばたき間隔より
        // 明らかに短い組が現れることを確かめる。
        let mut closed_at = Vec::new();
        let mut was_closed = false;
        for now_ms in (0..300_000).step_by(10) {
            let closed = animator.frame_at(now_ms).blink_openness == 0;
            if closed && !was_closed {
                closed_at.push(now_ms);
            }
            was_closed = closed;
        }

        let has_pair = closed_at
            .windows(2)
            .any(|pair| pair[1] - pair[0] < BLINK_MIN_INTERVAL_MS / 2);

        assert!(has_pair, "二連続まばたきが一度も起きなかった: {closed_at:?}");
    }

    #[test]
    fn idle_glances_move_gradually_instead_of_snapping() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        // 流し目の途中の向きを踏むまで待ち、直前の一コマと比べて
        // 一気にではなく少しずつ動いていることを確かめる。
        let mut previous = 0i8;
        let mut saw_gradual_step = false;
        for t in (0..120_000).step_by(20) {
            let current = animator.frame_at(t).gaze_x;
            let delta = (current - previous).abs();
            if current != 0 && delta > 0 && delta < IDLE_GLANCE_MAGNITUDE {
                saw_gradual_step = true;
                break;
            }
            previous = current;
        }

        assert!(saw_gradual_step, "流し目は途中の向きを経てなめらかに動くはず");
    }

    #[test]
    fn idle_glance_timing_is_100_500_100() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        let mut started = None;
        for t in (0..20_000).step_by(5) {
            animator.frame_at(t);
            if let Some(s) = animator.idle_glance_started_at_ms {
                started = Some(s);
                break;
            }
        }
        let started = started.expect("流し目が始まらなかった");

        let magnitude_at = |animator: &mut FaceAnimator, offset: u64| -> i8 {
            let frame = animator.frame_at(started + offset);
            frame.gaze_x.abs().max(frame.gaze_y.abs())
        };

        assert_eq!(magnitude_at(&mut animator, 0), 0, "遷移の始まりはまだ正面のはず");
        assert_eq!(
            magnitude_at(&mut animator, IDLE_GLANCE_TRANSITION_MS),
            IDLE_GLANCE_MAGNITUDE,
            "100msで向きに達するはず"
        );
        assert_eq!(
            magnitude_at(&mut animator, IDLE_GLANCE_TRANSITION_MS + IDLE_GLANCE_PLATEAU_MS),
            IDLE_GLANCE_MAGNITUDE,
            "500ms留まる間は向きを保つはず"
        );
        assert_eq!(
            magnitude_at(&mut animator, IDLE_GLANCE_HOLD_MS),
            0,
            "そこから100msで正面へ戻るはず"
        );
    }

    #[test]
    fn blinking_does_not_change_the_resting_eye_openness() {
        let mut animator = FaceAnimator::new();
        animator.set_expression(Expression::Idle);

        // まばたきの最中でも、真円の大きさを表す eye_openness は
        // 表情ごとの一定値のまま変わらない。細まりは blink_openness が表す。
        for now_ms in (0..60_000).step_by(20) {
            let frame = animator.frame_at(now_ms);
            assert_eq!(frame.eye_openness, Expression::Idle.resting_eye_openness());
        }
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
