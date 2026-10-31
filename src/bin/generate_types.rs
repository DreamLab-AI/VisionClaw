use chrono::Utc;
use regex::Regex;
use std::fs;
use std::path::Path;
use visionclaw_ontology::types::ontology_tools::{
    Closure, Entailment, FactBasis, RelationConstraint,
};

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

    write_ontology_agent_types()?;

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
  sssp_enabled: boolean;
  /** Stored layout mode, ADR-2140 (e.g. "forceDirected", "dagLeftRight"). */
  layout_mode: string;
  clustering_algorithm: string;
  cluster_count: number;
  clustering_resolution: number;
  clustering_iterations: number;
  /** Most nodes in one position chunk, 100-20000 (default 2500). */
  stream_chunk_nodes: number;
  /** Re-send a node once it is this far from what the client acknowledged, 0-100 (default 0.25). */
  stream_move_threshold: number;
  /** Milliseconds before an unacknowledged chunk is rebuilt and resent, 20-10000 (default 500). */
  stream_ack_timeout_ms: number;
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
  /** Server ping interval, ms (min 1000). */
  heartbeat_interval: number;
  /** Close after this many ms with no inbound frame (min 2 intervals). */
  heartbeat_timeout: number;
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

/// Variants of the ontology-agent enums, in declaration order. Keep these in
/// step with the enums by hand: `exhaustive` below forces an edit when a
/// variant is added, but cannot force the array entry (stable Rust has no
/// variant enumeration and `strum` is not a dependency), so review catches it.
const FACT_BASES: [FactBasis; 3] = [
    FactBasis::Asserted,
    FactBasis::Inferred,
    FactBasis::Provenance,
];
const ENTAILMENTS: [Entailment; 3] = [
    Entailment::Entailed,
    Entailment::EntailedFalse,
    Entailment::NotAsserted,
];
const CLOSURES: [Closure; 1] = [Closure::Open];
const CONSTRAINTS: [RelationConstraint; 2] =
    [RelationConstraint::Domain, RelationConstraint::Range];

/// Serde wire name of a unit-variant enum value, quoted for a TS union.
fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("unit enum variants serialise")
}

fn union_of<T: serde::Serialize>(variants: &[T]) -> String {
    variants.iter().map(wire).collect::<Vec<_>>().join(" | ")
}

/// Compile-time exhaustiveness guards for the variant tables above.
#[allow(dead_code)]
fn exhaustive(a: FactBasis, b: Entailment, c: Closure, d: RelationConstraint) {
    match a {
        FactBasis::Asserted | FactBasis::Inferred | FactBasis::Provenance => {}
    }
    match b {
        Entailment::Entailed | Entailment::EntailedFalse | Entailment::NotAsserted => {}
    }
    match c {
        Closure::Open => {}
    }
    match d {
        RelationConstraint::Domain | RelationConstraint::Range => {}
    }
}

