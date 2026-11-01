//! Layout types for graph visualization — domain representation.
//!
//! Mirrors `src/layout/types.rs` without specta/validator dependencies.

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum LayoutMode {
    /// ForceAtlas2 with LinLog (default)
    #[default]
    ForceDirected,
    /// Sugiyama DAG layers
    Hierarchical,
    /// Centrality rings
    Radial,
    /// Graph Laplacian eigenvectors
    Spectral,
    /// Z-axis = timestamp
    Temporal,
    /// ForceAtlas2 + Louvain metanodes
    Clustered,
    /// The quality-gate `dag-leftright` mode (ADR-2140). It does exactly what
    /// `dag-leftright` did before it was a variant: the Sugiyama layer spring
    /// of [`LayoutMode::Hierarchical`] plus the DAG mode defaults. There is no
    /// separate orientation code, so it is not a left-to-right layout in fact.
    DagLeftRight,
    /// The quality-gate `type-clustering` mode (ADR-2140). It does exactly what
    /// `type-clustering` did: the force-directed baseline with a cluster
    /// strength (default 0.01 when none is stored).
    TypeClustering,
}

/// The seven `qualityGates.layoutMode` strings, in the order the settings
/// validator lists them. [`LayoutMode::from_quality_gate_str`] maps each one.
pub const QUALITY_GATE_LAYOUT_MODES: [&str; 7] = [
    "force-directed",
    "dag-topdown",
    "dag-radial",
    "dag-leftright",
    "type-clustering",
    "spectral",
    "clustered",
];

/// Source keying the radial shells of the `dag_radial_bias` term (ADR-141 P3).
/// The shell radius per node is `key * dag_level_distance`; only the CPU-side key
/// source (and, for Ego, the shell centre) differs between modes. The GPU term is
/// unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RadialMode {
    /// Shell key = DAG hierarchy rank (existing behaviour); centre = origin.
    #[default]
    DagRank,
    /// Shell key = node-type tier (Agent inner → Knowledge → Ontology outer);
    /// centre = origin.
    TypeTier,
    /// Shell key = BFS hop-distance from a focus node; centre = focus position.
    Ego,
}

impl LayoutMode {
    /// Stable discriminant uploaded to the GPU-aligned `SimParams.layout_mode`
    /// field (ADR-141 P1). The kernel branches on this to select per-mode force
    /// terms. The mapping is frozen — never renumber; append new modes only.
    ///
    /// `DagLeftRight` (6) and `TypeClustering` (7) are new with ADR-2140; before it both rode
    /// on 1 and 0. The kernel only declares the field today, so nothing branches on it, but
    /// any future kernel branch on `layout_mode == 1` (hierarchical) or `== 0` (force-directed)
    /// must also take 6 and 7 respectively, or those two modes silently lose the behaviour.
    pub fn as_gpu_u32(self) -> u32 {
        match self {
            LayoutMode::ForceDirected => 0,
            LayoutMode::Hierarchical => 1,
            LayoutMode::Radial => 2,
            LayoutMode::Spectral => 3,
            LayoutMode::Temporal => 4,
            LayoutMode::Clustered => 5,
            LayoutMode::DagLeftRight => 6,
            LayoutMode::TypeClustering => 7,
        }
    }

    /// Inverse of [`LayoutMode::as_gpu_u32`]. Unknown discriminants fall back to
    /// `ForceDirected` so a corrupt/older GPU struct never panics.
    pub fn from_gpu_u32(v: u32) -> Self {
        match v {
            1 => LayoutMode::Hierarchical,
            2 => LayoutMode::Radial,
            3 => LayoutMode::Spectral,
            4 => LayoutMode::Temporal,
            5 => LayoutMode::Clustered,
            6 => LayoutMode::DagLeftRight,
            7 => LayoutMode::TypeClustering,
            _ => LayoutMode::ForceDirected,
        }
    }

    /// True when the mode is realised by the GPU force engine (continuous settling)
    /// rather than a one-shot CPU placement. ForceDirected and Radial (via the
    /// `dag_radial_bias` shell term) settle on the GPU; Clustered rides the GPU
    /// cluster-cohesion term; Hierarchical (ADR-141 P4) now settles on the GPU via
    /// the Sugiyama Y-by-rank layer spring. Spectral/Temporal remain CPU one-shot
    /// placements (Laplacian eigenvectors, timestamp axis).
    pub fn is_gpu_resident(self) -> bool {
        matches!(
            self,
            LayoutMode::ForceDirected
                | LayoutMode::Radial
                | LayoutMode::Clustered
                | LayoutMode::Hierarchical
                | LayoutMode::DagLeftRight
                | LayoutMode::TypeClustering
        )
    }

