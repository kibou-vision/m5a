//! SDカード上の設定ファイルの生成・読み込み・検証。
//!
//! 親がPCでテキストエディタから編集する運用のため、書式の誤りや未記入は
//! 「何をどう直せばよいか」を伴うエラーとして返す。

use serde::{Deserialize, Serialize};

use crate::ports::{Storage, StorageError};

/// 設定一式を置くディレクトリ。SDカードのマウント点は含まない。
pub const CONFIG_DIR: &str = "/.m5a";
/// 設定ファイルの経路。
pub const CONFIG_PATH: &str = "/.m5a/config.toml";
/// 会話ログを置くディレクトリ。
pub const LOG_DIR: &str = "/.m5a/logs";

/// 未編集を判定するための、テンプレートに書かれた見本の値。
const NAME_EXAMPLE: &str = "なまえをここに";
const SSID_EXAMPLE: &str = "WiFiのSSIDをここに";
const PASSWORD_EXAMPLE: &str = "WiFiのパスワードをここに";
const API_KEY_EXAMPLE: &str = "sk-ここにAPIキーを";

/// 既定のアシスタント名。名前を書かなくても自己紹介できるようにする。
const DEFAULT_ASSISTANT_NAME: &str = "アシスタント";
/// 既定のモデル。`gpt-realtime-mini` は2027-01-20に廃止されるため使わない。
const DEFAULT_MODEL: &str = "gpt-realtime-2.1-mini";
/// 既定の声。OpenAI が marin と cedar を推奨している。
const DEFAULT_VOICE: &str = "marin";

/// Realtime API が受け付ける声の一覧。
pub const SUPPORTED_VOICES: [&str; 10] = [
    "alloy", "ash", "ballad", "coral", "echo", "sage", "shimmer", "verse", "marin", "cedar",
];

/// 親が編集する設定ファイルの雛形。
pub const CONFIG_TEMPLATE: &str = r#"# m5a 設定ファイル
# "" の 中を変更して保存してください。
# 変更後、SDカードを M5Stack に戻して電源を入れます。

[child]
# 利用者の名前。アシスタントがこの名前で呼びかけます。
name = ""
# 年齢。話し方の難しさの目安に使います。
age = 5

[assistant]
# アシスタント自身の名前。呼ばれたらこの名前で応えます。
name = "アシスタント"

[wifi]
# 接続する WiFi の名前 (SSID) とパスワード。
# 5GHz の WiFi は使えません。2.4GHz を選んでください。
ssid = ""
password = ""

[openai]
# OpenAI の APIキー。https://platform.openai.com/api-keys で 作成できます。
# このキーは料金がかかります。
# 専用のプロジェクトを作成し、上限を設定することを推奨します。
api_key = ""

# 使用するモデル
model = "gpt-realtime-2.1-mini"

# 声の種類。
# alloy / ash / ballad / coral / echo / sage / shimmer / verse / marin / cedar
voice = "marin"

# 音声の形式。
#   ulaw  = 低い音質。通信量が少なく、安定します。
#   pcm16 = 高い音質。通信が重く、とぎれることがあります。
audio_format = "ulaw"

[search]
# アシスタントが分からないことをインターネットで調べられるようにする
# ための設定。無効にすれば、調べる機能は使えません。
#
# Tavily (https://www.tavily.com/) で無料のアカウントを作成できます
api_key = ""
"#;

/// 音声のやりとりに使う形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    /// G.711 μ-law 8kHz。片方向約85kbpsで、ESP32-S3の帯域に収まる。
    #[default]
    Ulaw,
    /// PCM16 24kHz。片方向約512kbpsと重い。
    Pcm16,
}

impl AudioFormat {
    /// Realtime API の `session.audio.*.format` に入れる型名。
    pub fn wire_type(self) -> &'static str {
        match self {
            Self::Ulaw => "audio/pcmu",
            Self::Pcm16 => "audio/pcm",
        }
    }

    /// この形式で送受信する標本化周波数。
    pub fn sample_rate(self) -> u32 {
        match self {
            Self::Ulaw => 8_000,
            Self::Pcm16 => 24_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildConfig {
    pub name: String,
    #[serde(default = "default_age")]
    pub age: u8,
}

/// アシスタント自身についての設定。未記入なら既定の名前を使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantConfig {
    #[serde(default = "default_assistant_name")]
    pub name: String,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        Self {
            name: default_assistant_name(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default)]
    pub audio_format: AudioFormat,
}

/// web検索の設定。記入は任意で、無ければ検索の機能そのものを使わない。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchConfig {
    #[serde(default)]
    pub api_key: Option<String>,
}

