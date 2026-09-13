use chrono::Utc;
use regex::Regex;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating TypeScript types from Rust structs...");

    let typescript_interfaces = generate_typescript_interfaces();

    let header = format!(
        "// Auto-generated TypeScript type definitions\n// Generated: {}\n\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let camel_case_code = convert_to_camel_case(format!("{}{}", header, typescript_interfaces));

    let output_path = Path::new("client/src/types/generated/settings.ts");
    let output_dir = output_path
        .parent()
        .expect("output_path has a known parent directory");
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
        println!("📁 Created output directory: {}", output_dir.display());
    }

    fs::write(output_path, &camel_case_code)?;

    println!(
        "Successfully generated TypeScript types at: {}",
        output_path.display()
    );

    let metadata = fs::metadata(output_path)?;
    println!("Generated file size: {} bytes", metadata.len());

    if metadata.len() > 1000 {
        println!("Type generation completed successfully!");

        let preview: String = camel_case_code
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        println!("📋 Preview of generated types:");
        println!("{}", preview);
        if camel_case_code.lines().count() > 20 {
            println!("... ({} more lines)", camel_case_code.lines().count() - 20);
        }
    } else {
        println!("⚠️  Generated file seems small, please verify content");
    }

    println!("📦 Type generation complete! Run `npm run build` in client to use new types.");

    Ok(())
}

