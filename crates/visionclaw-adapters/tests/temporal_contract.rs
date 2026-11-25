//! Conformance to the estate temporal contract (VisionFlow ADR-2022 P1; spec
//! and vectors owned by ontomark `conformance/temporal/`), for both vector
//! files: `estate-temporal-v1` (3 versions, 9 cases, inline `validTo`) and
//! `estate-temporal-v2` (closure records).
//!
//! Every version and closure is built through the PRODUCTION builders
//! ([`build_assertion_version`], [`build_closure`]), then the cases are answered
//! twice: by the pure evaluator ([`state_at`]) and by the production SPARQL
//! ([`build_state_at_sparql`], [`build_state_at_subjects_sparql`]) over an
//! in-memory Oxigraph. A version's inline `validTo` is shorthand for a
//! valid-time closure recorded at its `generatedAt`; the harness realises it as
//! a closure record because the builders have no end input.

use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

use chrono::{DateTime, TimeZone, Utc};
use oxigraph::model::{Quad, Term};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use visionclaw_adapters::provenance_writer::{
    build_assertion_version, build_closure, build_state_at_sparql, build_state_at_subjects_sparql,
    effective_valid_to, state_at, AssertionInput, AssertionVersion, Clock, ClosureInput,
    ClosureRecord,
};

/// Vendored copy of ontomark `conformance/temporal/vectors.json` (contract v1).
const VECTORS_V1: &str = include_str!("fixtures/temporal/estate-temporal-v1.json");
/// The SHA-256 ontomark publishes for that file; a drifted copy fails here.
const VECTORS_V1_SHA256: &str = "6973aeef35e538412375dd6e4a9d9539f12f88fa7513d237371098da3bc46f75";

/// Vendored copy of ontomark `conformance/temporal/vectors-v2.json` (contract v2,
/// ontomark commit ce0d30b): versions never carry an end; closures do.
const VECTORS_V2: &str = include_str!("fixtures/temporal/estate-temporal-v2.json");
/// The SHA-256 ontomark publishes for that file.
const VECTORS_V2_SHA256: &str = "afd2bca7acfadbe455abf5f834e5069133c6f8f7b6229044031be42881b8c134";

const AGENT: &str = "did:nostr:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vectors {
    contract: String,
    versions: Vec<VectorVersion>,
    #[serde(default)]
    closures: Vec<VectorClosure>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorVersion {
    id: String,
    subject: String,
    predicate: String,
    object: String,
    valid_from: DateTime<Utc>,
    /// v1 shorthand for a valid-time closure recorded at `generated_at`.
    valid_to: Option<DateTime<Utc>>,
    generated_at: DateTime<Utc>,
}

/// A closure record of the v2 file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorClosure {
    id: String,
    version: String,
    closes: Vec<String>,
    valid_until: Option<DateTime<Utc>>,
    recorded_at: DateTime<Utc>,
    reason: Option<String>,
    agent: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    t: DateTime<Utc>,
    recorded_as_of: Option<DateTime<Utc>>,
    expect: Vec<String>,
}

fn load(raw: &str) -> Vectors {
    serde_json::from_str(raw).expect("vectors parse")
}

/// The vectors, realised through the production builders.
struct World {
    /// Production version values, in vector order.
    versions: Vec<AssertionVersion>,
    /// Production closure values.
    closures: Vec<ClosureRecord>,
    /// Every provenance quad the builders emitted.
    quads: Vec<Quad>,
    /// Minted entity IRI → vector id.
    ids: HashMap<String, String>,
}

