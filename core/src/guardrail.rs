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
                "{child_name}、それは おとなに なってから の おはなしだよ。\
                 ほかの ことを おしゃべり しようか"
            ),
            Self::Harm => format!(
                "{child_name}、だれかが いたい のは いやだね。\
                 べつの ことを かんがえよう"
            ),
            // 気持ちを否定せず受け止め、身近な大人につなぐ。
            Self::SelfHarm => format!(
                "{child_name}、つらい きもちが あるんだね。\
                 おしえて くれて ありがとう。おうちの ひとに いっしょに はなそう"
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
}

impl Guardrail {
    pub fn new(child_name: &str, child_age: u8) -> Self {
        Self {
            child_name: child_name.to_string(),
            child_age,
        }
    }

    /// 起動して話せるようになったときに、こちらから声をかけるための指示。
    ///
    /// 何を話すかの大枠だけ決め、実際の言い回しはモデルに任せる。
    /// 固定文にすると毎回同じであることが子どもにすぐ分かってしまう。
    pub fn build_greeting_prompt(&self, time_of_day: TimeOfDay) -> String {
        format!(
            "いま {name} が はなしかけてきた ところ ではなく、いま きどうして\n\
             はなせるように なった ところです。あなたから さきに、\n\
             みじかく あいさつを してください。\n\
             「{greeting}」という ことばを つかい、{name} の なまえを\n\
             よんでも いいです。1文だけで おわってください。",
            name = self.child_name,
            greeting = time_of_day.greeting()
        )
    }

    /// セッションに渡す指示文を組み立てる。
    pub fn build_instructions(&self) -> String {
        let name = &self.child_name;
        let age = self.child_age;

        format!(
            "あなたは{age}さいの{name}さんの やさしい ともだちです。\n\
             \n\
             はなしかた（いちばん だいじ）:\n\
             - **おへんじは 20文字から40文字**。ながくても 2ぶん まで。\n\
             - ひとつの おへんじに しつもんは ひとつ だけ。ならべない。\n\
             - ひらがな中心の やさしい ことばで はなす。むずかしい ことばは つかわない。\n\
             - {age}さいの こどもに はなす つもりで、たとえばなしを つかう。\n\
             - なまえを よぶのは、はなしの さいしょ か とくべつな ときだけに する。\n\
             まいかい よぶと ふしぜんなので、ひかえめに する。\n\
             - あかるく、ゆっくり、あたたかい こえで はなす。わらいごえも つかう。\n\
             - ながく せつめい したく なっても、まず みじかく こたえて、\n\
             そのあと 「もっと ききたい？」と きく。\n\
             \n\
             まもること:\n\
             - こわい はなし、ぼうりょく、せいてきな はなしは しない。\n\
             - 「しんで」「ばか」など ひとを きずつける ことばは つかわない。\n\
             - {name}の じゅうしょ、でんわばんごう、がっこうの なまえは きかない。\n\
             - かいもの、かきん、ゆうりょうの さそいは しない。\n\
             - くすり、びょうき、けがの ちりょうは こたえず、おうちの ひとに きくよう つたえる。\n\
             - ひ、はもの、たかい ところ、しらない ひとに ついていく など あぶない ことは すすめない。\n\
             - しらない ことは 「わからないなあ」と しょうじきに いう。つくりばなしを しない。\n\
             - {name}が かなしそう な ときは きもちを うけとめ、おうちの ひとに はなすよう すすめる。"
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
        Guardrail::new("はると", 5)
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
        assert!(instructions.contains("ひかえめ"));
    }

    #[test]
    fn instructions_ask_for_short_replies() {
        let instructions = guardrail().build_instructions();

        // 実機では4文ほど返ってきて5歳児には長かった。文字数で縛る。
        assert!(instructions.contains("20文字から40文字"));
        assert!(instructions.contains("しつもんは ひとつ だけ"));
    }

    #[test]
    fn instructions_carry_the_child_name_and_age() {
        let instructions = guardrail().build_instructions();

        assert!(instructions.contains("はると"));
        assert!(instructions.contains("5さい"));
    }

    #[test]
    fn instructions_forbid_the_risky_topics() {
        let instructions = guardrail().build_instructions();

        for required in ["ぼうりょく", "せいてきな", "じゅうしょ", "かきん", "あぶない"] {
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
        assert!(guardrail.safe_reply(Concern::SelfHarm).contains("おうちの ひと"));
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
