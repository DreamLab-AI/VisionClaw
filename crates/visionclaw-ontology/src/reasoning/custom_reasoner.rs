use crate::reasoning::ReasoningResult;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub trait OntologyReasoner: Send + Sync {
    fn infer_axioms(&self, ontology: &Ontology) -> ReasoningResult<Vec<InferredAxiom>>;

    fn is_subclass_of(&self, child: &str, parent: &str, ontology: &Ontology) -> bool;

    fn are_disjoint(&self, class_a: &str, class_b: &str, ontology: &Ontology) -> bool;
}

#[derive(Debug, Clone, Default)]
pub struct Ontology {
    pub classes: HashMap<String, OWLClass>,

    pub subclass_of: HashMap<String, HashSet<String>>,

    pub disjoint_classes: Vec<HashSet<String>>,

    pub equivalent_classes: HashMap<String, HashSet<String>>,

    pub functional_properties: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OWLClass {
    pub iri: String,
    pub label: Option<String>,
    pub parent_class_iri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferredAxiom {
    pub axiom_type: AxiomType,
    pub subject: String,
    pub object: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AxiomType {
    SubClassOf,
    DisjointWith,
    EquivalentTo,
    FunctionalProperty,
}

pub struct CustomReasoner {
    transitive_cache: HashMap<String, HashSet<String>>,
}

impl CustomReasoner {
    pub fn new() -> Self {
        Self {
            transitive_cache: HashMap::new(),
        }
    }

    fn compute_transitive_closure(&mut self, ontology: &Ontology) {
        self.transitive_cache.clear();

        for child in ontology.classes.keys() {
            let mut ancestors = HashSet::new();
            self.collect_ancestors(child, ontology, &mut ancestors);
            self.transitive_cache.insert(child.clone(), ancestors);
        }
    }

    fn collect_ancestors(&self, class: &str, ontology: &Ontology, ancestors: &mut HashSet<String>) {
        if let Some(parents) = ontology.subclass_of.get(class) {
            for parent in parents {
                if ancestors.insert(parent.clone()) {
                    self.collect_ancestors(parent, ontology, ancestors);
                }
            }
        }
    }

    fn infer_transitive_subclass(&mut self, ontology: &Ontology) -> Vec<InferredAxiom> {
        let mut inferred = Vec::new();

        self.compute_transitive_closure(ontology);

        for (child, ancestors) in &self.transitive_cache {
            let direct_parents = ontology.subclass_of.get(child).cloned().unwrap_or_default();

            for ancestor in ancestors {
                if !direct_parents.contains(ancestor) {
                    inferred.push(InferredAxiom {
                        axiom_type: AxiomType::SubClassOf,
                        subject: child.clone(),
                        object: Some(ancestor.clone()),
                        confidence: 1.0,
                    });
                }
            }
        }

        inferred
    }

    fn infer_disjoint(&self, ontology: &Ontology) -> Vec<InferredAxiom> {
        let mut inferred = Vec::new();

        for disjoint_set in &ontology.disjoint_classes {
            let classes: Vec<_> = disjoint_set.iter().collect();

            for i in 0..classes.len() {
                for j in (i + 1)..classes.len() {
                    let class_a = classes[i];
                    let class_b = classes[j];

                    if let Some(a_subclasses) = self.get_all_subclasses(class_a, ontology) {
                        for subclass in &a_subclasses {
                            if subclass != class_a && !disjoint_set.contains(subclass.as_str()) {
                                inferred.push(InferredAxiom {
                                    axiom_type: AxiomType::DisjointWith,
                                    subject: subclass.clone(),
                                    object: Some(class_b.to_string()),
                                    confidence: 1.0,
                                });
                            }
                        }
                    }

                    if let Some(b_subclasses) = self.get_all_subclasses(class_b, ontology) {
                        for subclass in &b_subclasses {
                            if subclass != class_b && !disjoint_set.contains(subclass.as_str()) {
                                inferred.push(InferredAxiom {
                                    axiom_type: AxiomType::DisjointWith,
                                    subject: subclass.clone(),
                                    object: Some(class_a.to_string()),
                                    confidence: 1.0,
                                });
                            }
                        }
                    }
                }
            }
        }

        inferred
    }

    fn get_all_subclasses(&self, class: &str, ontology: &Ontology) -> Option<HashSet<String>> {
        let mut subclasses = HashSet::new();

        for (child, parents) in &ontology.subclass_of {
            if parents.contains(class) {
                subclasses.insert(child.clone());

                if let Some(child_subclasses) = self.get_all_subclasses(child, ontology) {
                    subclasses.extend(child_subclasses);
                }
            }
        }

        if subclasses.is_empty() {
            None
        } else {
            Some(subclasses)
        }
    }

    fn infer_equivalent(&self, ontology: &Ontology) -> Vec<InferredAxiom> {
        let mut inferred = Vec::new();

        for (class_a, equivalents) in &ontology.equivalent_classes {
            for class_b in equivalents {
                if !ontology
                    .equivalent_classes
                    .get(class_b)
                    .map(|set| set.contains(class_a))
                    .unwrap_or(false)
                {
                    inferred.push(InferredAxiom {
                        axiom_type: AxiomType::EquivalentTo,
                        subject: class_b.clone(),
                        object: Some(class_a.clone()),
                        confidence: 1.0,
                    });
                }

                if let Some(b_equivalents) = ontology.equivalent_classes.get(class_b) {
                    for class_c in b_equivalents {
                        if class_c != class_a && !equivalents.contains(class_c) {
                            inferred.push(InferredAxiom {
                                axiom_type: AxiomType::EquivalentTo,
                                subject: class_a.clone(),
                                object: Some(class_c.clone()),
                                confidence: 1.0,
                            });
                        }
                    }
                }
            }
        }

        inferred
    }
}

impl Default for CustomReasoner {
    fn default() -> Self {
        Self::new()
    }
}

impl OntologyReasoner for CustomReasoner {
    fn infer_axioms(&self, ontology: &Ontology) -> ReasoningResult<Vec<InferredAxiom>> {
        let mut reasoner = Self::new();
        let mut all_inferred = Vec::new();

        all_inferred.extend(reasoner.infer_transitive_subclass(ontology));

        all_inferred.extend(reasoner.infer_disjoint(ontology));

        all_inferred.extend(reasoner.infer_equivalent(ontology));

        Ok(all_inferred)
    }

    fn is_subclass_of(&self, child: &str, parent: &str, ontology: &Ontology) -> bool {
        if let Some(parents) = ontology.subclass_of.get(child) {
            if parents.contains(parent) {
                return true;
            }
        }

        if let Some(ancestors) = self.transitive_cache.get(child) {
            return ancestors.contains(parent);
        }

        let mut visited = HashSet::new();
        self.is_subclass_of_recursive(child, parent, ontology, &mut visited)
    }

    fn are_disjoint(&self, class_a: &str, class_b: &str, ontology: &Ontology) -> bool {
        for disjoint_set in &ontology.disjoint_classes {
            if disjoint_set.contains(class_a) && disjoint_set.contains(class_b) {
                return true;
            }
        }
        false
    }
}

impl CustomReasoner {
    fn is_subclass_of_recursive(
        &self,
        child: &str,
        parent: &str,
        ontology: &Ontology,
        visited: &mut HashSet<String>,
    ) -> bool {
        if child == parent {
            return true;
        }

        if !visited.insert(child.to_string()) {
            return false;
        }

        if let Some(parents) = ontology.subclass_of.get(child) {
            for p in parents {
                if self.is_subclass_of_recursive(p, parent, ontology, visited) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_ontology() -> Ontology {
        let mut ontology = Ontology::default();

        ontology.classes.insert(
            "Entity".to_string(),
            OWLClass {
                iri: "Entity".to_string(),
                label: Some("Entity".to_string()),
                parent_class_iri: None,
            },
        );

        ontology.classes.insert(
            "MaterialEntity".to_string(),
            OWLClass {
                iri: "MaterialEntity".to_string(),
                label: Some("Material Entity".to_string()),
                parent_class_iri: Some("Entity".to_string()),
            },
        );

        ontology.classes.insert(
            "Cell".to_string(),
            OWLClass {
                iri: "Cell".to_string(),
                label: Some("Cell".to_string()),
                parent_class_iri: Some("MaterialEntity".to_string()),
            },
        );

        ontology.classes.insert(
            "Neuron".to_string(),
            OWLClass {
                iri: "Neuron".to_string(),
                label: Some("Neuron".to_string()),
                parent_class_iri: Some("Cell".to_string()),
            },
        );

        ontology.classes.insert(
            "Astrocyte".to_string(),
            OWLClass {
                iri: "Astrocyte".to_string(),
                label: Some("Astrocyte".to_string()),
                parent_class_iri: Some("Cell".to_string()),
            },
        );

        ontology.subclass_of.insert(
            "MaterialEntity".to_string(),
            vec!["Entity".to_string()].into_iter().collect(),
        );
        ontology.subclass_of.insert(
            "Cell".to_string(),
            vec!["MaterialEntity".to_string()].into_iter().collect(),
        );
        ontology.subclass_of.insert(
            "Neuron".to_string(),
            vec!["Cell".to_string()].into_iter().collect(),
        );
        ontology.subclass_of.insert(
            "Astrocyte".to_string(),
            vec!["Cell".to_string()].into_iter().collect(),
        );

        ontology.disjoint_classes.push(
            vec!["Neuron".to_string(), "Astrocyte".to_string()]
                .into_iter()
                .collect(),
        );

        ontology
    }

    #[test]
    fn test_transitive_subclass() {
        let ontology = create_test_ontology();
        let mut reasoner = CustomReasoner::new();

        let inferred = reasoner.infer_transitive_subclass(&ontology);

        assert!(inferred
            .iter()
            .any(|axiom| axiom.axiom_type == AxiomType::SubClassOf
                && axiom.subject == "Neuron"
                && axiom.object.as_ref() == Some(&"MaterialEntity".to_string())));

        assert!(inferred
            .iter()
            .any(|axiom| axiom.axiom_type == AxiomType::SubClassOf
                && axiom.subject == "Neuron"
                && axiom.object.as_ref() == Some(&"Entity".to_string())));
    }

    #[test]
    fn test_is_subclass_of() {
        let ontology = create_test_ontology();
        let mut reasoner = CustomReasoner::new();
        reasoner.compute_transitive_closure(&ontology);

        assert!(reasoner.is_subclass_of("Neuron", "Cell", &ontology));
        assert!(reasoner.is_subclass_of("Neuron", "MaterialEntity", &ontology));
        assert!(reasoner.is_subclass_of("Neuron", "Entity", &ontology));
        assert!(!reasoner.is_subclass_of("Cell", "Neuron", &ontology));
    }

    #[test]
    fn test_disjoint_inference() {
        let ontology = create_test_ontology();
        let reasoner = CustomReasoner::new();

        let inferred = reasoner.infer_disjoint(&ontology);

        assert_eq!(inferred.len(), 0);
    }

    #[test]
    fn test_are_disjoint() {
        let ontology = create_test_ontology();
        let reasoner = CustomReasoner::new();

        assert!(reasoner.are_disjoint("Neuron", "Astrocyte", &ontology));
        assert!(reasoner.are_disjoint("Astrocyte", "Neuron", &ontology));
        assert!(!reasoner.are_disjoint("Neuron", "Cell", &ontology));
    }

    #[test]
    fn test_equivalent_class_inference() {
        let mut ontology = Ontology::default();

        ontology
            .equivalent_classes
            .insert("A".to_string(), vec!["B".to_string()].into_iter().collect());
        ontology
            .equivalent_classes
            .insert("B".to_string(), vec!["C".to_string()].into_iter().collect());

        let reasoner = CustomReasoner::new();
        let inferred = reasoner.infer_equivalent(&ontology);

        assert!(inferred
            .iter()
            .any(|axiom| axiom.axiom_type == AxiomType::EquivalentTo
                && axiom.subject == "B"
                && axiom.object.as_ref() == Some(&"A".to_string())));

        assert!(inferred
            .iter()
            .any(|axiom| axiom.axiom_type == AxiomType::EquivalentTo
                && axiom.subject == "A"
                && axiom.object.as_ref() == Some(&"C".to_string())));
    }
}
