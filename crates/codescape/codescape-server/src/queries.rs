//! Execute a typed [`Command`] against a repository's graph.
//!
//! Every path into this module (typed, spoken, Loom) arrives as a validated
//! `Command`. Graph commands answer with highlighted nodes and edges; view
//! commands (filters, zoom, follow, timeline) are echoed back in
//! `QueryResult.command` as the command's JSON for the client to apply, with a
//! short spoken acknowledgement. Nothing here dispatches work to agents:
//! `ask` needs a signed intent from the speaker's browser (P6).

use std::collections::{HashSet, VecDeque};

use codescape_collab::{fuzzy_rank, Command, Entry, Target};
use codescape_ingest::Ingestor;
use codescape_model::events::{PresenceKind, QueryResult};
use codescape_model::{GraphSnapshot, NodeKind, Relation};

use crate::engine::RepoEngine;

/// Most nodes a relation walk returns.
pub const MAX_WALK_NODES: usize = 500;
/// Candidates offered for disambiguation.
pub const CANDIDATES: usize = 3;

/// Per-client command context.
#[derive(Debug, Clone, Default)]
pub struct CommandCtx {
    /// The issuing actor.
    pub actor: u32,
    /// Its current focus.
    pub focus: Option<u32>,
    /// Candidates from the last name lookup (for "number two").
    pub candidates: Vec<u32>,
    /// Previous focuses (for `back`).
    pub history: Vec<u32>,
}

/// A side effect the caller applies (view focus, presence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing.
    None,
    /// Move the issuing client's focus.
    Focus(u32),
}

fn describe(s: &GraphSnapshot, idx: &std::collections::HashMap<u32, usize>, id: u32) -> String {
    let Some(n) = idx.get(&id).map(|&i| &s.nodes[i]) else {
        return format!("node {id}");
    };
    match (&n.path, n.span, &n.kind) {
        (Some(p), Some(sp), k) if *k != NodeKind::File => {
            format!("{} {} in {}:{}", n.kind.as_str(), n.label, p, sp.start_line)
        }
        (Some(p), _, NodeKind::File) => format!("file {p}"),
        _ => format!("{} {}", n.kind.as_str(), n.label),
    }
}

/// Search scores below this are noise (observed: real matches ≥ 0.33, unrelated
/// phrases ≈ 0.03), so a name that only matches below it is "not found".
pub const MIN_MATCH_SCORE: f32 = 0.15;

/// Score margin within which a callable candidate beats a better-scored non-callable one.
pub const CALLABLE_MARGIN: f32 = 0.1;

fn resolve(
    ing: &Ingestor,
    t: &Target,
    ctx: &mut CommandCtx,
    s: &GraphSnapshot,
) -> Result<(u32, Vec<(String, u32)>), String> {
    resolve_pref(ing, t, ctx, s, false)
}

/// Resolve a target. With `callable`, a function or method within [`CALLABLE_MARGIN`]
/// of the best match wins (`who calls stable match` means the function, not the struct).
/// Search depth when a callable is preferred: import stubs (`external`) and
/// same-named types tie with the function, so the top [`CANDIDATES`] can miss it
/// on a large repository.
pub const CALLABLE_POOL: usize = 12;

/// Move the best-ranked callable within [`CALLABLE_MARGIN`] of the top score to the
/// front, keeping the rest in rank order. `hits` is ranked best first and non-empty.
pub fn prefer_callable(
    mut hits: Vec<(u32, f32)>,
    is_callable: impl Fn(u32) -> bool,
) -> Vec<(u32, f32)> {
    let top = hits.first().map_or(0.0, |h| h.1);
    if let Some(pos) = hits
        .iter()
        .position(|&(id, score)| score >= top - CALLABLE_MARGIN && is_callable(id))
    {
        let h = hits.remove(pos);
        hits.insert(0, h);
    }
    hits
}

