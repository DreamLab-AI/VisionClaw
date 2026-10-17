//! HTTP and WebSocket routes on the main port.
//!
//! | Route | |
//! |---|---|
//! | `GET /api/health` | engine, repo, revision, counts, settled, version |
//! | `GET /api/graph/snapshot?tier=l0\|l1\|l2&focus_files=1,2` | projected `GraphSnapshot` |
//! | `GET /api/graph/node/{id}` | node, position, heat, neighbours |
//! | `GET /api/graph/search?q=&limit=` | ranked matches |
//! | `POST /api/graph/query {text}` | `QueryResult` |
//! | `GET /api/settings/physics` | layout parameters (XR `PhysicsView` keys) |
//! | `GET /api/graph/settlement` | XR settlement body |
//! | `WS /ws/positions`, `/ws/events`, `/wss`, `/ws/voice` | see [`ws`] and [`voice`] |
//!
//! Every route takes an optional `?repo=<label>` (default: the first repo).

pub mod voice;
pub mod ws;

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use codescape_collab::{command_from_llm, command_schema, parse_command, ParseError};
use codescape_model::events::QueryResult;
use codescape_model::{lod, GraphSnapshot, Tier};
use codescape_server::{now_ms, VERSION};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;

use crate::engine::RepoEngine;
use crate::queries::{execute, CommandCtx, Effect};
use crate::state::{parse_tier, AppState};

/// Common `?repo=` parameter.
#[derive(Debug, Deserialize, Default)]
pub struct RepoQuery {
    /// Repository label.
    pub repo: Option<String>,
}

fn repo_or_404(st: &AppState, label: Option<&str>) -> Result<Arc<RepoEngine>, Box<HttpResponse>> {
    st.repo(label).ok_or_else(|| {
        Box::new(HttpResponse::NotFound().json(json!({"error": "unknown repo", "repo": label})))
    })
}

async fn health(st: web::Data<AppState>, q: web::Query<RepoQuery>) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let p = repo.current();
    let l = repo.layout.read().clone();
    HttpResponse::Ok().json(json!({
        "engine": l.engine,
        "engine_reason": l.reason,
        "repo": repo.label,
        "repos": st.repos.iter().map(|r| r.label.clone()).collect::<Vec<_>>(),
        "epoch": p.snapshot.epoch,
        "revision": p.snapshot.revision,
        "nodes": p.snapshot.nodes.len(),
        "edges": p.snapshot.edges.len(),
        "tiers": p.snapshot.tier_counts(),
        "settled": l.settled,
        "layout_steps": l.steps,
        "energy": l.energy,
        "auth": st.verifier.name(),
        "dropped_change_batches": repo.changes.dropped(),
        "connections": st.connections(),
        "version": VERSION,
    }))
}

/// `?tier=&focus_files=` for snapshot projection.
#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    repo: Option<String>,
    tier: Option<String>,
    focus_files: Option<String>,
}

/// The projected sub-snapshot: nodes at or above `tier` plus focus-file symbols, and the edges among them.
pub fn project_snapshot(s: &GraphSnapshot, tier: Tier, focus_files: &[u32]) -> GraphSnapshot {
    let ids: std::collections::HashSet<u32> =
        lod::project(s, tier, focus_files).into_iter().collect();
    GraphSnapshot {
        repo: s.repo.clone(),
        epoch: s.epoch,
        revision: s.revision,
        generated_at_ms: s.generated_at_ms,
        nodes: s
            .nodes
            .iter()
            .filter(|n| ids.contains(&n.id))
            .cloned()
            .collect(),
        edges: s
            .edges
            .iter()
            .filter(|e| ids.contains(&e.source) && ids.contains(&e.target))
            .cloned()
            .collect(),
    }
}

fn parse_ids(s: Option<&str>) -> Vec<u32> {
    s.unwrap_or("")
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect()
}

