//! Briefing Service — orchestrates the brief → execute → debrief workflow
//! from the VisionClaw Rust backend through the Management API.
//!
//! This service translates BriefingRequest structs into Management API calls,
//! creating briefs, spawning role-specific agents, and consolidating debriefs.

use crate::services::management_api_client::ManagementApiClient;
use crate::types::user_context::{BriefingRequest, BriefingResponse, RoleTask, UserContext};
use log::info;

pub struct BriefingService {
    api_client: ManagementApiClient,
}

impl BriefingService {
    pub fn new(api_client: ManagementApiClient) -> Self {
        Self { api_client }
    }

    /// Submit a briefing request to the Management API.
    ///
    /// This creates a brief file in the team folder structure, optionally creates
    /// a Beads epic, then spawns role-specific agents to respond.
    pub async fn submit_brief(
        &self,
        request: &BriefingRequest,
        user_context: &UserContext,
    ) -> Result<BriefingResponse, BriefingError> {
        info!(
            "[BriefingService] Submitting brief for user={}, roles={:?}",
            user_context.display_name, request.roles
        );

        // Step 1: Create the brief via Management API
        let brief_result = self
            .api_client
            .create_brief(
                &request.content,
                &request.roles,
                user_context,
                request.version.as_deref(),
                request.brief_type.as_deref(),
                request.slug.as_deref(),
            )
            .await
            .map_err(|e| BriefingError::ApiError(format!("Failed to create brief: {}", e)))?;

        let brief_id = brief_result.brief_id.clone();
        let brief_path = brief_result.brief_path.clone();
        let bead_id = brief_result.bead_id.clone();

        // Step 2: Execute the brief (spawn role agents)
        let role_tasks = self
            .api_client
            .execute_brief(
                &brief_id,
                &brief_path,
                &request.roles,
                user_context,
                bead_id.as_deref(),
            )
            .await
            .map_err(|e| BriefingError::ApiError(format!("Failed to execute brief: {}", e)))?;

        info!(
            "[BriefingService] Brief {} submitted: {} role agents spawned",
            brief_id,
            role_tasks.len()
        );

        Ok(BriefingResponse {
            brief_id,
            brief_path,
            bead_id,
            role_tasks,
        })
    }

    /// Request a debrief consolidation for a completed brief.
    pub async fn request_debrief(
        &self,
        brief_id: &str,
        role_tasks: &[RoleTask],
        user_context: &UserContext,
    ) -> Result<String, BriefingError> {
        info!(
            "[BriefingService] Requesting debrief for brief={}, user={}",
            brief_id, user_context.display_name
        );

        let debrief_path = self
            .api_client
            .create_debrief(brief_id, role_tasks, user_context)
            .await
            .map_err(|e| BriefingError::ApiError(format!("Failed to create debrief: {}", e)))?;

        info!("[BriefingService] Debrief created at {}", debrief_path);

        Ok(debrief_path)
    }
}

#[derive(Debug)]
pub enum BriefingError {
    ApiError(String),
}

impl std::fmt::Display for BriefingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BriefingError::ApiError(msg) => write!(f, "Briefing API error: {}", msg),
        }
    }
}

impl std::error::Error for BriefingError {}

#[cfg(test)]
mod tests {
    //! Integration tests for the brief → execute → debrief cycle against the
    //! agentbox `/v1/briefs` contract (ADR-2085, implemented by agentbox
    //! ADR-2072 in `management-api/routes/briefing.js`).
    //!
    //! These drive a REAL `ManagementApiClient` — the same code path production
    //! uses, per ADR-2085's "not a curl approximation" rule — against a mock
    //! HTTP origin that replies with exactly the JSON the agentbox route emits.
    //! The origin is a bare tokio `TcpListener` speaking minimal HTTP/1.1 rather
    //! than a mock-server crate, so this adds no dependency to Cargo.toml.
    //!
    //! What the mock pins is the wire contract's asymmetry, which is the part
    //! that silently breaks: request bodies are snake_case as the client
    //! literally writes them, response envelopes are camelCase
    //! (`#[serde(rename_all = "camelCase")]`), and the `RoleTask` elements
    //! nested inside `roleTasks` are snake_case because `RoleTask` itself
    //! carries no rename attribute. A server that camelCases those inner fields
    //! deserialises into a hard error here rather than in production.

