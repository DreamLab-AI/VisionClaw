use config::ConfigError;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// New imports for enhanced validation and type generation
use lazy_static::lazy_static;
use regex::Regex;
use specta::Type;
use validator::{Validate, ValidationError};

pub mod dev_config;
pub mod path_access;

// Import the trait and functions we need
use path_access::{parse_path, PathAccessible};

// Centralized validation patterns
lazy_static! {
    
    static ref HEX_COLOR_REGEX: Regex = Regex::new(r"^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{8})$").expect("Invalid regex pattern");

    
    static ref URL_REGEX: Regex = Regex::new(r"^https?://[^\s/$.?#].[^\s]*$").expect("Invalid regex pattern");

    
    static ref FILE_PATH_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9._/\\-]+$").expect("Invalid regex pattern");

    
    static ref DOMAIN_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?)*$").expect("Invalid regex pattern");
}

pub fn validate_hex_color(color: &str) -> Result<(), ValidationError> {
    if !HEX_COLOR_REGEX.is_match(color) {
        return Err(ValidationError::new("invalid_hex_color"));
    }
    Ok(())
}

pub fn validate_width_range(range: &[f32]) -> Result<(), ValidationError> {
    if range.len() != 2 {
        return Err(ValidationError::new("width_range_length"));
    }
    if range[0] >= range[1] {
        return Err(ValidationError::new("width_range_order"));
    }
    Ok(())
}

pub fn validate_port(port: u16) -> Result<(), ValidationError> {
    if port == 0 {
        return Err(ValidationError::new("invalid_port"));
    }
    Ok(())
}

pub fn validate_percentage(value: f32) -> Result<(), ValidationError> {
    if !(0.0..=100.0).contains(&value) {
        return Err(ValidationError::new("invalid_percentage"));
    }
    Ok(())
}

pub fn validate_bloom_glow_settings(
    glow: &GlowSettings,
    bloom: &BloomSettings,
) -> Result<(), ValidationError> {
    
    if glow.intensity < 0.0 || glow.intensity > 10.0 {
        return Err(ValidationError::new("glow_intensity_out_of_range"));
    }
    if glow.radius < 0.0 || glow.radius > 10.0 {
        return Err(ValidationError::new("glow_radius_out_of_range"));
    }
    if glow.threshold < 0.0 || glow.threshold > 1.0 {
        return Err(ValidationError::new("glow_threshold_out_of_range"));
    }
    if glow.opacity < 0.0 || glow.opacity > 1.0 {
        return Err(ValidationError::new("glow_opacity_out_of_range"));
    }

    
    validate_hex_color(&glow.base_color)?;
    validate_hex_color(&glow.emission_color)?;

    
    if !glow.intensity.is_finite() {
        return Err(ValidationError::new("glow_intensity_not_finite"));
    }
    if !glow.radius.is_finite() {
        return Err(ValidationError::new("glow_radius_not_finite"));
    }
    if !glow.threshold.is_finite() {
        return Err(ValidationError::new("glow_threshold_not_finite"));
    }

    
    if bloom.intensity < 0.0 || bloom.intensity > 10.0 {
        return Err(ValidationError::new("bloom_intensity_out_of_range"));
    }
    if bloom.radius < 0.0 || bloom.radius > 10.0 {
        return Err(ValidationError::new("bloom_radius_out_of_range"));
    }
    if bloom.threshold < 0.0 || bloom.threshold > 1.0 {
        return Err(ValidationError::new("bloom_threshold_out_of_range"));
    }
    if bloom.strength < 0.0 || bloom.strength > 1.0 {
        return Err(ValidationError::new("bloom_strength_out_of_range"));
    }
    if bloom.knee < 0.0 || bloom.knee > 2.0 {
        return Err(ValidationError::new("bloom_knee_out_of_range"));
    }

    
    validate_hex_color(&bloom.color)?;
    validate_hex_color(&bloom.tint_color)?;

    
    if !bloom.intensity.is_finite() {
        return Err(ValidationError::new("bloom_intensity_not_finite"));
    }
    if !bloom.radius.is_finite() {
        return Err(ValidationError::new("bloom_radius_not_finite"));
    }
    if !bloom.threshold.is_finite() {
        return Err(ValidationError::new("bloom_threshold_not_finite"));
    }

    Ok(())
}

fn to_camel_case(snake_str: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for ch in snake_str.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

fn default_auto_balance_interval() -> u32 {
    500
}

fn default_constraint_ramp_frames() -> u32 {
    60 
}

fn default_constraint_max_force_per_node() -> f32 {
    50.0 
}

fn default_glow_color() -> String {
    "#00ffff".to_string()
}

fn default_glow_opacity() -> f32 {
    0.8
}

fn default_bounds_size() -> f32 {
    1000.0
}

pub mod feature_access;
// pub mod tests;

// Types are already public in this module, no need to re-export

// Helper function to convert empty strings to null for Option<String> fields
fn convert_empty_strings_to_null(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let new_map = map
                .into_iter()
                .map(|(k, v)| {
                    let new_v = match v {
                        Value::String(s) if s.is_empty() => {
                            
                            
                            
                            let required_string_fields = vec![
                                "base_color",
                                "color",
                                "background_color",
                                "text_color",
                                "text_outline_color",
                                "billboard_mode",
                                "quality",
                                "mode",
                                "context",
                                "cookie_samesite",
                                "audit_log_path",
                                "bind_address",
                                "domain",
                                "min_tls_version",
                                "tunnel_id",
                                "provider",
                                "ring_color",
                                "hand_mesh_color",
                                "hand_ray_color",
                                "teleport_ray_color",
                                "controller_ray_color",
                                "plane_color",
                                "portal_edge_color",
                                "space_type",
                                "locomotion_method",
                            ];

                            if required_string_fields.contains(&k.as_str()) {
                                
                                Value::String(s)
                            } else {
                                
                                Value::Null
                            }
                        }
                        Value::Object(_) => convert_empty_strings_to_null(v),
                        Value::Array(_) => convert_empty_strings_to_null(v),
                        _ => v,
                    };
                    (k, new_v)
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.into_iter().map(convert_empty_strings_to_null).collect())
        }
        _ => value,
    }
}

// Helper function to merge two JSON values
fn merge_json_values(base: Value, update: Value) -> Value {
    use serde_json::map::Entry;

    match (base, update) {
        (Value::Object(mut base_map), Value::Object(update_map)) => {
            for (key, update_value) in update_map {
                match base_map.entry(key) {
                    Entry::Occupied(mut entry) => {
                        let merged = merge_json_values(entry.get().clone(), update_value);
                        entry.insert(merged);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(update_value);
                    }
                }
            }
            Value::Object(base_map)
        }
        (_, update) => update, 
    }
}

