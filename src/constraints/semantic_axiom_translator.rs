// Semantic Axiom Translator - Enhanced OWL Axiom to Physics Constraint Translation
// Maps OWL semantics to physics-based layout constraints with priority blending

use super::axiom_mapper::{AxiomType, OWLAxiom, TranslationConfig};
use super::physics_constraint::*;
use super::semantic_physics_types::*;
use std::collections::HashMap;

/// Configuration for semantic physics translation
#[derive(Debug, Clone)]
pub struct SemanticPhysicsConfig {
    /// Base configuration for standard translation
    pub base_config: TranslationConfig,

    /// Multiplier for DisjointWith separation (repel_k * multiplier)
    pub disjoint_repel_multiplier: f32,

    /// Multiplier for SubClassOf attraction (spring_k * multiplier)
    pub subclass_spring_multiplier: f32,

    /// Enable automatic axis alignment for hierarchies
    pub enable_hierarchy_alignment: bool,

    /// Enable bidirectional constraints for symmetric relations
    pub enable_bidirectional_constraints: bool,

    /// Priority blending strategy
    pub priority_blending: PriorityBlendingStrategy,
}

impl Default for SemanticPhysicsConfig {
    fn default() -> Self {
        Self {
            base_config: TranslationConfig::default(),
            disjoint_repel_multiplier: 2.0,
            subclass_spring_multiplier: 0.5,
            enable_hierarchy_alignment: true,
            enable_bidirectional_constraints: true,
            priority_blending: PriorityBlendingStrategy::Weighted,
        }
    }
}

/// Priority blending strategies for conflicting constraints
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PriorityBlendingStrategy {
    /// Weighted average by priority
    Weighted,
    /// Take highest priority (lowest number)
    HighestPriority,
    /// Take strongest constraint
    Strongest,
    /// Blend all equally
    Equal,
}

/// Semantic axiom translator with enhanced constraint generation
pub struct SemanticAxiomTranslator {
    config: SemanticPhysicsConfig,
    /// Cache of class IRI to NodeId mappings
    class_to_node: HashMap<String, NodeId>,
    /// Cache of parent-child relationships for hierarchy alignment
    hierarchy_cache: HashMap<NodeId, Vec<NodeId>>,
    /// Next available node ID
    next_node_id: NodeId,
}

impl SemanticAxiomTranslator {
    /// Create new translator with default config
    pub fn new() -> Self {
        Self {
            config: SemanticPhysicsConfig::default(),
            class_to_node: HashMap::new(),
            hierarchy_cache: HashMap::new(),
            next_node_id: 1,
        }
    }

    /// Create translator with custom config
    pub fn with_config(config: SemanticPhysicsConfig) -> Self {
        Self {
            config,
            class_to_node: HashMap::new(),
            hierarchy_cache: HashMap::new(),
            next_node_id: 1,
        }
    }

    /// Get or create node ID for class IRI
    pub fn get_or_create_node_id(&mut self, class_iri: &str) -> NodeId {
        if let Some(&node_id) = self.class_to_node.get(class_iri) {
            node_id
        } else {
            let node_id = self.next_node_id;
            self.next_node_id += 1;
            self.class_to_node.insert(class_iri.to_string(), node_id);
            node_id
        }
    }

    /// Translate OWL axioms to semantic physics constraints
    pub fn translate_axioms(&mut self, axioms: &[OWLAxiom]) -> Vec<SemanticPhysicsConstraint> {
        axioms
            .iter()
            .flat_map(|axiom| self.translate_axiom(axiom))
            .collect()
    }

    /// Translate single axiom to semantic constraints
    pub fn translate_axiom(&mut self, axiom: &OWLAxiom) -> Vec<SemanticPhysicsConstraint> {
        let priority = self.calculate_priority(axiom);

        match &axiom.axiom_type {
            AxiomType::DisjointClasses { classes } => {
                self.translate_disjoint_classes(classes, priority)
            }
            AxiomType::SubClassOf {
                subclass,
                superclass,
            } => self.translate_subclass_of(*subclass, *superclass, priority),
            AxiomType::EquivalentClasses { class1, class2 } => {
                self.translate_equivalent_classes(*class1, *class2, priority)
            }
            AxiomType::SameAs {
                individual1,
                individual2,
            } => self.translate_same_as(*individual1, *individual2, priority),
            AxiomType::DifferentFrom {
                individual1,
                individual2,
            } => self.translate_different_from(*individual1, *individual2, priority),
            AxiomType::PropertyDomainRange {
                property,
                domain,
                range,
            } => self.translate_property_domain_range(*property, *domain, *range, priority),
            AxiomType::PartOf { part, whole } => self.translate_part_of(*part, *whole, priority),
            _ => vec![],
        }
    }