/// TypeScript for the ontology-agent answer types (ADR-2127 decision 4).
///
/// Unlike `settings.ts` this file keeps the **wire** (snake_case) field names:
/// the agent surface serialises with serde defaults and the client reads those
/// fields verbatim, so the camelCase pass must not run over it.
fn generate_ontology_agent_types() -> String {
    format!(
        r#"// Auto-generated by `cargo run --bin generate_types` -- do not edit.
// Source: crates/visionclaw-ontology/src/types/ontology_tools.rs (ADR-2127).
// Field names are the serde wire names (snake_case); no camelCase pass.

/** Why a returned fact holds. */
export type FactBasis = {fact_basis};

/** Tri-valued membership/relation answer. `not_asserted` is NOT `entailed_false`. */
export type Entailment = {entailment};

/** World assumption of a result. Only `open` exists. */
export type Closure = {closure};

/** Which declared signature an entailed-false relation contradicts. */
export type RelationConstraint = {constraint};

/** What a result (empty or not) was computed against. */
export interface AnswerScope {{
  closure: Closure;
  generation: string | null;
}}

export interface DisjointnessWitness {{
  subject_side: string;
  class_side: string;
}}

export interface MembershipCheckInput {{
  subject: string;
  class: string;
}}

export interface MembershipCheck {{
  subject: string;
  class: string;
  verdict: Entailment;
  basis: FactBasis | null;
  witness: DisjointnessWitness | null;
  scope: AnswerScope;
}}

export interface RelationCheckInput {{
  subject: string;
  property: string;
  object: string;
}}

export interface RelationWitness {{
  constraint: RelationConstraint;
  property: string;
  declared_class: string;
  disjointness: DisjointnessWitness;
}}

export interface RelationCheck {{
  subject: string;
  property: string;
  object: string;
  verdict: Entailment;
  basis: FactBasis | null;
  witness: RelationWitness | null;
  scope: AnswerScope;
}}

export interface RelationshipSummary {{
  rel_type: string;
  target_iri: string;
  target_term: string;
  basis: FactBasis;
}}

export interface DiscoveryResult {{
  iri: string;
  preferred_term: string;
  definition_summary: string;
  relevance_score: number;
  quality_score: number;
  domain: string;
  relationships: RelationshipSummary[];
  whelk_inferred: boolean;
  basis: FactBasis;
}}

export interface InferredAxiomSummary {{
  axiom_type: string;
  subject: string;
  object: string;
  is_inferred: boolean;
  basis: FactBasis;
}}

export interface RelatedNote {{
  iri: string;
  preferred_term: string;
  relationship_type: string;
  direction: string;
  summary: string;
  basis: FactBasis;
}}

export interface QueryRow {{
  values: Record<string, unknown>;
  basis: FactBasis;
}}

export interface QueryResult {{
  columns: string[];
  rows: QueryRow[];
  row_count: number;
  scope: AnswerScope;
}}

export interface TraversalNode {{
  iri: string;
  preferred_term: string;
  domain: string;
  depth: number;
}}

export interface TraversalEdge {{
  source_iri: string;
  target_iri: string;
  relationship_type: string;
  basis: FactBasis;
}}

export interface TraversalResult {{
  start_iri: string;
  nodes: TraversalNode[];
  edges: TraversalEdge[];
  scope: AnswerScope;
}}

/** An empty result is silence at `scope.generation`, never a negative answer. */
export function isEntailedFalse(v: Entailment): v is "entailed_false" {{
  return v === "entailed_false";
}}
"#,
        fact_basis = union_of(&FACT_BASES),
        entailment = union_of(&ENTAILMENTS),
        closure = union_of(&CLOSURES),
        constraint = union_of(&CONSTRAINTS),
    )
}