    /// Migrate a `qualityGates.layoutMode` string (ADR-2140). Every one of the seven
    /// strings in [`QUALITY_GATE_LAYOUT_MODES`] maps to its own variant; anything else
    /// is `None`, which the caller turns into a 400 on write and a logged default on load.
    pub fn from_quality_gate_str(s: &str) -> Option<Self> {
        Some(match s {
            "force-directed" => LayoutMode::ForceDirected,
            "dag-topdown" => LayoutMode::Hierarchical,
            "dag-radial" => LayoutMode::Radial,
            "dag-leftright" => LayoutMode::DagLeftRight,
            "type-clustering" => LayoutMode::TypeClustering,
            "spectral" => LayoutMode::Spectral,
            "clustered" => LayoutMode::Clustered,
            _ => return None,
        })
    }

    /// The `qualityGates.layoutMode` string for this mode, or `None` for
    /// [`LayoutMode::Temporal`], which that vocabulary never had.
    pub fn quality_gate_str(self) -> Option<&'static str> {
        Some(match self {
            LayoutMode::ForceDirected => "force-directed",
            LayoutMode::Hierarchical => "dag-topdown",
            LayoutMode::Radial => "dag-radial",
            LayoutMode::DagLeftRight => "dag-leftright",
            LayoutMode::TypeClustering => "type-clustering",
            LayoutMode::Spectral => "spectral",
            LayoutMode::Clustered => "clustered",
            LayoutMode::Temporal => return None,
        })
    }

    /// Parse either vocabulary: the serde name (`forceDirected`, `dagLeftRight`, ...) that
    /// is stored and sent by `POST /api/layout/mode`, or a quality-gate string.
    pub fn parse(s: &str) -> Option<Self> {
        Self::from_quality_gate_str(s)
            .or_else(|| serde_json::from_value(serde_json::Value::String(s.to_owned())).ok())
    }

    /// True for the three modes that were `dag-*` quality-gate strings: they carry the
    /// DAG defaults for centre gravity and SSSP strength.
    pub fn is_dag(self) -> bool {
        matches!(
            self,
            LayoutMode::Hierarchical | LayoutMode::Radial | LayoutMode::DagLeftRight
        )
    }
}

