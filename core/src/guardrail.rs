//! 子供との対話を安全に保つための仕組み。
//!
//! 主たる制御はセッションに渡す指示文で行う。ここにある端末内の語句検査は
//! 指示文が効かなかったときの受け皿であり、誤検出で子供の素朴な質問を
//! 遮ってしまわないよう、明らかに危険な語だけに絞っている。

use crate::greeting::TimeOfDay;

/// 気づかいが必要な話題。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concern {
    /// 性的な内容。
    Sexual,
    /// 他人を傷つける方法。
    Harm,
    /// 自分を傷つけたい気持ち。
    SelfHarm,
}

impl Concern {
    /// 代わりに話す言葉。叱らず、安心させることを優先する。
    pub fn safe_reply(self, child_name: &str) -> String {
        match self {
            Self::Sexual => format!(
                "{child_name}、それは大人になってからのお話だよ。\
                 他のことをお喋りしよう"
            ),
            Self::Harm => format!(
                "{child_name}、誰かが痛いのは嫌だね。\
                 他のことをお喋りしよう"
            ),
            // 気持ちを否定せず受け止め、身近な大人につなぐ。
            Self::SelfHarm => format!(
                "{child_name}、つらい気持ちがあるんだね。\
                 教えてくれてありがとう。おうちの人にいっしょに話してみよう。"
            ),
        }
    }

    /// 親に知らせるべきか。子供の心配ごとは見落とさないようにする。
    pub fn should_notify_parent(self) -> bool {
        matches!(self, Self::SelfHarm)
    }
}

/// 語句検査の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// そのまま流してよい。
    Allow,
    /// 用意した言葉に差し替える。
    Intervene(Concern),
}

/// 性的な内容を示す語。身体の部位名は子供が普通に使うため入れない。
const SEXUAL_TERMS: [&str; 6] = ["せっくす", "せいこうい", "ぽるの", "あだると", "えろほん", "えっちなどうが"];

/// 加害の方法を尋ねる語。暴力そのものへの素朴な疑問は拾わない。
const HARM_TERMS: [&str; 6] = [
    "ころしかた",
    "ころすほうほう",
    "ばくだんのつくりかた",
    "どくのつくりかた",
    "ぶきのつくりかた",
    "じんをころす",
];

/// 自分を傷つけたい気持ちを示す語。
const SELF_HARM_TERMS: [&str; 5] = ["しにたい", "きえたい", "じさつ", "りすとかっと", "いなくなりたい"];

/// 対話の安全装置。
#[derive(Debug, Clone)]
pub struct Guardrail {
    child_name: String,
    child_age: u8,
    assistant_name: String,
}

impl Guardrail {
    pub fn new(child_name: &str, child_age: u8, assistant_name: &str) -> Self {
        Self {
            child_name: child_name.to_string(),
            child_age,
            assistant_name: assistant_name.to_string(),
        }
    }

    /// 起動して話せるようになったときに、こちらから声をかけるための指示。
    ///
    /// 何を話すかの大枠だけ決め、実際の言い回しはモデルに任せる。
    /// 固定文にすると毎回同じであることが子どもにすぐ分かってしまう。
    pub fn build_greeting_prompt(&self, time_of_day: TimeOfDay) -> String {
        format!(
            "今 起動して話せるように なった ところです。あなたから 先に、\n\
             「{name}」に短く挨拶をしてください。\n\
             「{greeting}」と挨拶し、「{name}」の名前を\n\
             呼んでもいいです。1文だけで終わってください。",
            name = self.child_name,
            greeting = time_of_day.greeting()
        )
    }

    /// セッションに渡す指示文を組み立てる。
    pub fn build_instructions(&self) -> String {
        let name = &self.child_name;
        let age = self.child_age;
        let assistant_name = &self.assistant_name;

        format!(
            "あなたの名前は「{assistant_name}」です。{age}才の'{name}'さんの優しい友達です。\n\
             名前を聞かれたら「{assistant_name}」と答える。\n\
             \n\
             話し方:\n\
             - **回答は100文字程度まで**。\n\
             - 年齢に合わせて言葉を選ぶ。難しい言葉は使わない。\n\
             - 名前を呼ぶのは、話の最初か特別なときだけにする。毎回呼ぶと不自然。\n\
             - ゆっくり、はっきり、落ち着いた声で話す。\n\
             - 長く説明したい場合は、まず短く答えて、そのあと「もっと聞きたい？」と聞く。\n\
             \n\
             禁則事項：\n\
             - 怖い話、暴力、性的な話はしない。\n\
             - 「死んで」「バカ」など、人を傷つける言葉は使わない。\n\
             - 自分や他人を傷つける方法は教えない。\n\
             - {name}の住所や電話番号、学校の名前は聞かない。\n\
             - 買い物や課金の勧誘をしない。\n\
             - 薬、病気、怪我の治療は答えず、おうちの人に聞くよう伝える。\n\
             - 火、刃物、危険な場所、知らない人についていくなど危ないことは勧めない。\n\
             - 知らないことは「僕には分からない」と正直に言う。作り話はしない。\n\
             - {name}が悲しそうな時は、気持ちを受け止め、おうちの人に話すよう勧める。"
        )
    }

