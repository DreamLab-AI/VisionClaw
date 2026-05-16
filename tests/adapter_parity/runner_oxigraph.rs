// tests/adapter_parity/runner_oxigraph.rs
//! Concrete runner: parity harness against the new Oxigraph + SQLite adapters.
//!
//! Gated behind `feature = "persistence-oxigraph"`. The Oxigraph and SQLite
//! adapters are being scaffolded in parallel by another agent under
//! `src/adapters/oxigraph_*.rs` and `src/adapters/sqlite_*.rs`. This runner
//! is the contract those adapters must satisfy.
//!
//! Quick start (once adapters are present):
//! ```bash
//! cargo test --features persistence-oxigraph --test adapter_parity
//! ```
//!
//! Storage layout (per ADR-11 §D1):
//! ```
//! <tempdir>/
//! ├── oxigraph/        (RocksDB-backed Oxigraph dataset)
//! └── settings.sqlite3 (SQLite settings + physics_profiles + audit_log)
//! ```
//!
//! ## Wiring procedure when the adapters land
//!
//! 1. Remove the `compile_error!` line below.
//! 2. Uncomment the `use` imports for `OxigraphOntologyRepository`,
//!    `OxigraphGraphRepository`, `SqliteSettingsRepository`.
//! 3. Uncomment the factory bodies.
//! 4. In each `#[tokio::test]` body, uncomment the `run_all(...)` call
//!    and delete the `panic!` line.
//! 5. Drop the `#[ignore]` attributes once a CI runner is configured.

#![cfg(feature = "persistence-oxigraph")]

// =============================================================================
// PLACEHOLDER — REMOVE WHEN ADAPTERS LAND.
// Compile under --features persistence-oxigraph to see this message.
// =============================================================================

compile_error!(
    "tests/adapter_parity/runner_oxigraph.rs is a stub. \
     Once Oxigraph/SQLite adapters are scaffolded in src/adapters/, \
     follow the wiring procedure at the top of this file."
);

// =============================================================================
// THE REST OF THIS FILE IS COMMENTED OUT until the adapters land.
// The compile_error! above prevents anything below from being type-checked.
// =============================================================================

// use webxr::adapters::oxigraph_ontology_repository::OxigraphOntologyRepository;
// use webxr::adapters::oxigraph_graph_repository::OxigraphGraphRepository;
// use webxr::adapters::sqlite_settings_repository::SqliteSettingsRepository;
//
// use super::{graph_parity, named_graph_invariants, ontology_parity, settings_parity};
//
// /// One data directory per parity scenario. Dropped at the end of each test.
// struct DataDir {
//     _td: tempfile::TempDir,
//     pub oxigraph_dir: std::path::PathBuf,
//     pub sqlite_path: std::path::PathBuf,
// }
//
// impl DataDir {
//     fn new() -> Self {
//         let td = tempfile::tempdir().expect("tempdir");
//         let oxigraph_dir = td.path().join("oxigraph");
//         std::fs::create_dir_all(&oxigraph_dir).expect("mkdir oxigraph/");
//         let sqlite_path = td.path().join("settings.sqlite3");
//         Self { _td: td, oxigraph_dir, sqlite_path }
//     }
// }
//
// async fn fresh_oxigraph_ontology_repo() -> OxigraphOntologyRepository {
//     let dd = DataDir::new();
//     let leaked: &'static DataDir = Box::leak(Box::new(dd));
//     OxigraphOntologyRepository::open(&leaked.oxigraph_dir)
//         .await
//         .expect("OxigraphOntologyRepository::open must succeed")
// }
//
// async fn fresh_oxigraph_graph_repo() -> OxigraphGraphRepository {
//     let dd = DataDir::new();
//     let leaked: &'static DataDir = Box::leak(Box::new(dd));
//     OxigraphGraphRepository::open(&leaked.oxigraph_dir)
//         .await
//         .expect("OxigraphGraphRepository::open must succeed")
// }
//
// async fn fresh_sqlite_settings_repo() -> SqliteSettingsRepository {
//     let dd = DataDir::new();
//     let leaked: &'static DataDir = Box::leak(Box::new(dd));
//     SqliteSettingsRepository::open(&leaked.sqlite_path)
//         .await
//         .expect("SqliteSettingsRepository::open must succeed")
// }
//
// #[tokio::test]
// async fn oxigraph_ontology_parity_battery() {
//     ontology_parity::run_all(|| async { fresh_oxigraph_ontology_repo().await }).await;
// }
//
// #[tokio::test]
// async fn oxigraph_graph_parity_battery() {
//     graph_parity::run_all(|| async { fresh_oxigraph_graph_repo().await }).await;
// }
//
// #[tokio::test]
// async fn sqlite_settings_parity_battery() {
//     settings_parity::run_all(|| async { fresh_sqlite_settings_repo().await }).await;
// }
//
// #[tokio::test]
// async fn oxigraph_invariant_ontology_class_not_in_knowledge_graph() {
//     let dd = DataDir::new();
//     let ont = OxigraphOntologyRepository::open(&dd.oxigraph_dir).await.unwrap();
//     let graph = OxigraphGraphRepository::open(&dd.oxigraph_dir).await.unwrap();
//     named_graph_invariants::invariant_ontology_class_not_in_knowledge_graph(ont, graph).await;
// }
//
// #[tokio::test]
// async fn oxigraph_invariant_knowledge_node_not_in_ontology_list() {
//     let dd = DataDir::new();
//     let ont = OxigraphOntologyRepository::open(&dd.oxigraph_dir).await.unwrap();
//     let graph = OxigraphGraphRepository::open(&dd.oxigraph_dir).await.unwrap();
//     named_graph_invariants::invariant_knowledge_node_not_in_ontology_list(ont, graph).await;
// }
//
// #[tokio::test]
// async fn oxigraph_invariant_knowledge_vs_agent_graph_isolated() {
//     let graph = fresh_oxigraph_graph_repo().await;
//     named_graph_invariants::invariant_knowledge_vs_agent_graph_isolated(graph).await;
// }
//
// #[tokio::test]
// async fn oxigraph_invariant_inferred_does_not_leak_into_asserted() {
//     let ont = fresh_oxigraph_ontology_repo().await;
//     named_graph_invariants::invariant_inferred_does_not_leak_into_asserted(ont).await;
// }
//
// #[tokio::test]
// async fn oxigraph_invariant_class_remove_does_not_cascade_to_kg() {
//     let dd = DataDir::new();
//     let ont = OxigraphOntologyRepository::open(&dd.oxigraph_dir).await.unwrap();
//     let graph = OxigraphGraphRepository::open(&dd.oxigraph_dir).await.unwrap();
//     named_graph_invariants::invariant_class_remove_does_not_cascade_to_kg(ont, graph).await;
// }
