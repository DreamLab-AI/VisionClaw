//! OntologyQueryService — Agent read path for ontology-guided intelligence.
//!
//! Provides semantic discovery, enriched note reading, validated Cypher queries,
//! and ontology graph traversal. Agents call these methods via MCP tools to
//! discover relevant vault notes via OWL class hierarchies and Whelk inferences.
//!
//! The vault markdown pages with OKF frontmatter ARE the knowledge graph nodes.
//! Discovery happens via ontology semantics: class hierarchy traversal, Whelk EL++
//! subsumption reasoning, and relationship fan-out (has-part, requires, enables, bridges-to).

use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::services::schema_service::SchemaService;
use crate::types::ontology_tools::*;
use log::info;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use visionclaw_domain::ports::inference_engine::InferenceEngine;
use visionclaw_domain::ports::ontology_repository::{
    AxiomType, OntologyRepository, OwlAxiom, OwlClass,
};
use visionclaw_ontology::open_world::{
    self, BundleLocation, PropertySignature, RelationIndex, SubsumptionIndex,
};

/// Where the generation of the loaded ontology comes from (ADR-2127
/// decision 3).
#[derive(Debug, Clone)]
enum GenerationSource {
    /// Resolve on every reload: the bundle `ONTOLOGY_BUNDLE_DIR` pins, else
    /// the one the vault root's `vault.toml` names, else the store digest.
    Environment,
    /// A fixed bundle location, else the store digest.
    Bundle(BundleLocation),
    /// A caller-supplied generation (tests, or a caller that already knows).
    Fixed(Option<String>),
}

/// Everything derived from one loaded ontology, rebuilt only when the store
/// or the Whelk closure changes.
struct LoadedOntology {
    fingerprint: u64,
    index: SubsumptionIndex,
    relations: RelationIndex,
    asserted: HashSet<(String, String)>,
    generation: Option<String>,
    terms: TermIndex,
}

/// Why a check could not be asked: a class term names nothing in the loaded
/// ontology (or names several things), or the store failed. The first is the
/// caller's question, not the corpus's silence, so it is never answered
/// `not_asserted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// A class term resolves to no class, by IRI or by label.
    UnknownTerm(String),
    /// A label matches more than one class; the candidates are listed.
    AmbiguousTerm(String, Vec<String>),
    /// The repository or reasoner failed.
    Internal(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTerm(t) => write!(
                f,
                "'{t}' names no class in the loaded ontology (give a class IRI or its exact label)"
            ),
            Self::AmbiguousTerm(t, iris) => {
                write!(f, "'{t}' is the label of {} classes: {}", iris.len(), iris.join(", "))
            }
            Self::Internal(e) => f.write_str(e),
        }
    }
}

/// How a check's class terms resolve: an IRI the ontology mentions (a declared
/// class or an axiom endpoint) stands for itself; otherwise a class label,
/// case-insensitively, when exactly one class carries it.
#[derive(Debug, Default)]
struct TermIndex {
    iris: HashSet<String>,
    labels: HashMap<String, Vec<String>>,
}

impl TermIndex {
    fn new<'a>(
        classes: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
        axiom_terms: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut terms = Self::default();
        for (iri, label) in classes {
            terms.iris.insert(iri.to_owned());
            if let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) {
                let iris = terms.labels.entry(label.to_lowercase()).or_default();
                if !iris.iter().any(|i| i == iri) {
                    iris.push(iri.to_owned());
                }
            }
        }
        terms.iris.extend(axiom_terms.into_iter().map(str::to_owned));
        terms
    }

    fn resolve(&self, term: &str) -> Result<String, CheckError> {
        let term = term.trim();
        if self.iris.contains(term) {
            return Ok(term.to_owned());
        }
        match self.labels.get(&term.to_lowercase()).map(Vec::as_slice) {
            Some([only]) => Ok(only.clone()),
            Some(many) if !many.is_empty() => {
                let mut iris = many.to_vec();
                iris.sort();
                Err(CheckError::AmbiguousTerm(term.to_owned(), iris))
            }
            _ => Err(CheckError::UnknownTerm(term.to_owned())),
        }
    }
}

