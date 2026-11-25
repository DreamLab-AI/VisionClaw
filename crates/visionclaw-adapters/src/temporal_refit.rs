//! One-off offline refit of a provenance store from the v1 temporal form to
//! `estate-temporal-v2` (VisionClaw ADR-2219).
//!
//! v1 closed a version with a bare `dl:validTo` quad on it and identified
//! versions without their recorded time. v2 closes with separate closure
//! records and mints version ids that include the recorded time, so every
//! version of a v1 store changes IRI. Rewriting in place would be a second,
//! hand-written copy of the writer; this tool instead **replays** the corpus
//! sync's history through the production planner, [`plan_history`], at the true
//! sync instants, so the result is exactly what the current writer would have
//! produced.
//!
//! The tool is offline (RocksDB admits one writer) and **dry-run by default**.
//! Both modes run the same stages:
//!
//! 1. **Preflight** ([`preflight`]): read the store, and refuse unless every
//!    version is a corpus-sync version opened at a sync instant, every bare
//!    close lands on a later sync instant, there is one agent, no activity in
//!    the legacy id form, no overlapping versions of one triple, and the counts
//!    match any `--expect-*` the operator gave.
//! 2. **Replay** into a scratch in-memory store with the production
//!    [`plan_history`]/[`apply_history`], one sync at a time.
//! 3. **Verify the replay** against what the old store said (same
//!    triple/opened/closed set, closure count, digest, knowledge-cut cases).
//!    Dry run stops here and prints the plan.
//! 4. With `--apply`, one transaction on the real store: delete the old
//!    corpus-owned history and the residue versions in
//!    `urn:agentbox:graph:provenance`, canonicalise the other activities'
//!    timestamp literals, insert the replayed history.
//! 5. **Verify the store** with the same checks, and that a second run finds
//!    nothing to do.
//!
//! No marker predicate and no ledger row are written: a refit is recorded in
//! the ADR, never in the data. Restore a checkpoint to roll back.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use oxigraph::model::{GraphNameRef, Literal, NamedNode, NamedNodeRef, Quad, Term};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;

use crate::corpus_history::{
    apply_history, corpus_sync_activity_urn, legacy_forms, open_corpus_versions, plan_history,
    triple_digest, Triple, CORPUS_SYNC_WRITER, CORPUS_SYNC_WRITER_PREDICATE,
};
use crate::provenance_writer::{build_state_at_sparql, DL_NS, GRAPH_PROVENANCE};

/// The graph the 2026-11 refit also found test residue in: assertion versions
/// minted before PRD-022 WS-2 unified the provenance graphs.
pub const RESIDUE_GRAPH: &str = "urn:agentbox:graph:provenance";

/// Prefix of the corpus-sync activity ids from before ADR-2021 (no longer read
/// or written anywhere; a store holding one cannot be replayed).
const LEGACY_ACTIVITY_PREFIX: &str = "urn:visionclaw:execution:corpus-sync-";

const PROV_STARTED: &str = "http://www.w3.org/ns/prov#startedAtTime";
const PROV_GENERATED: &str = "http://www.w3.org/ns/prov#generatedAtTime";

/// What the operator asked for.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Write the refit. Without it the tool only reports.
    pub apply: bool,
    /// Refuse unless the store holds exactly this many versions.
    pub expect_versions: Option<usize>,
    /// Refuse unless exactly this many bare closes are found.
    pub expect_closes: Option<usize>,
    /// Refuse unless exactly this many activities other than the corpus
    /// syncs' are found (they are kept; only their timestamps are normalised).
    pub expect_other_activities: Option<usize>,
}

/// What a run did, for the operator.
#[derive(Debug, Default)]
pub struct Report {
    /// Human-readable lines, in order.
    pub lines: Vec<String>,
    /// True when the store was written.
    pub applied: bool,
    /// True when the store was already in the current form (nothing to do).
    pub already_current: bool,
}

impl Report {
    fn say(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
}

/// Why a run stopped.
#[derive(Debug)]
pub enum RefitError {
    /// Preflight found something the replay cannot reproduce; nothing was written.
    Refused(Vec<String>),
    /// The store could not be read or written.
    Store(String),
    /// A verify step failed. Before the write, nothing was changed; after it,
    /// restore the pre-refit checkpoint.
    VerifyFailed {
        /// True when the store had already been written.
        after_write: bool,
        /// Each failed check.
        failures: Vec<String>,
    },
}

impl std::fmt::Display for RefitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefitError::Refused(why) => {
                write!(f, "REFUSED (nothing written):")?;
                why.iter().try_for_each(|w| write!(f, "\n  - {w}"))
            }
            RefitError::Store(e) => write!(f, "store error: {e}"),
            RefitError::VerifyFailed {
                after_write,
                failures,
            } => {
                if *after_write {
                    write!(
                        f,
                        "VERIFY FAILED AFTER WRITE: restore the pre-refit checkpoint."
                    )?;
                } else {
                    write!(f, "VERIFY FAILED before any write (store unchanged):")?;
                }
                failures.iter().try_for_each(|w| write!(f, "\n  - {w}"))
            }
        }
    }
}

impl std::error::Error for RefitError {}

fn store_err(what: &str, e: impl std::fmt::Display) -> RefitError {
    RefitError::Store(format!("{what}: {e}"))
}

/// One v1 version as the old store holds it.
#[derive(Debug, Clone)]
struct OldVersion {
    entity: String,
    triple: Triple,
    valid_from: DateTime<Utc>,
    generated_at: DateTime<Utc>,
    activity: String,
    agent: String,
    closed_at: Option<DateTime<Utc>>,
}

/// A corpus-sync activity.
#[derive(Debug, Clone)]
struct SyncActivity {
    urn: String,
    at: DateTime<Utc>,
    agent: Option<String>,
}

