pub mod dev_config;
pub mod feature_access;
pub mod path_access;
mod path_accessible_impls;
pub mod security_profile;

/// Canonical default for max_velocity across the entire codebase.
/// All modules MUST use this constant instead of hardcoded values.
pub const CANONICAL_MAX_VELOCITY: f32 = 200.0;

/// Canonical default for max_force across the entire codebase.
/// All modules MUST use this constant instead of hardcoded values.
pub const CANONICAL_MAX_FORCE: f32 = 50.0;

// ---------------------------------------------------------------------------
// ADR-090 Phase A6 slice 3: settings types promoted to visionclaw-domain.
// Re-export everything so existing `use crate::config::*` callers are
// unaffected.
// ---------------------------------------------------------------------------

// ADR-2041: the graph-type vocabulary and the bounded `logseq` alias have
// exactly one definition, in the domain crate's `config::graph_type`.
pub use visionclaw_domain::config::graph_type::{
    graphs_map_has_knowledge, knowledge_graph_value, normalise_graph_type,
    path_targets_knowledge_graph,
};

pub use visionclaw_domain::config::validation::{
    validate_bloom_glow_settings, validate_hex_color, validate_percentage, validate_port,
    validate_width_range,
};

pub use visionclaw_domain::config::visualisation::{
    AnimationSettings, BloomSettings, CameraSettings, EdgeSettings, GlowSettings, GraphSettings,
    GraphsSettings, HologramSettings, LabelSettings, NodeSettings, Position, RenderingSettings,
    Sensitivity, SpacePilotSettings, VisualisationSettings,
};

pub use visionclaw_domain::config::system::{
    DebugSettings, NetworkSettings, SecuritySettings, SystemSettings, WebSocketSettings,
};

pub use visionclaw_domain::config::xr::{MovementAxes, XRSettings};

pub use visionclaw_domain::config::services::{
    AgentVoicePreset, AuthSettings, LiveKitSettings, OntologyAgentSettings, OpenAISettings,
    PerplexitySettings, PocketTtsSettings, RagFlowSettings, TurboWhisperSettings,
    VoiceRoutingSettings, WhisperSettings,
};

pub use visionclaw_domain::config::{
    AppFullSettings, DeveloperConfig, FeatureFlags, UserPreferences,
};

// PhysicsSettings and siblings already live in domain/types — keep the physics
// submodule path so `crate::config::physics::PhysicsSettings` still resolves.
pub mod physics {
    pub use visionclaw_domain::types::physics_config::{
        AutoBalanceConfig, AutoPauseConfig, ClusteringConfiguration, ConstraintSystem,
        LegacyConstraintData, PhysicsSettings, PhysicsUpdate,
    };
}

pub use physics::{
    AutoBalanceConfig, AutoPauseConfig, ClusteringConfiguration, ConstraintSystem,
    LegacyConstraintData, PhysicsSettings, PhysicsUpdate,
};

// Re-export path_access traits
pub use path_access::{JsonPathAccessible, PathAccessible};
