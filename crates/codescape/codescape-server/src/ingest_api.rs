//! The `:4101` presence-ingest listener (ADR-2143 §7).
//!
//! `POST /ingest/v1/activity` with `Authorization: Bearer <token>` and a JSON
//! body `{actor, kind: agent|human, event: post_tool_use|user_prompt|stop,
//! tool, paths, text, ts_ms}`. The order is rate limit (429) → token (401,
//! constant time, before the body is parsed) → parse (400) → validate (422)
//! → [`ActivitySink::accept`] (202). The sink, implemented by the engine, maps
//! paths, resolves nodes, broadcasts presence and queues re-extraction.

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use codescape_model::events::PresenceKind;
use serde::{Deserialize, Serialize};

use crate::auth::{IngestToken, RateLimiter};

/// Largest accepted body.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Longest kept `text` (characters); the hook already truncates to 2 KiB.
pub const MAX_TEXT_CHARS: usize = 4096;
/// Most paths per event.
pub const MAX_PATHS: usize = 256;
/// Authenticated requests per second.
pub const RATE_PER_SEC: u32 = 100;
/// Failed-authentication answers per second (beyond: 429). A separate bucket, so
/// unauthenticated traffic can never use up the hooks' allowance.
pub const AUTH_FAIL_PER_SEC: u32 = 10;
/// Longest presence text kept when agent text is not shared (one spoken caption).
pub const SHARED_TEXT_CHARS: usize = 280;

/// Truncate to `n` characters (not bytes).
pub fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The text a hook event contributes to its presence event, which every
/// collaborator sees, the timeline stores and `what is X doing` repeats.
/// Until NIP-98 (P6) identifies who is watching, prompts and assistant answers
/// stay private: only the tool name is shared. `share_agent_text`
/// (`--share-agent-text`) restores the full, already hook-redacted text.
pub fn presence_text(body: &ActivityBody, share_agent_text: bool) -> Option<String> {
    if share_agent_text {
        return body.text.clone().or_else(|| body.tool.clone());
    }
    body.tool.as_deref().map(|t| truncate_chars(t, 64))
}

/// One hook event.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ActivityBody {
    /// Actor label (`claude:tab0`, a session id, a human name).
    pub actor: String,
    /// `agent` or `human`.
    pub kind: String,
    /// `post_tool_use`, `user_prompt` or `stop`.
    pub event: String,
    /// Tool name for `post_tool_use`.
    #[serde(default)]
    pub tool: Option<String>,
    /// Paths the tool touched, as the agent saw them (rewritten by the path map).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Prompt or last assistant text (already redacted by the hook).
    #[serde(default)]
    pub text: Option<String>,
    /// Event time, Unix ms; server time when absent.
    #[serde(default)]
    pub ts_ms: Option<u64>,
}

impl ActivityBody {
    /// Check enumerations and bounds; truncates `text`.
    pub fn validate(mut self) -> Result<Self, String> {
        if self.actor.trim().is_empty() || self.actor.chars().count() > 128 {
            return Err("actor must be 1-128 characters".into());
        }
        if !matches!(self.kind.as_str(), "agent" | "human") {
            return Err("kind must be agent or human".into());
        }
        if !matches!(
            self.event.as_str(),
            "post_tool_use" | "user_prompt" | "stop"
        ) {
            return Err("event must be post_tool_use, user_prompt or stop".into());
        }
        if self.paths.len() > MAX_PATHS {
            return Err(format!("at most {MAX_PATHS} paths"));
        }
        if let Some(t) = &self.text {
            if t.chars().count() > MAX_TEXT_CHARS {
                self.text = Some(t.chars().take(MAX_TEXT_CHARS).collect());
            }
        }
        Ok(self)
    }

    /// Presence kind before node resolution. `Write` reads as `Create`; the sink
    /// downgrades it to `Update` when the file is already in the graph.
    pub fn presence_kind(&self) -> PresenceKind {
        match self.event.as_str() {
            "user_prompt" | "stop" => PresenceKind::Say,
            _ => match self.tool.as_deref().unwrap_or("") {
                "Edit" | "MultiEdit" | "NotebookEdit" => PresenceKind::Update,
                "Write" => PresenceKind::Create,
                _ => PresenceKind::Query,
            },
        }
    }
}

/// What the sink did with an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AcceptReport {
    /// Repository the paths resolved into, if any.
    pub repo: Option<String>,
    /// Nodes the event touched.
    pub nodes: usize,
    /// Paths queued for re-extraction.
    pub queued_paths: usize,
}

/// Receives validated events.
pub trait ActivitySink: Send + Sync + 'static {
    /// Handle one event.
    fn accept(&self, body: ActivityBody) -> AcceptReport;
}

