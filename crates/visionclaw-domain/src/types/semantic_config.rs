//! Semantic-force configuration: the ONE definition, held in the central settings store.
//!
//! Every parameter of every semantic force (DAG layering, type / physicality / role
//! clustering, collision, attribute springs, maturity layering, cross-domain links) lives
//! here and nowhere else. It is nested in [`PhysicsSettings`](super::PhysicsSettings) as
//! `semantic`, so it travels the same path as every other physics value: client control ->
//! settings store (validated once) -> GPU. There are no scale factors, caps or hidden
//! defaults downstream: what is stored is what the kernels apply, and each force has its own
//! `enabled` flag.
//!
//! # Defaults are calibrated, not guessed
//!
//! With no scale or cap downstream, the defaults below are the only thing bounding these
//! forces, so they were measured (`semantic_force_magnitudes_against_the_live_force_field`,
//! 9.5k nodes, live field median |f| ~36, p95 ~50): each enabled force sits at or below the
//! live field's median at its p95. The two forces that work by pulling nodes onto horizontal
//! planes (`dag`, `maturityLayout`) ship OFF: they are meaningful only for hierarchical or
//! staged corpora, and switching them on is a deliberate act in the control panel.
//!
//! The field order and widths mirror the device struct `SemanticConfig` in
//! `semantic_forces.cu`; the host image is built field by field in
//! `unified_gpu_compute/semantic.rs` and verified against the device by a GPU test, so adding
//! a field here is a change to both.

use serde::{Deserialize, Serialize};
use specta::Type;
use validator::Validate;

/// DAG layering: pull each node onto the plane for its hierarchy level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct DagForce {
    /// Distance between hierarchy levels.
    #[validate(range(min = 10.0, max = 1000.0))]
    pub vertical_spacing: f32,
    /// Minimum separation between siblings on one level.
    #[validate(range(min = 10.0, max = 1000.0))]
    pub horizontal_spacing: f32,
    /// Pull toward the level plane (force per unit of distance from it).
    #[validate(range(min = 0.0, max = 1.0))]
    pub level_attraction: f32,
    /// Push between nodes closer than `horizontalSpacing` on the same level.
    #[validate(range(min = 0.0, max = 5.0))]
    pub sibling_repulsion: f32,
    pub enabled: bool,
}

impl Default for DagForce {
    fn default() -> Self {
        Self { vertical_spacing: 100.0, horizontal_spacing: 50.0, level_attraction: 0.03, sibling_repulsion: 0.05, enabled: false }
    }
}

/// Cluster nodes of the same ontology type around the type's centroid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeClusterForce {
    /// Pull toward the centroid once a node is farther from it than `clusterRadius`.
    #[validate(range(min = 0.0, max = 2.0))]
    pub cluster_attraction: f32,
    #[validate(range(min = 10.0, max = 1000.0))]
    pub cluster_radius: f32,
    /// Push away from nodes of other types within `2 x clusterRadius` (falls off as 1/d^2).
    #[validate(range(min = 0.0, max = 50.0))]
    pub inter_cluster_repulsion: f32,
    pub enabled: bool,
}

impl Default for TypeClusterForce {
    fn default() -> Self {
        Self { cluster_attraction: 0.4, cluster_radius: 150.0, inter_cluster_repulsion: 0.2, enabled: true }
    }
}

/// Keep nodes from overlapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct CollisionForce {
    #[validate(range(min = 0.0, max = 100.0))]
    pub min_distance: f32,
    #[validate(range(min = 0.0, max = 5.0))]
    pub collision_strength: f32,
    #[validate(range(min = 1.0, max = 200.0))]
    pub node_radius: f32,
    pub enabled: bool,
}

impl Default for CollisionForce {
    fn default() -> Self {
        Self { min_distance: 5.0, collision_strength: 0.2, node_radius: 10.0, enabled: true }
    }
}

/// Springs whose rest length follows the edge weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct AttributeSpringForce {
    #[validate(range(min = 0.0, max = 1.0))]
    pub base_spring_k: f32,
    /// How strongly the edge weight scales the spring.
    #[validate(range(min = 0.0, max = 5.0))]
    pub weight_multiplier: f32,
    #[validate(range(min = 5.0, max = 1000.0))]
    pub rest_length_min: f32,
    #[validate(range(min = 5.0, max = 1000.0))]
    pub rest_length_max: f32,
    pub enabled: bool,
}