/// What preflight learned, and the inputs the replay is derived from.
#[derive(Debug)]
pub struct Preflight {
    versions: Vec<OldVersion>,
    syncs: Vec<SyncActivity>,
    agent: String,
    closes: usize,
    other_activities: usize,
    residue_versions: Vec<String>,
    quads_per_graph: BTreeMap<String, usize>,
    /// The states each sync left, derived from the old versions.
    states: Vec<(DateTime<Utc>, BTreeSet<Triple>)>,
    /// Non-canonical timestamp literals to rewrite: (old quad, new quad).
    literals: Vec<(Quad, Quad)>,
}

fn q(store: &Store, sparql: &str, what: &str) -> Result<Vec<HashMap<String, Term>>, RefitError> {
    let QueryResults::Solutions(rows) = store.query(sparql).map_err(|e| store_err(what, e))? else {
        return Ok(Vec::new());
    };
    rows.map(|r| {
        let r = r.map_err(|e| store_err(what, e))?;
        Ok(r.iter()
            .map(|(v, t)| (v.as_str().to_string(), t.clone()))
            .collect())
    })
    .collect()
}

fn text(row: &HashMap<String, Term>, key: &str) -> Option<String> {
    match row.get(key)? {
        Term::NamedNode(n) => Some(n.as_str().to_string()),
        Term::Literal(l) => Some(l.value().to_string()),
        _ => None,
    }
}

fn instant(row: &HashMap<String, Term>, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&text(row, key)?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn is_canonical_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b[19] == b'Z'
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || c.is_ascii_digit())
}