fn write_ontology_agent_types() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("client/src/types/generated/ontology-agent.ts");
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, generate_ontology_agent_types())?;
    println!("Generated ontology-agent types at: {}", path.display());
    Ok(())
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

    /// Fields of `export interface <name>` in the emitted TypeScript.
    fn ts_fields(ts: &str, name: &str) -> std::collections::BTreeSet<String> {
        let head = format!("export interface {name} {{");
        let body = ts
            .split(&head)
            .nth(1)
            .unwrap_or_else(|| panic!("interface {name} not emitted"))
            .split("\n}")
            .next()
            .expect("interface body");
        body.lines()
            .filter_map(|l| l.trim().split(':').next().map(str::to_string))
            .filter(|f| !f.is_empty())
            .collect()
    }

    fn json_keys<T: serde::Serialize>(v: &T) -> std::collections::BTreeSet<String> {
        serde_json::to_value(v)
            .expect("serialise")
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn ontology_agent_unions_match_serde_wire_names() {
        let ts = generate_ontology_agent_types();
        assert!(ts.contains(
            r#"export type Entailment = "entailed" | "entailed_false" | "not_asserted";"#
        ));
        assert!(ts.contains(r#"export type FactBasis = "asserted" | "inferred" | "provenance";"#));
        assert!(ts.contains(r#"export type Closure = "open";"#));
        assert!(ts.contains(r#"export type RelationConstraint = "domain" | "range";"#));
    }

    #[test]
    fn ontology_agent_interfaces_match_serde_keys() {
        use std::collections::HashMap;
        use visionclaw_ontology::types::ontology_tools::*;
        let ts = generate_ontology_agent_types();
        let scope = AnswerScope::open(Some("g".into()));
        let dj = DisjointnessWitness {
            subject_side: "a".into(),
            class_side: "b".into(),
        };
        let cases: Vec<(&str, std::collections::BTreeSet<String>)> = vec![
            ("AnswerScope", json_keys(&scope)),
            ("DisjointnessWitness", json_keys(&dj)),
            (
                "MembershipCheckInput",
                json_keys(&MembershipCheckInput {
                    subject: "s".into(),
                    class: "c".into(),
                }),
            ),
            (
                "MembershipCheck",
                json_keys(&MembershipCheck {
                    subject: "s".into(),
                    class: "c".into(),
                    verdict: Entailment::NotAsserted,
                    basis: None,
                    witness: None,
                    scope: scope.clone(),
                }),
            ),
            (
                "RelationCheckInput",
                json_keys(&RelationCheckInput {
                    subject: "s".into(),
                    property: "p".into(),
                    object: "o".into(),
                }),
            ),
            (
                "RelationWitness",
                json_keys(&RelationWitness {
                    constraint: RelationConstraint::Domain,
                    property: "p".into(),
                    declared_class: "d".into(),
                    disjointness: dj.clone(),
                }),
            ),
            (
                "RelationCheck",
                json_keys(&RelationCheck {
                    subject: "s".into(),
                    property: "p".into(),
                    object: "o".into(),
                    verdict: Entailment::EntailedFalse,
                    basis: None,
                    witness: None,
                    scope: scope.clone(),
                }),
            ),
            (
                "RelationshipSummary",
                json_keys(&RelationshipSummary {
                    rel_type: "r".into(),
                    target_iri: "t".into(),
                    target_term: "t".into(),
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "DiscoveryResult",
                json_keys(&DiscoveryResult {
                    iri: "i".into(),
                    preferred_term: "p".into(),
                    definition_summary: "d".into(),
                    relevance_score: 0.0,
                    quality_score: 0.0,
                    domain: "d".into(),
                    relationships: vec![],
                    whelk_inferred: false,
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "InferredAxiomSummary",
                json_keys(&InferredAxiomSummary {
                    axiom_type: "a".into(),
                    subject: "s".into(),
                    object: "o".into(),
                    is_inferred: false,
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "RelatedNote",
                json_keys(&RelatedNote {
                    iri: "i".into(),
                    preferred_term: "p".into(),
                    relationship_type: "r".into(),
                    direction: "outgoing".into(),
                    summary: "s".into(),
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "QueryRow",
                json_keys(&QueryRow {
                    values: HashMap::new(),
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "QueryResult",
                json_keys(&QueryResult {
                    columns: vec![],
                    rows: vec![],
                    row_count: 0,
                    scope: scope.clone(),
                }),
            ),
            (
                "TraversalNode",
                json_keys(&TraversalNode {
                    iri: "i".into(),
                    preferred_term: "p".into(),
                    domain: "d".into(),
                    depth: 0,
                }),
            ),
            (
                "TraversalEdge",
                json_keys(&TraversalEdge {
                    source_iri: "s".into(),
                    target_iri: "t".into(),
                    relationship_type: "r".into(),
                    basis: FactBasis::Asserted,
                }),
            ),
            (
                "TraversalResult",
                json_keys(&TraversalResult {
                    start_iri: "s".into(),
                    nodes: vec![],
                    edges: vec![],
                    scope,
                }),
            ),
        ];
        for (name, keys) in cases {
            assert_eq!(
                ts_fields(&ts, name),
                keys,
                "interface {name} drifted from its serde keys"
            );
        }
    }

    #[test]
    fn ontology_agent_output_is_not_camel_cased() {
        let ts = generate_ontology_agent_types();
        assert!(ts.contains("declared_class: string;"));
        assert!(ts.contains("row_count: number;"));
    }
}
