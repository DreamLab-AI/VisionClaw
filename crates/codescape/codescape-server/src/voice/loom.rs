//! Loom fallback: OpenAI-compatible chat completions with structured output.
//!
//! Used only when the deterministic grammar returns `NoMatch`. The request
//! declines the ontology scaffold (`{"loom_options":{"scaffold":false}}`, a code
//! question is not an ontology subject), asks for `max_tokens` 1536 (reasoning
//! models need the headroom), and constrains the answer with the JSON schema of
//! the command AST (`strict: false`: the schema uses `anyOf` and bounds that
//! strict mode rejects; validation is the caller's job). The caller validates the returned JSON into the AST; output
//! that does not validate is spoken as text and never executed.

use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::OnceCell;

use crate::egress::{EgressAllowlist, EgressError};

/// `max_tokens` for every request.
pub const MAX_TOKENS: u32 = 1536;

/// Loom failures.
#[derive(Debug, thiserror::Error)]
pub enum LoomError {
    /// Not on the allowlist.
    #[error(transparent)]
    Egress(#[from] EgressError),
    /// Transport failure.
    #[error("loom request: {0}")]
    Http(String),
    /// Non-2xx response.
    #[error("loom status {0}")]
    Status(u16),
    /// The response had no usable JSON content.
    #[error("loom response: {0}")]
    Shape(String),
    /// Every Loom slot stayed busy for [`QUEUE_WAIT`].
    #[error("the language model is busy; try again shortly")]
    Busy,
}

/// Loom requests in flight across the whole server.
pub const MAX_CONCURRENT: usize = 4;
/// How long a request waits for a slot before [`LoomError::Busy`].
pub const QUEUE_WAIT: Duration = Duration::from_secs(10);
/// Longest user text sent to Loom (characters); the command path already caps at this.
pub const MAX_USER_CHARS: usize = 512;

/// The chat-completions body.
pub fn chat_request(model: &str, system: &str, user: &str, schema: &Value) -> Value {
    json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "max_tokens": MAX_TOKENS,
        "temperature": 0,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "codescape_command", "schema": schema, "strict": false },
        },
        "loom_options": { "scaffold": false },
    })
}

/// Pull the JSON object out of `choices[0].message.content`, tolerating a
/// `<think>…</think>` preamble and Markdown code fences.
pub fn extract_json(resp: &Value) -> Result<Value, LoomError> {
    let content = resp
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::Shape("no choices[0].message.content".into()))?;
    let mut s = content;
    if let Some(end) = s.rfind("</think>") {
        s = &s[end + "</think>".len()..];
    }
    let s = s
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = s
        .find('{')
        .ok_or_else(|| LoomError::Shape("no JSON object".into()))?;
    let end = s
        .rfind('}')
        .ok_or_else(|| LoomError::Shape("no JSON object".into()))?;
    // `}` before the first `{` (e.g. "}{") would make the slice below panic.
    if end < start {
        return Err(LoomError::Shape("no JSON object".into()));
    }
    serde_json::from_str(&s[start..=end]).map_err(|e| LoomError::Shape(e.to_string()))
}

/// Loom client.
pub struct LoomClient {
    http: reqwest::Client,
    base: String,
    model: String,
    resolved: OnceCell<String>,
    allow: EgressAllowlist,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
}

impl LoomClient {
    /// A client for `base` (`…/v1`). `model = "auto"` asks `GET {base}/models` once.
    pub fn new(allow: &EgressAllowlist, base: &str, model: &str) -> Self {
        Self {
            http: allow.http_client(Duration::from_secs(90)),
            base: base.trim_end_matches('/').to_string(),
            model: model.to_string(),
            resolved: OnceCell::new(),
            allow: allow.clone(),
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
        }
    }

    async fn model(&self) -> Result<String, LoomError> {
        if self.model != "auto" {
            return Ok(self.model.clone());
        }
        self.resolved
            .get_or_try_init(|| async {
                let url = format!("{}/models", self.base);
                self.allow.check(&url)?;
                let r = self
                    .http
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| LoomError::Http(e.to_string()))?;
                if !r.status().is_success() {
                    return Err(LoomError::Status(r.status().as_u16()));
                }
                let v: Value = r.json().await.map_err(|e| LoomError::Http(e.to_string()))?;
                v.pointer("/data/0/id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| LoomError::Shape("no models listed".into()))
            })
            .await
            .cloned()
    }

    /// Ask for a structured answer; returns the parsed JSON object (unvalidated).
    /// At most [`MAX_CONCURRENT`] run at once; the rest wait up to [`QUEUE_WAIT`].
    pub async fn ask(&self, system: &str, user: &str, schema: &Value) -> Result<Value, LoomError> {
        let _slot = tokio::time::timeout(QUEUE_WAIT, self.slots.acquire())
            .await
            .map_err(|_| LoomError::Busy)?
            .map_err(|_| LoomError::Busy)?;
        let user: String = user.chars().take(MAX_USER_CHARS).collect();
        let user = user.as_str();
        let url = format!("{}/chat/completions", self.base);
        self.allow.check(&url)?;
        let model = self.model().await?;
        let r = self
            .http
            .post(&url)
            .json(&chat_request(&model, system, user, schema))
            .send()
            .await
            .map_err(|e| LoomError::Http(e.to_string()))?;
        if !r.status().is_success() {
            return Err(LoomError::Status(r.status().as_u16()));
        }
        let v: Value = r.json().await.map_err(|e| LoomError::Http(e.to_string()))?;
        extract_json(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_declines_the_scaffold_and_carries_the_schema() {
        let schema = json!({"type":"object"});
        let r = chat_request("m", "sys", "callers of main", &schema);
        assert_eq!(r["loom_options"], json!({"scaffold": false}));
        assert_eq!(r["max_tokens"], 1536);
        assert_eq!(r["response_format"]["type"], "json_schema");
        assert_eq!(r["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(r["messages"][1]["content"], "callers of main");
    }

    #[test]
    fn content_extraction_tolerates_think_and_fences() {
        let wrap = |c: &str| json!({"choices":[{"message":{"content": c}}]});
        assert_eq!(
            extract_json(&wrap(r#"{"answer":"x"}"#)).unwrap(),
            json!({"answer":"x"})
        );
        assert_eq!(
            extract_json(&wrap(
                "<think>hmm {no}</think>\n```json\n{\"answer\":\"y\"}\n```"
            ))
            .unwrap(),
            json!({"answer":"y"})
        );
        assert!(extract_json(&wrap("no json here")).is_err());
        assert!(
            extract_json(&wrap("}{")).is_err(),
            "close before open must not panic"
        );
        assert!(extract_json(&wrap("} x {")).is_err());
        assert!(extract_json(&json!({})).is_err());
    }

    #[tokio::test]
    async fn unlisted_loom_is_refused() {
        let allow = EgressAllowlist::from_urls(["http://192.168.2.132:8084/v1"]).unwrap();
        let c = LoomClient::new(&allow, "http://192.168.2.48:8084/v1", "m");
        assert!(matches!(
            c.ask("s", "u", &json!({})).await,
            Err(LoomError::Egress(_))
        ));
    }
}