fn generate_typescript_interfaces() -> String {
    r#"
export interface MovementAxes {
  horizontal: number;
  vertical: number;
}

// Position coordinates
export interface Position {
  x: number;
  y: number;
  z: number;
}

// Sensitivity controls
export interface Sensitivity {
  translation: number;
  rotation: number;
}

// Node rendering settings
export interface NodeSettings {
  base_color: string;
  metalness: number;
  opacity: number;
  roughness: number;
  node_size: number;
  quality: string;
  enable_instancing: boolean;
  enable_hologram: boolean;
  enable_metadata_shape: boolean;
  enable_metadata_visualisation: boolean;
}

// Edge rendering settings
export interface EdgeSettings {
  arrow_size: number;
  base_width: number;
  color: string;
  enable_arrows: boolean;
  opacity: number;
  width_range: number[];
  quality: string;
}

// Auto-balance configuration
export interface AutoBalanceConfig {
  stability_variance_threshold: number;
  stability_frame_count: number;
  clustering_distance_threshold: number;
  bouncing_node_percentage: number;
  boundary_min_distance: number;
  boundary_max_distance: number;
  extreme_distance_threshold: number;
  explosion_distance_threshold: number;
  spreading_distance_threshold: number;
  oscillation_detection_frames: number;
  oscillation_change_threshold: number;
  min_oscillation_changes: number;
  grid_cell_size_min: number;
  grid_cell_size_max: number;
  repulsion_cutoff_min: number;
  repulsion_cutoff_max: number;
  repulsion_softening_min: number;
  repulsion_softening_max: number;
  center_gravity_min: number;
  center_gravity_max: number;
  spatial_hash_efficiency_threshold: number;
  cluster_density_threshold: number;
  numerical_instability_threshold: number;
}

// Physics simulation settings
export interface PhysicsSettings {
  auto_balance: boolean;
  auto_balance_interval_ms: number;
  auto_balance_config: AutoBalanceConfig;
  bounds_size: number;
  separation_radius: number;
  damping: number;
  enable_bounds: boolean;
  enabled: boolean;
  iterations: number;
  max_velocity: number;
  max_force: number;
  repel_k: number;
  spring_k: number;
  boundary_damping: number;
  dt: number;
  temperature: number;
  gravity: number;
  cluster_strength: number;
  rest_length: number;
  repulsion_softening_epsilon: number;
  center_gravity_k: number;
  grid_cell_size: number;
  warmup_iterations: number;
  cooling_rate: number;
  max_repulsion_dist: number;
  sssp_alpha: number;
  clustering_algorithm: string;
  cluster_count: number;
  clustering_resolution: number;
  clustering_iterations: number;
}

// Rendering settings
export interface RenderingSettings {
  ambient_light_intensity: number;
  background_color: string;
  directional_light_intensity: number;
  enable_ambient_occlusion: boolean;
  enable_antialiasing: boolean;
  enable_shadows: boolean;
  environment_intensity: number;
  shadow_map_size?: string;
  shadow_bias?: number;
  context?: string;
}

// Animation settings
export interface AnimationSettings {
  enable_motion_blur: boolean;
  enable_node_animations: boolean;
  motion_blur_strength: number;
  selection_wave_enabled: boolean;
  pulse_enabled: boolean;
  pulse_speed: number;
  pulse_strength: number;
  wave_speed: number;
}

// Label settings
export interface LabelSettings {
  desktop_font_size: number;
  enable_labels: boolean;
  text_color: string;
  text_outline_color: string;
  text_outline_width: number;
  text_resolution: number;
  text_padding: number;
  billboard_mode: string;
  show_metadata?: boolean;
  max_label_width?: number;
}

// Glow effect settings
export interface GlowSettings {
  enabled: boolean;
  intensity: number;
  radius: number;
  threshold: number;
  diffuse_strength: number;
  atmospheric_density: number;
  volumetric_intensity: number;
  base_color: string;
  emission_color: string;
  opacity: number;
  pulse_speed: number;
  flow_speed: number;
  node_glow_strength: number;
  edge_glow_strength: number;
  environment_glow_strength: number;
}

// Hologram settings
export interface HologramSettings {
  ring_count: number;
  ring_color: string;
  ring_opacity: number;
  sphere_sizes: number[];
  ring_rotation_speed: number;
  enable_buckminster: boolean;
  buckminster_size: number;
  buckminster_opacity: number;
  enable_geodesic: boolean;
  geodesic_size: number;
  geodesic_opacity: number;
  enable_triangle_sphere: boolean;
  triangle_sphere_size: number;
  triangle_sphere_opacity: number;
  global_rotation_speed: number;
}

// Camera settings
export interface CameraSettings {
  fov: number;
  near: number;
  far: number;
  position: Position;
  look_at: Position;
}

// SpacePilot settings
export interface SpacePilotSettings {
  enabled: boolean;
  mode: string;
  sensitivity: Sensitivity;
  smoothing: number;
  deadzone: number;
  button_functions: Record<string, string>;
}

// Graph settings
export interface GraphSettings {
  nodes: NodeSettings;
  edges: EdgeSettings;
  labels: LabelSettings;
  physics: PhysicsSettings;
}

// Multi-graph settings
export interface GraphsSettings {
  knowledge: GraphSettings;
  visionclaw: GraphSettings;
}

// Visualization settings
export interface VisualisationSettings {
  rendering: RenderingSettings;
  animations: AnimationSettings;
  glow: GlowSettings;
  hologram: HologramSettings;
  graphs: GraphsSettings;
  camera?: CameraSettings;
  space_pilot?: SpacePilotSettings;
}

// Network settings
export interface NetworkSettings {
  bind_address: string;
  domain: string;
  enable_http2: boolean;
  enable_rate_limiting: boolean;
  enable_tls: boolean;
  max_request_size: number;
  min_tls_version: string;
  port: number;
  rate_limit_requests: number;
  rate_limit_window: number;
  tunnel_id: string;
  api_client_timeout: number;
  enable_metrics: boolean;
  max_concurrent_requests: number;
  max_retries: number;
  metrics_port: number;
  retry_delay: number;
}

// WebSocket settings
export interface WebSocketSettings {
  binary_chunk_size: number;
  binary_update_rate: number;
  min_update_rate: number;
  max_update_rate: number;
  motion_threshold: number;
  motion_damping: number;
  binary_message_version: number;
  compression_enabled: boolean;
  compression_threshold: number;
  heartbeat_interval: number;
  heartbeat_timeout: number;
  max_connections: number;
  max_message_size: number;
  reconnect_attempts: number;
  reconnect_delay: number;
  update_rate: number;
}

// Security settings
export interface SecuritySettings {
  allowed_origins: string[];
  audit_log_path: string;
  cookie_httponly: boolean;
  cookie_samesite: string;
  cookie_secure: boolean;
  csrf_token_timeout: number;
  enable_audit_logging: boolean;
  enable_request_validation: boolean;
  session_timeout: number;
}

// Debug settings
export interface DebugSettings {
  enabled: boolean;
}

// System settings
export interface SystemSettings {
  network: NetworkSettings;
  websocket: WebSocketSettings;
  security: SecuritySettings;
  debug: DebugSettings;
  persist_settings: boolean;
  custom_backend_url?: string;
}

// XR settings
export interface XRSettings {
  enabled?: boolean;
  client_side_enable_xr?: boolean;
  mode?: string;
  room_scale: number;
  space_type: string;
  quality: string;
  render_scale?: number;
  interaction_distance: number;
  locomotion_method: string;
  teleport_ray_color: string;
  controller_ray_color: string;
  controller_model?: string;
  enable_hand_tracking: boolean;
  hand_mesh_enabled: boolean;
  hand_mesh_color: string;
  hand_mesh_opacity: number;
  hand_point_size: number;
  hand_ray_enabled: boolean;
  hand_ray_color: string;
  hand_ray_width: number;
  gesture_smoothing: number;
  enable_haptics: boolean;
  haptic_intensity: number;
  drag_threshold: number;
  pinch_threshold: number;
  rotation_threshold: number;
  interaction_radius: number;
  movement_speed: number;
  dead_zone: number;
  movement_axes: MovementAxes;
  enable_light_estimation: boolean;
  enable_plane_detection: boolean;
  enable_scene_understanding: boolean;
  plane_color: string;
  plane_opacity: number;
  plane_detection_distance: number;
  show_plane_overlay: boolean;
  snap_to_floor: boolean;
  enable_passthrough_portal: boolean;
  passthrough_opacity: number;
  passthrough_brightness: number;
  passthrough_contrast: number;
  portal_size: number;
  portal_edge_color: string;
  portal_edge_width: number;
}

// Authentication settings
export interface AuthSettings {
  enabled: boolean;
  provider: string;
  required: boolean;
}

// External service settings
export interface RagFlowSettings {
  api_key?: string;
  agent_id?: string;
  api_base_url?: string;
  timeout?: number;
  max_retries?: number;
  chat_id?: string;
}

export interface PerplexitySettings {
  api_key?: string;
  model?: string;
  api_url?: string;
  max_tokens?: number;
  temperature?: number;
  top_p?: number;
  presence_penalty?: number;
  frequency_penalty?: number;
  timeout?: number;
  rate_limit?: number;
}

export interface OpenAISettings {
  api_key?: string;
  base_url?: string;
  timeout?: number;
  rate_limit?: number;
}

export interface PocketTtsSettings {
  api_url?: string;
  default_voice?: string;
  default_format?: string;
  default_speed?: number;
  timeout?: number;
  stream?: boolean;
  return_timestamps?: boolean;
  sample_rate?: number;
}

export interface WhisperSettings {
  api_url?: string;
  default_model?: string;
  default_language?: string;
  timeout?: number;
  temperature?: number;
  return_timestamps?: boolean;
  vad_filter?: boolean;
  word_timestamps?: boolean;
  initial_prompt?: string;
}

// Constraint system
export interface ConstraintData {
  constraint_type: number;
  strength: number;
  param1: number;
  param2: number;
  node_mask: number;
  enabled: boolean;
}

export interface ConstraintSystem {
  separation: ConstraintData;
  boundary: ConstraintData;
  alignment: ConstraintData;
  cluster: ConstraintData;
}

// Clustering configuration
export interface ClusteringConfiguration {
  algorithm: string;
  num_clusters: number;
  resolution: number;
  iterations: number;
  export_assignments: boolean;
  auto_update: boolean;
}

// Physics update helper
export interface PhysicsUpdate {
  damping?: number;
  spring_k?: number;
  repel_k?: number;
  iterations?: number;
  enabled?: boolean;
  bounds_size?: number;
  enable_bounds?: boolean;
  max_velocity?: number;
  max_force?: number;
  separation_radius?: number;
  boundary_damping?: number;
  dt?: number;
  temperature?: number;
  gravity?: number;
  cluster_strength?: number;
  sssp_alpha?: number;
  max_repulsion_dist?: number;
  warmup_iterations?: number;
  cooling_rate?: number;
  clustering_algorithm?: string;
  cluster_count?: number;
  clustering_resolution?: number;
  clustering_iterations?: number;
  repulsion_softening_epsilon?: number;
  center_gravity_k?: number;
  grid_cell_size?: number;
  rest_length?: number;
}

// Main application settings
export interface AppFullSettings {
  visualisation: VisualisationSettings;
  system: SystemSettings;
  xr: XRSettings;
  auth: AuthSettings;
  ragflow?: RagFlowSettings;
  perplexity?: PerplexitySettings;
  openai?: OpenAISettings;
  pocketTts?: PocketTtsSettings;
  whisper?: WhisperSettings;
}

// Type aliases for convenience
export type Settings = AppFullSettings;
export type SettingsUpdate = Partial<Settings>;
export type SettingsPath = string;

// Type guards for runtime type checking
export function isAppFullSettings(obj: any): obj is AppFullSettings {
    return obj && typeof obj === 'object' &&
           'visualisation' in obj &&
           'system' in obj &&
           'xr' in obj &&
           'auth' in obj;
}

export function isPosition(obj: any): obj is Position {
    return obj && typeof obj === 'object' &&
           typeof obj.x === 'number' &&
           typeof obj.y === 'number' &&
           typeof obj.z === 'number';
}

// Partial update helpers
export type DeepPartial<T> = {
    [P in keyof T]?: T[P] extends object ? DeepPartial<T[P]> : T[P];
};

export type NestedSettings = DeepPartial<AppFullSettings>;

// Default export
export default AppFullSettings;
"#
    .to_string()
}

