// src/settings/models.rs
//! Settings data models

use crate::config::{PhysicsSettings, RenderingSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PriorityWeighting {
    Linear,

    #[default]
    Exponential,

    Quadratic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSettings {
    pub lod_enabled: bool,

    pub far_threshold: f32,

    pub medium_threshold: f32,

    pub near_threshold: f32,

    pub priority_weighting: PriorityWeighting,

    pub progressive_activation: bool,

    pub activation_frames: u32,
}

impl Default for ConstraintSettings {
    fn default() -> Self {
        Self {
            lod_enabled: true,
            far_threshold: 1000.0,
            medium_threshold: 100.0,
            near_threshold: 10.0,
            priority_weighting: PriorityWeighting::Exponential,
            progressive_activation: true,
            activation_frames: 60,
        }
    }
}

/// Quality gate settings for feature toggles and performance thresholds
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityGateSettings {
    /// Enable GPU-accelerated physics computation (20-50x faster)
    pub gpu_acceleration: bool,

    /// Enable ontology constraint forces in physics simulation
    pub ontology_physics: bool,

    /// Enable semantic forces (DAG layout, type clustering)
    pub semantic_forces: bool,

    /// Layout mode: force-directed, dag-topdown, dag-radial, dag-leftright, type-clustering
    pub layout_mode: String,

    /// Enable cluster visualization (color-coded node groups)
    pub show_clusters: bool,

    /// Enable anomaly visualization (red glow on outliers)
    pub show_anomalies: bool,

    /// Enable community visualization (Louvain algorithm results)
    pub show_communities: bool,

    /// Enable RuVector HNSW integration (150x faster similarity search)
    pub ruvector_enabled: bool,

    /// Enable GNN-enhanced physics (graph neural network weights)
    pub gnn_physics: bool,

    /// Minimum FPS threshold before disabling expensive features
    pub min_fps_threshold: u32,

    /// Maximum node count before aggressive filtering
    pub max_node_count: usize,

    /// Auto-adjust quality based on performance
    pub auto_adjust: bool,

    /// Global strength of ontology constraint forces (0.0 - 1.0)
    #[serde(default = "default_ontology_strength")]
    pub ontology_strength: f32,

    /// DAG hierarchy level attraction strength (0.0 - 2.0)
    #[serde(default = "default_dag_level_attraction")]
    pub dag_level_attraction: f32,

    /// DAG sibling repulsion strength (0.0 - 2.0)
    #[serde(default = "default_dag_sibling_repulsion")]
    pub dag_sibling_repulsion: f32,

    /// Type cluster attraction strength (0.0 - 2.0)
    #[serde(default = "default_type_cluster_attraction")]
    pub type_cluster_attraction: f32,

    /// Type cluster radius (10.0 - 500.0)
    #[serde(default = "default_type_cluster_radius")]
    pub type_cluster_radius: f32,
}

fn default_ontology_strength() -> f32 {
    0.5
}
fn default_dag_level_attraction() -> f32 {
    0.5
}
fn default_dag_sibling_repulsion() -> f32 {
    0.3
}
fn default_type_cluster_attraction() -> f32 {
    0.3
}
fn default_type_cluster_radius() -> f32 {
    100.0
}

impl Default for QualityGateSettings {
    fn default() -> Self {
        Self {
            gpu_acceleration: true,  // GPU on by default if available
            ontology_physics: false, // Off by default (expensive)
            semantic_forces: true,   // Semantic clustering enabled by default
            layout_mode: "force-directed".to_string(),
            show_clusters: true,     // Show clustering by default
            show_anomalies: true,    // Show anomalies by default
            show_communities: false, // Off by default (requires computation)
            ruvector_enabled: false, // Off by default (requires integration)
            gnn_physics: false,      // Off by default (advanced)
            min_fps_threshold: 30,
            // Retained for settings-wire compatibility; no graph-selection cap.
            max_node_count: usize::MAX,
            auto_adjust: true, // Auto-adjust on by default
            ontology_strength: 0.5,
            dag_level_attraction: 0.5,
            dag_sibling_repulsion: 0.3,
            type_cluster_attraction: 0.3,
            type_cluster_radius: 100.0,
        }
    }
}

/// Node filter settings for confidence-based filtering
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeFilterSettings {
    /// Whether node filtering is enabled
    pub enabled: bool,

    /// Minimum quality score threshold (0.0 - 1.0)
    /// Nodes with quality_score below this are filtered out
    pub quality_threshold: f64,

    /// Minimum authority score threshold (0.0 - 1.0)
    /// Nodes with authority_score below this are filtered out
    pub authority_threshold: f64,

    /// Whether to use quality score for filtering
    pub filter_by_quality: bool,

    /// Whether to use authority score for filtering
    pub filter_by_authority: bool,

    /// How to combine filters: "and" requires both, "or" requires either
    pub filter_mode: String,

    /// Whether to include linked_page stub nodes (wikilink targets without public:: true).
    /// Defaults to false so the initial view shows only fully-authored page nodes,
    /// matching the client (schemaMappings) and the runtime client filter default.
    #[serde(default)]
    pub include_linked_pages: bool,
}

impl Default for NodeFilterSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality_threshold: 0.9,
            authority_threshold: 0.5,
            filter_by_quality: true,
            filter_by_authority: false,
            filter_mode: "or".to_string(),
            include_linked_pages: false,
        }
    }
}

fn default_visual_json() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllSettings {
    pub physics: PhysicsSettings,
    pub constraints: ConstraintSettings,
    pub rendering: RenderingSettings,
    pub node_filter: NodeFilterSettings,
    pub quality_gates: QualityGateSettings,
    /// Opaque JSON blob for client visual settings (glow, hologram, graphTypeVisuals,
    /// gemMaterial, sceneEffects, clusterHulls, animations, interaction, nodes, edges, labels).
    #[serde(default = "default_visual_json")]
    pub visual: serde_json::Value,
}

impl Default for AllSettings {
    fn default() -> Self {
        Self {
            physics: PhysicsSettings::default(),
            constraints: ConstraintSettings::default(),
            rendering: RenderingSettings::default(),
            node_filter: NodeFilterSettings::default(),
            quality_gates: QualityGateSettings::default(),
            visual: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsProfile {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}
