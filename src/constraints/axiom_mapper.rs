// Axiom Mapper - OWL Axiom → Physics Constraint Translation
// Week 3 Deliverable: Translation Rules for All Axiom Types

use super::physics_constraint::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum AxiomType {
    SubClassOf {
        subclass: NodeId,
        superclass: NodeId,
    },

    DisjointClasses {
        classes: Vec<NodeId>,
    },

    EquivalentClasses {
        class1: NodeId,
        class2: NodeId,
    },

    SameAs {
        individual1: NodeId,
        individual2: NodeId,
    },

    DifferentFrom {
        individual1: NodeId,
        individual2: NodeId,
    },

    PropertyDomainRange {
        property: NodeId,
        domain: NodeId,
        range: NodeId,
    },

    FunctionalProperty {
        property: NodeId,
        nodes: Vec<NodeId>,
    },

    DisjointUnion {
        union_class: NodeId,
        disjoint_classes: Vec<NodeId>,
    },

    PartOf {
        part: NodeId,
        whole: NodeId,
    },
}

#[derive(Debug, Clone)]
pub struct OWLAxiom {
    pub id: Option<i64>,
    pub axiom_type: AxiomType,
    pub inferred: bool,
    pub user_defined: bool,
}

impl OWLAxiom {
    pub fn asserted(axiom_type: AxiomType) -> Self {
        Self {
            id: None,
            axiom_type,
            inferred: false,
            user_defined: false,
        }
    }

    pub fn inferred(axiom_type: AxiomType) -> Self {
        Self {
            id: None,
            axiom_type,
            inferred: true,
            user_defined: false,
        }
    }

    pub fn user_defined(axiom_type: AxiomType) -> Self {
        Self {
            id: None,
            axiom_type,
            inferred: false,
            user_defined: true,
        }
    }

    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }
}

#[derive(Debug, Clone)]
pub struct TranslationConfig {
    pub disjoint_separation_distance: f32,
    pub disjoint_separation_strength: f32,

    pub subclass_clustering_distance: f32,
    pub subclass_clustering_stiffness: f32,

    pub equivalent_colocation_distance: f32,
    pub equivalent_colocation_strength: f32,

    pub hierarchical_layer_spacing: f32,
    pub hierarchical_layer_strength: f32,

    pub containment_radius_multiplier: f32,
    pub containment_strength: f32,

    pub boundary_size: f32,
    pub boundary_strength: f32,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            disjoint_separation_distance: 35.0,
            disjoint_separation_strength: 0.8,

            subclass_clustering_distance: 20.0,
            subclass_clustering_stiffness: 0.6,

            equivalent_colocation_distance: 2.0,
            equivalent_colocation_strength: 0.9,

            hierarchical_layer_spacing: 50.0,
            hierarchical_layer_strength: 0.7,

            containment_radius_multiplier: 1.5,
            containment_strength: 0.8,

            boundary_size: 20.0,
            boundary_strength: 0.7,
        }
    }
}

pub struct AxiomMapper {
    config: TranslationConfig,

    hierarchy_cache: HashMap<NodeId, Vec<NodeId>>,
}

impl AxiomMapper {
    pub fn new() -> Self {
        Self {
            config: TranslationConfig::default(),
            hierarchy_cache: HashMap::new(),
        }
    }

    pub fn with_config(config: TranslationConfig) -> Self {
        Self {
            config,
            hierarchy_cache: HashMap::new(),
        }
    }

    pub fn update_hierarchy_cache(&mut self, subclass: NodeId, superclass: NodeId) {
        self.hierarchy_cache
            .entry(superclass)
            .or_default()
            .push(subclass);
    }

    pub fn get_subclasses(&self, superclass: NodeId) -> Vec<NodeId> {
        self.hierarchy_cache
            .get(&superclass)
            .cloned()
            .unwrap_or_default()
    }

    pub fn translate_axiom(&mut self, axiom: &OWLAxiom) -> Vec<PhysicsConstraint> {
        let priority = if axiom.user_defined {
            PRIORITY_USER_DEFINED
        } else if axiom.inferred {
            PRIORITY_INFERRED
        } else {
            PRIORITY_ASSERTED
        };

        match &axiom.axiom_type {
            AxiomType::DisjointClasses { classes } => {
                self.translate_disjoint_classes(classes, priority, axiom.id)
            }

            AxiomType::SubClassOf {
                subclass,
                superclass,
            } => self.translate_subclass_of(*subclass, *superclass, priority, axiom.id),

            AxiomType::EquivalentClasses { class1, class2 } => {
                self.translate_equivalent_classes(*class1, *class2, priority, axiom.id)
            }

            AxiomType::SameAs {
                individual1,
                individual2,
            } => self.translate_same_as(*individual1, *individual2, priority, axiom.id),

            AxiomType::DifferentFrom {
                individual1,
                individual2,
            } => self.translate_different_from(*individual1, *individual2, priority, axiom.id),

            AxiomType::PropertyDomainRange {
                property,
                domain,
                range,
            } => {
                self.translate_property_domain_range(*property, *domain, *range, priority, axiom.id)
            }

            AxiomType::FunctionalProperty { property, nodes } => {
                self.translate_functional_property(*property, nodes, priority, axiom.id)
            }

            AxiomType::DisjointUnion {
                union_class,
                disjoint_classes,
            } => self.translate_disjoint_union(*union_class, disjoint_classes, priority, axiom.id),

            AxiomType::PartOf { part, whole } => {
                self.translate_part_of(*part, *whole, priority, axiom.id)
            }
        }
    }

