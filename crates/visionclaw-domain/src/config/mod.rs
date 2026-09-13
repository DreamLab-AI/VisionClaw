//! Settings type definitions — promoted from webxr/src/config/ per ADR-090 Phase A6 slice 3.
//!
//! Pure data structs (serde DTOs) only. Config *loading* (TOML parser, env-var
//! overrides, hot-reload) stays in visionclaw-server.

pub mod app_settings;
pub mod field_mappings;
pub mod graph_type;
pub mod services;
pub mod system;
pub mod validation;
pub mod visualisation;
pub mod xr;

// Flat re-exports matching the old `crate::config::*` surface
pub use app_settings::{AppFullSettings, DeveloperConfig, FeatureFlags, UserPreferences};

pub use graph_type::{
    graphs_map_has_knowledge, knowledge_graph_value, normalise_graph_type,
    path_targets_knowledge_graph,
};

pub use validation::{
    validate_bloom_glow_settings, validate_hex_color, validate_percentage, validate_port,
    validate_width_range,
};

pub use visualisation::{
    AnimationSettings, BloomSettings, CameraSettings, EdgeSettings, GlowSettings, GraphSettings,
    GraphsSettings, HologramSettings, LabelSettings, NodeSettings, Position, RenderingSettings,
    Sensitivity, SpacePilotSettings, VisualisationSettings,
};

pub use system::{
    DebugSettings, NetworkSettings, SecuritySettings, SystemSettings, WebSocketSettings,
};

pub use xr::{MovementAxes, XRSettings};

pub use services::{
    AgentVoicePreset, AuthSettings, LiveKitSettings, OntologyAgentSettings, OpenAISettings,
    PerplexitySettings, PocketTtsSettings, RagFlowSettings, TurboWhisperSettings,
    VoiceRoutingSettings, WhisperSettings,
};