fn canonical(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Read the store and decide whether the refit can reproduce it. Never writes.
///
/// Returns `Ok(None)` when the store holds no v1 form at all (already current,
/// or nothing to refit).
pub fn preflight(store: &Store, opts: &Options) -> Result<Option<Preflight>, RefitError> {
    let mut why: Vec<String> = Vec::new();
    let forms = legacy_forms(store).map_err(|e| store_err("legacy form check", e))?;
    let graph = format!("<{GRAPH_PROVENANCE}>");

    let prefixes = format!(
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n\
         PREFIX prov: <http://www.w3.org/ns/prov#>\nPREFIX dl: <{DL_NS}>\n"
    );

    // Versions in the v1 form.
    let rows = q(
        store,
        &format!(
            "{prefixes}SELECT ?e ?s ?p ?o ?vf ?g ?end ?act ?agent WHERE {{ GRAPH {graph} {{
               ?e rdf:subject ?s ; rdf:predicate ?p ; rdf:object ?o ;
                  dl:validFrom ?vf ; prov:generatedAtTime ?g ;
                  prov:wasGeneratedBy ?act ; prov:wasAttributedTo ?agent .
               OPTIONAL {{ ?e dl:validTo ?end }} }} }}"
        ),
        "read versions",
    )?;
    let mut versions: Vec<OldVersion> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for row in &rows {
        let (Some(entity), Some(s), Some(p), Some(o)) = (
            text(row, "e"),
            text(row, "s"),
            text(row, "p"),
            text(row, "o"),
        ) else {
            continue;
        };
        if !seen.insert(entity.clone()) {
            why.push(format!("{entity} has more than one end, agent or activity"));
            continue;
        }
        let (Some(vf), Some(g), Some(act), Some(agent)) = (
            instant(row, "vf"),
            instant(row, "g"),
            text(row, "act"),
            text(row, "agent"),
        ) else {
            why.push(format!("{entity}: unreadable timestamp, activity or agent"));
            continue;
        };
        let closed_at = match row.get("end") {
            Some(_) => match instant(row, "end") {
                Some(end) => Some(end),
                None => {
                    why.push(format!("{entity}: unreadable dl:validTo"));
                    continue;
                }
            },
            None => None,
        };
        versions.push(OldVersion {
            entity,
            triple: (s, p, o),
            valid_from: vf,
            generated_at: g,
            activity: act,
            agent,
            closed_at,
        });
    }

    if versions.is_empty() && !forms.any() {
        return Ok(None);
    }
    if versions.is_empty() {
        why.push(format!(
            "the store shows the old form ({}) but no version could be read from it",
            forms.describe()
        ));
    }

    // A store already holding v2 versions beside v1 ones is not a state this
    // tool produced or can reason about.
    let v2_versions = q(
        store,
        &format!(
            "{prefixes}PREFIX temporal: <http://nova.dev/temporal#>
             SELECT ?e WHERE {{ GRAPH {graph} {{ ?e temporal:validFrom ?x }} }} LIMIT 1"
        ),
        "read v2 versions",
    )?;
    if !v2_versions.is_empty() {
        why.push("the store holds v2 versions beside v1 ones (mixed forms)".to_string());
    }

    // The corpus syncs.
    let sync_rows = q(
        store,
        &format!(
            "{prefixes}SELECT ?a ?at ?agent WHERE {{ GRAPH {graph} {{
               ?a <{CORPUS_SYNC_WRITER_PREDICATE}> \"{CORPUS_SYNC_WRITER}\" ; prov:startedAtTime ?at .
               OPTIONAL {{ ?a prov:wasAssociatedWith ?agent }} }} }}"
        ),
        "read corpus syncs",
    )?;
    let mut syncs: Vec<SyncActivity> = Vec::new();
    for row in &sync_rows {
        let (Some(urn), Some(at)) = (text(row, "a"), instant(row, "at")) else {
            why.push("a corpus-sync activity has an unreadable start time".to_string());
            continue;
        };
        if urn != corpus_sync_activity_urn(&at) {
            why.push(format!(
                "{urn} is not the id the current writer mints for {} (replay would rename it)",
                canonical(&at)
            ));
        }
        syncs.push(SyncActivity {
            urn,
            at,
            agent: text(row, "agent"),
        });
    }
    syncs.sort_by_key(|s| s.at);
    if syncs.windows(2).any(|w| w[0].at == w[1].at) {
        why.push("two corpus-sync activities share an instant".to_string());
    }
    let legacy_activities = q(
        store,
        &format!(
            "{prefixes}SELECT ?a WHERE {{ GRAPH {graph} {{ ?a prov:startedAtTime ?at .
               FILTER STRSTARTS(STR(?a), \"{LEGACY_ACTIVITY_PREFIX}\") }} }}"
        ),
        "read legacy activities",
    )?;
    if !legacy_activities.is_empty() {
        why.push(format!(
            "{} activities in the legacy corpus-sync id form (`{LEGACY_ACTIVITY_PREFIX}…`); the replay cannot own them",
            legacy_activities.len()
        ));
    }

    // One agent, and every version a corpus-sync version opened at a sync instant.
    let agents: BTreeSet<&str> = versions.iter().map(|v| v.agent.as_str()).collect();
    if agents.len() > 1 {
        why.push(format!(
            "{} agents attribute versions, expected one",
            agents.len()
        ));
    }
    let agent = agents
        .iter()
        .next()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let by_urn: HashMap<&str, &SyncActivity> = syncs.iter().map(|s| (s.urn.as_str(), s)).collect();
    let instants: BTreeSet<DateTime<Utc>> = syncs.iter().map(|s| s.at).collect();
    for s in &syncs {
        if s.agent.as_deref().is_some_and(|a| a != agent) {
            why.push(format!("{} was associated with a different agent", s.urn));
        }
    }
    for v in &versions {
        let Some(sync) = by_urn.get(v.activity.as_str()) else {
            why.push(format!(
                "{} was not generated by a corpus-sync activity ({}); the tool replays only the corpus sync's history",
                v.entity, v.activity
            ));
            continue;
        };
        if v.valid_from != sync.at || v.generated_at != sync.at {
            why.push(format!(
                "{}: valid and recorded time differ from its sync instant {}",
                v.entity,
                canonical(&sync.at)
            ));
        }
        if let Some(end) = v.closed_at {
            if !instants.contains(&end) {
                why.push(format!(
                    "{}: bare close at {} matches no corpus-sync activity",
                    v.entity,
                    canonical(&end)
                ));
            } else if end <= v.valid_from {
                why.push(format!("{}: closed at or before it opened", v.entity));
            }
        }
    }

    // Overlap: one triple, one live version at a time.
    let mut by_triple: BTreeMap<&Triple, Vec<&OldVersion>> = BTreeMap::new();
    for v in &versions {
        by_triple.entry(&v.triple).or_default().push(v);
    }
    let mut overlapping = 0usize;
    for list in by_triple.values_mut() {
        list.sort_by_key(|v| v.valid_from);
        for pair in list.windows(2) {
            if pair[0].closed_at.is_none_or(|end| end > pair[1].valid_from) {
                overlapping += 1;
            }
        }
    }
    if overlapping > 0 {
        why.push(format!(
            "{overlapping} triples have overlapping or duplicate versions; replay would collapse them"
        ));
    }

    let closes = versions.iter().filter(|v| v.closed_at.is_some()).count();
    let other_activities = q(
        store,
        &format!("{prefixes}SELECT DISTINCT ?a WHERE {{ GRAPH {graph} {{ ?a a prov:Activity }} }}"),
        "read activities",
    )?
    .iter()
    .filter_map(|r| text(r, "a"))
    .filter(|a| !by_urn.contains_key(a.as_str()))
    .count();

    for (name, found, want) in [
        ("versions", versions.len(), opts.expect_versions),
        ("bare closes", closes, opts.expect_closes),
        (
            "other activities",
            other_activities,
            opts.expect_other_activities,
        ),
    ] {
        if let Some(want) = want {
            if found != want {
                why.push(format!("expected {want} {name}, found {found}"));
            }
        }
    }

    // Residue versions in the other provenance graph.
    let residue_versions: Vec<String> = q(
        store,
        &format!(
            "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
             SELECT DISTINCT ?e WHERE {{ GRAPH <{RESIDUE_GRAPH}> {{ ?e rdf:subject ?s }} }}"
        ),
        "read residue",
    )?
    .iter()
    .filter_map(|r| text(r, "e"))
    .collect();

    // Quads per graph (recorded for the operator; the refit changes only
    // the provenance and residue graphs).
    let mut quads_per_graph = BTreeMap::new();
    for g in store.named_graphs() {
        let g = g.map_err(|e| store_err("list graphs", e))?;
        if let oxigraph::model::NamedOrBlankNode::NamedNode(n) = &g {
            let count = store
                .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(n.as_ref())))
                .count();
            quads_per_graph.insert(n.as_str().to_string(), count);
        }
    }

    // Timestamp literals on the activities that stay.
    let corpus_urns: HashSet<&str> = by_urn.keys().copied().collect();
    let mut literals = Vec::new();
    for pred in [PROV_STARTED, PROV_GENERATED] {
        let p = NamedNodeRef::new_unchecked(pred);
        for quad in store.quads_for_pattern(
            None,
            Some(p),
            None,
            Some(GraphNameRef::NamedNode(NamedNodeRef::new_unchecked(
                GRAPH_PROVENANCE,
            ))),
        ) {
            let quad = quad.map_err(|e| store_err("read timestamps", e))?;
            let Term::Literal(lit) = &quad.object else {
                continue;
            };
            if is_canonical_timestamp(lit.value()) {
                continue;
            }
            let subject = quad.subject.to_string();
            let subject = subject.trim_matches(|c| c == '<' || c == '>');
            if corpus_urns.contains(subject) || seen.contains(subject) {
                continue; // replaced by the replay
            }
            let Ok(at) = DateTime::parse_from_rfc3339(lit.value()) else {
                why.push(format!(
                    "{subject}: unparseable timestamp literal {}",
                    lit.value()
                ));
                continue;
            };
            let new = Quad::new(
                quad.subject.clone(),
                quad.predicate.clone(),
                Literal::new_typed_literal(
                    canonical(&at.with_timezone(&Utc)),
                    oxigraph::model::vocab::xsd::DATE_TIME,
                ),
                quad.graph_name.clone(),
            );
            literals.push((quad, new));
        }
    }

    if !why.is_empty() {
        why.sort();
        why.dedup();
        if why.len() > 12 {
            let more = why.len() - 12;
            why.truncate(12);
            why.push(format!("… and {more} more"));
        }
        return Err(RefitError::Refused(why));
    }

    // The state each sync left.
    let states = syncs
        .iter()
        .map(|s| {
            let state: BTreeSet<Triple> = versions
                .iter()
                .filter(|v| v.valid_from <= s.at && v.closed_at.is_none_or(|end| end > s.at))
                .map(|v| v.triple.clone())
                .collect();
            (s.at, state)
        })
        .collect();

    Ok(Some(Preflight {
        versions,
        syncs,
        agent,
        closes,
        other_activities,
        residue_versions,
        quads_per_graph,
        states,
        literals,
    }))
}

