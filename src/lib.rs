pub mod actors;
pub mod adapters;
pub mod agent_events;
pub mod app_state;
pub mod application;
pub mod client;
pub mod config;
pub mod constraints;
pub mod layout;
// Storage-agnostic domain kernels (ADR-130 Decision 2 broker kernel lives here).
pub mod domain;
pub mod errors;
pub mod events;
pub mod gpu;
pub mod handlers;
pub mod inference;
pub mod middleware;
pub mod models;
pub mod ontology;
pub mod openapi;
pub mod physics;
pub mod ports;
pub mod protocol;
pub mod reasoning;
pub mod repositories;
pub mod services;
pub mod settings;
pub mod telemetry;
pub mod types;
// Converged urn:visionclaw minter (BC20 counterpart). Coexists with the legacy
// urn:ngm scheme until that is retired.
pub mod uri;

// Import utils with macro_use to make response macros available everywhere
#[macro_use]
pub mod utils;
pub mod validation;

// ADR-124 build-out: the gitmark/blocktrails web-contract substrate (4-layer
// reducer/state/ledger/trail + validate/anchor/verify ritual). Identity-rail-
// agnostic — carries did:nostr strings unchanged (I1-I4 hold trivially).
// Its trail type and walker are solid-pod-rs's (`solid_pod_rs::blocktrail`).
#[cfg(feature = "solid-pod-embed")]
pub mod web_contract;

// #[cfg(test)]
// pub mod test_settings_fix;

pub mod test_helpers;

pub use actors::{ClientCoordinatorActor, MetadataActor, OptimizedSettingsActor};
pub use app_state::AppState;
// ADR-090: model types now canonical in visionclaw-domain. Re-exports here
// preserve the old `visionclaw_server::MetadataStore` etc. API for external callers
// (tests, downstream binaries) without forcing them through a long import.
pub use models::user_settings::UserSettings;
pub use visionclaw_domain::models::metadata::MetadataStore;
pub use visionclaw_domain::models::protected_settings::ProtectedSettings;
pub use visionclaw_domain::models::simulation_params::SimulationParams;

// Re-export commonly used utilities for easier access
pub use utils::json::{from_json, to_json};
pub use utils::result_helpers::safe_json_number;
pub use utils::time;
// Re-export HandlerResponse trait for response macros
pub use utils::handler_commons::HandlerResponse;
