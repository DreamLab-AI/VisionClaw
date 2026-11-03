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

use crate::handlers::api_handler::ontology::build_state_at_sparql;
use crate::services::provenance_writer::{
    build_assertion_version, state_at, AssertionInput, AssertionVersion,
};

/// Vendored copy of ontomark `conformance/temporal/vectors.json`.
const VECTORS: &str = include_str!("../../tests/fixtures/temporal/estate-temporal-v1.json");
/// The SHA-256 ontomark publishes for that file; a drifted copy fails here.
const VECTORS_SHA256: &str = "6973aeef35e538412375dd6e4a9d9539f12f88fa7513d237371098da3bc46f75";

const AGENT: &str = "did:nostr:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vectors {
    contract: String,
    versions: Vec<VectorVersion>,
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