async fn snapshot(st: web::Data<AppState>, q: web::Query<SnapshotQuery>) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let tier = match q.tier.as_deref() {
        None => Tier::L1,
        Some(t) => match parse_tier(t) {
            Some(t) => t,
            None => {
                return HttpResponse::BadRequest()
                    .json(json!({"error": "tier must be l0, l1 or l2"}))
            }
        },
    };
    let focus = parse_ids(q.focus_files.as_deref());
    // Cached per (revision, tier, focus set): repeated l2 requests reuse one body.
    match web::block(move || repo.snapshot_body(tier, &focus)).await {
        Ok(Ok(body)) => HttpResponse::Ok()
            .content_type("application/json")
            .body(body),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

async fn node(
    st: web::Data<AppState>,
    path: web::Path<u32>,
    q: web::Query<RepoQuery>,
) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let id = path.into_inner();
    let p = repo.current();
    let Some(n) = p.index.get(&id).map(|&i| p.snapshot.nodes[i].clone()) else {
        return HttpResponse::NotFound().json(json!({"error": "unknown node", "id": id}));
    };
    let pos = {
        let f = repo.positions.read();
        f.index.get(&id).map(|&i| f.pos[i])
    };
    let heat = repo
        .collab
        .lock()
        .registry
        .heat(now_ms())
        .get(&id)
        .copied()
        .unwrap_or(0.0);
    let r2 = repo.clone();
    let neighbours = web::block(move || {
        let ing = r2.ingest.read();
        let mut v: Vec<_> = ing
            .neighbors(id, None, false, true)
            .into_iter()
            .map(|(m, rel)| json!({"id": m, "relation": rel.as_str(), "direction": "out"}))
            .collect();
        v.extend(
            ing.neighbors(id, None, true, false)
                .into_iter()
                .map(|(m, rel)| json!({"id": m, "relation": rel.as_str(), "direction": "in"})),
        );
        v.truncate(200);
        v
    })
    .await
    .unwrap_or_default();
    HttpResponse::Ok()
        .json(json!({ "node": n, "position": pos, "heat": heat, "neighbours": neighbours }))
}

/// `?q=&limit=`.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    repo: Option<String>,
    q: String,
    limit: Option<usize>,
}

