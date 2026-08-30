//! OpenAI Realtime API とやりとりする電文の組み立てと解析。
//!
//! 通信そのものはハードウェア層が担い、ここでは文字列の出し入れだけを扱う。
//! 押している間だけ録音する方式に合わせ、サーバ側の発話区切り検出は止めて
//! 録音の確定と応答の要求を自分で送る。

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::config::AudioFormat;

/// 接続先。GA 版のため `OpenAI-Beta` ヘッダは付けない。
const ENDPOINT: &str = "wss://api.openai.com/v1/realtime";
/// 子供の発話を文字に起こすモデル。会話ログに使う。
const TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";
/// 応答の待ち時間を抑えるため、推論の深さは最小にする。
const REASONING_EFFORT: &str = "minimal";

/// セッションに与える設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSetup {
    pub model: String,
    pub voice: String,
    pub audio_format: AudioFormat,
    pub instructions: String,
    /// モデルが呼び出せる function tool。空なら何も渡さない。
    pub tools: Vec<Value>,
}

/// サーバから届いた出来事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    /// 接続できた。ここで設定を送る。
    SessionCreated,
    /// こちらの設定が反映された。ここから話しかけられる。
    SessionConfigured,
    /// 応答音声の断片。復号済みの生バイト。
    AudioDelta(Vec<u8>),
    /// アシスタントの発話の文字起こし（断片）。
    AssistantSaid(String),
    /// 子供の発話の文字起こし（確定）。
    ChildSaid(String),
    /// 応答が終わった。
    ResponseFinished,
    /// モデルが function tool の呼び出しを求めた。
    ToolCallRequested {
        call_id: String,
        name: String,
        arguments: String,
    },
    /// サーバがエラーを報告した。多くは回復可能なのでセッションは維持する。
    Reported { code: Option<String>, message: String },
    /// この端末では使わない出来事。
    Ignored,
}

/// 電文を解釈できなかった。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub detail: String,
}

/// 接続先の URL を組み立てる。
pub fn build_endpoint_url(model: &str) -> String {
    format!("{ENDPOINT}?model={model}")
}

/// WebSocket に付ける追加ヘッダ。各行を CRLF で終端する規約に従う。
pub fn build_auth_header(api_key: &str) -> String {
    format!("Authorization: Bearer {api_key}\r\n")
}