pub struct OntologyQueryService {
    ontology_repo: Arc<dyn OntologyRepository>,
    #[allow(dead_code)]
    graph_repo: Arc<dyn KnowledgeGraphRepository>,
    whelk: Arc<RwLock<WhelkInferenceEngine>>,
    schema_service: Arc<SchemaService>,
    generation_source: GenerationSource,
    /// The index and generation of the ontology last seen, keyed by its
    /// fingerprint (ADR-2127 decision 3: answers name the generation they
    /// were computed against, and it refreshes when the ontology reloads).
    loaded: RwLock<Option<Arc<LoadedOntology>>>,
}

/// The property a relation axiom is about: the object-property predicate or
/// the restriction's `onProperty` (both surfaced by the Oxigraph adapter).
fn axiom_property(a: &OwlAxiom) -> &str {
    a.annotations
        .get("predicate")
        .or_else(|| a.annotations.get("property"))
        .map(String::as_str)
        .unwrap_or("")
}

fn axiom_type_name(a: &OwlAxiom) -> String {
    format!("{:?}", a.axiom_type)
}

impl OntologyQueryService {
    pub fn new(
        ontology_repo: Arc<dyn OntologyRepository>,
        graph_repo: Arc<dyn KnowledgeGraphRepository>,
        whelk: Arc<RwLock<WhelkInferenceEngine>>,
        schema_service: Arc<SchemaService>,
    ) -> Self {
        Self {
            ontology_repo,
            graph_repo,
            whelk,
            schema_service,
            generation_source: GenerationSource::Environment,
            loaded: RwLock::new(None),
        }
    }

    /// Override the generation answers report (tests, or a caller that has
    /// already resolved it).
    pub fn with_generation(mut self, generation: Option<String>) -> Self {
        self.generation_source = GenerationSource::Fixed(generation);
        self
    }

    /// Read the generation from this bundle location instead of the
    /// environment.
    pub fn with_bundle_location(mut self, location: BundleLocation) -> Self {
        self.generation_source = GenerationSource::Bundle(location);
        self
    }

    /// The loaded ontology's index and generation, rebuilt when the asserted
    /// axioms, the class set or the Whelk closure changed since the last call.
    ///
    /// The cache check is cheap. The class set is probed by IRI only
    /// ([`OntologyRepository::class_iris`]) rather than by materialising
    /// every class, and the Whelk closure is keyed on the engine's
    /// [`WhelkInferenceEngine::generation`] counter rather than cloned and
    /// hashed; the hierarchy is read only on a rebuild. The axioms are still
    /// read per call: the Oxigraph store is shared through
    /// `OxigraphOntologyRepository::store()` with direct writers (GitHub sync,
    /// the decision handler, the mutation service), so no repository-side
    /// revision counter would see every change, and a stale index would
    /// answer for a generation that is gone.
    async fn loaded(&self) -> Arc<LoadedOntology> {
        let axioms = self.ontology_repo.get_axioms().await.unwrap_or_default();
        let class_iris = self.ontology_repo.class_iris().await.unwrap_or_default();
        self.loaded_from(axioms, &class_iris).await
    }

    /// The cache key: the asserted axioms, the class set and the Whelk
    /// closure's generation.
    fn fingerprint_of(axioms: &[OwlAxiom], class_iris: &[String], whelk_generation: u64) -> u64 {
        open_world::fingerprint([
            open_world::fingerprint(
                axioms
                    .iter()
                    .map(|a| (axiom_type_name(a), &a.subject, axiom_property(a), &a.object)),
            ),
            open_world::fingerprint(class_iris.iter()),
            whelk_generation,
        ])
    }

