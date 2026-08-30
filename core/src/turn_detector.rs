//! 発話の区切りを端末側で見つける。
//!
//! サーバー側の発話区切り検出（VAD）は使わない方針（[音声対話](../../docs/spec/conversation.md)参照）
//! のため、録音ボタンを押している間ではなく、実際の声の有無と沈黙の長さから
//! 話し終わりを判断する。声が一度もないまま沈黙が続いた場合は、何も送らず
//! 静かに終える。
//!
//! 背景に音楽など常に鳴っている音があると、しきい値を超え続けて沈黙が
//! 一度も訪れず、区切りをいつまでも検出できないことがある。そのため
//! 沈黙とは別に、録音の長さ自体にも上限（[`MAX_DURATION_MS`]）を設ける。

/// 声とみなす音量のしきい値（[`crate::audio::measure_level`] の 0〜100 のうち）。
pub const VOICE_THRESHOLD: u8 = 6;
/// 沈黙がこの時間続いたら区切りとみなす。
pub const SILENCE_TIMEOUT_MS: u32 = 1_400;
/// 沈黙が訪れなくても、録音の長さがこれに達したら区切りとみなす。
pub const MAX_DURATION_MS: u32 = 15_000;

/// 沈黙が区切りの長さに達したときの判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    /// 声が一度も無いまま沈黙が続いた。何も送らず終える。
    NothingSaid,
    /// 声のあとに沈黙が続いた。ここまでの録音を確定して送る。
    SpeechEnded,
}

/// 録音中の声と沈黙を追跡する。
#[derive(Debug, Clone, Default)]
pub struct TurnDetector {
    silence_ms: u32,
    /// 録音を始めてからの経過時間。声が途切れず沈黙が来ない場合の
    /// 上限判定（[`MAX_DURATION_MS`]）に使う。
    total_ms: u32,
    spoke: bool,
}

impl TurnDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// これまでに一度でも声を検出したか。
    ///
    /// これが真になってから録音を送り始める。偽の間に来た音は、
    /// 声を待っているだけの沈黙として送らずに捨てる。
    pub fn has_spoken(&self) -> bool {
        self.spoke
    }

    /// 直近 `elapsed_ms` の間の音量を反映し、区切りに達していれば判定を返す。
    pub fn observe(&mut self, level: u8, elapsed_ms: u32) -> Option<TurnOutcome> {
        self.total_ms = self.total_ms.saturating_add(elapsed_ms);

        if level >= VOICE_THRESHOLD {
            self.spoke = true;
            self.silence_ms = 0;
        } else {
            self.silence_ms = self.silence_ms.saturating_add(elapsed_ms);
        }

        let silence_reached_timeout = self.silence_ms >= SILENCE_TIMEOUT_MS;
        let duration_reached_limit = self.total_ms >= MAX_DURATION_MS;
        if !silence_reached_timeout && !duration_reached_limit {
            return None;
        }

        Some(if self.spoke {
            TurnOutcome::SpeechEnded
        } else {
            TurnOutcome::NothingSaid
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_without_having_spoken() {
        assert!(!TurnDetector::new().has_spoken());
    }

    #[test]
    fn voice_marks_that_speech_happened() {
        let mut detector = TurnDetector::new();

        assert_eq!(detector.observe(VOICE_THRESHOLD, 20), None);
        assert!(detector.has_spoken());
    }

    #[test]
    fn silence_below_the_timeout_does_not_conclude() {
        let mut detector = TurnDetector::new();

        assert_eq!(detector.observe(0, SILENCE_TIMEOUT_MS - 1), None);
    }

    #[test]
    fn silence_alone_reaching_the_timeout_means_nothing_was_said() {
        let mut detector = TurnDetector::new();

        assert_eq!(
            detector.observe(0, SILENCE_TIMEOUT_MS),
            Some(TurnOutcome::NothingSaid)
        );
    }

    #[test]
    fn speech_then_silence_reaching_the_timeout_ends_the_turn() {
        let mut detector = TurnDetector::new();

        detector.observe(50, 200);
        assert_eq!(
            detector.observe(0, SILENCE_TIMEOUT_MS),
            Some(TurnOutcome::SpeechEnded)
        );
    }

    #[test]
    fn voice_resets_the_silence_timer() {
        let mut detector = TurnDetector::new();

        detector.observe(50, 200);
        assert_eq!(detector.observe(0, SILENCE_TIMEOUT_MS - 500), None);
        // ここで再び声が来ると、沈黙の時計はゼロから数え直すはず。
        assert_eq!(detector.observe(50, 20), None);
        assert_eq!(detector.observe(0, SILENCE_TIMEOUT_MS - 1), None);
        assert_eq!(
            detector.observe(0, 1),
            Some(TurnOutcome::SpeechEnded)
        );
    }

    #[test]
    fn quiet_background_noise_below_the_threshold_does_not_count_as_speech() {
        let mut detector = TurnDetector::new();

        detector.observe(VOICE_THRESHOLD - 1, SILENCE_TIMEOUT_MS);

        assert!(!detector.has_spoken());
    }

    #[test]
    fn continuous_noise_never_reaching_silence_still_ends_at_the_duration_limit() {
        let mut detector = TurnDetector::new();

        // 背景の音楽などでしきい値を超え続け、沈黙が一度も来ない状況を想定する。
        let mut outcome = None;
        for _ in 0..(MAX_DURATION_MS / 20 - 1) {
            outcome = detector.observe(50, 20);
        }
        assert_eq!(outcome, None, "上限に達する前は区切られないはず");

        assert_eq!(detector.observe(50, 20), Some(TurnOutcome::SpeechEnded));
    }

    #[test]
    fn duration_limit_does_not_fire_early() {
        let mut detector = TurnDetector::new();

        assert_eq!(detector.observe(50, MAX_DURATION_MS - 1), None);
    }
}