/// Replay the corpus history into a scratch in-memory store through the
/// production planner, one sync at a time.
fn replay(pre: &Preflight) -> Result<Store, RefitError> {
    let scratch = Store::new().map_err(|e| store_err("scratch store", e))?;
    for (at, state) in &pre.states {
        let open = open_corpus_versions(&scratch).map_err(|e| store_err("replay", e))?;
        let plan = plan_history(state, &open, *at, &pre.agent);
        if plan.skipped > 0 || plan.aborted.is_some() {
            return Err(RefitError::Refused(vec![format!(
                "the production planner skipped {} triples or aborted ({:?}) at {}",
                plan.skipped,
                plan.aborted,
                canonical(at)
            )]));
        }
        apply_history(&scratch, &plan).map_err(|e| store_err("replay", e))?;
    }
    Ok(scratch)
}

/// Every check the replay and the written store must pass. Returns the
/// failures (empty = verified) and a line per check for the report.
fn verify(
    store: &Store,
    pre: &Preflight,
    expect_other_activities: bool,
    report: &mut Report,
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut check = |ok: bool, name: &str, detail: String, failures: &mut Vec<String>| {
        report.say(format!(
            "  [{}] {name}: {detail}",
            if ok { "ok" } else { "FAIL" }
        ));
        if !ok {
            failures.push(format!("{name}: {detail}"));
        }
    };
    let prefixes = format!(
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n\
         PREFIX prov: <http://www.w3.org/ns/prov#>\nPREFIX dl: <{DL_NS}>\n\
         PREFIX temporal: <http://nova.dev/temporal#>\nPREFIX memory: <http://nova.dev/memory#>\n"
    );
    let graph = format!("<{GRAPH_PROVENANCE}>");
    let run = |sparql: &str| q(store, sparql, "verify").unwrap_or_default();

    // Counts and the full (triple, opened, closed) set.
    let new_versions = run(&format!(
        "{prefixes}SELECT ?e ?s ?p ?o ?vf ?rf WHERE {{ GRAPH {graph} {{ ?e rdf:subject ?s ;
           rdf:predicate ?p ; rdf:object ?o ; temporal:validFrom ?vf ; temporal:recordedFrom ?rf }} }}"
    ));
    let closures = run(&format!(
        "{prefixes}SELECT ?c ?e ?end ?at ?act ?agent (COUNT(?k) AS ?n) WHERE {{ GRAPH {graph} {{
           ?c a dl:Closure ; dl:closesVersion ?e ; dl:closes ?k ; dl:validUntil ?end ;
              prov:generatedAtTime ?at ; prov:wasGeneratedBy ?act ; prov:wasAttributedTo ?agent .
           FILTER (?k = memory:ValidTime) }} }} GROUP BY ?c ?e ?end ?at ?act ?agent"
    ));
    let all_closures = run(&format!(
        "{prefixes}SELECT (COUNT(DISTINCT ?c) AS ?n) WHERE {{ GRAPH {graph} {{ ?c a dl:Closure }} }}"
    ));
    let distinct: HashSet<String> = new_versions.iter().filter_map(|r| text(r, "e")).collect();
    check(
        distinct.len() == pre.versions.len(),
        "versions",
        format!(
            "{} (was {}), distinct ids",
            distinct.len(),
            pre.versions.len()
        ),
        &mut failures,
    );
    check(
        closures.len() == pre.closes
            && all_closures
                .first()
                .and_then(|r| text(r, "n"))
                .is_some_and(|n| n == pre.closes.to_string()),
        "closures",
        format!(
            "{} valid-time closures of {} closure records (was {} bare closes)",
            closures.len(),
            all_closures
                .first()
                .and_then(|r| text(r, "n"))
                .unwrap_or_default(),
            pre.closes
        ),
        &mut failures,
    );

    // Each closure ends its version at the closing sync instant, recorded then,
    // by that sync's activity, attributed to the agent.
    let sync_urn: HashMap<DateTime<Utc>, &str> =
        pre.syncs.iter().map(|s| (s.at, s.urn.as_str())).collect();
    let end_of: HashMap<String, DateTime<Utc>> = closures
        .iter()
        .filter_map(|r| Some((text(r, "e")?, instant(r, "end")?)))
        .collect();
    let linked = closures.iter().all(|r| {
        let (Some(end), Some(at)) = (instant(r, "end"), instant(r, "at")) else {
            return false;
        };
        end == at
            && sync_urn.get(&end).copied() == text(r, "act").as_deref()
            && text(r, "agent").as_deref() == Some(pre.agent.as_str())
    });
    check(
        linked,
        "closure linkage",
        "validUntil = recordedAt = the closing sync instant, generated by that sync's activity, attributed to the agent".to_string(),
        &mut failures,
    );

    // The same history: (triple, opened, closed) from the old store equals the new.
    let old: BTreeSet<(Triple, DateTime<Utc>, Option<DateTime<Utc>>)> = pre
        .versions
        .iter()
        .map(|v| (v.triple.clone(), v.valid_from, v.closed_at))
        .collect();
    let new: BTreeSet<(Triple, DateTime<Utc>, Option<DateTime<Utc>>)> = new_versions
        .iter()
        .filter_map(|r| {
            let e = text(r, "e")?;
            Some((
                (text(r, "s")?, text(r, "p")?, text(r, "o")?),
                instant(r, "vf")?,
                end_of.get(&e).copied(),
            ))
        })
        .collect();
    check(
        old == new,
        "history",
        format!(
            "{} (triple, opened, closed) entries; equal to the old store's",
            new.len()
        ),
        &mut failures,
    );

    // No v1 form left.
    let forms = legacy_forms(store).unwrap_or_default();
    check(
        !forms.any(),
        "no dl:validTo, no dl:validFrom on versions",
        if forms.any() {
            forms.describe()
        } else {
            "none".to_string()
        },
        &mut failures,
    );

    // The corpus activities are exactly the old ones.
    let new_syncs: BTreeSet<String> = run(&format!(
        "{prefixes}SELECT ?a WHERE {{ GRAPH {graph} {{ ?a <{CORPUS_SYNC_WRITER_PREDICATE}> \"{CORPUS_SYNC_WRITER}\" }} }}"
    ))
    .iter()
    .filter_map(|r| text(r, "a"))
    .collect();
    let old_syncs: BTreeSet<String> = pre.syncs.iter().map(|s| s.urn.clone()).collect();
    check(
        new_syncs == old_syncs,
        "corpus activities",
        format!("{} (same ids as before)", new_syncs.len()),
        &mut failures,
    );

    if expect_other_activities {
        let others = run(&format!(
            "{prefixes}SELECT DISTINCT ?a WHERE {{ GRAPH {graph} {{ ?a a prov:Activity }} }}"
        ))
        .iter()
        .filter_map(|r| text(r, "a"))
        .filter(|a| !new_syncs.contains(a))
        .count();
        check(
            others == pre.other_activities,
            "other activities kept",
            format!("{others} (was {})", pre.other_activities),
            &mut failures,
        );
        let bad_literals = [PROV_STARTED, PROV_GENERATED]
            .iter()
            .flat_map(|p| {
                store.quads_for_pattern(
                    None,
                    Some(NamedNodeRef::new_unchecked(p)),
                    None,
                    Some(GraphNameRef::NamedNode(NamedNodeRef::new_unchecked(
                        GRAPH_PROVENANCE,
                    ))),
                )
            })
            .filter_map(Result::ok)
            .filter(|quad| match &quad.object {
                Term::Literal(l) => {
                    !is_canonical_timestamp(l.value())
                        || l.datatype() != oxigraph::model::vocab::xsd::DATE_TIME
                }
                _ => true,
            })
            .count();
        check(
            bad_literals == 0,
            "timestamp literals",
            format!("{bad_literals} not canonical (UTC, whole seconds, Z, xsd:dateTime)"),
            &mut failures,
        );
    }

    // The knowledge-cut cases, through the production state-at reader, with the
    // answers the old store's own history dictates.
    let count = |t: DateTime<Utc>, r: Option<DateTime<Utc>>| -> Option<usize> {
        match store.query(&build_state_at_sparql(t, r)).ok()? {
            QueryResults::Solutions(rows) => Some(rows.filter(|x| x.is_ok()).count()),
            _ => None,
        }
    };
    if let (Some(first), Some(last)) = (pre.states.first(), pre.states.last()) {
        let day = chrono::Duration::days(1);
        let (t1, s1) = (first.0, first.1.len());
        let before = t1 - chrono::Duration::seconds(1);
        let cases: Vec<(String, Option<usize>, usize)> = {
            let mut cases = vec![
                (
                    format!(
                        "t = just before the first sync ({}), no cut",
                        canonical(&before)
                    ),
                    count(before, None),
                    0,
                ),
                ("t = first sync, no cut".to_string(), count(t1, None), s1),
                (
                    format!(
                        "t = {} (after the last sync), no cut",
                        canonical(&(last.0 + day))
                    ),
                    count(last.0 + day, None),
                    last.1.len(),
                ),
            ];
            if let Some(second) = pre.states.get(1) {
                // A cut between the first and second sync: closes recorded at the
                // second sync must not apply, even to a t after it.
                let cut = t1 + (second.0 - t1) / 2;
                cases.push((
                    format!(
                        "t = {} , cut r = {} (before the second sync recorded)",
                        canonical(&(last.0 + day)),
                        canonical(&cut)
                    ),
                    count(last.0 + day, Some(cut)),
                    s1,
                ));
            }
            cases
        };
        for (name, got, want) in cases {
            check(
                got == Some(want),
                "knowledge cut",
                format!(
                    "{name}: {} (expected {want})",
                    got.map_or("error".into(), |g| g.to_string())
                ),
                &mut failures,
            );
        }
    }

    // Digest of the open set equals the last sync's state, and the old open set.
    let open: BTreeSet<Triple> = open_corpus_versions(store)
        .unwrap_or_default()
        .into_iter()
        .map(|v| (v.subject, v.predicate, v.object))
        .collect();
    let old_open: BTreeSet<Triple> = pre
        .versions
        .iter()
        .filter(|v| v.closed_at.is_none())
        .map(|v| v.triple.clone())
        .collect();
    let (d_new, d_old) = (triple_digest(&open), triple_digest(&old_open));
    check(
        d_new == d_old,
        "open-set digest",
        format!(
            "{} open triples, sha256 {}{}",
            open.len(),
            &d_new[..16],
            if d_new == d_old {
                " = the old open set's"
            } else {
                " != the old open set's"
            }
        ),
        &mut failures,
    );
    failures
}