impl Default for AttributeSpringForce {
    fn default() -> Self {
        Self { base_spring_k: 0.01, weight_multiplier: 0.5, rest_length_min: 30.0, rest_length_max: 200.0, enabled: true }
    }
}

/// Cluster nodes by physicality (virtual / physical / conceptual).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct PhysicalityClusterForce {
    #[validate(range(min = 0.0, max = 2.0))]
    pub cluster_attraction: f32,
    #[validate(range(min = 10.0, max = 1000.0))]
    pub cluster_radius: f32,
    #[validate(range(min = 0.0, max = 50.0))]
    pub inter_physicality_repulsion: f32,
    pub enabled: bool,
}

impl Default for PhysicalityClusterForce {
    fn default() -> Self {
        Self { cluster_attraction: 0.5, cluster_radius: 180.0, inter_physicality_repulsion: 0.25, enabled: true }
    }
}

/// Cluster nodes by ontology role (process / agent / resource / concept).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct RoleClusterForce {
    #[validate(range(min = 0.0, max = 2.0))]
    pub cluster_attraction: f32,
    #[validate(range(min = 10.0, max = 1000.0))]
    pub cluster_radius: f32,
    #[validate(range(min = 0.0, max = 50.0))]
    pub inter_role_repulsion: f32,
    pub enabled: bool,
}

impl Default for RoleClusterForce {
    fn default() -> Self {
        Self { cluster_attraction: 0.45, cluster_radius: 160.0, inter_role_repulsion: 0.2, enabled: true }
    }
}

/// Layer nodes along Z by maturity stage (emerging -> mature -> declining).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct MaturityLayoutForce {
    #[validate(range(min = 10.0, max = 1000.0))]
    pub vertical_spacing: f32,
    #[validate(range(min = 0.0, max = 1.0))]
    pub level_attraction: f32,
    /// Z distance between adjacent stages.
    #[validate(range(min = 0.0, max = 1000.0))]
    pub stage_separation: f32,
    pub enabled: bool,
}

impl Default for MaturityLayoutForce {
    fn default() -> Self {
        Self { vertical_spacing: 150.0, level_attraction: 0.08, stage_separation: 100.0, enabled: false }
    }
}

/// Strength of links that cross ontology domains (applied by the relationship kernel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct CrossDomainForce {
    #[validate(range(min = 0.0, max = 2.0))]
    pub base_strength: f32,
    /// Extra strength per cross-domain link a node has.
    #[validate(range(min = 0.0, max = 1.0))]
    pub link_count_multiplier: f32,
    /// Upper bound on the link-count boost.
    #[validate(range(min = 0.0, max = 10.0))]
    pub max_strength_boost: f32,
    #[validate(range(min = 5.0, max = 1000.0))]
    pub rest_length: f32,
    pub enabled: bool,
}

impl Default for CrossDomainForce {
    fn default() -> Self {
        Self { base_strength: 0.3, link_count_multiplier: 0.1, max_strength_boost: 2.0, rest_length: 200.0, enabled: true }
    }
}

/// Every semantic force, as stored in the central physics settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type, Validate)]
#[serde(rename_all = "camelCase", default)]
pub struct SemanticForces {
    #[validate(nested)]
    pub dag: DagForce,
    #[validate(nested)]
    pub type_cluster: TypeClusterForce,
    #[validate(nested)]
    pub collision: CollisionForce,
    #[validate(nested)]
    pub attribute_spring: AttributeSpringForce,
    #[validate(nested)]
    pub physicality_cluster: PhysicalityClusterForce,
    #[validate(nested)]
    pub role_cluster: RoleClusterForce,
    #[validate(nested)]
    pub maturity_layout: MaturityLayoutForce,
    #[validate(nested)]
    pub cross_domain: CrossDomainForce,
}

impl SemanticForces {
    /// True when at least one force is switched on, i.e. the GPU pass has work to do.
    pub fn any_enabled(&self) -> bool {
        self.dag.enabled
            || self.type_cluster.enabled
            || self.collision.enabled
            || self.attribute_spring.enabled
            || self.physicality_cluster.enabled
            || self.role_cluster.enabled
            || self.maturity_layout.enabled
            || self.cross_domain.enabled
    }