    fn translate_disjoint_classes(
        &self,
        classes: &[NodeId],
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraints = Vec::new();

        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                let mut constraint = PhysicsConstraint::separation(
                    vec![classes[i], classes[j]],
                    self.config.disjoint_separation_distance,
                    self.config.disjoint_separation_strength,
                    priority,
                );

                if let Some(id) = axiom_id {
                    constraint = constraint.with_axiom_id(id);
                }

                constraints.push(constraint);
            }
        }

        constraints
    }

    fn translate_subclass_of(
        &mut self,
        subclass: NodeId,
        superclass: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        self.update_hierarchy_cache(subclass, superclass);

        let mut constraint = PhysicsConstraint::clustering(
            vec![subclass, superclass],
            self.config.subclass_clustering_distance,
            self.config.subclass_clustering_stiffness,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    fn translate_equivalent_classes(
        &self,
        class1: NodeId,
        class2: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraint = PhysicsConstraint::colocation(
            vec![class1, class2],
            self.config.equivalent_colocation_distance,
            self.config.equivalent_colocation_strength,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    fn translate_same_as(
        &self,
        individual1: NodeId,
        individual2: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraint = PhysicsConstraint::colocation(
            vec![individual1, individual2],
            self.config.equivalent_colocation_distance,
            self.config.equivalent_colocation_strength,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    fn translate_different_from(
        &self,
        individual1: NodeId,
        individual2: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraint = PhysicsConstraint::separation(
            vec![individual1, individual2],
            self.config.disjoint_separation_distance,
            self.config.disjoint_separation_strength,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    fn translate_property_domain_range(
        &self,
        _property: NodeId,
        domain: NodeId,
        range: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let bounds = [
            -self.config.boundary_size,
            self.config.boundary_size,
            -self.config.boundary_size,
            self.config.boundary_size,
            -self.config.boundary_size,
            self.config.boundary_size,
        ];

        let mut constraints = vec![
            PhysicsConstraint::boundary(
                vec![domain],
                bounds,
                self.config.boundary_strength,
                priority,
            ),
            PhysicsConstraint::boundary(
                vec![range],
                bounds,
                self.config.boundary_strength,
                priority,
            ),
        ];

        if let Some(id) = axiom_id {
            constraints[0] = constraints[0].clone().with_axiom_id(id);
            constraints[1] = constraints[1].clone().with_axiom_id(id);
        }

        constraints
    }

    fn translate_functional_property(
        &self,
        _property: NodeId,
        nodes: &[NodeId],
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let bounds = [
            -self.config.boundary_size,
            self.config.boundary_size,
            -self.config.boundary_size,
            self.config.boundary_size,
            -self.config.boundary_size,
            self.config.boundary_size,
        ];

        let mut constraint = PhysicsConstraint::boundary(
            nodes.to_vec(),
            bounds,
            self.config.boundary_strength,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    fn translate_disjoint_union(
        &self,
        union_class: NodeId,
        disjoint_classes: &[NodeId],
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraints = Vec::new();

        constraints.extend(self.translate_disjoint_classes(disjoint_classes, priority, axiom_id));

        for &disjoint_class in disjoint_classes {
            let mut constraint = PhysicsConstraint::clustering(
                vec![disjoint_class, union_class],
                self.config.subclass_clustering_distance,
                self.config.subclass_clustering_stiffness,
                priority,
            );

            if let Some(id) = axiom_id {
                constraint = constraint.with_axiom_id(id);
            }

            constraints.push(constraint);
        }

        constraints
    }

    fn translate_part_of(
        &self,
        part: NodeId,
        whole: NodeId,
        priority: u8,
        axiom_id: Option<i64>,
    ) -> Vec<PhysicsConstraint> {
        let mut constraint = PhysicsConstraint::containment(
            vec![part],
            whole,
            self.config.subclass_clustering_distance * self.config.containment_radius_multiplier,
            self.config.containment_strength,
            priority,
        );

        if let Some(id) = axiom_id {
            constraint = constraint.with_axiom_id(id);
        }

        vec![constraint]
    }

    pub fn translate_axioms(&mut self, axioms: &[OWLAxiom]) -> Vec<PhysicsConstraint> {
        axioms
            .iter()
            .flat_map(|axiom| self.translate_axiom(axiom))
            .collect()
    }
}

impl Default for AxiomMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disjoint_classes_translation() {
        let mapper = AxiomMapper::new();
        let _axiom = OWLAxiom::asserted(AxiomType::DisjointClasses {
            classes: vec![1, 2, 3],
        });

        let constraints = mapper.translate_disjoint_classes(&[1, 2, 3], PRIORITY_ASSERTED, None);

        assert_eq!(constraints.len(), 3);

        for constraint in &constraints {
            assert_eq!(constraint.nodes.len(), 2);
            assert_eq!(constraint.priority, PRIORITY_ASSERTED);
            match &constraint.constraint_type {
                PhysicsConstraintType::Separation {
                    min_distance,
                    strength,
                } => {
                    assert_eq!(*min_distance, 35.0);
                    assert_eq!(*strength, 0.8);
                }
                _ => panic!("Wrong constraint type"),
            }
        }
    }

    #[test]
    fn test_subclass_of_translation() {
        let mut mapper = AxiomMapper::new();
        let axiom = OWLAxiom::asserted(AxiomType::SubClassOf {
            subclass: 10,
            superclass: 20,
        });

        let constraints = mapper.translate_axiom(&axiom);

        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].nodes, vec![10, 20]);

        match &constraints[0].constraint_type {
            PhysicsConstraintType::Clustering {
                ideal_distance,
                stiffness,
            } => {
                assert_eq!(*ideal_distance, 20.0);
                assert_eq!(*stiffness, 0.6);
            }
            _ => panic!("Wrong constraint type"),
        }

        assert_eq!(mapper.get_subclasses(20), vec![10]);
    }

    #[test]
    fn test_equivalent_classes_translation() {
        let mapper = AxiomMapper::new();
        let _axiom = OWLAxiom::asserted(AxiomType::EquivalentClasses {
            class1: 5,
            class2: 6,
        });

        let constraints = mapper.translate_equivalent_classes(5, 6, PRIORITY_ASSERTED, None);

        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].nodes, vec![5, 6]);

        match &constraints[0].constraint_type {
            PhysicsConstraintType::Colocation {
                target_distance,
                strength,
            } => {
                assert_eq!(*target_distance, 2.0);
                assert_eq!(*strength, 0.9);
            }
            _ => panic!("Wrong constraint type"),
        }
    }

    #[test]
    fn test_inferred_axiom_priority() {
        let mut mapper = AxiomMapper::new();
        let axiom = OWLAxiom::inferred(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        });

        let constraints = mapper.translate_axiom(&axiom);

        assert_eq!(constraints[0].priority, PRIORITY_INFERRED);
    }

    #[test]
    fn test_user_defined_axiom_priority() {
        let mut mapper = AxiomMapper::new();
        let axiom = OWLAxiom::user_defined(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        });

        let constraints = mapper.translate_axiom(&axiom);

        assert_eq!(constraints[0].priority, PRIORITY_USER_DEFINED);
    }

    #[test]
    fn test_disjoint_union_translation() {
        let mapper = AxiomMapper::new();
        let _axiom = OWLAxiom::asserted(AxiomType::DisjointUnion {
            union_class: 1,
            disjoint_classes: vec![2, 3, 4],
        });

        let constraints = mapper.translate_disjoint_union(1, &[2, 3, 4], PRIORITY_ASSERTED, None);

        assert_eq!(constraints.len(), 6);
    }

    #[test]
    fn test_part_of_translation() {
        let mapper = AxiomMapper::new();
        let _axiom = OWLAxiom::asserted(AxiomType::PartOf {
            part: 10,
            whole: 20,
        });

        let constraints = mapper.translate_part_of(10, 20, PRIORITY_ASSERTED, None);

        assert_eq!(constraints.len(), 1);
        match &constraints[0].constraint_type {
            PhysicsConstraintType::Containment {
                parent_node,
                radius,
                ..
            } => {
                assert_eq!(*parent_node, 20);
                assert!(*radius > 0.0);
            }
            _ => panic!("Wrong constraint type"),
        }
    }

    #[test]
    fn test_batch_translation() {
        let mut mapper = AxiomMapper::new();
        let axioms = vec![
            OWLAxiom::asserted(AxiomType::SubClassOf {
                subclass: 1,
                superclass: 2,
            }),
            OWLAxiom::asserted(AxiomType::DisjointClasses {
                classes: vec![1, 3],
            }),
        ];

        let constraints = mapper.translate_axioms(&axioms);

        assert_eq!(constraints.len(), 2);
    }

    #[test]
    fn test_custom_config() {
        let config = TranslationConfig {
            disjoint_separation_distance: 50.0,
            disjoint_separation_strength: 0.9,
            ..Default::default()
        };

        let mapper = AxiomMapper::with_config(config);
        let constraints = mapper.translate_disjoint_classes(&[1, 2], PRIORITY_ASSERTED, None);

        match &constraints[0].constraint_type {
            PhysicsConstraintType::Separation {
                min_distance,
                strength,
            } => {
                assert_eq!(*min_distance, 50.0);
                assert_eq!(*strength, 0.9);
            }
            _ => panic!("Wrong constraint type"),
        }
    }
}