impl SearchConfig {
    /// 検索を有効にするAPIキー。空欄・未記入なら `None`。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub child: ChildConfig,
    #[serde(default)]
    pub assistant: AssistantConfig,
    pub wifi: WifiConfig,
    pub openai: OpenAiConfig,
    #[serde(default)]
    pub search: SearchConfig,
}

fn default_age() -> u8 {
    5
}

fn default_assistant_name() -> String {
    DEFAULT_ASSISTANT_NAME.to_string()
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

fn default_voice() -> String {
    DEFAULT_VOICE.to_string()
}

/// 設定内容の不備。いずれも親がファイルを直せば回復する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigProblem {
    ChildNameUnwritten,
    WifiSsidUnwritten,
    WifiPasswordUnwritten,
    ApiKeyUnwritten,
    UnsupportedVoice(String),
}

impl ConfigProblem {
    /// 何が問題かを親に示す文。
    pub fn describe(&self) -> String {
        match self {
            Self::ChildNameUnwritten => "こどもの名前が書かれていません".to_string(),
            Self::WifiSsidUnwritten => "WiFiのSSIDが書かれていません".to_string(),
            Self::WifiPasswordUnwritten => "WiFiのパスワードが書かれていません".to_string(),
            Self::ApiKeyUnwritten => "OpenAIのAPIキーが書かれていません".to_string(),
            Self::UnsupportedVoice(voice) => {
                format!("voice の \"{voice}\" は使えません")
            }
        }
    }

    /// どう直せばよいかを示す文。
    pub fn remedy(&self) -> String {
        match self {
            Self::ChildNameUnwritten => {
                format!("{CONFIG_PATH} の [child] name にお子さんの名前を書いてください")
            }
            Self::WifiSsidUnwritten => {
                format!("{CONFIG_PATH} の [wifi] ssid に接続先のWiFi名を書いてください")
            }
            Self::WifiPasswordUnwritten => {
                format!("{CONFIG_PATH} の [wifi] password にWiFiのパスワードを書いてください")
            }
            Self::ApiKeyUnwritten => format!(
                "{CONFIG_PATH} の [openai] api_key にAPIキーを書いてください。\
                 https://platform.openai.com/api-keys で作れます"
            ),
            Self::UnsupportedVoice(_) => format!(
                "{CONFIG_PATH} の [openai] voice を次のいずれかにしてください: {}",
                SUPPORTED_VOICES.join(" / ")
            ),
        }
    }
}

/// 設定を読めなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// 設定ファイルが無かったので雛形を作った。親の記入待ち。
    TemplateCreated,
    /// 雛形のまま、または空欄が残っている。
    Unwritten(Vec<ConfigProblem>),
    /// TOML として解釈できない。
    Malformed { detail: String },
    /// SDカードを読み書きできない。
    Unreadable(StorageError),
    /// 書き込みは成功したのに内容が残らない。カードの構造が壊れている。
    Unwritable,
}

impl ConfigError {
    /// 何が起きたかを親に示す文。
    pub fn describe(&self) -> String {
        match self {
            Self::TemplateCreated => "せっていファイルを作りました".to_string(),
            Self::Unwritten(problems) => problems
                .iter()
                .map(ConfigProblem::describe)
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Malformed { .. } => "せっていファイルの書き方に誤りがあります".to_string(),
            Self::Unreadable(error) => format!("SDカードを読めません: {error}"),
            Self::Unwritable => "SDカードにせっていファイルを残せません".to_string(),
        }
    }

    /// どう直せばよいかを示す文。
    pub fn remedy(&self) -> String {
        match self {
            Self::TemplateCreated => format!(
                "SDカードをパソコンに挿し、{CONFIG_PATH} をテキストエディタで開いて\
                 名前・WiFi・APIキーを記入してください"
            ),
            Self::Unwritten(problems) => problems
                .iter()
                .map(ConfigProblem::remedy)
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Malformed { detail } => {
                format!("{CONFIG_PATH} を開いて次の箇所を直してください: {detail}")
            }
            Self::Unreadable(_) => {
                "SDカードが入っているか、書き込み禁止になっていないか確かめてください".to_string()
            }
            Self::Unwritable => {
                "SDカードをパソコンで FAT32 に初期化しなおしてから、もう一度電源を入れてください"
                    .to_string()
            }
        }
    }
}