    /// Every float parameter with its wire path. `validator`'s range check does NOT reject NaN
    /// (NaN fails both comparisons it makes), so finiteness is checked from this list.
    pub fn floats(&self) -> Vec<(&'static str, f32)> {
        vec![
            ("dag.verticalSpacing", self.dag.vertical_spacing),
            ("dag.horizontalSpacing", self.dag.horizontal_spacing),
            ("dag.levelAttraction", self.dag.level_attraction),
            ("dag.siblingRepulsion", self.dag.sibling_repulsion),
            ("typeCluster.clusterAttraction", self.type_cluster.cluster_attraction),
            ("typeCluster.clusterRadius", self.type_cluster.cluster_radius),
            ("typeCluster.interClusterRepulsion", self.type_cluster.inter_cluster_repulsion),
            ("collision.minDistance", self.collision.min_distance),
            ("collision.collisionStrength", self.collision.collision_strength),
            ("collision.nodeRadius", self.collision.node_radius),
            ("attributeSpring.baseSpringK", self.attribute_spring.base_spring_k),
            ("attributeSpring.weightMultiplier", self.attribute_spring.weight_multiplier),
            ("attributeSpring.restLengthMin", self.attribute_spring.rest_length_min),
            ("attributeSpring.restLengthMax", self.attribute_spring.rest_length_max),
            ("physicalityCluster.clusterAttraction", self.physicality_cluster.cluster_attraction),
            ("physicalityCluster.clusterRadius", self.physicality_cluster.cluster_radius),
            ("physicalityCluster.interPhysicalityRepulsion", self.physicality_cluster.inter_physicality_repulsion),
            ("roleCluster.clusterAttraction", self.role_cluster.cluster_attraction),
            ("roleCluster.clusterRadius", self.role_cluster.cluster_radius),
            ("roleCluster.interRoleRepulsion", self.role_cluster.inter_role_repulsion),
            ("maturityLayout.verticalSpacing", self.maturity_layout.vertical_spacing),
            ("maturityLayout.levelAttraction", self.maturity_layout.level_attraction),
            ("maturityLayout.stageSeparation", self.maturity_layout.stage_separation),
            ("crossDomain.baseStrength", self.cross_domain.base_strength),
            ("crossDomain.linkCountMultiplier", self.cross_domain.link_count_multiplier),
            ("crossDomain.maxStrengthBoost", self.cross_domain.max_strength_boost),
            ("crossDomain.restLength", self.cross_domain.rest_length),
        ]
    }

    /// Validate every range and finiteness, and the cross-field rule that a spring's rest length
    /// range is ordered. Returns every problem found, not only the first.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        for (name, v) in self.floats() {
            if !v.is_finite() {
                problems.push(format!("semantic forces: {name} must be a finite number"));
            }
        }
        if let Err(e) = self.validate() {
            problems.push(format!("semantic forces: {e}"));
        }
        if self.attribute_spring.rest_length_min > self.attribute_spring.rest_length_max {
            problems.push("semantic forces: attributeSpring.restLengthMin must not exceed restLengthMax".to_string());
        }
        if problems.is_empty() { Ok(()) } else { Err(problems) }
    }
}