fn realise(v: &Vectors) -> World {
    let mut w = World {
        versions: Vec::new(),
        closures: Vec::new(),
        quads: Vec::new(),
        ids: HashMap::new(),
    };
    let mut by_id: HashMap<&str, AssertionVersion> = HashMap::new();
    for x in &v.versions {
        let q = build_assertion_version(&AssertionInput {
            subject: x.subject.clone(),
            predicate: x.predicate.clone(),
            object: x.object.clone(),
            valid_from: x.valid_from,
            generated_at: x.generated_at,
            activity_urn: format!("urn:visionclaw:execution:vector-{}", x.id),
            agent_iri: AGENT.to_string(),
        })
        .expect("version builds");
        w.quads.extend(q.provenance_quads);
        w.ids.insert(q.entity_iri.clone(), x.id.clone());
        by_id.insert(x.id.as_str(), q.version.clone());
        w.versions.push(q.version);
    }
    // v1 shorthand → a closure recorded when the version was.
    let inline = v.versions.iter().filter_map(|x| {
        x.valid_to.map(|end| VectorClosure {
            id: format!("inline-{}", x.id),
            version: x.id.clone(),
            closes: vec!["valid".into()],
            valid_until: Some(end),
            recorded_at: x.generated_at,
            reason: None,
            agent: None,
        })
    });
    for c in v
        .closures
        .iter()
        .map(|c| VectorClosure {
            id: c.id.clone(),
            version: c.version.clone(),
            closes: c.closes.clone(),
            valid_until: c.valid_until,
            recorded_at: c.recorded_at,
            reason: c.reason.clone(),
            agent: c.agent.clone(),
        })
        .chain(inline)
    {
        let clocks = c
            .closes
            .iter()
            .map(|k| match k.as_str() {
                "valid" => Clock::ValidTime,
                "recorded" => Clock::RecordedTime,
                other => panic!("unknown clock {other}"),
            })
            .collect();
        let q = build_closure(
            &by_id[c.version.as_str()],
            &ClosureInput {
                clocks,
                valid_until: c.valid_until,
                recorded_at: c.recorded_at,
                activity_urn: format!("urn:visionclaw:execution:vector-{}", c.id),
                agent_iri: c.agent.unwrap_or_else(|| AGENT.to_string()),
                reason: c.reason,
            },
        )
        .unwrap_or_else(|e| panic!("closure {} builds: {e}", c.id));
        w.quads.extend(q.provenance_quads);
        w.closures.push(q.closure);
    }
    w
}

fn store_of(w: &World) -> Store {
    let store = Store::new().unwrap();
    store.extend(w.quads.iter().cloned()).unwrap();
    store
}

#[test]
fn vendored_vectors_match_the_published_pins() {
    for (raw, pin, contract) in [
        (VECTORS_V1, VECTORS_V1_SHA256, "estate-temporal-v1"),
        (VECTORS_V2, VECTORS_V2_SHA256, "estate-temporal-v2"),
    ] {
        assert_eq!(
            hex::encode(Sha256::digest(raw.as_bytes())),
            pin,
            "{contract}: re-vendor from ontomark and update the pin"
        );
        assert_eq!(load(raw).contract, contract);
    }
}

#[test]
fn the_v2_vectors_are_well_formed() {
    let v = load(VECTORS_V2);
    assert_eq!(
        (v.versions.len(), v.closures.len(), v.cases.len()),
        (12, 10, 35)
    );
    let ids: BTreeSet<&str> = v.versions.iter().map(|x| x.id.as_str()).collect();
    for c in &v.closures {
        assert!(
            ids.contains(c.version.as_str()),
            "{}: unknown version",
            c.id
        );
        // A2: a closure closes at least one clock, and carries validUntil iff it
        // closes valid time.
        assert!(!c.closes.is_empty(), "{}: closes no clock", c.id);
        assert_eq!(
            c.closes.iter().any(|k| k == "valid"),
            c.valid_until.is_some(),
            "{}: validUntil iff valid time is closed",
            c.id
        );
    }
    // Versions carry no end apart from v1's inline shorthand.
    assert_eq!(
        v.versions.iter().filter(|x| x.valid_to.is_some()).count(),
        1
    );
}

/// Every vector closure is realised through the builder (so none is malformed)
/// and the first nine v2 cases keep their v1 answers.
#[test]
fn v2_keeps_the_v1_versions_and_cases() {
    let (one, two) = (load(VECTORS_V1), load(VECTORS_V2));
    for (a, b) in one.versions.iter().zip(&two.versions) {
        assert_eq!(
            (&a.id, &a.subject, &a.object),
            (&b.id, &b.subject, &b.object)
        );
    }
    for (a, b) in one.cases.iter().zip(&two.cases) {
        assert_eq!((&a.name, &a.t, &a.expect), (&b.name, &b.t, &b.expect));
    }
}

fn pure_answer(w: &World, case: &Case) -> BTreeSet<String> {
    state_at(&w.versions, &w.closures, case.t, case.recorded_as_of)
        .into_iter()
        .map(|a| w.ids[&a.entity_iri].clone())
        .collect()
}

