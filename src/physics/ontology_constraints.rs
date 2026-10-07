//! Ontology constraints translator for converting OWL axioms into physics constraints
//!
//! This module provides a bridge between semantic ontology reasoning and physics-based
//! graph layout optimization. It converts OWL axioms and logical inferences into
//! constraint forces that can be applied to knowledge graph nodes in 3D space.
//!
//! ## Core Concepts
//!
//! - **Axiom Translation**: Converts logical axioms (DisjointClasses, SubClassOf, etc.)
//!   into specific physics constraints with appropriate force parameters
//! - **Inference Integration**: Applies reasoning results as dynamic constraints
//!   that adapt as the ontology evolves
//! - **Constraint Grouping**: Organizes ontology-derived constraints into logical
//!   categories for efficient processing and debugging
//!
//! ## Translation Mappings
//!
//! | OWL Axiom           | Physics Constraint      | Effect                    |
//! |---------------------|-------------------------|---------------------------|
//! | DisjointClasses(A,B)| Separation force        | Push A and B instances apart |
//! | SubClassOf(A,B)     | Hierarchical alignment  | Group A instances near B    |
//! | InverseOf(P,Q)      | Bidirectional edges     | Symmetric relationship forces|
//! | SameAs(a,b)         | Co-location/merge       | Pull a and b together       |
//! | FunctionalProperty  | Cardinality boundaries  | Limit connections per node  |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::physics::ontology_constraints::OntologyConstraintTranslator;
//! use crate::models::constraints::ConstraintSet;
//!
//! let translator = OntologyConstraintTranslator::new();
//!
//!
//! let constraints = translator.axioms_to_constraints(&axioms, &nodes)?;
//!
//!
//! let constraint_set = translator.apply_ontology_constraints(&graph, &reasoning_report)?;
//! ```