/// One control-panel field of the semantic group. This table is the SINGLE description of the
/// group's controls: the web registry and the XR HUD both load the JSON emitted from it
/// ([`fields_json`]), so labels, ranges and steps cannot drift. A test proves every `min`/`max`
/// is exactly the validator's accepted range, and that the committed JSON copies are current.
#[derive(Debug, Clone, Copy)]
pub struct FieldSpec {
    /// Wire path below `physics.semantic`, e.g. `dag.levelAttraction`.
    pub path: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// `slider` or `toggle`.
    pub kind: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

const fn slider(path: &'static str, group: &'static str, label: &'static str, description: &'static str, min: f32, max: f32, step: f32) -> FieldSpec {
    FieldSpec { path, group, label, description, kind: "slider", min, max, step }
}
const fn toggle(path: &'static str, group: &'static str, label: &'static str, description: &'static str) -> FieldSpec {
    FieldSpec { path, group, label, description, kind: "toggle", min: 0.0, max: 0.0, step: 0.0 }
}

pub const FIELDS: &[FieldSpec] = &[
    toggle("dag.enabled", "DAG Layering", "DAG Layering", "Pull nodes onto the plane for their hierarchy level. Imposes layers: meaningful only for hierarchical corpora"),
    slider("dag.verticalSpacing", "DAG Layering", "Level Spacing", "Distance between hierarchy levels", 10.0, 1000.0, 10.0),
    slider("dag.horizontalSpacing", "DAG Layering", "Sibling Spacing", "Minimum separation between siblings on one level", 10.0, 1000.0, 10.0),
    slider("dag.levelAttraction", "DAG Layering", "Level Attraction", "Pull toward the level plane", 0.0, 1.0, 0.005),
    slider("dag.siblingRepulsion", "DAG Layering", "Sibling Repulsion", "Push between same-level nodes closer than the sibling spacing", 0.0, 5.0, 0.01),
    toggle("typeCluster.enabled", "Type Clustering", "Type Clustering", "Gather nodes of the same ontology type around the type centroid"),
    slider("typeCluster.clusterAttraction", "Type Clustering", "Attraction", "Pull toward the type centroid beyond the cluster radius", 0.0, 2.0, 0.01),
    slider("typeCluster.clusterRadius", "Type Clustering", "Cluster Radius", "Distance within which no pull applies", 10.0, 1000.0, 10.0),
    slider("typeCluster.interClusterRepulsion", "Type Clustering", "Inter-Cluster Repulsion", "Push away from nodes of other types", 0.0, 50.0, 0.1),
    toggle("collision.enabled", "Collision", "Collision", "Keep nodes from overlapping"),
    slider("collision.minDistance", "Collision", "Minimum Distance", "Smallest allowed gap between nodes", 0.0, 100.0, 1.0),
    slider("collision.collisionStrength", "Collision", "Strength", "Push applied when nodes collide", 0.0, 5.0, 0.05),
    slider("collision.nodeRadius", "Collision", "Node Radius", "Collision radius of a node", 1.0, 200.0, 1.0),
    toggle("attributeSpring.enabled", "Weighted Springs", "Weighted Springs", "Springs whose rest length follows the edge weight"),
    slider("attributeSpring.baseSpringK", "Weighted Springs", "Base Strength", "Base spring constant", 0.0, 1.0, 0.005),
    slider("attributeSpring.weightMultiplier", "Weighted Springs", "Weight Influence", "How strongly the edge weight scales the spring", 0.0, 5.0, 0.05),
    slider("attributeSpring.restLengthMin", "Weighted Springs", "Shortest Rest Length", "Rest length of the heaviest edges", 5.0, 1000.0, 5.0),
    slider("attributeSpring.restLengthMax", "Weighted Springs", "Longest Rest Length", "Rest length of the lightest edges", 5.0, 1000.0, 5.0),
    toggle("physicalityCluster.enabled", "Physicality Clustering", "Physicality Clustering", "Gather virtual, physical and conceptual nodes separately"),
    slider("physicalityCluster.clusterAttraction", "Physicality Clustering", "Attraction", "Pull toward the physicality centroid", 0.0, 2.0, 0.01),
    slider("physicalityCluster.clusterRadius", "Physicality Clustering", "Cluster Radius", "Distance within which no pull applies", 10.0, 1000.0, 10.0),
    slider("physicalityCluster.interPhysicalityRepulsion", "Physicality Clustering", "Inter-Cluster Repulsion", "Push away from nodes of another physicality", 0.0, 50.0, 0.1),
    toggle("roleCluster.enabled", "Role Clustering", "Role Clustering", "Gather process, agent, resource and concept nodes separately"),
    slider("roleCluster.clusterAttraction", "Role Clustering", "Attraction", "Pull toward the role centroid", 0.0, 2.0, 0.01),
    slider("roleCluster.clusterRadius", "Role Clustering", "Cluster Radius", "Distance within which no pull applies", 10.0, 1000.0, 10.0),
    slider("roleCluster.interRoleRepulsion", "Role Clustering", "Inter-Cluster Repulsion", "Push away from nodes of another role", 0.0, 50.0, 0.1),
    toggle("maturityLayout.enabled", "Maturity Layering", "Maturity Layering", "Layer nodes along Z by maturity stage. Imposes layers: off unless wanted"),
    slider("maturityLayout.verticalSpacing", "Maturity Layering", "Stage Spacing", "Spacing between maturity stages", 10.0, 1000.0, 10.0),
    slider("maturityLayout.levelAttraction", "Maturity Layering", "Stage Attraction", "Pull toward the stage plane", 0.0, 1.0, 0.005),
    slider("maturityLayout.stageSeparation", "Maturity Layering", "Stage Separation", "Z distance between adjacent stages", 0.0, 1000.0, 10.0),
    toggle("crossDomain.enabled", "Cross-Domain Links", "Cross-Domain Links", "Strengthen links that cross ontology domains"),
    slider("crossDomain.baseStrength", "Cross-Domain Links", "Base Strength", "Strength of a cross-domain link", 0.0, 2.0, 0.01),
    slider("crossDomain.linkCountMultiplier", "Cross-Domain Links", "Per-Link Boost", "Extra strength per cross-domain link a node has", 0.0, 1.0, 0.01),
    slider("crossDomain.maxStrengthBoost", "Cross-Domain Links", "Maximum Boost", "Upper bound on the link-count boost", 0.0, 10.0, 0.1),
    slider("crossDomain.restLength", "Cross-Domain Links", "Rest Length", "Rest length of cross-domain links", 5.0, 1000.0, 5.0),
];

/// The field table as the JSON document the web registry and the XR HUD load, with each field's
/// default taken from [`SemanticForces::default`] so the table cannot disagree with it.
pub fn fields_json() -> serde_json::Value {
    // f32 -> the shortest decimal that round-trips as an f32 (0.03, not 0.029999999329447746).
    let clean = |v: f32| -> serde_json::Value {
        serde_json::Value::from(format!("{v}").parse::<f64>().expect("a finite f32 prints as a number"))
    };
    let defaults = serde_json::to_value(SemanticForces::default()).expect("defaults serialise");
    let rows: Vec<serde_json::Value> = FIELDS
        .iter()
        .map(|f| {
            let pointer = format!("/{}", f.path.replace('.', "/"));
            let default = match defaults.pointer(&pointer) {
                Some(serde_json::Value::Number(n)) => clean(n.as_f64().unwrap_or(0.0) as f32),
                Some(other) => other.clone(),
                None => serde_json::Value::Null,
            };
            let mut row = serde_json::json!({
                "path": f.path, "group": f.group, "label": f.label,
                "description": f.description, "kind": f.kind, "default": default,
            });
            if f.kind == "slider" {
                row["min"] = clean(f.min);
                row["max"] = clean(f.max);
                row["step"] = clean(f.step);
            }
            row
        })
        .collect();
    serde_json::json!({ "generatedFrom": "visionclaw_domain::types::semantic_config::FIELDS", "fields": rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set one field of the default document by wire path and return the resulting group.
    fn with_field(path: &str, v: serde_json::Value) -> SemanticForces {
        let mut doc = serde_json::to_value(SemanticForces::default()).unwrap();
        *doc.pointer_mut(&format!("/{}", path.replace('.', "/"))).expect("path exists") = v;
        serde_json::from_value(doc).expect("still a valid document")
    }

    #[test]
    fn the_field_table_lists_every_control_exactly_once() {
        let mut paths: Vec<&str> = FIELDS.iter().map(|f| f.path).collect();
        let n = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), n, "no duplicate path");
        // every float and every enable flag has a row
        let mut expected: Vec<String> = SemanticForces::default().floats().into_iter().map(|(p, _)| p.to_string()).collect();
        for g in ["dag", "typeCluster", "collision", "attributeSpring", "physicalityCluster", "roleCluster", "maturityLayout", "crossDomain"] {
            expected.push(format!("{g}.enabled"));
        }
        expected.sort();
        let mut listed: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        listed.sort();
        assert_eq!(listed, expected);
    }

    #[test]
    fn every_slider_range_is_exactly_the_validators_range() {
        for f in FIELDS.iter().filter(|f| f.kind == "slider") {
            // the ends are accepted ...
            for edge in [f.min, f.max] {
                let c = with_field(f.path, serde_json::json!(edge));
                let mut c = c;
                // keep the cross-field rule satisfied while probing a rest-length end
                if f.path.starts_with("attributeSpring.restLength") {
                    c.attribute_spring.rest_length_min = c.attribute_spring.rest_length_min.min(c.attribute_spring.rest_length_max);
                }
                assert!(c.validate().is_ok(), "{} = {edge} must be accepted: {:?}", f.path, c.validate());
            }
            // ... and just outside either end is rejected
            let below = f.min - f.step.max(1e-3);
            let above = f.max + f.step.max(1e-3);
            assert!(with_field(f.path, serde_json::json!(below)).validate().is_err(), "{} = {below} must be rejected", f.path);
            assert!(with_field(f.path, serde_json::json!(above)).validate().is_err(), "{} = {above} must be rejected", f.path);
            // and the default lies inside
            let d = SemanticForces::default();
            let doc = serde_json::to_value(&d).unwrap();
            let dv = doc.pointer(&format!("/{}", f.path.replace('.', "/"))).and_then(|v| v.as_f64()).unwrap() as f32;
            assert!(dv >= f.min && dv <= f.max, "{} default {dv} outside {}..{}", f.path, f.min, f.max);
            assert!(f.step > 0.0 && f.step <= f.max - f.min, "{} has a usable step", f.path);
        }
    }

    /// The committed JSON copies (docs reference, web registry data, XR HUD data) must equal what
    /// the table emits now. Regenerate with `cargo test -p visionclaw-domain -- --ignored
    /// regenerate_semantic_field_copies`.
    #[test]
    fn the_committed_json_copies_are_current() {
        let want = serde_json::to_string_pretty(&fields_json()).unwrap() + "\n";
        for rel in COPIES {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
            let have = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(have, want, "{rel} is stale: regenerate the semantic field copies");
        }
    }

    #[test]
    #[ignore = "writes the committed JSON copies"]
    fn regenerate_semantic_field_copies() {
        let want = serde_json::to_string_pretty(&fields_json()).unwrap() + "\n";
        for rel in COPIES {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
        }
    }

    const COPIES: &[&str] = &[
        "docs/reference/semantic-forces-fields.json",
        "client/src/features/control-center/registry/semanticFields.json",
        "xr-client/data/semantic_fields.json",
    ];

    #[test]
    fn defaults_pass_validation_and_round_trip() {
        let d = SemanticForces::default();
        assert!(d.check().is_ok());
        let back: SemanticForces = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn wire_names_are_camel_case_and_stable() {
        let j = serde_json::to_value(SemanticForces::default()).unwrap();
        for k in ["dag", "typeCluster", "collision", "attributeSpring", "physicalityCluster", "roleCluster", "maturityLayout", "crossDomain"] {
            assert!(j.get(k).is_some(), "missing group {k}");
        }
        assert!(j["dag"].get("levelAttraction").is_some() && j["dag"].get("enabled").is_some());
        assert!(j["typeCluster"].get("interClusterRepulsion").is_some());
        assert!(j["crossDomain"].get("maxStrengthBoost").is_some());
    }

    #[test]
    fn a_partial_document_fills_the_rest_from_defaults() {
        let p: SemanticForces = serde_json::from_str(r#"{"dag":{"levelAttraction":2.0}}"#).unwrap();
        assert_eq!(p.dag.level_attraction, 2.0);
        assert_eq!(p.dag.vertical_spacing, DagForce::default().vertical_spacing);
        assert_eq!(p.collision, CollisionForce::default());
    }

    #[test]
    fn out_of_range_non_finite_and_inverted_values_are_rejected() {
        let mut c = SemanticForces::default();
        c.dag.level_attraction = 11.0;
        assert!(c.check().is_err(), "above range");
        let mut c = SemanticForces::default();
        c.type_cluster.cluster_radius = f32::NAN;
        assert!(c.check().is_err(), "NaN must never reach the GPU");
        let mut c = SemanticForces::default();
        c.collision.node_radius = f32::INFINITY;
        assert!(c.check().is_err(), "infinity must never reach the GPU");
        let mut c = SemanticForces::default();
        c.attribute_spring.rest_length_min = 300.0;
        assert!(c.check().is_err(), "min above max");
        let mut c = SemanticForces::default();
        c.dag.vertical_spacing = 0.0;
        assert!(c.check().is_err(), "a zero spacing would collapse every level onto one plane");
    }

    #[test]
    fn the_float_list_covers_every_numeric_field_of_the_wire_document() {
        // Every number in the serialised document must be in `floats()`, so a field added to a
        // group cannot silently escape the finiteness check.
        fn numbers(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(m) => m.iter().for_each(|(k, x)| numbers(x, &if path.is_empty() { k.clone() } else { format!("{path}.{k}") }, out)),
                serde_json::Value::Number(_) => out.push(path.to_string()),
                _ => {}
            }
        }
        let mut wire = Vec::new();
        numbers(&serde_json::to_value(SemanticForces::default()).unwrap(), "", &mut wire);
        let mut listed: Vec<String> = SemanticForces::default().floats().into_iter().map(|(n, _)| n.to_string()).collect();
        wire.sort();
        listed.sort();
        assert_eq!(listed, wire);
    }

    #[test]
    fn any_enabled_follows_the_flags() {
        let mut c = SemanticForces::default();
        assert!(c.any_enabled());
        c.dag.enabled = false;
        c.type_cluster.enabled = false;
        c.collision.enabled = false;
        c.attribute_spring.enabled = false;
        c.physicality_cluster.enabled = false;
        c.role_cluster.enabled = false;
        c.maturity_layout.enabled = false;
        c.cross_domain.enabled = false;
        assert!(!c.any_enabled());
    }
}