#[test]
fn pure_state_at_conforms() {
    for raw in [VECTORS_V1, VECTORS_V2] {
        let v = load(raw);
        let w = realise(&v);
        for case in &v.cases {
            let want: BTreeSet<String> = case.expect.iter().cloned().collect();
            assert_eq!(
                pure_answer(&w, case),
                want,
                "{}: pure state_at: {}",
                v.contract,
                case.name
            );
        }
    }
}

fn s(t: Option<&Term>) -> String {
    match t {
        Some(Term::NamedNode(n)) => n.as_str().to_string(),
        Some(Term::Literal(l)) => l.value().to_string(),
        _ => String::new(),
    }
}

#[test]
#[ignore = "enabled by step 3 (closure-aware reader)"]
fn state_at_sparql_conforms() {
    for raw in [VECTORS_V1, VECTORS_V2] {
        let v = load(raw);
        let w = realise(&v);
        let store = store_of(&w);
        for case in &v.cases {
            let QueryResults::Solutions(rows) = store
                .query(&build_state_at_sparql(case.t, case.recorded_as_of))
                .unwrap()
            else {
                panic!("state-at is a SELECT");
            };
            let mut got_ids = Vec::new();
            for r in rows {
                let r = r.unwrap();
                let entity = s(r.get("entity"));
                // The reported end is the effective end under the same cut.
                let want_end = effective_valid_to(&entity, &w.closures, case.recorded_as_of);
                let got_end = r.get("validTo").map(|t| {
                    DateTime::parse_from_rfc3339(&s(Some(t)))
                        .unwrap()
                        .with_timezone(&Utc)
                });
                assert_eq!(got_end, want_end, "{}: validTo of {entity}", case.name);
                got_ids.push(w.ids[&entity].clone());
            }
            let got: BTreeSet<String> = got_ids.iter().cloned().collect();
            assert_eq!(got_ids.len(), got.len(), "{}: duplicate rows", case.name);
            let want: BTreeSet<String> = case.expect.iter().cloned().collect();
            assert_eq!(got, want, "{}: state-at SPARQL: {}", v.contract, case.name);
            assert_eq!(got, pure_answer(&w, case), "SPARQL and pure agree");
        }
    }
}

#[test]
#[ignore = "enabled by step 3 (closure-aware reader)"]
fn state_at_subjects_view_conforms() {
    for raw in [VECTORS_V1, VECTORS_V2] {
        let v = load(raw);
        let w = realise(&v);
        let store = store_of(&w);
        let by_id: HashMap<&str, &VectorVersion> =
            v.versions.iter().map(|x| (x.id.as_str(), x)).collect();
        for case in &v.cases {
            let QueryResults::Solutions(rows) = store
                .query(&build_state_at_subjects_sparql(case.t, case.recorded_as_of))
                .unwrap()
            else {
                panic!("subjects view is a SELECT");
            };
            let mut got: HashMap<String, DateTime<Utc>> = HashMap::new();
            for r in rows {
                let r = r.unwrap();
                let earliest = DateTime::parse_from_rfc3339(&s(r.get("earliest")))
                    .unwrap()
                    .with_timezone(&Utc);
                assert!(
                    got.insert(s(r.get("subject")), earliest).is_none(),
                    "{}: one row per subject",
                    case.name
                );
            }
            let mut want: HashMap<String, DateTime<Utc>> = HashMap::new();
            for id in &case.expect {
                let x = by_id[id.as_str()];
                want.entry(x.subject.clone())
                    .and_modify(|e| *e = (*e).min(x.valid_from))
                    .or_insert(x.valid_from);
            }
            assert_eq!(got, want, "{}: subjects view: {}", v.contract, case.name);
        }
    }
}

// ── Knowledge-cut regression and scale ──────────────────────────────────────

fn day(d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, d, h, 0, 0).unwrap()
}