fn resolve_pref(
    ing: &Ingestor,
    t: &Target,
    ctx: &mut CommandCtx,
    s: &GraphSnapshot,
    callable: bool,
) -> Result<(u32, Vec<(String, u32)>), String> {
    match t {
        Target::Name(name) => {
            let pool = if callable { CALLABLE_POOL } else { CANDIDATES };
            let hits: Vec<(u32, f32)> = ing
                .search(name, pool)
                .into_iter()
                .filter(|h| h.1 >= MIN_MATCH_SCORE)
                .collect();
            if hits.is_empty() {
                return Err(format!("I could not find {name}."));
            }
            let idx = s.index();
            let mut hits = if callable {
                prefer_callable(hits, |id| {
                    idx.get(&id).is_some_and(|&i| {
                        matches!(s.nodes[i].kind, NodeKind::Function | NodeKind::Method)
                    })
                })
            } else {
                hits
            };
            hits.truncate(CANDIDATES);
            let best = hits[0].0;
            ctx.candidates = hits.iter().map(|h| h.0).collect();
            let cands = hits
                .iter()
                .map(|&(id, _)| {
                    (
                        idx.get(&id)
                            .map(|&i| s.nodes[i].label.to_string())
                            .unwrap_or_default(),
                        id,
                    )
                })
                .collect();
            Ok((best, cands))
        }
        Target::Candidate(k) => ctx
            .candidates
            .get((*k as usize).saturating_sub(1))
            .map(|&id| (id, vec![]))
            .ok_or_else(|| format!("There is no candidate number {k}.")),
        Target::This => ctx
            .focus
            .map(|f| (f, vec![]))
            .ok_or_else(|| "Nothing is focused yet.".to_string()),
    }
}

/// BFS over `calls` edges in one direction. Returns nodes (excluding the start) and edge pairs as (caller, callee).
fn walk_calls(
    ing: &Ingestor,
    start: u32,
    depth: u32,
    callers: bool,
) -> (Vec<u32>, Vec<(u32, u32)>) {
    let mut seen: HashSet<u32> = [start].into_iter().collect();
    let mut q: VecDeque<(u32, u32)> = [(start, 0)].into_iter().collect();
    let mut nodes = vec![];
    let mut edges = vec![];
    while let Some((n, d)) = q.pop_front() {
        if d >= depth {
            continue;
        }
        for (m, _) in ing.neighbors(n, Some(&[Relation::Calls]), callers, !callers) {
            edges.push(if callers { (m, n) } else { (n, m) });
            if seen.insert(m) {
                nodes.push(m);
                if nodes.len() >= MAX_WALK_NODES {
                    return (nodes, edges);
                }
                q.push_back((m, d + 1));
            }
        }
    }
    (nodes, edges)
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn verb(kind: PresenceKind) -> &'static str {
    match kind {
        PresenceKind::Query => "read",
        PresenceKind::Update => "edited",
        PresenceKind::Create => "created",
        PresenceKind::Delete => "deleted",
        PresenceKind::Say => "said",
        PresenceKind::Focus => "looked at",
    }
}