static FIELD_MAPPINGS: std::sync::LazyLock<std::collections::HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        let mut field_mappings = std::collections::HashMap::new();

        
        field_mappings.insert("base_color", "baseColor");
        field_mappings.insert("emission_color", "emissionColor");
        field_mappings.insert("node_size", "nodeSize");
        field_mappings.insert("enable_instancing", "enableInstancing");
        field_mappings.insert("enable_hologram", "enableHologram");
        field_mappings.insert("enable_metadata_shape", "enableMetadataShape");
        field_mappings.insert(
            "enable_metadata_visualisation",
            "enableMetadataVisualisation",
        );

        
        field_mappings.insert("arrow_size", "arrowSize");
        field_mappings.insert("base_width", "baseWidth");
        field_mappings.insert("edge_color", "color");
        field_mappings.insert("edge_opacity", "opacity");
        field_mappings.insert("edge_width", "edgeWidth");
        field_mappings.insert("enable_arrows", "enableArrows");
        field_mappings.insert("width_range", "widthRange");

        
        field_mappings.insert("ambient_light_intensity", "ambientLightIntensity");
        field_mappings.insert("background_color", "backgroundColor");
        field_mappings.insert("directional_light_intensity", "directionalLightIntensity");
        field_mappings.insert("enable_ambient_occlusion", "enableAmbientOcclusion");
        field_mappings.insert("enable_antialiasing", "enableAntialiasing");
        field_mappings.insert("enable_shadows", "enableShadows");
        field_mappings.insert("environment_intensity", "environmentIntensity");
        field_mappings.insert("shadow_map_size", "shadowMapSize");
        field_mappings.insert("shadow_bias", "shadowBias");

        
        field_mappings.insert("enable_motion_blur", "enableMotionBlur");
        field_mappings.insert("enable_node_animations", "enableNodeAnimations");
        field_mappings.insert("motion_blur_strength", "motionBlurStrength");
        field_mappings.insert("animation_speed", "animationSpeed");

        
        field_mappings.insert(
            "equilibrium_velocity_threshold",
            "equilibriumVelocityThreshold",
        );
        field_mappings.insert("equilibrium_check_frames", "equilibriumCheckFrames");
        field_mappings.insert("equilibrium_energy_threshold", "equilibriumEnergyThreshold");
        field_mappings.insert("pause_on_equilibrium", "pauseOnEquilibrium");
        field_mappings.insert("resume_on_interaction", "resumeOnInteraction");

        
        field_mappings.insert("stability_variance_threshold", "stabilityVarianceThreshold");
        field_mappings.insert("stability_frame_count", "stabilityFrameCount");
        field_mappings.insert(
            "clustering_distance_threshold",
            "clusteringDistanceThreshold",
        );
        field_mappings.insert("clustering_hysteresis_buffer", "clusteringHysteresisBuffer");
        field_mappings.insert("bouncing_node_percentage", "bouncingNodePercentage");
        field_mappings.insert("boundary_min_distance", "boundaryMinDistance");
        field_mappings.insert("boundary_max_distance", "boundaryMaxDistance");
        field_mappings.insert("extreme_distance_threshold", "extremeDistanceThreshold");
        field_mappings.insert("explosion_distance_threshold", "explosionDistanceThreshold");
        field_mappings.insert("spreading_distance_threshold", "spreadingDistanceThreshold");
        field_mappings.insert("spreading_hysteresis_buffer", "spreadingHysteresisBuffer");
        field_mappings.insert("oscillation_detection_frames", "oscillationDetectionFrames");
        field_mappings.insert("oscillation_change_threshold", "oscillationChangeThreshold");
        field_mappings.insert("min_oscillation_changes", "minOscillationChanges");
        field_mappings.insert("parameter_adjustment_rate", "parameterAdjustmentRate");
        field_mappings.insert("max_adjustment_factor", "maxAdjustmentFactor");
        field_mappings.insert("min_adjustment_factor", "minAdjustmentFactor");
        field_mappings.insert("adjustment_cooldown_ms", "adjustmentCooldownMs");
        field_mappings.insert("state_change_cooldown_ms", "stateChangeCooldownMs");
        field_mappings.insert("parameter_dampening_factor", "parameterDampeningFactor");
        field_mappings.insert("hysteresis_delay_frames", "hysteresisDelayFrames");
        field_mappings.insert("grid_cell_size_min", "gridCellSizeMin");
        field_mappings.insert("grid_cell_size_max", "gridCellSizeMax");
        field_mappings.insert("repulsion_cutoff_min", "repulsionCutoffMin");
        field_mappings.insert("repulsion_cutoff_max", "repulsionCutoffMax");
        field_mappings.insert("repulsion_softening_min", "repulsionSofteningMin");
        field_mappings.insert("repulsion_softening_max", "repulsionSofteningMax");
        field_mappings.insert("center_gravity_min", "centerGravityMin");
        field_mappings.insert("center_gravity_max", "centerGravityMax");
        field_mappings.insert(
            "spatial_hash_efficiency_threshold",
            "spatialHashEfficiencyThreshold",
        );
        field_mappings.insert("cluster_density_threshold", "clusterDensityThreshold");
        field_mappings.insert(
            "numerical_instability_threshold",
            "numericalInstabilityThreshold",
        );

        
        field_mappings.insert("bounds_size", "boundsSize");
        field_mappings.insert("separation_radius", "separationRadius");
        field_mappings.insert("enable_bounds", "enableBounds");
        field_mappings.insert("max_velocity", "maxVelocity");
        field_mappings.insert("max_force", "maxForce");
        field_mappings.insert("repel_k", "repelK");
        field_mappings.insert("spring_k", "springK");
        field_mappings.insert("mass_scale", "massScale");
        field_mappings.insert("boundary_damping", "boundaryDamping");
        field_mappings.insert("update_threshold", "updateThreshold");
        field_mappings.insert("stress_weight", "stressWeight");
        field_mappings.insert("stress_alpha", "stressAlpha");
        field_mappings.insert("boundary_limit", "boundaryLimit");
        field_mappings.insert("alignment_strength", "alignmentStrength");
        field_mappings.insert("cluster_strength", "clusterStrength");
        field_mappings.insert("compute_mode", "computeMode");
        field_mappings.insert("rest_length", "restLength");
        field_mappings.insert("repulsion_cutoff", "repulsionCutoff");
        field_mappings.insert("repulsion_softening_epsilon", "repulsionSofteningEpsilon");
        field_mappings.insert("center_gravity_k", "centerGravityK");
        field_mappings.insert("grid_cell_size", "gridCellSize");
        field_mappings.insert("warmup_iterations", "warmupIterations");
        field_mappings.insert("cooling_rate", "coolingRate");
        field_mappings.insert("boundary_extreme_multiplier", "boundaryExtremeMultiplier");
        field_mappings.insert(
            "boundary_extreme_force_multiplier",
            "boundaryExtremeForceMultiplier",
        );
        field_mappings.insert("boundary_velocity_damping", "boundaryVelocityDamping");
        field_mappings.insert("min_distance", "minDistance");
        field_mappings.insert("max_repulsion_dist", "maxRepulsionDist");
        field_mappings.insert("boundary_margin", "boundaryMargin");
        field_mappings.insert("boundary_force_strength", "boundaryForceStrength");
        field_mappings.insert("warmup_curve", "warmupCurve");
        field_mappings.insert("zero_velocity_iterations", "zeroVelocityIterations");

        
        field_mappings.insert("host_port", "hostPort");
        field_mappings.insert("log_level", "logLevel");
        field_mappings.insert("persist_settings", "persistSettings");
        field_mappings.insert("gpu_memory_limit", "gpuMemoryLimit");

        field_mappings
    });

fn normalize_field_names_to_camel_case(value: Value) -> Result<Value, String> {
    normalize_object_fields(value, &FIELD_MAPPINGS)
}

fn normalize_object_fields(
    value: Value,
    mappings: &std::collections::HashMap<&str, &str>,
) -> Result<Value, String> {
    match value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();

            for (key, val) in map {
                
                let normalized_key = if let Some(&camel_case_key) = mappings.get(key.as_str()) {
                    
                    camel_case_key.to_string()
                } else {
                    
                    key
                };

                
                let normalized_value = normalize_object_fields(val, mappings)?;
                new_map.insert(normalized_key, normalized_value);
            }

            Ok(Value::Object(new_map))
        }
        Value::Array(arr) => {
            let normalized_array: Result<Vec<Value>, String> = arr
                .into_iter()
                .map(|item| normalize_object_fields(item, mappings))
                .collect();
            Ok(Value::Array(normalized_array?))
        }
        
        _ => Ok(value),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct MovementAxes {
    #[serde(alias = "horizontal")]
    pub horizontal: i32,
    #[serde(alias = "vertical")]
    pub vertical: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct NodeSettings {
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(alias = "base_color")]
    pub base_color: String,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "metalness")]
    pub metalness: f32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "opacity")]
    pub opacity: f32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "roughness")]
    pub roughness: f32,
    #[validate(range(min = 0.1, max = 100.0))]
    #[serde(alias = "node_size")]
    pub node_size: f32,
    #[serde(alias = "quality")]
    pub quality: String,
    #[serde(alias = "enable_instancing")]
    pub enable_instancing: bool,
    #[serde(alias = "enable_hologram")]
    pub enable_hologram: bool,
    #[serde(alias = "enable_metadata_shape")]
    pub enable_metadata_shape: bool,
    #[serde(alias = "enable_metadata_visualisation")]
    pub enable_metadata_visualisation: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSettings {
    #[validate(range(min = 0.01, max = 5.0))]
    #[serde(alias = "arrow_size")]
    pub arrow_size: f32,
    #[validate(range(min = 0.01, max = 5.0))]
    #[serde(alias = "base_width")]
    pub base_width: f32,
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(alias = "color")]
    pub color: String,
    #[serde(alias = "enable_arrows")]
    pub enable_arrows: bool,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "opacity")]
    pub opacity: f32,
    #[validate(custom(function = "validate_width_range"))]
    #[serde(alias = "width_range")]
    pub width_range: Vec<f32>,
    #[serde(alias = "quality")]
    pub quality: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AutoPauseConfig {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(alias = "equilibrium_velocity_threshold")]
    pub equilibrium_velocity_threshold: f32,
    #[validate(range(min = 1, max = 300))]
    #[serde(alias = "equilibrium_check_frames")]
    pub equilibrium_check_frames: u32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "equilibrium_energy_threshold")]
    pub equilibrium_energy_threshold: f32,
    #[serde(alias = "pause_on_equilibrium")]
    pub pause_on_equilibrium: bool,
    #[serde(alias = "resume_on_interaction")]
    pub resume_on_interaction: bool,
}