/// セッションの設定を送る電文。
pub fn build_session_update(setup: &SessionSetup) -> String {
    let format = describe_audio_format(setup.audio_format);

    let mut session = json!({
        "type": "realtime",
        "model": setup.model,
        "instructions": setup.instructions,
        "output_modalities": ["audio"],
        "audio": {
            "input": {
                "format": format,
                "transcription": {
                    "model": TRANSCRIPTION_MODEL,
                    "language": "ja",
                },
                "noise_reduction": { "type": "near_field" },
                // 押している間だけ録音するので、サーバ側の発話区切り検出は使わない。
                "turn_detection": Value::Null,
            },
            "output": {
                "format": format,
                "voice": setup.voice,
            },
        },
        "reasoning": { "effort": REASONING_EFFORT },
    });

    // ツールを渡さない子機では tools 自体を省き、モデルに存在しない機能を
    // 匂わせない。
    if !setup.tools.is_empty() {
        session["tools"] = Value::Array(setup.tools.clone());
        session["tool_choice"] = json!("auto");
    }

    json!({
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

/// 録音した音声を送る電文。
pub fn build_audio_append(encoded_audio: &[u8]) -> String {
    json!({
        "type": "input_audio_buffer.append",
        "audio": BASE64.encode(encoded_audio),
    })
    .to_string()
}

/// 録音を確定する電文。
pub fn build_audio_commit() -> String {
    json!({ "type": "input_audio_buffer.commit" }).to_string()
}

/// 録音を捨てる電文。
pub fn build_audio_clear() -> String {
    json!({ "type": "input_audio_buffer.clear" }).to_string()
}

/// 応答を要求する電文。
pub fn build_response_create() -> String {
    json!({ "type": "response.create" }).to_string()
}

/// このやり取り限りの指示を添えて応答を求める電文。
///
/// セッションの指示文は変えずに、起動時のあいさつのように
/// 一度だけ違う振る舞いをさせたいときに使う。
pub fn build_response_create_with_instructions(instructions: &str) -> String {
    json!({
        "type": "response.create",
        "response": { "instructions": instructions },
    })
    .to_string()
}

/// 生成中の応答を打ち切る電文。
pub fn build_response_cancel() -> String {
    json!({ "type": "response.cancel" }).to_string()
}

/// function tool の実行結果をモデルへ返す電文。
pub fn build_function_call_output(call_id: &str, output: &str) -> String {
    json!({
        "type": "conversation.item.create",
        "item": {
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        },
    })
    .to_string()
}

/// サーバからの電文を解釈する。知らない種類は [`ServerEvent::Ignored`] にする。
pub fn parse_server_event(payload: &str) -> Result<ServerEvent, ProtocolError> {
    let value: Value = serde_json::from_str(payload).map_err(|error| ProtocolError {
        detail: error.to_string(),
    })?;

    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return Err(ProtocolError {
            detail: "type が無い".to_string(),
        });
    };

    let event = match event_type {
        // 接続直後に session.created が届く。設定を送るのはこれを見てから。
        "session.created" => ServerEvent::SessionCreated,
        "session.updated" => ServerEvent::SessionConfigured,

        "response.output_audio.delta" => match take_base64(&value, "delta") {
            Some(audio) => ServerEvent::AudioDelta(audio),
            None => ServerEvent::Ignored,
        },

        "response.output_audio_transcript.delta" => match take_text(&value, "delta") {
            Some(text) => ServerEvent::AssistantSaid(text),
            None => ServerEvent::Ignored,
        },

        "conversation.item.input_audio_transcription.completed" => {
            match take_text(&value, "transcript") {
                Some(text) => ServerEvent::ChildSaid(text),
                None => ServerEvent::Ignored,
            }
        }

        // モデルが function tool の呼び出しを求めるときも response.done で届く。
        // このアプリでは検索の1種類しかツールを渡さないため、最初の1件だけ扱う。
        "response.done" => match take_tool_call(&value) {
            Some(event) => event,
            None => ServerEvent::ResponseFinished,
        },

        "error" => ServerEvent::Reported {
            code: value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .map(str::to_string),
            message: value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("原因不明")
                .to_string(),
        },

        _ => ServerEvent::Ignored,
    };

    Ok(event)
}

fn describe_audio_format(format: AudioFormat) -> Value {
    match format {
        AudioFormat::Ulaw => json!({ "type": "audio/pcmu" }),
        AudioFormat::Pcm16 => json!({ "type": "audio/pcm", "rate": 24_000 }),
    }
}

fn take_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn take_base64(value: &Value, key: &str) -> Option<Vec<u8>> {
    let encoded = value.get(key).and_then(Value::as_str)?;
    BASE64.decode(encoded).ok().filter(|bytes| !bytes.is_empty())
}

/// `response.done` の `response.output` から最初の function_call を取り出す。
fn take_tool_call(value: &Value) -> Option<ServerEvent> {
    let output = value.pointer("/response/output")?.as_array()?;

    let call = output
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))?;

    Some(ServerEvent::ToolCallRequested {
        call_id: call.get("call_id").and_then(Value::as_str)?.to_string(),
        name: call.get("name").and_then(Value::as_str)?.to_string(),
        arguments: call
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> SessionSetup {
        SessionSetup {
            model: "gpt-realtime-2.1-mini".to_string(),
            voice: "marin".to_string(),
            audio_format: AudioFormat::Ulaw,
            instructions: "やさしく はなしてね".to_string(),
            tools: Vec::new(),
        }
    }

    fn parsed(json_text: &str) -> Value {
        serde_json::from_str(json_text).expect("組み立てた電文は JSON のはず")
    }

    #[test]
    fn endpoint_carries_the_model_as_a_query() {
        assert_eq!(
            build_endpoint_url("gpt-realtime-2.1-mini"),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime-2.1-mini"
        );
    }

    #[test]
    fn auth_header_is_crlf_terminated() {
        let header = build_auth_header("sk-proj-secret");

        assert_eq!(header, "Authorization: Bearer sk-proj-secret\r\n");
    }

    #[test]
    fn session_update_disables_server_side_turn_detection() {
        let value = parsed(&build_session_update(&setup()));

        assert!(value
            .pointer("/session/audio/input/turn_detection")
            .expect("turn_detection を送るはず")
            .is_null());
    }

    #[test]
    fn session_update_uses_the_ga_shapes() {
        let value = parsed(&build_session_update(&setup()));

        assert_eq!(value["type"], "session.update");
        assert_eq!(value["session"]["type"], "realtime");
        assert_eq!(value["session"]["model"], "gpt-realtime-2.1-mini");
        assert_eq!(value["session"]["instructions"], "やさしく はなしてね");
        assert_eq!(value["session"]["output_modalities"][0], "audio");
        assert_eq!(value["session"]["audio"]["output"]["voice"], "marin");
    }

    #[test]
    fn session_update_selects_the_configured_audio_format() {
        let ulaw = parsed(&build_session_update(&setup()));
        assert_eq!(ulaw["session"]["audio"]["input"]["format"]["type"], "audio/pcmu");
        assert_eq!(ulaw["session"]["audio"]["output"]["format"]["type"], "audio/pcmu");

        let pcm = parsed(&build_session_update(&SessionSetup {
            audio_format: AudioFormat::Pcm16,
            ..setup()
        }));
        assert_eq!(pcm["session"]["audio"]["input"]["format"]["type"], "audio/pcm");
        assert_eq!(pcm["session"]["audio"]["input"]["format"]["rate"], 24_000);
    }

    #[test]
    fn session_update_asks_for_japanese_transcription() {
        let value = parsed(&build_session_update(&setup()));

        assert_eq!(
            value["session"]["audio"]["input"]["transcription"]["language"],
            "ja"
        );
    }

    #[test]
    fn session_update_omits_tools_when_none_are_given() {
        let value = parsed(&build_session_update(&setup()));

        assert!(value.pointer("/session/tools").is_none());
        assert!(value.pointer("/session/tool_choice").is_none());
    }

    #[test]
    fn session_update_carries_the_given_tools() {
        let value = parsed(&build_session_update(&SessionSetup {
            tools: vec![crate::search::tool_definition()],
            ..setup()
        }));

        assert_eq!(value["session"]["tool_choice"], "auto");
        assert_eq!(
            value["session"]["tools"][0]["name"],
            crate::search::TOOL_NAME
        );
    }

    #[test]
    fn audio_append_base64_encodes_the_payload() {
        let value = parsed(&build_audio_append(&[0xFF, 0x00, 0x7F]));

        assert_eq!(value["type"], "input_audio_buffer.append");
        assert_eq!(value["audio"], BASE64.encode([0xFF, 0x00, 0x7F]));
    }

    #[test]
    fn response_create_can_carry_one_off_instructions() {
        let value = parsed(&build_response_create_with_instructions("あさの あいさつをして"));

        assert_eq!(value["type"], "response.create");
        assert_eq!(value["response"]["instructions"], "あさの あいさつをして");
    }

    #[test]
    fn control_messages_carry_only_their_type() {
        let cases = [
            (build_audio_commit(), "input_audio_buffer.commit"),
            (build_audio_clear(), "input_audio_buffer.clear"),
            (build_response_create(), "response.create"),
            (build_response_cancel(), "response.cancel"),
        ];

        for (message, expected_type) in cases {
            assert_eq!(parsed(&message)["type"], expected_type);
        }
    }

    #[test]
    fn reads_audio_deltas_under_the_ga_event_name() {
        let payload = json!({
            "type": "response.output_audio.delta",
            "delta": BASE64.encode([1_u8, 2, 3]),
        })
        .to_string();

        assert_eq!(
            parse_server_event(&payload),
            Ok(ServerEvent::AudioDelta(vec![1, 2, 3]))
        );
    }

    #[test]
    fn reads_both_sides_of_the_transcript() {
        let assistant = json!({
            "type": "response.output_audio_transcript.delta",
            "delta": "こんにちは",
        })
        .to_string();
        let child = json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "transcript": "きょうりゅう すき",
        })
        .to_string();

        assert_eq!(
            parse_server_event(&assistant),
            Ok(ServerEvent::AssistantSaid("こんにちは".to_string()))
        );
        assert_eq!(
            parse_server_event(&child),
            Ok(ServerEvent::ChildSaid("きょうりゅう すき".to_string()))
        );
    }

    #[test]
    fn reads_session_and_response_lifecycle() {
        for (payload, expected) in [
            (json!({"type": "session.created"}), ServerEvent::SessionCreated),
            (json!({"type": "session.updated"}), ServerEvent::SessionConfigured),
            (json!({"type": "response.done"}), ServerEvent::ResponseFinished),
        ] {
            assert_eq!(parse_server_event(&payload.to_string()), Ok(expected));
        }
    }

    #[test]
    fn separates_connecting_from_being_configured() {
        // 接続直後の session.created で設定を送り、session.updated で話し始める。
        // ひとまとめにすると、設定が届く前に話しかけてしまう。
        assert_ne!(
            parse_server_event(&json!({"type": "session.created"}).to_string()),
            parse_server_event(&json!({"type": "session.updated"}).to_string())
        );
    }

    #[test]
    fn reads_reported_errors_with_their_code() {
        let payload = json!({
            "type": "error",
            "error": { "code": "invalid_event", "message": "type が足りない" },
        })
        .to_string();

        assert_eq!(
            parse_server_event(&payload),
            Ok(ServerEvent::Reported {
                code: Some("invalid_event".to_string()),
                message: "type が足りない".to_string(),
            })
        );
    }

    #[test]
    fn reads_a_tool_call_from_response_done() {
        let payload = json!({
            "type": "response.done",
            "response": {
                "output": [{
                    "type": "function_call",
                    "call_id": "call_123",
                    "name": "search_web",
                    "arguments": "{\"query\":\"きょうりゅう\"}",
                }],
            },
        })
        .to_string();

        assert_eq!(
            parse_server_event(&payload),
            Ok(ServerEvent::ToolCallRequested {
                call_id: "call_123".to_string(),
                name: "search_web".to_string(),
                arguments: "{\"query\":\"きょうりゅう\"}".to_string(),
            })
        );
    }

    #[test]
    fn response_done_without_a_tool_call_still_finishes_the_response() {
        let payload = json!({
            "type": "response.done",
            "response": { "output": [{ "type": "message" }] },
        })
        .to_string();

        assert_eq!(
            parse_server_event(&payload),
            Ok(ServerEvent::ResponseFinished)
        );
    }

    #[test]
    fn function_call_output_carries_the_call_id_and_result() {
        let value = parsed(&build_function_call_output("call_123", "きょうりゅうの はなし"));

        assert_eq!(value["type"], "conversation.item.create");
        assert_eq!(value["item"]["type"], "function_call_output");
        assert_eq!(value["item"]["call_id"], "call_123");
        assert_eq!(value["item"]["output"], "きょうりゅうの はなし");
    }

    #[test]
    fn unknown_events_are_ignored_rather_than_fatal() {
        let payload = json!({ "type": "rate_limits.updated", "rate_limits": [] }).to_string();

        assert_eq!(parse_server_event(&payload), Ok(ServerEvent::Ignored));
    }

    #[test]
    fn empty_deltas_are_ignored() {
        let payload = json!({ "type": "response.output_audio.delta", "delta": "" }).to_string();

        assert_eq!(parse_server_event(&payload), Ok(ServerEvent::Ignored));
    }

    #[test]
    fn rejects_payloads_that_are_not_events() {
        assert!(parse_server_event("{").is_err());
        assert!(parse_server_event(&json!({ "no_type": 1 }).to_string()).is_err());
    }
}