/// Run `cmd`. Holds the ingest lock for graph commands only.
pub fn execute(
    repo: &RepoEngine,
    cmd: &Command,
    ctx: &mut CommandCtx,
    now_ms: u64,
) -> (QueryResult, Effect) {
    let mut out = QueryResult {
        command: serde_json::to_string(cmd).unwrap_or_default(),
        ..Default::default()
    };
    let published = repo.current();
    let s = &published.snapshot;
    let idx = &published.index;
    let mut effect = Effect::None;

    let graph = |ctx: &mut CommandCtx,
                 out: &mut QueryResult,
                 f: &mut dyn FnMut(
        &Ingestor,
        &mut CommandCtx,
        &mut QueryResult,
    ) -> Result<(), String>| {
        let ing = repo.ingest.read();
        if let Err(e) = f(&ing, ctx, out) {
            out.answer = e;
        }
    };

    match cmd {
        Command::Focus(t) => graph(ctx, &mut out, &mut |ing, ctx, out| {
            let (id, cands) = resolve(ing, t, ctx, s)?;
            out.node_ids = vec![id];
            out.answer = format!("Focused {}.", describe(s, idx, id));
            if cands.len() > 1 {
                out.answer.push_str(&format!(
                    " Also: {}.",
                    cands[1..]
                        .iter()
                        .enumerate()
                        .map(|(i, c)| format!("number {} {}", i + 2, c.0))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            out.candidates = cands;
            effect = Effect::Focus(id);
            Ok(())
        }),
        Command::Explain(t) => graph(ctx, &mut out, &mut |ing, ctx, out| {
            let (id, cands) = resolve(ing, t, ctx, s)?;
            out.candidates = cands;
            out.node_ids = vec![id];
            let ex = ing
                .explain(id, 1500)
                .map_err(|e| format!("I could not read that: {e}"))?;
            out.answer = ex.summary;
            effect = Effect::Focus(id);
            Ok(())
        }),
        Command::Callers { target, depth } | Command::Callees { target, depth } => {
            let callers = matches!(cmd, Command::Callers { .. });
            graph(ctx, &mut out, &mut |ing, ctx, out| {
                let (id, cands) = resolve_pref(ing, target, ctx, s, true)?;
                out.candidates = cands;
                let (nodes, edges) = walk_calls(ing, id, *depth, callers);
                out.answer = format!(
                    "{} {} {}.",
                    count(
                        nodes.len(),
                        if callers { "caller" } else { "callee" },
                        if callers { "callers" } else { "callees" }
                    ),
                    if callers { "reach" } else { "reached from" },
                    describe(s, idx, id)
                );
                out.node_ids = std::iter::once(id).chain(nodes).collect();
                out.edge_pairs = edges;
                effect = Effect::Focus(id);
                Ok(())
            })
        }
        Command::Affected { target, depth } => graph(ctx, &mut out, &mut |ing, ctx, out| {
            let (id, cands) = resolve(ing, target, ctx, s)?;
            out.candidates = cands;
            let hits = ing.affected(id, *depth);
            out.answer = format!(
                "{} within {} of {}.",
                count(hits.len(), "node depends on it", "nodes depend on it"),
                count(*depth as usize, "hop", "hops"),
                describe(s, idx, id)
            );
            out.node_ids = std::iter::once(id)
                .chain(hits.into_iter().map(|h| h.0).take(MAX_WALK_NODES))
                .collect();
            effect = Effect::Focus(id);
            Ok(())
        }),
        Command::Path { from, to } => graph(ctx, &mut out, &mut |ing, ctx, out| {
            let (a, _) = resolve(ing, from, ctx, s)?;
            let (b, _) = resolve(ing, to, ctx, s)?;
            match ing.shortest_path(a, b) {
                Some(p) => {
                    out.edge_pairs = p.windows(2).map(|w| (w[0], w[1])).collect();
                    out.answer = format!(
                        "{} from {} to {}.",
                        count(p.len().saturating_sub(1), "hop", "hops"),
                        describe(s, idx, a),
                        describe(s, idx, b)
                    );
                    out.node_ids = p.clone();
                    out.path = p;
                }
                None => {
                    out.answer = format!(
                        "No path from {} to {}.",
                        describe(s, idx, a),
                        describe(s, idx, b)
                    )
                }
            }
            Ok(())
        }),
        Command::Back => {
            ctx.history.pop();
            match ctx.history.pop() {
                Some(prev) => {
                    out.node_ids = vec![prev];
                    out.answer = format!("Back to {}.", describe(s, idx, prev));
                    effect = Effect::Focus(prev);
                }
                None => out.answer = "Nothing to go back to.".into(),
            }
        }
        Command::Rewind { seconds } => {
            let from = now_ms.saturating_sub(seconds.saturating_mul(1000));
            let c = repo.collab.lock();
            let window = c.timeline.window(from, now_ms);
            let mut nodes: Vec<u32> = vec![];
            let mut seen = HashSet::new();
            let mut actors = HashSet::new();
            let mut events = 0usize;
            let mut revisions = 0usize;
            for e in window {
                match &e.entry {
                    Entry::Presence(p) => {
                        events += 1;
                        actors.insert(p.actor);
                        for &n in &p.node_ids {
                            if seen.insert(n) {
                                nodes.push(n);
                            }
                        }
                    }
                    Entry::Revision(_) => revisions += 1,
                }
            }
            out.answer = format!(
                "In the last {}: {} by {}, touching {}, and {}.",
                count(*seconds as usize, "second", "seconds"),
                count(events, "event", "events"),
                count(actors.len(), "actor", "actors"),
                count(nodes.len(), "node", "nodes"),
                count(revisions, "revision", "revisions")
            );
            out.node_ids = nodes;
        }
        Command::WhatIsDoing { actor } => {
            let c = repo.collab.lock();
            let all: Vec<_> = c.registry.actors().cloned().collect();
            let chosen: Vec<_> = match actor {
                None => all,
                Some(name) => {
                    let ranked = fuzzy_rank(name, all.iter().map(|a| (a.id, a.label.as_str())), 1);
                    all.into_iter()
                        .filter(|a| ranked.first().is_some_and(|r| r.0 == a.id))
                        .collect()
                }
            };
            if chosen.is_empty() {
                out.answer = match actor {
                    Some(n) => format!("I have not seen {n}."),
                    None => "Nobody has been active.".into(),
                };
            } else {
                let mut parts = vec![];
                for a in chosen.iter().take(5) {
                    let recent = c.registry.recent(a.id, 1);
                    match recent.first() {
                        Some(ev) => {
                            let ago = now_ms.saturating_sub(ev.ts_ms) / 1000;
                            let what = ev
                                .paths
                                .first()
                                .cloned()
                                .or_else(|| ev.node_ids.first().map(|&n| describe(s, idx, n)))
                                .or_else(|| ev.text.clone())
                                .unwrap_or_default();
                            // Prompt text is withheld unless --share-agent-text, so a
                            // `Say` may carry nothing to quote.
                            parts.push(if what.is_empty() {
                                let did = if ev.kind == PresenceKind::Say {
                                    "sent a message"
                                } else {
                                    verb(ev.kind)
                                };
                                format!("{} {did} {ago} seconds ago", a.label)
                            } else {
                                format!("{} {} {what} {ago} seconds ago", a.label, verb(ev.kind))
                            });
                        }
                        None => parts.push(format!("{} has been quiet", a.label)),
                    }
                    if let Some(f) = a.focus {
                        out.node_ids.push(f);
                    }
                }
                out.answer = format!("{}.", parts.join("; "));
            }
        }
        Command::Ask { actor, .. } => {
            out.answer = format!("Asking {actor} needs a signed intent from your browser, which is not enabled yet. Nothing was sent.");
        }
        Command::Help => {
            out.answer = "Try: focus a name, explain this, who calls this, what does this affect, path from a to b, show only rust, hide tests, zoom out, rewind five minutes, what is everyone doing, follow an agent.".into();
        }
        Command::ShowOnly { lang, community } => {
            out.answer = match (lang, community) {
                (Some(l), Some(c)) => format!("Showing only {} in community {c}.", l.as_str()),
                (Some(l), None) => format!("Showing only {}.", l.as_str()),
                (None, Some(c)) => format!("Showing only community {c}."),
                (None, None) => "Showing everything.".into(),
            };
        }
        Command::IsolateCommunity(c) => {
            let c = c.or_else(|| {
                ctx.focus
                    .and_then(|f| idx.get(&f).map(|&i| s.nodes[i].community))
            });
            match c {
                Some(c) => {
                    out.node_ids = s
                        .nodes
                        .iter()
                        .filter(|n| n.community == c)
                        .map(|n| n.id)
                        .take(MAX_WALK_NODES)
                        .collect();
                    out.answer = format!("Isolating community {c}.");
                }
                None => out.answer = "Focus something first, or name a community number.".into(),
            }
        }
        Command::ShowAll => out.answer = "Showing everything.".into(),
        Command::HideTests => out.answer = "Hiding tests.".into(),
        Command::ZoomIn => out.answer = "Zooming in.".into(),
        Command::ZoomOut => out.answer = "Zooming out.".into(),
        Command::Overview => out.answer = "Overview.".into(),
        Command::Replay => out.answer = "Replaying the timeline.".into(),
        Command::Live => out.answer = "Back to live.".into(),
        Command::Follow { actor } => out.answer = format!("Following {actor}."),
        Command::StopFollowing => out.answer = "Stopped following.".into(),
    }
    if let Effect::Focus(id) = effect {
        ctx.focus = Some(id);
        if ctx.history.last() != Some(&id) {
            ctx.history.push(id);
            if ctx.history.len() > 64 {
                ctx.history.remove(0);
            }
        }
    }
    (out, effect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live visionclaw ranking for "stable match": the function is fourth,
    /// behind a struct and two import stubs with tied scores.
    #[test]
    fn callable_beyond_the_first_three_still_wins() {
        let hits = vec![
            (6706, 0.583),
            (26638, 0.583),
            (26500, 0.538),
            (2650, 0.538),
            (77, 0.4),
        ];
        let out = prefer_callable(hits, |id| id == 2650);
        assert_eq!(
            out.iter().map(|h| h.0).collect::<Vec<_>>(),
            vec![2650, 6706, 26638, 26500, 77]
        );
    }

    #[test]
    fn callable_outside_the_margin_does_not_win() {
        let hits = vec![(1, 0.9), (2, 0.5)];
        assert_eq!(prefer_callable(hits.clone(), |id| id == 2), hits);
        assert_eq!(prefer_callable(hits.clone(), |_| false), hits);
    }
}
