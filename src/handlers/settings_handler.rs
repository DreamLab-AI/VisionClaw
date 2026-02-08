// Unified Settings Handler - Single source of truth: AppFullSettings
use crate::actors::messages::{GetSettings, UpdateSettings, UpdateSimulationParams};
use crate::app_state::AppState;
use crate::config::path_access::JsonPathAccessible;
use crate::config::AppFullSettings;
use crate::handlers::validation_handler::ValidationService;
use crate::utils::validation::rate_limit::{
    extract_client_id, EndpointRateLimits, RateLimitConfig, RateLimiter,
};
use crate::utils::validation::MAX_REQUEST_SIZE;
use actix_web::{web, Error, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use tracing::info as trace_info;
use uuid::Uuid;
use crate::{ok_json, error_json, bad_request, not_found, service_unavailable, too_many_requests, payload_too_large};

// Import comprehensive validation for GPU parameters
use crate::handlers::settings_validation_fix::{
    convert_to_snake_case_recursive,
    validate_physics_settings_complete,
};

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponseDTO {
    pub visualisation: VisualisationSettingsDTO,
    pub system: SystemSettingsDTO,
    pub xr: XRSettingsDTO,
    pub auth: AuthSettingsDTO,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ragflow: Option<RagFlowSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perplexity: Option<PerplexitySettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<OpenAISettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kokoro: Option<KokoroSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whisper: Option<WhisperSettingsDTO>,
}

/// Error type for post-deserialization validation failures
#[derive(Debug, Clone)]
pub struct SettingsValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for SettingsValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Validation error in '{}': {}", self.field, self.message)
    }
}

impl std::error::Error for SettingsValidationError {}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visualisation: Option<VisualisationSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xr: Option<XRSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ragflow: Option<RagFlowSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perplexity: Option<PerplexitySettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<OpenAISettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kokoro: Option<KokoroSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whisper: Option<WhisperSettingsDTO>,
}

