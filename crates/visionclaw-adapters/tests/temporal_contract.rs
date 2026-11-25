//! Conformance to the estate temporal contract `estate-temporal-v1`
//! (VisionFlow ADR-2022 P1; spec and vectors owned by ontomark
//! `conformance/temporal/`). Both implementations here — the pure
//! [`state_at`] and the `state-at` endpoint's SPARQL — must return exactly the
//! expected versions for every vector case.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use oxigraph::model::Term;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use visionclaw_adapters::provenance_writer::{
    build_assertion_version, build_state_at_sparql, build_state_at_subjects_sparql, state_at,
    AssertionInput, AssertionVersion,
};

/// Vendored copy of ontomark `conformance/temporal/vectors.json` (contract v1).
const VECTORS: &str = include_str!("fixtures/temporal/estate-temporal-v1.json");
/// The SHA-256 ontomark publishes for that file; a drifted copy fails here.
const VECTORS_SHA256: &str = "6973aeef35e538412375dd6e4a9d9539f12f88fa7513d237371098da3bc46f75";

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

/// A closure record of the v2 file: names a version and the clock(s) it closes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorClosure {
    id: String,
    version: String,
    closes: Vec<String>,
    valid_until: Option<DateTime<Utc>>,
    recorded_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorVersion {
    id: String,
    subject: String,
    predicate: String,
    object: String,
    valid_from: DateTime<Utc>,
    valid_to: Option<DateTime<Utc>>,
    generated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    t: DateTime<Utc>,
    recorded_as_of: Option<DateTime<Utc>>,
    expect: Vec<String>,
}

fn load() -> Vectors {
    serde_json::from_str(VECTORS).expect("vectors parse")
}

fn key(s: &str, p: &str, o: &str, from: &DateTime<Utc>) -> String {
    format!("{s}|{p}|{o}|{}", from.timestamp())
}

#[test]
fn vendored_vectors_match_the_published_pin() {
    let digest = hex::encode(Sha256::digest(VECTORS.as_bytes()));
    assert_eq!(
        digest, VECTORS_SHA256,
        "re-vendor from ontomark and update the pin"
    );
    assert_eq!(load().contract, "estate-temporal-v1");
}

#[test]
fn vendored_v2_vectors_match_the_published_pin_and_are_well_formed() {
    let digest = hex::encode(Sha256::digest(VECTORS_V2.as_bytes()));
    assert_eq!(
        digest, VECTORS_V2_SHA256,
        "re-vendor from ontomark and update the pin"
    );
    let v: Vectors = serde_json::from_str(VECTORS_V2).expect("v2 vectors parse");
    assert_eq!(v.contract, "estate-temporal-v2");
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
        // A closure cannot be recorded before the version it closes.
        let generated = v
            .versions
            .iter()
            .find(|x| x.id == c.version)
            .unwrap()
            .generated_at;
        assert!(c.recorded_at >= generated, "{}: recorded early", c.id);
        // A2: a closure closes at least one clock, and carries validUntil iff it
        // closes valid time.
        assert!(!c.closes.is_empty(), "{}: closes no clock", c.id);
        assert!(c.closes.iter().all(|k| k == "valid" || k == "recorded"));
        assert_eq!(
            c.closes.iter().any(|k| k == "valid"),
            c.valid_until.is_some(),
            "{}: validUntil iff valid time is closed",
            c.id
        );
    }
    // v2 versions carry no end, apart from v1's inline shorthand.
    assert_eq!(
        v.versions.iter().filter(|x| x.valid_to.is_some()).count(),
        1
    );
}

#[test]
fn pure_state_at_conforms() {
    let v = load();
    let versions: Vec<(String, AssertionVersion)> = v
        .versions
        .iter()
        .map(|x| {
            (
                x.id.clone(),
                AssertionVersion {
                    entity_iri: x.id.clone(),
                    subject: x.subject.clone(),
                    predicate: x.predicate.clone(),
                    object: x.object.clone(),
                    valid_from: x.valid_from,
                    valid_to: x.valid_to,
                    generated_at: x.generated_at,
                },
            )
        })
        .collect();
    let plain: Vec<AssertionVersion> = versions.iter().map(|(_, a)| a.clone()).collect();
    for case in &v.cases {
        let got: BTreeSet<String> = state_at(&plain, case.t)
            .into_iter()
            .filter(|a| case.recorded_as_of.is_none_or(|r| a.generated_at <= r))
            .map(|a| a.entity_iri.clone())
            .collect();
        let want: BTreeSet<String> = case.expect.iter().cloned().collect();
        assert_eq!(got, want, "pure state_at: {}", case.name);
    }
}

