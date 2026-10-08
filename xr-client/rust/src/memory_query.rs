//! Memory search from the headset (ADR-2133 client side, ADR-2134 route look).
//!
//! The desktop explorer embeds a query with `POST /api/memory-cloud/query`,
//! runs its own HNSW over the sampled vectors and relays the winning path as
//! a `memoryRoute` frame. The headset never runs HNSW (the Quest CPU budget)
//! and never fetches the vectors blob, so it cannot place the 384-d query
//! vector in the cloud: the snapshot's PCA basis is not on the wire. What the
//! response does carry is the sidecar's own ranked hits, each with the
//! snapshot row it occupies (`sampleIndex`) when it was sampled.
//!
//! Since ADR-2136 the response also carries the query's own point in the
//! snapshot's cloud (`query.position`, the snapshot's PCA basis and scale
//! applied server-side). A headset query is drawn as a **query point →
//! sidecar top-k** route: from that point through the sampled hits in rank
//! order, the answer ring on the top hit, every sampled hit marked in gold.
//! Without a position (an older server) it falls back to the rank-order line
//! through the hits alone, the top hit last. The HUD names it as such
//! ([`crate::memory_route::RouteSource::SidecarTopK`]); it is never presented
//! as a search traversal.
//!
//! Queries are typed on the HUD's on-screen keyboard, and the Query page
//! keeps presets ([`presets`]) as shortcuts: the last few queries run on this
//! headset, a curated set, and one per namespace in the loaded snapshot.
//!
//! Wire types: `crates/visionclaw-memory-cloud/src/wire.rs`
//! (`MemoryCloudQueryRequest`, `MemoryCloudQueryResponse`). Both sides read
//! `tests/fixtures/memory_query_response.json`, so they cannot drift.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use serde::Deserialize;
use thiserror::Error;

use crate::memory_route::{RouteFrame, RouteSource, SidecarStats, Vec3, MAX_PATH, MAX_SIDECAR};

#[cfg(not(test))]
use godot::prelude::*;

/// Hits requested per query (the server's default k).
pub const DEFAULT_K: u32 = 10;
/// The server's ceiling on k (`validate.rs::MAX_K`).
pub const MAX_K: u32 = 50;
/// The server's ceiling on query text (`validate.rs::MAX_QUERY_CHARS`).
pub const MAX_QUERY_CHARS: usize = 2000;
/// Hits a headset query asks for: the server's ceiling. The cloud samples a
/// few thousand rows of a much larger store, so a top-10 rarely has two hits
/// with a point to route through (measured live 2026-10-07: 0–3 of 10 within
/// a namespace, 2–11 of 50); the HUD still lists only [`HUD_HITS`].
pub const HEADSET_K: u32 = MAX_K;
/// Hits shown on the HUD Query page.
pub const HUD_HITS: usize = 8;
/// Namespace questions offered: the largest sampled namespaces.
pub const NAMESPACE_PRESETS: usize = 8;
/// Recent queries remembered on this headset.
pub const RECENT_MAX: usize = 4;
/// Longest key shown on a HUD hit row before it is ellipsised.
pub const HIT_KEY_CHARS: usize = 34;

/// Curated presets: (question, namespace to search within when the loaded
/// snapshot has it). A global query lands in the largest corpus, which the
/// cloud samples thinly, so it rarely has hits to route through; scoped to an
/// estate namespace it does. Absent namespace → searched globally.
pub const CURATED: &[(&str, &str)] = &[
    ("recent architecture decisions", "project-state"),
    ("open bugs and known issues", "project-state"),
    ("lessons and patterns that worked", "patterns"),
    ("agent coordination and swarms", "coordination"),
    ("XR headset rendering and the HUD", "project-state"),
];