impl AutoPauseConfig {
    pub fn default() -> Self {
        Self {
            enabled: true,
            equilibrium_velocity_threshold: 0.1,
            equilibrium_check_frames: 30,
            equilibrium_energy_threshold: 0.01,
            pause_on_equilibrium: true,
            resume_on_interaction: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AutoBalanceConfig {
    #[serde(alias = "stability_variance_threshold")]
    pub stability_variance_threshold: f32,
    #[serde(alias = "stability_frame_count")]
    pub stability_frame_count: u32,
    #[serde(alias = "clustering_distance_threshold")]
    pub clustering_distance_threshold: f32,
    #[serde(alias = "clustering_hysteresis_buffer")]
    pub clustering_hysteresis_buffer: f32,
    #[serde(alias = "bouncing_node_percentage")]
    pub bouncing_node_percentage: f32,
    #[serde(alias = "boundary_min_distance")]
    pub boundary_min_distance: f32,
    #[serde(alias = "boundary_max_distance")]
    pub boundary_max_distance: f32,
    #[serde(alias = "extreme_distance_threshold")]
    pub extreme_distance_threshold: f32,
    #[serde(alias = "explosion_distance_threshold")]
    pub explosion_distance_threshold: f32,
    #[serde(alias = "spreading_distance_threshold")]
    pub spreading_distance_threshold: f32,
    #[serde(alias = "spreading_hysteresis_buffer")]
    pub spreading_hysteresis_buffer: f32,
    #[serde(alias = "oscillation_detection_frames")]
    pub oscillation_detection_frames: usize,
    #[serde(alias = "oscillation_change_threshold")]
    pub oscillation_change_threshold: f32,
    #[serde(alias = "min_oscillation_changes")]
    pub min_oscillation_changes: usize,

    
    #[serde(alias = "parameter_adjustment_rate")]
    pub parameter_adjustment_rate: f32,
    #[serde(alias = "max_adjustment_factor")]
    pub max_adjustment_factor: f32,
    #[serde(alias = "min_adjustment_factor")]
    pub min_adjustment_factor: f32,
    #[serde(alias = "adjustment_cooldown_ms")]
    pub adjustment_cooldown_ms: u64,
    #[serde(alias = "state_change_cooldown_ms")]
    pub state_change_cooldown_ms: u64,
    #[serde(alias = "parameter_dampening_factor")]
    pub parameter_dampening_factor: f32,
    #[serde(alias = "hysteresis_delay_frames")]
    pub hysteresis_delay_frames: u32,

    
    #[serde(alias = "grid_cell_size_min")]
    pub grid_cell_size_min: f32,
    #[serde(alias = "grid_cell_size_max")]
    pub grid_cell_size_max: f32,
    #[serde(alias = "repulsion_cutoff_min")]
    pub repulsion_cutoff_min: f32,
    #[serde(alias = "repulsion_cutoff_max")]
    pub repulsion_cutoff_max: f32,
    #[serde(alias = "repulsion_softening_min")]
    pub repulsion_softening_min: f32,
    #[serde(alias = "repulsion_softening_max")]
    pub repulsion_softening_max: f32,
    #[serde(alias = "center_gravity_min")]
    pub center_gravity_min: f32,
    #[serde(alias = "center_gravity_max")]
    pub center_gravity_max: f32,

    
    #[serde(alias = "spatial_hash_efficiency_threshold")]
    pub spatial_hash_efficiency_threshold: f32,
    #[serde(alias = "cluster_density_threshold")]
    pub cluster_density_threshold: f32,
    #[serde(alias = "numerical_instability_threshold")]
    pub numerical_instability_threshold: f32,
}

impl AutoBalanceConfig {
    pub fn default() -> Self {
        Self {
            stability_variance_threshold: 100.0,
            stability_frame_count: 180,
            clustering_distance_threshold: 20.0,
            clustering_hysteresis_buffer: 5.0,
            bouncing_node_percentage: 0.33,
            boundary_min_distance: 90.0,
            boundary_max_distance: 110.0,
            extreme_distance_threshold: 1000.0,
            explosion_distance_threshold: 10000.0,
            spreading_distance_threshold: 500.0,
            spreading_hysteresis_buffer: 50.0,
            oscillation_detection_frames: 20,
            oscillation_change_threshold: 10.0,
            min_oscillation_changes: 8,

            
            parameter_adjustment_rate: 0.1,
            max_adjustment_factor: 0.2,
            min_adjustment_factor: -0.2,
            adjustment_cooldown_ms: 2000,
            state_change_cooldown_ms: 1000,
            parameter_dampening_factor: 0.05,
            hysteresis_delay_frames: 30,

            
            grid_cell_size_min: 1.0,
            grid_cell_size_max: 50.0,
            repulsion_cutoff_min: 5.0,
            repulsion_cutoff_max: 200.0,
            repulsion_softening_min: 1e-6,
            repulsion_softening_max: 1.0,
            center_gravity_min: 0.0,
            center_gravity_max: 0.1,

            
            spatial_hash_efficiency_threshold: 0.3, 
            cluster_density_threshold: 50.0,        
            numerical_instability_threshold: 1e-3,  
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsSettings {
    #[serde(default, alias = "auto_balance")]
    pub auto_balance: bool,
    #[serde(
        default = "default_auto_balance_interval",
        alias = "auto_balance_interval_ms"
    )]
    pub auto_balance_interval_ms: u32,
    #[serde(default, alias = "auto_balance_config")]
    #[validate(nested)]
    pub auto_balance_config: AutoBalanceConfig,
    #[serde(default, alias = "auto_pause")]
    #[validate(nested)]
    pub auto_pause: AutoPauseConfig,
    #[serde(default = "default_bounds_size", alias = "bounds_size")]
    pub bounds_size: f32,
    #[serde(alias = "separation_radius")]
    pub separation_radius: f32,
    #[serde(alias = "damping")]
    pub damping: f32,
    #[serde(alias = "enable_bounds")]
    pub enable_bounds: bool,
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[serde(alias = "iterations")]
    pub iterations: u32,
    #[serde(alias = "max_velocity")]
    pub max_velocity: f32,
    #[serde(alias = "max_force")]
    pub max_force: f32,
    #[serde(alias = "repel_k")]
    pub repel_k: f32,
    #[serde(alias = "spring_k")]
    pub spring_k: f32,
    #[serde(alias = "mass_scale")]
    pub mass_scale: f32,
    #[serde(alias = "boundary_damping")]
    pub boundary_damping: f32,
    #[serde(alias = "update_threshold")]
    pub update_threshold: f32,
    #[serde(alias = "dt")]
    pub dt: f32,
    #[serde(alias = "temperature")]
    pub temperature: f32,
    #[serde(alias = "gravity")]
    pub gravity: f32,
    
    #[serde(alias = "stress_weight")]
    pub stress_weight: f32,
    #[serde(alias = "stress_alpha")]
    pub stress_alpha: f32,
    #[serde(alias = "boundary_limit")]
    pub boundary_limit: f32,
    #[serde(alias = "alignment_strength")]
    pub alignment_strength: f32,
    #[serde(alias = "cluster_strength")]
    pub cluster_strength: f32,
    #[serde(alias = "compute_mode")]
    pub compute_mode: i32,

    
    #[serde(alias = "rest_length")]
    pub rest_length: f32,
    #[serde(alias = "repulsion_cutoff")]
    pub repulsion_cutoff: f32,
    #[serde(alias = "repulsion_softening_epsilon")]
    pub repulsion_softening_epsilon: f32,
    #[serde(alias = "center_gravity_k")]
    pub center_gravity_k: f32,
    #[serde(alias = "grid_cell_size")]
    pub grid_cell_size: f32,
    #[serde(alias = "warmup_iterations")]
    pub warmup_iterations: u32,
    #[serde(alias = "cooling_rate")]
    pub cooling_rate: f32,
    #[serde(alias = "boundary_extreme_multiplier")]
    pub boundary_extreme_multiplier: f32,
    #[serde(alias = "boundary_extreme_force_multiplier")]
    pub boundary_extreme_force_multiplier: f32,
    #[serde(alias = "boundary_velocity_damping")]
    pub boundary_velocity_damping: f32,
    
    #[serde(alias = "min_distance")]
    pub min_distance: f32,
    #[serde(alias = "max_repulsion_dist")]
    pub max_repulsion_dist: f32,
    #[serde(alias = "boundary_margin")]
    pub boundary_margin: f32,
    #[serde(alias = "boundary_force_strength")]
    pub boundary_force_strength: f32,
    #[serde(alias = "warmup_curve")]
    pub warmup_curve: String,
    #[serde(alias = "zero_velocity_iterations")]
    pub zero_velocity_iterations: u32,

    
    #[serde(
        alias = "constraint_ramp_frames",
        default = "default_constraint_ramp_frames"
    )]
    pub constraint_ramp_frames: u32,
    #[serde(
        alias = "constraint_max_force_per_node",
        default = "default_constraint_max_force_per_node"
    )]
    pub constraint_max_force_per_node: f32,

    
    #[serde(alias = "clustering_algorithm")]
    pub clustering_algorithm: String,
    #[serde(alias = "cluster_count")]
    pub cluster_count: u32,
    #[serde(alias = "clustering_resolution")]
    pub clustering_resolution: f32,
    #[serde(alias = "clustering_iterations")]
    pub clustering_iterations: u32,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            auto_balance: false,
            auto_balance_interval_ms: 500,
            auto_balance_config: AutoBalanceConfig::default(),
            auto_pause: AutoPauseConfig::default(),
            bounds_size: 500.0,
            separation_radius: 2.0,
            damping: 0.95,
            enable_bounds: true,
            enabled: true,
            iterations: 100,
            max_velocity: 1.0,
            max_force: 100.0,
            repel_k: 50.0,
            spring_k: 0.005,
            mass_scale: 1.0,
            boundary_damping: 0.95,
            update_threshold: 0.01,
            dt: 0.016,
            temperature: 0.01,
            gravity: 0.0001,
            stress_weight: 0.1,
            stress_alpha: 0.1,
            boundary_limit: 490.0,
            alignment_strength: 0.0,
            cluster_strength: 0.0,
            compute_mode: 0,
            
