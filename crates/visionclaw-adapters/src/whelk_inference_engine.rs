// src/adapters/whelk_inference_engine.rs
//! Whelk Inference Engine Adapter
//!
//! Implements the InferenceEngine port using horned-owl for OWL ontology loading
//! and whelk-rs for EL reasoning. This adapter provides complete EL reasoning capabilities.

use async_trait::async_trait;
use tracing::{debug, info, instrument, warn};

use visionclaw_domain::ports::inference_engine::{
    InferenceEngine, InferenceEngineError, InferenceStatistics, Result as EngineResult,
};
use visionclaw_domain::ports::owl_types::{AxiomType, InferenceResults, OwlAxiom, OwlClass};

use horned_owl::model::{
    AnnotatedComponent, ArcStr, Build, Class, ClassExpression, Component, DeclareClass,
    MutableOntology, ObjectProperty, ObjectPropertyExpression, SubClassOf,
    SubObjectPropertyExpression, SubObjectPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct WhelkInferenceEngine {
    ontology: Option<SetOntology<ArcStr>>,

    cached_subsumptions: Option<Vec<OwlAxiom>>,

    last_checksum: Option<u64>,

    /// Bumped on every write to `cached_subsumptions`; see [`Self::generation`].
    generation: u64,

    _phantom: std::marker::PhantomData<()>,

    loaded_classes: usize,
    loaded_axioms: usize,
    inferred_axioms: usize,
    last_inference_time_ms: u64,
    total_inferences: usize,
}

use visionclaw_domain::utils::time;
use whelk;

/// Outcome of a self-contained EL++ consistency check over an explicit
/// class + axiom set (GOV-7 / ADR-130 Decision 2). `consistent == false` means
/// whelk subsumed one or more named classes under `owl:Nothing` — the
/// unsatisfiable set is carried so the caller can explain *why* the check
/// blocked (e.g. the elevation gate's reject reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsistencyOutcome {
    pub consistent: bool,
    /// IRIs of the classes whelk subsumed under `owl:Nothing`, sorted+deduped.
    pub unsatisfiable_classes: Vec<String>,
}

impl ConsistencyOutcome {
    /// One-line human-legible explanation (reject reason / decision record).
    pub fn explanation(&self) -> String {
        if self.consistent {
            "EL++ consistency check passed: no class subsumed under owl:Nothing.".to_string()
        } else {
            format!(
                "EL++ inconsistency: {} class(es) became unsatisfiable (owl:Nothing): {}",
                self.unsatisfiable_classes.len(),
                self.unsatisfiable_classes.join(", ")
            )
        }
    }
}