/// How the sidecar produced the hits (`SearchMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// The sidecar's HNSW index (what an agent's `memory_search` sees).
    Hnsw,
    /// An exact scan (namespace-restricted queries, or a filtered global one).
    Exact,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Hnsw => "hnsw",
            Method::Exact => "exact",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireHit {
    key: String,
    namespace: String,
    #[serde(default)]
    source_type: String,
    score: f32,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    sample_index: Option<serde_json::Value>,
    /// `MemoryCloudHit::position` (ADR-2136 amendment); absent from older servers.
    #[serde(default)]
    position: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct WireEcho {
    text: String,
    /// `QueryEcho::position` (ADR-2136); absent from older servers.
    #[serde(default)]
    position: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSidecar {
    results: Vec<WireHit>,
    #[serde(default)]
    took_ms: f64,
    method: Method,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireResponse {
    snapshot_id: String,
    #[serde(default)]
    embed_model: String,
    query: WireEcho,
    sidecar: WireSidecar,
}

/// One sidecar hit, rank 1 = nearest.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryHit {
    pub rank: u32,
    pub key: String,
    pub namespace: String,
    pub source_type: String,
    pub score: f32,
    pub snippet: String,
    /// Snapshot row, when the hit was sampled into the cloud.
    pub row: Option<u32>,
    /// The hit's own embedding in the cloud's coordinates, when the server
    /// placed it (sampled or not).
    pub position: Option<[f32; 3]>,
}

/// A parsed `POST /api/memory-cloud/query` response.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryAnswer {
    pub snapshot_id: String,
    pub embed_model: String,
    pub text: String,
    pub method: Method,
    pub took_ms: f64,
    pub hits: Vec<QueryHit>,
    /// The query's point in the snapshot's cloud coordinates, when the server
    /// sent a finite one.
    pub position: Option<[f32; 3]>,
}

impl QueryAnswer {
    /// Placed hits in rank order (point, sampled), at most [`MAX_PATH`].
    pub fn placed(&self) -> Vec<(Vec3, bool)> {
        self.hits
            .iter()
            .filter_map(|h| h.position.map(|p| (p, h.row.is_some())))
            .take(MAX_PATH)
            .collect()
    }

    /// Distinct sampled rows in rank order.
    pub fn sampled_rows(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in self.hits.iter().filter_map(|h| h.row) {
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum QueryError {
    #[error("memory query response is not valid JSON: {0}")]
    Json(String),
    #[error("memory query response has no snapshotId")]
    EmptySnapshot,
    #[error("query text is empty")]
    EmptyText,
    #[error("query text has {0} characters (max {MAX_QUERY_CHARS})")]
    TooLong(usize),
}

/// A finite `[x, y, z]`; anything else is no position (decoration, never an
/// invalid response).
fn as_point(v: &serde_json::Value) -> Option<[f32; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let mut p = [0.0f32; 3];
    for (o, x) in p.iter_mut().zip(a) {
        *o = x.as_f64().map(|f| f as f32).filter(|f| f.is_finite())?;
    }
    Some(p)
}

fn as_row(v: &serde_json::Value) -> Option<u32> {
    let f = v.as_f64()?;
    (f >= 0.0 && f.fract() == 0.0 && f <= u32::MAX as f64).then_some(f as u32)
}

/// Parse a query response. A malformed `sampleIndex` makes that hit
/// unsampled (decoration), never the response invalid.
pub fn parse_query_response(json: &str) -> Result<QueryAnswer, QueryError> {
    let w: WireResponse =
        serde_json::from_str(json).map_err(|e| QueryError::Json(e.to_string()))?;
    if w.snapshot_id.trim().is_empty() {
        return Err(QueryError::EmptySnapshot);
    }
    let hits = w
        .sidecar
        .results
        .into_iter()
        .take(MAX_K as usize)
        .enumerate()
        .map(|(i, h)| QueryHit {
            rank: i as u32 + 1,
            key: h.key,
            namespace: h.namespace,
            source_type: h.source_type,
            score: if h.score.is_finite() { h.score } else { 0.0 },
            snippet: h.snippet,
            row: h.sample_index.as_ref().and_then(as_row),
            position: h.position.as_ref().and_then(as_point),
        })
        .collect();
    Ok(QueryAnswer {
        snapshot_id: w.snapshot_id,
        embed_model: w.embed_model,
        position: w.query.position.as_ref().and_then(as_point),
        text: w.query.text,
        method: w.sidecar.method,
        took_ms: if w.sidecar.took_ms.is_finite() {
            w.sidecar.took_ms
        } else {
            0.0
        },
        hits,
    })
}

/// The route frame a headset query draws. With the query's point: from it
/// through the sampled hits in rank order (top hit first, where the answer
/// ring goes); one sampled hit is enough. Without it: the sampled hits in rank
/// order reversed (the top hit last), which needs two. Every sampled hit is
/// marked. Too few points give an empty path, which clears any previous route.
pub fn sidecar_route(a: &QueryAnswer, sent_at: f64, seq: u64) -> RouteFrame {
    let rows = a.sampled_rows();
    let placed = if a.position.is_some() {
        a.placed()
    } else {
        Vec::new()
    };
    let mut path: Vec<u32> = match a.position {
        Some(_) if !rows.is_empty() => rows.clone(),
        None if rows.len() >= 2 => rows.iter().rev().copied().collect(),
        _ => Vec::new(),
    };
    if path.len() > MAX_PATH {
        // keep the answer end
        if a.position.is_some() {
            path.truncate(MAX_PATH);
        } else {
            path.drain(..path.len() - MAX_PATH);
        }
    }
    if !placed.is_empty() {
        // every placed hit draws (ADR-2136 amendment); `path` keeps the
        // sampled rows lit in the cloud
        path = rows.iter().copied().take(MAX_PATH).collect();
    }
    let origin = a
        .position
        .filter(|_| !path.is_empty() || !placed.is_empty());
    let sidecar: Vec<u32> = rows.iter().take(MAX_SIDECAR).copied().collect();
    let total = u32::try_from(a.hits.len()).ok();
    let stats = SidecarStats::from_wire(total, Some(sidecar.len() as u32), sidecar.len());
    RouteFrame {
        snapshot_id: a.snapshot_id.clone(),
        seq,
        sent_at,
        path,
        sidecar,
        query: a
            .text
            .chars()
            .take(crate::memory_route::MAX_QUERY_CHARS)
            .collect(),
        stats,
        source: RouteSource::SidecarTopK,
        origin,
        hit_points: placed.iter().map(|(p, _)| *p).collect(),
        hit_sampled: placed.iter().map(|(_, s)| *s).collect(),
    }
}

/// `POST /api/memory-cloud/query` body: `{text, k, namespace?}`, trimmed and
/// held to the server's limits so a preset can never earn a 400.
pub fn request_body(text: &str, k: u32, namespace: Option<&str>) -> Result<String, QueryError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(QueryError::EmptyText);
    }
    let chars = text.chars().count();
    if chars > MAX_QUERY_CHARS {
        return Err(QueryError::TooLong(chars));
    }
    let mut body = serde_json::json!({ "text": text, "k": k.clamp(1, MAX_K) });
    if let Some(ns) = namespace.map(str::trim).filter(|ns| !ns.is_empty()) {
        body["namespace"] = serde_json::Value::from(ns);
    }
    Ok(body.to_string())
}

/// One entry of the Query page's preset list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub label: String,
    pub text: String,
    /// Restrict the sidecar search to one namespace.
    pub namespace: Option<String>,
}

impl Preset {
    pub fn query(text: &str, namespace: Option<&str>) -> Preset {
        let label = match namespace {
            Some(ns) if text == namespace_question(ns) => text.to_string(),
            Some(ns) => format!("{text} (in {ns})"),
            None => text.to_string(),
        };
        Preset {
            label,
            text: text.to_string(),
            namespace: namespace.map(str::to_string),
        }
    }

    fn same(&self, text: &str, namespace: Option<&str>) -> bool {
        self.text == text && self.namespace.as_deref() == namespace
    }
}

/// "what does <namespace> hold", searched within that namespace.
pub fn namespace_question(ns: &str) -> String {
    format!("what does {ns} hold")
}

/// The preset list: recent queries first (newest first), then the curated
/// set, then a question for each of the [`NAMESPACE_PRESETS`] largest
/// namespaces in the loaded snapshot. `namespaces` is (name, sampled rows).
/// A namespace needs two sampled rows (a route needs two points) and a name
/// without whitespace (the live store holds stray sentence-length values in
/// that column). No duplicates.
pub fn presets(namespaces: &[(String, usize)], recent: &[Preset]) -> Vec<Preset> {
    let mut out: Vec<Preset> = Vec::new();
    let mut push = |p: Preset| {
        if !out.iter().any(|q| q.same(&p.text, p.namespace.as_deref())) {
            out.push(p);
        }
    };
    for r in recent.iter().take(RECENT_MAX) {
        push(r.clone());
    }
    for (text, ns) in CURATED {
        let scoped = namespaces.iter().any(|(n, _)| n == ns);
        push(Preset::query(text, scoped.then_some(*ns)));
    }
    let mut usable: Vec<&(String, usize)> = namespaces
        .iter()
        .filter(|(ns, n)| *n >= 2 && !ns.is_empty() && !ns.chars().any(char::is_whitespace))
        .collect();
    usable.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (ns, _) in usable.into_iter().take(NAMESPACE_PRESETS) {
        push(Preset::query(&namespace_question(ns), Some(ns)));
    }
    out
}

/// Record a query run on this headset: newest first, no duplicates, at most
/// [`RECENT_MAX`].
pub fn push_recent(recent: &mut Vec<Preset>, text: &str, namespace: Option<&str>) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    recent.retain(|p| !p.same(text, namespace));
    recent.insert(0, Preset::query(text, namespace));
    recent.truncate(RECENT_MAX);
}