impl Config {
    /// 記入漏れと不正値を洗い出す。
    pub fn validate(&self) -> Result<(), Vec<ConfigProblem>> {
        let mut problems = Vec::new();

        if is_unwritten(&self.child.name, NAME_EXAMPLE) {
            problems.push(ConfigProblem::ChildNameUnwritten);
        }
        if is_unwritten(&self.wifi.ssid, SSID_EXAMPLE) {
            problems.push(ConfigProblem::WifiSsidUnwritten);
        }
        if is_unwritten(&self.wifi.password, PASSWORD_EXAMPLE) {
            problems.push(ConfigProblem::WifiPasswordUnwritten);
        }
        if is_unwritten(&self.openai.api_key, API_KEY_EXAMPLE) {
            problems.push(ConfigProblem::ApiKeyUnwritten);
        }
        if !SUPPORTED_VOICES.contains(&self.openai.voice.as_str()) {
            problems.push(ConfigProblem::UnsupportedVoice(self.openai.voice.clone()));
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// 空欄のまま、または雛形の見本のまま残っているか。
fn is_unwritten(value: &str, example: &str) -> bool {
    value.trim().is_empty() || value.trim() == example
}

/// 設定を読み込む。無ければ雛形を作って [`ConfigError::TemplateCreated`] を返す。
pub fn load_config<S: Storage>(storage: &mut S) -> Result<Config, ConfigError> {
    if !storage.exists(CONFIG_PATH) {
        create_template(storage)?;
        return Err(ConfigError::TemplateCreated);
    }

    let source = storage
        .read_text(CONFIG_PATH)
        .map_err(ConfigError::Unreadable)?;

    parse_config(&source)
}

/// TOML 文字列を設定として解釈し、記入漏れも検査する。
pub fn parse_config(source: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(source).map_err(|error| ConfigError::Malformed {
        detail: error.message().to_string(),
    })?;

    config.validate().map_err(ConfigError::Unwritten)?;

    Ok(config)
}

/// 設定ディレクトリと雛形ファイルを作る。
fn create_template<S: Storage>(storage: &mut S) -> Result<(), ConfigError> {
    storage
        .create_dir(CONFIG_DIR)
        .map_err(ConfigError::Unreadable)?;
    storage
        .write_text(CONFIG_PATH, CONFIG_TEMPLATE)
        .map_err(ConfigError::Unreadable)?;

    // 書き込みが成功を返しても内容が残らないことがある。ディレクトリ項目が
    // 重複したカードで実際に起きたため、読み戻して残ったことを確かめる。
    let stored = storage
        .read_text(CONFIG_PATH)
        .map_err(|_| ConfigError::Unwritable)?;
    if stored != CONFIG_TEMPLATE {
        return Err(ConfigError::Unwritable);
    }

    Ok(())
}

/// 声の種類を `config.toml` に書き戻す。
///
/// 親が書いたコメントやレイアウトを壊さないよう、TOML全体を作り直すのではなく
/// `[openai]` セクション内の `voice = "..."` 行だけを置き換える。
pub fn save_voice<S: Storage>(storage: &mut S, voice: &str) -> Result<(), ConfigError> {
    let source = storage
        .read_text(CONFIG_PATH)
        .map_err(ConfigError::Unreadable)?;

    let updated = replace_openai_voice_line(&source, voice).ok_or_else(|| ConfigError::Malformed {
        detail: "[openai] の voice の行が見つかりません".to_string(),
    })?;

    storage
        .write_text(CONFIG_PATH, &updated)
        .map_err(ConfigError::Unreadable)?;

    let stored = storage
        .read_text(CONFIG_PATH)
        .map_err(|_| ConfigError::Unwritable)?;
    if stored != updated {
        return Err(ConfigError::Unwritable);
    }

    Ok(())
}

/// `[openai]` セクション内の `voice = "..."` 行だけを置き換える。
/// 他の項目・コメント・空行はそのまま残す。
fn replace_openai_voice_line(source: &str, voice: &str) -> Option<String> {
    let mut in_openai_section = false;
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_openai_section = trimmed == "[openai]";
        } else if in_openai_section && trimmed.starts_with("voice") {
            let after_key = trimmed["voice".len()..].trim_start();
            if let Some(rest) = after_key.strip_prefix('=') {
                let _ = rest; // 値そのものは使わず、行全体を作り直す。
                lines.push(format!("voice = \"{voice}\""));
                replaced = true;
                continue;
            }
        }

        lines.push(line.to_string());
    }

    if !replaced {
        return None;
    }

    let mut result = lines.join("\n");
    if source.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::mock::MemoryStorage;

    /// 記入済みの設定として通る最小の内容。
    ///
    /// テンプレートの空欄はどれも `""` で見分けが付かないため、
    /// 各項目の鍵ごと1件だけ埋める。
    fn filled_source() -> String {
        CONFIG_TEMPLATE
            .replacen("name = \"\"", "name = \"はると\"", 1)
            .replacen("ssid = \"\"", "ssid = \"home-wifi\"", 1)
            .replacen("password = \"\"", "password = \"pass1234\"", 1)
            .replacen("api_key = \"\"", "api_key = \"sk-proj-abcdef\"", 1)
    }

    #[test]
    fn creates_template_when_config_is_absent() {
        let mut storage = MemoryStorage::new();

        let result = load_config(&mut storage);

        assert_eq!(result, Err(ConfigError::TemplateCreated));
        assert_eq!(storage.peek(CONFIG_PATH), Some(CONFIG_TEMPLATE));
        assert!(storage.has_dir(CONFIG_DIR));
    }

    #[test]
    fn detects_a_write_that_did_not_stick() {
        let mut storage = MemoryStorage::new();
        storage.discard_writes = true;

        let result = load_config(&mut storage);

        assert_eq!(result, Err(ConfigError::Unwritable));
        let error = result.unwrap_err();
        assert!(error.remedy().contains("FAT32"));
    }

    #[test]
    fn reports_unreadable_card_instead_of_creating_template() {
        let mut storage = MemoryStorage::new();
        storage.fail_writes = true;

        let result = load_config(&mut storage);

        assert!(matches!(result, Err(ConfigError::Unreadable(_))));
    }

    #[test]
    fn template_alone_is_rejected_as_unwritten() {
        let error = parse_config(CONFIG_TEMPLATE).unwrap_err();

        let ConfigError::Unwritten(problems) = error else {
            panic!("雛形のままなら Unwritten になるはず: {error:?}");
        };
        assert!(problems.contains(&ConfigProblem::ChildNameUnwritten));
        assert!(problems.contains(&ConfigProblem::WifiSsidUnwritten));
        assert!(problems.contains(&ConfigProblem::ApiKeyUnwritten));
    }

    #[test]
    fn loads_filled_config() {
        let mut storage = MemoryStorage::with_file(CONFIG_PATH, &filled_source());

        let config = load_config(&mut storage).expect("記入済みなら読めるはず");

        assert_eq!(config.child.name, "はると");
        assert_eq!(config.child.age, 5);
        assert_eq!(config.wifi.ssid, "home-wifi");
        assert_eq!(config.openai.api_key, "sk-proj-abcdef");
        assert_eq!(config.openai.model, DEFAULT_MODEL);
        assert_eq!(config.openai.voice, DEFAULT_VOICE);
        assert_eq!(config.openai.audio_format, AudioFormat::Ulaw);
    }

    #[test]
    fn assistant_name_defaults_when_absent() {
        let source = filled_source().replace(
            "[assistant]\n\
             # アシスタント自身の名前。呼ばれたらこの名前で応えます。\n\
             name = \"アシスタント\"\n\n",
            "",
        );

        let config = parse_config(&source).expect("[assistant] を省略しても読めるはず");

        assert_eq!(config.assistant.name, "アシスタント");
    }

    #[test]
    fn assistant_name_can_be_customized() {
        let source = filled_source().replace("name = \"アシスタント\"", "name = \"ルナ\"");

        let config = parse_config(&source).expect("記入済みなら読めるはず");

        assert_eq!(config.assistant.name, "ルナ");
    }

    #[test]
    fn applies_defaults_when_optional_keys_are_absent() {
        let source = r#"
            [child]
            name = "みなと"
            [wifi]
            ssid = "home"
            password = "pass"
            [openai]
            api_key = "sk-proj-1"
        "#;

        let config = parse_config(source).expect("任意項目は省略できるはず");

        assert_eq!(config.child.age, 5);
        assert_eq!(config.openai.model, DEFAULT_MODEL);
        assert_eq!(config.openai.voice, DEFAULT_VOICE);
        assert_eq!(config.openai.audio_format, AudioFormat::Ulaw);
    }

    #[test]
    fn rejects_broken_toml_with_detail() {
        let error = parse_config("[child\nname = \"a\"").unwrap_err();

        let ConfigError::Malformed { detail } = error else {
            panic!("壊れたTOMLなら Malformed になるはず: {error:?}");
        };
        assert!(!detail.is_empty());
    }

    #[test]
    fn rejects_unsupported_voice() {
        let source = filled_source().replace("voice = \"marin\"", "voice = \"ドラえもん\"");

        let error = parse_config(&source).unwrap_err();

        let ConfigError::Unwritten(problems) = error else {
            panic!("未対応の声なら Unwritten になるはず: {error:?}");
        };
        assert_eq!(
            problems,
            vec![ConfigProblem::UnsupportedVoice("ドラえもん".to_string())]
        );
    }

    #[test]
    fn search_api_key_is_absent_when_left_blank() {
        let config = load_config(&mut MemoryStorage::with_file(CONFIG_PATH, &filled_source()))
            .expect("記入済みなら読めるはず");

        assert_eq!(config.search.api_key(), None);
    }

    #[test]
    fn search_section_can_be_omitted_entirely() {
        let source = filled_source().replace(
            "[search]\n\
             # アシスタントが分からないことをインターネットで調べられるようにする\n\
             # ための設定。無効にすれば、調べる機能は使えません。\n\
             #\n\
             # Tavily (https://www.tavily.com/) で無料のアカウントを作成できます\n\
             api_key = \"\"\n",
            "",
        );

        let config = parse_config(&source).expect("[search] を省略しても読めるはず");

        assert_eq!(config.search.api_key(), None);
    }

    #[test]
    fn search_api_key_is_read_when_filled_in() {
        let source = filled_source().replace("api_key = \"\"\n", "api_key = \"tvly-secret\"\n");

        let config = parse_config(&source).expect("記入済みなら読めるはず");

        assert_eq!(config.search.api_key(), Some("tvly-secret"));
    }

    #[test]
    fn parses_pcm16_audio_format() {
        let source = filled_source().replace("audio_format = \"ulaw\"", "audio_format = \"pcm16\"");

        let config = parse_config(&source).expect("pcm16 も選べるはず");

        assert_eq!(config.openai.audio_format, AudioFormat::Pcm16);
        assert_eq!(config.openai.audio_format.wire_type(), "audio/pcm");
        assert_eq!(config.openai.audio_format.sample_rate(), 24_000);
    }

    #[test]
    fn save_voice_replaces_only_the_voice_line() {
        let mut storage = MemoryStorage::with_file(CONFIG_PATH, &filled_source());

        save_voice(&mut storage, "cedar").expect("voice を保存できるはず");

        let stored = storage.peek(CONFIG_PATH).expect("書き戻されているはず");
        let config = parse_config(stored).expect("書き戻した内容も読めるはず");
        assert_eq!(config.openai.voice, "cedar");
        // 他の項目やコメントは残っている。
        assert!(stored.contains("[openai]"));
        assert!(stored.contains("使用するモデル"));
        assert_eq!(config.child.name, "はると");
    }

    #[test]
    fn save_voice_fails_when_voice_line_is_missing() {
        let mut storage = MemoryStorage::with_file(CONFIG_PATH, "[child]\nname = \"はると\"\n");

        let result = save_voice(&mut storage, "cedar");

        assert!(matches!(result, Err(ConfigError::Malformed { .. })));
    }

    #[test]
    fn every_problem_offers_a_remedy() {
        let problems = [
            ConfigProblem::ChildNameUnwritten,
            ConfigProblem::WifiSsidUnwritten,
            ConfigProblem::WifiPasswordUnwritten,
            ConfigProblem::ApiKeyUnwritten,
            ConfigProblem::UnsupportedVoice("x".to_string()),
        ];

        for problem in problems {
            assert!(!problem.describe().is_empty());
            assert!(problem.remedy().contains(CONFIG_PATH));
        }
    }
}
