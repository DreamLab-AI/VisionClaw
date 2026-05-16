// tests/adapter_parity/runner_neo4j.rs
//! Concrete runner: parity harness against the existing Neo4j adapters.
//!
//! This proves the harness is CORRECT against the in-production adapter
//! before we run it against the new Oxigraph adapter. If a parity scenario
//! fails here, the harness is wrong — not the adapter.
//!
//! Gated behind `feature = "test-neo4j"` so it compiles to nothing on
//! environments without a Neo4j service. To run:
//!
//! ```bash
//! NEO4J_PASSWORD=test ALLOW_INSECURE_DEFAULTS=true \
//!     cargo test --features test-neo4j --test adapter_parity
//! ```
//!
//! Environment variables consumed (all optional; defaults shown):
//!   - NEO4J_URI       (bolt://localhost:7687)
//!   - NEO4J_USER      (neo4j)
//!   - NEO4J_PASSWORD  (required unless ALLOW_INSECURE_DEFAULTS=true)
//!   - NEO4J_DATABASE  (none → server default)
//!
//! Each test calls `neo4j_clean_database()` before constructing the adapter
//! so scenarios run isolated. If Neo4j is unreachable, the entire module
//! is skipped via the `#[ignore]` attribute and a clear stderr message.

#![cfg(feature = "test-neo4j")]

use std::sync::Arc;

use webxr::adapters::neo4j_ontology_repository::{
    Neo4jOntologyConfig, Neo4jOntologyRepository,
};
use webxr::adapters::neo4j_settings_repository::{
    Neo4jSettingsConfig, Neo4jSettingsRepository,
};
use webxr::adapters::neo4j_graph_repository::Neo4jGraphRepository;

use super::{graph_parity, named_graph_invariants, ontology_parity, settings_parity};

// ---------------------------------------------------------------------------
// Connection helpers
// ---------------------------------------------------------------------------

async fn open_neo4j() -> Arc<neo4rs::Graph> {
    let uri = std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://localhost:7687".into());
    let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let password = std::env::var("NEO4J_PASSWORD")
        .or_else(|_| {
            if std::env::var("ALLOW_INSECURE_DEFAULTS").map(|v| v == "true").unwrap_or(false) {
                Ok::<_, std::env::VarError>("password".to_string())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .expect("NEO4J_PASSWORD required (or ALLOW_INSECURE_DEFAULTS=true)");

    let g = neo4rs::Graph::new(&uri, &user, &password)
        .expect("Failed to construct neo4rs::Graph");
    Arc::new(g)
}

async fn clean_database(g: &Arc<neo4rs::Graph>) {
    // Wipe everything. The Neo4j adapters re-create their schema lazily.
    let q = neo4rs::query("MATCH (n) DETACH DELETE n");
    let _ = g.run(q).await;
}

async fn fresh_ontology_repo() -> Neo4jOntologyRepository {
    let g = open_neo4j().await;
    clean_database(&g).await;
    let cfg = Neo4jOntologyConfig::from_env()
        .expect("Neo4jOntologyConfig::from_env must succeed (set NEO4J_PASSWORD)");
    Neo4jOntologyRepository::new(cfg)
        .await
        .expect("Neo4jOntologyRepository::new must succeed against a clean DB")
}

async fn fresh_graph_repo() -> Neo4jGraphRepository {
    let g = open_neo4j().await;
    clean_database(&g).await;
    Neo4jGraphRepository::new(g)
}

async fn fresh_settings_repo() -> Neo4jSettingsRepository {
    let g = open_neo4j().await;
    clean_database(&g).await;
    let cfg = Neo4jSettingsConfig::default();
    Neo4jSettingsRepository::new(cfg)
        .await
        .expect("Neo4jSettingsRepository::new must succeed against a clean DB")
}

// ---------------------------------------------------------------------------
// Test entry points — these are the actual `#[tokio::test]` cases that
// cargo-test discovers. Each delegates into the generic parity battery.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_ontology_parity_battery() {
    ontology_parity::run_all(|| async { fresh_ontology_repo().await }).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_graph_parity_battery() {
    graph_parity::run_all(|| async { fresh_graph_repo().await }).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_settings_parity_battery() {
    settings_parity::run_all(|| async { fresh_settings_repo().await }).await;
}

// ---------------------------------------------------------------------------
// Cross-port invariants. Each must construct both ports against the same
// Neo4j DB so the segregation invariants are meaningful.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_invariant_ontology_class_not_in_knowledge_graph() {
    let g = open_neo4j().await;
    clean_database(&g).await;
    let ont = Neo4jOntologyRepository::new(
        Neo4jOntologyConfig::from_env().expect("config"),
    )
    .await
    .expect("ontology repo");
    let graph = Neo4jGraphRepository::new(g);

    named_graph_invariants::invariant_ontology_class_not_in_knowledge_graph(ont, graph).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_invariant_knowledge_node_not_in_ontology_list() {
    let g = open_neo4j().await;
    clean_database(&g).await;
    let ont = Neo4jOntologyRepository::new(
        Neo4jOntologyConfig::from_env().expect("config"),
    )
    .await
    .expect("ontology repo");
    let graph = Neo4jGraphRepository::new(g);

    named_graph_invariants::invariant_knowledge_node_not_in_ontology_list(ont, graph).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_invariant_knowledge_vs_agent_graph_isolated() {
    let graph = fresh_graph_repo().await;
    named_graph_invariants::invariant_knowledge_vs_agent_graph_isolated(graph).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_invariant_inferred_does_not_leak_into_asserted() {
    let ont = fresh_ontology_repo().await;
    named_graph_invariants::invariant_inferred_does_not_leak_into_asserted(ont).await;
}

#[tokio::test]
#[ignore = "requires a running Neo4j instance + --features test-neo4j"]
async fn neo4j_invariant_class_remove_does_not_cascade_to_kg() {
    let g = open_neo4j().await;
    clean_database(&g).await;
    let ont = Neo4jOntologyRepository::new(
        Neo4jOntologyConfig::from_env().expect("config"),
    )
    .await
    .expect("ontology repo");
    let graph = Neo4jGraphRepository::new(g);

    named_graph_invariants::invariant_class_remove_does_not_cascade_to_kg(ont, graph).await;
}