            rest_length: 50.0,
            repulsion_cutoff: 50.0,
            repulsion_softening_epsilon: 0.0001,
            center_gravity_k: 0.0,
            grid_cell_size: 50.0,
            warmup_iterations: 100,
            cooling_rate: 0.001,
            boundary_extreme_multiplier: 2.0,
            boundary_extreme_force_multiplier: 10.0,
            boundary_velocity_damping: 0.5,
            
            min_distance: 0.15,
            max_repulsion_dist: 50.0,
            boundary_margin: 0.85,
            boundary_force_strength: 2.0,
            warmup_curve: "quadratic".to_string(),
            zero_velocity_iterations: 5,
            
            constraint_ramp_frames: default_constraint_ramp_frames(),
            constraint_max_force_per_node: default_constraint_max_force_per_node(),
            
            clustering_algorithm: "none".to_string(),
            cluster_count: 5,
            clustering_resolution: 1.0,
            clustering_iterations: 30,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RenderingSettings {
    #[serde(alias = "ambient_light_intensity")]
    pub ambient_light_intensity: f32,
    #[serde(alias = "background_color")]
    pub background_color: String,
    #[serde(alias = "directional_light_intensity")]
    pub directional_light_intensity: f32,
    #[serde(alias = "enable_ambient_occlusion")]
    pub enable_ambient_occlusion: bool,
    #[serde(alias = "enable_antialiasing")]
    pub enable_antialiasing: bool,
    #[serde(alias = "enable_shadows")]
    pub enable_shadows: bool,
    #[serde(alias = "environment_intensity")]
    pub environment_intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none", alias = "shadow_map_size")]
    pub shadow_map_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "shadow_bias")]
    pub shadow_bias: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "context")]
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AnimationSettings {
    #[serde(alias = "enable_motion_blur")]
    pub enable_motion_blur: bool,
    #[serde(alias = "enable_node_animations")]
    pub enable_node_animations: bool,
    #[serde(alias = "motion_blur_strength")]
    pub motion_blur_strength: f32,
    #[serde(alias = "selection_wave_enabled")]
    pub selection_wave_enabled: bool,
    #[serde(alias = "pulse_enabled")]
    pub pulse_enabled: bool,
    #[serde(alias = "pulse_speed")]
    pub pulse_speed: f32,
    #[serde(alias = "pulse_strength")]
    pub pulse_strength: f32,
    #[serde(alias = "wave_speed")]
    pub wave_speed: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct LabelSettings {
    #[serde(alias = "desktop_font_size")]
    pub desktop_font_size: f32,
    #[serde(alias = "enable_labels")]
    pub enable_labels: bool,
    #[serde(alias = "text_color")]
    pub text_color: String,
    #[serde(alias = "text_outline_color")]
    pub text_outline_color: String,
    #[serde(alias = "text_outline_width")]
    pub text_outline_width: f32,
    #[serde(alias = "text_resolution")]
    pub text_resolution: u32,
    #[serde(alias = "text_padding")]
    pub text_padding: f32,
    #[serde(alias = "billboard_mode")]
    pub billboard_mode: String,
    #[serde(skip_serializing_if = "Option::is_none", alias = "show_metadata")]
    pub show_metadata: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "max_label_width")]
    pub max_label_width: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GlowSettings {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(alias = "intensity")]
    pub intensity: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(alias = "radius")]
    pub radius: f32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "threshold")]
    pub threshold: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "diffuse_strength")]
    pub diffuse_strength: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "atmospheric_density")]
    pub atmospheric_density: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "volumetric_intensity")]
    pub volumetric_intensity: f32,
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(
        skip_serializing_if = "String::is_empty",
        default = "default_glow_color",
        alias = "base_color"
    )]
    pub base_color: String,
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(
        skip_serializing_if = "String::is_empty",
        default = "default_glow_color",
        alias = "emission_color"
    )]
    pub emission_color: String,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(default = "default_glow_opacity", alias = "opacity")]
    pub opacity: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "pulse_speed")]
    pub pulse_speed: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "flow_speed")]
    pub flow_speed: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "node_glow_strength")]
    pub node_glow_strength: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "edge_glow_strength")]
    pub edge_glow_strength: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default, alias = "environment_glow_strength")]
    pub environment_glow_strength: f32,
}

fn default_bloom_intensity() -> f32 {
    1.0
}

fn default_bloom_radius() -> f32 {
    0.8
}

fn default_bloom_threshold() -> f32 {
    0.15
}

fn default_bloom_color() -> String {
    "#ffffff".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct BloomSettings {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default = "default_bloom_intensity", alias = "intensity")]
    pub intensity: f32,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(default = "default_bloom_radius", alias = "radius")]
    pub radius: f32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(default = "default_bloom_threshold", alias = "threshold")]
    pub threshold: f32,
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(
        skip_serializing_if = "String::is_empty",
        default = "default_bloom_color",
        alias = "color"
    )]
    pub color: String,
    #[validate(custom(function = "validate_hex_color"))]
    #[serde(
        skip_serializing_if = "String::is_empty",
        default = "default_bloom_color",
        alias = "tint_color"
    )]
    pub tint_color: String,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(default, alias = "strength")]
    pub strength: f32,
    #[validate(range(min = 0.0, max = 5.0))]
    #[serde(default, alias = "blur_passes")]
    pub blur_passes: f32,
    #[validate(range(min = 0.0, max = 2.0))]
    #[serde(default, alias = "knee")]
    pub knee: f32,
}

impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: default_bloom_intensity(),
            radius: default_bloom_radius(),
            threshold: default_bloom_threshold(),
            color: default_bloom_color(),
            tint_color: default_bloom_color(),
            strength: 0.8,
            blur_passes: 1.0,
            knee: 0.7,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct HologramSettings {
    #[serde(alias = "ring_count")]
    pub ring_count: u32,
    #[serde(alias = "ring_color")]
    pub ring_color: String,
    #[serde(alias = "ring_opacity")]
    pub ring_opacity: f32,
    #[serde(alias = "sphere_sizes")]
    pub sphere_sizes: Vec<f32>,
    #[serde(alias = "ring_rotation_speed")]
    pub ring_rotation_speed: f32,
    #[serde(alias = "enable_buckminster")]
    pub enable_buckminster: bool,
    #[serde(alias = "buckminster_size")]
    pub buckminster_size: f32,
    #[serde(alias = "buckminster_opacity")]
    pub buckminster_opacity: f32,
    #[serde(alias = "enable_geodesic")]
    pub enable_geodesic: bool,
    #[serde(alias = "geodesic_size")]
    pub geodesic_size: f32,
    #[serde(alias = "geodesic_opacity")]
    pub geodesic_opacity: f32,
    #[serde(alias = "enable_triangle_sphere")]
    pub enable_triangle_sphere: bool,
    #[serde(alias = "triangle_sphere_size")]
    pub triangle_sphere_size: f32,
    #[serde(alias = "triangle_sphere_opacity")]
    pub triangle_sphere_opacity: f32,
    #[serde(alias = "global_rotation_speed")]
    pub global_rotation_speed: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CameraSettings {
    #[serde(alias = "fov")]
    pub fov: f32,
    #[serde(alias = "near")]
    pub near: f32,
    #[serde(alias = "far")]
    pub far: f32,
    #[serde(alias = "position")]
    pub position: Position,
    #[serde(alias = "look_at")]
    pub look_at: Position,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    #[serde(alias = "x")]
    pub x: f32,
    #[serde(alias = "y")]
    pub y: f32,
    #[serde(alias = "z")]
    pub z: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SpacePilotSettings {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[serde(alias = "mode")]
    pub mode: String,
    #[serde(alias = "sensitivity")]
    pub sensitivity: Sensitivity,
    #[serde(alias = "smoothing")]
    pub smoothing: f32,
    #[serde(alias = "deadzone")]
    pub deadzone: f32,
    #[serde(alias = "button_functions")]
    pub button_functions: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sensitivity {
    #[serde(alias = "translation")]
    pub translation: f32,
    #[serde(alias = "rotation")]
    pub rotation: f32,
}

// Graph-specific settings
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GraphSettings {
    #[validate(nested)]
    pub nodes: NodeSettings,
    #[validate(nested)]
    pub edges: EdgeSettings,
    #[validate(nested)]
    pub labels: LabelSettings,
    #[validate(nested)]
    pub physics: PhysicsSettings,
}

// Multi-graph container
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GraphsSettings {
    #[validate(nested)]
    pub logseq: GraphSettings,
    #[validate(nested)]
    pub visionflow: GraphSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VisualisationSettings {
    
    #[validate(nested)]
    pub rendering: RenderingSettings,
    #[validate(nested)]
    pub animations: AnimationSettings,
    #[validate(nested)]
    pub glow: GlowSettings,
    #[validate(nested)]
    pub bloom: BloomSettings,
    #[validate(nested)]
    pub hologram: HologramSettings,
    #[validate(nested)]
    pub graphs: GraphsSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_pilot: Option<SpacePilotSettings>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    #[serde(alias = "bind_address")]
    pub bind_address: String,
    #[serde(alias = "domain")]
    pub domain: String,
    #[serde(alias = "enable_http2")]
    pub enable_http2: bool,
    #[serde(alias = "enable_rate_limiting")]
    pub enable_rate_limiting: bool,
    #[serde(alias = "enable_tls")]
    pub enable_tls: bool,
    #[serde(alias = "max_request_size")]
    pub max_request_size: usize,
    #[serde(alias = "min_tls_version")]
    pub min_tls_version: String,
    #[serde(alias = "port")]
    pub port: u16,
    #[serde(alias = "rate_limit_requests")]
    pub rate_limit_requests: u32,
    #[serde(alias = "rate_limit_window")]
    pub rate_limit_window: u32,
    #[serde(alias = "tunnel_id")]
    pub tunnel_id: String,
    #[serde(alias = "api_client_timeout")]
    pub api_client_timeout: u64,
    #[serde(alias = "enable_metrics")]
    pub enable_metrics: bool,
    #[serde(alias = "max_concurrent_requests")]
    pub max_concurrent_requests: u32,
    #[serde(alias = "max_retries")]
    pub max_retries: u32,
    #[serde(alias = "metrics_port")]
    pub metrics_port: u16,
    #[serde(alias = "retry_delay")]
    pub retry_delay: u32,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(), 
            port: 8080,                          
            domain: String::new(),
            enable_http2: false,
            enable_rate_limiting: false,
            enable_tls: false,
            max_request_size: 10485760, 
            min_tls_version: "1.2".to_string(),
            rate_limit_requests: 100,
            rate_limit_window: 60,
            tunnel_id: String::new(),
            api_client_timeout: 30,
            enable_metrics: true,
            max_concurrent_requests: 1000,
            max_retries: 3,
            metrics_port: 9090,
            retry_delay: 1000, 
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketSettings {
    #[serde(alias = "binary_chunk_size")]
    pub binary_chunk_size: usize,
    #[serde(alias = "binary_update_rate")]
    pub binary_update_rate: u32,
    #[serde(alias = "min_update_rate")]
    pub min_update_rate: u32,
    #[serde(alias = "max_update_rate")]
    pub max_update_rate: u32,
    #[serde(alias = "motion_threshold")]
    pub motion_threshold: f32,
    #[serde(alias = "motion_damping")]
    pub motion_damping: f32,
    #[serde(alias = "binary_message_version")]
    pub binary_message_version: u32,
    #[serde(alias = "compression_enabled")]
    pub compression_enabled: bool,
    #[serde(alias = "compression_threshold")]
    pub compression_threshold: usize,
    #[serde(alias = "heartbeat_interval")]
    pub heartbeat_interval: u64,
    #[serde(alias = "heartbeat_timeout")]
    pub heartbeat_timeout: u64,
    #[serde(alias = "max_connections")]
    pub max_connections: usize,
    #[serde(alias = "max_message_size")]
    pub max_message_size: usize,
    #[serde(alias = "reconnect_attempts")]
    pub reconnect_attempts: u32,
    #[serde(alias = "reconnect_delay")]
    pub reconnect_delay: u64,
    #[serde(alias = "update_rate")]
    pub update_rate: u32,
}

impl Default for WebSocketSettings {
    fn default() -> Self {
        Self {
            binary_chunk_size: 2048,
            binary_update_rate: 30,
            min_update_rate: 5,
            max_update_rate: 60,
            motion_threshold: 0.05,
            motion_damping: 0.9,
            binary_message_version: 1,
            compression_enabled: false,
            compression_threshold: 512,
            heartbeat_interval: 10000,
            heartbeat_timeout: 600000,
            max_connections: 100,
            max_message_size: 10485760,
            reconnect_attempts: 5,
            reconnect_delay: 1000,
            update_rate: 60,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    #[serde(alias = "allowed_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(alias = "audit_log_path")]
    pub audit_log_path: String,
    #[serde(alias = "cookie_httponly")]
    pub cookie_httponly: bool,
    #[serde(alias = "cookie_samesite")]
    pub cookie_samesite: String,
    #[serde(alias = "cookie_secure")]
    pub cookie_secure: bool,
    #[serde(alias = "csrf_token_timeout")]
    pub csrf_token_timeout: u32,
    #[serde(alias = "enable_audit_logging")]
    pub enable_audit_logging: bool,
    #[serde(alias = "enable_request_validation")]
    pub enable_request_validation: bool,
    #[serde(alias = "session_timeout")]
    pub session_timeout: u32,
}

// Simple debug settings for server-side control
#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DebugSettings {
    #[serde(default, alias = "enabled")]
    pub enabled: bool,
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettings {
    #[validate(nested)]
    #[serde(alias = "network")]
    pub network: NetworkSettings,
    #[validate(nested)]
    #[serde(alias = "websocket")]
    pub websocket: WebSocketSettings,
    #[validate(nested)]
    #[serde(alias = "security")]
    pub security: SecuritySettings,
    #[validate(nested)]
    #[serde(alias = "debug")]
    pub debug: DebugSettings,
    #[serde(default, alias = "persist_settings")]
    pub persist_settings: bool,
    #[serde(skip_serializing_if = "Option::is_none", alias = "custom_backend_url")]
    pub custom_backend_url: Option<String>,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            network: NetworkSettings::default(),
            websocket: WebSocketSettings::default(),
            security: SecuritySettings::default(),
            debug: DebugSettings::default(),
            persist_settings: false,
            custom_backend_url: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct XRSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "enabled")]
    pub enabled: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "client_side_enable_xr"
    )]
    pub client_side_enable_xr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "mode")]
    pub mode: Option<String>,
    #[serde(alias = "room_scale")]
    pub room_scale: f32,
    #[serde(alias = "space_type")]
    pub space_type: String,
    #[serde(alias = "quality")]
    pub quality: String,
    #[serde(skip_serializing_if = "Option::is_none", alias = "render_scale")]
    pub render_scale: Option<f32>,
    #[serde(alias = "interaction_distance")]
    pub interaction_distance: f32,
    #[serde(alias = "locomotion_method")]
    pub locomotion_method: String,
    #[serde(alias = "teleport_ray_color")]
    pub teleport_ray_color: String,
    #[serde(alias = "controller_ray_color")]
    pub controller_ray_color: String,
    #[serde(skip_serializing_if = "Option::is_none", alias = "controller_model")]
    pub controller_model: Option<String>,

    #[serde(alias = "enable_hand_tracking")]
    pub enable_hand_tracking: bool,
    #[serde(alias = "hand_mesh_enabled")]
    pub hand_mesh_enabled: bool,
    #[serde(alias = "hand_mesh_color")]
    pub hand_mesh_color: String,
    #[serde(alias = "hand_mesh_opacity")]
    pub hand_mesh_opacity: f32,
    #[serde(alias = "hand_point_size")]
    pub hand_point_size: f32,
    #[serde(alias = "hand_ray_enabled")]
    pub hand_ray_enabled: bool,
    #[serde(alias = "hand_ray_color")]
    pub hand_ray_color: String,
    #[serde(alias = "hand_ray_width")]
    pub hand_ray_width: f32,
    #[serde(alias = "gesture_smoothing")]
    pub gesture_smoothing: f32,

    #[serde(alias = "enable_haptics")]
    pub enable_haptics: bool,
    #[serde(alias = "haptic_intensity")]
    pub haptic_intensity: f32,
    #[serde(alias = "drag_threshold")]
    pub drag_threshold: f32,
    #[serde(alias = "pinch_threshold")]
    pub pinch_threshold: f32,
    #[serde(alias = "rotation_threshold")]
    pub rotation_threshold: f32,
    #[serde(alias = "interaction_radius")]
    pub interaction_radius: f32,
    #[serde(alias = "movement_speed")]
    pub movement_speed: f32,
    #[serde(alias = "dead_zone")]
    pub dead_zone: f32,
    #[serde(alias = "movement_axes")]
    pub movement_axes: MovementAxes,

    #[serde(alias = "enable_light_estimation")]
    pub enable_light_estimation: bool,
    #[serde(alias = "enable_plane_detection")]
    pub enable_plane_detection: bool,
    #[serde(alias = "enable_scene_understanding")]
    pub enable_scene_understanding: bool,
    #[serde(alias = "plane_color")]
    pub plane_color: String,
    #[serde(alias = "plane_opacity")]
    pub plane_opacity: f32,
    #[serde(alias = "plane_detection_distance")]
    pub plane_detection_distance: f32,
    #[serde(alias = "show_plane_overlay")]
    pub show_plane_overlay: bool,
    #[serde(alias = "snap_to_floor")]
    pub snap_to_floor: bool,

    #[serde(alias = "enable_passthrough_portal")]
    pub enable_passthrough_portal: bool,
    #[serde(alias = "passthrough_opacity")]
    pub passthrough_opacity: f32,
    #[serde(alias = "passthrough_brightness")]
    pub passthrough_brightness: f32,
    #[serde(alias = "passthrough_contrast")]
    pub passthrough_contrast: f32,
    #[serde(alias = "portal_size")]
    pub portal_size: f32,
    #[serde(alias = "portal_edge_color")]
    pub portal_edge_color: String,
    #[serde(alias = "portal_edge_width")]
    pub portal_edge_width: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AuthSettings {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[serde(alias = "provider")]
    pub provider: String,
    #[serde(alias = "required")]
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RagFlowSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "agent_id")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_base_url")]
    pub api_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "max_retries")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "chat_id")]
    pub chat_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "model")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "presence_penalty")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "frequency_penalty")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "rate_limit")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OpenAISettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "base_url")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "rate_limit")]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct KokoroSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_voice")]
    pub default_voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_format")]
    pub default_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_speed")]
    pub default_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "stream")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "return_timestamps")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "sample_rate")]
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSettings {
    #[serde(skip_serializing_if = "Option::is_none", alias = "api_url")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_model")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "default_language")]
    pub default_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "return_timestamps")]
    pub return_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "vad_filter")]
    pub vad_filter: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "word_timestamps")]
    pub word_timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "initial_prompt")]
    pub initial_prompt: Option<String>,
}

