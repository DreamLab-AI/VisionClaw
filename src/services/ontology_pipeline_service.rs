// src/services/ontology_pipeline_service.rs
//! Ontology Pipeline Service
//!
//! Orchestrates the end-to-end semantic physics pipeline:
//! 1. GitHub Sync → Parse Ontology → Save to Oxigraph (OxigraphOntologyRepository, ADR-11)
//! 2. Trigger Reasoning via ReasoningActor → CustomReasoner inference → Cache results
//! 3. Generate Constraints from axioms → ConstraintSet with Semantic kind
//! 4. Upload to GPU via OntologyConstraintActor → Apply semantic forces → Stream to client
//!
//! All ontology data persists in Oxigraph (RDF quad-store). Constraints use ConstraintKind::Semantic = 10.

use actix::Addr;
use log::{debug, error, info, warn};
use std::sync::Arc;

use crate::actors::gpu::ontology_constraint_actor::OntologyConstraintActor;
use crate::actors::graph_actor::GraphStateActor;
use crate::actors::ontology_actor::OntologyActor;
// REMOVED: reasoning_actor no longer exists - reasoning functionality moved to custom_reasoner
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::reasoning::custom_reasoner::Ontology;
use crate::services::github_sync_service::SyncStatistics;
use visionclaw_domain::models::constraints::ConstraintSet;

/// Configuration for semantic physics pipeline
#[derive(Debug, Clone)]
pub struct SemanticPhysicsConfig {
    /// Enable automatic reasoning after ontology changes
    pub auto_trigger_reasoning: bool,

    /// Enable automatic constraint generation
    pub auto_generate_constraints: bool,

    /// Constraint strength multiplier (0.0 - 10.0)
    pub constraint_strength: f32,

    /// Enable GPU acceleration for constraints
    pub use_gpu_constraints: bool,

    /// Wave 3 asserted/inferred channel: materialise reasoner-entailed SubClassOf
    /// axioms as tagged GraphData edges (`inferred:true`, amber-dashed in XR).
    /// OFF by default — it grows the edge set and feeds physics, so it is enabled
    /// deliberately once validated on a deployment.
    pub materialise_inferred_edges: bool,

    /// Per-child cap on materialised inferred parents (volume safety valve).
    pub max_inferred_parents_per_child: usize,

    /// Maximum reasoning depth
    pub max_reasoning_depth: usize,

    /// Cache reasoning results
    pub cache_inferences: bool,
}

impl Default for SemanticPhysicsConfig {
    fn default() -> Self {
        Self {
            auto_trigger_reasoning: true,
            auto_generate_constraints: true,
            constraint_strength: 1.0,
            use_gpu_constraints: true,
            materialise_inferred_edges: false,
            max_inferred_parents_per_child:
                crate::services::inferred_edge_materialiser::DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD,
            max_reasoning_depth: 10,
            cache_inferences: true,
        }
    }
}

/// Statistics for the ontology pipeline
#[derive(Debug, Clone)]
pub struct OntologyPipelineStats {
    pub sync_stats: Option<SyncStatistics>,
    pub reasoning_triggered: bool,
    pub inferred_axioms_count: usize,
    pub constraints_generated: usize,
    pub gpu_upload_success: bool,
    pub total_time_ms: u64,
}

/// Orchestrates the complete ontology-to-physics pipeline
/// This service coordinates between:
/// - ReasoningActor: Runs CustomReasoner for OWL inference
/// - OntologyConstraintActor: Applies semantic constraints to GPU physics
/// - GraphStateActor: Manages Oxigraph-backed graph data
///
/// The pipeline automatically triggers after ontology modifications from GitHub sync.
pub struct OntologyPipelineService {
    config: SemanticPhysicsConfig,
    // REMOVED: reasoning_actor - ReasoningActor no longer exists
    // reasoning_actor: Option<Addr<ReasoningActor>>,
    ontology_actor: Option<Addr<OntologyActor>>,
    graph_actor: Option<Addr<GraphStateActor>>,
    constraint_actor: Option<Addr<OntologyConstraintActor>>,
    graph_repo: Option<Arc<dyn KnowledgeGraphRepository>>,
}

impl OntologyPipelineService {
    /// Create a new pipeline service
    pub fn new(config: SemanticPhysicsConfig) -> Self {
        info!(
            "Initializing OntologyPipelineService with config: {:?}",
            config
        );

        Self {
            config,
            // reasoning_actor: None,
            ontology_actor: None,
            graph_actor: None,
            constraint_actor: None,
            graph_repo: None,
        }
    }