#[test]
fn state_at_sparql_conforms() {
    let v = load();
    let store = Store::new().unwrap();
    let mut ids: HashMap<String, String> = HashMap::new();
    for x in &v.versions {
        let q = build_assertion_version(&AssertionInput {
            subject: x.subject.clone(),
            predicate: x.predicate.clone(),
            object: x.object.clone(),
            valid_from: x.valid_from,
            valid_to: x.valid_to,
            generated_at: x.generated_at,
            activity_urn: format!("urn:visionclaw:execution:vector-{}", x.id),
            agent_iri: AGENT.to_string(),
        })
        .unwrap();
        store.extend(q.provenance_quads).unwrap();
        ids.insert(
            key(&x.subject, &x.predicate, &x.object, &x.valid_from),
            x.id.clone(),
        );
    }
    let s = |t: Option<&Term>| match t {
        Some(Term::NamedNode(n)) => n.as_str().to_string(),
        Some(Term::Literal(l)) => l.value().to_string(),
        _ => String::new(),
    };
    for case in &v.cases {
        let QueryResults::Solutions(rows) = store
            .query(&build_state_at_sparql(case.t, case.recorded_as_of))
            .unwrap()
        else {
            panic!("state-at is a SELECT");
        };
        let got: BTreeSet<String> = rows
            .map(|r| {
                let r = r.unwrap();
                let from = DateTime::parse_from_rfc3339(&s(r.get("validFrom")))
                    .unwrap()
                    .with_timezone(&Utc);
                ids[&key(
                    &s(r.get("subject")),
                    &s(r.get("predicate")),
                    &s(r.get("object")),
                    &from,
                )]
                    .clone()
            })
            .collect();
        let want: BTreeSet<String> = case.expect.iter().cloned().collect();
        assert_eq!(got, want, "state-at SPARQL: {}", case.name);
    }
}

#[test]
fn state_at_subjects_view_conforms() {
    let v = load();
    let store = Store::new().unwrap();
    for x in &v.versions {
        let q = build_assertion_version(&AssertionInput {
            subject: x.subject.clone(),
            predicate: x.predicate.clone(),
            object: x.object.clone(),
            valid_from: x.valid_from,
            valid_to: x.valid_to,
            generated_at: x.generated_at,
            activity_urn: format!("urn:visionclaw:execution:vector-{}", x.id),
            agent_iri: AGENT.to_string(),
        })
        .unwrap();
        store.extend(q.provenance_quads).unwrap();
    }
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
            let Some(Term::NamedNode(sub)) = r.get("subject") else {
                panic!("subject is an IRI")
            };
            let Some(Term::Literal(e)) = r.get("earliest") else {
                panic!("earliest is bound")
            };
            let e = DateTime::parse_from_rfc3339(e.value())
                .unwrap()
                .with_timezone(&Utc);
            got.insert(sub.as_str().to_string(), e);
        }
        let mut want: HashMap<String, DateTime<Utc>> = HashMap::new();
        for id in &case.expect {
            let x = by_id[id.as_str()];
            want.entry(x.subject.clone())
                .and_modify(|e| *e = (*e).min(x.valid_from))
                .or_insert(x.valid_from);
        }
        assert_eq!(got, want, "state-at subjects view: {}", case.name);
    }
}

// ── Knowledge-cut regression and scale (contract v2; enabled by the reader) ─

/// The v1 defect measured on the live store: 15 of 158,635 corpus versions were
/// closed by a bare end recorded on 2026-11-20, yet a query as of 2026-11-19T20:00Z
/// (before that close was recorded) still dropped them, giving 158,620 where
/// the knowledge at that instant was 158,635. A close recorded after the cut
/// must not apply.
#[test]
#[ignore = "red until step 3 (closure-aware reader); run with --ignored"]
fn a_close_recorded_after_the_cut_must_not_apply() {
    let day = |d, h| chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, d, h, 0, 0).unwrap();
    let store = Store::new().unwrap();
    for i in 0..20u32 {
        let from = day(19, 13);
        let closed = i < 3;
        let q = build_assertion_version(&AssertionInput {
            subject: format!("urn:ngm:class:s{i}"),
            predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf".into(),
            object: "urn:ngm:class:o".into(),
            valid_from: from,
            valid_to: closed.then(|| day(20, 8)),
            generated_at: from,
            activity_urn: "urn:visionclaw:execution:cut".into(),
            agent_iri: AGENT.into(),
        })
        .unwrap();
        store.extend(q.provenance_quads).unwrap();
    }
    let count = |t, r| {
        let QueryResults::Solutions(rows) = store.query(&build_state_at_sparql(t, r)).unwrap()
        else {
            panic!("SELECT")
        };
        rows.count()
    };
    assert_eq!(
        count(day(21, 0), Some(day(19, 20))),
        20,
        "the cut predates the close"
    );
    assert_eq!(count(day(21, 0), None), 17, "full knowledge: 3 closed");
    assert_eq!(count(day(19, 20), None), 20);
}