    async fn loaded_from(
        &self,
        axioms: Vec<OwlAxiom>,
        class_iris: &[String],
    ) -> Arc<LoadedOntology> {
        let probe = Self::fingerprint_of(&axioms, class_iris, self.whelk.read().await.generation());
        if let Some(current) = self.loaded.read().await.as_ref() {
            if current.fingerprint == probe {
                return current.clone();
            }
        }

        // Rebuild. The generation and the hierarchy are read under one guard,
        // so the key stored names exactly the closure the index was built from.
        let (fingerprint, hierarchy) = {
            let whelk = self.whelk.read().await;
            let hierarchy: Vec<(String, String)> =
                whelk.get_subclass_hierarchy().await.unwrap_or_default();
            (
                Self::fingerprint_of(&axioms, class_iris, whelk.generation()),
                hierarchy,
            )
        };

        let of_type = |t: AxiomType| {
            axioms
                .iter()
                .filter(move |a| a.axiom_type == t)
                .map(|a| (a.subject.clone(), a.object.clone()))
        };
        let asserted: Vec<(String, String)> = of_type(AxiomType::SubClassOf).collect();
        let index = SubsumptionIndex::new(
            asserted.clone(),
            hierarchy,
            of_type(AxiomType::DisjointWith),
        );
        let relations = RelationIndex::new(
            axioms
                .iter()
                .filter(|a| {
                    matches!(
                        a.axiom_type,
                        AxiomType::ObjectPropertyAssertion | AxiomType::SomeValuesFrom
                    ) && !axiom_property(a).is_empty()
                })
                .map(|a| {
                    (
                        a.subject.clone(),
                        axiom_property(a).to_string(),
                        a.object.clone(),
                    )
                }),
            of_type(AxiomType::SubPropertyOf),
        );
        let generation = match &self.generation_source {
            GenerationSource::Fixed(g) => g.clone(),
            source => {
                let bundle = match source {
                    GenerationSource::Bundle(location) => {
                        open_world::generation_from_location(location)
                    }
                    _ => open_world::generation_from_env(),
                };
                Some(bundle.unwrap_or_else(|| {
                    let types: Vec<String> = axioms.iter().map(axiom_type_name).collect();
                    open_world::store_generation(
                        class_iris.iter().map(String::as_str),
                        axioms.iter().zip(&types).map(|(a, t)| {
                            (
                                t.as_str(),
                                a.subject.as_str(),
                                axiom_property(a),
                                a.object.as_str(),
                            )
                        }),
                    )
                }))
            }
        };
        info!(
            "Ontology query service: loaded generation {:?} ({} axioms, {} classes)",
            generation,
            axioms.len(),
            class_iris.len()
        );
        // Labels are read only here, on a rebuild: the cache probe stays IRI-only.
        let classes = self.ontology_repo.list_owl_classes().await.unwrap_or_default();
        let terms = TermIndex::new(
            classes
                .iter()
                .map(|c| (c.iri.as_str(), c.label.as_deref().or(c.preferred_term.as_deref())))
                .chain(class_iris.iter().map(|iri| (iri.as_str(), None))),
            axioms
                .iter()
                .flat_map(|a| [a.subject.as_str(), a.object.as_str()]),
        );
        let fresh = Arc::new(LoadedOntology {
            fingerprint,
            index,
            relations,
            asserted: asserted.into_iter().collect(),
            generation,
            terms,
        });
        *self.loaded.write().await = Some(fresh.clone());
        fresh
    }

    /// ADR-2127 decision 3: the open-world scope every answer carries, so an
    /// empty result reads as "the corpus is silent at this generation". The
    /// generation is that of the ontology currently loaded.
    pub async fn answer_scope(&self) -> AnswerScope {
        AnswerScope::open(self.loaded().await.generation.clone())
    }

    /// ADR-2127 decision 2: is `subject` ⊑ `class`? Answers `entailed`
    /// (asserted or Whelk-entailed, with its basis), `entailed_false` (an
    /// `owl:disjointWith` between the two sides, with the witness pair), or
    /// `not_asserted` — never a boolean. Each side is a class IRI or a class
    /// label; a term that names no class is [`CheckError::UnknownTerm`], not
    /// silence, and the answer echoes the resolved IRIs.
    pub async fn check_membership(
        &self,
        subject: &str,
        class: &str,
    ) -> Result<MembershipCheck, CheckError> {
        info!("Ontology check_membership: '{}' ⊑ '{}'", subject, class);
        let loaded = self.loaded().await;
        let subject = loaded.terms.resolve(subject)?;
        let class = loaded.terms.resolve(class)?;
        Ok(loaded
            .index
            .membership(&subject, &class, AnswerScope::open(loaded.generation.clone())))
    }