// Constraint system structures
// Note: ConstraintData has been moved to models/constraints.rs for GPU compatibility
// The old simple structure has been replaced with a GPU-optimized version

// Legacy constraint system for web API compatibility
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct LegacyConstraintData {
    #[serde(alias = "constraint_type")]
    pub constraint_type: i32, 
    #[serde(alias = "strength")]
    pub strength: f32,
    #[serde(alias = "param1")]
    pub param1: f32,
    #[serde(alias = "param2")]
    pub param2: f32,
    #[serde(alias = "node_mask")]
    pub node_mask: i32, 
    #[serde(alias = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSystem {
    #[serde(alias = "separation")]
    pub separation: LegacyConstraintData,
    #[serde(alias = "boundary")]
    pub boundary: LegacyConstraintData,
    #[serde(alias = "alignment")]
    pub alignment: LegacyConstraintData,
    #[serde(alias = "cluster")]
    pub cluster: LegacyConstraintData,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringConfiguration {
    #[serde(alias = "algorithm")]
    pub algorithm: String,
    #[serde(alias = "num_clusters")]
    pub num_clusters: u32,
    #[serde(alias = "resolution")]
    pub resolution: f32,
    #[serde(alias = "iterations")]
    pub iterations: u32,
    #[serde(alias = "export_assignments")]
    pub export_assignments: bool,
    #[serde(alias = "auto_update")]
    pub auto_update: bool,
}

// Helper struct for physics updates
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsUpdate {
    #[serde(alias = "damping")]
    pub damping: Option<f32>,
    #[serde(alias = "spring_k")]
    pub spring_k: Option<f32>,
    #[serde(alias = "repel_k")]
    pub repel_k: Option<f32>,
    #[serde(alias = "iterations")]
    pub iterations: Option<u32>,
    #[serde(alias = "enabled")]
    pub enabled: Option<bool>,
    #[serde(alias = "bounds_size")]
    pub bounds_size: Option<f32>,
    #[serde(alias = "enable_bounds")]
    pub enable_bounds: Option<bool>,
    #[serde(alias = "max_velocity")]
    pub max_velocity: Option<f32>,
    #[serde(alias = "max_force")]
    pub max_force: Option<f32>,
    #[serde(alias = "separation_radius")]
    pub separation_radius: Option<f32>,
    #[serde(alias = "mass_scale")]
    pub mass_scale: Option<f32>,
    #[serde(alias = "boundary_damping")]
    pub boundary_damping: Option<f32>,
    #[serde(alias = "dt")]
    pub dt: Option<f32>,
    #[serde(alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(alias = "gravity")]
    pub gravity: Option<f32>,
    #[serde(alias = "update_threshold")]
    pub update_threshold: Option<f32>,
    
    #[serde(alias = "stress_weight")]
    pub stress_weight: Option<f32>,
    #[serde(alias = "stress_alpha")]
    pub stress_alpha: Option<f32>,
    #[serde(alias = "boundary_limit")]
    pub boundary_limit: Option<f32>,
    #[serde(alias = "alignment_strength")]
    pub alignment_strength: Option<f32>,
    #[serde(alias = "cluster_strength")]
    pub cluster_strength: Option<f32>,
    #[serde(alias = "compute_mode")]
    pub compute_mode: Option<i32>,
    
    #[serde(alias = "min_distance")]
    pub min_distance: Option<f32>,
    #[serde(alias = "max_repulsion_dist")]
    pub max_repulsion_dist: Option<f32>,
    #[serde(alias = "boundary_margin")]
    pub boundary_margin: Option<f32>,
    #[serde(alias = "boundary_force_strength")]
    pub boundary_force_strength: Option<f32>,
    #[serde(alias = "warmup_iterations")]
    pub warmup_iterations: Option<u32>,
    #[serde(alias = "warmup_curve")]
    pub warmup_curve: Option<String>,
    #[serde(alias = "zero_velocity_iterations")]
    pub zero_velocity_iterations: Option<u32>,
    #[serde(alias = "cooling_rate")]
    pub cooling_rate: Option<f32>,
    
    #[serde(alias = "clustering_algorithm")]
    pub clustering_algorithm: Option<String>,
    #[serde(alias = "cluster_count")]
    pub cluster_count: Option<u32>,
    #[serde(alias = "clustering_resolution")]
    pub clustering_resolution: Option<f32>,
    #[serde(alias = "clustering_iterations")]
    pub clustering_iterations: Option<u32>,
    
    #[serde(alias = "repulsion_softening_epsilon")]
    pub repulsion_softening_epsilon: Option<f32>,
    #[serde(alias = "center_gravity_k")]
    pub center_gravity_k: Option<f32>,
    #[serde(alias = "grid_cell_size")]
    pub grid_cell_size: Option<f32>,
    #[serde(alias = "rest_length")]
    pub rest_length: Option<f32>,
}

// User preferences configuration
#[derive(Debug, Clone, Deserialize, Serialize, Type, Validate, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    #[serde(default)]
    pub comfort_level: Option<f32>, 
    #[serde(default)]
    pub interaction_style: Option<String>, 
    #[serde(default)]
    pub ar_preference: Option<bool>, 
    #[serde(default)]
    pub theme: Option<String>, 
    #[serde(default)]
    pub language: Option<String>, 
}

