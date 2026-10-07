//! D2 (PRD-023 WP-3, CANARY-VC-D2-STEER) interrupt id-namespace resolution —
//! honest fake-Management-API integration test (FINAL CLOSE).
//!
//! `AgentDetailPanel` sends `selectedAgent.id`, a claude-flow swarm `agent_id`.
//! That is a DISJOINT minted namespace from the Management-API `task_id` the stop
//! call needs. A re-verifier proved the earlier fix was still a dead end: the only
//! way the resolver could join the two was the `t.agent == id` fallback, but
//! `TaskInfo.agent` carries a ROLE LABEL ("coder"/"researcher"/…) that can never
//! equal a claude-flow agent id — and the previous fake only passed because it
//! FABRICATED the join by stuffing the swarm agent_id into that `agent` field.
//!
//! The real join is an explicit key: a task created for a claude-flow agent now
//! carries `claude_flow_agent_id`, which the Management API persists and echoes in
//! `GET /v1/tasks`. The resolver joins on THAT field (never `agent`). This test
//! drives the REAL orchestrator against a REAL local HTTP server that mirrors the
//! REAL Management-API contract — and, critically, the fake echoes
//! `claudeFlowAgentId` ONLY when the create actually carried it (the join is never
//! fabricated). Same hand-rolled-socket idiom as `tests/voice_intent_roundtrip.rs`:
//! no mock framework, real request line, real headers, real body.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use actix::Actor;
use serde_json::{json, Value};
use visionclaw_server::actors::{
    CreateTask, InterruptAgentTask, InterruptError, TaskOrchestratorActor,
};
use visionclaw_server::services::management_api_client::ManagementApiClient;

#[derive(Debug, Clone)]
struct CapturedReq {
    method: String,
    path: String,
}

/// Server-side memory of what a `POST /v1/tasks` actually carried, so the
/// `GET /v1/tasks` echo reflects the REAL create — the join is never fabricated.
#[derive(Default)]
struct FakeState {
    /// The `claude_flow_agent_id` the create carried, if any. `None` means the
    /// create had no join key, so the echo omits `claudeFlowAgentId`.
    recorded_cfa: Option<String>,
    /// The `agent` (role label) the create carried, echoed verbatim.
    recorded_agent: String,
    captured: Vec<CapturedReq>,
}

/// Bind an ephemeral port and serve the agentbox Management-API `/v1/tasks`
/// contract from a background thread until the process exits:
///   * `POST   /v1/tasks`      → record the create's `claude_flow_agent_id` +
///     `agent`, respond 202 with the fixed `task_id`;
///   * `GET    /v1/tasks`      → a one-task active list whose `taskId` is
///     `task_id`, `agent` is the recorded role, and
///     `claudeFlowAgentId` is present IFF the create
///     carried one (never fabricated);
///   * `DELETE /v1/tasks/{id}` → 200 (task stopped).
///
/// Returns the bound port and the shared fake state.
fn spawn_fake_mgmt_api(task_id: &'static str) -> (u16, Arc<Mutex<FakeState>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    let state = Arc::new(Mutex::new(FakeState::default()));
    let st = state.clone();

    thread::spawn(move || loop {
        let (stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(_) => break,
        };
        let mut write_stream = stream.try_clone().expect("clone stream");
        let mut reader = BufReader::new(stream);

        // Request line: "<METHOD> <PATH> HTTP/1.1".
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            continue;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();

        // Drain headers; capture content-length (the POST body carries the join).
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break; // end of headers
            }
            if let Some(v) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = Vec::new();
        if content_length > 0 {
            body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
        }

        let body = {
            let mut st = st.lock().unwrap();
            st.captured.push(CapturedReq {
                method: method.clone(),
                path: path.clone(),
            });

            if method == "POST" && path == "/v1/tasks" {
                // Record EXACTLY what the create carried — this is the only source
                // of the join key; nothing is invented.
                let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
                st.recorded_cfa = parsed
                    .get("claude_flow_agent_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                st.recorded_agent = parsed
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                json!({ "taskId": task_id, "status": "accepted", "message": "ok" }).to_string()
            } else if method == "GET" && path == "/v1/tasks" {
                let mut task_obj = json!({
                    "taskId": task_id,
                    "agent": st.recorded_agent,
                    "task": "demo",
                    "provider": "gemini",
                    "status": "running",
                    "startTime": 0,
                    "duration": 0,
                });
                // Echo the join key ONLY when the create carried it.
                if let Some(cfa) = &st.recorded_cfa {
                    task_obj["claudeFlowAgentId"] = json!(cfa);
                }
                json!({ "activeTasks": [task_obj], "count": 1 }).to_string()
            } else {
                // DELETE /v1/tasks/{id} (or anything else) → bare success 200.
                json!({ "success": true }).to_string()
            }
        };

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = write_stream.write_all(response.as_bytes());
        let _ = write_stream.flush();
    });

    (port, state)
}