    /// ADR-2127 decision 2, relation form: does `subject` stand in `property`
    /// to `object`? `entailed` when the edge is asserted or follows from one
    /// through the class and property hierarchies; `entailed_false` only when
    /// `property` (or a super-property) declares a domain or range the subject
    /// or object is entailed-disjoint with; otherwise `not_asserted`. Subject
    /// and object resolve as in [`Self::check_membership`]; a property label
    /// maps to its IRI, and an undeclared property passes through (silence).
    pub async fn check_relation(
        &self,
        subject: &str,
        property: &str,
        object: &str,
    ) -> Result<RelationCheck, CheckError> {
        info!(
            "Ontology check_relation: '{}' {} '{}'",
            subject, property, object
        );
        let loaded = self.loaded().await;
        let subject = loaded.terms.resolve(subject)?;
        let object = loaded.terms.resolve(object)?;
        let properties = self
            .ontology_repo
            .list_owl_properties()
            .await
            .map_err(|e| CheckError::Internal(format!("Failed to list properties: {}", e)))?;
        let wanted = property.trim();
        let property = if properties.iter().any(|p| p.iri == wanted) {
            wanted.to_owned()
        } else {
            let by_label: Vec<&str> = properties
                .iter()
                .filter(|p| p.label.as_deref().is_some_and(|l| l.trim().eq_ignore_ascii_case(wanted)))
                .map(|p| p.iri.as_str())
                .collect();
            match by_label.as_slice() {
                [only] => (*only).to_owned(),
                [] => wanted.to_owned(),
                many => {
                    let mut iris: Vec<String> = many.iter().map(|i| (*i).to_owned()).collect();
                    iris.sort();
                    return Err(CheckError::AmbiguousTerm(wanted.to_owned(), iris));
                }
            }
        };
        let signatures: HashMap<String, PropertySignature> = properties
            .into_iter()
            .filter(|p| !p.domain.is_empty() || !p.range.is_empty())
            .map(|p| {
                (
                    p.iri,
                    PropertySignature {
                        domain: p.domain,
                        range: p.range,
                    },
                )
            })
            .collect();
        Ok(loaded.relations.relation(
            &loaded.index,
            &signatures,
            &subject,
            &property,
            &object,
            AnswerScope::open(loaded.generation.clone()),
        ))
    }

    /// Breadth-first walk from `start_iri` along each note's related notes.
    /// Every edge carries the basis of the relation it walked (ADR-2127); an
    /// unknown start yields an empty, open-scoped result rather than an error.
    pub async fn traverse(
        &self,
        start_iri: &str,
        max_depth: usize,
        rel_filter: Option<&[String]>,
    ) -> Result<TraversalResult, String> {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back((start_iri.to_string(), 0usize));
        visited.insert(start_iri.to_string());

        // One snapshot for the whole walk: the classes are listed and the
        // index looked up once, not once per visited node.
        let all_classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();
        let loaded = self.loaded_for(&all_classes).await;

        while let Some((current_iri, depth)) = queue.pop_front() {
            if depth > max_depth {
                continue;
            }
            match self.read_note_in(&loaded, &all_classes, &current_iri).await {
                Ok(note) => {
                    nodes.push(TraversalNode {
                        iri: note.iri.clone(),
                        preferred_term: note.preferred_term.clone(),
                        domain: note.ontology_metadata.domain.clone(),
                        depth,
                    });
                    for related in &note.related_notes {
                        let rel_type = &related.relationship_type;
                        let should_follow = rel_filter
                            .map(|types| types.iter().any(|t| t == rel_type))
                            .unwrap_or(true);
                        if !should_follow {
                            continue;
                        }
                        edges.push(TraversalEdge {
                            source_iri: current_iri.clone(),
                            target_iri: related.iri.clone(),
                            relationship_type: rel_type.clone(),
                            basis: related.basis,
                        });
                        if depth < max_depth && visited.insert(related.iri.clone()) {
                            queue.push_back((related.iri.clone(), depth + 1));
                        }
                    }
                }
                Err(e) => {
                    // Skip nodes that can't be read (may not exist)
                    log::debug!("Traversal: skipping {} — {}", current_iri, e);
                }
            }
        }

        Ok(TraversalResult {
            start_iri: start_iri.to_string(),
            nodes,
            edges,
            scope: AnswerScope::open(loaded.generation.clone()),
        })
    }

    /// [`Self::loaded`] for a class list the caller already holds.
    async fn loaded_for(&self, classes: &[OwlClass]) -> Arc<LoadedOntology> {
        let axioms = self.ontology_repo.get_axioms().await.unwrap_or_default();
        let class_iris: Vec<String> = classes.iter().map(|c| c.iri.clone()).collect();
        self.loaded_from(axioms, &class_iris).await
    }

