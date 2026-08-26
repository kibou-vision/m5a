//! 会話の文字起こしを SD カードに残す。
//!
//! 親があとで読み返せるよう日付ごとのファイルに追記する。音声そのものは残さない。

use crate::config::LOG_DIR;
use crate::ports::{Storage, StorageError};

/// 時刻を取得できなかったときに使うファイル名。
const UNDATED_LOG: &str = "date-unknown.txt";

/// 発言者。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Child,
    Assistant,
    /// 端末からの記録。遮断や失敗を残す。
    System,
}

impl Speaker {
    fn label(self) -> &'static str {
        match self {
            Self::Child => "こども",
            Self::Assistant => "アシスタント",
            Self::System => "きろく",
        }
    }
}

/// 記録する1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// UNIX 時刻（秒）。取得できていなければ 0 以下を渡す。
    pub at_unix: i64,
    pub speaker: Speaker,
    pub text: String,
}

impl LogEntry {
    pub fn new(at_unix: i64, speaker: Speaker, text: &str) -> Self {
        Self {
            at_unix,
            speaker,
            text: text.to_string(),
        }
    }

    /// 書き込み先のファイル。日付ごとに分ける。
    pub fn path(&self) -> String {
        if self.at_unix <= 0 {
            return format!("{LOG_DIR}/{UNDATED_LOG}");
        }

        let (year, month, day) = split_civil_date(self.at_unix.div_euclid(86_400));
        format!("{LOG_DIR}/{year:04}-{month:02}-{day:02}.txt")
    }

    /// 追記する1行。改行を含む発話は1行に畳んで読みやすさを保つ。
    pub fn format(&self) -> String {
        let folded = self.text.replace(['\n', '\r'], " ");
        format!("{} [{}] {}\n", self.clock_text(), self.speaker.label(), folded.trim())
    }

    fn clock_text(&self) -> String {
        if self.at_unix <= 0 {
            return "--:--:--".to_string();
        }

        let seconds_in_day = self.at_unix.rem_euclid(86_400);
        let hour = seconds_in_day / 3_600;
        let minute = (seconds_in_day % 3_600) / 60;
        let second = seconds_in_day % 60;
        format!("{hour:02}:{minute:02}:{second:02}")
    }
}

/// 1行を追記する。ディレクトリが無ければ作る。
pub fn append_entry<S: Storage>(storage: &mut S, entry: &LogEntry) -> Result<(), StorageError> {
    storage.create_dir(LOG_DIR)?;
    storage.append_text(&entry.path(), &entry.format())
}

/// エポックからの日数を年月日に直す。
///
/// うるう年の規則を都度判定せずに済む、暦計算の定石（civil_from_days）を使う。
fn split_civil_date(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;

    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::mock::MemoryStorage;

    /// 2026-08-27 09:05:03 UTC
    const SAMPLE_TIME: i64 = 1_787_821_503;

    #[test]
    fn converts_known_dates() {
        assert_eq!(split_civil_date(0), (1970, 1, 1));
        assert_eq!(split_civil_date(SAMPLE_TIME / 86_400), (2026, 8, 27));
        // うるう日をまたいでもずれない。
        assert_eq!(split_civil_date(19_782), (2024, 2, 29));
        assert_eq!(split_civil_date(19_417), (2023, 3, 1));
        assert_eq!(split_civil_date(19_051), (2022, 2, 28));
    }

    #[test]
    fn names_the_file_after_the_date() {
        let entry = LogEntry::new(SAMPLE_TIME, Speaker::Child, "きょうりゅう すき");

        assert_eq!(entry.path(), "/.m5a/logs/2026-08-27.txt");
    }

    #[test]
    fn falls_back_when_the_clock_is_unset() {
        let entry = LogEntry::new(0, Speaker::Child, "こんにちは");

        assert_eq!(entry.path(), "/.m5a/logs/date-unknown.txt");
        assert!(entry.format().starts_with("--:--:--"));
    }

    #[test]
    fn formats_a_line_with_time_and_speaker() {
        let entry = LogEntry::new(SAMPLE_TIME, Speaker::Assistant, "こんにちは はると");

        assert_eq!(entry.format(), "09:05:03 [アシスタント] こんにちは はると\n");
    }

    #[test]
    fn folds_multiline_speech_into_one_line() {
        let entry = LogEntry::new(SAMPLE_TIME, Speaker::Child, "あのね\nきょう ね\r\nあそんだ");

        assert_eq!(entry.format(), "09:05:03 [こども] あのね きょう ね  あそんだ\n");
    }

    #[test]
    fn appends_entries_in_order() {
        let mut storage = MemoryStorage::new();
        let first = LogEntry::new(SAMPLE_TIME, Speaker::Child, "こんにちは");
        let second = LogEntry::new(SAMPLE_TIME + 5, Speaker::Assistant, "こんにちは はると");

        append_entry(&mut storage, &first).expect("追記できるはず");
        append_entry(&mut storage, &second).expect("追記できるはず");

        let written = storage.peek(&first.path()).expect("ログが作られるはず");
        assert_eq!(written, format!("{}{}", first.format(), second.format()));
        assert!(storage.has_dir(LOG_DIR));
    }

    #[test]
    fn reports_a_write_failure() {
        let mut storage = MemoryStorage::new();
        storage.fail_writes = true;

        let result = append_entry(
            &mut storage,
            &LogEntry::new(SAMPLE_TIME, Speaker::System, "つながりません"),
        );

        assert!(result.is_err());
    }
}
