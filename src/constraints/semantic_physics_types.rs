// Semantic Physics Types - Enhanced Constraint System
// Semantic-aware physics constraints with axis alignment and bidirectional relationships

use serde::{Deserialize, Serialize};

/// Axis types for alignment constraints
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// X-axis (horizontal)
    X,
    /// Y-axis (vertical)
    Y,
    /// Z-axis (depth)
    Z,
}

/// Enhanced physics constraints with semantic awareness
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SemanticPhysicsConstraint {
    /// Separation constraint - forces nodes apart
    /// Used for DisjointWith axioms
    Separation {
        class_a: String,
        class_b: String,
        min_distance: f32,
        strength: f32,
        /// Priority level (1-10, lower = higher priority)
        priority: u8,
    },

    /// Hierarchical attraction - parent-child relationship
    /// Used for SubClassOf axioms
    HierarchicalAttraction {
        child_class: String,
        parent_class: String,
        ideal_distance: f32,
        strength: f32,
        priority: u8,
    },

    /// Axis alignment - align nodes along specific axis
    /// Used for organizing hierarchies and relationships
    Alignment {
        class_iri: String,
        axis: Axis,
        target_position: f32,
        strength: f32,
        priority: u8,
    },

    /// Bidirectional edge constraint - symmetric relationships
    /// Used for InverseOf and EquivalentTo axioms
    BidirectionalEdge {
        class_a: String,
        class_b: String,
        strength: f32,
        priority: u8,
    },

    /// Colocation - forces nodes to same position
    /// Used for SameAs and EquivalentClasses axioms
    Colocation {
        class_a: String,
        class_b: String,
        target_distance: f32,
        strength: f32,
        priority: u8,
    },

    /// Containment - child must stay within parent radius
    /// Used for PartOf relationships
    Containment {
        child_class: String,
        parent_class: String,
        radius: f32,
        strength: f32,
        priority: u8,
    },
}

impl SemanticPhysicsConstraint {
    /// Get the priority of this constraint
    pub fn priority(&self) -> u8 {
        match self {
            Self::Separation { priority, .. } => *priority,
            Self::HierarchicalAttraction { priority, .. } => *priority,
            Self::Alignment { priority, .. } => *priority,
            Self::BidirectionalEdge { priority, .. } => *priority,
            Self::Colocation { priority, .. } => *priority,
            Self::Containment { priority, .. } => *priority,
        }
    }

    /// Get the strength of this constraint
    pub fn strength(&self) -> f32 {
        match self {
            Self::Separation { strength, .. } => *strength,
            Self::HierarchicalAttraction { strength, .. } => *strength,
            Self::Alignment { strength, .. } => *strength,
            Self::BidirectionalEdge { strength, .. } => *strength,
            Self::Colocation { strength, .. } => *strength,
            Self::Containment { strength, .. } => *strength,
        }
    }

    /// Calculate priority weight (exponential falloff)
    /// Priority 1 = weight 1.0, Priority 10 = weight 0.1
    pub fn priority_weight(&self) -> f32 {
        let p = self.priority() as f32;
        10.0_f32.powf(-(p - 1.0) / 9.0)
    }

    /// Get class IRIs involved in this constraint
    pub fn involved_classes(&self) -> Vec<String> {
        match self {
            Self::Separation { class_a, class_b, .. } => vec![class_a.clone(), class_b.clone()],
            Self::HierarchicalAttraction { child_class, parent_class, .. } => {
                vec![child_class.clone(), parent_class.clone()]
            }
            Self::Alignment { class_iri, .. } => vec![class_iri.clone()],
            Self::BidirectionalEdge { class_a, class_b, .. } => {
                vec![class_a.clone(), class_b.clone()]
            }
            Self::Colocation { class_a, class_b, .. } => vec![class_a.clone(), class_b.clone()],
            Self::Containment { child_class, parent_class, .. } => {
                vec![child_class.clone(), parent_class.clone()]
            }
        }
    }
}

/// Semantic constraint builder for fluent API
pub struct SemanticConstraintBuilder {
    constraint_type: Option<SemanticPhysicsConstraint>,
}