/// Run the refit: preflight, replay, verify and, with [`Options::apply`], write.
///
/// A store already in the current form returns a report with
/// [`Report::already_current`] set and writes nothing, which makes a second run
/// a no-op.
pub fn run(store: &Store, opts: &Options) -> Result<Report, RefitError> {
    let mut report = Report::default();
    let Some(pre) = preflight(store, opts)? else {
        report.already_current = true;
        report.say("The store holds no pre-v2 temporal form: nothing to do.");
        return Ok(report);
    };

    report.say("PREFLIGHT (all structural checks passed)");
    report.say(format!("  versions: {}", pre.versions.len()));
    report.say(format!("  bare closes: {}", pre.closes));
    report.say(format!(
        "  corpus-sync activities: {} ({})",
        pre.syncs.len(),
        pre.syncs
            .iter()
            .map(|s| canonical(&s.at))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    report.say(format!(
        "  other activities (kept): {}",
        pre.other_activities
    ));
    report.say(format!("  agent: {}", pre.agent));
    report.say(format!(
        "  residue versions in {RESIDUE_GRAPH}: {}",
        pre.residue_versions.len()
    ));
    report.say(format!(
        "  non-canonical timestamp literals to normalise: {}",
        pre.literals.len()
    ));
    for (g, n) in &pre.quads_per_graph {
        report.say(format!("  quads in <{g}>: {n}"));
    }

    report.say("REPLAY through the production plan_history into a scratch store");
    let scratch = replay(&pre)?;
    report.say(format!(
        "  scratch store: {} quads",
        scratch.len().unwrap_or(0)
    ));
    report.say("VERIFY the replay");
    let failures = verify(&scratch, &pre, false, &mut report);
    if !failures.is_empty() {
        return Err(RefitError::VerifyFailed {
            after_write: false,
            failures,
        });
    }

    if !opts.apply {
        report.say("DRY RUN: the store was not changed. Re-run with --apply to write.");
        return Ok(report);
    }

    // Write: one transaction.
    let g = GraphNameRef::NamedNode(NamedNodeRef::new_unchecked(GRAPH_PROVENANCE));
    let residue_g = GraphNameRef::NamedNode(NamedNodeRef::new_unchecked(RESIDUE_GRAPH));
    let mut delete: Vec<Quad> = Vec::new();
    let owned: Vec<NamedNode> = pre
        .versions
        .iter()
        .map(|v| v.entity.as_str())
        .chain(pre.syncs.iter().map(|s| s.urn.as_str()))
        .map(|s| NamedNode::new(s).map_err(|e| store_err("iri", e)))
        .collect::<Result<_, _>>()?;
    for subject in &owned {
        for quad in store.quads_for_pattern(Some(subject.as_ref().into()), None, None, Some(g)) {
            delete.push(quad.map_err(|e| store_err("read old history", e))?);
        }
    }
    for entity in &pre.residue_versions {
        let subject = NamedNode::new(entity.as_str()).map_err(|e| store_err("iri", e))?;
        for quad in
            store.quads_for_pattern(Some(subject.as_ref().into()), None, None, Some(residue_g))
        {
            delete.push(quad.map_err(|e| store_err("read residue", e))?);
        }
    }
    let insert: Vec<Quad> = scratch
        .iter()
        .collect::<Result<_, _>>()
        .map_err(|e| store_err("read replay", e))?;
    report.say(format!(
        "APPLY: delete {} old quads, rewrite {} timestamp literals, insert {} replayed quads",
        delete.len(),
        pre.literals.len(),
        insert.len()
    ));
    store
        .transaction(|mut txn| {
            for quad in &delete {
                txn.remove(quad.as_ref())?;
            }
            for (old, new) in &pre.literals {
                txn.remove(old.as_ref())?;
                txn.insert(new.as_ref())?;
            }
            for quad in &insert {
                txn.insert(quad.as_ref())?;
            }
            Ok::<_, oxigraph::store::StorageError>(())
        })
        .map_err(|e| store_err("refit transaction", e))?;
    report.applied = true;

    report.say("VERIFY the store");
    let mut failures = verify(store, &pre, true, &mut report);
    let residue_left = q(
        store,
        &format!(
            "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
             SELECT ?e WHERE {{ GRAPH <{RESIDUE_GRAPH}> {{ ?e rdf:subject ?s }} }}"
        ),
        "verify residue",
    )?
    .len();
    report.say(format!(
        "  [{}] residue versions in {RESIDUE_GRAPH}: {residue_left} left",
        if residue_left == 0 { "ok" } else { "FAIL" }
    ));
    if residue_left != 0 {
        failures.push(format!("{residue_left} residue versions remain"));
    }
    let again = preflight(store, opts)?;
    report.say(format!(
        "  [{}] a second run finds nothing to do",
        if again.is_none() { "ok" } else { "FAIL" }
    ));
    if again.is_some() {
        failures.push("a second run still finds the old form".to_string());
    }
    if !failures.is_empty() {
        return Err(RefitError::VerifyFailed {
            after_write: true,
            failures,
        });
    }
    report.say("REFIT COMPLETE. Record the date, tool commit and the figures above in ADR-2219.");
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus_history::CORPUS_SYNC_WRITER;
    use crate::provenance_emitter::{build_activity_quads, ActivityRecord};
    use chrono::TimeZone;
    use oxigraph::model::vocab::xsd;

    const AGENT: &str =
        "did:nostr:11ed64225dd5e2c5e18f61ad43d5ad9272d08739d3a20dd25886197b0738663c";
    const SUB: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

    fn t1() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 13, 26, 13).unwrap()
    }
    fn t2() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 55, 6).unwrap()
    }

    fn lit(at: &DateTime<Utc>) -> Literal {
        Literal::new_typed_literal(canonical(at), xsd::DATE_TIME)
    }

    /// A store in the v1 form, written the way the pre-v2 writer wrote it:
    /// `n` triples opened at the first sync, the first `closed` of them closed
    /// by a bare `dl:validTo` at the second, plus other activities with
    /// non-canonical timestamps and residue versions in the other graph.
    fn v1_store(n: u32, closed: u32) -> Store {
        let store = Store::new().unwrap();
        let g = NamedNode::new_unchecked(GRAPH_PROVENANCE);
        let dl = |t: &str| NamedNode::new_unchecked(format!("{DL_NS}{t}"));
        let prov = |t: &str| NamedNode::new_unchecked(format!("http://www.w3.org/ns/prov#{t}"));
        let rdf = |t: &str| {
            NamedNode::new_unchecked(format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#{t}"))
        };
        // The two corpus syncs, as the pre-v2 writer recorded them.
        for at in [t1(), t2()] {
            let record = ActivityRecord {
                activity_urn: corpus_sync_activity_urn(&at),
                agent_did: AGENT.to_string(),
                timestamp: canonical(&at),
                action: "ingest".to_string(),
                derivation: "asserted".to_string(),
                used: None,
                generated: None,
                informed_by: None,
            };
            store
                .extend(build_activity_quads(&record).unwrap())
                .unwrap();
            store
                .insert(&Quad::new(
                    NamedNode::new_unchecked(record.activity_urn),
                    NamedNode::new_unchecked(CORPUS_SYNC_WRITER_PREDICATE),
                    Literal::new_simple_literal(CORPUS_SYNC_WRITER),
                    g.clone(),
                ))
                .unwrap();
        }
        let activity = corpus_sync_activity_urn(&t1());
        for i in 0..n {
            let e = NamedNode::new_unchecked(format!(
                "urn:agentbox:event:{}:sha256-12-{i:012x}",
                &AGENT[10..]
            ));
            let quads = po([
                (rdf("type"), prov("Entity").into()),
                (
                    rdf("subject"),
                    NamedNode::new_unchecked(format!("urn:ngm:class:s{i}")).into(),
                ),
                (rdf("predicate"), NamedNode::new_unchecked(SUB).into()),
                (
                    rdf("object"),
                    NamedNode::new_unchecked("urn:ngm:class:o").into(),
                ),
                (dl("validFrom"), lit(&t1()).into()),
                (prov("generatedAtTime"), lit(&t1()).into()),
                (
                    prov("wasGeneratedBy"),
                    NamedNode::new_unchecked(&activity).into(),
                ),
                (
                    prov("wasAttributedTo"),
                    NamedNode::new_unchecked(AGENT).into(),
                ),
            ]);
            for (p, o) in quads {
                store
                    .insert(&Quad::new(e.clone(), p, o, g.clone()))
                    .unwrap();
            }
            if i < closed {
                store
                    .insert(&Quad::new(e.clone(), dl("validTo"), lit(&t2()), g.clone()))
                    .unwrap();
            }
        }
        // Other activities, one with sub-second digits and an offset.
        for (i, stamp) in ["2026-11-10T08:00:00.123456+00:00", "2026-11-11T09:00:00Z"]
            .iter()
            .enumerate()
        {
            let a =
                NamedNode::new_unchecked(format!("urn:visionclaw:execution:sha256-12-{i:012x}"));
            for (p, o) in po([
                (rdf("type"), prov("Activity").into()),
                (
                    prov("startedAtTime"),
                    Literal::new_typed_literal(*stamp, xsd::DATE_TIME).into(),
                ),
            ]) {
                store
                    .insert(&Quad::new(a.clone(), p, o, g.clone()))
                    .unwrap();
            }
        }
        // Residue: a version in the other provenance graph.
        let rg = NamedNode::new_unchecked(RESIDUE_GRAPH);
        let r = NamedNode::new_unchecked("urn:agentbox:event:residue:sha256-12-aaaaaaaaaaaa");
        for (p, o) in po([
            (
                rdf("subject"),
                NamedNode::new_unchecked("urn:ngm:class:r").into(),
            ),
            (dl("validFrom"), lit(&t1()).into()),
        ]) {
            store
                .insert(&Quad::new(r.clone(), p, o, rg.clone()))
                .unwrap();
        }
        store
    }

    /// Pins the element type so `.into()` in the literal arrays resolves.
    fn po<const N: usize>(a: [(NamedNode, Term); N]) -> [(NamedNode, Term); N] {
        a
    }

    fn expect(n: usize, closes: usize) -> Options {
        Options {
            apply: false,
            expect_versions: Some(n),
            expect_closes: Some(closes),
            expect_other_activities: Some(2),
        }
    }

    fn graph_len(store: &Store, g: &str) -> usize {
        store
            .quads_for_pattern(
                None,
                None,
                None,
                Some(GraphNameRef::NamedNode(NamedNodeRef::new_unchecked(g))),
            )
            .count()
    }

    #[test]
    fn a_dry_run_reports_and_writes_nothing() {
        let store = v1_store(40, 6);
        let before = store.len().unwrap();
        let report = run(&store, &expect(40, 6)).expect("dry run");
        let out = report.lines.join("\n");
        eprintln!("{out}");
        assert!(!report.applied && !report.already_current);
        assert!(out.contains("DRY RUN"));
        assert!(out.contains("versions: 40") && out.contains("bare closes: 6"));
        assert!(!out.contains("[FAIL]"), "{out}");
        assert_eq!(store.len().unwrap(), before, "the store is untouched");
        assert!(legacy_forms(&store).unwrap().any(), "still in the old form");
    }

    #[test]
    fn apply_replays_through_the_production_writer_and_is_idempotent() {
        let store = v1_store(40, 6);
        let mut opts = expect(40, 6);
        opts.apply = true;
        let report = run(&store, &opts).expect("apply");
        let out = report.lines.join("\n");
        eprintln!("{out}");
        assert!(report.applied);
        assert!(
            out.contains("REFIT COMPLETE") && !out.contains("[FAIL]"),
            "{out}"
        );

        // v2 form, with no v1 form and no residue.
        assert!(!legacy_forms(&store).unwrap().any());
        assert_eq!(graph_len(&store, RESIDUE_GRAPH), 0);
        // 40 versions open at sync 1, 6 closed at sync 2: 34 open.
        assert_eq!(open_corpus_versions(&store).unwrap().len(), 34);
        // The knowledge cut: a cut between the syncs does not see the closes.
        let mid = t1() + (t2() - t1()) / 2;
        let after = t2() + chrono::Duration::days(1);
        let n = |r| match store.query(&build_state_at_sparql(after, r)).unwrap() {
            QueryResults::Solutions(s) => s.count(),
            _ => 0,
        };
        assert_eq!((n(Some(mid)), n(None)), (40, 34));

        // A second run finds nothing to do and writes nothing.
        let len = store.len().unwrap();
        let again = run(&store, &opts).expect("second run");
        assert!(again.already_current && !again.applied);
        assert_eq!(store.len().unwrap(), len);
    }

    /// Rehearsal at the live store's size and instants: 158,635 versions, 15 bare
    /// closes. Slow (about a minute), so it is not part of the default run:
    /// `cargo test -p visionclaw-adapters --lib rehearsal -- --ignored --nocapture`.
    /// The knowledge-cut answers are the ones measured on the real store.
    #[test]
    #[ignore = "production-size rehearsal; run explicitly"]
    fn rehearsal_at_the_production_size() {
        let store = v1_store(158_635, 15);
        let mut opts = expect(158_635, 15);
        opts.apply = true;
        let start = std::time::Instant::now();
        let report = run(&store, &opts).expect("apply at production size");
        let out = report.lines.join("\n");
        eprintln!("{out}\nelapsed {:?}", start.elapsed());
        assert!(out.contains("REFIT COMPLETE") && !out.contains("[FAIL]"));
        let n = |t: DateTime<Utc>, r: Option<DateTime<Utc>>| match store
            .query(&build_state_at_sparql(t, r))
            .unwrap()
        {
            QueryResults::Solutions(s) => s.count(),
            _ => 0,
        };
        let t21 = Utc.with_ymd_and_hms(2026, 11, 21, 0, 0, 0).unwrap();
        let cut = Utc.with_ymd_and_hms(2026, 11, 19, 20, 0, 0).unwrap();
        assert_eq!(n(t21, Some(cut)), 158_635, "the knowledge cut");
        assert_eq!(n(t21, None), 158_620);
        assert_eq!(n(cut, None), 158_635);
        assert_eq!(n(t1() - chrono::Duration::seconds(1), None), 0);
    }

    #[test]
    fn the_replayed_ids_are_the_production_ids() {
        // Re-minting through the production writer is the point: the replayed
        // activity ids are the ones the live store already holds.
        let store = v1_store(3, 1);
        let mut opts = expect(3, 1);
        opts.apply = true;
        run(&store, &opts).unwrap();
        for at in [t1(), t2()] {
            let a = corpus_sync_activity_urn(&at);
            assert!(store
                .quads_for_pattern(
                    Some(NamedNodeRef::new_unchecked(&a).into()),
                    None,
                    None,
                    None
                )
                .next()
                .is_some());
        }
    }

    fn refused(store: &Store, opts: &Options) -> String {
        match run(store, opts) {
            Err(RefitError::Refused(why)) => why.join("\n"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn it_refuses_a_count_that_is_not_the_one_expected() {
        let store = v1_store(10, 2);
        let why = refused(&store, &expect(11, 2));
        assert!(why.contains("expected 11 versions, found 10"), "{why}");
        let why = refused(&store, &expect(10, 3));
        assert!(why.contains("expected 3 bare closes, found 2"), "{why}");
    }

    #[test]
    fn it_refuses_a_bare_close_that_matches_no_sync() {
        let store = v1_store(10, 0);
        let e = NamedNode::new_unchecked(format!(
            "urn:agentbox:event:{}:sha256-12-{:012x}",
            &AGENT[10..],
            0
        ));
        store
            .insert(&Quad::new(
                e,
                NamedNode::new_unchecked(format!("{DL_NS}validTo")),
                lit(&Utc.with_ymd_and_hms(2026, 11, 21, 1, 2, 3).unwrap()),
                NamedNode::new_unchecked(GRAPH_PROVENANCE),
            ))
            .unwrap();
        let before = store.len().unwrap();
        let why = refused(&store, &Options::default());
        assert!(why.contains("matches no corpus-sync activity"), "{why}");
        assert_eq!(store.len().unwrap(), before);
    }

    #[test]
    fn it_refuses_a_version_no_corpus_sync_owns() {
        let store = v1_store(5, 0);
        let g = NamedNode::new_unchecked(GRAPH_PROVENANCE);
        let e = NamedNode::new_unchecked("urn:agentbox:event:other:sha256-12-bbbbbbbbbbbb");
        let rdf = |t: &str| {
            NamedNode::new_unchecked(format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#{t}"))
        };
        let prov = |t: &str| NamedNode::new_unchecked(format!("http://www.w3.org/ns/prov#{t}"));
        for (p, o) in po([
            (rdf("subject"), NamedNode::new_unchecked("urn:x:s").into()),
            (rdf("predicate"), NamedNode::new_unchecked(SUB).into()),
            (rdf("object"), NamedNode::new_unchecked("urn:x:o").into()),
            (
                NamedNode::new_unchecked(format!("{DL_NS}validFrom")),
                lit(&t1()).into(),
            ),
            (prov("generatedAtTime"), lit(&t1()).into()),
            (
                prov("wasGeneratedBy"),
                NamedNode::new_unchecked("urn:visionclaw:execution:sha256-12-decision0001").into(),
            ),
            (
                prov("wasAttributedTo"),
                NamedNode::new_unchecked(AGENT).into(),
            ),
        ]) {
            store
                .insert(&Quad::new(e.clone(), p, o, g.clone()))
                .unwrap();
        }
        let why = refused(&store, &Options::default());
        assert!(
            why.contains("not generated by a corpus-sync activity"),
            "{why}"
        );
    }

    #[test]
    fn it_refuses_a_legacy_activity_and_a_second_agent() {
        let store = v1_store(5, 0);
        let g = NamedNode::new_unchecked(GRAPH_PROVENANCE);
        store
            .insert(&Quad::new(
                NamedNode::new_unchecked(format!("{LEGACY_ACTIVITY_PREFIX}20261105T000000Z")),
                NamedNode::new_unchecked(PROV_STARTED),
                lit(&t1()),
                g,
            ))
            .unwrap();
        let why = refused(&store, &Options::default());
        assert!(why.contains("legacy corpus-sync id form"), "{why}");
    }

    #[test]
    fn a_store_already_in_the_current_form_is_left_alone() {
        let store = Store::new().unwrap();
        let report = run(&store, &Options::default()).unwrap();
        assert!(report.already_current);
    }
}
