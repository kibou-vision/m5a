//! web検索の電文の組み立てと解析。
//!
//! 通信そのものはハードウェア層が担い、ここでは Tavily API に渡す
//! リクエストの組み立てと、返ってきた JSON からの要約の取り出しだけを扱う。

use serde_json::{json, Value};

/// セッションに渡す function tool の名前。
pub const TOOL_NAME: &str = "search_web";

/// 問い合わせ先。
const ENDPOINT: &str = "https://api.tavily.com/search";
/// モデルへ返す要約の上限文字数。
/// 短い返事を求める指示文（[`crate::guardrail`]）に応じられる範囲に絞る。
const SUMMARY_LIMIT: usize = 200;

/// セッションに渡す function tool の定義。
pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "name": TOOL_NAME,
        "description": "自分が知らない、今のできごとや事実を調べたいときに使う。\
                         おしゃべりや気持ちの相談には使わない。",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "調べたいことを短い言葉で表したもの。",
                },
            },
            "required": ["query"],
        },
    })
}

/// Tavily への問い合わせ。実際の通信はハードウェア層が行う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// 問い合わせの電文を組み立てる。
pub fn build_request(query: &str, api_key: &str) -> SearchRequest {
    let body = json!({
        "query": query,
        "include_answer": true,
        "max_results": 1,
        "safe_search": true,
        "search_depth": "basic",
    })
    .to_string();

    SearchRequest {
        url: ENDPOINT.to_string(),
        headers: vec![
            ("Authorization".to_string(), format!("Bearer {api_key}")),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        body,
    }
}

/// モデルからの function_call の引数（`{"query": "..."}`）から検索語を取り出す。
pub fn extract_query(arguments_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(str::to_string)
}

/// Tavily の応答から、モデルに返す要約を取り出す。
///
/// `include_answer` で得た要約を優先し、無ければ先頭の検索結果の本文を使う。
/// どちらも無ければモデルが「わからない」と正直に答えられるよう `None` を返す。
pub fn parse_response(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;

    let text = value
        .get("answer")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            value
                .pointer("/results/0/content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        })?;

    Some(truncate(text, SUMMARY_LIMIT))
}

/// 文字数（Unicodeスカラー単位）で切り詰める。
fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition_declares_the_query_argument() {
        let definition = tool_definition();

        assert_eq!(definition["type"], "function");
        assert_eq!(definition["name"], TOOL_NAME);
        assert_eq!(definition["parameters"]["required"][0], "query");
    }

    #[test]
    fn build_request_carries_the_query_and_bearer_key() {
        let request = build_request("きょうりゅう", "tvly-secret");

        assert_eq!(request.url, "https://api.tavily.com/search");
        assert!(request
            .headers
            .contains(&("Authorization".to_string(), "Bearer tvly-secret".to_string())));

        let body: Value = serde_json::from_str(&request.body).expect("bodyはJSONのはず");
        assert_eq!(body["query"], "きょうりゅう");
        assert_eq!(body["include_answer"], true);
        assert_eq!(body["max_results"], 1);
        assert_eq!(body["safe_search"], true);
    }

    #[test]
    fn extract_query_reads_the_argument() {
        assert_eq!(
            extract_query(r#"{"query": "きょうりゅう の しゅるい"}"#),
            Some("きょうりゅう の しゅるい".to_string())
        );
    }

    #[test]
    fn extract_query_rejects_blank_or_missing_query() {
        assert_eq!(extract_query(r#"{"query": "  "}"#), None);
        assert_eq!(extract_query(r#"{}"#), None);
        assert_eq!(extract_query("not json"), None);
    }

    #[test]
    fn parse_response_prefers_the_generated_answer() {
        let body = json!({
            "answer": "きょうりゅうは とても むかし に いた いきものだよ。",
            "results": [{"content": "別の文"}],
        })
        .to_string();

        assert_eq!(
            parse_response(&body),
            Some("きょうりゅうは とても むかし に いた いきものだよ。".to_string())
        );
    }

    #[test]
    fn parse_response_falls_back_to_the_first_result() {
        let body = json!({
            "results": [{"content": "先頭の検索結果の本文"}],
        })
        .to_string();

        assert_eq!(parse_response(&body), Some("先頭の検索結果の本文".to_string()));
    }

    #[test]
    fn parse_response_is_none_when_nothing_usable_is_present() {
        assert_eq!(parse_response(&json!({"results": []}).to_string()), None);
        assert_eq!(parse_response("not json"), None);
    }

    #[test]
    fn parse_response_truncates_long_answers() {
        let long_answer = "あ".repeat(500);
        let body = json!({ "answer": long_answer }).to_string();

        let summary = parse_response(&body).expect("要約が取れるはず");

        assert_eq!(summary.chars().count(), SUMMARY_LIMIT);
    }
}