    /// Semantic discovery: find relevant notes via class hierarchy + Whelk inference.
    ///
    /// 1. Keyword match against OwlClass preferred_term/label
    /// 2. Expand via Whelk transitive closure (subclasses + superclasses)
    /// 3. Follow semantic relationships (has-part, requires, enables, bridges-to)
    /// 4. Score and rank results
    pub async fn discover(
        &self,
        query: &str,
        limit: usize,
        domain_filter: Option<&str>,
    ) -> Result<Vec<DiscoveryResult>, String> {
        info!(
            "Ontology discover: query='{}', limit={}, domain={:?}",
            query, limit, domain_filter
        );

        // Step 1: Get all OWL classes
        let classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .map_err(|e| format!("Failed to list classes: {}", e))?;

        // Step 2: Keyword matching — score each class against query terms
        let query_terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut scored: Vec<(f32, crate::ports::ontology_repository::OwlClass, bool)> = Vec::new();

        for class in &classes {
            // Domain filter
            if let Some(domain) = domain_filter {
                if class.source_domain.as_deref() != Some(domain) {
                    continue;
                }
            }

            let term = class.preferred_term.as_deref().unwrap_or("");
            let label = class.label.as_deref().unwrap_or("");
            let description = class.description.as_deref().unwrap_or("");

            let text = format!("{} {} {}", term, label, description).to_lowercase();

            let keyword_score: f32 = query_terms
                .iter()
                .map(|t| if text.contains(t.as_str()) { 1.0 } else { 0.0 })
                .sum::<f32>()
                / query_terms.len().max(1) as f32;

            if keyword_score > 0.0 {
                let quality = class.quality_score.unwrap_or(0.5);
                let authority = class.authority_score.unwrap_or(0.5);
                let combined = keyword_score * 0.4 + quality * 0.3 + authority * 0.2 + 0.1;
                scored.push((combined, class.clone(), false));
            }
        }

        // Step 3: Whelk expansion — for top matches, include subclasses via inference
        let whelk = self.whelk.read().await;
        let hierarchy: Vec<(String, String)> = whelk
            .get_subclass_hierarchy()
            .await
            .unwrap_or_else(|_| Vec::new());

        // Build parent->children and child->parents maps
        let mut children_of: HashMap<String, HashSet<String>> = HashMap::new();
        let mut parents_of: HashMap<String, HashSet<String>> = HashMap::new();
        for (child, parent) in &hierarchy {
            children_of
                .entry(parent.clone())
                .or_default()
                .insert(child.clone());
            parents_of
                .entry(child.clone())
                .or_default()
                .insert(parent.clone());
        }

        let matched_iris: HashSet<String> = scored.iter().map(|(_, c, _)| c.iri.clone()).collect();

        // Expand: add subclasses of matched classes (depth 2)
        let mut expansion_iris: HashSet<String> = HashSet::new();
        for iri in &matched_iris {
            if let Some(children) = children_of.get(iri) {
                for child in children {
                    if !matched_iris.contains(child) {
                        expansion_iris.insert(child.clone());
                        // depth 2
                        if let Some(grandchildren) = children_of.get(child) {
                            for gc in grandchildren {
                                expansion_iris.insert(gc.clone());
                            }
                        }
                    }
                }
            }
        }

        // Look up expanded classes and add with lower score
        for class in &classes {
            if expansion_iris.contains(&class.iri) {
                let quality = class.quality_score.unwrap_or(0.5);
                let combined = 0.2 + quality * 0.3; // Lower base score for inferred results
                scored.push((combined, class.clone(), true));
            }
        }

        // Step 4: Sort by score descending, dedup, limit
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut seen = HashSet::new();
        let results: Vec<DiscoveryResult> = scored
            .into_iter()
            .filter(|(_, c, _)| seen.insert(c.iri.clone()))
            .take(limit)
            .map(|(score, class, inferred)| {
                let definition = class
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect();

                DiscoveryResult {
                    iri: class.iri.clone(),
                    preferred_term: class.preferred_term.clone().unwrap_or_default(),
                    definition_summary: definition,
                    relevance_score: score,
                    quality_score: class.quality_score.unwrap_or(0.0),
                    domain: class.source_domain.clone().unwrap_or_default(),
                    relationships: Vec::new(), // Populated in step 3 extension
                    whelk_inferred: inferred,
                    basis: if inferred {
                        FactBasis::Inferred
                    } else {
                        FactBasis::Asserted
                    },
                }
            })
            .collect();

        info!("Ontology discover: found {} results", results.len());
        Ok(results)
    }