async fn search(st: web::Data<AppState>, q: web::Query<SearchQuery>) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let query = q.q.clone();
    let r2 = repo.clone();
    let hits = web::block(move || r2.ingest.read().search(&query, limit))
        .await
        .unwrap_or_default();
    let p = repo.current();
    let out: Vec<_> = hits
        .into_iter()
        .filter_map(|(id, score)| {
            let n = &p.snapshot.nodes[*p.index.get(&id)?];
            Some(json!({"id": id, "label": n.label.as_ref(), "kind": n.kind.as_str(), "path": n.path.as_deref(), "score": score}))
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// Body of `POST /api/graph/query`.
#[derive(Debug, Deserialize)]
pub struct QueryBody {
    text: String,
    #[serde(default)]
    focus: Option<u32>,
}

async fn query(
    st: web::Data<AppState>,
    q: web::Query<RepoQuery>,
    body: web::Json<QueryBody>,
) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let ctx = Arc::new(Mutex::new(CommandCtx {
        focus: body.focus,
        ..Default::default()
    }));
    match run_command(&st, &repo, &body.text, ctx).await {
        Ok((r, _)) => HttpResponse::Ok().json(r),
        Err(e) => HttpResponse::UnprocessableEntity().json(json!({"error": e})),
    }
}

const LOOM_SYSTEM: &str =
    "You map a spoken request about a source-code graph onto one view command. \
Reply with JSON only: {\"command\": <command or null>, \"answer\": <one short spoken sentence>}. \
Commands are externally tagged, for example: {\"focus\":{\"name\":\"stable_match\"}}, \
{\"explain\":\"this\"}, {\"callers\":{\"target\":{\"name\":\"parse_command\"},\"depth\":1}}, \
{\"affected\":{\"target\":{\"name\":\"GraphDelta\"},\"depth\":3}}, \
{\"path\":{\"from\":{\"name\":\"a\"},\"to\":{\"name\":\"b\"}}}, \
{\"show_only\":{\"lang\":\"rust\",\"community\":null}}, \"zoom_out\", \"overview\". \
\"Bring up\", \"show me\" or \"open\" a named thing means focus. Use the identifier the speaker \
described (snake_case or CamelCase) as the name. Use null when the request is not about the code \
graph, and answer briefly; never invent a target that was not described.";

/// Parse with the grammar; on no match, ask Loom (when configured) and validate its reply.
/// Returns the result and the focus effect for the caller to apply.
pub async fn run_command(
    st: &AppState,
    repo: &Arc<RepoEngine>,
    text: &str,
    ctx: Arc<Mutex<CommandCtx>>,
) -> Result<(QueryResult, Effect), String> {
    // Bound the work before the grammar (and Loom) sees it.
    let truncated = text.chars().count() > MAX_COMMAND_CHARS;
    let owned: String;
    let text = if truncated {
        owned = text.chars().take(MAX_COMMAND_CHARS).collect();
        owned.as_str()
    } else {
        text
    };
    let mut out = run_command_inner(st, repo, text, ctx).await;
    if truncated {
        let note = format!(" (Your command was cut to {MAX_COMMAND_CHARS} characters.)");
        match &mut out {
            Ok((r, _)) => r.answer.push_str(&note),
            Err(e) => e.push_str(&note),
        }
    }
    out
}

/// Longest command text parsed (characters); longer input is truncated and the answer says so.
pub const MAX_COMMAND_CHARS: usize = 512;

async fn run_command_inner(
    st: &AppState,
    repo: &Arc<RepoEngine>,
    text: &str,
    ctx: Arc<Mutex<CommandCtx>>,
) -> Result<(QueryResult, Effect), String> {
    let cmd = match parse_command(text) {
        Ok(c) => Some((c, None)),
        Err(ParseError::Empty) => return Err("Say or type a command.".into()),
        Err(ParseError::NoMatch { normalised })
        | Err(ParseError::Invalid { reason: normalised }) => {
            let Some(loom) = st.loom.as_ref() else {
                return Err(format!(
                    "I did not understand \"{normalised}\". Say help for examples."
                ));
            };
            match loom.ask(LOOM_SYSTEM, text, &command_schema()).await {
                Ok(v) => match command_from_llm(&v) {
                    Ok((Some(c), answer)) => Some((c, Some(answer))),
                    Ok((None, answer)) => {
                        return Ok((
                            QueryResult {
                                answer,
                                ..Default::default()
                            },
                            Effect::None,
                        ))
                    }
                    Err(e) => {
                        // Spoken as text, never executed.
                        tracing::info!(error = %e, "Loom reply failed validation");
                        let answer = v
                            .get("answer")
                            .and_then(|a| a.as_str())
                            .unwrap_or("I could not interpret that.")
                            .to_string();
                        return Ok((
                            QueryResult {
                                answer,
                                ..Default::default()
                            },
                            Effect::None,
                        ));
                    }
                },
                Err(codescape_server::voice::loom::LoomError::Busy) => {
                    return Err("The language model is busy; try again in a moment.".into());
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Loom fallback failed");
                    return Err(format!(
                        "I did not understand \"{text}\", and the language model is unavailable."
                    ));
                }
            }
        }
    };
    let (cmd, llm_answer) = cmd.expect("set above");
    let repo = repo.clone();
    let res = web::block(move || {
        let mut c = ctx.lock();
        execute(&repo, &cmd, &mut c, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?;
    let (mut r, eff) = res;
    if let Some(a) = llm_answer.filter(|a| !a.is_empty()) {
        if r.answer.is_empty() {
            r.answer = a;
        }
    }
    Ok((r, eff))
}

async fn physics(st: web::Data<AppState>) -> HttpResponse {
    let p = codescape_layout::LayoutParams::default();
    let engine = st
        .repos
        .first()
        .map(|r| r.layout.read().engine.clone())
        .unwrap_or_default();
    HttpResponse::Ok().json(json!({
        "repelK": p.repulsion,
        "springK": p.spring_k,
        "damping": p.damping,
        "gravity": p.gravity,
        "dt": p.dt,
        "maxVelocity": p.max_velocity,
        "theta": p.theta,
        "structuralWeight": p.structural_weight,
        "intraCommunityWeight": p.intra_community_weight,
        "interCommunityWeight": p.inter_community_weight,
        "dagBiasK": 0.0,
        "engine": engine,
        "source": "codescape",
    }))
}

async fn settlement(st: web::Data<AppState>, q: web::Query<RepoQuery>) -> HttpResponse {
    let repo = match repo_or_404(&st, q.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let l = repo.layout.read().clone();
    HttpResponse::Ok().json(json!({
        "settlementState": { "isSettled": l.settled, "kineticEnergy": l.energy, "steps": l.steps },
        "telemetryLive": !l.engine.is_empty(),
        "physicsRunning": !l.engine.is_empty(),
        "engine": l.engine,
        "revision": repo.current().snapshot.revision,
    }))
}

/// Register every main-port route except static files.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/health", web::get().to(health))
        .route("/api/graph/snapshot", web::get().to(snapshot))
        .route("/api/graph/node/{id}", web::get().to(node))
        .route("/api/graph/search", web::get().to(search))
        .route("/api/graph/query", web::post().to(query))
        .route("/api/settings/physics", web::get().to(physics))
        .route("/api/graph/settlement", web::get().to(settlement))
        .route("/ws/positions", web::get().to(ws::positions))
        .route("/ws/events", web::get().to(ws::events))
        .route("/wss", web::get().to(ws::wss))
        .route("/ws/voice", web::get().to(voice::voice));
}

/// Verify a request with the configured verifier (open access until P6).
pub fn verify(st: &AppState, req: &HttpRequest) -> Result<(), Box<HttpResponse>> {
    let auth = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    // Rebuilt from --public-origin, never from the client-controlled Host header.
    let url = codescape_server::http::public_url(&st.cfg.public_origin(), req.uri());
    st.verifier
        .verify_http(req.method().as_str(), &url, auth)
        .map(|_| ())
        .map_err(|e| Box::new(HttpResponse::Unauthorized().json(json!({"error": e.to_string()}))))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use codescape_model::{CodeEdge, CodeNode, Confidence, Lang, NodeKind, Relation};

    use super::*;

    fn n(id: u32, kind: NodeKind, tier: Tier, parent: u32) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("k{id}")),
            kind,
            lang: Lang::Rust,
            label: Arc::from(format!("n{id}")),
            path: Some(Arc::from("src/a.rs")),
            span: None,
            parent,
            community: 1,
            cluster: 1,
            importance: 0.0,
            tier,
            size: 0,
        }
    }

    /// Every example the Loom prompt teaches must validate into a `Command`.
    #[test]
    fn loom_prompt_examples_are_valid_commands() {
        let examples = [
            r#"{"focus":{"name":"stable_match"}}"#,
            r#"{"explain":"this"}"#,
            r#"{"callers":{"target":{"name":"parse_command"},"depth":1}}"#,
            r#"{"affected":{"target":{"name":"GraphDelta"},"depth":3}}"#,
            r#"{"path":{"from":{"name":"a"},"to":{"name":"b"}}}"#,
            r#"{"show_only":{"lang":"rust","community":null}}"#,
            r#""zoom_out""#,
            r#""overview""#,
        ];
        for ex in examples {
            assert!(LOOM_SYSTEM.contains(ex), "prompt lists {ex}");
            let v = json!({ "command": serde_json::from_str::<serde_json::Value>(ex).unwrap(), "answer": "" });
            let (cmd, _) = command_from_llm(&v).unwrap_or_else(|e| panic!("{ex}: {e}"));
            assert!(cmd.is_some(), "{ex}");
        }
    }

    #[test]
    fn snapshot_projection_keeps_tier_and_focus_symbols_only() {
        let s = GraphSnapshot {
            repo: Arc::from("t"),
            epoch: 1,
            revision: 3,
            generated_at_ms: 0,
            nodes: vec![
                n(1, NodeKind::Directory, Tier::L0, 0),
                n(2, NodeKind::File, Tier::L1, 1),
                n(3, NodeKind::Function, Tier::L2, 2),
                n(4, NodeKind::File, Tier::L1, 1),
                n(5, NodeKind::Function, Tier::L2, 4),
            ],
            edges: vec![
                CodeEdge {
                    source: 2,
                    target: 3,
                    relation: Relation::Contains,
                    weight: 1.0,
                    confidence: Confidence::Extracted,
                },
                CodeEdge {
                    source: 2,
                    target: 4,
                    relation: Relation::Imports,
                    weight: 1.0,
                    confidence: Confidence::Extracted,
                },
            ],
        };
        let l1 = project_snapshot(&s, Tier::L1, &[]);
        assert_eq!(
            l1.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![1, 2, 4]
        );
        assert_eq!(l1.edges.len(), 1);
        let focus = project_snapshot(&s, Tier::L1, &[2]);
        assert_eq!(
            focus.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(focus.edges.len(), 2);
        assert_eq!(project_snapshot(&s, Tier::L0, &[]).nodes.len(), 1);
        assert_eq!(parse_ids(Some("1, 2,x,3")), vec![1, 2, 3]);
    }
}