    /// Calculate priority for axiom (1-10, lower is higher priority)
    fn calculate_priority(&self, axiom: &OWLAxiom) -> u8 {
        if axiom.user_defined {
            1 // Highest priority
        } else if axiom.inferred {
            7 // Lower priority
        } else {
            5 // Medium priority (asserted)
        }
    }

    /// Translate DisjointClasses to Separation constraints
    fn translate_disjoint_classes(
        &mut self,
        classes: &[NodeId],
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let mut constraints = Vec::new();

        // Create pairwise separation constraints
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                let class_a = format!("node_{}", classes[i]);
                let class_b = format!("node_{}", classes[j]);

                let min_distance = self.config.base_config.disjoint_separation_distance
                    * self.config.disjoint_repel_multiplier;
                let strength = self.config.base_config.disjoint_separation_strength;

                constraints.push(SemanticPhysicsConstraint::Separation {
                    class_a,
                    class_b,
                    min_distance,
                    strength,
                    priority,
                });
            }
        }

        constraints
    }

    /// Translate SubClassOf to HierarchicalAttraction constraints
    fn translate_subclass_of(
        &mut self,
        subclass: NodeId,
        superclass: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        // Update hierarchy cache
        self.hierarchy_cache
            .entry(superclass)
            .or_default()
            .push(subclass);

        let child_class = format!("node_{}", subclass);
        let parent_class = format!("node_{}", superclass);

        let ideal_distance = self.config.base_config.subclass_clustering_distance;
        let strength = self.config.base_config.subclass_clustering_stiffness
            * self.config.subclass_spring_multiplier;

        let mut constraints = vec![SemanticPhysicsConstraint::HierarchicalAttraction {
            child_class: child_class.clone(),
            parent_class,
            ideal_distance,
            strength,
            priority,
        }];

        // Add axis alignment if enabled
        if self.config.enable_hierarchy_alignment {
            // Align child on Y axis relative to parent depth
            constraints.push(SemanticPhysicsConstraint::Alignment {
                class_iri: child_class,
                axis: Axis::Y,
                target_position: 0.0, // Will be calculated based on hierarchy depth
                strength: 0.5,
                priority: priority + 2, // Lower priority than main constraint
            });
        }

        constraints
    }

    /// Translate EquivalentClasses to Colocation and BidirectionalEdge
    fn translate_equivalent_classes(
        &mut self,
        class1: NodeId,
        class2: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let class_a = format!("node_{}", class1);
        let class_b = format!("node_{}", class2);

        let target_distance = self.config.base_config.equivalent_colocation_distance;
        let strength = self.config.base_config.equivalent_colocation_strength;

        let mut constraints = vec![SemanticPhysicsConstraint::Colocation {
            class_a: class_a.clone(),
            class_b: class_b.clone(),
            target_distance,
            strength,
            priority,
        }];

        // Add bidirectional edge if enabled
        if self.config.enable_bidirectional_constraints {
            constraints.push(SemanticPhysicsConstraint::BidirectionalEdge {
                class_a,
                class_b,
                strength: 0.9,
                priority,
            });
        }

        constraints
    }

    /// Translate SameAs to Colocation
    fn translate_same_as(
        &mut self,
        individual1: NodeId,
        individual2: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let class_a = format!("node_{}", individual1);
        let class_b = format!("node_{}", individual2);

        vec![SemanticPhysicsConstraint::Colocation {
            class_a,
            class_b,
            target_distance: 0.0, // Complete overlap
            strength: 1.0,        // Maximum strength
            priority,
        }]
    }

    /// Translate DifferentFrom to Separation
    fn translate_different_from(
        &mut self,
        individual1: NodeId,
        individual2: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let class_a = format!("node_{}", individual1);
        let class_b = format!("node_{}", individual2);

        let min_distance = self.config.base_config.disjoint_separation_distance;
        let strength = self.config.base_config.disjoint_separation_strength;

        vec![SemanticPhysicsConstraint::Separation {
            class_a,
            class_b,
            min_distance,
            strength,
            priority,
        }]
    }

    /// Translate PropertyDomainRange to Alignment constraints
    fn translate_property_domain_range(
        &mut self,
        _property: NodeId,
        domain: NodeId,
        range: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let domain_class = format!("node_{}", domain);
        let range_class = format!("node_{}", range);

        vec![
            // Align domain on left (X = -50)
            SemanticPhysicsConstraint::Alignment {
                class_iri: domain_class,
                axis: Axis::X,
                target_position: -50.0,
                strength: 0.6,
                priority,
            },
            // Align range on right (X = 50)
            SemanticPhysicsConstraint::Alignment {
                class_iri: range_class,
                axis: Axis::X,
                target_position: 50.0,
                strength: 0.6,
                priority,
            },
        ]
    }

    /// Translate PartOf to Containment
    fn translate_part_of(
        &mut self,
        part: NodeId,
        whole: NodeId,
        priority: u8,
    ) -> Vec<SemanticPhysicsConstraint> {
        let child_class = format!("node_{}", part);
        let parent_class = format!("node_{}", whole);

        let radius = self.config.base_config.subclass_clustering_distance
            * self.config.base_config.containment_radius_multiplier;
        let strength = self.config.base_config.containment_strength;

        vec![SemanticPhysicsConstraint::Containment {
            child_class,
            parent_class,
            radius,
            strength,
            priority,
        }]
    }

    /// Convert semantic constraints to standard physics constraints
    pub fn to_physics_constraints(
        &self,
        semantic_constraints: &[SemanticPhysicsConstraint],
    ) -> Vec<PhysicsConstraint> {
        semantic_constraints
            .iter()
            .filter_map(|sc| self.semantic_to_physics(sc))
            .collect()
    }

    /// Convert single semantic constraint to physics constraint
    fn semantic_to_physics(
        &self,
        semantic: &SemanticPhysicsConstraint,
    ) -> Option<PhysicsConstraint> {
        match semantic {
            SemanticPhysicsConstraint::Separation {
                class_a,
                class_b,
                min_distance,
                strength,
                priority,
            } => {
                let node_a = self.class_to_node.get(class_a)?;
                let node_b = self.class_to_node.get(class_b)?;
                Some(PhysicsConstraint::separation(
                    vec![*node_a, *node_b],
                    *min_distance,
                    *strength,
                    *priority,
                ))
            }
            SemanticPhysicsConstraint::HierarchicalAttraction {
                child_class,
                parent_class,
                ideal_distance,
                strength,
                priority,
            } => {
                let child = self.class_to_node.get(child_class)?;
                let parent = self.class_to_node.get(parent_class)?;
                Some(PhysicsConstraint::clustering(
                    vec![*child, *parent],
                    *ideal_distance,
                    *strength,
                    *priority,
                ))
            }
            SemanticPhysicsConstraint::Colocation {
                class_a,
                class_b,
                target_distance,
                strength,
                priority,
            } => {
                let node_a = self.class_to_node.get(class_a)?;
                let node_b = self.class_to_node.get(class_b)?;
                Some(PhysicsConstraint::colocation(
                    vec![*node_a, *node_b],
                    *target_distance,
                    *strength,
                    *priority,
                ))
            }
            SemanticPhysicsConstraint::Containment {
                child_class,
                parent_class,
                radius,
                strength,
                priority,
            } => {
                let child = self.class_to_node.get(child_class)?;
                let parent = self.class_to_node.get(parent_class)?;
                Some(PhysicsConstraint::containment(
                    vec![*child],
                    *parent,
                    *radius,
                    *strength,
                    *priority,
                ))
            }
            // Alignment and BidirectionalEdge need special handling
            _ => None,
        }
    }

    /// Get hierarchy depth for a node
    pub fn get_hierarchy_depth(&self, node: NodeId) -> usize {
        let mut depth = 0;
        let mut current = node;

        // Traverse up the hierarchy
        loop {
            let parent = self
                .hierarchy_cache
                .iter()
                .find(|(_, children)| children.contains(&current))
                .map(|(parent, _)| *parent);

            match parent {
                Some(p) => {
                    depth += 1;
                    current = p;
                }
                None => break,
            }
        }

        depth
    }
}

