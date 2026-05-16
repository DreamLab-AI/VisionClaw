// tests/adapter_parity/mod.rs
//! # Port Parity Test Harness
//!
//! Goal: every adapter that implements one of the three persistence ports
//! (`OntologyRepository`, `GraphRepository`, `SettingsRepository`) MUST pass
//! this harness, or the trait contract is broken.
//!
//! The harness is the runtime enforcement of PRD-11 §A2 ("Port parity. No
//! upstream caller changes by a single line"): any future adapter is expected
//! to be drop-in for the existing one, which means the same behaviour for
//! the same call. This file expresses that "same behaviour" as a battery of
//! generic parametric test functions of shape
//!
//! ```ignore
//! pub async fn parity_xxx<R: SomeRepository>(repo: R) { ... }
//! ```
//!
//! Concrete runners (one per backend) live in `runner_neo4j.rs`,
//! `runner_oxigraph.rs` (etc.) and call every parity function in turn,
//! feeding it a freshly-constructed adapter. The parity functions are
//! self-contained: they set up their own data, assert, and (where the
//! adapter supports it) tear it down again.
//!
//! Reading order:
//!
//! 1. `ontology_parity.rs`   — 10 representative scenarios over `OntologyRepository`.
//! 2. `graph_parity.rs`      — 9 scenarios over `GraphRepository`.
//! 3. `settings_parity.rs`   — coverage for all 17 `SettingsRepository` methods,
//!                             including per-user-vs-global resolution (ADR-11 §D5)
//!                             and `schema_version` round-trip.
//! 4. `named_graph_invariants.rs` — 5 cross-graph isolation invariants
//!                             (knowledge vs ontology vs agent vs inferred).
//! 5. `runner_neo4j.rs`      — exercises every parity function against
//!                             `Neo4jOntologyRepository / Neo4jGraphRepository /
//!                             Neo4jSettingsRepository`. Gated behind
//!                             `feature = "test-neo4j"` so it can be skipped
//!                             where Neo4j is not provisioned.
//! 6. `runner_oxigraph.rs`   — exercises every parity function against
//!                             `Oxigraph*Repository / Sqlite*Repository`.
//!                             Gated behind `feature = "persistence-oxigraph"`.
//!                             Stubbed out today; flips on as soon as the
//!                             scaffolded adapters land.
//!
//! ## Strict rules (replicated from the work order)
//!
//! - All tests use `tokio::test` async.
//! - No mocks. The runner against each backend constructs a real adapter
//!   over a real (or ephemerally provisioned) store.
//! - Each parity function owns its data lifecycle. No shared state between
//!   parity functions; no ordering dependency between them.

// All harness helpers are #[allow(dead_code)] because runners are
// feature-gated. Without `--features test-neo4j` or
// `--features persistence-oxigraph`, no `#[tokio::test]` entry points
// reference them — but the modules MUST still compile so a future
// adapter can be wired up by adding a runner alone.
#![allow(dead_code)]

pub mod ontology_parity;
pub mod graph_parity;
pub mod settings_parity;
pub mod named_graph_invariants;

// Concrete runners. Gated so the test crate compiles even when the
// requested backend is not provisioned.
#[cfg(feature = "test-neo4j")]
pub mod runner_neo4j;
#[cfg(feature = "persistence-oxigraph")]
pub mod runner_oxigraph;

use std::collections::HashMap;

use webxr::config::PhysicsSettings;
use webxr::models::edge::Edge;
use webxr::models::node::Node;
use webxr::ports::ontology_repository::{
    AxiomType, OwlAxiom, OwlClass, OwlProperty, PropertyType,
};
use webxr::ports::settings_repository::SettingValue;

// ---------------------------------------------------------------------------
// Test data builders
// ---------------------------------------------------------------------------
//
// Builders here are deliberately minimal. The parity harness is about
// behaviour at the trait boundary; we do NOT want a builder DSL that hides
// which fields the adapter does/doesn't preserve. Every parity test asserts
// the fields it cares about explicitly.

/// Build a representative `OwlClass` for parity testing.
///
/// `slug` controls both the IRI and the label so that tests producing
/// multiple classes get distinct, stable identifiers.
pub fn make_owl_class(slug: &str) -> OwlClass {
    let mut c = OwlClass::default();
    c.iri = format!("https://visionflow.dreamlab/ns/onto/{}", slug);
    c.label = Some(format!("Test Class {}", slug));
    c.description = Some(format!("Parity-harness fixture for slug={}", slug));
    c.term_id = Some(format!("TEST-{}", slug.to_uppercase()));
    c.preferred_term = Some(format!("Test {}", slug));
    c.source_domain = Some("ParityHarness".to_string());
    c.version = Some("1.0.0".to_string());
    c.class_type = Some("ObjectClass".to_string());
    c.status = Some("active".to_string());
    c.quality_score = Some(0.87);
    c.authority_score = Some(0.91);
    c.public_access = Some(true);
    c.owl_role = Some("category".to_string());
    c.belongs_to_domain = Some("test-domain".to_string());
    let mut props = HashMap::new();
    props.insert("k1".to_string(), "v1".to_string());
    props.insert("k2".to_string(), "v2".to_string());
    c.properties = props;
    c
}

/// Build a representative `OwlProperty`.
pub fn make_owl_property(slug: &str, kind: PropertyType) -> OwlProperty {
    OwlProperty {
        iri: format!("https://visionflow.dreamlab/ns/prop/{}", slug),
        label: Some(format!("Property {}", slug)),
        property_type: kind,
        domain: vec![format!("https://visionflow.dreamlab/ns/onto/{}-d", slug)],
        range: vec![format!("https://visionflow.dreamlab/ns/onto/{}-r", slug)],
        quality_score: Some(0.75),
        authority_score: Some(0.80),
        source_file: Some("parity-harness".to_string()),
    }
}

/// Build a representative `OwlAxiom` of the given kind.
pub fn make_owl_axiom(subject: &str, kind: AxiomType, object: &str) -> OwlAxiom {
    OwlAxiom {
        id: None,
        axiom_type: kind,
        subject: subject.to_string(),
        object: object.to_string(),
        annotations: HashMap::new(),
    }
}

/// Build a Node with a deterministic metadata_id.
///
/// The Rust runtime mints a u32 id on construction; the harness DOES NOT
/// hardcode ids — the parity test discovers them from the adapter's
/// `add_nodes` return value.
pub fn make_node(metadata_id: &str, label: &str) -> Node {
    let mut n = Node::new(metadata_id.to_string());
    n.label = label.to_string();
    n
}

/// Build an Edge with the given endpoint ids and weight.
pub fn make_edge(source: u32, target: u32, weight: f32) -> Edge {
    Edge::new(source, target, weight)
}

/// Build a representative `PhysicsSettings`. Uses `PhysicsSettings::default()`
/// and then mutates two specific fields so a saved-then-loaded profile is
/// detectably different from `default()`.
pub fn make_physics_profile(spring_k: f32, damping: f32) -> PhysicsSettings {
    let mut p = PhysicsSettings::default();
    p.spring_k = spring_k;
    p.damping = damping;
    p
}

/// A 64-char hex lowercase pubkey, for per-user-settings tests.
/// Two distinct pubkeys are exposed so we can also test cross-user isolation.
pub const PUBKEY_ALICE: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub const PUBKEY_BOB: &str =
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// Convenience constructor for `SettingValue::String`.
pub fn sv_string(s: &str) -> SettingValue {
    SettingValue::String(s.to_string())
}