    use super::*;
    use crate::services::management_api_client::ManagementApiClient;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One request the mock origin observed.
    #[derive(Debug, Clone)]
    struct Captured {
        method: String,
        path: String,
        authorization: Option<String>,
        body: serde_json::Value,
    }

    /// A canned reply: HTTP status code and JSON (or plain-text) body.
    #[derive(Clone)]
    struct Canned {
        status: u16,
        reason: &'static str,
        body: String,
    }

    fn json_reply(status: u16, reason: &'static str, body: serde_json::Value) -> Canned {
        Canned {
            status,
            reason,
            body: body.to_string(),
        }
    }

    /// Spawn a single-shot mock origin that serves `replies` in order, one per
    /// connection, and records every request it saw. Returns the bound port and
    /// the shared capture log.
    async fn spawn_origin(replies: Vec<Canned>) -> (u16, Arc<Mutex<Vec<Captured>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock origin");
        let port = listener.local_addr().expect("local_addr").port();
        let captured: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);

        tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => return,
                };

                // Read until the headers are complete, then until Content-Length
                // bytes of body have arrived. Enough HTTP/1.1 for reqwest's
                // fixed-length JSON POSTs; no chunked encoding is used here.
                let mut raw: Vec<u8> = Vec::new();
                let mut buf = [0u8; 4096];
                let (head_end, content_length) = loop {
                    match socket.read(&mut buf).await {
                        Ok(0) => break (raw.len(), 0usize),
                        Ok(n) => {
                            raw.extend_from_slice(&buf[..n]);
                            if let Some(pos) =
                                raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
                            {
                                let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                                let len = head
                                    .lines()
                                    .find_map(|l| {
                                        let (k, v) = l.split_once(':')?;
                                        if k.eq_ignore_ascii_case("content-length") {
                                            v.trim().parse::<usize>().ok()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                break (pos, len);
                            }
                        }
                        Err(_) => break (raw.len(), 0usize),
                    }
                };
                while raw.len() < head_end + content_length {
                    match socket.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                        Err(_) => break,
                    }
                }

                let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or_default().to_string();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let authorization = head.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.eq_ignore_ascii_case("authorization") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });
                let body_bytes = &raw[head_end.min(raw.len())..];
                let body: serde_json::Value =
                    serde_json::from_slice(body_bytes).unwrap_or(serde_json::Value::Null);

                sink.lock().unwrap().push(Captured {
                    method,
                    path,
                    authorization,
                    body,
                });

                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.reason,
                    reply.body.len(),
                    reply.body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });

        (port, captured)
    }

    fn user_context() -> UserContext {
        UserContext {
            user_id: "npub1testuser".to_string(),
            pubkey: "b".repeat(64),
            display_name: "test_operator".to_string(),
            session_id: "session-abc".to_string(),
            is_power_user: true,
        }
    }

    fn briefing_request() -> BriefingRequest {
        BriefingRequest {
            content: "Assess the interaction plane rebuild risk.".to_string(),
            roles: vec!["architect".to_string(), "reviewer".to_string()],
            version: Some("v0.2.33".to_string()),
            brief_type: Some("assessment".to_string()),
            slug: Some("rebuild-risk".to_string()),
        }
    }

    const BRIEF_ID: &str = "urn:agentbox:thing:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:brief-2026-09-05-rebuild-risk-0a1b2c3d";
    const BRIEF_DIR: &str = "/briefs/2026-09-05/rebuild-risk-0a1b2c3d";

    fn create_reply() -> Canned {
        json_reply(
            201,
            "Created",
            serde_json::json!({
                "briefId": BRIEF_ID,
                "briefPath": format!("{}/brief.md", BRIEF_DIR),
                "beadId": "urn:agentbox:bead:aaaa:sha256-12-deadbeefcafe",
            }),
        )
    }

    fn execute_reply() -> Canned {
        json_reply(
            202,
            "Accepted",
            serde_json::json!({
                "briefId": BRIEF_ID,
                // camelCase envelope, snake_case RoleTask members — exactly what
                // routes/briefing.js emits and what RoleTask's serde expects.
                "roleTasks": [
                    {
                        "role": "architect",
                        "task_id": "task-0",
                        "bead_id": "urn:agentbox:bead:aaaa:sha256-12-child00000a",
                        "response_path": format!("{}/responses/architect.md", BRIEF_DIR),
                    },
                    {
                        "role": "reviewer",
                        "task_id": "task-1",
                        "bead_id": serde_json::Value::Null,
                        "response_path": format!("{}/responses/reviewer.md", BRIEF_DIR),
                    }
                ],
            }),
        )
    }

    fn debrief_reply() -> Canned {
        json_reply(
            201,
            "Created",
            serde_json::json!({ "debriefPath": format!("{}/debrief.md", BRIEF_DIR) }),
        )
    }

    fn service(port: u16) -> BriefingService {
        BriefingService::new(ManagementApiClient::new(
            "127.0.0.1".to_string(),
            port,
            "test-management-api-key".to_string(),
        ))
    }

    #[tokio::test]
    async fn submit_brief_creates_then_executes_against_the_agentbox_contract() {
        let (port, captured) = spawn_origin(vec![create_reply(), execute_reply()]).await;

        let response = service(port)
            .submit_brief(&briefing_request(), &user_context())
            .await
            .expect("submit_brief should succeed against the ADR-2072 contract");

        // --- the workflow result the caller receives -------------------------
        assert_eq!(response.brief_id, BRIEF_ID);
        assert_eq!(response.brief_path, format!("{}/brief.md", BRIEF_DIR));
        assert_eq!(
            response.bead_id.as_deref(),
            Some("urn:agentbox:bead:aaaa:sha256-12-deadbeefcafe")
        );
        assert_eq!(response.role_tasks.len(), 2);
        assert_eq!(response.role_tasks[0].role, "architect");
        assert_eq!(response.role_tasks[0].task_id, "task-0");
        assert_eq!(
            response.role_tasks[0].response_path,
            format!("{}/responses/architect.md", BRIEF_DIR)
        );
        // A role with no work-ledger child must arrive as None, not as a fabricated id.
        assert_eq!(response.role_tasks[1].bead_id, None);

        // --- what actually went on the wire ---------------------------------
        let calls = captured.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "expected exactly create + execute");

        let create = &calls[0];
        assert_eq!(create.method, "POST");
        assert_eq!(create.path, "/v1/briefs");
        assert_eq!(
            create.authorization.as_deref(),
            Some("Bearer test-management-api-key")
        );
        // Request fields are snake_case — the client writes them literally, they
        // are NOT camelCased by the response struct's rename_all.
        assert_eq!(
            create.body["content"],
            "Assess the interaction plane rebuild risk."
        );
        assert_eq!(create.body["roles"][1], "reviewer");
        assert_eq!(create.body["version"], "v0.2.33");
        assert_eq!(create.body["brief_type"], "assessment");
        assert_eq!(create.body["slug"], "rebuild-risk");
        assert_eq!(create.body["user_context"]["user_id"], "npub1testuser");
        assert_eq!(create.body["user_context"]["display_name"], "test_operator");
        assert_eq!(create.body["user_context"]["session_id"], "session-abc");
        assert_eq!(create.body["user_context"]["is_power_user"], true);

        // Execute is addressed by the id create returned, and carries the epic
        // bead id forward so the role children hang off the right epic.
        let execute = &calls[1];
        assert_eq!(execute.method, "POST");
        assert_eq!(execute.path, format!("/v1/briefs/{}/execute", BRIEF_ID));
        assert_eq!(
            execute.body["brief_path"],
            format!("{}/brief.md", BRIEF_DIR)
        );
        assert_eq!(
            execute.body["epic_bead_id"],
            "urn:agentbox:bead:aaaa:sha256-12-deadbeefcafe"
        );
        assert_eq!(execute.body["roles"][0], "architect");
    }

    #[tokio::test]
    async fn request_debrief_posts_role_responses_and_returns_the_debrief_path() {
        let (port, captured) = spawn_origin(vec![debrief_reply()]).await;

        let role_tasks = vec![
            RoleTask {
                role: "architect".to_string(),
                task_id: "task-0".to_string(),
                bead_id: Some("urn:agentbox:bead:aaaa:sha256-12-child00000a".to_string()),
                response_path: format!("{}/responses/architect.md", BRIEF_DIR),
            },
            RoleTask {
                role: "reviewer".to_string(),
                task_id: "task-1".to_string(),
                bead_id: None,
                response_path: format!("{}/responses/reviewer.md", BRIEF_DIR),
            },
        ];

        let debrief_path = service(port)
            .request_debrief(BRIEF_ID, &role_tasks, &user_context())
            .await
            .expect("request_debrief should succeed against the ADR-2072 contract");

        assert_eq!(debrief_path, format!("{}/debrief.md", BRIEF_DIR));

        let calls = captured.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        let debrief = &calls[0];
        assert_eq!(debrief.method, "POST");
        assert_eq!(debrief.path, format!("/v1/briefs/{}/debrief", BRIEF_ID));

        // `role_responses` is a literal snake_case key, but its INNER fields are
        // camelCase as the client hand-writes them. Both halves are load-bearing.
        let responses = debrief.body["role_responses"]
            .as_array()
            .expect("role_responses must be an array");
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["role"], "architect");
        assert_eq!(responses[0]["taskId"], "task-0");
        assert_eq!(
            responses[0]["responsePath"],
            format!("{}/responses/architect.md", BRIEF_DIR)
        );
        // status is derived from bead_id presence: Some -> completed, None -> pending.
        assert_eq!(responses[0]["status"], "completed");
        assert_eq!(responses[1]["status"], "pending");
        assert_eq!(debrief.body["user_context"]["pubkey"], "b".repeat(64));
    }

    #[tokio::test]
    async fn submit_brief_surfaces_a_server_error_instead_of_a_silent_success() {
        // The regression ADR-2085 was written about: before ADR-2072 these
        // routes did not exist, so every call 404'd. Assert the client turns a
        // non-2xx into a BriefingError rather than swallowing it.
        let (port, _captured) = spawn_origin(vec![Canned {
            status: 404,
            reason: "Not Found",
            body: "{\"message\":\"Route POST:/v1/briefs not found\"}".to_string(),
        }])
        .await;

        let err = service(port)
            .submit_brief(&briefing_request(), &user_context())
            .await
            .expect_err("a 404 from the brief route must not read as success");

        let BriefingError::ApiError(message) = err;
        assert!(
            message.contains("Failed to create brief"),
            "error should name the failed step, got: {message}"
        );
    }

    #[tokio::test]
    async fn submit_brief_fails_when_execute_rejects_after_a_successful_create() {
        // Partial-failure honesty: create succeeded, execute was refused (e.g.
        // the agentbox action plane fail-closed 503 when the execution journal
        // has no events adapter). submit_brief must surface that, not return a
        // BriefingResponse with an empty role_tasks list.
        let (port, captured) = spawn_origin(vec![
            create_reply(),
            Canned {
                status: 503,
                reason: "Service Unavailable",
                body: "{\"message\":\"the execution journal has no events adapter\"}".to_string(),
            },
        ])
        .await;

        let err = service(port)
            .submit_brief(&briefing_request(), &user_context())
            .await
            .expect_err("a 503 from execute must fail the whole submit");

        let BriefingError::ApiError(message) = err;
        assert!(
            message.contains("Failed to execute brief"),
            "error should name the execute step, got: {message}"
        );
        assert_eq!(
            captured.lock().unwrap().len(),
            2,
            "create ran, then execute failed"
        );
    }
}