    // REMOVED: ReasoningActor no longer exists
    // /// Set the reasoning actor address
    // pub fn set_reasoning_actor(&mut self, addr: Addr<ReasoningActor>) {
    //     info!("OntologyPipelineService: Reasoning actor address registered");
    //     self.reasoning_actor = Some(addr);
    // }

    /// Set the ontology actor address
    pub fn set_ontology_actor(&mut self, addr: Addr<OntologyActor>) {
        info!("OntologyPipelineService: Ontology actor address registered");
        self.ontology_actor = Some(addr);
    }

    /// Set the graph service actor address
    pub fn set_graph_actor(&mut self, addr: Addr<GraphStateActor>) {
        info!("OntologyPipelineService: Graph service actor address registered");
        self.graph_actor = Some(addr);
    }

    /// Set the constraint actor address
    pub fn set_constraint_actor(&mut self, addr: Addr<OntologyConstraintActor>) {
        info!("OntologyPipelineService: Constraint actor address registered");
        self.constraint_actor = Some(addr);
    }

    /// Set the graph repository for IRI to node ID resolution
    pub fn set_graph_repository(&mut self, repo: Arc<dyn KnowledgeGraphRepository>) {
        info!("OntologyPipelineService: Graph repository registered");
        self.graph_repo = Some(repo);
    }

