// Unified Settings Handler - Single source of truth: AppFullSettings
// Split into submodules for maintainability.

pub mod conversions;
pub mod enhanced;
pub mod helpers;
pub mod physics;
pub mod routes;
pub mod types;
pub mod validation;
pub mod write_handlers;

// Re-export all public types from types module
pub use types::{
    value_type_name, AgentColorsDTO, AnimationSettingsDTO, AuthSettingsDTO, AutoBalanceConfigDTO,
    CameraSettingsDTO, DebugSettingsDTO, EdgeSettingsDTO, GlowSettingsDTO, GraphSettingsDTO,
    GraphsSettingsDTO, HologramSettingsDTO, LabelSettingsDTO, MovementAxesDTO, NetworkSettingsDTO,
    NodeSettingsDTO, OpenAISettingsDTO, PerplexitySettingsDTO, PhysicsSettingsDTO,
    PocketTtsSettingsDTO, PositionDTO, RagFlowSettingsDTO, RenderingSettingsDTO,
    SecuritySettingsDTO, SensitivityDTO, SettingsResponseDTO, SettingsUpdateDTO,
    SettingsValidationError, SpacePilotSettingsDTO, SystemSettingsDTO, VisualisationSettingsDTO,
    WebSocketSettingsDTO, WhisperSettingsDTO, XRSettingsDTO,
};

// Re-export enhanced handler
pub use enhanced::EnhancedSettingsHandler;

// Re-export route configuration
pub use routes::config;

// Re-export batch operations (used externally)
pub use write_handlers::{batch_get_settings, batch_update_settings};

// Re-export physics propagation (used by other handlers)
pub use physics::propagate_physics_to_gpu;
pub use physics::propagate_physics_to_gpu_with_layout;

// Re-export helpers (used externally)
pub use helpers::{
    count_fields, create_physics_settings_update, extract_failed_field, extract_physics_updates,
    get_field_variant,
};

// Re-export validation functions (used externally)
pub use validation::{validate_constraints, validate_settings_update, validate_xr_settings};