/// The v1 defect measured on the live store: 15 of 158,635 corpus versions were
/// closed on 2026-11-20, yet a query as of 2026-11-19T20:00Z (before that close
/// was recorded) still dropped them, giving 158,620 where the knowledge at that
/// instant was 158,635. A close recorded after the cut must not apply.
#[test]
#[ignore = "enabled by step 3 (closure-aware reader)"]
fn a_close_recorded_after_the_cut_must_not_apply() {
    let store = Store::new().unwrap();
    let agent = AGENT.to_string();
    for i in 0..20u32 {
        let q = build_assertion_version(&AssertionInput {
            subject: format!("urn:ngm:class:s{i}"),
            predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf".into(),
            object: "urn:ngm:class:o".into(),
            valid_from: day(19, 13),
            generated_at: day(19, 13),
            activity_urn: "urn:visionclaw:execution:cut".into(),
            agent_iri: agent.clone(),
        })
        .unwrap();
        store.extend(q.provenance_quads).unwrap();
        if i < 3 {
            let c = build_closure(
                &q.version,
                &ClosureInput {
                    clocks: vec![Clock::ValidTime],
                    valid_until: Some(day(20, 8)),
                    recorded_at: day(20, 8),
                    activity_urn: "urn:visionclaw:execution:cut-2".into(),
                    agent_iri: agent.clone(),
                    reason: None,
                },
            )
            .unwrap();
            store.extend(c.provenance_quads).unwrap();
        }
    }
    let count = |t, r| {
        let QueryResults::Solutions(rows) = store.query(&build_state_at_sparql(t, r)).unwrap()
        else {
            panic!("SELECT")
        };
        rows.filter(|r| r.is_ok()).count()
    };
    assert_eq!(
        count(day(21, 0), Some(day(19, 20))),
        20,
        "the cut predates the close"
    );
    assert_eq!(count(day(21, 0), None), 17, "full knowledge: 3 closed");
    assert_eq!(count(day(19, 20), None), 20);
    assert_eq!(count(day(12, 0), None), 0, "before any version");
}

/// Production scale: 20k versions, 1k valid-time closures, built through the
/// production builders. The reader must stay near a second, not the 12 s of the
/// multi-pattern `FILTER NOT EXISTS` form nor the 299 s of `OPTIONAL` +
/// `GROUP BY` measured on the real store.
#[test]
#[ignore = "enabled by step 3 (closure-aware reader)"]
fn the_reader_stays_fast_at_production_scale() {
    const VERSIONS: u32 = 20_000;
    const CLOSURES: u32 = 1_000;
    let store = Store::new().unwrap();
    let mut quads = Vec::new();
    for i in 0..VERSIONS {
        let q = build_assertion_version(&AssertionInput {
            subject: format!("urn:ngm:class:s{i}"),
            predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf".into(),
            object: "urn:ngm:class:o".into(),
            valid_from: day(19, 13),
            generated_at: day(19, 13),
            activity_urn: "urn:visionclaw:execution:scale".into(),
            agent_iri: AGENT.to_string(),
        })
        .unwrap();
        quads.extend(q.provenance_quads);
        if i < CLOSURES {
            let c = build_closure(
                &q.version,
                &ClosureInput {
                    clocks: vec![Clock::ValidTime],
                    valid_until: Some(day(20, 8)),
                    recorded_at: day(20, 8),
                    activity_urn: "urn:visionclaw:execution:scale-2".into(),
                    agent_iri: AGENT.to_string(),
                    reason: None,
                },
            )
            .unwrap();
            quads.extend(c.provenance_quads);
        }
    }
    store.extend(quads).unwrap();

    let run = |t, r| {
        let start = Instant::now();
        let QueryResults::Solutions(rows) = store.query(&build_state_at_sparql(t, r)).unwrap()
        else {
            panic!("SELECT")
        };
        let n = rows.filter(|r| r.is_ok()).count();
        (n, start.elapsed())
    };
    let (n, took) = run(day(21, 0), None);
    assert_eq!(n, (VERSIONS - CLOSURES) as usize, "full knowledge");
    assert!(took.as_secs_f64() < 5.0, "state-at took {took:?}");
    let (n, took) = run(day(21, 0), Some(day(19, 20)));
    assert_eq!(
        n, VERSIONS as usize,
        "closures recorded after the cut do not apply"
    );
    assert!(
        took.as_secs_f64() < 5.0,
        "state-at with a cut took {took:?}"
    );
    eprintln!("scale: {VERSIONS} versions, {CLOSURES} closures, last query {took:?}");
}
