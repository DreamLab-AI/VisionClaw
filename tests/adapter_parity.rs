// tests/adapter_parity.rs
//! Top-level entry for the persistence-port parity harness.
//!
//! Rust's integration-test discovery treats every `.rs` file directly under
//! `tests/` as an independent test binary. To share helper modules in a
//! `tests/<name>/` subdirectory we declare them here.
//!
//! The harness itself lives in `tests/adapter_parity/`. See
//! `tests/adapter_parity/mod.rs` for the full reading order and the
//! `docs/migration-sprint/11-persistence-migration/TEST-PLAN.md` for the
//! narrative.
//!
//! ## Quick reference
//!
//! Run everything (Neo4j tests will be marked `#[ignore]` unless
//! `--features test-neo4j` plus a live Neo4j is provided):
//! ```bash
//! cargo test --test adapter_parity
//! ```
//!
//! Run against Neo4j:
//! ```bash
//! NEO4J_PASSWORD=test ALLOW_INSECURE_DEFAULTS=true \
//!     cargo test --features test-neo4j --test adapter_parity -- --include-ignored
//! ```
//!
//! Run against Oxigraph (once scaffolded):
//! ```bash
//! cargo test --features persistence-oxigraph --test adapter_parity -- --include-ignored
//! ```

#[path = "adapter_parity/mod.rs"]
mod adapter_parity;