    /// Read a note with full ontology context: markdown, metadata, Whelk axioms, related notes.
    pub async fn read_note(&self, iri: &str) -> Result<EnrichedNote, String> {
        let all_classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();
        let loaded = self.loaded_for(&all_classes).await;
        self.read_note_in(&loaded, &all_classes, iri).await
    }

    /// [`Self::read_note`] against an already-loaded index and class list.
    async fn read_note_in(
        &self,
        loaded: &LoadedOntology,
        all_classes: &[OwlClass],
        iri: &str,
    ) -> Result<EnrichedNote, String> {
        info!("Ontology read_note: iri='{}'", iri);

        // Fetch OwlClass from Oxigraph
        let class = self
            .ontology_repo
            .get_owl_class(iri)
            .await
            .map_err(|e| format!("Failed to get class: {}", e))?
            .ok_or_else(|| format!("Class not found: {}", iri))?;

        // ADR-2127: the closure (Whelk ∪ asserted) gives each axiom and each
        // related note its basis. An axiom both asserted and in the closure is
        // reported once, as asserted. The subject's ancestors and descendants
        // are walked once (`subject_view`), not once per class.
        let view = loaded.index.subject_view(iri);

        let mut whelk_axioms: Vec<InferredAxiomSummary> = Vec::new();

        // Every SubClassOf this class is entailed into but not asserted with.
        for ancestor in view.ancestors() {
            if ancestor == iri
                || loaded
                    .asserted
                    .contains(&(iri.to_string(), ancestor.clone()))
            {
                continue;
            }
            whelk_axioms.push(InferredAxiomSummary {
                axiom_type: "SubClassOf".to_string(),
                subject: iri.to_string(),
                object: ancestor.clone(),
                is_inferred: true,
                basis: FactBasis::Inferred,
            });
        }

        // Asserted axioms from the repo
        let asserted = self
            .ontology_repo
            .get_class_axioms(iri)
            .await
            .unwrap_or_default();

        for axiom in &asserted {
            whelk_axioms.push(InferredAxiomSummary {
                axiom_type: format!("{:?}", axiom.axiom_type),
                subject: axiom.subject.clone(),
                object: axiom.object.clone(),
                is_inferred: false,
                basis: FactBasis::Asserted,
            });
        }

        // Related notes: classes connected by subsumption either way, each
        // with the direction and basis of that particular edge.
        let related_notes: Vec<RelatedNote> = all_classes
            .iter()
            .filter(|c| c.iri != iri)
            .filter_map(|c| {
                let (direction, basis) = view
                    .basis_to(&c.iri)
                    .map(|b| ("outgoing", b))
                    .or_else(|| view.basis_from(&c.iri).map(|b| ("incoming", b)))?;
                let summary = c
                    .markdown_content
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(150)
                    .collect();
                Some(RelatedNote {
                    iri: c.iri.clone(),
                    preferred_term: c.preferred_term.clone().unwrap_or_default(),
                    relationship_type: "SubClassOf".to_string(),
                    direction: direction.to_string(),
                    summary,
                    basis,
                })
            })
            .take(10) // Limit related notes
            .collect();

        // Get schema context for query grounding
        let schema = self.schema_service.get_schema().await;
        let schema_context = schema.to_llm_context();

        Ok(EnrichedNote {
            iri: class.iri.clone(),
            term_id: class.term_id.clone().unwrap_or_default(),
            preferred_term: class.preferred_term.clone().unwrap_or_default(),
            markdown_content: class.markdown_content.clone().unwrap_or_default(),
            ontology_metadata: OntologyMetadata {
                owl_class: class.iri.clone(),
                physicality: class.owl_physicality.clone().unwrap_or_default(),
                role: class.owl_role.clone().unwrap_or_default(),
                domain: class.source_domain.clone().unwrap_or_default(),
                quality_score: class.quality_score.unwrap_or(0.0),
                authority_score: class.authority_score.unwrap_or(0.0),
                maturity: class.maturity.clone().unwrap_or_default(),
                status: class.status.clone().unwrap_or_default(),
                parent_classes: class.parent_classes.clone(),
            },
            whelk_axioms,
            related_notes,
            schema_context,
        })
    }