/// Shared state of the ingest app.
pub struct IngestState {
    /// The bearer token.
    pub token: IngestToken,
    /// Rate limiter for authenticated requests.
    pub limiter: RateLimiter,
    /// Rate limiter for failed authentication.
    pub auth_fail_limiter: RateLimiter,
    /// Where events go.
    pub sink: Arc<dyn ActivitySink>,
}

impl IngestState {
    /// State with the default limit of [`RATE_PER_SEC`].
    pub fn new(token: IngestToken, sink: Arc<dyn ActivitySink>) -> Self {
        Self {
            token,
            limiter: RateLimiter::new(RATE_PER_SEC, RATE_PER_SEC),
            auth_fail_limiter: RateLimiter::new(AUTH_FAIL_PER_SEC, AUTH_FAIL_PER_SEC),
            sink,
        }
    }
}

async fn activity(req: HttpRequest, body: web::Bytes, st: web::Data<IngestState>) -> HttpResponse {
    let auth = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    // Token first (constant time, cheap): failures draw from their own bucket.
    if st.token.check_header(auth).is_err() {
        if !st.auth_fail_limiter.try_acquire() {
            return HttpResponse::TooManyRequests()
                .json(serde_json::json!({"error":"rate limited"}));
        }
        return HttpResponse::Unauthorized()
            .insert_header(("WWW-Authenticate", "Bearer"))
            .json(serde_json::json!({"error":"unauthorized"}));
    }
    if !st.limiter.try_acquire() {
        return HttpResponse::TooManyRequests().json(serde_json::json!({"error":"rate limited"}));
    }
    let parsed: ActivityBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()}))
        }
    };
    let valid = match parsed.validate() {
        // Hooks speak for agents; humans appear only through the client sockets.
        Ok(v) => ActivityBody {
            kind: "agent".into(),
            ..v
        },
        Err(e) => return HttpResponse::UnprocessableEntity().json(serde_json::json!({"error": e})),
    };
    let sink = st.sink.clone();
    match web::block(move || sink.accept(valid)).await {
        Ok(report) => HttpResponse::Accepted().json(report),
        Err(e) => {
            HttpResponse::InternalServerError().json(serde_json::json!({"error": e.to_string()}))
        }
    }
}

/// Register the ingest routes (`POST /ingest/v1/activity`; everything else 404).
pub fn configure(cfg: &mut web::ServiceConfig, state: web::Data<IngestState>) {
    cfg.app_data(state)
        .app_data(web::PayloadConfig::new(MAX_BODY_BYTES))
        .route("/ingest/v1/activity", web::post().to(activity))
        .default_service(
            web::route()
                .to(|req: HttpRequest| async move { crate::http::not_found_json(req.path()) }),
        );
}

#[cfg(test)]
mod tests {
    use actix_web::{test, App};
    use parking_lot::Mutex;

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[derive(Default)]
    struct Recorder(Mutex<Vec<ActivityBody>>);
    impl ActivitySink for Recorder {
        fn accept(&self, body: ActivityBody) -> AcceptReport {
            let n = body.paths.len();
            self.0.lock().push(body);
            AcceptReport {
                repo: Some("t".into()),
                nodes: n,
                queued_paths: n,
            }
        }
    }

    fn body() -> serde_json::Value {
        serde_json::json!({"actor":"claude:tab0","kind":"agent","event":"post_tool_use","tool":"Edit","paths":["/w/src/a.rs"],"ts_ms":1})
    }

    #[::core::prelude::v1::test]
    fn agent_text_is_private_by_default() {
        let mut b: ActivityBody = serde_json::from_value(body()).unwrap();
        b.text = Some("secret prompt".into());
        assert_eq!(presence_text(&b, false).as_deref(), Some("Edit"));
        assert_eq!(presence_text(&b, true).as_deref(), Some("secret prompt"));
        let prompt = ActivityBody {
            event: "user_prompt".into(),
            tool: None,
            ..b.clone()
        };
        assert_eq!(
            presence_text(&prompt, false),
            None,
            "a prompt contributes no text"
        );
        assert_eq!(
            presence_text(&prompt, true).as_deref(),
            Some("secret prompt")
        );
        assert_eq!(truncate_chars("ééé", 2), "éé");
    }