impl SemanticConstraintBuilder {
    pub fn new() -> Self {
        Self {
            constraint_type: None,
        }
    }

    /// Build a separation constraint
    pub fn separation(
        class_a: String,
        class_b: String,
        min_distance: f32,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::Separation {
            class_a,
            class_b,
            min_distance,
            strength,
            priority: 5,
        }
    }

    /// Build a hierarchical attraction constraint
    pub fn hierarchical_attraction(
        child_class: String,
        parent_class: String,
        ideal_distance: f32,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::HierarchicalAttraction {
            child_class,
            parent_class,
            ideal_distance,
            strength,
            priority: 5,
        }
    }

    /// Build an alignment constraint
    pub fn alignment(
        class_iri: String,
        axis: Axis,
        target_position: f32,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::Alignment {
            class_iri,
            axis,
            target_position,
            strength,
            priority: 5,
        }
    }

    /// Build a bidirectional edge constraint
    pub fn bidirectional_edge(
        class_a: String,
        class_b: String,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::BidirectionalEdge {
            class_a,
            class_b,
            strength,
            priority: 5,
        }
    }

    /// Build a colocation constraint
    pub fn colocation(
        class_a: String,
        class_b: String,
        target_distance: f32,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::Colocation {
            class_a,
            class_b,
            target_distance,
            strength,
            priority: 5,
        }
    }

    /// Build a containment constraint
    pub fn containment(
        child_class: String,
        parent_class: String,
        radius: f32,
        strength: f32,
    ) -> SemanticPhysicsConstraint {
        SemanticPhysicsConstraint::Containment {
            child_class,
            parent_class,
            radius,
            strength,
            priority: 5,
        }
    }
}

impl Default for SemanticConstraintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_separation_constraint() {
        let constraint = SemanticConstraintBuilder::separation(
            "ClassA".to_string(),
            "ClassB".to_string(),
            50.0,
            0.8,
        );

        assert_eq!(constraint.priority(), 5);
        assert_eq!(constraint.strength(), 0.8);

        let classes = constraint.involved_classes();
        assert_eq!(classes.len(), 2);
        assert!(classes.contains(&"ClassA".to_string()));
    }

    #[test]
    fn test_hierarchical_attraction() {
        let constraint = SemanticConstraintBuilder::hierarchical_attraction(
            "Child".to_string(),
            "Parent".to_string(),
            30.0,
            0.6,
        );

        match constraint {
            SemanticPhysicsConstraint::HierarchicalAttraction { ideal_distance, .. } => {
                assert_eq!(ideal_distance, 30.0);
            }
            _ => panic!("Wrong constraint type"),
        }
    }

    #[test]
    fn test_alignment_constraint() {
        let constraint = SemanticConstraintBuilder::alignment(
            "ClassA".to_string(),
            Axis::X,
            100.0,
            0.7,
        );

        match constraint {
            SemanticPhysicsConstraint::Alignment { axis, target_position, .. } => {
                assert_eq!(axis, Axis::X);
                assert_eq!(target_position, 100.0);
            }
            _ => panic!("Wrong constraint type"),
        }
    }

    #[test]
    fn test_priority_weight() {
        let c1 = SemanticPhysicsConstraint::Separation {
            class_a: "A".to_string(),
            class_b: "B".to_string(),
            min_distance: 50.0,
            strength: 0.8,
            priority: 1,
        };

        let c10 = SemanticPhysicsConstraint::Separation {
            class_a: "A".to_string(),
            class_b: "B".to_string(),
            min_distance: 50.0,
            strength: 0.8,
            priority: 10,
        };

        assert!((c1.priority_weight() - 1.0).abs() < 0.001);
        assert!((c10.priority_weight() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_bidirectional_edge() {
        let constraint = SemanticConstraintBuilder::bidirectional_edge(
            "PropertyA".to_string(),
            "PropertyB".to_string(),
            0.9,
        );

        assert_eq!(constraint.strength(), 0.9);
        assert_eq!(constraint.involved_classes().len(), 2);
    }
}