fn methods_paths(state: &Arc<Mutex<FakeState>>) -> Vec<CapturedReq> {
    state.lock().unwrap().captured.clone()
}

#[actix_rt::test]
async fn claude_flow_agent_id_join_resolves_to_task_id_and_issues_stop() {
    // The genuine end-to-end join. A task is CREATED carrying the claude-flow swarm
    // agent_id; the Management API persists + echoes it as `claudeFlowAgentId`. An
    // interrupt BY THAT swarm agent_id resolves to the disjoint task_id via the
    // explicit join field — NOT via the role-label `agent` field (which is "coder").
    const SWARM_AGENT_ID: &str = "agent-swarm-7f3a"; // claude-flow shape, not a task_id
    const TASK_ID: &str = "task-abc123"; // disjoint Management-API id
    const ROLE: &str = "coder"; // the `agent` field — a role label, never the join

    let (port, state) = spawn_fake_mgmt_api(TASK_ID);
    let client = ManagementApiClient::new("127.0.0.1".to_string(), port, "test-key".to_string());
    let addr = TaskOrchestratorActor::new(client).start();

    // 1) Create the task WITH the claude-flow agent id — the real producer of the
    //    join. The orchestrator caches it under the returned task_id.
    let created = addr
        .send(CreateTask {
            agent: ROLE.to_string(),
            task: "demo".to_string(),
            provider: "gemini".to_string(),
            claude_flow_agent_id: Some(SWARM_AGENT_ID.to_string()),
        })
        .await
        .expect("create mailbox delivered")
        .expect("create ok");
    assert_eq!(created.task_id, TASK_ID);

    // 2) Interrupt by the SWARM AGENT ID — exactly what AgentDetailPanel sends. It
    //    is NOT the cached task_id, so it must resolve via GET /v1/tasks on the
    //    `claudeFlowAgentId` echo.
    let result = addr
        .send(InterruptAgentTask {
            id: SWARM_AGENT_ID.to_string(),
        })
        .await
        .expect("interrupt mailbox delivered");

    // The swarm agent_id resolved to the disjoint task_id.
    assert_eq!(
        result,
        Ok(TASK_ID.to_string()),
        "a claude_flow_agent_id must resolve to its Management-API task_id"
    );

    // Wire proof: a GET /v1/tasks (resolve), then a DELETE on the RESOLVED task_id
    // (stop) — and NEVER a DELETE on the raw swarm agent_id (the 404).
    let reqs = methods_paths(&state);
    assert!(
        reqs.iter()
            .any(|r| r.method == "GET" && r.path == "/v1/tasks"),
        "resolution must consult GET /v1/tasks; saw {reqs:?}"
    );
    assert!(
        reqs.iter()
            .any(|r| r.method == "DELETE" && r.path == format!("/v1/tasks/{TASK_ID}")),
        "stop must DELETE the RESOLVED task_id /v1/tasks/{TASK_ID}; saw {reqs:?}"
    );
    assert!(
        !reqs
            .iter()
            .any(|r| r.method == "DELETE" && r.path.contains(SWARM_AGENT_ID)),
        "must NOT DELETE the raw swarm agent_id (that is the 404 bug); saw {reqs:?}"
    );

    // Contract proof: the join key the echo carried was the recorded create value,
    // and the role label was a DISTINCT value — so the resolve could only have
    // matched `claudeFlowAgentId`, never `agent`.
    let st = state.lock().unwrap();
    assert_eq!(st.recorded_cfa.as_deref(), Some(SWARM_AGENT_ID));
    assert_eq!(st.recorded_agent, ROLE);
}