impl Default for SemanticAxiomTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disjoint_classes_translation() {
        let mut translator = SemanticAxiomTranslator::new();
        let axiom = OWLAxiom::asserted(AxiomType::DisjointClasses {
            classes: vec![1, 2, 3],
        });

        let constraints = translator.translate_axiom(&axiom);

        // Should create 3 pairwise separation constraints
        assert_eq!(constraints.len(), 3);

        for constraint in &constraints {
            match constraint {
                SemanticPhysicsConstraint::Separation {
                    min_distance,
                    strength,
                    ..
                } => {
                    // Check multiplier is applied (2.0x)
                    assert!(*min_distance > 35.0);
                    assert_eq!(*strength, 0.8);
                }
                _ => panic!("Wrong constraint type"),
            }
        }
    }

    #[test]
    fn test_subclass_of_translation() {
        let mut translator = SemanticAxiomTranslator::new();
        let axiom = OWLAxiom::asserted(AxiomType::SubClassOf {
            subclass: 10,
            superclass: 20,
        });

        let constraints = translator.translate_axiom(&axiom);

        // Should create hierarchical attraction + alignment
        assert!(!constraints.is_empty());

        let has_attraction = constraints
            .iter()
            .any(|c| matches!(c, SemanticPhysicsConstraint::HierarchicalAttraction { .. }));
        assert!(has_attraction);
    }

    #[test]
    fn test_priority_calculation() {
        let translator = SemanticAxiomTranslator::new();

        let user_axiom = OWLAxiom::user_defined(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        });
        assert_eq!(translator.calculate_priority(&user_axiom), 1);

        let inferred_axiom = OWLAxiom::inferred(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        });
        assert_eq!(translator.calculate_priority(&inferred_axiom), 7);

        let asserted_axiom = OWLAxiom::asserted(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        });
        assert_eq!(translator.calculate_priority(&asserted_axiom), 5);
    }

    #[test]
    fn test_equivalent_classes_with_bidirectional() {
        let mut translator = SemanticAxiomTranslator::new();
        let axiom = OWLAxiom::asserted(AxiomType::EquivalentClasses {
            class1: 5,
            class2: 6,
        });

        let constraints = translator.translate_axiom(&axiom);

        // Should have colocation + bidirectional edge
        assert_eq!(constraints.len(), 2);

        let has_colocation = constraints
            .iter()
            .any(|c| matches!(c, SemanticPhysicsConstraint::Colocation { .. }));
        let has_bidirectional = constraints
            .iter()
            .any(|c| matches!(c, SemanticPhysicsConstraint::BidirectionalEdge { .. }));

        assert!(has_colocation);
        assert!(has_bidirectional);
    }

    #[test]
    fn test_part_of_translation() {
        let mut translator = SemanticAxiomTranslator::new();
        let axiom = OWLAxiom::asserted(AxiomType::PartOf {
            part: 10,
            whole: 20,
        });

        let constraints = translator.translate_axiom(&axiom);

        assert_eq!(constraints.len(), 1);
        match &constraints[0] {
            SemanticPhysicsConstraint::Containment {
                radius, strength, ..
            } => {
                assert!(*radius > 0.0);
                assert_eq!(*strength, 0.8);
            }
            _ => panic!("Wrong constraint type"),
        }
    }

    #[test]
    fn test_node_id_mapping() {
        let mut translator = SemanticAxiomTranslator::new();

        let id1 = translator.get_or_create_node_id("ClassA");
        let id2 = translator.get_or_create_node_id("ClassB");
        let id3 = translator.get_or_create_node_id("ClassA"); // Should reuse

        assert_ne!(id1, id2);
        assert_eq!(id1, id3);
    }
}
