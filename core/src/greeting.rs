//! 時刻に応じたあいさつ。
//!
//! 起動して話せるようになったとき、こちらから声をかける。
//! 子どもが最初の一言を考えなくてよいようにするため。

/// 一日のうちのいつか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Morning,
    Afternoon,
    Evening,
}

/// 朝と呼ぶ範囲。
const MORNING_STARTS: i64 = 5;
const AFTERNOON_STARTS: i64 = 11;
const EVENING_STARTS: i64 = 18;

impl TimeOfDay {
    /// その土地の時刻から、いまがいつかを決める。
    pub fn at(local_unix: i64) -> Self {
        let hour = local_unix.rem_euclid(86_400) / 3_600;

        if (MORNING_STARTS..AFTERNOON_STARTS).contains(&hour) {
            Self::Morning
        } else if (AFTERNOON_STARTS..EVENING_STARTS).contains(&hour) {
            Self::Afternoon
        } else {
            Self::Evening
        }
    }

    /// かける言葉。
    pub fn greeting(self) -> &'static str {
        match self {
            Self::Morning => "おはよう",
            Self::Afternoon => "こんにちは",
            Self::Evening => "こんばんは",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// その日の指定時刻の UNIX 時刻（その土地の時刻として）。
    fn at_hour(hour: i64) -> i64 {
        1_787_821_503 / 86_400 * 86_400 + hour * 3_600
    }

    #[test]
    fn morning_runs_from_five_to_eleven() {
        for hour in 5..11 {
            assert_eq!(TimeOfDay::at(at_hour(hour)), TimeOfDay::Morning, "{hour}時");
        }
    }

    #[test]
    fn afternoon_runs_from_eleven_to_six() {
        for hour in 11..18 {
            assert_eq!(TimeOfDay::at(at_hour(hour)), TimeOfDay::Afternoon, "{hour}時");
        }
    }

    #[test]
    fn evening_covers_the_night_and_early_hours() {
        for hour in (18..24).chain(0..5) {
            assert_eq!(TimeOfDay::at(at_hour(hour)), TimeOfDay::Evening, "{hour}時");
        }
    }

    #[test]
    fn every_time_of_day_has_a_greeting() {
        assert_eq!(TimeOfDay::Morning.greeting(), "おはよう");
        assert_eq!(TimeOfDay::Afternoon.greeting(), "こんにちは");
        assert_eq!(TimeOfDay::Evening.greeting(), "こんばんは");
    }
}