impl WhelkInferenceEngine {
    /// A counter that moves whenever the cached closure is replaced or
    /// dropped — and so whenever `get_subclass_hierarchy` can answer
    /// differently — and never otherwise. A caller that derives state from
    /// the hierarchy keys its cache on this instead of re-reading and hashing
    /// the whole closure (ADR-2127 query service).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn new() -> Self {
        info!("Initializing WhelkInferenceEngine");
        Self {
            ontology: None,

            cached_subsumptions: None,

            last_checksum: None,

            generation: 0,

            _phantom: std::marker::PhantomData,

            loaded_classes: 0,
            loaded_axioms: 0,
            inferred_axioms: 0,
            last_inference_time_ms: 0,
            total_inferences: 0,
        }
    }

    fn convert_class_to_horned(class: &OwlClass) -> Option<AnnotatedComponent<ArcStr>> {
        let iri = Build::new().iri(class.iri.clone());
        let class_decl = Class(iri);
        Some(AnnotatedComponent {
            component: Component::DeclareClass(DeclareClass(class_decl)),
            ann: Default::default(),
        })
    }

    fn convert_axiom_to_horned(axiom: &OwlAxiom) -> Option<AnnotatedComponent<ArcStr>> {
        let component = match axiom.axiom_type {
            AxiomType::SubClassOf => {
                let sub_iri = Build::new().iri(axiom.subject.clone());
                let sup_iri = Build::new().iri(axiom.object.clone());

                let sub_class = ClassExpression::Class(Class(sub_iri));
                let sup_class = ClassExpression::Class(Class(sup_iri));

                Component::SubClassOf(SubClassOf {
                    sub: sub_class,
                    sup: sup_class,
                })
            }
            AxiomType::EquivalentClass => {
                // ADR-099 D2: preserve equivalence bidirectionally. Whelk's EL
                // classifier derives BOTH `A ⊑ B` and `B ⊑ A` from a single
                // `EquivalentClasses(A, B)` axiom, so we emit the native
                // equivalence component instead of the lossy single-direction
                // SubClassOf the old `:86-87` path used.
                let a_iri = Build::new().iri(axiom.subject.clone());
                let b_iri = Build::new().iri(axiom.object.clone());
                Component::EquivalentClasses(horned_owl::model::EquivalentClasses(vec![
                    ClassExpression::Class(Class(a_iri)),
                    ClassExpression::Class(Class(b_iri)),
                ]))
            }
            AxiomType::DisjointWith => {
                // ADR-099 D3: EL-derivable disjointness. Disjoint classes that
                // are also subsumption-related collapse to owl:Nothing, which
                // `check_consistency` surfaces and which whelk can entail.
                let a_iri = Build::new().iri(axiom.subject.clone());
                let b_iri = Build::new().iri(axiom.object.clone());
                Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
                    ClassExpression::Class(Class(a_iri)),
                    ClassExpression::Class(Class(b_iri)),
                ]))
            }
            AxiomType::SubPropertyOf => {
                let sub_iri = Build::new().iri(axiom.subject.clone());
                let sup_iri = Build::new().iri(axiom.object.clone());
                Component::SubObjectPropertyOf(SubObjectPropertyOf {
                    sub: SubObjectPropertyExpression::ObjectPropertyExpression(
                        ObjectPropertyExpression::ObjectProperty(ObjectProperty(sub_iri)),
                    ),
                    sup: ObjectPropertyExpression::ObjectProperty(ObjectProperty(sup_iri)),
                })
            }
            AxiomType::TransitiveProperty => {
                let prop_iri = Build::new().iri(axiom.subject.clone());
                Component::TransitiveObjectProperty(horned_owl::model::TransitiveObjectProperty(
                    ObjectPropertyExpression::ObjectProperty(ObjectProperty(prop_iri)),
                ))
            }
            AxiomType::SymmetricProperty => {
                let prop_iri = Build::new().iri(axiom.subject.clone());
                Component::SymmetricObjectProperty(horned_owl::model::SymmetricObjectProperty(
                    ObjectPropertyExpression::ObjectProperty(ObjectProperty(prop_iri)),
                ))
            }
            AxiomType::InverseProperties => {
                let prop1_iri = Build::new().iri(axiom.subject.clone());
                let prop2_iri = Build::new().iri(axiom.object.clone());
                Component::InverseObjectProperties(horned_owl::model::InverseObjectProperties(
                    ObjectProperty(prop1_iri),
                    ObjectProperty(prop2_iri),
                ))
            }
            AxiomType::SomeValuesFrom => {
                // Existential restriction: subject subClassOf (property some filler)
                // axiom.subject = class IRI, axiom.object = filler IRI
                // The property IRI is stored in annotations["property"]
                let sub_iri = Build::new().iri(axiom.subject.clone());
                let filler_iri = Build::new().iri(axiom.object.clone());
                let prop_iri_str = axiom
                    .annotations
                    .get("property")
                    .cloned()
                    .unwrap_or_else(|| axiom.object.clone());
                let prop_iri = Build::new().iri(prop_iri_str);

                Component::SubClassOf(SubClassOf {
                    sub: ClassExpression::Class(Class(sub_iri)),
                    sup: ClassExpression::ObjectSomeValuesFrom {
                        ope: ObjectPropertyExpression::ObjectProperty(ObjectProperty(prop_iri)),
                        bce: Box::new(ClassExpression::Class(Class(filler_iri))),
                    },
                })
            }
            AxiomType::ObjectPropertyAssertion => {
                // Mereological/associative assertions (hasPart, partOf, sameAs …) are
                // not EL Tbox axioms; they drive GPU constraint forces directly via the
                // mapper. Skip them here without log spam (5589+ hasPart triples flow through).
                debug!("ObjectPropertyAssertion skipped for EL Tbox (drives forces directly)");
                return None;
            }
            _ => {
                warn!("Unsupported axiom type: {:?}", axiom.axiom_type);
                return None;
            }
        };

        Some(AnnotatedComponent {
            component,
            ann: Default::default(),
        })
    }

    fn compute_ontology_checksum(ontology: &SetOntology<ArcStr>) -> u64 {
        let mut hasher = DefaultHasher::new();

        let mut axioms: Vec<String> = ontology
            .iter()
            .map(|ann| format!("{:?}", ann.component))
            .collect();
        axioms.sort();

        for axiom in axioms {
            axiom.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Convert whelk's named subsumptions into materialisable axioms.
    ///
    /// ADR-099 D2/D3: in addition to the directed `SubClassOf` closure, this
    /// detects *bidirectional* subsumptions (`A ⊑ B` AND `B ⊑ A`) and emits a
    /// single canonical `EquivalentClass(A, B)` axiom for each such pair so the
    /// store preserves equivalence rather than two opaque sub-class edges. The
    /// directed `SubClassOf` axioms are retained (asserted-vs-inferred and the
    /// transitive closure both remain queryable); the equivalence axiom is an
    /// additional, lossless marker.
    fn convert_subsumptions_to_axioms<V>(subsumptions: &V) -> Vec<OwlAxiom>
    where
        V: IntoIterator<
                Item = (
                    std::rc::Rc<whelk::whelk::model::AtomicConcept>,
                    std::rc::Rc<whelk::whelk::model::AtomicConcept>,
                ),
            > + Clone,
    {
        // owl:Thing / owl:Nothing sentinels are not real equivalences.
        const TOP: &str = "http://www.w3.org/2002/07/owl#Thing";
        const BOTTOM: &str = "http://www.w3.org/2002/07/owl#Nothing";

        let pairs: Vec<(String, String)> = subsumptions
            .clone()
            .into_iter()
            .map(|(sub, sup)| (sub.id.clone(), sup.id.clone()))
            .collect();

        let directed: std::collections::HashSet<(String, String)> = pairs.iter().cloned().collect();

        let mut out: Vec<OwlAxiom> = pairs
            .iter()
            .map(|(sub, sup)| OwlAxiom {
                id: None,
                axiom_type: AxiomType::SubClassOf,
                subject: sub.clone(),
                object: sup.clone(),
                annotations: std::collections::HashMap::new(),
            })
            .collect();

        // Emit one canonical EquivalentClass per bidirectional, non-reflexive,
        // non-sentinel pair. Canonical ordering (subject < object) + a seen-set
        // dedupes the symmetric match.
        let mut emitted: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (a, b) in &pairs {
            if a == b || a == TOP || b == TOP || a == BOTTOM || b == BOTTOM {
                continue;
            }
            if directed.contains(&(b.clone(), a.clone())) {
                let (lo, hi) = if a <= b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                if emitted.insert((lo.clone(), hi.clone())) {
                    out.push(OwlAxiom {
                        id: None,
                        axiom_type: AxiomType::EquivalentClass,
                        subject: lo,
                        object: hi,
                        annotations: std::collections::HashMap::new(),
                    });
                }
            }
        }
        out
    }

    /// Self-contained EL++ consistency check over an explicit class + axiom set.
    ///
    /// The trait's [`InferenceEngine::check_consistency`] reads the *cached*
    /// subsumptions of an engine already advanced through `load_ontology` +
    /// `infer` (both `&mut self`), which an `Arc<_>`-shared, immutably-held
    /// engine cannot reach and which `SetOntology` (not `Clone`) cannot snapshot
    /// non-destructively. This associated method instead builds a FRESH whelk
    /// reasoner over the supplied classes+axioms and inspects the named
    /// subsumptions directly — no `self`, no cached state — so it is the public,
    /// re-entrant entry point the GOV-7 elevation gate uses to check
    /// `base-ontology ∪ draft` before a PR is opened. A class whelk subsumes
    /// under `owl:Nothing` marks the combined set inconsistent.
    pub fn check_axiom_set(classes: &[OwlClass], axioms: &[OwlAxiom]) -> ConsistencyOutcome {
        const BOTTOM: &str = "http://www.w3.org/2002/07/owl#Nothing";

        let mut ontology = SetOntology::new();
        for class in classes {
            if let Some(c) = Self::convert_class_to_horned(class) {
                ontology.insert(c);
            }
        }
        for axiom in axioms {
            if let Some(a) = Self::convert_axiom_to_horned(axiom) {
                ontology.insert(a);
            }
        }

        let whelk_axioms = whelk::whelk::owl::translate_ontology(&ontology);
        let reasoner_state = whelk::whelk::reasoner::assert(&whelk_axioms);
        let subsumptions = reasoner_state.named_subsumptions();

        let mut unsatisfiable_classes: Vec<String> = subsumptions
            .iter()
            .filter(|(sub, sup)| sup.id == BOTTOM && sub.id != BOTTOM)
            .map(|(sub, _)| sub.id.clone())
            .collect();
        unsatisfiable_classes.sort();
        unsatisfiable_classes.dedup();

        ConsistencyOutcome {
            consistent: unsatisfiable_classes.is_empty(),
            unsatisfiable_classes,
        }
    }
}

impl Default for WhelkInferenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl InferenceEngine for WhelkInferenceEngine {
    #[instrument(skip(self, classes, axioms), fields(classes = classes.len(), axioms = axioms.len()), level = "debug")]
    async fn load_ontology(
        &mut self,
        classes: Vec<OwlClass>,
        axioms: Vec<OwlAxiom>,
    ) -> EngineResult<()> {
        {
            let mut ontology = SetOntology::new();

            for class in &classes {
                if let Some(horned_class) = Self::convert_class_to_horned(class) {
                    ontology.insert(horned_class);
                }
            }

            for axiom in &axioms {
                if let Some(horned_axiom) = Self::convert_axiom_to_horned(axiom) {
                    ontology.insert(horned_axiom);
                }
            }

            let checksum = Self::compute_ontology_checksum(&ontology);

            let needs_reasoning = match self.last_checksum {
                Some(last) => last != checksum,
                None => true,
            };

            if needs_reasoning {
                info!("Ontology changed, will perform fresh reasoning");
                self.last_checksum = Some(checksum);
                self.cached_subsumptions = None;
                self.generation += 1;
            } else {
                info!("Ontology unchanged, reusing cached reasoning results");
            }

            self.ontology = Some(ontology);
            self.loaded_classes = classes.len();
            self.loaded_axioms = axioms.len();

            info!(
                "Loaded ontology with {} classes and {} axioms",
                classes.len(),
                axioms.len()
            );
            Ok(())
        }
    }

    #[instrument(skip(self), level = "debug")]
    async fn infer(&mut self) -> EngineResult<InferenceResults> {
        let start = std::time::Instant::now();

        {
            let ontology = self
                .ontology
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            if let Some(ref cached) = self.cached_subsumptions {
                info!("Using cached reasoning results");

                let inference_time_ms = start.elapsed().as_millis() as u64;
                self.last_inference_time_ms = inference_time_ms;

                return Ok(InferenceResults {
                    timestamp: time::now(),
                    inferred_axioms: cached.clone(),
                    inference_time_ms,
                    reasoner_version: format!("whelk-rs-{}", env!("CARGO_PKG_VERSION")),
                });
            }

            info!("Performing EL reasoning with whelk-rs");

            let whelk_axioms = whelk::whelk::owl::translate_ontology(ontology);
            debug!("Translated {} axioms to whelk format", whelk_axioms.len());

            let reasoner_state = whelk::whelk::reasoner::assert(&whelk_axioms);

            let subsumptions = reasoner_state.named_subsumptions();
            info!("Inferred {} subsumption relationships", subsumptions.len());

            let inferred_axioms = Self::convert_subsumptions_to_axioms(&subsumptions);
            self.inferred_axioms = inferred_axioms.len();

            self.cached_subsumptions = Some(inferred_axioms.clone());
            self.generation += 1;
            self.total_inferences += 1;

            let inference_time_ms = start.elapsed().as_millis() as u64;
            self.last_inference_time_ms = inference_time_ms;

            info!(
                "EL reasoning completed in {}ms with {} inferred axioms",
                inference_time_ms,
                inferred_axioms.len()
            );

            Ok(InferenceResults {
                timestamp: time::now(),
                inferred_axioms,
                inference_time_ms,
                reasoner_version: format!("whelk-rs-{}", env!("CARGO_PKG_VERSION")),
            })
        }
    }

    async fn is_entailed(&self, axiom: &OwlAxiom) -> EngineResult<bool> {
        {
            let cached = self
                .cached_subsumptions
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            if axiom.axiom_type == AxiomType::SubClassOf {
                let is_entailed = cached.iter().any(|inferred| {
                    inferred.axiom_type == AxiomType::SubClassOf
                        && inferred.subject == axiom.subject
                        && inferred.object == axiom.object
                });

                return Ok(is_entailed);
            }

            Ok(false)
        }
    }

    async fn get_subclass_hierarchy(&self) -> EngineResult<Vec<(String, String)>> {
        {
            let cached = self
                .cached_subsumptions
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            let hierarchy: Vec<(String, String)> = cached
                .iter()
                .filter(|ax| ax.axiom_type == AxiomType::SubClassOf)
                .map(|ax| (ax.subject.clone(), ax.object.clone()))
                .collect();

            debug!("Extracted {} subsumption relationships", hierarchy.len());
            Ok(hierarchy)
        }
    }

    async fn classify_instance(&self, instance_iri: &str) -> EngineResult<Vec<String>> {
        {
            let cached = self
                .cached_subsumptions
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            let class_iris: Vec<String> = cached
                .iter()
                .filter(|ax| ax.axiom_type == AxiomType::SubClassOf && ax.subject == instance_iri)
                .map(|ax| ax.object.clone())
                .collect();

            debug!(
                "Instance {} belongs to {} classes",
                instance_iri,
                class_iris.len()
            );
            Ok(class_iris)
        }
    }

    async fn check_consistency(&self) -> EngineResult<bool> {
        {
            let cached = self
                .cached_subsumptions
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            let bottom_iri = "http://www.w3.org/2002/07/owl#Nothing";

            let inconsistent_classes: Vec<&OwlAxiom> = cached
                .iter()
                .filter(|ax| {
                    ax.axiom_type == AxiomType::SubClassOf
                        && ax.object == bottom_iri
                        && ax.subject != bottom_iri
                })
                .collect();

            if !inconsistent_classes.is_empty() {
                warn!(
                    "Ontology is inconsistent: {} classes are equivalent to owl:Nothing",
                    inconsistent_classes.len()
                );
                return Ok(false);
            }

            info!("Ontology is consistent");
            Ok(true)
        }
    }

    async fn explain_entailment(&self, axiom: &OwlAxiom) -> EngineResult<Vec<OwlAxiom>> {
        {
            if axiom.axiom_type != AxiomType::SubClassOf {
                return Ok(Vec::new());
            }

            let cached = self
                .cached_subsumptions
                .as_ref()
                .ok_or(InferenceEngineError::OntologyNotLoaded)?;

            let mut explanation = Vec::new();

            for inferred in cached.iter() {
                if inferred.subject == axiom.subject && inferred.axiom_type == AxiomType::SubClassOf
                {
                    explanation.push(inferred.clone());
                }
            }

            debug!("Found {} axioms in explanation", explanation.len());
            Ok(explanation)
        }
    }

    async fn clear(&mut self) -> EngineResult<()> {
        {
            self.ontology = None;
            self.cached_subsumptions = None;
            self.last_checksum = None;
            self.generation += 1;
        }

        self.loaded_classes = 0;
        self.loaded_axioms = 0;
        self.inferred_axioms = 0;

        info!("Cleared ontology from inference engine");
        Ok(())
    }

    async fn get_statistics(&self) -> EngineResult<InferenceStatistics> {
        Ok(InferenceStatistics {
            loaded_classes: self.loaded_classes,
            loaded_axioms: self.loaded_axioms,
            inferred_axioms: self.inferred_axioms,
            last_inference_time_ms: self.last_inference_time_ms,
            total_inferences: self.total_inferences as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    /// The generation counter moves whenever the cached closure (and so
    /// `get_subclass_hierarchy`) can have changed, and only then.
    #[tokio::test]
    async fn generation_moves_with_the_cached_closure() {
        let class = |iri: &str| OwlClass {
            iri: iri.into(),
            ..Default::default()
        };
        let sub = |a: &str, b: &str| OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: a.into(),
            object: b.into(),
            annotations: std::collections::HashMap::new(),
        };
        let mut engine = WhelkInferenceEngine::new();
        let g0 = engine.generation();
        let classes = [class("urn:a"), class("urn:b"), class("urn:c")];
        let axioms = [sub("urn:a", "urn:b"), sub("urn:b", "urn:c")];
        engine
            .load_ontology(classes.to_vec(), axioms.to_vec())
            .await
            .unwrap();
        let g1 = engine.generation();
        assert_ne!(g0, g1, "a changed ontology drops the cached closure");
        engine.infer().await.unwrap();
        let g2 = engine.generation();
        assert_ne!(g1, g2, "inference fills the cache");
        engine.infer().await.unwrap();
        assert_eq!(engine.generation(), g2, "a cache hit changes nothing");
        engine
            .load_ontology(classes.to_vec(), axioms.to_vec())
            .await
            .unwrap();
        assert_eq!(engine.generation(), g2, "identical reload keeps the cache");
        engine.clear().await.unwrap();
        assert_ne!(engine.generation(), g2, "clear drops it");
    }

    use super::*;
    use visionclaw_domain::ports::inference_engine::InferenceEngine;
    use visionclaw_domain::ports::owl_types::{AxiomType, OwlAxiom, OwlClass};

    fn cls(iri: &str) -> OwlClass {
        OwlClass {
            iri: iri.to_string(),
            ..Default::default()
        }
    }

    fn sub_axiom(sub: &str, sup: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: sub.to_string(),
            object: sup.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    fn equiv_axiom(a: &str, b: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::EquivalentClass,
            subject: a.to_string(),
            object: b.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    fn disjoint_axiom(a: &str, b: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::DisjointWith,
            subject: a.to_string(),
            object: b.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    /// GOV-7: a satisfiable class + axiom set passes the self-contained gate.
    #[test]
    fn check_axiom_set_passes_for_consistent_ontology() {
        let classes = vec![cls("urn:test:A"), cls("urn:test:B"), cls("urn:test:D")];
        let axioms = vec![sub_axiom("urn:test:D", "urn:test:A")];
        let outcome = WhelkInferenceEngine::check_axiom_set(&classes, &axioms);
        assert!(
            outcome.consistent,
            "no contradiction ⇒ consistent: {outcome:?}"
        );
        assert!(outcome.unsatisfiable_classes.is_empty());
    }

    /// GOV-7: a minimal contradictory pair — `D ⊑ A`, `D ⊑ B`, `A disjoint B` —
    /// collapses `D` to owl:Nothing and the gate BLOCKS (consistent == false).
    #[test]
    fn check_axiom_set_blocks_inconsistent_draft() {
        let classes = vec![cls("urn:test:A"), cls("urn:test:B"), cls("urn:test:D")];
        let axioms = vec![
            sub_axiom("urn:test:D", "urn:test:A"),
            sub_axiom("urn:test:D", "urn:test:B"),
            disjoint_axiom("urn:test:A", "urn:test:B"),
        ];
        let outcome = WhelkInferenceEngine::check_axiom_set(&classes, &axioms);
        assert!(
            !outcome.consistent,
            "disjoint parents ⇒ inconsistent: {outcome:?}"
        );
        assert!(
            outcome
                .unsatisfiable_classes
                .contains(&"urn:test:D".to_string()),
            "D must be unsatisfiable: {outcome:?}"
        );
        assert!(outcome.explanation().contains("inconsistency"));
    }

    /// ADR-099 D2: an asserted `EquivalentClass(A, B)` must NOT be downgraded.
    /// Whelk derives both directions, and `convert_subsumptions_to_axioms`
    /// re-materialises a single canonical `EquivalentClass` axiom in addition
    /// to the directed sub-class closure — equivalence survives end-to-end.
    #[tokio::test]
    async fn equivalent_class_preserved_bidirectionally_no_downgrade() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:B")];
        let axioms = vec![equiv_axiom("urn:test:A", "urn:test:B")];
        engine.load_ontology(classes, axioms).await.unwrap();
        let results = engine.infer().await.unwrap();

        // Both subsumption directions are entailed (the equivalence, not a
        // single SubClassOf).
        let a_sub_b = results.inferred_axioms.iter().any(|ax| {
            ax.axiom_type == AxiomType::SubClassOf
                && ax.subject == "urn:test:A"
                && ax.object == "urn:test:B"
        });
        let b_sub_a = results.inferred_axioms.iter().any(|ax| {
            ax.axiom_type == AxiomType::SubClassOf
                && ax.subject == "urn:test:B"
                && ax.object == "urn:test:A"
        });
        assert!(
            a_sub_b && b_sub_a,
            "both directions must be entailed: {:?}",
            results.inferred_axioms
        );

        // A canonical EquivalentClass axiom is materialised (lossless marker).
        let equiv = results
            .inferred_axioms
            .iter()
            .filter(|ax| ax.axiom_type == AxiomType::EquivalentClass)
            .count();
        assert_eq!(
            equiv, 1,
            "exactly one canonical EquivalentClass expected: {:?}",
            results.inferred_axioms
        );
    }

    #[tokio::test]
    async fn new_returns_valid_engine() {
        let engine = WhelkInferenceEngine::new();
        let stats = engine.get_statistics().await.unwrap();
        assert_eq!(stats.loaded_classes, 0);
        assert_eq!(stats.loaded_axioms, 0);
        assert_eq!(stats.total_inferences, 0);
    }

    #[tokio::test]
    async fn load_ontology_empty_returns_ok() {
        let mut engine = WhelkInferenceEngine::new();
        let result = engine.load_ontology(vec![], vec![]).await;
        assert!(result.is_ok());
        let stats = engine.get_statistics().await.unwrap();
        assert_eq!(stats.loaded_classes, 0);
        assert_eq!(stats.loaded_axioms, 0);
    }

    #[tokio::test]
    async fn load_ontology_small_fixture_succeeds() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:B"), cls("urn:test:C")];
        let axioms = vec![sub_axiom("urn:test:A", "urn:test:B")];
        let result = engine.load_ontology(classes, axioms).await;
        assert!(result.is_ok());
        let stats = engine.get_statistics().await.unwrap();
        assert_eq!(stats.loaded_classes, 3);
        assert_eq!(stats.loaded_axioms, 1);
    }

    #[tokio::test]
    async fn infer_derives_transitive_subsumption() {
        // A ⊑ B, B ⊑ C  ⟹  whelk should derive A ⊑ C
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:B"), cls("urn:test:C")];
        let axioms = vec![
            sub_axiom("urn:test:A", "urn:test:B"),
            sub_axiom("urn:test:B", "urn:test:C"),
        ];
        engine.load_ontology(classes, axioms).await.unwrap();
        let results = engine.infer().await.unwrap();
        let has_transitive = results
            .inferred_axioms
            .iter()
            .any(|ax| ax.subject == "urn:test:A" && ax.object == "urn:test:C");
        assert!(
            has_transitive,
            "Expected A ⊑ C to be inferred; got {:?}",
            results.inferred_axioms
        );
    }

    #[tokio::test]
    async fn is_entailed_true_for_known_subsumption() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:B"), cls("urn:test:C")];
        let axioms = vec![
            sub_axiom("urn:test:A", "urn:test:B"),
            sub_axiom("urn:test:B", "urn:test:C"),
        ];
        engine.load_ontology(classes, axioms).await.unwrap();
        engine.infer().await.unwrap();

        let query = sub_axiom("urn:test:A", "urn:test:C");
        assert!(engine.is_entailed(&query).await.unwrap());
    }

    #[tokio::test]
    async fn is_entailed_false_for_unrelated_classes() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:X")];
        engine.load_ontology(classes, vec![]).await.unwrap();
        engine.infer().await.unwrap();

        let query = sub_axiom("urn:test:A", "urn:test:X");
        // No axiom was asserted — should not be entailed
        assert!(!engine.is_entailed(&query).await.unwrap());
    }

    #[tokio::test]
    async fn get_subclass_hierarchy_returns_expected_pairs() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:Dog"), cls("urn:test:Animal")];
        let axioms = vec![sub_axiom("urn:test:Dog", "urn:test:Animal")];
        engine.load_ontology(classes, axioms).await.unwrap();
        engine.infer().await.unwrap();

        let hierarchy = engine.get_subclass_hierarchy().await.unwrap();
        let contains_pair = hierarchy
            .iter()
            .any(|(sub, sup)| sub == "urn:test:Dog" && sup == "urn:test:Animal");
        assert!(
            contains_pair,
            "Expected (Dog, Animal) in hierarchy; got {:?}",
            hierarchy
        );
    }

    #[tokio::test]
    async fn clear_resets_state() {
        let mut engine = WhelkInferenceEngine::new();
        engine
            .load_ontology(vec![cls("urn:test:A")], vec![])
            .await
            .unwrap();
        engine.infer().await.unwrap();
        engine.clear().await.unwrap();
        let stats = engine.get_statistics().await.unwrap();
        assert_eq!(stats.loaded_classes, 0);
        // infer() after clear should return OntologyNotLoaded
        let err = engine.infer().await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn infer_uses_cache_on_unchanged_ontology() {
        let mut engine = WhelkInferenceEngine::new();
        let classes = vec![cls("urn:test:A"), cls("urn:test:B")];
        let axioms = vec![sub_axiom("urn:test:A", "urn:test:B")];
        engine
            .load_ontology(classes.clone(), axioms.clone())
            .await
            .unwrap();
        let first = engine.infer().await.unwrap();
        // Reload identical content — checksum matches, cache must be reused.
        engine.load_ontology(classes, axioms).await.unwrap();
        let second = engine.infer().await.unwrap();
        assert_eq!(first.inferred_axioms.len(), second.inferred_axioms.len());
    }
}