    #[actix_web::test]
    async fn token_gates_ingest() {
        let rec = Arc::new(Recorder::default());
        let state = web::Data::new(IngestState::new(
            IngestToken::new(TOKEN).unwrap(),
            rec.clone(),
        ));
        let app = test::init_service(App::new().configure(|c| configure(c, state.clone()))).await;

        let no_auth = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .set_json(body())
            .to_request();
        assert_eq!(
            test::call_service(&app, no_auth).await.status().as_u16(),
            401
        );

        let wrong = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .insert_header(("Authorization", "Bearer 0123456789abcdef0123456789abcdeX"))
            .set_json(body())
            .to_request();
        assert_eq!(test::call_service(&app, wrong).await.status().as_u16(), 401);

        let ok = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .insert_header(("Authorization", format!("Bearer {TOKEN}")))
            .set_json(body())
            .to_request();
        let res = test::call_service(&app, ok).await;
        assert_eq!(res.status().as_u16(), 202);
        let report: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(report["nodes"], 1);
        assert_eq!(rec.0.lock().len(), 1);

        let bad_kind = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .insert_header(("Authorization", format!("Bearer {TOKEN}")))
            .set_json(serde_json::json!({"actor":"a","kind":"robot","event":"stop"}))
            .to_request();
        assert_eq!(
            test::call_service(&app, bad_kind).await.status().as_u16(),
            422
        );

        let garbage = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .insert_header(("Authorization", format!("Bearer {TOKEN}")))
            .set_payload("{")
            .to_request();
        assert_eq!(
            test::call_service(&app, garbage).await.status().as_u16(),
            400
        );
        assert_eq!(
            rec.0.lock().len(),
            1,
            "only the valid authenticated event reached the sink"
        );
    }

    #[actix_web::test]
    async fn rate_limit_answers_429() {
        let rec = Arc::new(Recorder::default());
        let mut st = IngestState::new(IngestToken::new(TOKEN).unwrap(), rec);
        st.limiter = RateLimiter::new(2, 1);
        let state = web::Data::new(st);
        let app = test::init_service(App::new().configure(|c| configure(c, state.clone()))).await;
        let mut codes = vec![];
        for _ in 0..3 {
            let r = test::TestRequest::post()
                .uri("/ingest/v1/activity")
                .insert_header(("Authorization", format!("Bearer {TOKEN}")))
                .set_json(body())
                .to_request();
            codes.push(test::call_service(&app, r).await.status().as_u16());
        }
        assert_eq!(codes, vec![202, 202, 429]);
    }

    /// Unauthenticated floods hit their own bucket and never starve the hooks.
    #[actix_web::test]
    async fn failed_auth_cannot_exhaust_the_hook_bucket() {
        let rec = Arc::new(Recorder::default());
        let state = web::Data::new(IngestState::new(
            IngestToken::new(TOKEN).unwrap(),
            rec.clone(),
        ));
        let app = test::init_service(App::new().configure(|c| configure(c, state.clone()))).await;
        let mut codes = vec![];
        for _ in 0..(AUTH_FAIL_PER_SEC + 5) {
            let r = test::TestRequest::post()
                .uri("/ingest/v1/activity")
                .set_json(body())
                .to_request();
            codes.push(test::call_service(&app, r).await.status().as_u16());
        }
        assert_eq!(
            codes.iter().filter(|&&c| c == 401).count(),
            AUTH_FAIL_PER_SEC as usize
        );
        assert!(codes.ends_with(&[429; 5]));
        let ok = test::TestRequest::post()
            .uri("/ingest/v1/activity")
            .insert_header(("Authorization", format!("Bearer {TOKEN}")))
            .set_json(serde_json::json!({"actor":"x","kind":"human","event":"stop"}))
            .to_request();
        assert_eq!(
            test::call_service(&app, ok).await.status().as_u16(),
            202,
            "the hook still gets through"
        );
        assert_eq!(
            rec.0.lock().last().unwrap().kind,
            "agent",
            "hook events are always agents"
        );
    }

    #[::core::prelude::v1::test]
    fn presence_kinds_follow_the_tool() {
        let mk = |event: &str, tool: Option<&str>| ActivityBody {
            actor: "a".into(),
            kind: "agent".into(),
            event: event.into(),
            tool: tool.map(str::to_string),
            paths: vec![],
            text: None,
            ts_ms: None,
        };
        assert_eq!(
            mk("post_tool_use", Some("Read")).presence_kind(),
            PresenceKind::Query
        );
        assert_eq!(
            mk("post_tool_use", Some("Grep")).presence_kind(),
            PresenceKind::Query
        );
        assert_eq!(
            mk("post_tool_use", Some("MultiEdit")).presence_kind(),
            PresenceKind::Update
        );
        assert_eq!(
            mk("post_tool_use", Some("Write")).presence_kind(),
            PresenceKind::Create
        );
        assert_eq!(mk("user_prompt", None).presence_kind(), PresenceKind::Say);
        let long = ActivityBody {
            text: Some("x".repeat(MAX_TEXT_CHARS + 10)),
            ..mk("stop", None)
        };
        assert_eq!(long.validate().unwrap().text.unwrap().len(), MAX_TEXT_CHARS);
    }
}
