//! VisionClaw Domain Crate
//!
//! Pure domain model — types, errors, events, and port traits with no framework
//! or infrastructure dependencies. Per ADR-090 (Hexagonal Crate Modularisation),
//! this is the innermost crate of the workspace: every other VisionClaw crate
//! depends on it, and it depends only on `visionclaw-contracts` plus a small
//! set of pure-Rust ecosystem crates (serde, chrono, log, async-trait, rand).
//!
//! Layout:
//! - `models/`    — node/edge/graph/metadata/constraints + simulation params
//! - `types/`     — value objects (vec3, layout, physics config, MCP responses…)
//! - `errors/`    — unified error enum (`VisionClawError`)
//! - `utils/`     — small framework-agnostic helpers (`json`, `time`)
//! - `events/`    — domain events (extracted incrementally — see ADR-090 Phase 1b)
//! - `ports/`     — port trait interfaces (extracted incrementally — see ADR-090 Phase 1b)

pub mod analytics;
pub mod config;
pub mod errors;
pub mod events;
pub mod models;
pub mod ports;
pub mod relations;
pub mod telemetry;
pub mod types;
pub mod uri;
pub mod utils;
pub mod vault;
pub mod weighting;

// Convenience re-exports — keep this list focused on the *shared kernel*
// (types every other crate needs). Specialised types stay behind their
// module path to avoid namespace collisions when the workspace grows.
pub use errors::{VisionClawError, VisionClawResult};
pub use models::{
    Edge, FeatureFlags, GraphData, MetadataStore, Node, PaginationParams, SemanticEdgeType,
    SettleMode, SimulationMode, SimulationParams, SimulationPhase,
};
pub use types::{
    AutoBalanceConfig, AutoBalanceNotification, AutoPauseConfig, BinaryNodeData, LayoutMode,
    PhysicsSettings, PhysicsState, Vec3Data,
};