// Feature flags for experimental or optional features
#[derive(Debug, Clone, Deserialize, Serialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlags {
    #[serde(default)]
    pub gpu_clustering: bool,
    #[serde(default)]
    pub ontology_validation: bool,
    #[serde(default)]
    pub gpu_anomaly_detection: bool,
    #[serde(default)]
    pub real_time_insights: bool,
    #[serde(default)]
    pub advanced_visualizations: bool,
    #[serde(default)]
    pub performance_monitoring: bool,
    #[serde(default)]
    pub stress_majorization: bool,
    #[serde(default)]
    pub semantic_constraints: bool,
    #[serde(default)]
    pub sssp_integration: bool,
}

// Developer and debugging configuration
#[derive(Debug, Clone, Deserialize, Serialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperConfig {
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub show_performance_stats: bool,
    #[serde(default)]
    pub enable_profiling: bool,
    #[serde(default)]
    pub verbose_logging: bool,
    #[serde(default)]
    pub dev_tools_enabled: bool,
}

// Single unified settings struct
#[derive(Debug, Clone, Deserialize, Serialize, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AppFullSettings {
    #[validate(nested)]
    #[serde(alias = "visualisation")]
    pub visualisation: VisualisationSettings,
    #[validate(nested)]
    #[serde(alias = "system")]
    pub system: SystemSettings,
    #[validate(nested)]
    #[serde(alias = "xr")]
    pub xr: XRSettings,
    #[validate(nested)]
    #[serde(alias = "auth")]
    pub auth: AuthSettings,
    #[serde(skip_serializing_if = "Option::is_none", alias = "ragflow")]
    pub ragflow: Option<RagFlowSettings>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "perplexity")]
    pub perplexity: Option<PerplexitySettings>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "openai")]
    pub openai: Option<OpenAISettings>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "kokoro")]
    pub kokoro: Option<KokoroSettings>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "whisper")]
    pub whisper: Option<WhisperSettings>,
    #[serde(default = "default_version", alias = "version")]
    pub version: String,
    
    #[serde(default, alias = "user_preferences")]
    #[validate(nested)]
    pub user_preferences: UserPreferences,
    #[serde(default, alias = "physics")]
    #[validate(nested)]
    pub physics: PhysicsSettings,
    #[serde(default, alias = "feature_flags")]
    pub feature_flags: FeatureFlags,
    #[serde(default, alias = "developer_config")]
    pub developer_config: DeveloperConfig,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl Default for AppFullSettings {
    fn default() -> Self {
        Self {
            visualisation: VisualisationSettings::default(),
            system: SystemSettings::default(),
            xr: XRSettings::default(),
            auth: AuthSettings::default(),
            ragflow: None,
            perplexity: None,
            openai: None,
            kokoro: None,
            whisper: None,
            version: default_version(),
            user_preferences: UserPreferences::default(),
            physics: PhysicsSettings::default(),
            feature_flags: FeatureFlags::default(),
            developer_config: DeveloperConfig::default(),
        }
    }
}

impl AppFullSettings {
    
    
    pub fn new() -> Result<Self, ConfigError> {
        debug!("Initializing AppFullSettings with defaults (database-first architecture)");
        info!("IMPORTANT: Settings should be loaded from database via DatabaseService");
        info!("Legacy YAML file loading has been removed - all settings are now in Neo4j");


        Ok(Self::default())
    }



    pub fn save(&self) -> Result<(), String> {
        debug!("save() called but ignored - settings are now automatically persisted to database");
        info!("Legacy YAML file saving has been removed - all settings are now in Neo4j");
        Ok(())
    }

    
    
    
    
    
    pub fn get_physics(&self, graph: &str) -> &PhysicsSettings {
        match graph {
            "logseq" | "knowledge" => &self.visualisation.graphs.logseq.physics,
            "visionflow" | "agent" | "bots" => &self.visualisation.graphs.visionflow.physics,
            _ => {
                log::debug!(
                    "Unknown graph type '{}', defaulting to logseq (knowledge graph)",
                    graph
                );
                &self.visualisation.graphs.logseq.physics
            }
        }
    }

    
    

    
    pub fn merge_update(&mut self, update: serde_json::Value) -> Result<(), String> {
        
        if crate::utils::logging::is_debug_enabled() {
            debug!(
                "merge_update: Incoming update (camelCase): {}",
                crate::utils::json::to_json_pretty(&update)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }

        
        let processed_update = convert_empty_strings_to_null(update.clone());
        if crate::utils::logging::is_debug_enabled() {
            debug!(
                "merge_update: After null conversion: {}",
                crate::utils::json::to_json_pretty(&processed_update)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }

        
        
        let current_value = serde_json::to_value(&self)
            .map_err(|e| format!("Failed to serialize current settings: {}", e))?;

        
        let normalized_current = normalize_field_names_to_camel_case(current_value)?;
        let normalized_update = normalize_field_names_to_camel_case(processed_update)?;

        if crate::utils::logging::is_debug_enabled() {
            debug!(
                "merge_update: After field normalization (current): {}",
                crate::utils::json::to_json_pretty(&normalized_current)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
            debug!(
                "merge_update: After field normalization (update): {}",
                crate::utils::json::to_json_pretty(&normalized_update)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }

        let merged = merge_json_values(normalized_current, normalized_update);
        if crate::utils::logging::is_debug_enabled() {
            debug!(
                "merge_update: After merge: {}",
                crate::utils::json::to_json_pretty(&merged)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }

        
        *self = serde_json::from_value(merged.clone()).map_err(|e| {
            if crate::utils::logging::is_debug_enabled() {
                error!(
                    "merge_update: Failed to deserialize merged JSON: {}",
                    crate::utils::json::to_json_pretty(&merged)
                        .unwrap_or_else(|_| "Could not serialize".to_string())
                );
                error!(
                    "merge_update: Original update was: {}",
                    crate::utils::json::to_json_pretty(&update)
                        .unwrap_or_else(|_| "Could not serialize".to_string())
                );
            }
            format!("Failed to deserialize merged settings: {}", e)
        })?;

        Ok(())
    }

    
    pub fn validate_config_camel_case(&self) -> Result<(), validator::ValidationErrors> {
        
        self.validate()?;

        
        self.validate_cross_field_constraints()?;

        Ok(())
    }

    
    fn validate_cross_field_constraints(&self) -> Result<(), validator::ValidationErrors> {
        let mut errors = validator::ValidationErrors::new();

        
        if self.visualisation.graphs.logseq.physics.gravity != 0.0
            && !self.visualisation.graphs.logseq.physics.enabled
        {
            errors.add("physics", ValidationError::new("physics_enabled_required"));
        }

        
        if let Err(validation_error) =
            validate_bloom_glow_settings(&self.visualisation.glow, &self.visualisation.bloom)
        {
            errors.add("visualisation.bloom_glow", validation_error);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    
    pub fn get_validation_errors_camel_case(
        errors: &validator::ValidationErrors,
    ) -> HashMap<String, Vec<String>> {
        let mut result = HashMap::new();

        for (field, field_errors) in errors.field_errors() {
            let camel_case_field = to_camel_case(field);
            let messages: Vec<String> = field_errors
                .iter()
                .map(|error| match error.code.as_ref() {
                    "invalid_hex_color" => {
                        "Must be a valid hex color (#RRGGBB or #RRGGBBAA)".to_string()
                    }
                    "width_range_length" => "Width range must have exactly 2 values".to_string(),
                    "width_range_order" => {
                        "Width range minimum must be less than maximum".to_string()
                    }
                    "invalid_port" => "Port must be between 1 and 65535".to_string(),
                    "invalid_percentage" => "Value must be between 0 and 100".to_string(),
                    "physics_enabled_required" => {
                        "Physics must be enabled when gravity is configured".to_string()
                    }
                    _ => format!("Invalid value for {}", camel_case_field),
                })
                .collect();

            result.insert(camel_case_field, messages);
        }

        result
    }
}

// PathAccessible implementation for AppFullSettings
impl PathAccessible for AppFullSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "visualisation" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.visualisation.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.visualisation.get_by_path(&remaining)
                }
            }
            "system" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.system.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.system.get_by_path(&remaining)
                }
            }
            "xr" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.xr.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.xr.get_by_path(&remaining)
                }
            }
            "auth" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.auth.clone()))
                } else {
                    Err("Auth fields are not deeply accessible".to_string())
                }
            }
            _ => Err(format!("Unknown top-level field: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "visualisation" => {
                if segments.len() == 1 {
                    match value.downcast::<VisualisationSettings>() {
                        Ok(v) => {
                            self.visualisation = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for visualisation field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.visualisation.set_by_path(&remaining, value)
                }
            }
            "system" => {
                if segments.len() == 1 {
                    match value.downcast::<SystemSettings>() {
                        Ok(v) => {
                            self.system = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for system field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.system.set_by_path(&remaining, value)
                }
            }
            "xr" => {
                if segments.len() == 1 {
                    match value.downcast::<XRSettings>() {
                        Ok(v) => {
                            self.xr = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for xr field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.xr.set_by_path(&remaining, value)
                }
            }
            "auth" => {
                if segments.len() == 1 {
                    match value.downcast::<AuthSettings>() {
                        Ok(v) => {
                            self.auth = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for auth field".to_string()),
                    }
                } else {
                    Err("Auth nested fields are not modifiable".to_string())
                }
            }
            _ => Err(format!("Unknown top-level field: {}", segments[0])),
        }
    }
}

// Basic PathAccessible implementations for nested structures
impl PathAccessible for VisualisationSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "graphs" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.graphs.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.graphs.get_by_path(&remaining)
                }
            }
            _ => Err(format!(
                "Only graphs field is currently supported: {}",
                segments[0]
            )),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "graphs" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphsSettings>() {
                        Ok(v) => {
                            self.graphs = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for graphs field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.graphs.set_by_path(&remaining, value)
                }
            }
            _ => Err("Only graphs field is currently supported for modification".to_string()),
        }
    }
}