#[actix_rt::test]
async fn direct_task_id_still_resolves_via_fast_path_and_stops() {
    // Backward compatibility: an agent spawned via the task-registry path (e.g.
    // `spawn_agent_hybrid`, whose swarmId == task_id) is interrupted by its
    // task_id. The orchestrator cached it on create, so the fast path resolves it
    // with no GET round-trip, then stops.
    const TASK_ID: &str = "task-direct-9";

    let (port, state) = spawn_fake_mgmt_api(TASK_ID);
    let client = ManagementApiClient::new("127.0.0.1".to_string(), port, "test-key".to_string());
    let addr = TaskOrchestratorActor::new(client).start();

    let created = addr
        .send(CreateTask {
            agent: "researcher".to_string(),
            task: "demo".to_string(),
            provider: "gemini".to_string(),
            // No claude-flow id — this path is resolvable by task_id alone.
            claude_flow_agent_id: None,
        })
        .await
        .expect("create mailbox delivered")
        .expect("create ok");
    assert_eq!(created.task_id, TASK_ID);

    let result = addr
        .send(InterruptAgentTask {
            id: TASK_ID.to_string(),
        })
        .await
        .expect("interrupt mailbox delivered");

    assert_eq!(result, Ok(TASK_ID.to_string()));

    let reqs = methods_paths(&state);
    assert!(
        reqs.iter()
            .any(|r| r.method == "DELETE" && r.path == format!("/v1/tasks/{TASK_ID}")),
        "a direct task_id must DELETE /v1/tasks/{TASK_ID}; saw {reqs:?}"
    );
}

#[actix_rt::test]
async fn role_label_is_never_a_false_match() {
    // The re-verifier's proof, now a guard: a task created with role `agent="coder"`
    // and NO claude_flow_agent_id must NOT be interruptible by the string "coder".
    // The removed `t.agent == id` fallback would have stopped this task on a role
    // collision; the resolver must now refuse.
    const TASK_ID: &str = "task-role-1";
    const ROLE: &str = "coder";

    let (port, state) = spawn_fake_mgmt_api(TASK_ID);
    let client = ManagementApiClient::new("127.0.0.1".to_string(), port, "test-key".to_string());
    let addr = TaskOrchestratorActor::new(client).start();

    addr.send(CreateTask {
        agent: ROLE.to_string(),
        task: "demo".to_string(),
        provider: "gemini".to_string(),
        claude_flow_agent_id: None,
    })
    .await
    .expect("create mailbox delivered")
    .expect("create ok");

    // Interrupt by the ROLE LABEL — a false-positive hazard the fix closes.
    let result = addr
        .send(InterruptAgentTask {
            id: ROLE.to_string(),
        })
        .await
        .expect("interrupt mailbox delivered");

    assert_eq!(
        result,
        Err(InterruptError::Unresolved(
            "no active task resolves id 'coder'".to_string()
        )),
        "a role label must NOT resolve to a task (that was the false-positive hazard)"
    );

    let reqs = methods_paths(&state);
    assert!(
        !reqs.iter().any(|r| r.method == "DELETE"),
        "no DELETE may be issued for a role-label id; saw {reqs:?}"
    );
}

#[actix_rt::test]
async fn unresolvable_id_errors_without_stopping() {
    // Honesty: an id that matches neither a task_id nor any task's
    // claude_flow_agent_id errors with the DISTINCT `Unresolved` variant — the HTTP
    // layer maps this to the disclosed "not interruptible from here" 422. It must
    // NOT blind-stop an unrelated task: only the resolve GET is issued, no DELETE.
    const TASK_ID: &str = "task-other";

    let (port, state) = spawn_fake_mgmt_api(TASK_ID);
    let client = ManagementApiClient::new("127.0.0.1".to_string(), port, "test-key".to_string());
    let addr = TaskOrchestratorActor::new(client).start();

    // A task exists, carrying a DIFFERENT claude-flow id, so the list is non-empty.
    addr.send(CreateTask {
        agent: "researcher".to_string(),
        task: "demo".to_string(),
        provider: "gemini".to_string(),
        claude_flow_agent_id: Some("agent-other".to_string()),
    })
    .await
    .expect("create mailbox delivered")
    .expect("create ok");

    let result = addr
        .send(InterruptAgentTask {
            id: "agent-nonexistent".to_string(),
        })
        .await
        .expect("interrupt mailbox delivered");

    assert!(
        matches!(result, Err(InterruptError::Unresolved(_))),
        "an unresolvable id must error with Unresolved, not silently succeed; got {result:?}"
    );

    let reqs = methods_paths(&state);
    assert!(
        !reqs.iter().any(|r| r.method == "DELETE"),
        "no DELETE may be issued for an unresolvable id; saw {reqs:?}"
    );
}