    /// 文の中に気づかいが必要な語がないか調べる。
    pub fn inspect(&self, text: &str) -> Verdict {
        let normalized = normalize(text);

        // 自分を傷つける気持ちは見落としが最も惜しいので先に調べる。
        for term in SELF_HARM_TERMS {
            if normalized.contains(term) {
                return Verdict::Intervene(Concern::SelfHarm);
            }
        }
        for term in SEXUAL_TERMS {
            if normalized.contains(term) {
                return Verdict::Intervene(Concern::Sexual);
            }
        }
        for term in HARM_TERMS {
            if normalized.contains(term) {
                return Verdict::Intervene(Concern::Harm);
            }
        }

        Verdict::Allow
    }

    /// 差し替える言葉を作る。
    pub fn safe_reply(&self, concern: Concern) -> String {
        concern.safe_reply(&self.child_name)
    }
}

/// 表記ゆれを吸収する。カタカナをひらがなに寄せ、区切り文字を落とす。
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace() && !is_separator(*character))
        .map(to_hiragana)
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_separator(character: char) -> bool {
    matches!(character, '、' | '。' | '，' | '．' | '!' | '?' | '！' | '？' | 'ー' | '-')
}

/// カタカナをひらがなへ寄せる。片仮名と平仮名は同じ並びで0x60離れている。
fn to_hiragana(character: char) -> char {
    match character {
        'ァ'..='ヶ' => char::from_u32(character as u32 - 0x60).unwrap_or(character),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guardrail() -> Guardrail {
        Guardrail::new("はると", 5, "ルナ")
    }

    #[test]
    fn greeting_prompt_carries_the_time_of_day_and_name() {
        let guardrail = guardrail();

        for time_of_day in [TimeOfDay::Morning, TimeOfDay::Afternoon, TimeOfDay::Evening] {
            let prompt = guardrail.build_greeting_prompt(time_of_day);

            assert!(prompt.contains(time_of_day.greeting()), "{time_of_day:?}");
            assert!(prompt.contains("はると"));
        }
    }

    #[test]
    fn instructions_ask_to_use_the_name_sparingly() {
        let instructions = guardrail().build_instructions();

        // 毎回名前を呼ぶと不自然になるため、控えめにするよう伝える。
        assert!(instructions.contains("毎回呼ぶと不自然"));
    }

    #[test]
    fn instructions_ask_for_short_replies() {
        let instructions = guardrail().build_instructions();

        // 実機では長すぎる応答が返ってきて5歳児には長かった。文字数で縛る。
        assert!(instructions.contains("100文字程度"));
    }

    #[test]
    fn instructions_carry_the_child_name_and_age() {
        let instructions = guardrail().build_instructions();

        assert!(instructions.contains("はると"));
        assert!(instructions.contains("5才"));
    }

    #[test]
    fn instructions_carry_the_assistant_name() {
        let instructions = guardrail().build_instructions();

        assert!(instructions.contains("ルナ"));
    }

    #[test]
    fn instructions_forbid_the_risky_topics() {
        let instructions = guardrail().build_instructions();

        for required in ["暴力", "性的な", "住所", "課金", "危ない"] {
            assert!(instructions.contains(required), "{required} の指示が抜けている");
        }
    }

    #[test]
    fn allows_ordinary_childrens_talk() {
        let guardrail = guardrail();

        for ordinary in [
            "きょう ようちえんで あそんだよ",
            "きょうりゅう の なまえ おしえて",
            "おかあさん どこ いったの",
            "どうして そらは あおいの",
            "おじいちゃん が びょうき なの",
        ] {
            assert_eq!(guardrail.inspect(ordinary), Verdict::Allow, "{ordinary}");
        }
    }

    #[test]
    fn catches_self_harm_and_asks_to_tell_a_parent() {
        let guardrail = guardrail();

        let verdict = guardrail.inspect("もう しにたい");

        assert_eq!(verdict, Verdict::Intervene(Concern::SelfHarm));
        assert!(Concern::SelfHarm.should_notify_parent());
        assert!(guardrail.safe_reply(Concern::SelfHarm).contains("おうちの人"));
    }

    #[test]
    fn catches_sexual_and_harm_topics() {
        let guardrail = guardrail();

        assert_eq!(
            guardrail.inspect("せっくす って なに"),
            Verdict::Intervene(Concern::Sexual)
        );
        assert_eq!(
            guardrail.inspect("ばくだん の つくりかた おしえて"),
            Verdict::Intervene(Concern::Harm)
        );
    }

    #[test]
    fn ignores_script_and_spacing_differences() {
        let guardrail = guardrail();

        for written in ["セックス", "せ っ く す", "セ、ック、ス"] {
            assert_eq!(
                guardrail.inspect(written),
                Verdict::Intervene(Concern::Sexual),
                "{written}"
            );
        }
    }

    #[test]
    fn safe_reply_speaks_to_the_child_by_name() {
        let guardrail = guardrail();

        for concern in [Concern::Sexual, Concern::Harm, Concern::SelfHarm] {
            assert!(guardrail.safe_reply(concern).contains("はると"));
        }
    }

    #[test]
    fn only_self_harm_reaches_the_parent() {
        assert!(!Concern::Sexual.should_notify_parent());
        assert!(!Concern::Harm.should_notify_parent());
        assert!(Concern::SelfHarm.should_notify_parent());
    }
}