impl std::fmt::Display for LayoutMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutMode::ForceDirected => write!(f, "forceDirected"),
            LayoutMode::Hierarchical => write!(f, "hierarchical"),
            LayoutMode::Radial => write!(f, "radial"),
            LayoutMode::Spectral => write!(f, "spectral"),
            LayoutMode::Temporal => write!(f, "temporal"),
            LayoutMode::Clustered => write!(f, "clustered"),
            LayoutMode::DagLeftRight => write!(f, "dagLeftRight"),
            LayoutMode::TypeClustering => write!(f, "typeClustering"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutModeConfig {
    pub mode: LayoutMode,
    pub transition_duration_ms: u32,
    // ForceAtlas2 specific
    pub scaling_ratio: f32,
    pub gravity: f32,
    pub lin_log_mode: bool,
    pub dissuade_hubs: bool,
    pub barnes_hut_theta: f32,
    pub strong_gravity: bool,
    // Hierarchical specific
    pub layer_spacing: f32,
    pub node_spacing: f32,
    pub hierarchy_direction: String, // "top_down", "left_right", "radial"
    // Radial specific
    pub centrality_measure: String, // "pagerank", "degree", "betweenness"
    pub ring_count: u32,
    // Zone constraints
    pub zones: Vec<ConstraintZone>,
}

impl Default for LayoutModeConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::ForceDirected,
            transition_duration_ms: 500,
            scaling_ratio: 10.0,
            gravity: 1.0,
            lin_log_mode: true,
            dissuade_hubs: true,
            barnes_hut_theta: 0.5,
            strong_gravity: false,
            layer_spacing: 150.0,
            node_spacing: 80.0,
            hierarchy_direction: "top_down".to_string(),
            centrality_measure: "pagerank".to_string(),
            ring_count: 8,
            zones: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintZone {
    pub id: String,
    pub center: [f32; 3],
    pub radius: f32,
    pub strength: f32,
    pub node_types: Vec<String>, // e.g. ["owl_class", "ontology_node"]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutStatus {
    pub current_mode: LayoutMode,
    pub transitioning: bool,
    pub transition_progress: f32,
    pub iterations: u64,
    pub converged: bool,
    pub kinetic_energy: f64,
    pub available_modes: Vec<LayoutMode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        assert_eq!(LayoutMode::default(), LayoutMode::ForceDirected);
    }

    #[test]
    fn test_serde_round_trip() {
        let mode = LayoutMode::Hierarchical;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"hierarchical\"");
        let back: LayoutMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }

    #[test]
    fn test_display() {
        assert_eq!(LayoutMode::ForceDirected.to_string(), "forceDirected");
        assert_eq!(LayoutMode::Clustered.to_string(), "clustered");
    }

    #[test]
    fn test_gpu_u32_round_trip() {
        // ADR-141 P1: the GPU discriminant mapping must round-trip for every mode
        // and stay frozen (renumbering would silently repoint the GPU field).
        for mode in [
            LayoutMode::ForceDirected,
            LayoutMode::Hierarchical,
            LayoutMode::Radial,
            LayoutMode::Spectral,
            LayoutMode::Temporal,
            LayoutMode::Clustered,
            LayoutMode::DagLeftRight,
            LayoutMode::TypeClustering,
        ] {
            assert_eq!(LayoutMode::from_gpu_u32(mode.as_gpu_u32()), mode);
        }
        // Frozen discriminants.
        assert_eq!(LayoutMode::ForceDirected.as_gpu_u32(), 0);
        assert_eq!(LayoutMode::Clustered.as_gpu_u32(), 5);
        // Appended after the first six; never renumbered.
        assert_eq!(LayoutMode::DagLeftRight.as_gpu_u32(), 6);
        assert_eq!(LayoutMode::TypeClustering.as_gpu_u32(), 7);
        // Unknown discriminants fall back to ForceDirected, never panic.
        assert_eq!(LayoutMode::from_gpu_u32(999), LayoutMode::ForceDirected);
    }

    #[test]
    fn test_gpu_resident_split() {
        // GPU-resident modes settle on the GPU; CPU one-shot modes do not.
        assert!(LayoutMode::ForceDirected.is_gpu_resident());
        assert!(LayoutMode::Radial.is_gpu_resident());
        assert!(LayoutMode::Clustered.is_gpu_resident());
        // ADR-141 P4: Hierarchical is now GPU-resident (Sugiyama layer spring).
        assert!(LayoutMode::Hierarchical.is_gpu_resident());
        assert!(LayoutMode::DagLeftRight.is_gpu_resident());
        assert!(LayoutMode::TypeClustering.is_gpu_resident());
        assert!(!LayoutMode::Spectral.is_gpu_resident());
        assert!(!LayoutMode::Temporal.is_gpu_resident());
    }

    #[test]
    fn all_seven_quality_gate_strings_map_to_their_own_variant() {
        let expect = [
            ("force-directed", LayoutMode::ForceDirected),
            ("dag-topdown", LayoutMode::Hierarchical),
            ("dag-radial", LayoutMode::Radial),
            ("dag-leftright", LayoutMode::DagLeftRight),
            ("type-clustering", LayoutMode::TypeClustering),
            ("spectral", LayoutMode::Spectral),
            ("clustered", LayoutMode::Clustered),
        ];
        assert_eq!(expect.len(), QUALITY_GATE_LAYOUT_MODES.len());
        for (s, mode) in expect {
            assert!(QUALITY_GATE_LAYOUT_MODES.contains(&s));
            assert_eq!(LayoutMode::from_quality_gate_str(s), Some(mode), "{s}");
            assert_eq!(mode.quality_gate_str(), Some(s), "{s} round-trips");
            assert_eq!(LayoutMode::parse(s), Some(mode));
        }
        // Nothing is merged: seven strings, seven distinct modes.
        let distinct: std::collections::HashSet<_> = expect.iter().map(|(_, m)| *m).collect();
        assert_eq!(distinct.len(), 7);
    }

    #[test]
    fn an_unknown_string_is_none_in_both_vocabularies() {
        assert_eq!(LayoutMode::from_quality_gate_str("forceDirected"), None);
        assert_eq!(LayoutMode::parse("spiral"), None);
        assert_eq!(LayoutMode::parse(""), None);
    }

    #[test]
    fn the_serde_names_parse_too_including_the_two_new_variants() {
        assert_eq!(
            LayoutMode::parse("dagLeftRight"),
            Some(LayoutMode::DagLeftRight)
        );
        assert_eq!(
            LayoutMode::parse("typeClustering"),
            Some(LayoutMode::TypeClustering)
        );
        assert_eq!(LayoutMode::parse("temporal"), Some(LayoutMode::Temporal));
        assert_eq!(LayoutMode::Temporal.quality_gate_str(), None);
    }

    #[test]
    fn test_layout_mode_config_default() {
        let cfg = LayoutModeConfig::default();
        assert_eq!(cfg.mode, LayoutMode::ForceDirected);
        assert_eq!(cfg.transition_duration_ms, 500);
        assert!(cfg.zones.is_empty());
    }
}
