#![allow(dead_code)]
// Type definitions / request-response DTOs for the settings handler

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

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
    pub boundary_damping: f32,
    pub update_threshold: f32,
    pub dt: f32,
    pub temperature: f32,
    pub gravity: f32,
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
    pub clustering_algorithm: String,
    pub cluster_count: u32,
    pub clustering_resolution: f32,
    pub clustering_iterations: u32,
    /// X-axis separation between knowledge and ontology graph populations (default 0 = merged)
    #[serde(default)]
    pub graph_separation_x: f32,
    /// Semantic force strength for physicality clustering (default 0.40)
    #[serde(default)]
    pub physicality_strength: f32,
    /// Semantic force strength for role clustering (default 0.30)
    #[serde(default)]
    pub role_strength: f32,
    /// Semantic force strength for maturity clustering (default 0.15)
    #[serde(default)]
    pub maturity_strength: f32,
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