/// Production scale: ~20k versions, 1k valid-time closures. The reader must
/// stay in the region of a second, not the 12 s of the multi-pattern
/// `FILTER NOT EXISTS` form nor the 299 s of `OPTIONAL` + `GROUP BY` measured
/// on the real store. Closures are written as raw A2 quads here so the test
/// reads the reader alone; the production builders are covered elsewhere.
#[test]
#[ignore = "red until step 3 (closure-aware reader); run with --ignored"]
fn the_reader_stays_fast_at_production_scale() {
    use oxigraph::model::{vocab::xsd, GraphName, Literal, NamedNode, Quad};
    const VERSIONS: u32 = 20_000;
    const CLOSURES: u32 = 1_000;
    let dl = |s: &str| NamedNode::new_unchecked(format!("https://narrativegoldmine.com/ns/dl#{s}"));
    let prov = |s: &str| NamedNode::new_unchecked(format!("http://www.w3.org/ns/prov#{s}"));
    let g = GraphName::NamedNode(NamedNode::new_unchecked("urn:ngm:graph:provenance"));
    let t0 = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 19, 13, 0, 0).unwrap();
    let t1 = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 20, 8, 0, 0).unwrap();
    let dt = |d: DateTime<Utc>| {
        Literal::new_typed_literal(
            d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            xsd::DATE_TIME,
        )
    };
    let store = Store::new().unwrap();
    let activity = NamedNode::new_unchecked("urn:visionclaw:execution:scale");
    let agent = NamedNode::new_unchecked(AGENT);
    let rdf = |s: &str| {
        NamedNode::new_unchecked(format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#{s}"))
    };
    let mut quads = Vec::new();
    for i in 0..VERSIONS {
        let e = NamedNode::new_unchecked(format!("urn:agentbox:event:scale:v{i}"));
        quads.push(Quad::new(e.clone(), rdf("type"), prov("Entity"), g.clone()));
        quads.push(Quad::new(
            e.clone(),
            rdf("subject"),
            NamedNode::new_unchecked(format!("urn:ngm:class:s{i}")),
            g.clone(),
        ));
        quads.push(Quad::new(
            e.clone(),
            rdf("predicate"),
            NamedNode::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subClassOf"),
            g.clone(),
        ));
        quads.push(Quad::new(
            e.clone(),
            rdf("object"),
            NamedNode::new_unchecked("urn:ngm:class:o"),
            g.clone(),
        ));
        quads.push(Quad::new(e.clone(), dl("validFrom"), dt(t0), g.clone()));
        quads.push(Quad::new(
            e.clone(),
            prov("generatedAtTime"),
            dt(t0),
            g.clone(),
        ));
        quads.push(Quad::new(
            e.clone(),
            prov("wasGeneratedBy"),
            activity.clone(),
            g.clone(),
        ));
        quads.push(Quad::new(
            e.clone(),
            prov("wasAttributedTo"),
            agent.clone(),
            g.clone(),
        ));
    }
    for i in 0..CLOSURES {
        let c = NamedNode::new_unchecked(format!("urn:agentbox:event:scale:c{i}"));
        quads.push(Quad::new(c.clone(), rdf("type"), dl("Closure"), g.clone()));
        quads.push(Quad::new(
            c.clone(),
            dl("closesVersion"),
            NamedNode::new_unchecked(format!("urn:agentbox:event:scale:v{i}")),
            g.clone(),
        ));
        quads.push(Quad::new(
            c.clone(),
            dl("closes"),
            NamedNode::new_unchecked("http://nova.dev/memory#ValidTime"),
            g.clone(),
        ));
        quads.push(Quad::new(c.clone(), dl("validUntil"), dt(t1), g.clone()));
        quads.push(Quad::new(
            c.clone(),
            prov("generatedAtTime"),
            dt(t1),
            g.clone(),
        ));
    }
    store.extend(quads).unwrap();

    let run = |t, r| {
        let start = std::time::Instant::now();
        let QueryResults::Solutions(rows) = store.query(&build_state_at_sparql(t, r)).unwrap()
        else {
            panic!("SELECT")
        };
        let n = rows.filter(|r| r.is_ok()).count();
        (n, start.elapsed())
    };
    let after = t1 + chrono::Duration::days(1);
    let (n, took) = run(after, None);
    assert_eq!(n, (VERSIONS - CLOSURES) as usize, "full knowledge");
    assert!(took.as_secs_f64() < 5.0, "state-at took {took:?}");
    let (n, took) = run(after, Some(t1 - chrono::Duration::hours(1)));
    assert_eq!(
        n, VERSIONS as usize,
        "closures recorded after the cut do not apply"
    );
    assert!(
        took.as_secs_f64() < 5.0,
        "state-at with a cut took {took:?}"
    );
}
