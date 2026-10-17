//! The engine side of `:4101` presence ingest.
//!
//! For each event: rewrite paths through the [`PathMap`](codescape_collab::PathMap),
//! assign each to the repository whose root contains it (else by a known
//! relative path, else by suffix match), resolve to file ids, record presence
//! (heat, timeline, `/ws/events`), emit `0x23` actions for `/wss`, and queue
//! edited paths for re-extraction so freshness does not depend on inotify
//! crossing a bind mount.

use std::collections::BTreeMap;
use std::sync::Arc;

use actix_web::web;
use codescape_collab::resolve_paths;
use codescape_model::events::{PresenceEvent, PresenceKind};
use codescape_model::wire::agent_action_frame;
use codescape_server::ingest_api::{presence_text, AcceptReport, ActivityBody, ActivitySink};
use codescape_server::now_ms;
use codescape_server::xr::action_for;

use crate::engine::RepoEngine;
use crate::state::AppState;

/// How long an agent action beam is shown, in ms.
pub const ACTION_DURATION_MS: u16 = 1500;
/// Future timestamps beyond this skew are replaced by server time.
pub const MAX_CLOCK_SKEW_MS: u64 = 60_000;

/// Whether a hook path may be resolved and queued: a regular file, or a path that
/// no longer exists but is a file the graph knows (a deletion). Directories,
/// unknown vanished paths and `..` components are refused.
pub fn accept_path(rel: &str, abs: &std::path::Path, known_file: bool) -> bool {
    if std::path::Path::new(rel)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    match std::fs::metadata(abs) {
        Ok(m) => m.is_file(),
        Err(_) => known_file,
    }
}

/// Sink wired to the running engines.
pub struct EngineSink {
    st: web::Data<AppState>,
}

impl EngineSink {
    /// A sink over the app state.
    pub fn new(st: web::Data<AppState>) -> Self {
        Self { st }
    }

    /// Group mapped paths by repository, as repo-relative paths.
    fn assign(&self, mapped: &[String]) -> BTreeMap<usize, Vec<String>> {
        let mut out: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        'paths: for p in mapped {
            if p.starts_with('/') {
                for (i, r) in self.st.repos.iter().enumerate() {
                    if let Some(rel) = r.relative(p) {
                        out.entry(i).or_default().push(rel);
                        continue 'paths;
                    }
                }
                // Not under any root: suffix-match against known files.
                for (i, r) in self.st.repos.iter().enumerate() {
                    let pubd = r.current();
                    if let Some(&id) = resolve_paths(&pubd.files, std::slice::from_ref(p)).first() {
                        if let Some(path) = pubd
                            .index
                            .get(&id)
                            .and_then(|&k| pubd.snapshot.nodes[k].path.clone())
                        {
                            out.entry(i).or_default().push(path.to_string());
                            continue 'paths;
                        }
                    }
                }
                tracing::debug!(path = %p, "activity path outside every repository");
            } else {
                let rel = p.trim_start_matches("./").to_string();
                let hit = self
                    .st
                    .repos
                    .iter()
                    .position(|r| r.current().files.contains_key(rel.as_str()));
                // A relative path no repo knows yet (a new file) goes to the first repo.
                out.entry(hit.unwrap_or(0)).or_default().push(rel);
            }
        }
        out
    }

    fn record(
        &self,
        repo: &Arc<RepoEngine>,
        body: &ActivityBody,
        actor: u32,
        rels: Vec<String>,
        ts: u64,
    ) -> (usize, usize) {
        let pubd = repo.current();
        // Only files: a directory would make apply_changed re-extract a whole subtree.
        let rels: Vec<String> = rels
            .into_iter()
            .filter(|r| {
                let keep = accept_path(r, &repo.absolute(r), pubd.files.contains_key(r.as_str()));
                if !keep {
                    tracing::debug!(path = %r, "activity path dropped: not a file");
                }
                keep
            })
            .collect();
        let ids = resolve_paths(&pubd.files, &rels);
        let mut kind = body.presence_kind();
        if kind == PresenceKind::Create && !ids.is_empty() {
            kind = PresenceKind::Update;
        }
        if !repo.admit_actor(
            actor,
            &body.actor,
            codescape_collab::ActorKind::from_wire(&body.kind),
            ts,
        ) {
            tracing::warn!(repo = %repo.label, "actor registry full of active actors; activity dropped");
            return (0, 0);
        }
        // Prompts and answers stay private until NIP-98 (`--share-agent-text` restores them).
        let text = presence_text(body, self.st.cfg.share_agent_text);
        let queued = if matches!(
            kind,
            PresenceKind::Update | PresenceKind::Create | PresenceKind::Delete
        ) && !rels.is_empty()
        {
            let abs: Vec<_> = rels.iter().map(|r| repo.absolute(r)).collect();
            let n = abs.len();
            repo.changes.push(abs);
            n
        } else {
            0
        };
        if let Some(action) = action_for(kind).filter(|_| !ids.is_empty()) {
            let evs: Vec<_> = ids
                .iter()
                .map(|&n| (actor, n, action, ts as u32, ACTION_DURATION_MS))
                .collect();
            let _ = repo.actions.send(Arc::new(agent_action_frame(&evs)));
        }
        let n = ids.len();
        repo.record_presence(PresenceEvent {
            actor,
            actor_label: body.actor.clone(),
            actor_kind: body.kind.clone(),
            kind,
            node_ids: ids,
            paths: rels,
            text,
            ts_ms: ts,
        });
        (n, queued)
    }
}

impl ActivitySink for EngineSink {
    fn accept(&self, body: ActivityBody) -> AcceptReport {
        let now = now_ms();
        let ts = body
            .ts_ms
            .filter(|&t| t <= now + MAX_CLOCK_SKEW_MS)
            .unwrap_or(now);
        // Authenticated by the ingest token: no per-address allocation cap.
        let actor = match self.st.actor_id(&body.actor, None) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %e, "activity dropped");
                return AcceptReport::default();
            }
        };
        let mapped: Vec<String> = body
            .paths
            .iter()
            .map(|p| self.st.path_map.rewrite(p))
            .collect();
        let mut report = AcceptReport::default();
        let groups = if mapped.is_empty() {
            // Prompts and stops carry no paths: the actor is present in every repo.
            (0..self.st.repos.len()).map(|i| (i, vec![])).collect()
        } else {
            self.assign(&mapped)
        };
        for (i, rels) in groups {
            let repo = self.st.repos[i].clone();
            report.repo.get_or_insert_with(|| repo.label.clone());
            let (n, q) = self.record(&repo, &body, actor, rels, ts);
            report.nodes += n;
            report.queued_paths += q;
        }
        tracing::debug!(actor = %body.actor, event = %body.event, nodes = report.nodes, "activity ingested");
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_and_known_deletions_are_accepted() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/a.rs"), "fn a() {}").unwrap();
        assert!(accept_path("src/a.rs", &d.path().join("src/a.rs"), false));
        assert!(
            !accept_path("src", &d.path().join("src"), false),
            "a directory is refused"
        );
        assert!(!accept_path("src", &d.path().join("src"), true));
        assert!(
            accept_path("src/gone.rs", &d.path().join("src/gone.rs"), true),
            "a known file that vanished is a deletion"
        );
        assert!(
            !accept_path("src/never.rs", &d.path().join("src/never.rs"), false),
            "an unknown vanished path (e.g. a deleted directory) is refused"
        );
        assert!(!accept_path(
            "../etc/passwd",
            &d.path().join("../etc/passwd"),
            false
        ));
    }
}
