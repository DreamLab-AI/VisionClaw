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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use visionclaw_domain::ports::inference_engine::InferenceEngine;
use visionclaw_domain::ports::ontology_repository::OntologyRepository;

pub struct OntologyQueryService {
    ontology_repo: Arc<dyn OntologyRepository>,
    #[allow(dead_code)]
    graph_repo: Arc<dyn KnowledgeGraphRepository>,
    whelk: Arc<RwLock<WhelkInferenceEngine>>,
    schema_service: Arc<SchemaService>,
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
        }
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
                }
            })
            .collect();

        info!("Ontology discover: found {} results", results.len());
        Ok(results)
    }

    /// Read a note with full ontology context: markdown, metadata, Whelk axioms, related notes.
    pub async fn read_note(&self, iri: &str) -> Result<EnrichedNote, String> {
        info!("Ontology read_note: iri='{}'", iri);

        // Fetch OwlClass from Oxigraph
        let class = self
            .ontology_repo
            .get_owl_class(iri)
            .await
            .map_err(|e| format!("Failed to get class: {}", e))?
            .ok_or_else(|| format!("Class not found: {}", iri))?;

        // Fetch Whelk-inferred axioms
        let whelk = self.whelk.read().await;
        let hierarchy: Vec<(String, String)> = whelk
            .get_subclass_hierarchy()
            .await
            .unwrap_or_else(|_| Vec::new());

        let mut whelk_axioms: Vec<InferredAxiomSummary> = Vec::new();

        // Find all SubClassOf axioms where this class is the subject
        for (child, parent) in &hierarchy {
            if child == iri {
                whelk_axioms.push(InferredAxiomSummary {
                    axiom_type: "SubClassOf".to_string(),
                    subject: child.clone(),
                    object: parent.clone(),
                    is_inferred: true,
                });
            }
        }

        // Also add asserted axioms from the repo
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
            });
        }

        // Fetch related notes (classes connected via relationships)
        let all_classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();
        let related_notes: Vec<RelatedNote> = all_classes
            .iter()
            .filter(|c| {
                // Check if connected via parent/child
                hierarchy.iter().any(|(child, parent)| {
                    (child == iri && parent == &c.iri) || (parent == iri && child == &c.iri)
                })
            })
            .take(10) // Limit related notes
            .map(|c| {
                let summary = c
                    .markdown_content
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(150)
                    .collect();
                let direction = if hierarchy.iter().any(|(child, _)| child == iri) {
                    "outgoing"
                } else {
                    "incoming"
                };
                RelatedNote {
                    iri: c.iri.clone(),
                    preferred_term: c.preferred_term.clone().unwrap_or_default(),
                    relationship_type: "SubClassOf".to_string(),
                    direction: direction.to_string(),
                    summary,
                }
            })
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