fn convert_to_camel_case(typescript_code: String) -> String {
    let field_regex =
        Regex::new(r"(\s+)([a-z][a-z0-9_]*[a-z0-9])(\s*:\s*)").expect("Invalid regex pattern");

    field_regex
        .replace_all(&typescript_code, |caps: &regex::Captures| {
            let indent = &caps[1];
            let field_name = &caps[2];
            let colon_and_space = &caps[3];

            let camel_case = snake_to_camel_case(field_name);

            format!("{}{}{}", indent, camel_case, colon_and_space)
        })
        .to_string()
}

fn snake_to_camel_case(snake_str: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for (i, c) in snake_str.chars().enumerate() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next && i > 0 {
            result.push(c.to_uppercase().next().unwrap_or(c));
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    // NOTE: `use crate::utils::time;` removed - module doesn't exist in bin crate

    #[test]
    fn test_snake_to_camel_case() {
        assert_eq!(snake_to_camel_case("hello_world"), "helloWorld");
        assert_eq!(
            snake_to_camel_case("auto_balance_interval_ms"),
            "autoBalanceIntervalMs"
        );
        assert_eq!(snake_to_camel_case("enable_bounds"), "enableBounds");
        assert_eq!(snake_to_camel_case("api_key"), "apiKey");
        assert_eq!(snake_to_camel_case("simple"), "simple");
        assert_eq!(snake_to_camel_case("a_b_c_d"), "aBCD");
    }

    #[test]
    fn test_convert_to_camel_case() {
        let typescript_input = r#"
export interface TestInterface {
  field_one: string;
  another_field: number;
  simple: boolean;
}
"#;

        let expected = r#"
export interface TestInterface {
  fieldOne: string;
  anotherField: number;
  simple: boolean;
}
"#;

        let result = convert_to_camel_case(typescript_input.to_string());
        assert_eq!(result, expected);
    }
}