/// The Query page caption: what was asked, how the sidecar answered, how many
/// hits have a point in the cloud, and what the drawn route is.
pub fn caption(a: &QueryAnswer) -> String {
    let sampled = a.hits.iter().filter(|h| h.row.is_some()).count();
    let points = a.sampled_rows().len();
    let drawn = a.placed().len();
    let route = match a.position {
        Some(_) if drawn >= 1 => format!(
            "route: query point → sidecar top-k ({drawn} drawn, {sampled} in sample; not a search path)"
        ),
        Some(_) if points >= 1 => "route: query point → sidecar top-k (not a search path)".into(),
        Some(_) => "no route: no hit in the sample".into(),
        None if points >= 2 => "route: sidecar top-k in rank order (not a search path)".into(),
        None => "no route: fewer than 2 hits in the sample".into(),
    };
    let n = a.hits.len();
    format!(
        "{n} {} · {} {} ms · {sampled} of {n} in the sample · {route}",
        if n == 1 { "hit" } else { "hits" },
        a.method.label(),
        a.took_ms.round() as i64,
    )
}

fn ellipsise(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// One HUD hit row: rank, key, namespace, score, and ● when the hit is a
/// sampled point in the cloud, ☐ when it is drawn from its own position
/// outside the sample, — when it could not be placed.
pub fn hit_line(h: &QueryHit) -> String {
    format!(
        "{}. {} · {} · {:.2} {}",
        h.rank,
        ellipsise(&h.key, HIT_KEY_CHARS),
        h.namespace,
        h.score,
        if h.row.is_some() {
            "●"
        } else if h.position.is_some() {
            "☐"
        } else {
            "—"
        }
    )
}

// ── Godot adapter ──

/// Query-page state for the HUD: presets, recent queries, the last answer's
/// hit list. The route itself goes through `MemoryRoute.offer_query_response`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct MemoryQuery {
    recent: Vec<Preset>,
    answer: Option<QueryAnswer>,
    last_error: String,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
fn preset_dict(p: &Preset) -> Dictionary {
    let mut d = Dictionary::new();
    d.set("label", p.label.as_str());
    d.set("text", p.text.as_str());
    d.set("namespace", p.namespace.as_deref().unwrap_or(""));
    d
}

#[cfg(not(test))]
fn ns_opt(ns: &GString) -> Option<String> {
    let s = ns.to_string();
    (!s.trim().is_empty()).then_some(s)
}

#[cfg(not(test))]
#[godot_api]
impl MemoryQuery {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            recent: Vec::new(),
            answer: None,
            last_error: String::new(),
            base,
        })
    }

    /// Presets for the loaded snapshot: `namespaces` with their sampled row
    /// `counts` (parallel; MemoryCloud.namespaces / namespace_row_counts).
    /// Array of Dictionaries {label, text, namespace ("" = all)}.
    #[func]
    fn presets(&self, namespaces: PackedStringArray, counts: PackedInt32Array) -> VariantArray {
        let ns: Vec<(String, usize)> = namespaces
            .as_slice()
            .iter()
            .zip(counts.as_slice())
            .map(|(s, &n)| (s.to_string(), n.max(0) as usize))
            .collect();
        let mut arr = VariantArray::new();
        for p in presets(&ns, &self.recent) {
            arr.push(&preset_dict(&p).to_variant());
        }
        arr
    }

    /// Remember a query run on this headset (newest first).
    #[func]
    fn record_recent(&mut self, text: GString, namespace: GString) {
        let ns = ns_opt(&namespace);
        push_recent(&mut self.recent, &text.to_string(), ns.as_deref());
    }

    /// The recent queries, oldest last, as preset Dictionaries (persisted by
    /// the scene between launches).
    #[func]
    fn recent(&self) -> VariantArray {
        let mut arr = VariantArray::new();
        for p in &self.recent {
            arr.push(&preset_dict(p).to_variant());
        }
        arr
    }

    /// The JSON body for a query, or "" (see `last_error`) when the text is
    /// empty or too long.
    #[func]
    fn request_body(&mut self, text: GString, k: i64, namespace: GString) -> GString {
        let ns = ns_opt(&namespace);
        match request_body(
            &text.to_string(),
            k.clamp(1, MAX_K as i64) as u32,
            ns.as_deref(),
        ) {
            Ok(b) => GString::from(b.as_str()),
            Err(e) => {
                self.last_error = e.to_string();
                GString::new()
            }
        }
    }

    /// Parse a response for the HUD. Returns {ok, error, caption, snapshot_id,
    /// hits: [{rank, key, namespace, score, row (-1 = not sampled), line}]};
    /// at most `HUD_HITS` hits.
    #[func]
    fn ingest_response(&mut self, json: GString) -> Dictionary {
        let mut d = Dictionary::new();
        match parse_query_response(&json.to_string()) {
            Ok(a) => {
                let mut hits = VariantArray::new();
                for h in a.hits.iter().take(HUD_HITS) {
                    let mut hd = Dictionary::new();
                    hd.set("rank", h.rank as i64);
                    hd.set("key", h.key.as_str());
                    hd.set("namespace", h.namespace.as_str());
                    hd.set("score", h.score);
                    hd.set("row", h.row.map_or(-1, |r| r as i64));
                    hd.set("has_position", h.position.is_some());
                    let p = h.position.unwrap_or([0.0; 3]);
                    hd.set("position", Vector3::new(p[0], p[1], p[2]));
                    hd.set("line", hit_line(h).as_str());
                    hits.push(&hd.to_variant());
                }
                d.set("ok", true);
                d.set("error", "");
                d.set("caption", caption(&a).as_str());
                d.set("snapshot_id", a.snapshot_id.as_str());
                d.set("hits", hits);
                self.answer = Some(a);
            }
            Err(e) => {
                self.last_error = e.to_string();
                d.set("ok", false);
                d.set("error", self.last_error.as_str());
                d.set("caption", "");
                d.set("snapshot_id", "");
                d.set("hits", VariantArray::new());
            }
        }
        d
    }

    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.last_error.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_route::{agreement_line, parse_route, RouteDecision, RouteGate};

    fn fixture() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/memory_query_response.json");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }

    fn hit(rank: u32, row: Option<u32>) -> QueryHit {
        QueryHit {
            rank,
            key: format!("k{rank}"),
            namespace: "patterns".into(),
            source_type: String::new(),
            score: 1.0 - rank as f32 * 0.05,
            snippet: String::new(),
            row,
            position: None,
        }
    }

    fn answer(rows: &[Option<u32>]) -> QueryAnswer {
        QueryAnswer {
            snapshot_id: "s".into(),
            embed_model: "m".into(),
            text: "q".into(),
            method: Method::Hnsw,
            took_ms: 3.0,
            hits: rows
                .iter()
                .enumerate()
                .map(|(i, r)| hit(i as u32 + 1, *r))
                .collect(),
            position: None,
        }
    }

    #[test]
    fn parses_the_shared_server_fixture() {
        let a = parse_query_response(&fixture()).unwrap();
        assert_eq!(a.snapshot_id, "snap-7f3a");
        assert_eq!(a.embed_model, "bge-small-en-v1.5");
        assert_eq!(a.text, "how does the headset lay out the graphs");
        assert_eq!(a.method, Method::Hnsw);
        assert_eq!(a.took_ms, 12.5);
        assert_eq!(a.hits.len(), 6);
        assert_eq!(a.hits[0].rank, 1);
        assert_eq!(a.hits[0].key, "adr-2135-separated-layout");
        assert_eq!(a.hits[0].namespace, "project-state");
        assert_eq!(a.hits[0].source_type, "decision");
        assert!((a.hits[0].score - 0.8123).abs() < 1e-6);
        assert_eq!(a.hits[0].row, Some(12));
        assert_eq!(a.hits[1].row, None, "unsampled hit");
        assert_eq!(a.sampled_rows(), vec![12, 4, 27, 31]);
        assert_eq!(a.position, Some([-18.5, 6.25, 31.0]), "the query point");
    }

    #[test]
    fn a_missing_or_bad_position_is_no_position() {
        let base = |pos: &str| {
            format!(
                r#"{{"snapshotId":"s","embedModel":"m","query":{{"text":"q","vector":[]{pos}}},"sidecar":{{"results":[],"tookMs":1,"method":"exact"}}}}"#
            )
        };
        assert_eq!(
            parse_query_response(&base("")).unwrap().position,
            None,
            "older server"
        );
        for bad in [
            r#","position":null"#,
            r#","position":[1,2]"#,
            r#","position":[1,"2",3]"#,
            r#","position":{"x":1}"#,
        ] {
            assert_eq!(
                parse_query_response(&base(bad)).unwrap().position,
                None,
                "{bad}"
            );
        }
        assert_eq!(
            parse_query_response(&base(r#","position":[1,2.5,-3]"#))
                .unwrap()
                .position,
            Some([1.0, 2.5, -3.0])
        );
    }

    #[test]
    fn rejects_what_cannot_be_a_response_and_tolerates_bad_rows() {
        assert!(matches!(
            parse_query_response("nope"),
            Err(QueryError::Json(_))
        ));
        assert!(matches!(
            parse_query_response(r#"{"snapshotId":"s"}"#),
            Err(QueryError::Json(_))
        ));
        let empty = r#"{"snapshotId":" ","embedModel":"m","query":{"text":"q","vector":[]},"sidecar":{"results":[],"tookMs":1,"method":"exact"}}"#;
        assert_eq!(parse_query_response(empty), Err(QueryError::EmptySnapshot));
        let odd = r#"{"snapshotId":"s","embedModel":"m","query":{"text":"q","vector":[]},"sidecar":{"results":[
            {"id":"a","key":"a","namespace":"n","sourceType":"","score":0.9,"snippet":"","sampleIndex":-3},
            {"id":"b","key":"b","namespace":"n","sourceType":"","score":0.8,"snippet":"","sampleIndex":1.5},
            {"id":"c","key":"c","namespace":"n","sourceType":"","score":0.7,"snippet":"","sampleIndex":"7"},
            {"id":"d","key":"d","namespace":"n","sourceType":"","score":0.6,"snippet":"","sampleIndex":7}
        ],"tookMs":2,"method":"exact"}}"#;
        let a = parse_query_response(odd).unwrap();
        assert_eq!(a.method, Method::Exact);
        let rows: Vec<Option<u32>> = a.hits.iter().map(|h| h.row).collect();
        assert_eq!(rows, vec![None, None, None, Some(7)]);
    }

    #[test]
    fn every_placed_hit_draws_from_the_query_point_in_rank_order() {
        // the fixture: six hits, four sampled, five placed (hit 2 placed
        // outside the sample, hit 6 not placed)
        let a = parse_query_response(&fixture()).unwrap();
        assert_eq!(a.hits[1].row, None);
        assert_eq!(
            a.hits[1].position,
            Some([-61.0, 12.75, 40.5]),
            "unsampled but placed"
        );
        assert_eq!(a.hits[5].position, None);
        let f = sidecar_route(&a, 1000.0, 3);
        assert_eq!(f.origin, Some([-18.5, 6.25, 31.0]));
        assert_eq!(
            f.hit_points,
            vec![
                [22.5, -4.0, 9.25],
                [-61.0, 12.75, 40.5],
                [3.5, 3.5, -18.0],
                [14.0, -27.5, 6.0],
                [-8.25, 19.0, -2.5]
            ],
            "rank order, unplaced hit skipped"
        );
        assert_eq!(f.hit_sampled, vec![true, false, true, true, true]);
        assert_eq!(f.anchors(), 6);
        assert_eq!(f.path, vec![12, 4, 27, 31], "sampled rows stay lit");
        assert_eq!(
            agreement_line(Some(&f)),
            "Route: query point → sidecar top-k (5 drawn, 4 in sample)",
        );
        assert_eq!(
            caption(&a),
            "6 hits · hnsw 13 ms · 4 of 6 in the sample · route: query point → sidecar top-k (5 drawn, 4 in sample; not a search path)"
        );
        assert!(
            hit_line(&a.hits[1]).ends_with("☐"),
            "drawn outside the sample"
        );
        assert!(hit_line(&a.hits[5]).ends_with("—"), "not placed");
        // a global query with no sampled hit still draws (the live failure)
        let mut global = answer(&[None, None, None]);
        global.position = Some([0.0; 3]);
        for (i, h) in global.hits.iter_mut().enumerate() {
            h.position = Some([i as f32, 1.0, 2.0]);
        }
        let g = sidecar_route(&global, 1.0, 1);
        assert_eq!((g.anchors(), g.path.len()), (4, 0));
        assert!(caption(&global).contains("(3 drawn, 0 in sample;"));
        // an older server (no hit positions) keeps the sampled-row route
        let mut old = parse_query_response(&fixture()).unwrap();
        for h in &mut old.hits {
            h.position = None;
        }
        let f = sidecar_route(&old, 1000.0, 3);
        assert!(f.hit_points.is_empty());
        assert_eq!(f.path, vec![12, 4, 27, 31], "rank order, top hit first");
        assert_eq!(f.sidecar, vec![12, 4, 27, 31], "every sampled hit marked");
        assert_eq!(
            agreement_line(Some(&f)),
            "Route: query point → sidecar top-k · 4 of 6 sidecar hits are in the sample",
        );
        assert!(caption(&old).ends_with("route: query point → sidecar top-k (not a search path)"));
        // one sampled hit is enough once the query point is known
        let mut one = answer(&[Some(5), None]);
        one.position = Some([1.0, 2.0, 3.0]);
        let f1 = sidecar_route(&one, 1.0, 1);
        assert_eq!(
            (f1.path.clone(), f1.origin),
            (vec![5], Some([1.0, 2.0, 3.0]))
        );
        assert_eq!(f1.anchors(), 2);
        // no sampled hit: nothing to draw, and no stray origin
        let mut none = answer(&[None]);
        none.position = Some([1.0, 2.0, 3.0]);
        let f0 = sidecar_route(&none, 1.0, 1);
        assert!(f0.path.is_empty() && f0.origin.is_none());
        assert!(caption(&none).ends_with("no route: no hit in the sample"));
    }

    #[test]
    fn without_a_query_point_the_route_ends_on_the_top_hit() {
        let mut a = parse_query_response(&fixture()).unwrap();
        a.position = None;
        let f = sidecar_route(&a, 1000.0, 3);
        assert_eq!(f.origin, None);
        assert_eq!(
            f.path,
            vec![31, 27, 4, 12],
            "rank order reversed: top hit last"
        );
        assert_eq!(f.sidecar, vec![12, 4, 27, 31], "every sampled hit marked");
        assert_eq!(f.snapshot_id, "snap-7f3a");
        assert_eq!((f.sent_at, f.seq), (1000.0, 3));
        assert_eq!(f.source, RouteSource::SidecarTopK);
        let s = f.stats.unwrap();
        assert_eq!((s.total, s.in_sample), (6, 4));
        assert_eq!(
            agreement_line(Some(&f)),
            "Route: sidecar top-k in rank order · 4 of 6 sidecar hits are in the sample",
            "never claims a local top-k agreement"
        );
    }

    #[test]
    fn duplicate_rows_collapse_and_short_answers_clear() {
        let f = sidecar_route(&answer(&[Some(5), Some(5), Some(2)]), 1.0, 1);
        assert_eq!(f.path, vec![2, 5]);
        let one = sidecar_route(&answer(&[Some(5), None, None]), 1.0, 1);
        assert!(one.path.is_empty(), "one sampled hit cannot draw a route");
        assert_eq!(one.sidecar, vec![5]);
        assert_eq!(agreement_line(Some(&one)), "");
        let none = sidecar_route(&answer(&[None, None]), 1.0, 1);
        assert!(none.path.is_empty() && none.sidecar.is_empty());
    }

    #[test]
    fn a_long_answer_keeps_the_top_hit_end() {
        let rows: Vec<Option<u32>> = (0..(MAX_PATH as u32 + 10)).map(Some).collect();
        let mut a = answer(&rows);
        a.hits.truncate(rows.len());
        let f = sidecar_route(&a, 1.0, 1);
        assert_eq!(f.path.len(), MAX_PATH);
        assert_eq!(*f.path.last().unwrap(), 0, "the top hit stays the answer");
        a.position = Some([0.0; 3]);
        let g = sidecar_route(&a, 1.0, 1);
        assert_eq!(g.path.len(), MAX_PATH);
        assert_eq!(g.path[0], 0, "from the query point, the top hit first");
    }

    #[test]
    fn the_route_goes_through_the_same_gate_as_a_relay() {
        let a = parse_query_response(&fixture()).unwrap();
        let mut g = RouteGate::default();
        match g.offer(sidecar_route(&a, 10.0, 1), Some("snap-7f3a"), 40) {
            RouteDecision::Apply(f) => assert_eq!(f.path.len(), 4),
            d => panic!("{d:?}"),
        }
        // another snapshot loaded: reload once, like a relayed frame
        assert_eq!(
            g.offer(sidecar_route(&a, 11.0, 2), Some("other"), 40),
            RouteDecision::Reload
        );
        // a relayed desktop frame sent later still wins over the headset's
        let relay = parse_route(
            r#"{"type":"memoryRoute","snapshotId":"snap-7f3a","seq":1,"sentAt":20,"path":[1,2]}"#,
        )
        .unwrap();
        assert!(matches!(
            g.after_reload(Some("snap-7f3a"), 40),
            Some(RouteDecision::Apply(_))
        ));
        assert!(matches!(
            g.offer(relay, Some("snap-7f3a"), 40),
            RouteDecision::Apply(_)
        ));
    }

    #[test]
    fn request_body_matches_the_server_request() {
        let b: serde_json::Value =
            serde_json::from_str(&request_body("  graph layout ", 10, None).unwrap()).unwrap();
        assert_eq!(b, serde_json::json!({"text": "graph layout", "k": 10}));
        let b: serde_json::Value = serde_json::from_str(
            &request_body("what does patterns hold", 99, Some("patterns")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            b,
            serde_json::json!({"text": "what does patterns hold", "k": 50, "namespace": "patterns"})
        );
        assert_eq!(request_body(" \n", 10, None), Err(QueryError::EmptyText));
        let long = "x".repeat(MAX_QUERY_CHARS + 1);
        assert_eq!(
            request_body(&long, 10, None),
            Err(QueryError::TooLong(MAX_QUERY_CHARS + 1))
        );
        // `k` 0 is clamped, never sent out of range
        let b: serde_json::Value =
            serde_json::from_str(&request_body("q", 0, Some(" ")).unwrap()).unwrap();
        assert_eq!(b, serde_json::json!({"text": "q", "k": 1}));
    }

    /// The headset's limits are the server's (`validate.rs`).
    #[test]
    fn limits_match_the_server_validation() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/visionclaw-memory-cloud/src/validate.rs");
        let src = std::fs::read_to_string(&p).unwrap();
        for (name, ours) in [
            ("MAX_QUERY_CHARS", MAX_QUERY_CHARS),
            ("DEFAULT_K", DEFAULT_K as usize),
            ("MAX_K", MAX_K as usize),
        ] {
            let line = src
                .lines()
                .find(|l| l.starts_with(&format!("pub const {name}: usize = ")))
                .unwrap_or_else(|| panic!("{name} not in validate.rs"));
            let v: usize = line
                .rsplit('=')
                .next()
                .unwrap()
                .trim()
                .trim_end_matches(';')
                .parse()
                .unwrap();
            assert_eq!(v, ours, "{name} drifted from validate.rs");
        }
    }

    fn ns(list: &[(&str, usize)]) -> Vec<(String, usize)> {
        list.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    #[test]
    fn presets_put_recent_first_then_curated_then_namespaces() {
        let mut recent = Vec::new();
        push_recent(&mut recent, "first", None);
        push_recent(&mut recent, "what does patterns hold", Some("patterns"));
        let p = presets(&ns(&[("project-state", 9), ("patterns", 4)]), &recent);
        assert_eq!(p[0].text, "what does patterns hold", "newest recent first");
        assert_eq!(p[0].namespace.as_deref(), Some("patterns"));
        assert_eq!(p[1].text, "first");
        assert_eq!(p[2].text, CURATED[0].0);
        assert_eq!(
            p.len(),
            2 + CURATED.len() + 1,
            "the recent namespace question is not repeated"
        );
        let last = p.last().unwrap();
        assert_eq!(last.text, "what does project-state hold");
        assert_eq!(last.namespace.as_deref(), Some("project-state"));
        assert_eq!(last.label, "what does project-state hold");
        assert_eq!(Preset::query("x", Some("n")).label, "x (in n)");
    }

    #[test]
    fn curated_presets_search_their_namespace_only_when_it_is_loaded() {
        let p = presets(&ns(&[("project-state", 9)]), &[]);
        let decisions = p
            .iter()
            .find(|p| p.text == "recent architecture decisions")
            .unwrap();
        assert_eq!(decisions.namespace.as_deref(), Some("project-state"));
        assert_eq!(
            decisions.label,
            "recent architecture decisions (in project-state)"
        );
        let swarms = p
            .iter()
            .find(|p| p.text == "agent coordination and swarms")
            .unwrap();
        assert_eq!(
            swarms.namespace, None,
            "no coordination namespace: searched globally"
        );
    }

    #[test]
    fn namespace_questions_take_the_largest_usable_namespaces() {
        let names: Vec<String> = (0..12).map(|i| format!("ns{i:02}")).collect();
        let mut list: Vec<(String, usize)> = vec![
            ("tiny".into(), 1),
            ("Batch 1.1 COMPLETE. 9 files migrated".into(), 400),
            (String::new(), 300),
        ];
        for (i, n) in names.iter().enumerate() {
            list.push((n.clone(), 100 - i));
        }
        let p = presets(&list, &[]);
        let qs: Vec<&str> = p[CURATED.len()..]
            .iter()
            .map(|p| p.namespace.as_deref().unwrap())
            .collect();
        assert_eq!(qs.len(), NAMESPACE_PRESETS, "capped");
        let want: Vec<&str> = names[..NAMESPACE_PRESETS]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(qs, want, "largest first");
        assert!(
            !p.iter().any(|p| p.text.contains("tiny")),
            "one sampled row cannot route"
        );
        assert!(
            !p.iter().any(|p| p.text.contains("Batch")),
            "not a namespace name"
        );
    }

    #[test]
    fn recent_is_newest_first_deduplicated_and_capped() {
        let mut r = Vec::new();
        for q in ["a", "b", "a", "c", "d", "e", "  "] {
            push_recent(&mut r, q, None);
        }
        let texts: Vec<&str> = r.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, vec!["e", "d", "c", "a"]);
        push_recent(&mut r, "e", Some("ns"));
        assert_eq!(
            r[0].namespace.as_deref(),
            Some("ns"),
            "same text, other namespace: separate entry"
        );
        assert_eq!(r.len(), RECENT_MAX);
    }

    #[test]
    fn hud_lines_say_what_the_route_is() {
        let a = parse_query_response(&fixture()).unwrap();
        assert_eq!(
            caption(&a),
            "6 hits · hnsw 13 ms · 4 of 6 in the sample · route: query point → sidecar top-k (5 drawn, 4 in sample; not a search path)"
        );
        let mut old = a.clone();
        old.position = None;
        for h in &mut old.hits {
            h.position = None;
        }
        assert_eq!(
            caption(&old),
            "6 hits · hnsw 13 ms · 4 of 6 in the sample · route: sidecar top-k in rank order (not a search path)"
        );
        assert_eq!(
            caption(&answer(&[Some(1), None])),
            "2 hits · hnsw 3 ms · 1 of 2 in the sample · no route: fewer than 2 hits in the sample"
        );
        assert_eq!(
            hit_line(&a.hits[0]),
            "1. adr-2135-separated-layout · project-state · 0.81 ●"
        );
        assert_eq!(
            hit_line(&a.hits[1]),
            "2. xr-hud-532px-host · patterns · 0.77 ☐"
        );
        let mut long = hit(3, None);
        long.key = "k".repeat(60);
        assert!(hit_line(&long).contains(&format!("{}…", "k".repeat(HIT_KEY_CHARS - 1))));
    }
}