use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::models::{
    constraints::{Constraint, ConstraintKind, ConstraintSet},
    graph::GraphData,
    node::Node,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OWLAxiomType {
    DisjointClasses,

    SubClassOf,

    EquivalentClasses,

    SameAs,

    DifferentFrom,

    InverseOf,

    FunctionalProperty,

    InverseFunctionalProperty,

    TransitiveProperty,

    SymmetricProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OWLAxiom {
    pub axiom_type: OWLAxiomType,
    pub subject: String,
    pub object: Option<String>,
    pub property: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyInference {
    pub inferred_axiom: OWLAxiom,
    pub premise_axioms: Vec<String>,
    pub reasoning_confidence: f32,
    pub is_derived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraintConfig {
    pub disjoint_separation_strength: f32,

    pub hierarchy_alignment_strength: f32,

    pub sameas_colocation_strength: f32,

    pub cardinality_boundary_strength: f32,

    pub max_separation_distance: f32,

    pub min_colocation_distance: f32,

    pub enable_constraint_caching: bool,

    pub cache_invalidation_enabled: bool,
}

impl Default for OntologyConstraintConfig {
    fn default() -> Self {
        Self {
            disjoint_separation_strength: 0.8,
            hierarchy_alignment_strength: 0.6,
            sameas_colocation_strength: 0.9,
            cardinality_boundary_strength: 0.7,
            max_separation_distance: 50.0,
            min_colocation_distance: 2.0,
            enable_constraint_caching: true,
            cache_invalidation_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OntologyConstraintGroup {
    OntologySeparation,

    OntologyAlignment,

    OntologyBoundaries,

    OntologyIdentity,
}

#[derive(Debug, Clone)]
struct ConstraintCacheEntry {
    constraints: Vec<Constraint>,
    #[allow(dead_code)]
    axiom_hash: u64,
    #[allow(dead_code)]
    last_updated: std::time::Instant,
}

pub struct OntologyConstraintTranslator {
    config: OntologyConstraintConfig,
    constraint_cache: HashMap<String, ConstraintCacheEntry>,
    node_type_cache: HashMap<u32, HashSet<String>>,
}

impl OntologyConstraintTranslator {
    pub fn new() -> Self {
        Self::with_config(OntologyConstraintConfig::default())
    }

    pub fn with_config(config: OntologyConstraintConfig) -> Self {
        Self {
            config,
            constraint_cache: HashMap::new(),
            node_type_cache: HashMap::new(),
        }
    }

    pub fn axioms_to_constraints(
        &mut self,
        axioms: &[OWLAxiom],
        nodes: &[Node],
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        info!(
            "Converting {} OWL axioms to physics constraints",
            axioms.len()
        );

        let node_by_id: HashMap<String, &Node> = nodes
            .iter()
            .map(|node| (node.metadata_id.clone(), node))
            .collect();

        self.update_node_type_cache(nodes);

        let mut constraints = Vec::new();

        for axiom in axioms {
            trace!("Processing axiom: {:?}", axiom);

            match self.translate_single_axiom(axiom, &node_by_id) {
                Ok(mut axiom_constraints) => {
                    constraints.append(&mut axiom_constraints);
                }
                Err(e) => {
                    warn!("Failed to translate axiom {:?}: {}", axiom, e);
                }
            }
        }

        info!(
            "Generated {} constraints from {} axioms",
            constraints.len(),
            axioms.len()
        );
        Ok(constraints)
    }

    pub fn inferences_to_constraints(
        &mut self,
        inferences: &[OntologyInference],
        graph: &GraphData,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        info!(
            "Converting {} ontology inferences to constraints",
            inferences.len()
        );

        let mut constraints = Vec::new();

        let mut inference_constraints = Vec::new();

        for inference in inferences {
            let mut single_inference_constraints = self.axioms_to_constraints(
                std::slice::from_ref(&inference.inferred_axiom),
                &graph.nodes,
            )?;

            for constraint in &mut single_inference_constraints {
                constraint.weight *= inference.reasoning_confidence;
            }

            inference_constraints.push(single_inference_constraints);
        }

        for mut batch in inference_constraints {
            constraints.append(&mut batch);
        }

        info!(
            "Generated {} constraints from {} inferences",
            constraints.len(),
            inferences.len()
        );
        Ok(constraints)
    }

    pub fn apply_ontology_constraints(
        &mut self,
        graph: &GraphData,
        reasoning_report: &OntologyReasoningReport,
    ) -> Result<ConstraintSet, Box<dyn std::error::Error>> {
        info!(
            "Applying ontology constraints to graph with {} nodes",
            graph.nodes.len()
        );

        let mut all_constraints = Vec::new();

        let mut axiom_constraints =
            self.axioms_to_constraints(&reasoning_report.axioms, &graph.nodes)?;
        all_constraints.append(&mut axiom_constraints);

        let mut inference_constraints =
            self.inferences_to_constraints(&reasoning_report.inferences, graph)?;
        all_constraints.append(&mut inference_constraints);

        let mut constraint_set = ConstraintSet {
            constraints: all_constraints,
            groups: std::collections::HashMap::new(),
        };

        let grouped_constraints = self.group_constraints_by_category(&constraint_set.constraints);
        for (group, indices) in grouped_constraints {
            let group_name = match group {
                OntologyConstraintGroup::OntologySeparation => "ontology_separation",
                OntologyConstraintGroup::OntologyAlignment => "ontology_alignment",
                OntologyConstraintGroup::OntologyBoundaries => "ontology_boundaries",
                OntologyConstraintGroup::OntologyIdentity => "ontology_identity",
            };
            constraint_set
                .groups
                .insert(group_name.to_string(), indices);
        }

        info!(
            "Applied {} total ontology constraints",
            constraint_set.constraints.len()
        );
        Ok(constraint_set)
    }

    pub fn get_constraint_strength(&self, axiom_type: &OWLAxiomType) -> f32 {
        match axiom_type {
            OWLAxiomType::DisjointClasses | OWLAxiomType::DifferentFrom => {
                self.config.disjoint_separation_strength
            }
            OWLAxiomType::SubClassOf => self.config.hierarchy_alignment_strength,
            OWLAxiomType::SameAs | OWLAxiomType::EquivalentClasses => {
                self.config.sameas_colocation_strength
            }
            OWLAxiomType::FunctionalProperty | OWLAxiomType::InverseFunctionalProperty => {
                self.config.cardinality_boundary_strength
            }
            _ => 0.5,
        }
    }

    pub fn clear_cache(&mut self) {
        self.constraint_cache.clear();
        self.node_type_cache.clear();
        debug!("Cleared ontology constraint cache");
    }

    pub fn get_cache_stats(&self) -> OntologyConstraintCacheStats {
        let total_entries = self.constraint_cache.len();
        let total_cached_constraints: usize = self
            .constraint_cache
            .values()
            .map(|entry| entry.constraints.len())
            .sum();

        OntologyConstraintCacheStats {
            total_cache_entries: total_entries,
            total_cached_constraints,
            node_type_entries: self.node_type_cache.len(),
        }
    }

    pub fn update_node_type_cache(&mut self, nodes: &[Node]) {
        for node in nodes {
            let mut types = HashSet::new();

            if let Some(node_type) = &node.node_type {
                types.insert(node_type.clone());
            }

            if let Some(group) = &node.group {
                types.insert(group.clone());
            }

            for (key, value) in &node.metadata {
                if key.contains("type") || key.contains("class") || key.contains("category") {
                    types.insert(value.clone());
                }
            }

            self.node_type_cache.insert(node.id, types);
        }
    }

    fn translate_single_axiom(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let base_strength = self.get_constraint_strength(&axiom.axiom_type) * axiom.confidence;

        match axiom.axiom_type {
            OWLAxiomType::DisjointClasses => {
                self.create_disjoint_class_constraints(axiom, node_lookup, base_strength)
            }
            OWLAxiomType::SubClassOf => {
                self.create_subclass_constraints(axiom, node_lookup, base_strength)
            }
            OWLAxiomType::SameAs => {
                self.create_sameas_constraints(axiom, node_lookup, base_strength)
            }
            OWLAxiomType::DifferentFrom => {
                self.create_different_from_constraints(axiom, node_lookup, base_strength)
            }
            OWLAxiomType::FunctionalProperty => {
                self.create_functional_property_constraints(axiom, node_lookup, base_strength)
            }
            OWLAxiomType::InverseOf => {
                self.create_inverse_property_constraints(axiom, node_lookup, base_strength)
            }
            _ => {
                debug!("Axiom type {:?} not yet supported", axiom.axiom_type);
                Ok(Vec::new())
            }
        }
    }

    fn create_disjoint_class_constraints(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
        strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let object = axiom
            .object
            .as_ref()
            .ok_or("DisjointClasses axiom missing object")?;

        let class_a_nodes = self.find_nodes_of_type(&axiom.subject, node_lookup);
        let class_b_nodes = self.find_nodes_of_type(object, node_lookup);

        let mut constraints = Vec::new();

        for &node_a in &class_a_nodes {
            for &node_b in &class_b_nodes {
                constraints.push(Constraint {
                    kind: ConstraintKind::Separation,
                    node_indices: vec![node_a.id, node_b.id],
                    params: vec![self.config.max_separation_distance * 0.7],
                    weight: strength,
                    active: true,
                });
            }
        }

        debug!(
            "Created {} disjoint class constraints between {} and {} nodes",
            constraints.len(),
            class_a_nodes.len(),
            class_b_nodes.len()
        );

        Ok(constraints)
    }

    fn create_subclass_constraints(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
        strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let superclass = axiom
            .object
            .as_ref()
            .ok_or("SubClassOf axiom missing superclass")?;

        let subclass_nodes = self.find_nodes_of_type(&axiom.subject, node_lookup);
        let superclass_nodes = self.find_nodes_of_type(superclass, node_lookup);

        let mut constraints = Vec::new();

        if !superclass_nodes.is_empty() {
            let superclass_centroid = self.calculate_node_centroid(&superclass_nodes);

            for &node in &subclass_nodes {
                constraints.push(Constraint {
                    kind: ConstraintKind::Clustering,
                    node_indices: vec![node.id],
                    params: vec![
                        0.0,
                        strength,
                        superclass_centroid.0,
                        superclass_centroid.1,
                        superclass_centroid.2,
                    ],
                    weight: strength,
                    active: true,
                });
            }
        }

        debug!(
            "Created {} subclass alignment constraints",
            constraints.len()
        );
        Ok(constraints)
    }

    fn create_sameas_constraints(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
        strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let object = axiom.object.as_ref().ok_or("SameAs axiom missing object")?;

        if let (Some(&node_a), Some(&node_b)) =
            (node_lookup.get(&axiom.subject), node_lookup.get(object))
        {
            Ok(vec![Constraint {
                kind: ConstraintKind::Clustering,
                node_indices: vec![node_a.id, node_b.id],
                params: vec![0.0, strength, self.config.min_colocation_distance],
                weight: strength,
                active: true,
            }])
        } else {
            debug!("SameAs constraint: one or both nodes not found");
            Ok(Vec::new())
        }
    }

    fn create_different_from_constraints(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
        _strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let object = axiom
            .object
            .as_ref()
            .ok_or("DifferentFrom axiom missing object")?;

        if let (Some(&node_a), Some(&node_b)) =
            (node_lookup.get(&axiom.subject), node_lookup.get(object))
        {
            Ok(vec![Constraint::separation(
                node_a.id,
                node_b.id,
                self.config.max_separation_distance * 0.5,
            )])
        } else {
            debug!("DifferentFrom constraint: one or both nodes not found");
            Ok(Vec::new())
        }
    }

    fn create_functional_property_constraints(
        &self,
        axiom: &OWLAxiom,
        node_lookup: &HashMap<String, &Node>,
        strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let property_name = &axiom.subject;
        let affected_nodes: Vec<&Node> = node_lookup
            .values()
            .filter(|node| {
                node.metadata.contains_key(property_name)
                    || node.metadata.values().any(|v| v.contains(property_name))
            })
            .cloned()
            .collect();

        let mut constraints = Vec::new();

        for &node in &affected_nodes {
            constraints.push(Constraint {
                kind: ConstraintKind::Boundary,
                node_indices: vec![node.id],
                params: vec![-20.0, 20.0, -20.0, 20.0, -10.0, 10.0],
                weight: strength,
                active: true,
            });
        }

        debug!(
            "Created {} functional property boundary constraints",
            constraints.len()
        );
        Ok(constraints)
    }

    fn create_inverse_property_constraints(
        &self,
        _axiom: &OWLAxiom,
        _node_lookup: &HashMap<String, &Node>,
        _strength: f32,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        debug!("Inverse property constraints not fully implemented yet");
        Ok(Vec::new())
    }

    fn find_nodes_of_type<'a>(
        &self,
        type_name: &str,
        node_lookup: &HashMap<String, &'a Node>,
    ) -> Vec<&'a Node> {
        node_lookup
            .values()
            .filter(|node| {
                node.node_type.as_ref().is_some_and(|t| t == type_name)
                    || node.group.as_ref().is_some_and(|g| g == type_name)
                    || node.metadata.values().any(|v| v == type_name)
                    || node.metadata_id.contains(type_name)
            })
            .cloned()
            .collect()
    }

    fn calculate_node_centroid(&self, nodes: &[&Node]) -> (f32, f32, f32) {
        if nodes.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let count = nodes.len() as f32;
        let sum = nodes.iter().fold((0.0, 0.0, 0.0), |acc, node| {
            let pos = node.data.position();
            (acc.0 + pos.x, acc.1 + pos.y, acc.2 + pos.z)
        });

        (sum.0 / count, sum.1 / count, sum.2 / count)
    }

    fn group_constraints_by_category(
        &self,
        constraints: &[Constraint],
    ) -> HashMap<OntologyConstraintGroup, Vec<usize>> {
        let mut groups: HashMap<OntologyConstraintGroup, Vec<usize>> = HashMap::new();

        for (idx, constraint) in constraints.iter().enumerate() {
            let group = match constraint.kind {
                ConstraintKind::Separation => OntologyConstraintGroup::OntologySeparation,
                ConstraintKind::Clustering => OntologyConstraintGroup::OntologyAlignment,
                ConstraintKind::Boundary => OntologyConstraintGroup::OntologyBoundaries,
                ConstraintKind::FixedPosition => OntologyConstraintGroup::OntologyIdentity,
                _ => OntologyConstraintGroup::OntologyAlignment,
            };

            groups.entry(group).or_default().push(idx);
        }

        debug!(
            "Grouped constraints: {:?}",
            groups.iter().map(|(k, v)| (k, v.len())).collect::<Vec<_>>()
        );

        groups
    }
}

impl Default for OntologyConstraintTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OntologyReasoningReport {
    pub axioms: Vec<OWLAxiom>,
    pub inferences: Vec<OntologyInference>,
    pub consistency_checks: Vec<ConsistencyCheck>,
    pub reasoning_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConsistencyCheck {
    pub is_consistent: bool,
    pub conflicting_axioms: Vec<String>,
    pub suggested_resolution: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OntologyConstraintCacheStats {
    pub total_cache_entries: usize,
    pub total_cached_constraints: usize,
    pub node_type_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    use visionclaw_domain::models::node::Node;
    // ADR-090: Node.data is domain's BinaryNodeData, not webxr's BinaryNodeDataClient.
    use visionclaw_domain::types::vec3::BinaryNodeData;

    fn create_test_node(id: u32, metadata_id: String, node_type: Option<String>) -> Node {
        Node {
            id,
            metadata_id,
            label: format!("Test Node {}", id),
            data: BinaryNodeData {
                node_id: id,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
            },
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            mass: Some(1.0),
            owl_class_iri: None,
            metadata: HashMap::new(),
            file_size: 0,
            node_type,
            size: None,
            color: None,
            weight: None,
            group: None,
            user_data: None,
        }
    }

    #[test]
    fn test_disjoint_classes_translation() {
        let mut translator = OntologyConstraintTranslator::new();

        let nodes = vec![
            create_test_node(1, "animal1".to_string(), Some("Animal".to_string())),
            create_test_node(2, "plant1".to_string(), Some("Plant".to_string())),
            create_test_node(3, "animal2".to_string(), Some("Animal".to_string())),
        ];

        let axiom = OWLAxiom {
            axiom_type: OWLAxiomType::DisjointClasses,
            subject: "Animal".to_string(),
            object: Some("Plant".to_string()),
            property: None,
            confidence: 1.0,
        };

        let constraints = translator.axioms_to_constraints(&[axiom], &nodes).unwrap();

        assert!(!constraints.is_empty());
        assert!(constraints
            .iter()
            .all(|c| c.kind == ConstraintKind::Separation));
    }

    #[test]
    fn test_sameas_translation() {
        let mut translator = OntologyConstraintTranslator::new();

        let nodes = vec![
            create_test_node(1, "person1".to_string(), None),
            create_test_node(2, "person2".to_string(), None),
        ];

        let axiom = OWLAxiom {
            axiom_type: OWLAxiomType::SameAs,
            subject: "person1".to_string(),
            object: Some("person2".to_string()),
            property: None,
            confidence: 1.0,
        };

        let constraints = translator.axioms_to_constraints(&[axiom], &nodes).unwrap();

        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].kind, ConstraintKind::Clustering);
    }

    #[test]
    fn test_constraint_strength_calculation() {
        let translator = OntologyConstraintTranslator::new();

        let disjoint_strength = translator.get_constraint_strength(&OWLAxiomType::DisjointClasses);
        let sameas_strength = translator.get_constraint_strength(&OWLAxiomType::SameAs);

        assert!(disjoint_strength > 0.0);
        assert!(sameas_strength > 0.0);
        assert!(sameas_strength > disjoint_strength);
    }

    #[test]
    fn test_cache_functionality() {
        let mut translator = OntologyConstraintTranslator::new();

        let nodes = vec![create_test_node(1, "test".to_string(), None)];
        translator.update_node_type_cache(&nodes);

        let stats = translator.get_cache_stats();
        assert_eq!(stats.node_type_entries, 1);

        translator.clear_cache();
        let stats_after_clear = translator.get_cache_stats();
        assert_eq!(stats_after_clear.node_type_entries, 0);
    }
}