    /// Validate a Cypher query against the OWL schema and execute if valid.
    pub async fn validate_and_execute_cypher(
        &self,
        cypher: &str,
    ) -> Result<CypherValidationResult, String> {
        info!(
            "Ontology validate_cypher: '{}'",
            &cypher[..cypher.len().min(100)]
        );

        let mut errors = Vec::new();
        let mut hints = Vec::new();

        // Get known classes and properties for validation
        let classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();
        let known_iris: HashSet<String> = classes.iter().map(|c| c.iri.clone()).collect();
        let known_terms: HashMap<String, String> = classes
            .iter()
            .filter_map(|c| {
                c.preferred_term
                    .as_ref()
                    .map(|t| (t.to_lowercase(), c.iri.clone()))
            })
            .collect();

        // Basic validation: check if referenced labels exist in ontology
        // Extract labels from MATCH (n:Label) patterns
        let label_re = regex::Regex::new(r"\((\w+):(\w+)\)").unwrap_or_else(|_| {
            regex::Regex::new(r"x").expect("single-char fallback regex is always valid")
        });

        for cap in label_re.captures_iter(cypher) {
            if let Some(label) = cap.get(2) {
                let label_str = label.as_str();
                // Check if it's a known OWL class (by IRI suffix or preferred_term)
                let is_known = known_iris.iter().any(|iri| iri.ends_with(label_str))
                    || known_terms.contains_key(&label_str.to_lowercase())
                    || label_str == "OwlClass"
                    || label_str == "OntologyProposal"
                    || label_str == "Node";

                if !is_known {
                    errors.push(format!(
                        "Unknown label '{}' — not found in ontology",
                        label_str
                    ));

                    // Find closest match for hint
                    let closest = known_terms
                        .keys()
                        .min_by_key(|k| levenshtein_distance(k, &label_str.to_lowercase()))
                        .cloned();

                    if let Some(closest_term) = closest {
                        if let Some(closest_iri) = known_terms.get(&closest_term) {
                            hints.push(format!(
                                "Did you mean '{}' ({})?",
                                closest_term, closest_iri
                            ));
                        }
                    }
                }
            }
        }

        Ok(CypherValidationResult {
            valid: errors.is_empty(),
            errors,
            hints,
        })
    }

    /// Get LLM-friendly schema context for query grounding
    pub async fn get_schema_context(&self) -> String {
        let schema = self.schema_service.get_schema().await;
        schema.to_llm_context()
    }
}

/// Simple Levenshtein distance for fuzzy matching
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

#[cfg(test)]
mod term_index_tests {
    use super::{CheckError, TermIndex};

    fn index() -> TermIndex {
        TermIndex::new(
            [
                ("urn:ngm:class:1-inch", Some("1inch")),
                ("urn:ngm:class:bias", Some("Bias")),
                ("urn:a:bias", Some("bias ")),
                ("urn:ngm:class:token", None),
            ],
            ["urn:axiom:only"],
        )
    }

    #[test]
    fn an_iri_stands_for_itself() {
        let terms = index();
        assert_eq!(terms.resolve("urn:ngm:class:token").unwrap(), "urn:ngm:class:token");
        assert_eq!(terms.resolve(" urn:axiom:only ").unwrap(), "urn:axiom:only");
    }

    #[test]
    fn a_unique_label_resolves_whatever_its_case() {
        assert_eq!(index().resolve("1INCH").unwrap(), "urn:ngm:class:1-inch");
    }

    #[test]
    fn a_shared_label_is_ambiguous_and_lists_its_classes() {
        assert_eq!(
            index().resolve("bias").unwrap_err(),
            CheckError::AmbiguousTerm(
                "bias".into(),
                vec!["urn:a:bias".into(), "urn:ngm:class:bias".into()]
            )
        );
    }

    #[test]
    fn a_term_naming_nothing_is_unknown() {
        assert_eq!(
            index().resolve("Decentralized Exchange").unwrap_err(),
            CheckError::UnknownTerm("Decentralized Exchange".into())
        );
    }
}
