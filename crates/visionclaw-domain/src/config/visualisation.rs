use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use validator::Validate;

use super::validation::{validate_hex_color, validate_width_range};
use crate::types::physics_config::PhysicsSettings;

fn default_glow_color() -> String {
    "#00ffff".to_string()
}

fn default_glow_opacity() -> f32 {
    0.8
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Type, Validate)]
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

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            base_color: "#202724".to_string(),
            metalness: 0.1,
            opacity: 0.88,
            roughness: 0.6,
            node_size: 1.7,
            quality: "high".to_string(),
            enable_instancing: true,
            enable_hologram: true,
            enable_metadata_shape: false,
            enable_metadata_visualisation: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Type, Validate)]
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

impl Default for EdgeSettings {
    fn default() -> Self {
        Self {
            arrow_size: 0.02,
            base_width: 0.61,
            color: "#ff0000".to_string(),
            enable_arrows: false,
            opacity: 0.5,
            width_range: vec![0.3, 1.5],
            quality: "high".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
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

impl Default for RenderingSettings {
    fn default() -> Self {
        Self {
            ambient_light_intensity: 0.5,
            background_color: "#000000".to_string(),
            directional_light_intensity: 0.4,
            enable_ambient_occlusion: false,
            enable_antialiasing: true,
            enable_shadows: false,
            environment_intensity: 0.3,
            shadow_map_size: None,
            shadow_bias: None,
            context: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
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

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enable_motion_blur: false,
            enable_node_animations: true,
            motion_blur_strength: 0.1,
            selection_wave_enabled: true,
            pulse_enabled: true,
            pulse_speed: 1.2,
            pulse_strength: 0.8,
            wave_speed: 0.5,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Type, Validate)]
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

impl Default for LabelSettings {
    fn default() -> Self {
        Self {
            desktop_font_size: 1.41,
            enable_labels: true,
            text_color: "#676565".to_string(),
            text_outline_color: "#00ff40".to_string(),
            text_outline_width: 0.007,
            text_resolution: 32,
            text_padding: 0.3,
            billboard_mode: "camera".to_string(),
            show_metadata: Some(true),
            max_label_width: Some(5.0),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
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

impl Default for GlowSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 1.2,
            radius: 1.2,
            threshold: 0.39,
            diffuse_strength: 1.5,
            atmospheric_density: 0.8,
            volumetric_intensity: 1.2,
            base_color: "#00ffff".to_string(),
            emission_color: "#00e5ff".to_string(),
            opacity: 0.6,
            pulse_speed: 1.2,
            flow_speed: 0.5,
            node_glow_strength: 0.7,
            edge_glow_strength: 0.5,
            environment_glow_strength: 0.96,
        }
    }
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

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
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

impl Default for HologramSettings {
    fn default() -> Self {
        Self {
            ring_count: 3,
            ring_color: "#f0e800".to_string(),
            ring_opacity: 0.2,
            sphere_sizes: vec![80.0, 180.0],
            ring_rotation_speed: 0.14,
            enable_buckminster: true,
            buckminster_size: 250.0,
            buckminster_opacity: 0.3,
            enable_geodesic: true,
            geodesic_size: 600.0,
            geodesic_opacity: 0.25,
            enable_triangle_sphere: true,
            triangle_sphere_size: 1000.0,
            triangle_sphere_opacity: 0.4,
            global_rotation_speed: 0.5,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
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

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            fov: 75.0,
            near: 0.1,
            far: 2000.0,
            position: Position {
                x: 0.0,
                y: 10.0,
                z: 50.0,
            },
            look_at: Position::default(),
        }
    }
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
    pub button_functions: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sensitivity {
    #[serde(alias = "translation")]
    pub translation: f32,
    #[serde(alias = "rotation")]
    pub rotation: f32,
}

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

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GraphsSettings {
    /// The knowledge graph. ADR-2041 renamed this key from `logseq`;
    /// ADR-2115 retired the deserialisation alias, so the old name is now
    /// rejected like any other unknown key.
    #[validate(nested)]
    pub knowledge: GraphSettings,
    #[validate(nested)]
    pub visionclaw: GraphSettings,
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