impl PathAccessible for GraphsSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "logseq" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.logseq.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.logseq.get_by_path(&remaining)
                }
            }
            "visionflow" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.visionflow.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.visionflow.get_by_path(&remaining)
                }
            }
            _ => Err(format!("Unknown graph type: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "logseq" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphSettings>() {
                        Ok(v) => {
                            self.logseq = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for logseq field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.logseq.set_by_path(&remaining, value)
                }
            }
            "visionflow" => {
                if segments.len() == 1 {
                    match value.downcast::<GraphSettings>() {
                        Ok(v) => {
                            self.visionflow = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for visionflow field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.visionflow.set_by_path(&remaining, value)
                }
            }
            _ => Err(format!("Unknown graph type: {}", segments[0])),
        }
    }
}

impl PathAccessible for GraphSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "physics" => {
                if segments.len() == 1 {
                    Ok(Box::new(self.physics.clone()))
                } else {
                    let remaining = segments[1..].join(".");
                    self.physics.get_by_path(&remaining)
                }
            }
            _ => Err(format!(
                "Only physics is supported currently: {}",
                segments[0]
            )),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "physics" => {
                if segments.len() == 1 {
                    match value.downcast::<PhysicsSettings>() {
                        Ok(v) => {
                            self.physics = *v;
                            Ok(())
                        }
                        Err(_) => Err("Type mismatch for physics field".to_string()),
                    }
                } else {
                    let remaining = segments[1..].join(".");
                    self.physics.set_by_path(&remaining, value)
                }
            }
            _ => Err("Only physics field is currently supported for modification".to_string()),
        }
    }
}

// Critical: PhysicsSettings PathAccessible implementation for performance fix
impl PathAccessible for PhysicsSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "damping" => Ok(Box::new(self.damping)),
            "springK" => Ok(Box::new(self.spring_k)),
            "repelK" => Ok(Box::new(self.repel_k)),
            "enabled" => Ok(Box::new(self.enabled)),
            "iterations" => Ok(Box::new(self.iterations)),
            "maxVelocity" => Ok(Box::new(self.max_velocity)),
            "boundsSize" => Ok(Box::new(self.bounds_size)),
            "gravity" => Ok(Box::new(self.gravity)),
            "temperature" => Ok(Box::new(self.temperature)),
            _ => Err(format!("Unknown physics field: {}", segments[0])),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let segments = parse_path(path)?;

        match segments[0] {
            "damping" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.damping = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for damping field".to_string()),
            },
            "springK" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.spring_k = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for springK field".to_string()),
            },
            "repelK" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.repel_k = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for repelK field".to_string()),
            },
            "enabled" => match value.downcast::<bool>() {
                Ok(v) => {
                    self.enabled = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for enabled field".to_string()),
            },
            "iterations" => match value.downcast::<u32>() {
                Ok(v) => {
                    self.iterations = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for iterations field".to_string()),
            },
            "maxVelocity" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.max_velocity = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for maxVelocity field".to_string()),
            },
            "boundsSize" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.bounds_size = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for boundsSize field".to_string()),
            },
            "gravity" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.gravity = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for gravity field".to_string()),
            },
            "temperature" => match value.downcast::<f32>() {
                Ok(v) => {
                    self.temperature = *v;
                    Ok(())
                }
                Err(_) => Err("Type mismatch for temperature field".to_string()),
            },
            _ => Err(format!("Unknown physics field: {}", segments[0])),
        }
    }
}

// Implementation for SystemSettings path access
impl PathAccessible for SystemSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        match path {
            "network" => Ok(Box::new(self.network.clone())),
            "websocket" => Ok(Box::new(self.websocket.clone())),
            "security" => Ok(Box::new(self.security.clone())),
            "debug" => Ok(Box::new(self.debug.clone())),
            "persist_settings" => Ok(Box::new(self.persist_settings)),
            "custom_backend_url" => Ok(Box::new(self.custom_backend_url.clone())),
            _ => Err(format!("Unknown SystemSettings field: {}", path)),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        match path {
            "persist_settings" => {
                if let Some(val) = value.downcast_ref::<bool>() {
                    self.persist_settings = *val;
                    Ok(())
                } else {
                    Err("Invalid type for persist_settings, expected bool".to_string())
                }
            }
            "custom_backend_url" => {
                if let Some(val) = value.downcast_ref::<Option<String>>() {
                    self.custom_backend_url = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for custom_backend_url, expected Option<String>".to_string())
                }
            }
            _ => Err(format!("Setting {} not supported for SystemSettings", path)),
        }
    }
}

impl PathAccessible for XRSettings {
    fn get_by_path(&self, path: &str) -> Result<Box<dyn std::any::Any>, String> {
        match path {
            "enabled" => Ok(Box::new(self.enabled.clone())),
            "client_side_enable_xr" => Ok(Box::new(self.client_side_enable_xr.clone())),
            "mode" => Ok(Box::new(self.mode.clone())),
            "room_scale" => Ok(Box::new(self.room_scale)),
            "space_type" => Ok(Box::new(self.space_type.clone())),
            "quality" => Ok(Box::new(self.quality.clone())),
            "render_scale" => Ok(Box::new(self.render_scale.clone())),
            "interaction_distance" => Ok(Box::new(self.interaction_distance)),
            "locomotion_method" => Ok(Box::new(self.locomotion_method.clone())),
            "teleport_ray_color" => Ok(Box::new(self.teleport_ray_color.clone())),
            "controller_ray_color" => Ok(Box::new(self.controller_ray_color.clone())),
            _ => Err(format!("Unknown XRSettings field: {}", path)),
        }
    }

    fn set_by_path(&mut self, path: &str, value: Box<dyn std::any::Any>) -> Result<(), String> {
        match path {
            "enabled" => {
                if let Some(val) = value.downcast_ref::<Option<bool>>() {
                    self.enabled = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for enabled, expected Option<bool>".to_string())
                }
            }
            "room_scale" => {
                if let Some(val) = value.downcast_ref::<f32>() {
                    self.room_scale = *val;
                    Ok(())
                } else {
                    Err("Invalid type for room_scale, expected f32".to_string())
                }
            }
            "space_type" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.space_type = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for space_type, expected String".to_string())
                }
            }
            "quality" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.quality = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for quality, expected String".to_string())
                }
            }
            "interaction_distance" => {
                if let Some(val) = value.downcast_ref::<f32>() {
                    self.interaction_distance = *val;
                    Ok(())
                } else {
                    Err("Invalid type for interaction_distance, expected f32".to_string())
                }
            }
            "locomotion_method" => {
                if let Some(val) = value.downcast_ref::<String>() {
                    self.locomotion_method = val.clone();
                    Ok(())
                } else {
                    Err("Invalid type for locomotion_method, expected String".to_string())
                }
            }
            _ => Err(format!("Setting {} not supported for XRSettings", path)),
        }
    }
}