    /// Handle ontology modification event
    /// Called automatically by GitHubSyncService after parsing OntologyBlock sections.
    /// Pipeline flow:
    /// 1. Sends ontology data to ReasoningActor
    /// 2. ReasoningActor runs CustomReasoner inference
    /// 3. Inferred axioms converted to ConstraintSet with Semantic constraints
    /// 4. Constraints uploaded to GPU via OntologyConstraintActor
    /// 5. GPU physics applies semantic forces to node positions
    pub async fn on_ontology_modified(
        &self,
        ontology_id: i64,
        ontology: Ontology,
    ) -> Result<OntologyPipelineStats, String> {
        info!("Ontology modification detected for ID: {}", ontology_id);

        let start_time = std::time::Instant::now();
        let mut stats = OntologyPipelineStats {
            sync_stats: None,
            reasoning_triggered: false,
            inferred_axioms_count: 0,
            constraints_generated: 0,
            gpu_upload_success: false,
            total_time_ms: 0,
        };

        // Step 1: Trigger reasoning if enabled
        if self.config.auto_trigger_reasoning {
            match self.trigger_reasoning(ontology_id, ontology.clone()).await {
                Ok(axioms) => {
                    stats.reasoning_triggered = true;
                    stats.inferred_axioms_count = axioms.len();
                    info!("Reasoning complete: {} inferred axioms", axioms.len());

                    // Step 2: Generate constraints from inferred axioms
                    if self.config.auto_generate_constraints && !axioms.is_empty() {
                        match self.generate_constraints_from_axioms(&axioms).await {
                            Ok(constraint_set) => {
                                stats.constraints_generated = constraint_set.constraints.len();
                                info!("Generated {} constraints", stats.constraints_generated);

                                // Step 3: Upload constraints to GPU
                                if self.config.use_gpu_constraints {
                                    match self.upload_constraints_to_gpu(constraint_set).await {
                                        Ok(_) => {
                                            stats.gpu_upload_success = true;
                                            info!("Constraints uploaded to GPU successfully");
                                        }
                                        Err(e) => {
                                            error!("❌ Failed to upload constraints to GPU: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("❌ Failed to generate constraints: {}", e);
                            }
                        }
                    }

                    // Step 2.5 (Wave 3): materialise inferred SubClassOf axioms as
                    // tagged GraphData edges for the asserted/inferred visual
                    // channel. Gated OFF by default. Never fails the pipeline.
                    if self.config.materialise_inferred_edges && !axioms.is_empty() {
                        match self.materialise_inferred_edges_from_axioms(&axioms).await {
                            Ok(n) => info!("Materialised {} inferred edges", n),
                            Err(e) => error!("❌ Inferred-edge materialisation failed: {}", e),
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Reasoning failed: {}", e);
                    return Err(format!("Reasoning failed: {}", e));
                }
            }
        }

        stats.total_time_ms = start_time.elapsed().as_millis() as u64;
        info!("Ontology pipeline complete in {}ms", stats.total_time_ms);

        Ok(stats)
    }

    /// Trigger reasoning process using CustomReasoner directly
    async fn trigger_reasoning(
        &self,
        ontology_id: i64,
        ontology: Ontology,
    ) -> Result<Vec<crate::reasoning::custom_reasoner::InferredAxiom>, String> {
        info!(
            "🧠 Triggering reasoning for ontology {} using CustomReasoner",
            ontology_id
        );

        use crate::reasoning::custom_reasoner::{CustomReasoner, OntologyReasoner};

        let reasoner = CustomReasoner::new();

        match reasoner.infer_axioms(&ontology) {
            Ok(axioms) => {
                info!("Reasoning succeeded: {} axioms inferred", axioms.len());
                Ok(axioms)
            }
            Err(e) => {
                error!("❌ Reasoning failed: {}", e);
                Err(format!("Reasoning error: {}", e))
            }
        }
    }

    /// Generate physics constraints from inferred axioms
    /// Converts CustomReasoner axiom types to semantic constraints:
    /// - SubClassOf: Hierarchical attraction forces (child → parent clustering)
    /// - EquivalentTo: Strong colocation forces (equivalent classes align)
    /// - DisjointWith: Separation/repulsion forces (disjoint classes separate)
    ///
    /// All constraints use ConstraintKind::Semantic (= 10) which is processed
    /// by ontology_constraints.cu in the CUDA kernel pipeline.
    ///
    /// Constraint params format:
    /// - [0]: Semantic constraint sub-type (0=separation, 1=hierarchical, 2=alignment, etc.)
    /// - [1]: Force magnitude
    /// - [2-4]: Optional direction vector or additional parameters
    async fn generate_constraints_from_axioms(
        &self,
        axioms: &[crate::reasoning::custom_reasoner::InferredAxiom],
    ) -> Result<ConstraintSet, String> {
        info!("Generating constraints from {} axioms", axioms.len());

        use crate::reasoning::custom_reasoner::AxiomType;
        use visionclaw_domain::models::constraints::{Constraint, ConstraintKind};

        // Get graph repository for IRI → node ID resolution
        let graph_repo = self
            .graph_repo
            .as_ref()
            .ok_or_else(|| "Graph repository not configured".to_string())?;

        let mut constraints = Vec::new();
        let mut skipped_count = 0;

        for axiom in axioms {
            // Resolve subject IRI to node IDs
            let subject_nodes = match graph_repo.get_nodes_by_owl_class_iri(&axiom.subject).await {
                Ok(nodes) => nodes,
                Err(e) => {
                    debug!("No nodes found for subject IRI '{}': {}", axiom.subject, e);
                    skipped_count += 1;
                    continue;
                }
            };

            if subject_nodes.is_empty() {
                debug!("No nodes found with owl_class_iri: {}", axiom.subject);
                skipped_count += 1;
                continue;
            }

            // Convert inferred axioms to physics constraints
            match axiom.axiom_type {
                AxiomType::SubClassOf => {
                    // HierarchicalAttraction: Child nodes are pulled toward parent class nodes
                    if let Some(superclass) = &axiom.object {
                        let object_nodes =
                            match graph_repo.get_nodes_by_owl_class_iri(superclass).await {
                                Ok(nodes) => nodes,
                                Err(e) => {
                                    debug!("No nodes found for object IRI '{}': {}", superclass, e);
                                    skipped_count += 1;
                                    continue;
                                }
                            };

                        if object_nodes.is_empty() {
                            debug!("No nodes found with owl_class_iri: {}", superclass);
                            skipped_count += 1;
                            continue;
                        }

                        // Build constraint with all subject and object nodes
                        let mut node_indices: Vec<u32> = Vec::new();
                        node_indices.extend(subject_nodes.iter().map(|n| n.id));
                        node_indices.extend(object_nodes.iter().map(|n| n.id));

                        // Params: [constraint_subtype, force_magnitude]
                        // SubType 1 = HierarchicalAttraction
                        let force_magnitude = self.config.constraint_strength * 0.5; // Gentler pull

                        constraints.push(Constraint {
                            kind: ConstraintKind::Semantic,
                            node_indices,
                            params: vec![1.0, force_magnitude], // subtype=1, magnitude
                            weight: self.config.constraint_strength,
                            active: true,
                        });

                        debug!(
                            "Created SubClassOf constraint: {} → {} ({} nodes)",
                            axiom.subject,
                            superclass,
                            subject_nodes.len() + object_nodes.len()
                        );
                    }
                }
                AxiomType::EquivalentTo => {
                    // Colocation: Equivalent classes should cluster tightly together
                    if let Some(class_b) = &axiom.object {
                        let object_nodes =
                            match graph_repo.get_nodes_by_owl_class_iri(class_b).await {
                                Ok(nodes) => nodes,
                                Err(e) => {
                                    debug!("No nodes found for object IRI '{}': {}", class_b, e);
                                    skipped_count += 1;
                                    continue;
                                }
                            };

                        if object_nodes.is_empty() {
                            debug!("No nodes found with owl_class_iri: {}", class_b);
                            skipped_count += 1;
                            continue;
                        }

                        let mut node_indices: Vec<u32> = Vec::new();
                        node_indices.extend(subject_nodes.iter().map(|n| n.id));
                        node_indices.extend(object_nodes.iter().map(|n| n.id));

                        // Params: [constraint_subtype, force_magnitude]
                        // SubType 4 = Colocation (equivalence)
                        let force_magnitude = self.config.constraint_strength * 1.5; // Strong attraction

                        constraints.push(Constraint {
                            kind: ConstraintKind::Semantic,
                            node_indices,
                            params: vec![4.0, force_magnitude], // subtype=4, magnitude
                            weight: self.config.constraint_strength * 1.5,
                            active: true,
                        });

                        debug!(
                            "Created EquivalentTo constraint: {} ≡ {} ({} nodes)",
                            axiom.subject,
                            class_b,
                            subject_nodes.len() + object_nodes.len()
                        );
                    }
                }
                AxiomType::DisjointWith => {
                    // Separation: Disjoint classes should repel each other
                    if let Some(class_b) = &axiom.object {
                        let object_nodes =
                            match graph_repo.get_nodes_by_owl_class_iri(class_b).await {
                                Ok(nodes) => nodes,
                                Err(e) => {
                                    debug!("No nodes found for object IRI '{}': {}", class_b, e);
                                    skipped_count += 1;
                                    continue;
                                }
                            };

                        if object_nodes.is_empty() {
                            debug!("No nodes found with owl_class_iri: {}", class_b);
                            skipped_count += 1;
                            continue;
                        }

                        let mut node_indices: Vec<u32> = Vec::new();
                        node_indices.extend(subject_nodes.iter().map(|n| n.id));
                        node_indices.extend(object_nodes.iter().map(|n| n.id));

                        // Params: [constraint_subtype, force_magnitude]
                        // SubType 0 = Separation (repulsion)
                        let force_magnitude = self.config.constraint_strength * 2.0; // Strong repulsion

                        constraints.push(Constraint {
                            kind: ConstraintKind::Semantic,
                            node_indices,
                            params: vec![0.0, force_magnitude], // subtype=0, magnitude
                            weight: self.config.constraint_strength * 2.0,
                            active: true,
                        });

                        debug!(
                            "Created DisjointWith constraint: {} ⊥ {} ({} nodes)",
                            axiom.subject,
                            class_b,
                            subject_nodes.len() + object_nodes.len()
                        );
                    }
                }
                _ => {
                    debug!("Skipping axiom type: {:?}", axiom.axiom_type);
                }
            }
        }

        if skipped_count > 0 {
            warn!(
                "⚠️  Skipped {} axioms due to missing nodes in graph",
                skipped_count
            );
        }

        info!(
            "Generated {} constraints from {} axioms ({} skipped)",
            constraints.len(),
            axioms.len(),
            skipped_count
        );

        Ok(ConstraintSet {
            constraints,
            groups: std::collections::HashMap::new(),
        })
    }

    /// Wave 3: materialise reasoner-entailed `SubClassOf` axioms as tagged inferred
    /// GraphData edges (child→parent), for the asserted/inferred visual channel.
    ///
    /// Resolves each axiom's subject/object class IRI to node ids (reusing the same
    /// `get_nodes_by_owl_class_iri` path as constraint generation), forms the
    /// candidate `(child, parent)` pairs, then runs the pure
    /// [`inferred_edge_materialiser::select_inferred_edges`] set-logic (self-loop
    /// drop, dedup, per-child cap). The candidates are already reasoner ENTAILMENTS
    /// (not asserted triples), so the asserted-diff at this site is the empty set;
    /// the graph-state injection path — which has the full edge list in hand — can
    /// pass a populated asserted set to the same pure function. Returns the count of
    /// edges added. Never panics; a resolution miss skips that axiom.
    async fn materialise_inferred_edges_from_axioms(
        &self,
        axioms: &[crate::reasoning::custom_reasoner::InferredAxiom],
    ) -> Result<usize, String> {
        use crate::reasoning::custom_reasoner::AxiomType;
        use crate::services::inferred_edge_materialiser as mat;
        use std::collections::{HashMap, HashSet};

        let graph_repo = self
            .graph_repo
            .as_ref()
            .ok_or_else(|| "Graph repository not configured".to_string())?;

        // Step A: the reasoner emits TRANSITIVE subclass ancestors, so first build
        // the child→ancestor-IRI map and reduce it to IMMEDIATE inferred parents —
        // otherwise deep hierarchies would materialise long-range grandparent edges.
        let mut child_to_ancestors: HashMap<String, HashSet<String>> = HashMap::new();
        for axiom in axioms {
            if axiom.axiom_type != AxiomType::SubClassOf {
                continue;
            }
            if let Some(superclass) = &axiom.object {
                if superclass != &axiom.subject {
                    child_to_ancestors
                        .entry(axiom.subject.clone())
                        .or_default()
                        .insert(superclass.clone());
                }
            }
        }
        let immediate = mat::immediate_inferred_parents(&child_to_ancestors);

        // Step B: project the immediate (child_iri, parent_iri) pairs to node-id
        // pairs via the same IRI resolver the constraint path uses. Cache lookups.
        let mut resolve_cache: HashMap<String, Vec<u32>> = HashMap::new();
        let mut candidates: Vec<(u32, u32)> = Vec::new();
        for (child_iri, parent_iri) in &immediate {
            let child_ids =
                Self::resolve_nodes(graph_repo.as_ref(), &mut resolve_cache, child_iri).await;
            if child_ids.is_empty() {
                continue;
            }
            let parent_ids =
                Self::resolve_nodes(graph_repo.as_ref(), &mut resolve_cache, parent_iri).await;
            for &c in &child_ids {
                for &p in &parent_ids {
                    candidates.push((c, p));
                }
            }
        }

        // Step C: diff against the REAL asserted edge set (both directions) so a
        // materialised edge can never mirror — and thereby overwrite (Edge id is
        // `source-target`) — an existing asserted edge.
        let current = graph_repo
            .load_graph()
            .await
            .map_err(|e| format!("load_graph failed: {}", e))?;
        let cfg = mat::InferredMaterialisationConfig {
            enabled: true,
            max_parents_per_child: self.config.max_inferred_parents_per_child,
        };
        let edges = mat::materialise(&candidates, &current.edges, &cfg);
        if edges.is_empty() {
            return Ok(0);
        }
        let added = edges.len();
        graph_repo
            .batch_add_edges(edges)
            .await
            .map_err(|e| format!("batch_add_edges failed: {}", e))?;
        Ok(added)
    }

    /// Resolve an `owl_class_iri` to its node ids, memoised for the batch.
    async fn resolve_nodes(
        repo: &dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository,
        cache: &mut std::collections::HashMap<String, Vec<u32>>,
        iri: &str,
    ) -> Vec<u32> {
        if let Some(v) = cache.get(iri) {
            return v.clone();
        }
        let ids = repo
            .get_nodes_by_owl_class_iri(iri)
            .await
            .map(|ns| ns.into_iter().map(|n| n.id).collect::<Vec<u32>>())
            .unwrap_or_default();
        cache.insert(iri.to_string(), ids.clone());
        ids
    }

    /// Upload constraints to GPU
    async fn upload_constraints_to_gpu(&self, constraint_set: ConstraintSet) -> Result<(), String> {
        info!(
            "📤 Uploading {} constraints to GPU",
            constraint_set.constraints.len()
        );

        let constraint_actor = self
            .constraint_actor
            .as_ref()
            .ok_or_else(|| "Constraint actor not configured".to_string())?;

        use crate::actors::messages::ApplyOntologyConstraints;
        use crate::actors::messages::ConstraintMergeMode;

        let msg = ApplyOntologyConstraints {
            constraint_set,
            merge_mode: ConstraintMergeMode::Merge,
            graph_id: 0, // Main knowledge graph
        };

        match constraint_actor.send(msg).await {
            Ok(Ok(_)) => {
                info!("Constraints uploaded to GPU successfully");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("❌ Failed to apply constraints: {}", e);
                Err(e)
            }
            Err(e) => {
                error!("❌ Failed to send constraint message: {}", e);
                Err(format!("Mailbox error: {}", e))
            }
        }
    }

    /// Get current configuration
    pub fn get_config(&self) -> &SemanticPhysicsConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: SemanticPhysicsConfig) {
        info!("Updating OntologyPipelineService configuration");
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SemanticPhysicsConfig::default();
        assert!(config.auto_trigger_reasoning);
        assert!(config.auto_generate_constraints);
        assert_eq!(config.constraint_strength, 1.0);
        assert!(config.use_gpu_constraints);
    }

    #[test]
    fn test_pipeline_creation() {
        let config = SemanticPhysicsConfig::default();
        let pipeline = OntologyPipelineService::new(config.clone());
        assert_eq!(pipeline.get_config().constraint_strength, 1.0);
    }
}