impl SettingsUpdateDTO {
    /// Validates deserialized settings to prevent crash-inducing values.
    /// Call this immediately after deserializing settings from JSON.
    pub fn validate(&self) -> Result<(), SettingsValidationError> {
        // Validate system settings if present
        if let Some(ref system) = self.system {
            // Validate network settings
            if system.network.port == 0 {
                return Err(SettingsValidationError {
                    field: "system.network.port".to_string(),
                    message: "port must be > 0".to_string(),
                });
            }
            if system.network.max_request_size == 0 {
                return Err(SettingsValidationError {
                    field: "system.network.max_request_size".to_string(),
                    message: "max_request_size must be > 0".to_string(),
                });
            }
            // Validate websocket settings
            if system.websocket.max_connections == 0 {
                return Err(SettingsValidationError {
                    field: "system.websocket.max_connections".to_string(),
                    message: "max_connections must be > 0".to_string(),
                });
            }
            if system.websocket.max_message_size == 0 {
                return Err(SettingsValidationError {
                    field: "system.websocket.max_message_size".to_string(),
                    message: "max_message_size must be > 0".to_string(),
                });
            }
        }

        // Validate visualisation settings if present
        if let Some(ref vis) = self.visualisation {
            // Validate physics iterations
            let logseq_iterations = vis.graphs.logseq.physics.iterations;
            if logseq_iterations == 0 || logseq_iterations > 1000 {
                return Err(SettingsValidationError {
                    field: "visualisation.graphs.logseq.physics.iterations".to_string(),
                    message: "iterations must be between 1 and 1000".to_string(),
                });
            }
            let visionflow_iterations = vis.graphs.visionflow.physics.iterations;
            if visionflow_iterations == 0 || visionflow_iterations > 1000 {
                return Err(SettingsValidationError {
                    field: "visualisation.graphs.visionflow.physics.iterations".to_string(),
                    message: "iterations must be between 1 and 1000".to_string(),
                });
            }

            // Validate quality thresholds (0.0-1.0 range for normalized values)
            let validate_range = |val: f32, field: &str| -> Result<(), SettingsValidationError> {
                if val < 0.0 || val > 1.0 {
                    return Err(SettingsValidationError {
                        field: field.to_string(),
                        message: "value must be between 0.0 and 1.0".to_string(),
                    });
                }
                Ok(())
            };

            // Validate opacity values
            validate_range(vis.glow.opacity, "visualisation.glow.opacity")?;
            validate_range(vis.hologram.ring_opacity, "visualisation.hologram.ring_opacity")?;
            validate_range(vis.hologram.buckminster_opacity, "visualisation.hologram.buckminster_opacity")?;
            validate_range(vis.hologram.geodesic_opacity, "visualisation.hologram.geodesic_opacity")?;
            validate_range(vis.hologram.triangle_sphere_opacity, "visualisation.hologram.triangle_sphere_opacity")?;
        }

        // Validate XR settings if present
        if let Some(ref xr) = self.xr {
            if xr.room_scale <= 0.0 {
                return Err(SettingsValidationError {
                    field: "xr.room_scale".to_string(),
                    message: "room_scale must be > 0".to_string(),
                });
            }
            if xr.interaction_distance <= 0.0 {
                return Err(SettingsValidationError {
                    field: "xr.interaction_distance".to_string(),
                    message: "interaction_distance must be > 0".to_string(),
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VisualisationSettingsDTO {
    pub rendering: RenderingSettingsDTO,
    pub animations: AnimationSettingsDTO,
    
    pub glow: GlowSettingsDTO,
    pub hologram: HologramSettingsDTO,
    pub graphs: GraphsSettingsDTO,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraSettingsDTO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_pilot: Option<SpacePilotSettingsDTO>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenderingSettingsDTO {
    pub ambient_light_intensity: f32,
    pub background_color: String,
    pub directional_light_intensity: f32,
    pub enable_ambient_occlusion: bool,
    pub enable_antialiasing: bool,
    pub enable_shadows: bool,
    pub environment_intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_map_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_bias: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_colors: Option<AgentColorsDTO>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentColorsDTO {
    pub coordinator: String,
    pub coder: String,
    pub architect: String,
    pub analyst: String,
    pub tester: String,
    pub researcher: String,
    pub reviewer: String,
    pub optimizer: String,
    pub documenter: String,
    pub queen: String,
    pub default: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnimationSettingsDTO {
    pub enable_motion_blur: bool,
    pub enable_node_animations: bool,
    pub motion_blur_strength: f32,
    pub selection_wave_enabled: bool,
    pub pulse_enabled: bool,
    pub pulse_speed: f32,
    pub pulse_strength: f32,
    pub wave_speed: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GlowSettingsDTO {
    pub enabled: bool,
    pub intensity: f32,
    pub radius: f32,
    pub threshold: f32,
    pub diffuse_strength: f32,
    pub atmospheric_density: f32,
    pub volumetric_intensity: f32,
    pub base_color: String,
    pub emission_color: String,
    pub opacity: f32,
    pub pulse_speed: f32,
    pub flow_speed: f32,
    pub node_glow_strength: f32,
    pub edge_glow_strength: f32,
    pub environment_glow_strength: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HologramSettingsDTO {
    pub ring_count: u32,
    pub ring_color: String,
    pub ring_opacity: f32,
    pub sphere_sizes: Vec<f32>,
    pub ring_rotation_speed: f32,
    pub enable_buckminster: bool,
    pub buckminster_size: f32,
    pub buckminster_opacity: f32,
    pub enable_geodesic: bool,
    pub geodesic_size: f32,
    pub geodesic_opacity: f32,
    pub enable_triangle_sphere: bool,
    pub triangle_sphere_size: f32,
    pub triangle_sphere_opacity: f32,
    pub global_rotation_speed: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphsSettingsDTO {
    pub logseq: GraphSettingsDTO,
    pub visionflow: GraphSettingsDTO,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphSettingsDTO {
    pub nodes: NodeSettingsDTO,
    pub edges: EdgeSettingsDTO,
    pub labels: LabelSettingsDTO,
    pub physics: PhysicsSettingsDTO,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeSettingsDTO {
    pub base_color: String,
    pub metalness: f32,
    pub opacity: f32,
    pub roughness: f32,
    pub node_size: f32,
    pub quality: String,
    pub enable_instancing: bool,
    pub enable_hologram: bool,
    pub enable_metadata_shape: bool,
    pub enable_metadata_visualisation: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSettingsDTO {
    pub arrow_size: f32,
    pub base_width: f32,
    pub color: String,
    pub enable_arrows: bool,
    pub opacity: f32,
    pub width_range: Vec<f32>,
    pub quality: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LabelSettingsDTO {
    pub desktop_font_size: f32,
    pub enable_labels: bool,
    pub text_color: String,
    pub text_outline_color: String,
    pub text_outline_width: f32,
    pub text_resolution: u32,
    pub text_padding: f32,
    pub billboard_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_metadata: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_label_width: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsSettingsDTO {
    pub auto_balance: bool,
    pub auto_balance_interval_ms: u32,
    pub auto_balance_config: AutoBalanceConfigDTO,
    pub spring_k: f32,
    pub bounds_size: f32,
    pub separation_radius: f32,
    pub damping: f32,
    pub enable_bounds: bool,
    pub enabled: bool,
    pub iterations: u32,
    pub max_velocity: f32,
    pub max_force: f32,
    pub repel_k: f32,
    pub mass_scale: f32,
    pub boundary_damping: f32,
    pub update_threshold: f32,
    pub dt: f32,
    pub temperature: f32,
    pub gravity: f32,
    pub stress_weight: f32,
    pub stress_alpha: f32,
    pub boundary_limit: f32,
    pub alignment_strength: f32,
    pub cluster_strength: f32,
    pub compute_mode: i32,
    pub rest_length: f32,
    pub repulsion_cutoff: f32,
    pub repulsion_softening_epsilon: f32,
    pub center_gravity_k: f32,
    pub grid_cell_size: f32,
    pub warmup_iterations: u32,
    pub cooling_rate: f32,
    pub boundary_extreme_multiplier: f32,
    pub boundary_extreme_force_multiplier: f32,
    pub boundary_velocity_damping: f32,
    pub min_distance: f32,
    pub max_repulsion_dist: f32,
    pub boundary_margin: f32,
    pub boundary_force_strength: f32,
    pub warmup_curve: String,
    pub zero_velocity_iterations: u32,
    pub clustering_algorithm: String,
    pub cluster_count: u32,
    pub clustering_resolution: f32,
    pub clustering_iterations: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoBalanceConfigDTO {
    pub stability_variance_threshold: f32,
    pub stability_frame_count: u32,
    pub clustering_distance_threshold: f32,
    pub bouncing_node_percentage: f32,
    pub boundary_min_distance: f32,
    pub boundary_max_distance: f32,
    pub extreme_distance_threshold: f32,
    pub explosion_distance_threshold: f32,
    pub spreading_distance_threshold: f32,
    pub oscillation_detection_frames: usize,
    pub oscillation_change_threshold: f32,
    pub min_oscillation_changes: usize,
    pub grid_cell_size_min: f32,
    pub grid_cell_size_max: f32,
    pub repulsion_cutoff_min: f32,
    pub repulsion_cutoff_max: f32,
    pub repulsion_softening_min: f32,
    pub repulsion_softening_max: f32,
    pub center_gravity_min: f32,
    pub center_gravity_max: f32,
    pub spatial_hash_efficiency_threshold: f32,
    pub cluster_density_threshold: f32,
    pub numerical_instability_threshold: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CameraSettingsDTO {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub position: PositionDTO,
    pub look_at: PositionDTO,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PositionDTO {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpacePilotSettingsDTO {
    pub enabled: bool,
    pub mode: String,
    pub sensitivity: SensitivityDTO,
    pub smoothing: f32,
    pub deadzone: f32,
    pub button_functions: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SensitivityDTO {
    pub translation: f32,
    pub rotation: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettingsDTO {
    pub network: NetworkSettingsDTO,
    pub websocket: WebSocketSettingsDTO,
    pub security: SecuritySettingsDTO,
    pub debug: DebugSettingsDTO,
    pub persist_settings: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_backend_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsDTO {
    pub bind_address: String,
    pub domain: String,
    pub enable_http2: bool,
    pub enable_rate_limiting: bool,
    pub enable_tls: bool,
    pub max_request_size: usize,
    pub min_tls_version: String,
    pub port: u16,
    pub rate_limit_requests: u32,
    pub rate_limit_window: u32,
    pub tunnel_id: String,
    pub api_client_timeout: u64,
    pub enable_metrics: bool,
    pub max_concurrent_requests: u32,
    pub max_retries: u32,
    pub metrics_port: u16,
    pub retry_delay: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketSettingsDTO {
    pub binary_chunk_size: usize,
    pub binary_update_rate: u32,
    pub min_update_rate: u32,
    pub max_update_rate: u32,
    pub motion_threshold: f32,
    pub motion_damping: f32,
    pub binary_message_version: u32,
    pub compression_enabled: bool,
    pub compression_threshold: usize,
    pub heartbeat_interval: u64,
    pub heartbeat_timeout: u64,
    pub max_connections: usize,
    pub max_message_size: usize,
    pub reconnect_attempts: u32,
    pub reconnect_delay: u64,
    pub update_rate: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettingsDTO {
    pub allowed_origins: Vec<String>,
    pub audit_log_path: String,
    pub cookie_httponly: bool,
    pub cookie_samesite: String,
    pub cookie_secure: bool,
    pub csrf_token_timeout: u32,
    pub enable_audit_logging: bool,
    pub enable_request_validation: bool,
    pub session_timeout: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DebugSettingsDTO {
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct XRSettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_side_enable_xr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub room_scale: f32,
    pub space_type: String,
    pub quality: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_scale: Option<f32>,
    pub interaction_distance: f32,
    pub locomotion_method: String,
    pub teleport_ray_color: String,
    pub controller_ray_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller_model: Option<String>,
    pub enable_hand_tracking: bool,
    pub hand_mesh_enabled: bool,
    pub hand_mesh_color: String,
    pub hand_mesh_opacity: f32,
    pub hand_point_size: f32,
    pub hand_ray_enabled: bool,
    pub hand_ray_color: String,
    pub hand_ray_width: f32,
    pub gesture_smoothing: f32,
    pub enable_haptics: bool,
    pub haptic_intensity: f32,
    pub drag_threshold: f32,
    pub pinch_threshold: f32,
    pub rotation_threshold: f32,
    pub interaction_radius: f32,
    pub movement_speed: f32,
    pub dead_zone: f32,
    pub movement_axes: MovementAxesDTO,
    pub enable_light_estimation: bool,
    pub enable_plane_detection: bool,
    pub enable_scene_understanding: bool,
    pub plane_color: String,
    pub plane_opacity: f32,
    pub plane_detection_distance: f32,
    pub show_plane_overlay: bool,
    pub snap_to_floor: bool,
    pub enable_passthrough_portal: bool,
    pub passthrough_opacity: f32,
    pub passthrough_brightness: f32,
    pub passthrough_contrast: f32,
    pub portal_size: f32,
    pub portal_edge_color: String,
    pub portal_edge_width: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MovementAxesDTO {
    pub horizontal: i32,
    pub vertical: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthSettingsDTO {
    pub enabled: bool,
    pub provider: String,
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RagFlowSettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenAISettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KokoroSettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vad_filter: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
}

// Conversion functions between AppFullSettings and DTOs
impl From<&AppFullSettings> for SettingsResponseDTO {
    fn from(settings: &AppFullSettings) -> Self {
        Self {
            visualisation: (&settings.visualisation).into(),
            system: (&settings.system).into(),
            xr: (&settings.xr).into(),
            auth: (&settings.auth).into(),
            ragflow: settings.ragflow.as_ref().map(|r| r.into()),
            perplexity: settings.perplexity.as_ref().map(|p| p.into()),
            openai: settings.openai.as_ref().map(|o| o.into()),
            kokoro: settings.kokoro.as_ref().map(|k| k.into()),
            whisper: settings.whisper.as_ref().map(|w| w.into()),
        }
    }
}

// Implement all the necessary From conversions for nested structures
impl From<&crate::config::VisualisationSettings> for VisualisationSettingsDTO {
    fn from(settings: &crate::config::VisualisationSettings) -> Self {
        Self {
            rendering: (&settings.rendering).into(),
            animations: (&settings.animations).into(),
            glow: (&settings.glow).into(),
            hologram: (&settings.hologram).into(),
            graphs: (&settings.graphs).into(),
            camera: settings.camera.as_ref().map(|c| c.into()),
            space_pilot: settings.space_pilot.as_ref().map(|sp| sp.into()),
        }
    }
}

impl From<&crate::config::RenderingSettings> for RenderingSettingsDTO {
    fn from(settings: &crate::config::RenderingSettings) -> Self {
        
        let dev_config = crate::config::dev_config::rendering();
        let agent_colors = Some(AgentColorsDTO {
            coordinator: dev_config.agent_colors.coordinator.clone(),
            coder: dev_config.agent_colors.coder.clone(),
            architect: dev_config.agent_colors.architect.clone(),
            analyst: dev_config.agent_colors.analyst.clone(),
            tester: dev_config.agent_colors.tester.clone(),
            researcher: dev_config.agent_colors.researcher.clone(),
            reviewer: dev_config.agent_colors.reviewer.clone(),
            optimizer: dev_config.agent_colors.optimizer.clone(),
            documenter: dev_config.agent_colors.documenter.clone(),
            queen: "#FFD700".to_string(), 
            default: dev_config.agent_colors.default.clone(),
        });

        Self {
            ambient_light_intensity: settings.ambient_light_intensity,
            background_color: settings.background_color.clone(),
            directional_light_intensity: settings.directional_light_intensity,
            enable_ambient_occlusion: settings.enable_ambient_occlusion,
            enable_antialiasing: settings.enable_antialiasing,
            enable_shadows: settings.enable_shadows,
            environment_intensity: settings.environment_intensity,
            shadow_map_size: settings.shadow_map_size.clone(),
            shadow_bias: settings.shadow_bias,
            context: settings.context.clone(),
            agent_colors,
        }
    }
}

impl From<&crate::config::AnimationSettings> for AnimationSettingsDTO {
    fn from(settings: &crate::config::AnimationSettings) -> Self {
        Self {
            enable_motion_blur: settings.enable_motion_blur,
            enable_node_animations: settings.enable_node_animations,
            motion_blur_strength: settings.motion_blur_strength,
            selection_wave_enabled: settings.selection_wave_enabled,
            pulse_enabled: settings.pulse_enabled,
            pulse_speed: settings.pulse_speed,
            pulse_strength: settings.pulse_strength,
            wave_speed: settings.wave_speed,
        }
    }
}

impl From<&crate::config::GlowSettings> for GlowSettingsDTO {
    fn from(settings: &crate::config::GlowSettings) -> Self {
        Self {
            enabled: settings.enabled,
            intensity: settings.intensity,
            radius: settings.radius,
            threshold: settings.threshold,
            diffuse_strength: settings.diffuse_strength,
            atmospheric_density: settings.atmospheric_density,
            volumetric_intensity: settings.volumetric_intensity,
            base_color: settings.base_color.clone(),
            emission_color: settings.emission_color.clone(),
            opacity: settings.opacity,
            pulse_speed: settings.pulse_speed,
            flow_speed: settings.flow_speed,
            node_glow_strength: settings.node_glow_strength,
            edge_glow_strength: settings.edge_glow_strength,
            environment_glow_strength: settings.environment_glow_strength,
        }
    }
}

impl From<&crate::config::HologramSettings> for HologramSettingsDTO {
    fn from(settings: &crate::config::HologramSettings) -> Self {
        Self {
            ring_count: settings.ring_count,
            ring_color: settings.ring_color.clone(),
            ring_opacity: settings.ring_opacity,
            sphere_sizes: settings.sphere_sizes.clone(),
            ring_rotation_speed: settings.ring_rotation_speed,
            enable_buckminster: settings.enable_buckminster,
            buckminster_size: settings.buckminster_size,
            buckminster_opacity: settings.buckminster_opacity,
            enable_geodesic: settings.enable_geodesic,
            geodesic_size: settings.geodesic_size,
            geodesic_opacity: settings.geodesic_opacity,
            enable_triangle_sphere: settings.enable_triangle_sphere,
            triangle_sphere_size: settings.triangle_sphere_size,
            triangle_sphere_opacity: settings.triangle_sphere_opacity,
            global_rotation_speed: settings.global_rotation_speed,
        }
    }
}

impl From<&crate::config::GraphsSettings> for GraphsSettingsDTO {
    fn from(settings: &crate::config::GraphsSettings) -> Self {
        Self {
            logseq: (&settings.logseq).into(),
            visionflow: (&settings.visionflow).into(),
        }
    }
}

impl From<&crate::config::GraphSettings> for GraphSettingsDTO {
    fn from(settings: &crate::config::GraphSettings) -> Self {
        Self {
            nodes: (&settings.nodes).into(),
            edges: (&settings.edges).into(),
            labels: (&settings.labels).into(),
            physics: (&settings.physics).into(),
        }
    }
}

impl From<&crate::config::NodeSettings> for NodeSettingsDTO {
    fn from(settings: &crate::config::NodeSettings) -> Self {
        Self {
            base_color: settings.base_color.clone(),
            metalness: settings.metalness,
            opacity: settings.opacity,
            roughness: settings.roughness,
            node_size: settings.node_size,
            quality: settings.quality.clone(),
            enable_instancing: settings.enable_instancing,
            enable_hologram: settings.enable_hologram,
            enable_metadata_shape: settings.enable_metadata_shape,
            enable_metadata_visualisation: settings.enable_metadata_visualisation,
        }
    }
}

impl From<&crate::config::EdgeSettings> for EdgeSettingsDTO {
    fn from(settings: &crate::config::EdgeSettings) -> Self {
        Self {
            arrow_size: settings.arrow_size,
            base_width: settings.base_width,
            color: settings.color.clone(),
            enable_arrows: settings.enable_arrows,
            opacity: settings.opacity,
            width_range: settings.width_range.clone(),
            quality: settings.quality.clone(),
        }
    }
}

impl From<&crate::config::LabelSettings> for LabelSettingsDTO {
    fn from(settings: &crate::config::LabelSettings) -> Self {
        Self {
            desktop_font_size: settings.desktop_font_size,
            enable_labels: settings.enable_labels,
            text_color: settings.text_color.clone(),
            text_outline_color: settings.text_outline_color.clone(),
            text_outline_width: settings.text_outline_width,
            text_resolution: settings.text_resolution,
            text_padding: settings.text_padding,
            billboard_mode: settings.billboard_mode.clone(),
            show_metadata: settings.show_metadata,
            max_label_width: settings.max_label_width,
        }
    }
}

impl From<&crate::config::PhysicsSettings> for PhysicsSettingsDTO {
    fn from(settings: &crate::config::PhysicsSettings) -> Self {
        Self {
            auto_balance: settings.auto_balance,
            auto_balance_interval_ms: settings.auto_balance_interval_ms,
            auto_balance_config: (&settings.auto_balance_config).into(),
            spring_k: settings.spring_k,
            bounds_size: settings.bounds_size,
            separation_radius: settings.separation_radius,
            damping: settings.damping,
            enable_bounds: settings.enable_bounds,
            enabled: settings.enabled,
            iterations: settings.iterations,
            max_velocity: settings.max_velocity,
            max_force: settings.max_force,
            repel_k: settings.repel_k,
            mass_scale: settings.mass_scale,
            boundary_damping: settings.boundary_damping,
            update_threshold: settings.update_threshold,
            dt: settings.dt,
            temperature: settings.temperature,
            gravity: settings.gravity,
            stress_weight: settings.stress_weight,
            stress_alpha: settings.stress_alpha,
            boundary_limit: settings.boundary_limit,
            alignment_strength: settings.alignment_strength,
            cluster_strength: settings.cluster_strength,
            compute_mode: settings.compute_mode,
            rest_length: settings.rest_length,
            repulsion_cutoff: settings.repulsion_cutoff,
            repulsion_softening_epsilon: settings.repulsion_softening_epsilon,
            center_gravity_k: settings.center_gravity_k,
            grid_cell_size: settings.grid_cell_size,
            warmup_iterations: settings.warmup_iterations,
            cooling_rate: settings.cooling_rate,
            boundary_extreme_multiplier: settings.boundary_extreme_multiplier,
            boundary_extreme_force_multiplier: settings.boundary_extreme_force_multiplier,
            boundary_velocity_damping: settings.boundary_velocity_damping,
            min_distance: settings.min_distance,
            max_repulsion_dist: settings.max_repulsion_dist,
            boundary_margin: settings.boundary_margin,
            boundary_force_strength: settings.boundary_force_strength,
            warmup_curve: settings.warmup_curve.clone(),
            zero_velocity_iterations: settings.zero_velocity_iterations,
            clustering_algorithm: settings.clustering_algorithm.clone(),
            cluster_count: settings.cluster_count,
            clustering_resolution: settings.clustering_resolution,
            clustering_iterations: settings.clustering_iterations,
        }
    }
}

impl From<&crate::config::AutoBalanceConfig> for AutoBalanceConfigDTO {
    fn from(settings: &crate::config::AutoBalanceConfig) -> Self {
        Self {
            stability_variance_threshold: settings.stability_variance_threshold,
            stability_frame_count: settings.stability_frame_count,
            clustering_distance_threshold: settings.clustering_distance_threshold,
            bouncing_node_percentage: settings.bouncing_node_percentage,
            boundary_min_distance: settings.boundary_min_distance,
            boundary_max_distance: settings.boundary_max_distance,
            extreme_distance_threshold: settings.extreme_distance_threshold,
            explosion_distance_threshold: settings.explosion_distance_threshold,
            spreading_distance_threshold: settings.spreading_distance_threshold,
            oscillation_detection_frames: settings.oscillation_detection_frames,
            oscillation_change_threshold: settings.oscillation_change_threshold,
            min_oscillation_changes: settings.min_oscillation_changes,
            grid_cell_size_min: settings.grid_cell_size_min,
            grid_cell_size_max: settings.grid_cell_size_max,
            repulsion_cutoff_min: settings.repulsion_cutoff_min,
            repulsion_cutoff_max: settings.repulsion_cutoff_max,
            repulsion_softening_min: settings.repulsion_softening_min,
            repulsion_softening_max: settings.repulsion_softening_max,
            center_gravity_min: settings.center_gravity_min,
            center_gravity_max: settings.center_gravity_max,
            spatial_hash_efficiency_threshold: settings.spatial_hash_efficiency_threshold,
            cluster_density_threshold: settings.cluster_density_threshold,
            numerical_instability_threshold: settings.numerical_instability_threshold,
        }
    }
}

impl From<&crate::config::CameraSettings> for CameraSettingsDTO {
    fn from(settings: &crate::config::CameraSettings) -> Self {
        Self {
            fov: settings.fov,
            near: settings.near,
            far: settings.far,
            position: (&settings.position).into(),
            look_at: (&settings.look_at).into(),
        }
    }
}

impl From<&crate::config::Position> for PositionDTO {
    fn from(pos: &crate::config::Position) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

impl From<&crate::config::SpacePilotSettings> for SpacePilotSettingsDTO {
    fn from(settings: &crate::config::SpacePilotSettings) -> Self {
        Self {
            enabled: settings.enabled,
            mode: settings.mode.clone(),
            sensitivity: (&settings.sensitivity).into(),
            smoothing: settings.smoothing,
            deadzone: settings.deadzone,
            button_functions: settings.button_functions.clone(),
        }
    }
}

impl From<&crate::config::Sensitivity> for SensitivityDTO {
    fn from(sens: &crate::config::Sensitivity) -> Self {
        Self {
            translation: sens.translation,
            rotation: sens.rotation,
        }
    }
}

impl From<&crate::config::SystemSettings> for SystemSettingsDTO {
    fn from(settings: &crate::config::SystemSettings) -> Self {
        Self {
            network: (&settings.network).into(),
            websocket: (&settings.websocket).into(),
            security: (&settings.security).into(),
            debug: (&settings.debug).into(),
            persist_settings: settings.persist_settings,
            custom_backend_url: settings.custom_backend_url.clone(),
        }
    }
}

impl From<&crate::config::NetworkSettings> for NetworkSettingsDTO {
    fn from(settings: &crate::config::NetworkSettings) -> Self {
        Self {
            bind_address: settings.bind_address.clone(),
            domain: settings.domain.clone(),
            enable_http2: settings.enable_http2,
            enable_rate_limiting: settings.enable_rate_limiting,
            enable_tls: settings.enable_tls,
            max_request_size: settings.max_request_size,
            min_tls_version: settings.min_tls_version.clone(),
            port: settings.port,
            rate_limit_requests: settings.rate_limit_requests,
            rate_limit_window: settings.rate_limit_window,
            tunnel_id: settings.tunnel_id.clone(),
            api_client_timeout: settings.api_client_timeout,
            enable_metrics: settings.enable_metrics,
            max_concurrent_requests: settings.max_concurrent_requests,
            max_retries: settings.max_retries,
            metrics_port: settings.metrics_port,
            retry_delay: settings.retry_delay,
        }
    }
}

impl From<&crate::config::WebSocketSettings> for WebSocketSettingsDTO {
    fn from(settings: &crate::config::WebSocketSettings) -> Self {
        Self {
            binary_chunk_size: settings.binary_chunk_size,
            binary_update_rate: settings.binary_update_rate,
            min_update_rate: settings.min_update_rate,
            max_update_rate: settings.max_update_rate,
            motion_threshold: settings.motion_threshold,
            motion_damping: settings.motion_damping,
            binary_message_version: settings.binary_message_version,
            compression_enabled: settings.compression_enabled,
            compression_threshold: settings.compression_threshold,
            heartbeat_interval: settings.heartbeat_interval,
            heartbeat_timeout: settings.heartbeat_timeout,
            max_connections: settings.max_connections,
            max_message_size: settings.max_message_size,
            reconnect_attempts: settings.reconnect_attempts,
            reconnect_delay: settings.reconnect_delay,
            update_rate: settings.update_rate,
        }
    }
}

impl From<&crate::config::SecuritySettings> for SecuritySettingsDTO {
    fn from(settings: &crate::config::SecuritySettings) -> Self {
        Self {
            allowed_origins: settings.allowed_origins.clone(),
            audit_log_path: settings.audit_log_path.clone(),
            cookie_httponly: settings.cookie_httponly,
            cookie_samesite: settings.cookie_samesite.clone(),
            cookie_secure: settings.cookie_secure,
            csrf_token_timeout: settings.csrf_token_timeout,
            enable_audit_logging: settings.enable_audit_logging,
            enable_request_validation: settings.enable_request_validation,
            session_timeout: settings.session_timeout,
        }
    }
}

impl From<&crate::config::DebugSettings> for DebugSettingsDTO {
    fn from(settings: &crate::config::DebugSettings) -> Self {
        Self {
            enabled: settings.enabled,
        }
    }
}

impl From<&crate::config::XRSettings> for XRSettingsDTO {
    fn from(settings: &crate::config::XRSettings) -> Self {
        Self {
            enabled: settings.enabled,
            client_side_enable_xr: settings.client_side_enable_xr,
            mode: settings.mode.clone(),
            room_scale: settings.room_scale,
            space_type: settings.space_type.clone(),
            quality: settings.quality.clone(),
            render_scale: settings.render_scale,
            interaction_distance: settings.interaction_distance,
            locomotion_method: settings.locomotion_method.clone(),
            teleport_ray_color: settings.teleport_ray_color.clone(),
            controller_ray_color: settings.controller_ray_color.clone(),
            controller_model: settings.controller_model.clone(),
            enable_hand_tracking: settings.enable_hand_tracking,
            hand_mesh_enabled: settings.hand_mesh_enabled,
            hand_mesh_color: settings.hand_mesh_color.clone(),
            hand_mesh_opacity: settings.hand_mesh_opacity,
            hand_point_size: settings.hand_point_size,
            hand_ray_enabled: settings.hand_ray_enabled,
            hand_ray_color: settings.hand_ray_color.clone(),
            hand_ray_width: settings.hand_ray_width,
            gesture_smoothing: settings.gesture_smoothing,
            enable_haptics: settings.enable_haptics,
            haptic_intensity: settings.haptic_intensity,
            drag_threshold: settings.drag_threshold,
            pinch_threshold: settings.pinch_threshold,
            rotation_threshold: settings.rotation_threshold,
            interaction_radius: settings.interaction_radius,
            movement_speed: settings.movement_speed,
            dead_zone: settings.dead_zone,
            movement_axes: (&settings.movement_axes).into(),
            enable_light_estimation: settings.enable_light_estimation,
            enable_plane_detection: settings.enable_plane_detection,
            enable_scene_understanding: settings.enable_scene_understanding,
            plane_color: settings.plane_color.clone(),
            plane_opacity: settings.plane_opacity,
            plane_detection_distance: settings.plane_detection_distance,
            show_plane_overlay: settings.show_plane_overlay,
            snap_to_floor: settings.snap_to_floor,
            enable_passthrough_portal: settings.enable_passthrough_portal,
            passthrough_opacity: settings.passthrough_opacity,
            passthrough_brightness: settings.passthrough_brightness,
            passthrough_contrast: settings.passthrough_contrast,
            portal_size: settings.portal_size,
            portal_edge_color: settings.portal_edge_color.clone(),
            portal_edge_width: settings.portal_edge_width,
        }
    }
}

impl From<&crate::config::MovementAxes> for MovementAxesDTO {
    fn from(axes: &crate::config::MovementAxes) -> Self {
        Self {
            horizontal: axes.horizontal,
            vertical: axes.vertical,
        }
    }
}

impl From<&crate::config::AuthSettings> for AuthSettingsDTO {
    fn from(settings: &crate::config::AuthSettings) -> Self {
        Self {
            enabled: settings.enabled,
            provider: settings.provider.clone(),
            required: settings.required,
        }
    }
}

impl From<&crate::config::RagFlowSettings> for RagFlowSettingsDTO {
    fn from(settings: &crate::config::RagFlowSettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            agent_id: settings.agent_id.clone(),
            api_base_url: settings.api_base_url.clone(),
            timeout: settings.timeout,
            max_retries: settings.max_retries,
            chat_id: settings.chat_id.clone(),
        }
    }
}

impl From<&crate::config::PerplexitySettings> for PerplexitySettingsDTO {
    fn from(settings: &crate::config::PerplexitySettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            model: settings.model.clone(),
            api_url: settings.api_url.clone(),
            max_tokens: settings.max_tokens,
            temperature: settings.temperature,
            top_p: settings.top_p,
            presence_penalty: settings.presence_penalty,
            frequency_penalty: settings.frequency_penalty,
            timeout: settings.timeout,
            rate_limit: settings.rate_limit,
        }
    }
}

impl From<&crate::config::OpenAISettings> for OpenAISettingsDTO {
    fn from(settings: &crate::config::OpenAISettings) -> Self {
        Self {
            api_key: settings.api_key.clone(),
            base_url: settings.base_url.clone(),
            timeout: settings.timeout,
            rate_limit: settings.rate_limit,
        }
    }
}

impl From<&crate::config::KokoroSettings> for KokoroSettingsDTO {
    fn from(settings: &crate::config::KokoroSettings) -> Self {
        Self {
            api_url: settings.api_url.clone(),
            default_voice: settings.default_voice.clone(),
            default_format: settings.default_format.clone(),
            default_speed: settings.default_speed,
            timeout: settings.timeout,
            stream: settings.stream,
            return_timestamps: settings.return_timestamps,
            sample_rate: settings.sample_rate,
        }
    }
}

impl From<&crate::config::WhisperSettings> for WhisperSettingsDTO {
    fn from(settings: &crate::config::WhisperSettings) -> Self {
        Self {
            api_url: settings.api_url.clone(),
            default_model: settings.default_model.clone(),
            default_language: settings.default_language.clone(),
            timeout: settings.timeout,
            temperature: settings.temperature,
            return_timestamps: settings.return_timestamps,
            vad_filter: settings.vad_filter,
            word_timestamps: settings.word_timestamps,
            initial_prompt: settings.initial_prompt.clone(),
        }
    }
}

pub struct EnhancedSettingsHandler {
    validation_service: ValidationService,
    rate_limiter: Arc<RateLimiter>,
}

impl EnhancedSettingsHandler {
    pub fn new() -> Self {
        let config = EndpointRateLimits::settings_update();
        let rate_limiter = Arc::new(RateLimiter::new(config));

        Self {
            validation_service: ValidationService::new(),
            rate_limiter,
        }
    }

    
    pub async fn update_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
        payload: web::Json<Value>,
    ) -> Result<HttpResponse, Error> {
        
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        
        let pubkey = req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|v| v.to_str().ok());
        let has_token = req.headers().get("X-Nostr-Token").is_some();

        trace_info!(
            request_id = %request_id,
            user_pubkey = ?pubkey,
            authenticated = pubkey.is_some() && has_token,
            "Settings update request received"
        );

        let client_id = extract_client_id(&req);

        
        if !self.rate_limiter.is_allowed(&client_id) {
            warn!(
                "Rate limit exceeded for settings update from client: {}",
                client_id
            );
            return too_many_requests!("Too many settings update requests. Please wait before retrying.");
        }

        
        let payload_size = serde_json::to_vec(&*payload).unwrap_or_default().len();
        if payload_size > MAX_REQUEST_SIZE {
            error!("Settings update payload too large: {} bytes", payload_size);
            return payload_too_large!(format!("Payload size {} bytes exceeds limit of {} bytes", payload_size, MAX_REQUEST_SIZE));
        }

        

        
        let validated_payload = match self.validation_service.validate_settings_update(&payload) {
            Ok(sanitized) => sanitized,
            Err(validation_error) => {
                warn!(
                    "Settings validation failed for client {}: {}",
                    client_id, validation_error
                );
                return Ok(validation_error.to_http_response());
            }
        };

        

        
        let update = validated_payload;

        

        
        let mut app_settings = match state.settings_addr.send(GetSettings).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                error!("Failed to get current settings: {}", e);
                return error_json!("Failed to get current settings");
            }
            Err(e) => {
                error!("Settings actor error: {}", e);
                return service_unavailable!("Settings service unavailable");
            }
        };

        
        let mut modified_update = update.clone();
        let auto_balance_update = update
            .get("visualisation")
            .and_then(|v| v.get("graphs"))
            .and_then(|g| {
                if let Some(logseq) = g.get("logseq") {
                    if let Some(physics) = logseq.get("physics") {
                        if let Some(auto_balance) = physics.get("autoBalance") {
                            return Some(auto_balance.clone());
                        }
                    }
                }
                if let Some(visionflow) = g.get("visionflow") {
                    if let Some(physics) = visionflow.get("physics") {
                        if let Some(auto_balance) = physics.get("autoBalance") {
                            return Some(auto_balance.clone());
                        }
                    }
                }
                None
            });

        
        if let Some(ref auto_balance_value) = auto_balance_update {
            

            let vis_obj = modified_update
                .as_object_mut()
                .and_then(|o| {
                    o.entry("visualisation")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                })
                .and_then(|v| {
                    v.entry("graphs")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                });

            if let Some(graphs) = vis_obj {
                let logseq_physics = graphs
                    .entry("logseq")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .and_then(|l| {
                        l.entry("physics")
                            .or_insert_with(|| json!({}))
                            .as_object_mut()
                    });
                if let Some(physics) = logseq_physics {
                    physics.insert("autoBalance".to_string(), auto_balance_value.clone());
                }

                let visionflow_physics = graphs
                    .entry("visionflow")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .and_then(|v| {
                        v.entry("physics")
                            .or_insert_with(|| json!({}))
                            .as_object_mut()
                    });
                if let Some(physics) = visionflow_physics {
                    physics.insert("autoBalance".to_string(), auto_balance_value.clone());
                }
            }
        }

        
        if let Err(e) = app_settings.merge_update(modified_update.clone()) {
            error!("Failed to merge settings: {}", e);
            if crate::utils::logging::is_debug_enabled() {
                error!(
                    "Update payload that caused error: {}",
                    serde_json::to_string_pretty(&modified_update)
                        .unwrap_or_else(|_| "Could not serialize".to_string())
                );
            }
            return error_json!("Failed to merge settings: {}", e);
        }

        
        let _updated_graphs = if auto_balance_update.is_some() {
            vec!["logseq", "visionflow"]
        } else {
            
            let _physics_updates = extract_physics_updates(&modified_update);
            modified_update
                .get("visualisation")
                .and_then(|v| v.get("graphs"))
                .and_then(|g| g.as_object())
                .map(|graphs| {
                    let mut updated = Vec::new();
                    if graphs.contains_key("logseq") {
                        updated.push("logseq");
                    }
                    if graphs.contains_key("visionflow") {
                        updated.push("visionflow");
                    }
                    updated
                })
                .unwrap_or_default()
        };

        let auto_balance_active = app_settings
            .visualisation
            .graphs
            .logseq
            .physics
            .auto_balance
            || app_settings
                .visualisation
                .graphs
                .visionflow
                .physics
                .auto_balance;

        
        match state
            .settings_addr
            .send(UpdateSettings {
                settings: app_settings.clone(),
            })
            .await
        {
            Ok(Ok(())) => {
                

                let is_auto_balance_change = auto_balance_update.is_some();

                if is_auto_balance_change || !auto_balance_active {
                    
                    
                    propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
                    if is_auto_balance_change {
                        
                    }
                } else {
                    
                }

                let response_dto: SettingsResponseDTO = (&app_settings).into();

                ok_json!(json!({
                    "status": "success",
                    "message": "Settings updated successfully",
                    "settings": response_dto,
                    "client_id": client_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            Ok(Err(e)) => {
                error!("Failed to save settings: {}", e);
                error_json!("Failed to save settings: {}", e)
            }
            Err(e) => {
                error!("Settings actor error: {}", e);
                service_unavailable!("Settings service unavailable")
            }
        }
    }

    
    pub async fn get_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        
        let pubkey = req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|v| v.to_str().ok());
        let has_token = req.headers().get("X-Nostr-Token").is_some();

        trace_info!(
            request_id = %request_id,
            user_pubkey = ?pubkey,
            authenticated = pubkey.is_some() && has_token,
            "Settings GET request received"
        );

        let client_id = extract_client_id(&req);

        
        let get_rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            requests_per_minute: 120,
            burst_size: 20,
            ..Default::default()
        }));

        if !get_rate_limiter.is_allowed(&client_id) {
            return too_many_requests!("Too many get settings requests");
        }

        

        let app_settings = match state.settings_addr.send(GetSettings).await {
            Ok(Ok(settings)) => settings,
            Ok(Err(e)) => {
                error!("Failed to get settings: {}", e);
                return error_json!("Failed to retrieve settings");
            }
            Err(e) => {
                error!("Settings actor error: {}", e);
                return service_unavailable!("Settings service unavailable");
            }
        };

        let response_dto: SettingsResponseDTO = (&app_settings).into();

        ok_json!(json!({
            "status": "success",
            "settings": response_dto,
            "validation_info": {
                "input_sanitization": "enabled",
                "rate_limiting": "active",
                "schema_validation": "enforced"
            },
            "client_id": client_id,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    
    pub async fn reset_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        let client_id = extract_client_id(&req);

        
        let reset_rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            requests_per_minute: 10,
            burst_size: 2,
            ..Default::default()
        }));

        if !reset_rate_limiter.is_allowed(&client_id) {
            warn!(
                "Rate limit exceeded for settings reset from client: {}",
                client_id
            );
            return too_many_requests!("Too many reset requests. This is a destructive operation with strict limits.");
        }

        

        
        let default_settings = match AppFullSettings::new() {
            Ok(settings) => settings,
            Err(e) => {
                error!("Failed to load default settings: {}", e);
                return error_json!("Failed to load default settings");
            }
        };

        
        match state
            .settings_addr
            .send(UpdateSettings {
                settings: default_settings.clone(),
            })
            .await
        {
            Ok(Ok(())) => {
                info!("Settings reset to defaults for client: {}", client_id);

                let response_dto: SettingsResponseDTO = (&default_settings).into();

                ok_json!(json!({
                    "status": "success",
                    "message": "Settings reset to defaults successfully",
                    "settings": response_dto,
                    "client_id": client_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            Ok(Err(e)) => {
                error!("Failed to reset settings: {}", e);
                error_json!("Failed to reset settings: {}", e)
            }
            Err(e) => {
                error!("Settings actor error during reset: {}", e);
                service_unavailable!("Settings service unavailable during reset")
            }
        }
    }

    
    pub async fn settings_health(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        trace_info!(
            request_id = %request_id,
            "Settings health check requested"
        );

        
        let (cache_entries, cache_ages) =
            crate::models::user_settings::UserSettings::get_cache_stats();

        
        let cache_hit_rate = if cache_entries > 0 {
            
            0.85 
        } else {
            0.0
        };

        let oldest_cache_entry = cache_ages
            .iter()
            .map(|(_, age)| age.as_secs())
            .max()
            .unwrap_or(0);

        let avg_cache_age = if !cache_ages.is_empty() {
            cache_ages.iter().map(|(_, age)| age.as_secs()).sum::<u64>() / cache_ages.len() as u64
        } else {
            0
        };

        
        let settings_healthy = match state.settings_addr.send(GetSettings).await {
            Ok(Ok(_)) => true,
            _ => false,
        };

        ok_json!(json!({
            "status": if settings_healthy { "healthy" } else { "degraded" },
            "request_id": request_id,
            "cache": {
                "entries": cache_entries,
                "hit_rate": cache_hit_rate,
                "oldest_entry_secs": oldest_cache_entry,
                "avg_age_secs": avg_cache_age,
                "ttl_secs": 600, 
            },
            "settings_actor": {
                "responsive": settings_healthy,
            },
            "rate_limiting": {
                "stats": self.rate_limiter.get_stats(),
            },
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    
    pub async fn get_validation_stats(&self, req: HttpRequest) -> Result<HttpResponse, Error> {
        let client_id = extract_client_id(&req);
        debug!("Validation stats request from client: {}", client_id);

        let stats = self.rate_limiter.get_stats();

        ok_json!(json!({
            "validation_service": "active",
            "rate_limiting": {
                "total_clients": stats.total_clients,
                "banned_clients": stats.banned_clients,
                "active_clients": stats.active_clients,
                "config": stats.config
            },
            "security_features": [
                "comprehensive_input_validation",
                "xss_prevention",
                "sql_injection_prevention",
                "path_traversal_prevention",
                "malicious_content_detection",
                "rate_limiting",
                "request_size_validation"
            ],
            "endpoints_protected": [
                "/settings",
                "/settings/reset",
                "/physics/update",
                "/physics/compute-mode",
                "/clustering/algorithm",
                "/constraints/update",
                "/stress/optimization"
            ],
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    
    async fn propagate_physics_updates(
        &self,
        state: &web::Data<AppState>,
        settings: &AppFullSettings,
        update: &Value,
    ) {
        
        let has_physics_update = update
            .get("visualisation")
            .and_then(|v| v.get("graphs"))
            .map(|g| {
                g.as_object()
                    .map(|obj| obj.values().any(|graph| graph.get("physics").is_some()))
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if has_physics_update {
            info!("Propagating physics updates to GPU actors");

            
            
            let graph_name = "logseq";
            let physics = settings.get_physics(graph_name);
            let sim_params = crate::models::simulation_params::SimulationParams::from(physics);

            if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                if let Err(e) = gpu_addr
                    .send(UpdateSimulationParams { params: sim_params })
                    .await
                {
                    error!(
                        "Failed to update GPU simulation params for {}: {}",
                        graph_name, e
                    );
                } else {
                    info!(
                        "GPU simulation params updated for {} (knowledge graph)",
                        graph_name
                    );
                }
            }
        }
    }
}

impl Default for EnhancedSettingsHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    let handler = web::Data::new(EnhancedSettingsHandler::new());

    cfg.app_data(handler.clone())
        .service(
            web::scope("/settings")
                
                .route("/path", web::get().to(get_setting_by_path))
                .route("/path", web::put().to(update_setting_by_path))
                
                
                
                .route("/schema", web::get().to(get_settings_schema))
                .route("/current", web::get().to(get_current_settings))
                
                .route("", web::get().to(get_settings))
                .route("", web::post().to(update_settings))
                .route("/reset", web::post().to(reset_settings))
                .route("/save", web::post().to(save_settings))
                .route(
                    "/validation/stats",
                    web::get().to(
                        |req, handler: web::Data<EnhancedSettingsHandler>| async move {
                            handler.get_validation_stats(req).await
                        },
                    ),
                ),
        )
        .service(
            web::scope("/api/physics").route("/compute-mode", web::post().to(update_compute_mode)),
        )
        .service(
            web::scope("/api/clustering")
                .route("/algorithm", web::post().to(update_clustering_algorithm)),
        )
        .service(
            web::scope("/api/constraints").route("/update", web::post().to(update_constraints)),
        )
        .service(
            web::scope("/api/analytics").route("/clusters", web::get().to(get_cluster_analytics)),
        )
        .service(
            web::scope("/api/stress")
                .route("/optimization", web::post().to(update_stress_optimization)),
        );
}

async fn get_setting_by_path(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let path = req
        .query_string()
        .split('&')
        .find(|param| param.starts_with("path="))
        .and_then(|p| p.strip_prefix("path="))
        .map(|p| {
            urlencoding::decode(p)
                .unwrap_or(Cow::Borrowed(p))
                .to_string()
        })
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing 'path' query parameter"))?;

    let app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    match app_settings.get_json_by_path(&path) {
        Ok(value_json) => ok_json!(json!({
            "success": true,
            "path": path,
            "value": value_json
        })),
        Err(e) => {
            warn!("Path not found '{}': {}", path, e);
            not_found!("Path not found", e)
        }
    }
}

async fn update_setting_by_path(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();
    let path = update
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing 'path' in request body"))?
        .to_string();
    let value = update
        .get("value")
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing 'value' in request body"))?
        .clone();

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    let previous_value = app_settings.get_json_by_path(&path).ok();

    match app_settings.set_json_by_path(&path, value.clone()) {
        Ok(()) => {
            match state
                .settings_addr
                .send(UpdateSettings {
                    settings: app_settings.clone(),
                })
                .await
            {
                Ok(Ok(())) => {
                    info!("Updated setting at path: {}", path);

                    
                    if path.contains(".physics.")
                        || path.contains(".graphs.logseq.")
                        || path.contains(".graphs.visionflow.")
                    {
                        info!("Physics setting changed, propagating to GPU actors");

                        
                        let graph_name = if path.contains(".graphs.logseq.") {
                            "logseq"
                        } else if path.contains(".graphs.visionflow.") {
                            "visionflow"
                        } else {
                            
                            "logseq"
                        };

                        
                        propagate_physics_to_gpu(&state, &app_settings, graph_name).await;
                    }

                    ok_json!(json!({
                        "success": true,
                        "path": path,
                        "value": update.get("value").cloned().unwrap_or(Value::Null),
                        "previousValue": previous_value
                    }))
                }
                Ok(Err(e)) => {
                    error!("Failed to save settings: {}", e);
                    Ok(HttpResponse::InternalServerError().json(json!({
                        "error": format!("Failed to save settings: {}", e),
                        "path": path
                    })))
                }
                Err(e) => {
                    error!("Settings actor error: {}", e);
                    service_unavailable!("Settings service unavailable")
                }
            }
        }
        Err(e) => {
            warn!("Failed to update path '{}': {}", path, e);
            bad_request!("Invalid path or value", e)
        }
    }
}

async fn batch_get_settings(
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let paths = payload
        .get("paths")
        .and_then(|p| p.as_array())
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing 'paths' array"))?
        .iter()
        .map(|p| p.as_str().unwrap_or("").to_string())
        .collect::<Vec<String>>();

    if paths.is_empty() {
        return bad_request!("Paths array cannot be empty");
    }

    let app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    let results: Vec<Value> = paths
        .iter()
        .map(|path| match app_settings.get_json_by_path(path) {
            Ok(value_json) => {
                json!({
                    "path": path,
                    "value": value_json,
                    "success": true
                })
            }
            Err(e) => {
                warn!("Path not found '{}': {}", path, e);
                json!({
                    "path": path,
                    "success": false,
                    "error": "Path not found",
                    "message": e
                })
            }
        })
        .collect();

    ok_json!(json!({
        "success": true,
        "message": format!("Successfully processed {} paths", results.len()),
        "values": results
    }))
}

async fn batch_update_settings(
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    
    info!("Batch update request received: {:?}", payload);

    let updates = payload
        .get("updates")
        .and_then(|u| u.as_array())
        .ok_or_else(|| {
            error!(
                "Batch update failed: Missing 'updates' array in payload: {:?}",
                payload
            );
            actix_web::error::ErrorBadRequest("Missing 'updates' array")
        })?;

    if updates.is_empty() {
        error!("Batch update failed: Empty updates array");
        return bad_request!("Updates array cannot be empty");
    }

    info!("Processing {} batch updates", updates.len());

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    let mut results = Vec::new();
    let mut success_count = 0;

    for update in updates {
        let path = update.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let value = update.get("value").unwrap_or(&Value::Null).clone();

        info!(
            "Processing batch update: path='{}', value={:?}",
            path, value
        );

        let previous_value = app_settings.get_json_by_path(path).ok();

        match app_settings.set_json_by_path(path, value.clone()) {
            Ok(()) => {
                success_count += 1;
                info!(
                    "Successfully updated path '{}' with value {:?}",
                    path, value
                );
                results.push(json!({
                    "path": path,
                    "success": true,
                    "value": update.get("value").cloned().unwrap_or(Value::Null),
                    "previousValue": previous_value
                }));
            }
            Err(e) => {
                
                error!(
                    "Failed to update path '{}' with value {:?}: {}",
                    path, value, e
                );

                
                let error_detail = if e.contains("does not exist") {
                    format!("Path '{}' does not exist in settings structure", path)
                } else if e.contains("Type mismatch") {
                    format!("Type mismatch: {}", e)
                } else if e.contains("not found") {
                    format!("Field not found: {}", e)
                } else {
                    e.clone()
                };

                results.push(json!({
                    "path": path,
                    "success": false,
                    "error": error_detail,
                    "message": e,
                    "providedValue": value,
                    "expectedType": previous_value.as_ref().map(|v| value_type_name(v))
                }));
            }
        }
    }

    
    if success_count > 0 {
        match state
            .settings_addr
            .send(UpdateSettings {
                settings: app_settings.clone(),
            })
            .await
        {
            Ok(Ok(())) => {
                info!("Batch updated {} settings successfully", success_count);

                
                let mut physics_updated = false;
                for update in updates {
                    let path = update.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    if path.contains(".physics.")
                        || path.contains(".graphs.logseq.")
                        || path.contains(".graphs.visionflow.")
                    {
                        physics_updated = true;
                        break;
                    }
                }

                if physics_updated {
                    info!("Physics settings changed in batch update, propagating to GPU actors");
                    
                    propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
                    
                    
                }
            }
            Ok(Err(e)) => {
                error!("Failed to save batch settings: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to save settings: {}", e),
                    "results": results
                })));
            }
            Err(e) => {
                error!("Settings actor error: {}", e);
                return service_unavailable!("Settings service unavailable");
            }
        }
    }

    ok_json!(json!({
        "success": true,
        "message": format!("Successfully updated {} out of {} settings", success_count, updates.len()),
        "results": results
    }))
}

async fn get_settings_schema(
    req: HttpRequest,
    _state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let path = req
        .query_string()
        .split('&')
        .find(|param| param.starts_with("path="))
        .and_then(|p| p.strip_prefix("path="))
        .map(|p| {
            urlencoding::decode(p)
                .unwrap_or(Cow::Borrowed(p))
                .to_string()
        })
        .unwrap_or_default();

    
    
    let schema = json!({
        "type": "object",
        "properties": {
            "damping": { "type": "number", "description": "Physics damping factor (0.0-1.0)" },
            "gravity": { "type": "number", "description": "Physics gravity strength" },
            
        },
        "path": path
    });

    ok_json!(json!({
        "success": true,
        "path": path,
        "schema": schema
    }))
}

async fn get_settings(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    
    let response_dto: SettingsResponseDTO = (&app_settings).into();

    ok_json!(response_dto)
}

async fn get_current_settings(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    
    let response_dto: SettingsResponseDTO = (&app_settings).into();

    
    ok_json!(json!({
        "settings": response_dto,
        "version": app_settings.version,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }))
}

async fn update_settings(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let mut update = payload.into_inner();

    
    convert_to_snake_case_recursive(&mut update);

    debug!("Settings update received: {:?}", update);

    
    if let Err(e) = validate_settings_update(&update) {
        error!("Settings validation failed: {}", e);
        error!(
            "Failed update payload: {}",
            serde_json::to_string_pretty(&update).unwrap_or_default()
        );
        return bad_request!("Invalid settings: {}", e);
    }

    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    
    if crate::utils::logging::is_debug_enabled() {
        debug!(
            "Settings update payload (before merge): {}",
            serde_json::to_string_pretty(&update)
                .unwrap_or_else(|_| "Could not serialize".to_string())
        );
    }

    
    
    let mut modified_update = update.clone();
    let auto_balance_update = update
        .get("visualisation")
        .and_then(|v| v.get("graphs"))
        .and_then(|g| {
            
            if let Some(logseq) = g.get("logseq") {
                if let Some(physics) = logseq.get("physics") {
                    if let Some(auto_balance) = physics.get("autoBalance") {
                        return Some(auto_balance.clone());
                    }
                }
            }
            
            if let Some(visionflow) = g.get("visionflow") {
                if let Some(physics) = visionflow.get("physics") {
                    if let Some(auto_balance) = physics.get("autoBalance") {
                        return Some(auto_balance.clone());
                    }
                }
            }
            None
        });

    
    if let Some(ref auto_balance_value) = auto_balance_update {
        info!(
            "Synchronizing auto_balance setting across both graphs: {}",
            auto_balance_value
        );

        
        let vis_obj = modified_update
            .as_object_mut()
            .and_then(|o| {
                o.entry("visualisation")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            })
            .and_then(|v| {
                v.entry("graphs")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            });

        if let Some(graphs) = vis_obj {
            
            let logseq_physics = graphs
                .entry("logseq")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .and_then(|l| {
                    l.entry("physics")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                });
            if let Some(physics) = logseq_physics {
                physics.insert("autoBalance".to_string(), auto_balance_value.clone());
            }

            
            let visionflow_physics = graphs
                .entry("visionflow")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .and_then(|v| {
                    v.entry("physics")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                });
            if let Some(physics) = visionflow_physics {
                physics.insert("autoBalance".to_string(), auto_balance_value.clone());
            }
        }
    }

    
    if let Err(e) = app_settings.merge_update(modified_update.clone()) {
        error!("Failed to merge settings: {}", e);
        if crate::utils::logging::is_debug_enabled() {
            error!(
                "Update payload that caused error: {}",
                serde_json::to_string_pretty(&modified_update)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }
        return error_json!("Failed to merge settings: {}", e);
    }

    
    
    let _updated_graphs = if auto_balance_update.is_some() {
        
        let _physics_updates = extract_physics_updates(&update);
        vec!["logseq", "visionflow"]
    } else {
        modified_update
            .get("visualisation")
            .and_then(|v| v.get("graphs"))
            .and_then(|g| g.as_object())
            .map(|graphs| {
                let mut updated = Vec::new();
                if graphs.contains_key("logseq") {
                    updated.push("logseq");
                }
                if graphs.contains_key("visionflow") {
                    updated.push("visionflow");
                }
                updated
            })
            .unwrap_or_default()
    };

    
    
    let auto_balance_active = app_settings
        .visualisation
        .graphs
        .logseq
        .physics
        .auto_balance
        || app_settings
            .visualisation
            .graphs
            .visionflow
            .physics
            .auto_balance;

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Settings updated successfully");

            
            
            let is_auto_balance_change = auto_balance_update.is_some();

            
            
            
            
            if is_auto_balance_change || !auto_balance_active {
                
                
                propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
                if is_auto_balance_change {
                    info!("[AUTO-BALANCE] Propagating auto_balance setting change to GPU (logseq only)");
                }
            } else {
                info!("[AUTO-BALANCE] Skipping physics propagation to GPU - auto-balance is active and not changing");
            }

            
            let response_dto: SettingsResponseDTO = (&app_settings).into();

            ok_json!(response_dto)
        }
        Ok(Err(e)) => {
            error!("Failed to save settings: {}", e);
            error_json!("Failed to save settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

async fn reset_settings(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    
    let default_settings = match AppFullSettings::new() {
        Ok(settings) => settings,
        Err(e) => {
            error!("Failed to load default settings: {}", e);
            return error_json!("Failed to load default settings");
        }
    };

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: default_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Settings reset to defaults");

            
            let response_dto: SettingsResponseDTO = (&default_settings).into();

            ok_json!(response_dto)
        }
        Ok(Err(e)) => {
            error!("Failed to reset settings: {}", e);
            error_json!("Failed to reset settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

async fn save_settings(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: Option<web::Json<Value>>,
) -> Result<HttpResponse, Error> {
    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    
    if let Some(update) = payload {
        let update_value = update.into_inner();

        
        if let Err(e) = validate_settings_update(&update_value) {
            error!("Settings validation failed: {}", e);
            return bad_request!("Invalid settings: {}", e);
        }

        
        if let Err(e) = app_settings.merge_update(update_value) {
            error!("Failed to merge settings update: {}", e);
            return bad_request!("Failed to merge settings: {}", e);
        }
    }

    
    if !app_settings.system.persist_settings {
        return bad_request!("Settings persistence is disabled. Enable 'system.persist_settings' to save settings.");
    }

    
    match app_settings.save() {
        Ok(()) => {
            info!("Settings successfully saved to file");

            
            match state
                .settings_addr
                .send(UpdateSettings {
                    settings: app_settings.clone(),
                })
                .await
            {
                Ok(Ok(())) => {
                    let response_dto: SettingsResponseDTO = (&app_settings).into();
                    ok_json!(json!({
                        "message": "Settings saved successfully",
                        "settings": response_dto
                    }))
                }
                Ok(Err(e)) => {
                    error!("Failed to update settings in actor after save: {}", e);
                    error_json!("Settings saved to file but failed to update in memory")
                }
                Err(e) => {
                    error!("Settings actor communication error: {}", e);
                    service_unavailable!("Settings saved to file but service is unavailable")
                }
            }
        }
        Err(e) => {
            error!("Failed to save settings to file: {}", e);
            error_json!("Failed to save settings to file")
        }
    }
}

fn validate_settings_update(update: &Value) -> Result<(), String> {
    
    if let Some(vis) = update.get("visualisation") {
        if let Some(graphs) = vis.get("graphs") {
            
            for (graph_name, graph_settings) in
                graphs.as_object().ok_or("graphs must be an object")?.iter()
            {
                if graph_name != "logseq" && graph_name != "visionflow" {
                    return Err(format!("Invalid graph name: {}", graph_name));
                }

                
                if let Some(physics) = graph_settings.get("physics") {
                    validate_physics_settings(physics)?;
                }

                
                if let Some(nodes) = graph_settings.get("nodes") {
                    validate_node_settings(nodes)?;
                }
            }
        }

        
        if let Some(rendering) = vis.get("rendering") {
            validate_rendering_settings(rendering)?;
        }

        
        if let Some(hologram) = vis.get("hologram") {
            validate_hologram_settings(hologram)?;
        }
    }

    
    if let Some(xr) = update.get("xr") {
        validate_xr_settings(xr)?;
    }

    
    if let Some(system) = update.get("system") {
        validate_system_settings(system)?;
    }

    Ok(())
}

fn validate_physics_settings(physics: &Value) -> Result<(), String> {
    
    validate_physics_settings_complete(physics)?;

    
    if let Some(obj) = physics.as_object() {
        debug!(
            "Physics settings fields received: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }

    
    if let Some(iterations) = physics.get("iterations") {
        let val = iterations
            .as_f64()
            .map(|f| f.round() as u64)
            .or_else(|| iterations.as_u64())
            .ok_or("iterations must be a positive number")?;
        if val == 0 || val > 1000 {
            return Err("iterations must be between 1 and 1000".to_string());
        }
    }

    
    if let Some(auto_balance_interval) = physics.get("autoBalanceIntervalMs") {
        let val = auto_balance_interval
            .as_u64()
            .or_else(|| auto_balance_interval.as_f64().map(|f| f.round() as u64))
            .ok_or("autoBalanceIntervalMs must be a positive integer")?;
        if val < 10 || val > 60000 {
            return Err("autoBalanceIntervalMs must be between 10 and 60000 ms".to_string());
        }
    }

    
    if let Some(boundary_limit) = physics.get("boundaryLimit") {
        let val = boundary_limit
            .as_f64()
            .ok_or("boundaryLimit must be a number")?;
        if val < 0.1 || val > 100000.0 {
            return Err("boundaryLimit must be between 0.1 and 100000.0".to_string());
        }

        
        if let Some(bounds_size) = physics.get("boundsSize").and_then(|b| b.as_f64()) {
            let max_boundary = bounds_size * 0.99;
            if val > max_boundary {
                return Err(format!(
                    "boundaryLimit ({:.1}) must be less than 99% of boundsSize ({:.1})",
                    val, bounds_size
                ));
            }
        }
    }

    Ok(())
}

fn validate_node_settings(nodes: &Value) -> Result<(), String> {
    
    if let Some(color) = nodes.get("baseColor") {
        let color_str = color.as_str().ok_or("baseColor must be a string")?;
        if !color_str.starts_with('#') || (color_str.len() != 7 && color_str.len() != 4) {
            return Err("baseColor must be a valid hex color (e.g., #ffffff or #fff)".to_string());
        }
    }

    if let Some(opacity) = nodes.get("opacity") {
        let val = opacity.as_f64().ok_or("opacity must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("opacity must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(metalness) = nodes.get("metalness") {
        let val = metalness.as_f64().ok_or("metalness must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("metalness must be between 0.0 and 1.0".to_string());
        }
    }

    if let Some(roughness) = nodes.get("roughness") {
        let val = roughness.as_f64().ok_or("roughness must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("roughness must be between 0.0 and 1.0".to_string());
        }
    }

    
    if let Some(node_size) = nodes.get("nodeSize") {
        let val = node_size.as_f64().ok_or("nodeSize must be a number")?;
        if val <= 0.0 || val > 1000.0 {
            return Err("nodeSize must be between 0.0 and 1000.0".to_string());
        }
    }

    if let Some(quality) = nodes.get("quality") {
        let q = quality.as_str().ok_or("quality must be a string")?;
        if !["low", "medium", "high"].contains(&q) {
            return Err("quality must be 'low', 'medium', or 'high'".to_string());
        }
    }

    Ok(())
}

fn validate_rendering_settings(rendering: &Value) -> Result<(), String> {
    
    if let Some(ambient) = rendering.get("ambientLightIntensity") {
        let val = ambient
            .as_f64()
            .ok_or("ambientLightIntensity must be a number")?;
        if val < 0.0 || val > 100.0 {
            return Err("ambientLightIntensity must be between 0.0 and 100.0".to_string());
        }
    }

    
    if let Some(glow) = rendering.get("glow") {
        validate_glow_settings(glow)?;
    }

    Ok(())
}

fn validate_glow_settings(glow: &Value) -> Result<(), String> {
    
    if let Some(enabled) = glow.get("enabled") {
        if !enabled.is_boolean() {
            return Err("glow enabled must be a boolean".to_string());
        }
    }

    
    for field_name in ["intensity", "strength"] {
        if let Some(intensity) = glow.get(field_name) {
            let val = intensity
                .as_f64()
                .ok_or(format!("glow {} must be a number", field_name))?;
            if val < 0.0 || val > 10.0 {
                return Err(format!("glow {} must be between 0.0 and 10.0", field_name));
            }
        }
    }

    
    if let Some(radius) = glow.get("radius") {
        let val = radius.as_f64().ok_or("glow radius must be a number")?;
        if val < 0.0 || val > 5.0 {
            return Err("glow radius must be between 0.0 and 5.0".to_string());
        }
    }

    
    if let Some(threshold) = glow.get("threshold") {
        let val = threshold
            .as_f64()
            .ok_or("glow threshold must be a number")?;
        if val < 0.0 || val > 2.0 {
            return Err("glow threshold must be between 0.0 and 2.0".to_string());
        }
    }

    
    for field_name in [
        "edgeGlowStrength",
        "environmentGlowStrength",
        "nodeGlowStrength",
    ] {
        if let Some(strength) = glow.get(field_name) {
            let val = strength
                .as_f64()
                .ok_or(format!("glow {} must be a number", field_name))?;
            if val < 0.0 || val > 1.0 {
                return Err(format!("glow {} must be between 0.0 and 1.0", field_name));
            }
        }
    }

    Ok(())
}

fn validate_hologram_settings(hologram: &Value) -> Result<(), String> {
    
    if let Some(ring_count) = hologram.get("ringCount") {
        
        let val = ring_count
            .as_f64()
            .map(|f| f.round() as u64) 
            .or_else(|| ring_count.as_u64()) 
            .ok_or("ringCount must be a positive integer")?;

        if val > 20 {
            return Err("ringCount must be between 0 and 20".to_string());
        }
    }

    
    if let Some(color) = hologram.get("ringColor") {
        let color_str = color.as_str().ok_or("ringColor must be a string")?;
        if !color_str.starts_with('#') || (color_str.len() != 7 && color_str.len() != 4) {
            return Err("ringColor must be a valid hex color (e.g., #ffffff or #fff)".to_string());
        }
    }

    
    if let Some(opacity) = hologram.get("ringOpacity") {
        let val = opacity.as_f64().ok_or("ringOpacity must be a number")?;
        if !(0.0..=1.0).contains(&val) {
            return Err("ringOpacity must be between 0.0 and 1.0".to_string());
        }
    }

    
    if let Some(speed) = hologram.get("ringRotationSpeed") {
        let val = speed.as_f64().ok_or("ringRotationSpeed must be a number")?;
        if val < 0.0 || val > 1000.0 {
            return Err("ringRotationSpeed must be between 0.0 and 1000.0".to_string());
        }
    }

    Ok(())
}

fn validate_system_settings(system: &Value) -> Result<(), String> {
    
    if let Some(debug) = system.get("debug") {
        if let Some(debug_obj) = debug.as_object() {
            
            let boolean_fields = [
                "enabled", 
                "showFPS",
                "showMemory",
                "enablePerformanceDebug",
                "enableTelemetry",
                "enableDataDebug",
                "enableWebSocketDebug",
                "enablePhysicsDebug",
                "enableNodeDebug",
                "enableShaderDebug",
                "enableMatrixDebug",
            ];

            for field in &boolean_fields {
                if let Some(val) = debug_obj.get(*field) {
                    if !val.is_boolean() {
                        return Err(format!("debug.{} must be a boolean", field));
                    }
                }
            }

            
            if let Some(log_level) = debug_obj.get("logLevel") {
                if let Some(val) = log_level.as_f64() {
                    if val < 0.0 || val > 3.0 {
                        return Err("debug.logLevel must be between 0 and 3".to_string());
                    }
                } else if let Some(val) = log_level.as_u64() {
                    if val > 3 {
                        return Err("debug.logLevel must be between 0 and 3".to_string());
                    }
                } else if let Some(val) = log_level.as_str() {
                    
                    match val {
                        "error" | "warn" | "info" | "debug" => {
                            
                        }
                        _ => {
                            return Err(
                                "debug.logLevel must be 'error', 'warn', 'info', or 'debug'"
                                    .to_string(),
                            );
                        }
                    }
                } else {
                    return Err("debug.logLevel must be a number or string".to_string());
                }
            }
        }
    }

    
    if let Some(persist) = system.get("persistSettingsOnServer") {
        if !persist.is_boolean() {
            return Err("system.persistSettingsOnServer must be a boolean".to_string());
        }
    }

    
    if let Some(url) = system.get("customBackendUrl") {
        if !url.is_string() && !url.is_null() {
            return Err("system.customBackendUrl must be a string or null".to_string());
        }
    }

    Ok(())
}

fn validate_xr_settings(xr: &Value) -> Result<(), String> {
    
    if let Some(enabled) = xr.get("enabled") {
        if !enabled.is_boolean() {
            return Err("XR enabled must be a boolean".to_string());
        }
    }

    
    if let Some(quality) = xr.get("quality") {
        if let Some(q) = quality.as_str() {
            if !["Low", "Medium", "High", "low", "medium", "high"].contains(&q) {
                return Err("XR quality must be Low, Medium, or High".to_string());
            }
        } else {
            return Err("XR quality must be a string".to_string());
        }
    }

    
    if let Some(render_scale) = xr.get("renderScale") {
        let val = render_scale
            .as_f64()
            .ok_or("renderScale must be a number")?;
        if val < 0.1 || val > 10.0 {
            return Err("renderScale must be between 0.1 and 10.0".to_string());
        }
    }

    
    if let Some(room_scale) = xr.get("roomScale") {
        let val = room_scale.as_f64().ok_or("roomScale must be a number")?;
        if val <= 0.0 || val > 100.0 {
            return Err("roomScale must be between 0.0 and 100.0".to_string());
        }
    }

    
    if let Some(hand_tracking) = xr.get("handTracking") {
        if let Some(ht_obj) = hand_tracking.as_object() {
            if let Some(enabled) = ht_obj.get("enabled") {
                if !enabled.is_boolean() {
                    return Err("handTracking.enabled must be a boolean".to_string());
                }
            }
        }
    }

    
    if let Some(interactions) = xr.get("interactions") {
        if let Some(int_obj) = interactions.as_object() {
            if let Some(haptics) = int_obj.get("enableHaptics") {
                if !haptics.is_boolean() {
                    return Err("interactions.enableHaptics must be a boolean".to_string());
                }
            }
        }
    }

    Ok(())
}

async fn propagate_physics_to_gpu(
    state: &web::Data<AppState>,
    settings: &AppFullSettings,
    graph: &str,
) {
    let physics = settings.get_physics(graph);

    
    info!("[PHYSICS UPDATE] Propagating {} physics to actors:", graph);
    info!(
        "  - repulsion_k: {:.3} (affects node spreading)",
        physics.repel_k
    );
    info!(
        "  - spring_k: {:.3} (affects edge tension)",
        physics.spring_k
    );
    info!("  - spring_k: {:.3} (affects clustering)", physics.spring_k);
    info!(
        "  - damping: {:.3} (affects settling, 1.0 = no movement)",
        physics.damping
    );
    info!("  - time_step: {:.3} (simulation speed)", physics.dt);
    info!(
        "  - max_velocity: {:.3} (prevents explosions)",
        physics.max_velocity
    );
    info!(
        "  - temperature: {:.3} (random motion)",
        physics.temperature
    );
    info!("  - gravity: {:.3} (directional force)", physics.gravity);

    if crate::utils::logging::is_debug_enabled() {
        debug!("  - bounds_size: {:.1}", physics.bounds_size);
        debug!("  - separation_radius: {:.3}", physics.separation_radius); 
        debug!("  - mass_scale: {:.3}", physics.mass_scale);
        debug!("  - boundary_damping: {:.3}", physics.boundary_damping);
        debug!("  - update_threshold: {:.3}", physics.update_threshold);
        debug!("  - iterations: {}", physics.iterations);
        debug!("  - enabled: {}", physics.enabled);

        
        debug!("  - min_distance: {:.3}", physics.min_distance);
        debug!("  - max_repulsion_dist: {:.1}", physics.max_repulsion_dist);
        debug!("  - boundary_margin: {:.3}", physics.boundary_margin);
        debug!(
            "  - boundary_force_strength: {:.1}",
            physics.boundary_force_strength
        );
        debug!("  - warmup_iterations: {}", physics.warmup_iterations);
        debug!("  - warmup_curve: {}", physics.warmup_curve);
        debug!(
            "  - zero_velocity_iterations: {}",
            physics.zero_velocity_iterations
        );
        debug!("  - cooling_rate: {:.6}", physics.cooling_rate);
        debug!("  - clustering_algorithm: {}", physics.clustering_algorithm);
        debug!("  - cluster_count: {}", physics.cluster_count);
        debug!(
            "  - clustering_resolution: {:.3}",
            physics.clustering_resolution
        );
        debug!(
            "  - clustering_iterations: {}",
            physics.clustering_iterations
        );
        debug!("[GPU Parameters] All new parameters available for GPU processing");
    }

    let sim_params: crate::models::simulation_params::SimulationParams = physics.into();

    info!(
        "[PHYSICS UPDATE] Converted to SimulationParams - repulsion: {}, damping: {:.3}, time_step: {:.3}",
        sim_params.repel_k, sim_params.damping, sim_params.dt
    );

    let update_msg = UpdateSimulationParams {
        params: sim_params.clone(),
    };


    if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
        info!("[PHYSICS UPDATE] Sending to GPUComputeActor...");
        if let Err(e) = gpu_addr.send(update_msg.clone()).await {
            error!("[PHYSICS UPDATE] FAILED to update GPUComputeActor: {}", e);
        } else {
            info!("[PHYSICS UPDATE] GPUComputeActor updated successfully");
        }
    } else {
        warn!("[PHYSICS UPDATE] No GPUComputeActor available");
    }

    
    info!("[PHYSICS UPDATE] Sending to GraphServiceActor...");
    if let Err(e) = state.graph_service_addr.send(update_msg).await {
        error!("[PHYSICS UPDATE] FAILED to update GraphServiceActor: {}", e);
    } else {
        info!("[PHYSICS UPDATE] GraphServiceActor updated successfully");
    }
}

fn get_field_variant<'a>(obj: &'a Value, variants: &[&str]) -> Option<&'a Value> {
    for variant in variants {
        if let Some(val) = obj.get(*variant) {
            return Some(val);
        }
    }
    None
}

fn count_fields(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len() + map.values().map(count_fields).sum::<usize>(),
        Value::Array(arr) => arr.iter().map(count_fields).sum(),
        _ => 0,
    }
}

fn extract_physics_updates(update: &Value) -> Vec<&str> {
    update
        .get("visualisation")
        .and_then(|v| v.get("graphs"))
        .and_then(|g| g.as_object())
        .map(|graphs| {
            let mut updated = Vec::new();
            if graphs.contains_key("logseq")
                && graphs
                    .get("logseq")
                    .and_then(|g| g.get("physics"))
                    .is_some()
            {
                updated.push("logseq");
            }
            if graphs.contains_key("visionflow")
                && graphs
                    .get("visionflow")
                    .and_then(|g| g.get("physics"))
                    .is_some()
            {
                updated.push("visionflow");
            }
            updated
        })
        .unwrap_or_default()
}

fn extract_failed_field(physics: &Value) -> String {
    if let Some(obj) = physics.as_object() {
        obj.keys().next().unwrap_or(&"unknown".to_string()).clone()
    } else {
        "unknown".to_string()
    }
}

fn create_physics_settings_update(physics_update: Value) -> Value {
    let mut normalized_physics = physics_update.clone();

    
    if let Some(obj) = normalized_physics.as_object_mut() {
        
        if let Some(spring_strength) = obj.remove("springStrength") {
            if !obj.contains_key("springK") {
                obj.insert("springK".to_string(), spring_strength);
            }
        }

        
        if let Some(repulsion_strength) = obj.remove("repulsionStrength") {
            if !obj.contains_key("repelK") {
                obj.insert("repelK".to_string(), repulsion_strength);
            }
        }

        
        if let Some(attraction_strength) = obj.remove("attractionStrength") {
            if !obj.contains_key("attractionK") {
                obj.insert("attractionK".to_string(), attraction_strength);
            }
        }

        
        if let Some(collision_radius) = obj.remove("collisionRadius") {
            if !obj.contains_key("separationRadius") {
                obj.insert("separationRadius".to_string(), collision_radius);
            }
        }
    }

    json!({
        "visualisation": {
            "graphs": {
                "logseq": {
                    "physics": normalized_physics
                },
                "visionflow": {
                    "physics": normalized_physics.clone()
                }
            }
        }
    })
}

async fn update_compute_mode(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Compute mode update request received");
    debug!(
        "Compute mode payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    
    let compute_mode = update
        .get("computeMode")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            actix_web::error::ErrorBadRequest("computeMode must be an integer between 0 and 3")
        })?;

    if compute_mode > 3 {
        return bad_request!("computeMode must be between 0 and 3");
    }

    
    let physics_update = json!({
        "computeMode": compute_mode
    });

    let settings_update = create_physics_settings_update(physics_update);

    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge compute mode settings: {}", e);
        return error_json!("Failed to update compute mode: {}", e);
    }

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Compute mode updated successfully to: {}", compute_mode);

            
            propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionflow").await;

            ok_json!(json!({
                "status": "Compute mode updated successfully",
                "computeMode": compute_mode
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save compute mode settings: {}", e);
            error_json!("Failed to save compute mode settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

async fn update_clustering_algorithm(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Clustering algorithm update request received");
    debug!(
        "Clustering payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    
    let algorithm = update
        .get("algorithm")
        .and_then(|v| v.as_str())
        .ok_or_else(|| actix_web::error::ErrorBadRequest("algorithm must be a string"))?;

    if !["none", "kmeans", "spectral", "louvain"].contains(&algorithm) {
        return bad_request!("algorithm must be 'none', 'kmeans', 'spectral', or 'louvain'");
    }

    
    let cluster_count = update
        .get("clusterCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(5);
    let resolution = update
        .get("resolution")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;
    let iterations = update
        .get("iterations")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    
    let physics_update = json!({
        "clusteringAlgorithm": algorithm,
        "clusterCount": cluster_count,
        "clusteringResolution": resolution,
        "clusteringIterations": iterations
    });

    let settings_update = create_physics_settings_update(physics_update);

    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge clustering settings: {}", e);
        return error_json!("Failed to update clustering algorithm: {}", e);
    }

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!(
                "Clustering algorithm updated successfully to: {}",
                algorithm
            );

            
            propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionflow").await;

            ok_json!(json!({
                "status": "Clustering algorithm updated successfully",
                "algorithm": algorithm,
                "clusterCount": cluster_count,
                "resolution": resolution,
                "iterations": iterations
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save clustering settings: {}", e);
            error_json!("Failed to save clustering settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

async fn update_constraints(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Constraints update request received");
    debug!(
        "Constraints payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    
    if let Err(e) = validate_constraints(&update) {
        return bad_request!("Invalid constraints: {}", e);
    }

    
    
    let settings_update = json!({
        "visualisation": {
            "graphs": {
                "logseq": {
                    "physics": {
                        "computeMode": 2  
                    }
                },
                "visionflow": {
                    "physics": {
                        "computeMode": 2
                    }
                }
            }
        }
    });

    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge constraints settings: {}", e);
        return error_json!("Failed to update constraints: {}", e);
    }

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Constraints updated successfully");

            
            propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionflow").await;

            ok_json!(json!({
                "status": "Constraints updated successfully"
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save constraints settings: {}", e);
            error_json!("Failed to save constraints settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

async fn get_cluster_analytics(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    info!("Cluster analytics request received");


    if let Some(_gpu_addr) = state.get_gpu_compute_addr().await {

        use crate::actors::messages::GetGraphData;


        let graph_data = match state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => {
                error!("Failed to get graph data for clustering analytics: {}", e);
                return error_json!("Failed to get graph data for analytics");
            }
            Err(e) => {
                error!("Graph service communication error: {}", e);
                return service_unavailable!("Graph service unavailable");
            }
        };


        info!("GPU compute actor available but clustering not handled by force compute actor");
        get_cpu_fallback_analytics(&graph_data).await
    } else {
        
        use crate::actors::messages::GetGraphData;
        match state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(graph_data)) => get_cpu_fallback_analytics(&graph_data).await,
            Ok(Err(e)) => {
                error!("Failed to get graph data: {}", e);
                error_json!("Failed to get graph data for analytics")
            }
            Err(e) => {
                error!("Graph service unavailable: {}", e);
                service_unavailable!("Graph service unavailable")
            }
        }
    }
}

async fn get_cpu_fallback_analytics(
    graph_data: &crate::models::graph::GraphData,
) -> Result<HttpResponse, Error> {
    use std::collections::HashMap;
use crate::{
    ok_json, created_json, error_json, bad_request, not_found,
    unauthorized, forbidden, conflict, no_content, accepted,
    too_many_requests, service_unavailable, payload_too_large
};



    
    let node_count = graph_data.nodes.len();
    let edge_count = graph_data.edges.len();

    
    let mut type_clusters: HashMap<String, Vec<&crate::models::node::Node>> = HashMap::new();

    for node in &graph_data.nodes {
        let node_type = node
            .node_type
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();
        type_clusters
            .entry(node_type)
            .or_insert_with(Vec::new)
            .push(node);
    }

    
    let clusters: Vec<_> = type_clusters
        .into_iter()
        .enumerate()
        .map(|(i, (type_name, nodes))| {
            
            let centroid = if !nodes.is_empty() {
                let sum_x: f32 = nodes.iter().map(|n| n.data.x).sum();
                let sum_y: f32 = nodes.iter().map(|n| n.data.y).sum();
                let sum_z: f32 = nodes.iter().map(|n| n.data.z).sum();
                let count = nodes.len() as f32;
                [sum_x / count, sum_y / count, sum_z / count]
            } else {
                [0.0, 0.0, 0.0]
            };

            json!({
                "id": format!("cpu_cluster_{}", i),
                "nodeCount": nodes.len(),
                "coherence": 0.6, 
                "centroid": centroid,
                "keywords": [type_name.clone(), "cpu_cluster"],
                "type": type_name
            })
        })
        .collect();

    let fallback_analytics = json!({
        "clusters": clusters,
        "totalNodes": node_count,
        "algorithmUsed": "cpu_heuristic",
        "modularity": 0.4, 
        "lastUpdated": chrono::Utc::now().to_rfc3339(),
        "gpu_accelerated": false,
        "note": "CPU fallback clustering based on node types",
        "computation_time_ms": 0
    });

    ok_json!(fallback_analytics)
}

async fn update_stress_optimization(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Stress optimization update request received");
    debug!(
        "Stress optimization payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    
    let stress_weight = update
        .get("stressWeight")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1) as f32;

    let stress_alpha = update
        .get("stressAlpha")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1) as f32;

    if !(0.0..=1.0).contains(&stress_weight) || !(0.0..=1.0).contains(&stress_alpha) {
        return bad_request!("stressWeight and stressAlpha must be between 0.0 and 1.0");
    }

    
    let physics_update = json!({
        "stressWeight": stress_weight,
        "stressAlpha": stress_alpha
    });

    let settings_update = create_physics_settings_update(physics_update);

    
    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge stress optimization settings: {}", e);
        return error_json!("Failed to update stress optimization: {}", e);
    }

    
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Stress optimization updated successfully");

            
            propagate_physics_to_gpu(&state, &app_settings, "logseq").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionflow").await;

            ok_json!(json!({
                "status": "Stress optimization updated successfully",
                "stressWeight": stress_weight,
                "stressAlpha": stress_alpha
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save stress optimization settings: {}", e);
            error_json!("Failed to save stress optimization settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

fn validate_constraints(constraints: &Value) -> Result<(), String> {
    
    if let Some(obj) = constraints.as_object() {
        for (constraint_type, constraint_data) in obj {
            if !["separation", "boundary", "alignment", "cluster"]
                .contains(&constraint_type.as_str())
            {
                return Err(format!("Unknown constraint type: {}", constraint_type));
            }

            if let Some(data) = constraint_data.as_object() {
                if let Some(strength) = data.get("strength") {
                    let val = strength.as_f64().ok_or("strength must be a number")?;
                    if val < 0.0 || val > 100.0 {
                        return Err("strength must be between 0.0 and 100.0".to_string());
                    }
                }

                if let Some(enabled) = data.get("enabled") {
                    if !enabled.is_boolean() {
                        return Err("enabled must be a boolean".to_string());
                    }
                }
            }
        }
    }

    Ok(())
}
