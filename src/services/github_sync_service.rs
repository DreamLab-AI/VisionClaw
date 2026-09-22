// src/services/github_sync_service.rs
//! Corpus Sync Service (ADR-2114)
//!
//! Synchronises the markdown corpus into Oxigraph. The corpus is read through
//! the [`CorpusSource`] port — a mounted vault directory by default, the GitHub
//! repository when selected — and everything downstream of the listing is
//! source-agnostic: parse -> dual graph -> post-sync Whelk reasoning ->
//! inferred-edge materialisation -> `ReloadGraphFromDatabase`.
//!
//! The type keeps its historical `GitHubSyncService` name; renaming it across
//! its 50-odd call sites is deliberately left out of this change.
//! - Parses every page once through `page_parser::parse_page` (`vault_core`);
//!   frontmatter `public: true` pages become knowledge graph nodes
//! - Pages declaring `type: Class|Property|Individual` under `knowledge/`
//!   become ontology nodes, their vocabulary-declared relations typed edges,
//!   and the assert graph is rebuilt from them for Whelk
//! - Uses the source's change marker to process only changed pages (unless FORCE_FULL_SYNC=1)
//! - Batch processing (50 files) to avoid memory issues with large repositories

use crate::adapters::oxigraph_ontology_repository::{OxigraphOntologyRepository, GRAPH_ONTOLOGY};
use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use crate::adapters::SqliteSettingsRepository;
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::services::corpus_source::{CorpusPage, CorpusSource};
use crate::services::decision_elevation::{decision_page_quads_logged, DECISIONS_DIR};
use crate::services::inferred_edge_materialiser as mat;
use crate::services::page_parser::{parse_page, ParsedPage, ONTOLOGY_VAULT_PREFIX};
use crate::services::parsers::KnowledgeGraphParser;
use crate::services::semantic_type_registry::SEMANTIC_TYPE_REGISTRY;
use futures::stream::{FuturesUnordered, StreamExt};
use log::{debug, error, info, warn};
use oxigraph::model::Quad;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vault_core::vocabulary::Vocabulary;
use visionclaw_domain::models::canonical_entity::{CanonicalEntity, EntityKind};
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::ports::inference_engine::InferenceEngine;
use visionclaw_domain::ports::ontology_repository::{AxiomType, OntologyRepository, OwlAxiom};

/// Outcome of ADR-2071 inferred-edge selection on the post-sync Whelk path.
///
/// Split out from `run_post_sync_reasoning` so the selection rules are exercisable
/// without a repository, a reasoner or a live corpus — see the tests at the foot of
/// this file, which pin the new shared-module behaviour against a reference copy of
/// the superseded hand-rolled loop.
#[derive(Debug, Default)]
pub(crate) struct InferredEdgeSelection {
    /// Tagged edges to write, already asserted-pair suppressed and capped.
    pub edges: Vec<Edge>,
    /// `SubClassOf` axioms that survived the vacuous-axiom filter.
    pub considered_axioms: usize,
    /// `(child, parent)` IRI pairs left after the transitive reduction.
    pub immediate_pairs: usize,
    /// Endpoints of those pairs that no node could be resolved for (coverage gate).
    pub unresolved_endpoints: usize,
}

const BATCH_SIZE: usize = 50;

/// Sync-database key holding the identity
/// ([`crate::services::corpus_source::SourceDescriptor::identity`]) of the
/// source the current store was built from.
const SOURCE_IDENTITY_KEY: &str = "corpus_source_identity";

// Predicate IRI constants for edge-type routing.
// Expanded forms (vc: prefix = https://narrativegoldmine.com/ns/v1#).
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const IRI_REQUIRES: &str = "https://narrativegoldmine.com/ns/v1#requires";
const IRI_ENABLES: &str = "https://narrativegoldmine.com/ns/v1#enables";
const IRI_DEPENDS_ON: &str = "https://narrativegoldmine.com/ns/v1#dependsOn";
const IRI_HAS_PART: &str = "https://narrativegoldmine.com/ns/v1#hasPart";
const IRI_IS_PART_OF: &str = "https://narrativegoldmine.com/ns/v1#isPartOf";
const IRI_RELATES_TO: &str = "https://narrativegoldmine.com/ns/v1#relatesTo";
const IRI_BRIDGES_TO: &str = "https://narrativegoldmine.com/ns/v1#bridgesTo";
const IRI_BRIDGES_FROM: &str = "https://narrativegoldmine.com/ns/v1#bridgesFrom";
const IRI_IMPLEMENTS: &str = "https://narrativegoldmine.com/ns/v1#implements";
const IRI_ENHANCES: &str = "https://narrativegoldmine.com/ns/v1#enhances";
const IRI_OPTIMIZES: &str = "https://narrativegoldmine.com/ns/v1#optimizes";
const IRI_SECURES: &str = "https://narrativegoldmine.com/ns/v1#secures";
const IRI_VALIDATES: &str = "https://narrativegoldmine.com/ns/v1#validates";
const IRI_WIKILINK: &str = "https://narrativegoldmine.com/ns/v1#wikilink";

// OWL2 / RDFS / PROV predicates
const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
const OWL_DISJOINT_WITH: &str = "http://www.w3.org/2002/07/owl#disjointWith";
const OWL_INVERSE_OF: &str = "http://www.w3.org/2002/07/owl#inverseOf";
const OWL_SAME_AS: &str = "http://www.w3.org/2002/07/owl#sameAs";
const RDFS_DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
const RDFS_RANGE: &str = "http://www.w3.org/2000/01/rdf-schema#range";
const RDFS_SUB_PROPERTY_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";
const PROV_WAS_DERIVED_FROM: &str = "http://www.w3.org/ns/prov#wasDerivedFrom";
const PROV_WAS_ATTRIBUTED_TO: &str = "http://www.w3.org/ns/prov#wasAttributedTo";
const PROV_WAS_GENERATED_BY: &str = "http://www.w3.org/ns/prov#wasGeneratedBy";
const IRI_ACHIEVES_OBJECTIVE: &str = "https://narrativegoldmine.com/ns/v1#achievesObjective";
const IRI_TRACKED_ON: &str = "https://narrativegoldmine.com/ns/v1#trackedOn";
const IRI_SIMILAR_TO: &str = "https://narrativegoldmine.com/ns/v1#similarTo";
const IRI_SIMULATED_IN: &str = "https://narrativegoldmine.com/ns/v1#simulatedIn";

// New predicates in the NGM schema.
const IRI_USES: &str = "https://narrativegoldmine.com/ns/v1#uses";
const IRI_SUPPORTS: &str = "https://narrativegoldmine.com/ns/v1#supports";
const IRI_CONTRASTS_WITH: &str = "https://narrativegoldmine.com/ns/v1#contrastsWith";
const IRI_STANDARDIZED_BY: &str = "https://narrativegoldmine.com/ns/v1#standardizedBy";
const IRI_APPLIES_TO: &str = "https://narrativegoldmine.com/ns/v1#appliesTo";
const IRI_RELATED_TO: &str = "https://narrativegoldmine.com/ns/v1#relatedTo";
const IRI_PART_OF: &str = "https://narrativegoldmine.com/ns/v1#partOf";
const IRI_INSTANCE_OF: &str = "https://narrativegoldmine.com/ns/v1#instanceOf";
const IRI_NGM_SAME_AS: &str = "https://narrativegoldmine.com/ns/v1#sameAs";
const IRI_DEFINED_IN: &str = "https://narrativegoldmine.com/ns/v1#definedIn";
const IRI_ENABLED_BY: &str = "https://narrativegoldmine.com/ns/v1#enabledBy";
const IRI_UTILISES: &str = "https://narrativegoldmine.com/ns/v1#utilises";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

#[derive(Debug, Clone)]
pub struct SyncStatistics {
    pub total_files: usize,
    pub kg_files_processed: usize,
    pub ontology_files_processed: usize,
    pub skipped_files: usize,
    pub errors: Vec<String>,
    pub duration: Duration,
    pub total_nodes: usize,
    pub total_edges: usize,
}

/// Build a graph node directly from a canonical entity.
///
/// Label, IRIs and node type come from the entity — the page's frontmatter as
/// `vault_core` parsed it. `identity` is the page's §V1 vault identity, the
/// same `metadata_id` the plain path gives a page, so a class and the links
/// into it agree on one node.
fn build_node_from_entity(
    entity: &CanonicalEntity,
    identity: &str,
    id: u32,
    parser: &KnowledgeGraphParser,
) -> visionclaw_domain::models::node::Node {
    use visionclaw_domain::types::BinaryNodeData;

    let mut node = visionclaw_domain::models::node::Node {
        id,
        metadata_id: identity.to_string(),
        label: entity.display_label().to_string(),
        ..Default::default()
    };
    // Population policy: EntityKind is the discriminator — a declared
    // `type: Class|Property` is an `ontology_node`, `type: Individual` an
    // `owl_individual`. When a working-graph page and an ontology class share
    // an identity (the cross-graph join) the class wins: see
    // `SyncScope::yields_to_knowledge`.
    let node_type = entity.kind.as_node_type();
    node.node_type = Some(node_type.to_string());
    if matches!(
        entity.kind,
        EntityKind::OntologyClass | EntityKind::OntologyIndividual
    ) {
        node.owl_class_iri = entity.class_iri.clone();
    }
    node.metadata
        .insert("type".to_string(), node_type.to_string());
    if entity.public {
        node.metadata
            .insert("public".to_string(), "true".to_string());
    }
    if !entity.page_iri.is_empty() {
        node.metadata
            .insert("page_iri".to_string(), entity.page_iri.clone());
    }
    if let Some(ref iri) = entity.class_iri {
        node.metadata.insert("class_iri".to_string(), iri.clone());
    }
    if !entity.slug.is_empty() {
        node.metadata
            .insert("slug".to_string(), entity.slug.clone());
    }

    // Position: reuse existing if present, else random. Going through the
    // parser keeps the existing-positions cache as the single source of truth.
    let (x, y, z) = parser.get_position_public(id);
    node.data = BinaryNodeData {
        node_id: id,
        x,
        y,
        z,
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
    }
    .into();
    node
}

pub struct GitHubSyncService {
    source: Arc<dyn CorpusSource>,
    kg_parser: Arc<KnowledgeGraphParser>,
    kg_repo: Arc<dyn KnowledgeGraphRepository>,
    onto_repo: Arc<OxigraphOntologyRepository>,
    inference_engine: Arc<RwLock<WhelkInferenceEngine>>,
    sync_db: Arc<SqliteSettingsRepository>,
    /// GPUManagerActor address for pushing post-sync semantic constraints to the
    /// live kernel (PRD-018 WS-3 / ADR-098). Set after the GPU actors spin up via
    /// `set_gpu_manager_addr`; `OnceLock` because the service is shared behind
    /// `Arc` and the address is not known at construction time. When unset (e.g.
    /// the `sync_github` CLI binary), the constraint dispatch is skipped.
    gpu_manager_addr: OnceLock<actix::Addr<crate::actors::gpu::gpu_manager_actor::GPUManagerActor>>,
}

impl GitHubSyncService {
    pub fn new(
        source: Arc<dyn CorpusSource>,
        kg_repo: Arc<dyn KnowledgeGraphRepository>,
        onto_repo: Arc<OxigraphOntologyRepository>,
        sync_db: Arc<SqliteSettingsRepository>,
    ) -> Self {
        // The ontology enrichment service is no longer wired into the
        // per-file ingest pass (ADR-090 Phase B replaced its filename-hash
        // node mutations with canonical-entity construction). The reasoner
        // is still used by `run_post_sync_reasoning`, hence the
        // `inference_engine` retention here.
        Self {
            source,
            kg_parser: Arc::new(KnowledgeGraphParser::new()),
            kg_repo,
            onto_repo,
            inference_engine: Arc::new(RwLock::new(WhelkInferenceEngine::new())),
            sync_db,
            gpu_manager_addr: OnceLock::new(),
        }
    }

    /// Register the GPUManagerActor address so post-sync reasoning can push
    /// materialised OWL axioms to the live-kernel constraint buffer. Idempotent;
    /// the first set wins (the address is stable for the process lifetime).
    pub fn set_gpu_manager_addr(
        &self,
        addr: actix::Addr<crate::actors::gpu::gpu_manager_actor::GPUManagerActor>,
    ) {
        if self.gpu_manager_addr.set(addr).is_err() {
            debug!("GitHubSyncService: GPUManagerActor address already set; ignoring");
        }
    }

    /// Synchronize graphs from GitHub — processes in batches with progress logging.
    pub async fn sync_graphs(&self) -> Result<SyncStatistics, String> {
        self.sync_graphs_with(false).await
    }

    /// Run a sync, optionally forcing a full clear + re-process of every file
    /// regardless of the SHA1 filter and the `FORCE_FULL_SYNC` env var. Used
    /// by the admin endpoint (`POST /api/admin/sync?force_full=true`) to
    /// rebuild the store from scratch without a container restart.
    pub async fn sync_graphs_with(
        &self,
        force_full_override: bool,
    ) -> Result<SyncStatistics, String> {
        info!(
            "Starting corpus sync from {} (batch size: {})",
            self.source.describe(),
            BATCH_SIZE
        );
        let start_time = Instant::now();

        let mut stats = SyncStatistics {
            total_files: 0,
            kg_files_processed: 0,
            ontology_files_processed: 0,
            skipped_files: 0,
            errors: Vec::new(),
            duration: Duration::from_secs(0),
            total_nodes: 0,
            total_edges: 0,
        };

        let base_path_changed = self.detect_and_handle_base_path_change().await;

        let files = match self.source.list_pages().await {
            Ok(files) => {
                info!("Found {} markdown pages", files.len());
                files
            }
            Err(e) => {
                let error_msg = format!("Failed to list pages: {}", e);
                error!("{}", error_msg);
                stats.duration = start_time.elapsed();
                return Err(format!("Corpus sync failed: {}", error_msg));
            }
        };

        stats.total_files = files.len();

        // ADR-2040 §V1: build the vault index from the FULL listing, never the
        // changed subset — an incremental sync must still resolve links into
        // unchanged pages, or every one of them mints a phantom stub.
        let base_paths = self.source.base_paths().to_vec();
        let vault_index = visionclaw_domain::vault::VaultIndex::from_identities(
            files
                .iter()
                .map(|f| visionclaw_domain::vault::page_name_from_repo_path(&f.path, &base_paths)),
        );
        info!(
            "Vault index: {} page identities from {} files",
            vault_index.len(),
            files.len()
        );
        let vault_ctx = visionclaw_domain::vault::VaultContext::new(&vault_index, &base_paths);

        // The relation vocabulary: frontmatter relation key -> OWL property.
        // A source that cannot supply one still ingests pages and wikilinks;
        // a vocabulary that exists but will not load is a sync error.
        let vocabulary = match self.source.vocabulary().await {
            Ok(Some(vocab)) => {
                info!(
                    "Vocabulary: {} relation keys drive typed edges",
                    vocab.relations.len()
                );
                Some(vocab)
            }
            Ok(None) => {
                warn!(
                    "No corpus vocabulary from this source — typed relation edges are not emitted"
                );
                None
            }
            Err(e) => {
                error!("Corpus vocabulary failed to load: {}", e);
                stats.errors.push(format!("vocabulary: {}", e));
                None
            }
        };
        // Node ids with a `knowledge/` page behind them, from the FULL listing:
        // a `working/` twin yields to its knowledge page whichever batch
        // either lands in.
        let knowledge_ids: std::collections::HashSet<u32> = files
            .iter()
            .filter(|f| is_knowledge_path(&f.path))
            .map(|f| {
                self.kg_parser
                    .page_name_to_id(&vault_ctx.identity_of(&f.path))
            })
            .collect();
        let scope = SyncScope {
            vault: vault_ctx,
            knowledge_ids: &knowledge_ids,
            vocabulary: vocabulary.as_ref(),
        };

        let force_full_sync = force_full_override
            || base_path_changed
            || std::env::var("FORCE_FULL_SYNC")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false);

        let files_to_process = if force_full_sync {
            info!(
                "Full sync — processing ALL {} files (bypassing SHA1 filter)",
                files.len()
            );
            files.clone()
        } else {
            match self.filter_changed_files(&files).await {
                Ok(filtered) => {
                    info!(
                        "Processing {} changed files ({} unchanged)",
                        filtered.len(),
                        files.len() - filtered.len()
                    );
                    stats.skipped_files = files.len() - filtered.len();
                    filtered
                }
                Err(e) => {
                    error!("SHA1 filter failed: {}", e);
                    files.clone()
                }
            }
        };

        let all_files_to_process = files_to_process.clone();

        // Clear the graph only on a full sync. On an incremental sync (SHA1
        // filter narrowed the file list), existing data must remain — otherwise
        // an unchanged corpus leaves the store empty after the clear + no-op
        // batch loop, wiping out the previous good state.
        if force_full_sync {
            if let Err(e) = self.kg_repo.clear_graph().await {
                error!("Failed to clear graph before sync: {}", e);
                stats.errors.push(format!("clear_graph: {}", e));
            }
        }

        // Collect all deferred (cross-graph bridge) edges across every batch.
        // These reference nodes that may live in different batches, so we write
        // them in a final pass after every node is in the store.
        let mut deferred_edges: Vec<Edge> = Vec::new();

        for (batch_idx, batch) in files_to_process.chunks(BATCH_SIZE).enumerate() {
            let batch_start = Instant::now();
            info!(
                "Processing batch {}/{} ({} files)",
                batch_idx + 1,
                (files_to_process.len() + BATCH_SIZE - 1) / BATCH_SIZE,
                batch.len()
            );

            match self
                .process_batch_incremental(batch, &mut stats, &mut deferred_edges, scope)
                .await
            {
                Ok(_) => {
                    info!(
                        "Batch {} completed in {:?}",
                        batch_idx + 1,
                        batch_start.elapsed()
                    );
                }
                Err(e) => {
                    error!("Batch {} failed: {}", batch_idx + 1, e);
                    stats.errors.push(format!("Batch {}: {}", batch_idx + 1, e));
                }
            }
        }

        // Final pass: resolve deferred edges now that every authored node is
        // present. Wikilink stubs are no longer materialised, so a deferred
        // edge either (a) connects two authored nodes from different batches —
        // write it — or (b) points at a target no file authored (a dangling
        // wikilink). Dangling links contribute NO node and NO edge to the KG;
        // they fold into the physics weight signal instead: +mass per
        // referring page, plus a co-citation spring between pages that share
        // a dangling target (bounded by FANOUT_NODE_THRESHOLD referrers so the
        // pairwise expansion can't explode on hub targets like [[AI]]).
        if !deferred_edges.is_empty() {
            const WEIGHT_PER_FOLDED_LINK: f32 = 0.1;
            const COCITE_WEIGHT: f32 = 0.5;
            let cocite_max_referrers: usize = std::env::var("FANOUT_NODE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n >= 1)
                .unwrap_or(3);

            let graph_snapshot = match self.kg_repo.load_graph().await {
                Ok(g) => Some(g),
                Err(e) => {
                    error!("load_graph for deferred-edge resolution failed: {}", e);
                    stats.errors.push(format!("deferred resolution: {}", e));
                    None
                }
            };
            let existing: std::collections::HashSet<u32> = graph_snapshot
                .as_ref()
                .map(|g| g.nodes.iter().map(|n| n.id).collect())
                .unwrap_or_default();

            let (resolvable, dangling): (Vec<Edge>, Vec<Edge>) = deferred_edges
                .drain(..)
                .partition(|e| existing.contains(&e.source) && existing.contains(&e.target));

            info!(
                "Deferred edge resolution: {} resolvable, {} dangling (folded to weights)",
                resolvable.len(),
                dangling.len()
            );

            if !resolvable.is_empty() {
                match self.kg_repo.batch_add_edges(resolvable).await {
                    Ok(ids) => {
                        info!("Successfully wrote {} deferred edges", ids.len());
                        stats.total_edges += ids.len();
                    }
                    Err(e) => {
                        error!("Deferred edges failed: {}", e);
                        stats.errors.push(format!("deferred edges: {}", e));
                    }
                }
            }

            if !dangling.is_empty() {
                // Group referrers by the missing endpoint.
                let mut referrers: std::collections::HashMap<u32, Vec<u32>> =
                    std::collections::HashMap::new();
                for edge in &dangling {
                    let (missing, real) = if existing.contains(&edge.source) {
                        (edge.target, edge.source)
                    } else if existing.contains(&edge.target) {
                        (edge.source, edge.target)
                    } else {
                        continue; // both endpoints missing — nothing to fold onto
                    };
                    referrers.entry(missing).or_default().push(real);
                }

                let mut weight_bonus: std::collections::HashMap<u32, f32> =
                    std::collections::HashMap::new();
                let mut cocite: std::collections::HashMap<(u32, u32), f32> =
                    std::collections::HashMap::new();
                for refs in referrers.values() {
                    for &n in refs {
                        *weight_bonus.entry(n).or_insert(0.0) += WEIGHT_PER_FOLDED_LINK;
                    }
                    if refs.len() <= cocite_max_referrers {
                        for i in 0..refs.len() {
                            for j in (i + 1)..refs.len() {
                                if refs[i] == refs[j] {
                                    continue;
                                }
                                let key = if refs[i] < refs[j] {
                                    (refs[i], refs[j])
                                } else {
                                    (refs[j], refs[i])
                                };
                                *cocite.entry(key).or_insert(0.0) += COCITE_WEIGHT;
                            }
                        }
                    }
                }

                if !cocite.is_empty() {
                    let cocite_edges: Vec<Edge> = cocite
                        .into_iter()
                        .map(|((a, b), w)| Edge {
                            id: format!("{}_{}_cocite", a, b),
                            source: a,
                            target: b,
                            weight: w,
                            edge_type: Some("co_citation".to_string()),
                            owl_property_iri: None,
                            metadata: None,
                        })
                        .collect();
                    let n_cocite = cocite_edges.len();
                    match self.kg_repo.batch_add_edges(cocite_edges).await {
                        Ok(ids) => {
                            info!(
                                "Wrote {} co-citation springs from dangling wikilinks",
                                ids.len()
                            );
                            stats.total_edges += ids.len();
                        }
                        Err(e) => {
                            warn!(
                                "Co-citation spring write failed (non-fatal, {} edges): {}",
                                n_cocite, e
                            );
                            stats.errors.push(format!("cocite_edges: {}", e));
                        }
                    }
                }

                if !weight_bonus.is_empty() {
                    if let Some(ref g) = graph_snapshot {
                        let updated: Vec<visionclaw_domain::models::node::Node> = g
                            .nodes
                            .iter()
                            .filter_map(|n| {
                                weight_bonus.get(&n.id).map(|bonus| {
                                    let mut node = n.clone();
                                    node.weight = Some(node.weight.unwrap_or(1.0) + bonus);
                                    node
                                })
                            })
                            .collect();
                        let n_rw = updated.len();
                        if !updated.is_empty() {
                            if let Err(e) = self.kg_repo.batch_update_nodes(updated).await {
                                warn!("Dangling-link mass nuance failed (non-fatal): {}", e);
                                stats.errors.push(format!("weight_nuance: {}", e));
                            } else {
                                info!("Re-weighted {} pages from dangling wikilinks", n_rw);
                            }
                        }
                    }
                }
            }
        }

        // Materialise domain root nodes and hierarchical edges to members.
        match self.materialise_domain_roots(&mut stats).await {
            Ok(n) => info!("Materialised {} domain root nodes with edges", n),
            Err(e) => {
                warn!("Domain root materialisation failed (non-fatal): {}", e);
                stats.errors.push(format!("domain_roots: {}", e));
            }
        }

        // Post-sync: fold low-fan-out wikilink stubs into weights + springs.
        match self.fold_low_fanout_stubs(&mut stats).await {
            Ok(n) => info!("Folded {} low-fan-out linked_page stub nodes", n),
            Err(e) => {
                warn!("Low-fan-out stub fold failed (non-fatal): {}", e);
                stats.errors.push(format!("fold_stubs: {}", e));
            }
        }

        // Rebuild the OWL **assert** graph (`urn:ngm:graph:ontology:assert`)
        // from the freshly-synced corpus BEFORE reasoning. `run_post_sync_reasoning`
        // → `onto_repo.get_classes()` (→ `list_owl_classes()`) reads the assert
        // graph, so the rebuild must land first for Whelk + the conflict gate to
        // see the clean current classes rather than the stale historical load.
        // Gated on `force_full_sync`: the CLEAR+INSERT is a full corpus replace,
        // conservative to run only on an operator-driven full sync. The
        // CLEAR-vs-decision-provenance tradeoff is documented on
        // `rebuild_assert_graph`.
        if force_full_sync {
            match self.rebuild_assert_graph(&mut stats).await {
                Ok(n) => info!("Rebuilt assert graph from {} ontology class nodes", n),
                Err(e) => {
                    warn!("Assert-graph rebuild failed (non-fatal): {}", e);
                    stats.errors.push(format!("assert_rebuild: {}", e));
                }
            }
        } else {
            debug!(
                "Incremental sync — skipping assert-graph rebuild (force_full only); \
                 conflict gate + Whelk read the existing assert graph"
            );
        }

        // Post-sync: run Whelk EL++ reasoning over the full ontology graph.
        match self.run_post_sync_reasoning(&mut stats).await {
            Ok(inferred) => info!("Post-sync reasoning produced {} inferred edges", inferred),
            Err(e) => {
                warn!("Post-sync reasoning failed (non-fatal): {}", e);
                stats.errors.push(format!("reasoning: {}", e));
            }
        }

        if let Err(e) = self.update_file_metadata(&all_files_to_process).await {
            warn!("Failed to update file_metadata: {}", e);
        }

        // ADR-114 seed leg (deliverable 2 — the trigger). When the ontology
        // corpus changed this sync, (re-)condense per-class summaries into the
        // RuVector `ontology-classes` namespace and fire
        // `ClassSummaryIndexRefreshed{changed_count}`. Config-gated, default-OFF
        // (`ONTOLOGY_CLASS_INDEX_ENABLED`), and fully fail-open — a failure here
        // never taints the sync result.
        let ontology_changed = stats.ontology_files_processed > 0;
        if ontology_changed {
            match self.onto_repo.list_owl_classes().await {
                Ok(classes) => {
                    let _ = crate::services::ontology_class_index::maybe_refresh_after_sync(
                        ontology_changed,
                        &classes,
                    )
                    .await;
                }
                Err(e) => {
                    // Only a warning: the seed leg is a derived projection; a
                    // failure to list classes must not fail the sync.
                    debug!(
                        "[class-index] skipped refresh — list_owl_classes failed: {}",
                        e
                    );
                }
            }
        }

        stats.duration = start_time.elapsed();
        info!(
            "Sync complete: {} nodes, {} edges in {:?}",
            stats.total_nodes, stats.total_edges, stats.duration
        );

        Ok(stats)
    }

    /// Post-sync: fold low-fan-out wikilink stubs out of the rendered graph.
    ///
    /// `ensure_stub_from_link` materialises a `linked_page` node for every
    /// outbound wikilink target lacking an authored page. Targets cited by only
    /// a handful of pages add no navigable structure — a degree-1 stub cannot
    /// cluster anything (it touches one page), and a low-degree stub is cheaper
    /// expressed as coupling between its referrers than as a body in the graph.
    /// Rather than render these as nodes, this pass folds their signal back into
    /// the real graph:
    ///
    ///   * every page that referenced a folded target gains weight (mass
    ///     nuance), so heavily-cross-referencing pages stay denser in layout;
    ///   * a target cited by ≥2 pages contributes a co-citation spring between
    ///     those pages (bibliographic coupling), so a shared rare concept still
    ///     pulls related pages together — without occupying a node.
    ///
    /// Authored pages, ontology stubs (`owl_class`/`owl_individual`), and
    /// `linked_page` hubs whose fan-out reaches `FANOUT_NODE_THRESHOLD` are left
    /// intact: for a high-degree hub the star (1 node, d edges) is far cheaper
    /// than the co-citation clique (d·(d-1)/2 edges) it would expand into, so
    /// the node *is* the efficient encoding above the threshold.
    ///
    /// `FANOUT_NODE_THRESHOLD` (env, default 3): stubs with global fan-out
    /// strictly below this are folded; ≥ this are kept as hubs. Returns the
    /// number of stub nodes folded out.
    async fn fold_low_fanout_stubs(&self, stats: &mut SyncStatistics) -> Result<usize, String> {
        const WEIGHT_PER_FOLDED_LINK: f32 = 0.1;
        const COCITE_WEIGHT: f32 = 0.5;

        let threshold: usize = std::env::var("FANOUT_NODE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&n| n >= 1)
            .unwrap_or(3);

        let graph = self
            .kg_repo
            .load_graph()
            .await
            .map_err(|e| format!("load_graph: {}", e))?;

        // A stub's degree == its global fan-out (all its edges are inbound
        // source→stub references aggregated across every batch).
        let mut degree: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for edge in &graph.edges {
            *degree.entry(edge.source).or_insert(0) += 1;
            *degree.entry(edge.target).or_insert(0) += 1;
        }

        let fold_ids: std::collections::HashSet<u32> = graph
            .nodes
            .iter()
            .filter(|n| n.node_type.as_deref() == Some("linked_page"))
            .filter(|n| degree.get(&n.id).copied().unwrap_or(0) < threshold)
            .map(|n| n.id)
            .collect();

        if fold_ids.is_empty() {
            return Ok(0);
        }

        // Map each folded stub to the real nodes that referenced it, and gather
        // every star edge incident to a folded stub for removal.
        let mut referrers: std::collections::HashMap<u32, Vec<u32>> =
            std::collections::HashMap::new();
        let mut remove_edge_ids: Vec<String> = Vec::new();
        for edge in &graph.edges {
            let (stub, other) = if fold_ids.contains(&edge.target) {
                (edge.target, edge.source)
            } else if fold_ids.contains(&edge.source) {
                (edge.source, edge.target)
            } else {
                continue;
            };
            remove_edge_ids.push(edge.id.clone());
            // A non-folded referrer is a real page/ontology node; only those
            // carry the folded signal (skip stub↔stub edges).
            if !fold_ids.contains(&other) {
                referrers.entry(stub).or_default().push(other);
            }
        }

        // (a) Mass nuance per referring page; (b) co-citation springs between
        // pages that shared a folded target. `refs` is tiny (< threshold), so
        // the pairwise expansion is bounded.
        let mut weight_bonus: std::collections::HashMap<u32, f32> =
            std::collections::HashMap::new();
        let mut cocite: std::collections::HashMap<(u32, u32), f32> =
            std::collections::HashMap::new();
        for refs in referrers.values() {
            for &n in refs {
                *weight_bonus.entry(n).or_insert(0.0) += WEIGHT_PER_FOLDED_LINK;
            }
            for i in 0..refs.len() {
                for j in (i + 1)..refs.len() {
                    if refs[i] == refs[j] {
                        continue;
                    }
                    let key = if refs[i] < refs[j] {
                        (refs[i], refs[j])
                    } else {
                        (refs[j], refs[i])
                    };
                    *cocite.entry(key).or_insert(0.0) += COCITE_WEIGHT;
                }
            }
        }

        // Remove the star edges into folded stubs.
        if !remove_edge_ids.is_empty() {
            let n_edges = remove_edge_ids.len();
            self.kg_repo
                .batch_remove_edges(remove_edge_ids)
                .await
                .map_err(|e| format!("batch_remove_edges: {}", e))?;
            stats.total_edges = stats.total_edges.saturating_sub(n_edges);
        }

        // Remove the folded stub nodes.
        let fold_node_ids: Vec<u32> = fold_ids.iter().copied().collect();
        let n_nodes = fold_node_ids.len();
        self.kg_repo
            .batch_remove_nodes(fold_node_ids)
            .await
            .map_err(|e| format!("batch_remove_nodes: {}", e))?;
        stats.total_nodes = stats.total_nodes.saturating_sub(n_nodes);

        // Add co-citation springs (deduped; weight accumulated across every
        // folded target two pages shared).
        let n_cocite = cocite.len();
        if !cocite.is_empty() {
            let cocite_edges: Vec<Edge> = cocite
                .into_iter()
                .map(|((a, b), w)| Edge {
                    id: format!("{}_{}_cocite", a, b),
                    source: a,
                    target: b,
                    weight: w,
                    edge_type: Some("co_citation".to_string()),
                    owl_property_iri: None,
                    metadata: None,
                })
                .collect();
            match self.kg_repo.batch_add_edges(cocite_edges).await {
                Ok(ids) => stats.total_edges += ids.len(),
                Err(e) => {
                    warn!("Co-citation spring write failed (non-fatal): {}", e);
                    stats.errors.push(format!("cocite_edges: {}", e));
                }
            }
        }

        // Apply mass nuance to the referring pages that survived the fold.
        let n_reweighted = weight_bonus.len();
        if !weight_bonus.is_empty() {
            let updated: Vec<visionclaw_domain::models::node::Node> = graph
                .nodes
                .iter()
                .filter(|n| !fold_ids.contains(&n.id))
                .filter_map(|n| {
                    weight_bonus.get(&n.id).map(|bonus| {
                        let mut node = n.clone();
                        node.weight = Some(node.weight.unwrap_or(1.0) + bonus);
                        node
                    })
                })
                .collect();
            if !updated.is_empty() {
                if let Err(e) = self.kg_repo.batch_update_nodes(updated).await {
                    warn!("Mass-nuance node update failed (non-fatal): {}", e);
                    stats.errors.push(format!("weight_nuance: {}", e));
                }
            }
        }

        info!(
            "Folded {} low-fan-out linked_page stubs (threshold {}): +{} co-citation springs, {} pages re-weighted",
            n_nodes, threshold, n_cocite, n_reweighted
        );

        Ok(n_nodes)
    }

    /// Create domain root nodes for the 6 NarrativeGoldmine domains and
    /// hierarchical edges from each node whose `group` matches a domain.
    async fn materialise_domain_roots(&self, stats: &mut SyncStatistics) -> Result<usize, String> {
        const DOMAINS: &[(&str, &str)] = &[
            ("spatial-computing", "Spatial Computing"),
            ("artificial-intelligence", "Artificial Intelligence"),
            ("infrastructure", "Infrastructure"),
            ("blockchain", "Blockchain"),
            ("robotics", "Robotics"),
            ("distributed-collaboration", "Distributed Collaboration"),
        ];

        let graph = self
            .kg_repo
            .load_graph()
            .await
            .map_err(|e| format!("load_graph: {}", e))?;

        // Collect domain → member node IDs from existing nodes.
        let mut domain_members: std::collections::HashMap<&str, Vec<u32>> =
            std::collections::HashMap::new();
        for node in &graph.nodes {
            if let Some(ref group) = node.group {
                for &(slug, _) in DOMAINS {
                    if group == slug {
                        domain_members.entry(slug).or_default().push(node.id);
                    }
                }
            }
        }

        let mut domain_nodes = Vec::new();
        let mut domain_edges = Vec::new();
        let mut created = 0;

        for &(slug, label) in DOMAINS {
            let members = match domain_members.get(slug) {
                Some(m) if !m.is_empty() => m,
                _ => continue,
            };

            let mut root = visionclaw_domain::models::node::Node::default();
            root.label = label.to_string();
            root.metadata_id = format!("domain-root-{}", slug);
            root.node_type = Some("domain_root".to_string());
            root.group = Some(slug.to_string());
            root.size = Some(3.0);
            root.weight = Some(1.0);
            root.owl_class_iri = Some(format!("urn:ngm:domain:{}", slug));
            root.metadata
                .insert("type".to_string(), "domain_root".to_string());
            domain_nodes.push(root);
        }

        if domain_nodes.is_empty() {
            return Ok(0);
        }

        let root_ids = self
            .kg_repo
            .batch_add_nodes(domain_nodes)
            .await
            .map_err(|e| format!("batch_add_nodes domain roots: {}", e))?;

        // Map slug → assigned root ID.
        let domain_slugs: Vec<&str> = DOMAINS
            .iter()
            .filter(|(slug, _)| domain_members.contains_key(slug))
            .map(|(slug, _)| *slug)
            .collect();

        for (idx, &root_id) in root_ids.iter().enumerate() {
            let slug = domain_slugs[idx];
            if let Some(members) = domain_members.get(slug) {
                for &member_id in members {
                    let edge = Edge {
                        id: format!("domain_{}_{}", root_id, member_id),
                        source: root_id,
                        target: member_id,
                        weight: 1.5,
                        edge_type: Some("hierarchical".to_string()),
                        owl_property_iri: None,
                        metadata: None,
                    };
                    domain_edges.push(edge);
                }
            }
            created += 1;
        }

        if !domain_edges.is_empty() {
            match self.kg_repo.batch_add_edges(domain_edges.clone()).await {
                Ok(ids) => {
                    stats.total_edges += ids.len();
                    info!(
                        "Created {} domain root edges for {} domains",
                        ids.len(),
                        created
                    );
                }
                Err(e) => warn!("Failed to write domain root edges: {}", e),
            }
        }

        stats.total_nodes += created;
        Ok(created)
    }

    /// Rebuild the Oxigraph OWL **assert** graph (`urn:ngm:graph:ontology:assert`)
    /// from the freshly-synced knowledge-graph nodes.
    ///
    /// ROOT-CAUSE FIX: the per-file ingest builds KG nodes WITH `owl_class_iri`
    /// set (and even classifies ontology nodes via `is_ontology`), but the sync
    /// only ever *read* the assert graph (`get_classes` → Whelk load) and never
    /// wrote it back. So the assert graph stayed frozen on a stale historical
    /// load carrying duplicate concepts, and the conflict gate
    /// (`onto_repo.list_owl_classes()`) kept flagging conflicts that no longer
    /// exist in the corpus. This collects the ontology class nodes — those
    /// stamped with a declared ontology type — plus the class↔class edges into a
    /// `GraphData` and calls `onto_repo.save_ontology_graph`, whose atomic
    /// `CLEAR GRAPH <assert> ; INSERT DATA {…}` rebuilds the assert graph from
    /// the current corpus (dropping the stale duplicates).
    ///
    /// Reuses the already-synced state via `kg_repo.load_graph()` — it does NOT
    /// re-fetch from GitHub. The node set is filtered to ontology classes only,
    /// never the whole KG (which includes plain page / agent / linked_page nodes).
    ///
    /// GATING: the caller invokes this on `force_full_sync` only. The CLEAR is a
    /// full corpus replace — correct as a full rebuild, but deliberately scoped
    /// to the operator-driven full-sync path.
    ///
    /// CLEAR-vs-decision-provenance tradeoff: `save_ontology_graph`'s CLEAR wipes
    /// the ENTIRE assert graph, including any OWL classes/axioms added at runtime
    /// via the governed write door (`add_owl_class` / `add_axiom`,
    /// `application/ontology/directives.rs`). This is acceptable for a corpus
    /// rebuild because:
    ///   • Decision *provenance* (the append-only audit trail) lives in a
    ///     SEPARATE named graph — `urn:ngm:graph:provenance` (GRAPH_PROVENANCE) —
    ///     which this CLEAR does NOT touch. Decision history is preserved.
    ///   • Whelk-*inferred* axioms live in `urn:ngm:graph:ontology:inferred`
    ///     (GRAPH_ONTOLOGY_INFERRED), also untouched by this CLEAR.
    ///   • A `force_full` is an explicit operator "reload from source of truth";
    ///     a governed class enrichment meant to persist is expected to be
    ///     promoted back into the corpus (the visionGraph vault), from which this rebuild
    ///     re-derives it.
    /// This is purely the corpus-ingestion writer; the governed propose /
    /// decision write path is a DIFFERENT writer to the same graph and is NOT
    /// touched here. `save_ontology_graph` itself is left unchanged.
    async fn rebuild_assert_graph(&self, stats: &mut SyncStatistics) -> Result<usize, String> {
        let graph = self
            .kg_repo
            .load_graph()
            .await
            .map_err(|e| format!("load_graph for assert rebuild: {}", e))?;

        // Ontology nodes only — pages that DECLARED themselves ontology in
        // `knowledge/` (PRD-sovereign-corpus Q16). `owl_class_iri.is_some()` is
        // NOT that test: the per-file ingest sets it on any node whose IRI looks
        // ontological, so a public `working/` Episode that mints an IRI was
        // being shipped to Whelk as a class. The declaration is the frontmatter
        // `type`, stamped by `mark_ontology_type` at ingest.
        let onto_nodes: Vec<visionclaw_domain::models::node::Node> = graph
            .nodes
            .iter()
            .filter(|n| n.metadata.contains_key(ONTOLOGY_TYPE_KEY))
            .cloned()
            .collect();

        if onto_nodes.is_empty() {
            info!(
                "No ontology-typed ({}) nodes in KG — skipping assert-graph rebuild",
                ONTOLOGY_TYPE_KEY
            );
            return Ok(0);
        }

        // Keep only edges whose BOTH endpoints are ontology class nodes, so the
        // rebuilt assert graph carries the class↔class relations (subClassOf,
        // hasPart, requires, …) and drops KG-page bridges. save_ontology_graph
        // already skips edges whose endpoints aren't in the node set; we
        // pre-filter to keep the INSERT tight.
        let onto_ids: std::collections::HashSet<u32> = onto_nodes.iter().map(|n| n.id).collect();
        let onto_edges: Vec<Edge> = graph
            .edges
            .iter()
            .filter(|e| onto_ids.contains(&e.source) && onto_ids.contains(&e.target))
            .cloned()
            .collect();

        let count = onto_nodes.len();
        let ontology_graph = visionclaw_domain::models::graph::GraphData {
            nodes: onto_nodes,
            edges: onto_edges,
            metadata: Default::default(),
            id_to_metadata: std::collections::HashMap::new(),
        };

        info!(
            "Rebuilding assert graph <{}>: {} ontology classes, {} class-relation edges (atomic CLEAR+INSERT)",
            GRAPH_ONTOLOGY,
            ontology_graph.nodes.len(),
            ontology_graph.edges.len()
        );

        self.onto_repo
            .save_ontology_graph(&ontology_graph)
            .await
            .map_err(|e| format!("save_ontology_graph: {}", e))?;

        // Honest reporting: count of ontology class nodes written to the assert
        // graph (was initialised 0 and never incremented before this fix).
        stats.ontology_files_processed += count;

        // ADR-050 read-half: the CLEAR+INSERT above rebuilds the assert graph from
        // the corpus CLASSES only (ontology-typed pages), which erases any
        // runtime decision-record instances (`dl:DecisionRecord`, a prov:Activity
        // individual with no owl_class_iri). Re-derive them from the elevated
        // decision pages in the corpus so a force_full preserves the decisions the
        // corpus contains (and only those) — the intended durability-through-resync
        // semantics. Non-fatal: a decision read-half failure never fails the class
        // rebuild.
        let decisions = match self.rematerialise_decisions().await {
            Ok(n) => n,
            Err(e) => {
                warn!("[DecisionElevation] read-half re-materialise failed (non-fatal): {e}");
                0
            }
        };
        stats.ontology_files_processed += decisions;
        Ok(count + decisions)
    }

    /// ADR-050 read-half: re-derive `dl:DecisionRecord` instances from the corpus
    /// into `urn:ngm:graph:ontology:assert` after the class CLEAR+INSERT.
    ///
    /// Lists the elevated decision pages under [`DECISIONS_DIR`] (a fresh corpus
    /// has none → returns 0), recognises each `dl:DecisionRecord` json-ld block
    /// via the shared parser node-typing ([`decision_page_quads_logged`]), and
    /// inserts the asserted decision quads (type memberships + direct causal
    /// edges) AFTER `save_ontology_graph`'s CLEAR so the rebuild does not wipe
    /// them. Attribution is deliberately NOT re-materialised here — the signed
    /// PROV-O attribution stays in the `:provenance` graph (ADR-049); the corpus
    /// page carries only the summary. Returns the count of decision records
    /// re-derived (honest reporting).
    async fn rematerialise_decisions(&self) -> Result<usize, String> {
        let files = match self.source.list_pages_under(DECISIONS_DIR).await {
            Ok(f) => f,
            Err(e) => {
                // A corpus with no decisions namespace yet is the common case.
                info!(
                    "[DecisionElevation] no '{}' namespace to re-materialise ({}); read-half skipped",
                    DECISIONS_DIR, e
                );
                return Ok(0);
            }
        };
        if files.is_empty() {
            return Ok(0);
        }

        let mut quads: Vec<Quad> = Vec::new();
        let mut decisions = 0usize;
        for f in &files {
            let content = match self.source.fetch_page(f).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "[DecisionElevation] fetch decision page '{}' failed: {}",
                        f.path, e
                    );
                    continue;
                }
            };
            let page_quads = decision_page_quads_logged(&content, &f.path);
            if !page_quads.is_empty() {
                decisions += 1;
                quads.extend(page_quads);
            }
        }

        if quads.is_empty() {
            return Ok(0);
        }
        let quad_count = quads.len();
        self.insert_quads_to_store(&quads).await?;
        info!(
            "[DecisionElevation] re-materialised {} decision record(s) ({} quads) into <{}> (ADR-050 read-half)",
            decisions, quad_count, GRAPH_ONTOLOGY
        );
        Ok(decisions)
    }

    /// ADR-2071 — the pure half of post-sync inferred-edge materialisation.
    ///
    /// Delegates every selection rule to [`crate::services::inferred_edge_materialiser`]:
    /// the vacuous-axiom filter (`is_materialisable_subclass_pair`), the reduction of
    /// the reasoner's TRANSITIVE ancestors to IMMEDIATE parents
    /// (`immediate_parents_from_subclass_pairs`), asserted-pair suppression plus the
    /// per-child cap (`select_inferred_edges`), and edge construction with the
    /// `inferred` provenance tag (`build_inferred_edge`). Nothing here re-implements
    /// those rules, so the sync path and `OntologyPipelineService` cannot drift.
    ///
    /// `resolve` maps a class IRI to a node id (the `IriNodeResolver` in production);
    /// `asserted` is the current graph's node-pair set in BOTH directions.
    /// Deterministic: the same inputs always yield the same edge list.
    pub(crate) fn select_inferred_edges_for_sync(
        axioms: &[OwlAxiom],
        resolve: &dyn Fn(&str) -> Option<u32>,
        asserted: &std::collections::HashSet<(u32, u32)>,
    ) -> InferredEdgeSelection {
        // Step A: keep the non-vacuous SubClassOf entailments.
        let mut considered_axioms = 0usize;
        let mut subclass_pairs: Vec<(&str, &str)> = Vec::new();
        for axiom in axioms {
            if axiom.axiom_type == AxiomType::SubClassOf
                && mat::is_materialisable_subclass_pair(&axiom.subject, &axiom.object)
            {
                considered_axioms += 1;
                subclass_pairs.push((axiom.subject.as_str(), axiom.object.as_str()));
            }
        }

        // Step B: Whelk emits the TRANSITIVE closure, so reduce to immediate parents
        // — otherwise deep hierarchies materialise long-range grandparent edges.
        let immediate = mat::immediate_parents_from_subclass_pairs(subclass_pairs);

        // Step C: project IRI pairs to node-id pairs, counting unresolved endpoints
        // for the ≥95% coverage gate. `pair_iris` keeps the first IRI pair that
        // produced each node pair so the written edge keeps its provenance metadata.
        let mut unresolved_endpoints = 0usize;
        let mut candidates: Vec<(u32, u32)> = Vec::with_capacity(immediate.len());
        let mut pair_iris: std::collections::HashMap<(u32, u32), (&str, &str)> =
            std::collections::HashMap::new();
        for (child_iri, parent_iri) in &immediate {
            let child = resolve(child_iri);
            let parent = resolve(parent_iri);
            if child.is_none() {
                unresolved_endpoints += 1;
            }
            if parent.is_none() {
                unresolved_endpoints += 1;
            }
            if let (Some(c), Some(p)) = (child, parent) {
                candidates.push((c, p));
                pair_iris
                    .entry((c, p))
                    .or_insert((child_iri.as_str(), parent_iri.as_str()));
            }
        }

        // Step D: the shared set-logic — self-loop drop, dedup, asserted-pair
        // suppression (both directions), per-child cap — then tagged construction.
        // Metadata is built only for the SELECTED pairs, not once per axiom.
        let selected = mat::select_inferred_edges(
            &candidates,
            asserted,
            mat::DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD,
        );
        let edges = selected
            .into_iter()
            .map(|(c, p)| {
                let mut edge = mat::build_inferred_edge(c, p)
                    .add_metadata("axiom_type".to_string(), "SubClassOf".to_string());
                if let Some((child_iri, parent_iri)) = pair_iris.get(&(c, p)) {
                    edge = edge
                        .add_metadata("source_iri".to_string(), (*child_iri).to_string())
                        .add_metadata("target_iri".to_string(), (*parent_iri).to_string());
                }
                edge
            })
            .collect();

        InferredEdgeSelection {
            edges,
            considered_axioms,
            immediate_pairs: immediate.len(),
            unresolved_endpoints,
        }
    }

    /// Run Whelk EL++ reasoning after all files have been synced.
    /// Loads OWL classes + axioms from Oxigraph, adds the NarrativeGoldmine
    /// property hierarchy, runs inference, stores results, and creates
    /// inferred edges in the knowledge graph.
    async fn run_post_sync_reasoning(&self, stats: &mut SyncStatistics) -> Result<usize, String> {
        let reasoning_start = Instant::now();

        let classes = self
            .onto_repo
            .get_classes()
            .await
            .map_err(|e| format!("Failed to load OWL classes: {}", e))?;
        let mut axioms = self
            .onto_repo
            .get_axioms()
            .await
            .map_err(|e| format!("Failed to load OWL axioms: {}", e))?;

        if classes.is_empty() {
            info!("No OWL classes in store — skipping reasoning");
            return Ok(0);
        }

        axioms.extend(Self::ngm_property_hierarchy_axioms());

        info!(
            "Loading {} classes and {} axioms into Whelk",
            classes.len(),
            axioms.len()
        );

        // Asserted axioms (disjointWith / equivalentClass / explicit subClassOf)
        // carry the layout forces that inference does not re-derive; keep a copy
        // before `load_ontology` consumes the vec so the post-sync constraint
        // dispatch can map them alongside the inferred closure (ADR-098 D1).
        let asserted_axioms = axioms.clone();

        let mut engine = self.inference_engine.write().await;
        engine
            .load_ontology(classes, axioms)
            .await
            .map_err(|e| format!("Whelk load_ontology: {}", e))?;

        let results = engine
            .infer()
            .await
            .map_err(|e| format!("Whelk infer: {}", e))?;

        info!(
            "Whelk produced {} inferred axioms in {}ms",
            results.inferred_axioms.len(),
            results.inference_time_ms
        );

        if let Err(e) = self.onto_repo.store_inference_results(&results).await {
            warn!("Failed to persist inference results: {}", e);
        }

        // PRD-018 WS-2 §B: build the IRI→node index via the LIFTED, reusable
        // `IriNodeResolver` (crate `visionclaw_ontology::services::iri_node_resolver`).
        // The previous inline closure has been promoted to that public struct so
        // the GPU/constraint mapper (ADR-098) can resolve endpoints identically.
        // Behaviour is unchanged: every addressable IRI form is indexed, with a
        // deterministic local-name hash fallback (the same hash that minted
        // every node id), and unresolved endpoints are counted for the
        // ≥95% coverage gate.
        let graph = self.kg_repo.load_graph().await.ok();
        let resolver = match &graph {
            Some(g) => {
                visionclaw_ontology::services::iri_node_resolver::IriNodeResolver::from_nodes(
                    &g.nodes,
                )
            }
            None => visionclaw_ontology::services::iri_node_resolver::IriNodeResolver::new(),
        };

        // ADR-2071: edge selection is the SHARED `inferred_edge_materialiser`
        // set-logic, not a hand-rolled loop. `select_inferred_edges_for_sync` is
        // the pure, unit-testable half (axioms + resolver + asserted set → tagged
        // edges). The asserted pairs come from the snapshot loaded immediately
        // above, so suppression sees the CURRENT edge set — `materialise_domain_roots`
        // and `fold_low_fanout_stubs` have already mutated the graph by this point
        // in `sync_graphs`, which is why this load cannot be folded into an earlier
        // one (see the ADR-2071 verification note).
        let asserted = graph
            .as_ref()
            .map(|g| mat::asserted_pairs(&g.edges))
            .unwrap_or_default();
        let InferredEdgeSelection {
            edges: inferred_edges,
            considered_axioms,
            immediate_pairs,
            unresolved_endpoints,
        } = Self::select_inferred_edges_for_sync(
            &results.inferred_axioms,
            &|iri| resolver.resolve(iri),
            &asserted,
        );

        let mut inferred_edge_count = 0;
        if !inferred_edges.is_empty() {
            info!(
                "Creating {} inferred edges (ADR-2071 shared selection: {} SubClassOf axioms → {} immediate parent pairs, capped at {} parents per child, asserted pairs suppressed)",
                inferred_edges.len(),
                considered_axioms,
                immediate_pairs,
                mat::DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD
            );
            match self.kg_repo.batch_add_edges(inferred_edges).await {
                Ok(ids) => {
                    inferred_edge_count = ids.len();
                    stats.total_edges += inferred_edge_count;
                }
                Err(e) => warn!("Failed to write inferred edges: {}", e),
            }
        }

        // WS-0 release gate: report IRI→node endpoint resolution coverage so
        // the historical "30–50% silent drop" is now observable, not silent.
        // ADR-2071: the denominator is the IMMEDIATE-parent pair set (post
        // transitive reduction), not every considered axiom — those are the pairs
        // materialisation actually has to resolve.
        let total_endpoints = immediate_pairs * 2;
        if total_endpoints > 0 {
            let resolved = total_endpoints.saturating_sub(unresolved_endpoints);
            let coverage = (resolved as f64 / total_endpoints as f64) * 100.0;
            if unresolved_endpoints > 0 {
                warn!(
                    "IRI→node resolution: {}/{} endpoints resolved ({:.1}%); {} unresolved across {} immediate inferred parent pairs from {} SubClassOf axioms (target ≥95%)",
                    resolved,
                    total_endpoints,
                    coverage,
                    unresolved_endpoints,
                    immediate_pairs,
                    considered_axioms
                );
            } else {
                info!(
                    "IRI→node resolution: {}/{} endpoints resolved (100.0%) across {} immediate inferred parent pairs from {} SubClassOf axioms",
                    resolved, total_endpoints, immediate_pairs, considered_axioms
                );
            }
        }

        // PRD-018 WS-3 / ADR-098 D1: push the materialised axioms (asserted +
        // inferred) to the GPU as live-kernel semantic constraints. This is the
        // producer that makes subClassOf attraction / disjointWith separation /
        // sameAs colocation actually move nodes. Skipped (logged) when no
        // GPUManagerActor address is registered (e.g. the sync_github CLI).
        if let Some(graph) = graph {
            self.dispatch_semantic_constraints(
                asserted_axioms,
                results.inferred_axioms,
                (*graph).clone(),
            )
            .await;
        } else {
            warn!("Post-sync reasoning: graph unavailable, skipping semantic constraint dispatch");
        }

        info!(
            "Post-sync reasoning complete in {:?}: {} inferred edges",
            reasoning_start.elapsed(),
            inferred_edge_count
        );
        Ok(inferred_edge_count)
    }

    /// PRD-018 WS-3 / ADR-098 D1 — map materialised OWL axioms (asserted +
    /// Whelk-inferred) to live-kernel constraints and upload them to the GPU.
    ///
    /// Sends `ApplyMaterializedAxioms` to the GPUManagerActor, which routes to
    /// the OntologyConstraintActor where the canonical `map_axioms_to_constraints`
    /// anti-corruption mapper runs. No-op (logged) when the GPU address is unset.
    async fn dispatch_semantic_constraints(
        &self,
        asserted_axioms: Vec<OwlAxiom>,
        inferred_axioms: Vec<OwlAxiom>,
        graph: visionclaw_domain::models::graph::GraphData,
    ) {
        let Some(gpu_addr) = self.gpu_manager_addr.get() else {
            info!(
                "Post-sync reasoning: GPUManagerActor address not registered — {} asserted + {} inferred axioms NOT pushed as constraints (CLI/headless run)",
                asserted_axioms.len(),
                inferred_axioms.len()
            );
            return;
        };

        let mut materialized = asserted_axioms;
        materialized.extend(inferred_axioms);
        let axiom_count = materialized.len();

        info!(
            "Post-sync reasoning: dispatching {} materialised axioms over {} nodes to the GPU constraint mapper",
            axiom_count,
            graph.nodes.len()
        );

        let msg = crate::actors::messages::ApplyMaterializedAxioms {
            axioms: materialized,
            graph_data: graph,
        };

        match gpu_addr.send(msg).await {
            Ok(Ok(produced)) => info!(
                "Post-sync reasoning: {} live-kernel semantic constraints produced from {} axioms",
                produced, axiom_count
            ),
            Ok(Err(e)) => warn!("Post-sync reasoning: constraint mapping failed: {}", e),
            Err(e) => warn!("Post-sync reasoning: GPUManagerActor mailbox error: {}", e),
        }
    }

    /// NarrativeGoldmine property hierarchy axioms for Whelk reasoning.
    /// Declares: requires subPropertyOf dependsOn,
    /// uses/supports/implements subPropertyOf utilises,
    /// hasPart/isPartOf transitive, relatesTo/similarTo symmetric,
    /// hasPart inverseOf isPartOf, enables inverseOf enabledBy.
    fn ngm_property_hierarchy_axioms() -> Vec<OwlAxiom> {
        let sub_prop = |sub: &str, sup: &str| OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubPropertyOf,
            subject: format!("https://narrativegoldmine.com/ns/v1#{sub}"),
            object: format!("https://narrativegoldmine.com/ns/v1#{sup}"),
            annotations: std::collections::HashMap::new(),
        };
        let transitive = |prop: &str| OwlAxiom {
            id: None,
            axiom_type: AxiomType::TransitiveProperty,
            subject: format!("https://narrativegoldmine.com/ns/v1#{prop}"),
            object: String::new(),
            annotations: std::collections::HashMap::new(),
        };
        let symmetric = |prop: &str| OwlAxiom {
            id: None,
            axiom_type: AxiomType::SymmetricProperty,
            subject: format!("https://narrativegoldmine.com/ns/v1#{prop}"),
            object: String::new(),
            annotations: std::collections::HashMap::new(),
        };
        let inverse = |p1: &str, p2: &str| OwlAxiom {
            id: None,
            axiom_type: AxiomType::InverseProperties,
            subject: format!("https://narrativegoldmine.com/ns/v1#{p1}"),
            object: format!("https://narrativegoldmine.com/ns/v1#{p2}"),
            annotations: std::collections::HashMap::new(),
        };

        vec![
            // Property hierarchy: requires subPropertyOf dependsOn
            sub_prop("requires", "dependsOn"),
            // uses, supports, implements subPropertyOf utilises
            sub_prop("uses", "utilises"),
            sub_prop("supports", "utilises"),
            sub_prop("implements", "utilises"),
            // Transitive properties
            transitive("hasPart"),
            transitive("isPartOf"),
            transitive("dependsOn"),
            // Symmetric properties
            symmetric("relatesTo"),
            symmetric("similarTo"),
            // Inverse property pairs
            inverse("hasPart", "isPartOf"),
            inverse("enables", "enabledBy"),
            inverse("implements", "implementedBy"),
        ]
    }

    /// Process a batch of files incrementally — adds nodes/edges to an
    /// already-cleared store without wiping previous batches. Bridge edges
    /// (cross-graph, e.g. agent↔knowledge) are collected into `deferred_edges`
    /// for a final pass after all nodes from every batch are present.
    async fn process_batch_incremental(
        &self,
        files: &[CorpusPage],
        stats: &mut SyncStatistics,
        deferred_edges: &mut Vec<Edge>,
        scope: SyncScope<'_>,
    ) -> Result<(), String> {
        let mut batch_nodes = std::collections::HashMap::new();
        let mut batch_edges = std::collections::HashMap::new();
        let mut public_pages = std::collections::HashSet::new();
        // IDs in `batch_nodes` that are wikilink/IRI stubs rather than authored
        // nodes. Stubs persist via the insert-if-absent path so they can never
        // overwrite a real node already in the store (from an earlier batch or
        // a previous incremental sync).
        let mut batch_stub_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();

        const PARALLEL_FETCHES: usize = 8;

        fn create_fetch_future(
            source: Arc<dyn CorpusSource>,
            file: CorpusPage,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = (CorpusPage, Result<String, String>)> + Send>,
        > {
            Box::pin(async move {
                let result = source.fetch_page(&file).await;
                (file, result)
            })
        }

        let mut fetch_futures: FuturesUnordered<_> = FuturesUnordered::new();
        let mut fetched_contents: Vec<(CorpusPage, Result<String, String>)> =
            Vec::with_capacity(files.len());
        let mut file_iter = files.iter().cloned().peekable();

        while fetch_futures.len() < PARALLEL_FETCHES {
            if let Some(file) = file_iter.next() {
                fetch_futures.push(create_fetch_future(Arc::clone(&self.source), file));
            } else {
                break;
            }
        }

        while let Some((file, content_result)) = fetch_futures.next().await {
            fetched_contents.push((file, content_result));
            if let Some(file) = file_iter.next() {
                fetch_futures.push(create_fetch_future(Arc::clone(&self.source), file));
            }
        }

        for (idx, (file, content_result)) in fetched_contents.into_iter().enumerate() {
            if idx % 10 == 0 && idx > 0 {
                info!(
                    "  Progress: {}/{} files (nodes: {}, edges: {})",
                    idx,
                    files.len(),
                    batch_nodes.len(),
                    batch_edges.len()
                );
            }

            match content_result {
                Ok(content) => {
                    match self
                        .process_fetched_file(
                            &file,
                            &content,
                            &mut batch_nodes,
                            &mut batch_edges,
                            &mut public_pages,
                            &mut batch_stub_ids,
                            scope,
                        )
                        .await
                    {
                        Ok(()) => {
                            stats.kg_files_processed += 1;
                        }
                        Err(e) => {
                            warn!("Error processing {}: {}", file.name, e);
                            stats.errors.push(format!("{}: {}", file.name, e));
                        }
                    }
                }
                Err(e) => {
                    warn!("Error fetching {}: {}", file.name, e);
                    stats.errors.push(format!("{}: {}", file.name, e));
                }
            }
        }

        if !batch_nodes.is_empty() {
            let node_vec: Vec<_> = batch_nodes.into_values().collect();
            let all_edges: Vec<_> = batch_edges.into_values().collect();

            info!(
                "Adding batch: {} nodes ({} stubs), {} edges",
                node_vec.len(),
                batch_stub_ids.len(),
                all_edges.len()
            );

            // Collect node IDs in this batch for bridge-edge detection.
            let batch_node_ids: std::collections::HashSet<u32> =
                node_vec.iter().map(|n| n.id).collect();

            // Authored nodes UPSERT (replace prior triples); stubs insert only
            // if the id is absent from the store, so a wikilink stub can never
            // wipe or pollute a real node written by an earlier batch or a
            // previous incremental sync.
            let (real_nodes, stub_nodes): (Vec<_>, Vec<_>) = node_vec
                .into_iter()
                .partition(|n| !batch_stub_ids.contains(&n.id));

            match self.kg_repo.batch_add_nodes(real_nodes).await {
                Ok(ids) => {
                    stats.total_nodes += ids.len();
                    info!("  Wrote {} authored nodes", ids.len());
                }
                Err(e) => {
                    error!("batch_add_nodes failed: {}", e);
                    return Err(format!("batch_add_nodes: {}", e));
                }
            }

            match self.kg_repo.batch_add_nodes_if_absent(stub_nodes).await {
                Ok(ids) => {
                    stats.total_nodes += ids.len();
                    info!("  Wrote {} stub nodes (if-absent)", ids.len());
                }
                Err(e) => {
                    error!("batch_add_nodes_if_absent failed: {}", e);
                    return Err(format!("batch_add_nodes_if_absent: {}", e));
                }
            }

            // Partition edges: same-batch edges (both endpoints in this batch)
            // are written immediately; cross-batch edges are deferred.
            let mut immediate_edges = Vec::new();
            for edge in all_edges {
                if batch_node_ids.contains(&edge.source) && batch_node_ids.contains(&edge.target) {
                    immediate_edges.push(edge);
                } else {
                    deferred_edges.push(edge);
                }
            }

            if !immediate_edges.is_empty() {
                match self.kg_repo.batch_add_edges(immediate_edges.clone()).await {
                    Ok(ids) => {
                        stats.total_edges += ids.len();
                        info!(
                            "  Wrote {} same-batch edges ({} deferred)",
                            ids.len(),
                            deferred_edges.len()
                        );
                    }
                    Err(e) => {
                        warn!("batch_add_edges (same-batch) failed: {} — deferring all", e);
                        deferred_edges.extend(immediate_edges);
                    }
                }
            }
        } else {
            warn!("Batch is empty after processing — nothing to save");
        }

        Ok(())
    }

    /// Process one pre-fetched file, populating nodes/edges in-place.
    ///
    /// The page is parsed once, by [`parse_page`] (`vault_core`). A page that
    /// declares ontology content — `type: Class|Property|Individual` under
    /// `knowledge/` — becomes a canonical ontology node here; every other page
    /// takes the plain markdown/wikilink path. Both paths apply the same §V4
    /// gate. Identity is the page's §V1 vault identity on both paths, so a
    /// class and every link into it agree on one node id.
    // The four `&mut` accumulators are one logical value (the batch being
    // built) and would read better bundled; that refactor touches every call
    // site in this file and is deliberately left for its own change.
    #[allow(clippy::too_many_arguments)]
    async fn process_fetched_file(
        &self,
        file: &CorpusPage,
        content: &str,
        nodes: &mut std::collections::HashMap<u32, visionclaw_domain::models::node::Node>,
        edges: &mut std::collections::HashMap<String, Edge>,
        public_pages: &mut std::collections::HashSet<String>,
        stub_ids: &mut std::collections::HashSet<u32>,
        scope: SyncScope<'_>,
    ) -> Result<(), String> {
        debug!("Processing file: {} ({} bytes)", file.name, content.len());

        let parsed = match parse_page(content, &file.path) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => {
                self.process_plain_vault_file(file, content, nodes, edges, stub_ids, scope);
                return Ok(());
            }
            Err(e) => {
                // Unreadable frontmatter means an unreadable publication flag:
                // fail closed.
                warn!("Skipping {}: {}", file.name, e);
                return Ok(());
            }
        };

        // §V4: the one publication gate, shared with the plain path. A class
        // page is published iff its frontmatter says `public: true`, which is
        // the boundary `vault build` projects the ontology through too.
        if !page_is_kg_included(content) {
            debug!(
                "Skipped non-public {} page: {}",
                parsed.ontology_type, file.name
            );
            return Ok(());
        }

        let identity = scope.vault.identity_of(&file.path);
        let source_id = self.kg_parser.page_name_to_id(&identity);
        let entity = &parsed.entity;
        let mut page_node =
            build_node_from_entity(entity, &identity, source_id, self.kg_parser.as_ref());
        enrich_node_from_frontmatter(&mut page_node, &parsed, &identity, content);
        // WS-0: guarantee a non-NULL source_domain for this node.
        ensure_source_domain(&mut page_node, &file.path);
        // Total outbound wikilink degree (resolved + dangling). Dangling links
        // do not materialise stub nodes, so this count is the weight signal
        // the GPU can consume for connectivity-based mass.
        page_node.metadata.insert(
            "wikilink_count".to_string(),
            entity.outbound_links.len().to_string(),
        );
        // Q16: the ontology bundle is built from this stamp, never from a
        // node's IRI shape or its directory alone.
        page_node
            .metadata
            .insert(ONTOLOGY_TYPE_KEY.to_string(), parsed.ontology_type.clone());
        nodes.insert(source_id, page_node);
        // A real authored node always supersedes a stub — or a yielding
        // working-graph twin — that a sibling file put in this batch earlier.
        stub_ids.remove(&source_id);
        if entity.public {
            public_pages.insert(entity.slug.clone());
        }

        // Outbound wikilinks — the curated `links` list, else the body scan.
        // No stub is materialised for a missing target: dangling edges are
        // pruned at the deferred pass and feed `wikilink_count` instead.
        for link in &entity.outbound_links {
            let resolved = scope.vault.resolve(&link.target_slug, &identity);
            let target_id = self.kg_parser.page_name_to_id(resolved.target());
            if target_id == source_id {
                continue;
            }
            let edge_id = format!("{}_{}_wikilink", source_id, target_id);
            edges.entry(edge_id.clone()).or_insert_with(|| Edge {
                id: edge_id,
                source: source_id,
                target: target_id,
                weight: 1.0,
                edge_type: Some("explicit_link".to_string()),
                metadata: None,
                owl_property_iri: None,
            });
        }

        // Elevation provenance: `elevatedFrom: "[[X]]"` becomes a typed bridge
        // edge from the class node to its working-graph origin page. Targets
        // that are not authored nodes fold to weight at the deferred pass like
        // any dangling link.
        if let Some(name) = visionclaw_domain::vault::parse(content).elevated_from {
            let resolved = scope.vault.resolve(&name, &identity);
            let target_id = self.kg_parser.page_name_to_id(resolved.target());
            if target_id != source_id {
                let edge_id = format!("{}_{}_elevated_from", source_id, target_id);
                edges.entry(edge_id.clone()).or_insert_with(|| Edge {
                    id: edge_id,
                    source: source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("elevated_from".to_string()),
                    metadata: None,
                    owl_property_iri: None,
                });
            }
        }

        // Typed semantic edges from the frontmatter relations the vocabulary
        // declares (`is-a` -> rdfs:subClassOf, `requires` -> vc:requires, …).
        // Each overwrites the generic wikilink edge for the same pair, so the
        // semantic type wins; the predicate IRI rides on the edge so the
        // assert-graph rebuild writes the real axiom. Like wikilinks, typed
        // edges join AUTHORED nodes only: a long-tail target with no page
        // mints no node (`vault build` models those as `skos:Concept` tail
        // stubs, not classes) and its edge is pruned at the deferred pass.
        if let Some(vocab) = scope.vocabulary {
            for edge in self.relation_edges(&parsed, vocab, &identity, source_id, scope) {
                edges.remove(&format!("{}_{}_wikilink", source_id, edge.target));
                edges.insert(edge.id.clone(), edge);
            }
        }

        Ok(())
    }

    /// One typed edge per frontmatter relation target, for every relation key
    /// the vocabulary declares. A relation whose property has no edge-type
    /// mapping is skipped (and counted in the debug log): its targets are
    /// still in the page's `links` list, so connectivity is not lost.
    fn relation_edges(
        &self,
        parsed: &ParsedPage,
        vocab: &Vocabulary,
        identity: &str,
        source_id: u32,
        scope: SyncScope<'_>,
    ) -> Vec<Edge> {
        let mut out = Vec::new();
        let mut unmapped = 0usize;
        for (key, def) in &vocab.relations {
            let targets = parsed.page.frontmatter.wikilinks(key);
            if targets.is_empty() {
                continue;
            }
            let predicate = vocab.expand(&def.owl);
            let edge_type = predicate_to_edge_type(&predicate);
            if edge_type.is_empty() {
                unmapped += targets.len();
                continue;
            }
            let reg_id = SEMANTIC_TYPE_REGISTRY.get_or_register_id(edge_type);
            // Normalise the registry's 0-1 strength to the 0-2 spring range.
            let weight = SEMANTIC_TYPE_REGISTRY
                .get_config(reg_id)
                .map(|c| c.strength * 2.0)
                .unwrap_or(1.0);
            for link in targets {
                let resolved = scope.vault.resolve(&link.target, identity);
                let target_id = self.kg_parser.page_name_to_id(resolved.target());
                if target_id == source_id {
                    continue;
                }
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("relation".to_string(), key.clone());
                out.push(Edge {
                    id: format!("{}_{}_{}", source_id, target_id, key),
                    source: source_id,
                    target: target_id,
                    weight,
                    edge_type: Some(edge_type.to_string()),
                    owl_property_iri: Some(predicate.clone()),
                    metadata: Some(metadata),
                });
            }
        }
        if unmapped > 0 {
            debug!(
                "{}: {} relation target(s) under properties with no edge-type mapping",
                parsed.entity.source_path, unmapped
            );
        }
        out
    }

    /// Ingest for every page that declares no ontology content — the working
    /// knowledge graph and any untyped knowledge page. The markdown parser
    /// emits a `page` node plus an edge for every `[[wikilink]]`; targets that
    /// another file materialises as a real node connect, the rest dangle
    /// harmlessly until their page syncs.
    ///
    /// Identity is the page's §V1 vault identity, hashed exactly as the
    /// ontology path hashes it, so a working-graph page and a knowledge page
    /// sharing an identity resolve to one node — the intended cross-graph
    /// join. The knowledge page wins that join: a working twin is written as
    /// a yielding (insert-if-absent) node, so it never clobbers the class
    /// whichever batch either lands in.
    fn process_plain_vault_file(
        &self,
        file: &CorpusPage,
        content: &str,
        nodes: &mut std::collections::HashMap<u32, visionclaw_domain::models::node::Node>,
        edges: &mut std::collections::HashMap<String, Edge>,
        stub_ids: &mut std::collections::HashSet<u32>,
        scope: SyncScope<'_>,
    ) {
        // §V4 gate: frontmatter `public: true` (or a valid `owl-class`).
        if !page_is_kg_included(content) {
            debug!(
                "Skipped non-public page: {} (no frontmatter `public: true`/`owl-class`)",
                file.name
            );
            return;
        }

        // §V1 identity: the vault-relative path, NOT `file.name`. A basename
        // collapses every namespaced page onto its leaf, which merged distinct
        // pages (`ETSI_Domain_Infrastructure/Security` with the root
        // `Security`) and orphaned every bare link into a subfolder.
        let vault_path = format!("{}.md", scope.vault.identity_of(&file.path));
        let parsed =
            self.kg_parser
                .parse_with_index(content, &vault_path, Some(scope.vault.index()));

        // Parser output mixes the authored page node with `linked_page` stubs
        // for its wikilink targets. Stubs are DROPPED entirely: wikilinks
        // contribute edges between authored nodes only (dangling edges are
        // pruned at the deferred pass), plus a wikilink_count weight signal on
        // the page node. Materialising stubs put 11k+ phantom nodes in the
        // Knowledge population.
        let wikilink_count = parsed.edges.len();
        for mut node in parsed.nodes {
            let is_stub = node
                .metadata
                .get("type")
                .map(|t| t == "linked_page")
                .unwrap_or(false);
            if is_stub {
                continue;
            }
            // WS-0: plain working-graph pages never carry a `vc:sourceDomain`
            // quad, so without this they were the bulk of the ~100%-NULL
            // MetadataStore. Derive a deterministic domain from path + label.
            ensure_source_domain(&mut node, &file.path);
            node.metadata
                .insert("wikilink_count".to_string(), wikilink_count.to_string());
            // A working twin of a knowledge page yields: it is kept only where
            // nothing else stands, and is written insert-if-absent like a stub.
            let yields = scope.yields_to_knowledge(&file.path, node.id);
            match nodes.entry(node.id) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    // A real authored node upgrades a stub an earlier sibling
                    // materialised; it never clobbers another authored node.
                    if !yields && stub_ids.remove(&node.id) {
                        e.insert(node);
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    if yields {
                        stub_ids.insert(node.id);
                    }
                    e.insert(node);
                }
            }
        }

        for edge in parsed.edges {
            edges.entry(edge.id.clone()).or_insert(edge);
        }
    }

    /// Insert quads into the Oxigraph store via spawn_blocking.
    async fn insert_quads_to_store(&self, quads: &[Quad]) -> Result<(), String> {
        let store = Arc::clone(self.onto_repo.store());
        let quads_owned: Vec<Quad> = quads.to_vec();
        tokio::task::spawn_blocking(move || {
            store
                .transaction(|mut tx| {
                    for quad in &quads_owned {
                        tx.insert(quad)?;
                    }
                    Ok(()) as Result<(), oxigraph::store::StorageError>
                })
                .map_err(|e| format!("Oxigraph transaction error: {}", e))
        })
        .await
        .map_err(|e| format!("spawn_blocking join error: {}", e))?
    }

    // ------------------------------------------------------------------
    // File listing + SHA1 change detection
    // ------------------------------------------------------------------

    async fn filter_changed_files(&self, files: &[CorpusPage]) -> Result<Vec<CorpusPage>, String> {
        let existing = self.get_existing_file_metadata().await?;

        // Key on the full repo path, NOT the basename: the source dirs
        // (e.g. mainKnowledgeGraph/pages/ + workingGraph/pages/) share
        // hundreds of basenames, and a shared key makes each pair overwrite
        // the other's SHA1 every sync — those files then re-process forever,
        // re-stamping their (id-colliding) node triples on every run.
        Ok(files
            .iter()
            .filter(|f| match existing.get(&f.path) {
                Some(marker) if marker == &f.change_marker => false,
                _ => true,
            })
            .cloned()
            .collect())
    }

    // ------------------------------------------------------------------
    // SHA1 / SyncConfig persistence via SQLite
    // ------------------------------------------------------------------

    async fn get_existing_file_metadata(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        info!("[SHA1] Querying SQLite for existing file SHA1 hashes");

        let map = self
            .sync_db
            .get_file_sha1s()
            .await
            .map_err(|e| format!("SQLite query error: {}", e))?;

        info!("[SHA1] Found {} existing SHA1 hashes", map.len());
        Ok(map)
    }

    async fn update_file_metadata(&self, files: &[CorpusPage]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }

        info!("[SHA1] Updating {} file SHA1 hashes in SQLite", files.len());

        // Full path as key — see filter_changed_files for why basenames
        // collide across source dirs.
        let pairs: Vec<(String, String)> = files
            .iter()
            .map(|f| (f.path.clone(), f.change_marker.clone()))
            .collect();

        self.sync_db
            .upsert_file_sha1s(&pairs)
            .await
            .map_err(|e| format!("SQLite update error: {}", e))
    }

    /// Detect a change of corpus source (kind, location or base paths) and
    /// clear stale data when it changed. Returns true on a change, which
    /// forces a full re-sync — a store built from one source is never topped
    /// up incrementally from another.
    async fn detect_and_handle_base_path_change(&self) -> bool {
        let current_identity = self.source.describe().identity();

        let stored_identity = match self.sync_db.get_sync_config(SOURCE_IDENTITY_KEY).await {
            Ok(val) => val,
            Err(e) => {
                warn!("Failed to read sync config: {}", e);
                None
            }
        };

        let changed = match &stored_identity {
            Some(stored) if stored == &current_identity => false,
            Some(stored) => {
                info!(
                    "Corpus source changed: '{}' -> '{}' — clearing stale data",
                    stored, current_identity
                );
                true
            }
            None => {
                info!(
                    "First sync run for this store — recording corpus source '{}'",
                    current_identity
                );
                false
            }
        };

        if changed {
            if let Err(e) = self.clear_stale_data().await {
                error!("Failed to clear stale data: {}", e);
            }
        }

        if let Err(e) = self
            .sync_db
            .set_sync_config(SOURCE_IDENTITY_KEY, &current_identity)
            .await
        {
            warn!("Failed to save the corpus source identity: {}", e);
        }

        changed
    }

    /// Clear all stale data when switching to a different corpus source.
    /// Clears Oxigraph ontology graph (actual RDF data) and SQLite sync metadata.
    async fn clear_stale_data(&self) -> Result<(), String> {
        info!("Clearing stale data for fresh ingest");

        // Clear Oxigraph ontology graph (real RDF data, not metadata).
        let update = format!("CLEAR GRAPH <{GRAPH_ONTOLOGY}>");
        let store = Arc::clone(self.onto_repo.store());
        tokio::task::spawn_blocking(move || {
            store
                .update(&update)
                .map_err(|e| format!("SPARQL clear error: {}", e))
        })
        .await
        .map_err(|e| format!("join error: {}", e))??;

        // Clear SQLite sync metadata (file hashes + config).
        self.sync_db
            .clear_sync_metadata()
            .await
            .map_err(|e| format!("SQLite clear error: {}", e))
    }

    // ------------------------------------------------------------------
    // Dead-code-safe filter helpers (kept for future use)
    // ------------------------------------------------------------------

    #[allow(dead_code)]
    fn filter_linked_pages(
        &self,
        nodes: &mut std::collections::HashMap<u32, visionclaw_domain::models::node::Node>,
        public_pages: &std::collections::HashSet<String>,
    ) {
        let before = nodes.len();
        nodes.retain(
            |_, node| match node.metadata.get("type").map(|s| s.as_str()) {
                Some("page") => true,
                Some("linked_page") => public_pages.contains(&node.metadata_id),
                _ => true,
            },
        );
        let filtered = before - nodes.len();
        if filtered > 0 {
            info!("Filtered {} linked_page nodes", filtered);
        }
    }

    #[allow(dead_code)]
    fn filter_orphan_edges(
        &self,
        edges: &mut std::collections::HashMap<String, Edge>,
        nodes: &std::collections::HashMap<u32, visionclaw_domain::models::node::Node>,
    ) {
        let before = edges.len();
        edges
            .retain(|_, edge| nodes.contains_key(&edge.source) && nodes.contains_key(&edge.target));
        let filtered = before - edges.len();
        if filtered > 0 {
            info!("Filtered {} orphan edges", filtered);
        }
    }
}

// ------------------------------------------------------------------
// Free functions
// ------------------------------------------------------------------

/// The ADR-2040 §V4 inclusion gate, shared by the ontology and plain paths:
/// frontmatter `public: true`, or a non-empty `owl-class`. Absence of both
/// means private — the working-graph gate excludes it.
///
/// Delegates to `visionclaw_domain::vault`, the single parsing entry point.
/// Only frontmatter counts; a `public:: true` line is body text.
fn page_is_kg_included(content: &str) -> bool {
    visionclaw_domain::vault::parse(content).is_kg_included()
}

/// Node-metadata key carrying the page's declared OKF ontology type.
///
/// Present ⇔ the page is part of the governed ontology bundle. Absence is what
/// keeps a `working/` page — an `Episode`, a `Note`, a `Draft Concept` — out of
/// the classes and axioms handed to Whelk, however public it is and whatever
/// IRI its quads mint.
pub(crate) const ONTOLOGY_TYPE_KEY: &str = "ontology_type";

/// `true` for a page under the `knowledge/` vault root.
fn is_knowledge_path(repo_path: &str) -> bool {
    repo_path
        .trim_start_matches("./")
        .trim_start_matches('/')
        .starts_with(ONTOLOGY_VAULT_PREFIX)
}

/// What one sync run resolves every page against. Bundled because each
/// per-file function needs all three, and passing them separately pushed
/// those functions past a readable arity.
#[derive(Clone, Copy)]
struct SyncScope<'a> {
    /// The §V1 identity index and the configured base paths.
    vault: visionclaw_domain::vault::VaultContext<'a>,
    /// Node ids that have a `knowledge/` page behind them, from the full
    /// listing.
    knowledge_ids: &'a std::collections::HashSet<u32>,
    /// The corpus vocabulary, when the source supplies one.
    vocabulary: Option<&'a Vocabulary>,
}

impl SyncScope<'_> {
    /// A page outside `knowledge/` whose node id a knowledge page also owns
    /// yields to it: the knowledge page is the governed record of that
    /// identity (PRD-sovereign-corpus Q16).
    fn yields_to_knowledge(&self, repo_path: &str, node_id: u32) -> bool {
        !is_knowledge_path(repo_path) && self.knowledge_ids.contains(&node_id)
    }
}

/// Copy a class page's frontmatter scalars onto its graph node: the display
/// and filter signals the client reads (`domain`, `quality_score`,
/// `maturity`), the definition (the body's leading paragraph, where
/// `vault migrate` placed it), and the page's source identity.
fn enrich_node_from_frontmatter(
    node: &mut visionclaw_domain::models::node::Node,
    parsed: &ParsedPage,
    identity: &str,
    content: &str,
) {
    let fm = &parsed.page.frontmatter;
    node.metadata
        .insert("source_file".to_string(), format!("{identity}.md"));
    node.metadata
        .insert("file_size".to_string(), content.len().to_string());
    node.file_size = content.len() as u64;
    node.metadata.insert(
        "owl_type".to_string(),
        if parsed.entity.kind == EntityKind::OntologyIndividual {
            "Individual"
        } else {
            "Class"
        }
        .to_string(),
    );
    if let Some(domain) = fm.text("domain").filter(|d| !d.trim().is_empty()) {
        node.group = Some(domain.clone());
        node.metadata
            .insert("source_domain".to_string(), domain.clone());
        node.metadata.insert("domain".to_string(), domain);
    }
    if let Some(maturity) = fm.text("maturity").filter(|m| !m.trim().is_empty()) {
        node.metadata.insert("maturity".to_string(), maturity);
    }
    if let Some(quality) = fm.number("quality").filter(|q| q.is_finite()) {
        let quality = quality.clamp(0.0, 1.0) as f32;
        node.metadata
            .insert("quality_score".to_string(), quality.to_string());
        node.size = Some(0.5 + quality * 1.5); // range 0.5-2.0
        node.weight = Some(quality);
    }
    let definition = parsed.page.leading_paragraph();
    if !definition.is_empty() {
        node.metadata.insert("definition".to_string(), definition);
    }
}

/// Map a fully-expanded predicate IRI to a canonical edge-type label.
/// Returns `""` for predicates that should not create graph edges.
/// The label is looked up in `SEMANTIC_TYPE_REGISTRY` for force config;
/// unknown IRIs auto-register with defaults via `get_or_register_id`.
fn predicate_to_edge_type(iri: &str) -> &'static str {
    match iri {
        RDFS_SUBCLASS_OF => "hierarchical",
        IRI_REQUIRES | IRI_ENABLES | IRI_DEPENDS_ON => "dependency",
        IRI_HAS_PART | IRI_IS_PART_OF => "structural",
        IRI_RELATES_TO => "associative",
        IRI_BRIDGES_TO | IRI_BRIDGES_FROM => "bridge",
        IRI_IMPLEMENTS => "implements",
        IRI_ENHANCES | IRI_OPTIMIZES => "enhancement",
        IRI_SECURES | IRI_VALIDATES => "security",
        OWL_EQUIVALENT_CLASS | OWL_SAME_AS => "hierarchical",
        OWL_DISJOINT_WITH => "bridge",
        OWL_INVERSE_OF => "associative",
        RDFS_DOMAIN | RDFS_RANGE => "structural",
        RDFS_SUB_PROPERTY_OF => "hierarchical",
        PROV_WAS_DERIVED_FROM | PROV_WAS_ATTRIBUTED_TO | PROV_WAS_GENERATED_BY => "provenance",
        IRI_ACHIEVES_OBJECTIVE => "goal",
        IRI_TRACKED_ON => "tracking",
        IRI_SIMILAR_TO | IRI_SIMULATED_IN => "similarity",
        IRI_WIKILINK => "explicit_link",
        IRI_USES | IRI_SUPPORTS | IRI_UTILISES => "utilisation",
        IRI_ENABLED_BY => "dependency",
        IRI_CONTRASTS_WITH => "bridge",
        IRI_STANDARDIZED_BY => "standardisation",
        IRI_APPLIES_TO | IRI_RELATED_TO => "associative",
        IRI_PART_OF => "structural",
        IRI_INSTANCE_OF => "hierarchical",
        IRI_NGM_SAME_AS => "hierarchical",
        IRI_DEFINED_IN => "structural",
        RDF_TYPE => "",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// WS-0 — MetadataStore population: deterministic `source_domain` derivation
// (ADR-100 D5). The "empty-MetadataStore bug" is that the upstream rarely
// emits `vc:sourceDomain`, so ~100% of nodes had a NULL domain and the live
// 6-bucket repulsion table received nothing. We derive a domain for EVERY
// node from the only signals always present at ingest — the file path and the
// page label/IRI — so coverage reaches ≥95% without inventing data.
// ---------------------------------------------------------------------------

/// The canonical six NarrativeGoldmine domains, as `(slug, keyword markers)`.
/// Markers are matched case-insensitively against the file path and label.
/// First match wins; order is deliberate (most specific first).
const DOMAIN_TABLE: &[(&str, &[&str])] = &[
    (
        "spatial-computing",
        &[
            "spatial", "/xr", "ar-", "vr-", "webxr", "babylon", "render", "hologram", "godot",
        ],
    ),
    (
        "artificial-intelligence",
        &[
            "/ai",
            "ai-",
            "agent",
            "llm",
            "ml",
            "neural",
            "transformer",
            "rag",
            "embedding",
            "reason",
        ],
    ),
    (
        "blockchain",
        &[
            "blockchain",
            "nostr",
            "did",
            "crypto",
            "ledger",
            "web3",
            "chain",
            "wallet",
        ],
    ),
    (
        "robotics",
        &["robot", "actuator", "sensor", "drone", "kinematic", "motor"],
    ),
    (
        "distributed-collaboration",
        &[
            "collab",
            "federation",
            "mesh",
            "p2p",
            "sync",
            "forum",
            "social",
            "swarm",
        ],
    ),
    // Infrastructure is the catch-all default, placed last.
    (
        "infrastructure",
        &[
            "infra", "deploy", "docker", "server", "network", "storage", "pipeline", "build",
        ],
    ),
];

/// Deterministically derive a node's `source_domain` from its file path and
/// label. Always returns a non-empty domain slug (defaults to
/// `infrastructure`), so the MetadataStore is never NULL. Deterministic:
/// identical (path,label) inputs always yield the same domain.
pub fn derive_source_domain(file_path: &str, label: &str) -> &'static str {
    let haystack = format!("{} {}", file_path.to_lowercase(), label.to_lowercase());
    for (slug, markers) in DOMAIN_TABLE {
        if markers.iter().any(|m| haystack.contains(m)) {
            return slug;
        }
    }
    "infrastructure"
}

/// Stamp `source_domain` onto a node if (and only if) it is not already set
/// from an authoritative `vc:sourceDomain` quad. Sets both `node.group` (the
/// field the live 6-bucket GPU repulsion table reads) and a `source_domain`
/// metadata entry (the MetadataStore key). Returns `true` if a value was
/// applied here (used for coverage accounting).
fn ensure_source_domain(node: &mut visionclaw_domain::models::node::Node, file_path: &str) -> bool {
    // Already authoritatively domained (quad-sourced) — leave it.
    if node
        .group
        .as_deref()
        .map(|g| !g.is_empty())
        .unwrap_or(false)
        && node.metadata.contains_key("source_domain")
    {
        return false;
    }
    let domain = derive_source_domain(file_path, &node.label);
    node.group = Some(domain.to_string());
    node.metadata
        .insert("source_domain".to_string(), domain.to_string());
    // Keep the legacy `domain` key in sync for the existing UI color-by path.
    node.metadata
        .entry("domain".to_string())
        .or_insert_with(|| domain.to_string());
    true
}

#[cfg(test)]
mod adr_2071_inferred_edge_tests {
    //! ADR-2071 acceptance evidence — the post-sync Whelk path now shares the
    //! `inferred_edge_materialiser` selection rules with `OntologyPipelineService`.
    //!
    //! [`legacy_select_inferred_edges`] is a verbatim reference copy of the
    //! hand-rolled loop this change deleted from `run_post_sync_reasoning`. It
    //! exists ONLY here, so the behavioural delta the ADR promises is asserted
    //! rather than argued: same fixtures through both, differing edge sets.

    use super::*;
    use crate::services::inferred_edge_materialiser::edge_is_inferred;
    use std::collections::{HashMap, HashSet};

    fn axiom(subject: &str, object: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: subject.to_string(),
            object: object.to_string(),
            annotations: HashMap::new(),
        }
    }

    /// Identity resolver over a fixed IRI→node-id table.
    fn table_resolver(pairs: &[(&'static str, u32)]) -> impl Fn(&str) -> Option<u32> {
        let map: HashMap<String, u32> = pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        move |iri: &str| map.get(iri).copied()
    }

    /// REFERENCE COPY of the superseded hand-rolled selection (deleted from
    /// production by ADR-2071). No transitive reduction, no asserted-pair
    /// suppression, no per-child cap, `inferred` edge-type, no `inferred`
    /// metadata key. Kept only to pin the delta.
    fn legacy_select_inferred_edges(
        axioms: &[OwlAxiom],
        resolve: &dyn Fn(&str) -> Option<u32>,
    ) -> Vec<Edge> {
        let mut inferred_edges = Vec::new();
        for axiom in axioms {
            if axiom.axiom_type == AxiomType::SubClassOf
                && !axiom.subject.contains("owl#Nothing")
                && !axiom.object.contains("owl#Thing")
                && axiom.subject != axiom.object
            {
                if let (Some(src_id), Some(tgt_id)) =
                    (resolve(&axiom.subject), resolve(&axiom.object))
                {
                    let mut edge_meta = HashMap::new();
                    edge_meta.insert("source_iri".to_string(), axiom.subject.clone());
                    edge_meta.insert("target_iri".to_string(), axiom.object.clone());
                    edge_meta.insert("axiom_type".to_string(), "SubClassOf".to_string());
                    inferred_edges.push(Edge {
                        id: format!("inferred_{}_{}", src_id, tgt_id),
                        source: src_id,
                        target: tgt_id,
                        weight: 0.4,
                        edge_type: Some("inferred".to_string()),
                        owl_property_iri: None,
                        metadata: Some(edge_meta),
                    });
                }
            }
        }
        inferred_edges
    }

    fn pairs_of(edges: &[Edge]) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = edges.iter().map(|e| (e.source, e.target)).collect();
        v.sort_unstable();
        v
    }

    /// A three-level chain A ⊑ B ⊑ C as Whelk emits it: the TRANSITIVE closure,
    /// so A→C is present in the axiom set. Node ids: A=1, B=2, C=3.
    fn chain_fixture() -> Vec<OwlAxiom> {
        vec![
            axiom("urn:c:A", "urn:c:B"),
            axiom("urn:c:A", "urn:c:C"), // long-range grandparent entailment
            axiom("urn:c:B", "urn:c:C"),
        ]
    }

    const CHAIN_NODES: &[(&str, u32)] = &[("urn:c:A", 1), ("urn:c:B", 2), ("urn:c:C", 3)];

    #[test]
    fn three_level_hierarchy_drops_the_long_range_grandparent_edge() {
        // ADR-2071 acceptance test 1: A→B and B→C survive; A→C does not.
        let resolve = table_resolver(CHAIN_NODES);
        let sel = GitHubSyncService::select_inferred_edges_for_sync(
            &chain_fixture(),
            &resolve,
            &HashSet::new(),
        );
        assert_eq!(
            pairs_of(&sel.edges),
            vec![(1, 2), (2, 3)],
            "immediate parents only — no A→C long-range edge"
        );
        assert_eq!(sel.considered_axioms, 3, "all three axioms are non-vacuous");
        assert_eq!(
            sel.immediate_pairs, 2,
            "transitive reduction keeps two pairs"
        );
        assert_eq!(sel.unresolved_endpoints, 0);
    }

    #[test]
    fn legacy_loop_emitted_the_long_range_edge_the_shared_path_suppresses() {
        // The recorded delta: the superseded loop emits 3 edges for the same
        // fixture, the shared path 2. This is the ADR's "edge counts will drop".
        let resolve = table_resolver(CHAIN_NODES);
        let legacy = legacy_select_inferred_edges(&chain_fixture(), &resolve);
        let shared = GitHubSyncService::select_inferred_edges_for_sync(
            &chain_fixture(),
            &resolve,
            &HashSet::new(),
        );
        assert_eq!(pairs_of(&legacy), vec![(1, 2), (1, 3), (2, 3)]);
        assert_eq!(pairs_of(&shared.edges), vec![(1, 2), (2, 3)]);

        // Every dropped pair is a transitive ancestor of a retained one
        // (acceptance criterion 2): (1,3) is reachable 1→2→3.
        let retained: HashSet<(u32, u32)> = pairs_of(&shared.edges).into_iter().collect();
        for dropped in pairs_of(&legacy)
            .into_iter()
            .filter(|p| !retained.contains(p))
        {
            assert!(
                retained.iter().any(|&(c, m)| c == dropped.0
                    && retained.iter().any(|&(c2, p2)| c2 == m && p2 == dropped.1)),
                "dropped {:?} is not a transitive ancestor of a retained edge",
                dropped
            );
        }
    }

    #[test]
    fn asserted_pairs_are_suppressed_on_the_sync_path() {
        // The legacy loop had no asserted diff and would duplicate the asserted
        // 1—2 hierarchy edge; the shared path suppresses it in both directions.
        let resolve = table_resolver(CHAIN_NODES);
        let asserted: HashSet<(u32, u32)> = [(2, 1), (1, 2)].into_iter().collect();
        let legacy = legacy_select_inferred_edges(&chain_fixture(), &resolve);
        assert!(pairs_of(&legacy).contains(&(1, 2)), "legacy duplicated it");

        let shared = GitHubSyncService::select_inferred_edges_for_sync(
            &chain_fixture(),
            &resolve,
            &asserted,
        );
        assert_eq!(
            pairs_of(&shared.edges),
            vec![(2, 3)],
            "asserted 1—2 suppressed either direction"
        );
    }

    #[test]
    fn per_child_cap_applies_to_the_sync_path() {
        // A child with more immediate inferred parents than the cap. Ten sibling
        // parents, none an ancestor of another, so the reduction keeps all ten and
        // only the cap can bound them.
        let mut axioms = Vec::new();
        let mut table: Vec<(&'static str, u32)> = vec![("urn:c:kid", 1)];
        const PARENTS: &[&str] = &[
            "urn:c:p0", "urn:c:p1", "urn:c:p2", "urn:c:p3", "urn:c:p4", "urn:c:p5", "urn:c:p6",
            "urn:c:p7", "urn:c:p8", "urn:c:p9",
        ];
        for (i, p) in PARENTS.iter().enumerate() {
            axioms.push(axiom("urn:c:kid", p));
            table.push((p, 100 + i as u32));
        }
        let resolve = table_resolver(&table);

        let legacy = legacy_select_inferred_edges(&axioms, &resolve);
        assert_eq!(legacy.len(), 10, "legacy path had no cap");

        let shared =
            GitHubSyncService::select_inferred_edges_for_sync(&axioms, &resolve, &HashSet::new());
        assert_eq!(
            shared.edges.len(),
            crate::services::inferred_edge_materialiser::DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD,
            "capped at 8 inferred parents per child"
        );
    }

    #[test]
    fn emitted_edges_carry_the_inferred_flag_the_client_reads() {
        // The behavioural bug ADR-2071 fixes: the legacy edges set edge_type
        // "inferred" but NOT metadata["inferred"], so `edge_is_inferred` — the
        // predicate the broadcast path and the XR shader use — returned false and
        // sync-produced edges never rendered on the inferred channel.
        let resolve = table_resolver(CHAIN_NODES);
        let legacy = legacy_select_inferred_edges(&chain_fixture(), &resolve);
        assert!(
            legacy.iter().all(|e| !edge_is_inferred(e)),
            "legacy edges were invisible to edge_is_inferred"
        );

        let shared = GitHubSyncService::select_inferred_edges_for_sync(
            &chain_fixture(),
            &resolve,
            &HashSet::new(),
        );
        assert!(
            shared.edges.iter().all(edge_is_inferred),
            "every shared-path edge classifies as inferred"
        );
        for e in &shared.edges {
            assert_eq!(e.edge_type.as_deref(), Some("hierarchical"));
            let meta = e.metadata.as_ref().expect("provenance metadata retained");
            assert_eq!(
                meta.get("axiom_type").map(String::as_str),
                Some("SubClassOf")
            );
            assert!(meta.contains_key("source_iri") && meta.contains_key("target_iri"));
        }
    }

    #[test]
    fn vacuous_axioms_and_unresolved_endpoints_are_accounted_for() {
        let axioms = vec![
            axiom("urn:c:A", "http://www.w3.org/2002/07/owl#Thing"), // vacuous: top parent
            axiom("http://www.w3.org/2002/07/owl#Nothing", "urn:c:A"), // vacuous: bottom child
            axiom("urn:c:A", "urn:c:A"),                             // vacuous: self
            axiom("urn:c:A", "urn:c:B"),                             // real
            axiom("urn:c:A", "urn:c:missing"),                       // unresolvable parent
        ];
        let resolve = table_resolver(CHAIN_NODES);
        let sel =
            GitHubSyncService::select_inferred_edges_for_sync(&axioms, &resolve, &HashSet::new());
        assert_eq!(
            sel.considered_axioms, 2,
            "three vacuous axioms filtered out"
        );
        assert_eq!(
            sel.immediate_pairs, 2,
            "A→B and A→missing survive reduction"
        );
        assert_eq!(sel.unresolved_endpoints, 1, "urn:c:missing counted");
        assert_eq!(pairs_of(&sel.edges), vec![(1, 2)]);
    }

    #[test]
    fn selection_is_deterministic_regardless_of_axiom_order() {
        let resolve = table_resolver(CHAIN_NODES);
        let mut reversed = chain_fixture();
        reversed.reverse();
        let a = GitHubSyncService::select_inferred_edges_for_sync(
            &chain_fixture(),
            &resolve,
            &HashSet::new(),
        );
        let b =
            GitHubSyncService::select_inferred_edges_for_sync(&reversed, &resolve, &HashSet::new());
        assert_eq!(pairs_of(&a.edges), pairs_of(&b.edges));
    }

    /// ADR-2071 acceptance evidence — REAL reasoner output, not a hand-written
    /// axiom list. Loads a corpus-shaped class hierarchy (a 6-level chain, a
    /// diamond and a multi-parent leaf) into the production
    /// [`WhelkInferenceEngine`], then runs the entailed axioms through BOTH the
    /// superseded hand-rolled loop and the shared module, reporting the edge-count
    /// delta the ADR requires. Stands in for the live Oxigraph shadow sync, which
    /// is not reachable from the build container.
    #[tokio::test]
    async fn shadow_comparison_over_real_whelk_output() {
        use visionclaw_domain::ports::ontology_repository::OwlClass;

        // A ⊑ B ⊑ C ⊑ D ⊑ E ⊑ F (6-level chain), plus a diamond
        // X ⊑ {Y,Z} ⊑ W, plus a leaf with three unrelated parents.
        let chain = ["A", "B", "C", "D", "E", "F"];
        let iri = |n: &str| format!("http://example.org/adr2071#{}", n);

        let mut classes: Vec<OwlClass> = Vec::new();
        let mut axioms: Vec<OwlAxiom> = Vec::new();
        let sub = |child: &str, parent: &str, axioms: &mut Vec<OwlAxiom>| {
            axioms.push(OwlAxiom {
                id: None,
                axiom_type: AxiomType::SubClassOf,
                subject: iri(child),
                object: iri(parent),
                annotations: std::collections::HashMap::new(),
            });
        };
        for name in chain
            .iter()
            .chain(["W", "X", "Y", "Z", "L", "P1", "P2", "P3"].iter())
        {
            classes.push(OwlClass {
                iri: iri(name),
                label: Some((*name).to_string()),
                ..Default::default()
            });
        }
        for w in chain.windows(2) {
            sub(w[0], w[1], &mut axioms);
        }
        sub("X", "Y", &mut axioms);
        sub("X", "Z", &mut axioms);
        sub("Y", "W", &mut axioms);
        sub("Z", "W", &mut axioms);
        for p in ["P1", "P2", "P3"] {
            sub("L", p, &mut axioms);
        }
        let asserted_axiom_count = axioms.len();

        let mut engine = WhelkInferenceEngine::new();
        engine
            .load_ontology(classes, axioms)
            .await
            .expect("whelk load_ontology");
        let results = engine.infer().await.expect("whelk infer");

        // Node ids mirror the IRI order; the resolver is exact-IRI.
        let table: std::collections::HashMap<String, u32> = chain
            .iter()
            .chain(["W", "X", "Y", "Z", "L", "P1", "P2", "P3"].iter())
            .enumerate()
            .map(|(i, n)| (iri(n), i as u32 + 1))
            .collect();
        let resolve = move |i: &str| table.get(i).copied();

        let legacy = legacy_select_inferred_edges(&results.inferred_axioms, &resolve);
        let shared = GitHubSyncService::select_inferred_edges_for_sync(
            &results.inferred_axioms,
            &resolve,
            &HashSet::new(),
        );

        let legacy_pairs = pairs_of(&legacy);
        let shared_pairs = pairs_of(&shared.edges);
        // Printed with `--nocapture`; these are the counts recorded in the ADR.
        println!(
            "ADR-2071 shadow comparison: {} asserted axioms → {} Whelk entailments → \
             legacy {} edges, shared {} edges (delta {})",
            asserted_axiom_count,
            results.inferred_axioms.len(),
            legacy_pairs.len(),
            shared_pairs.len(),
            legacy_pairs.len() as i64 - shared_pairs.len() as i64
        );

        assert!(
            shared_pairs.len() <= legacy_pairs.len(),
            "the shared path never emits MORE edges than the legacy loop"
        );
        // Retained set is a subset of what the legacy loop emitted: this change
        // only ever removes edges, it never invents new ones.
        let legacy_set: HashSet<(u32, u32)> = legacy_pairs.iter().copied().collect();
        for p in &shared_pairs {
            assert!(
                legacy_set.contains(p),
                "retained {:?} is new — not allowed",
                p
            );
        }
        // Per-child cap holds over real reasoner output.
        let mut per_child: HashMap<u32, usize> = HashMap::new();
        for (c, _) in &shared_pairs {
            *per_child.entry(*c).or_insert(0) += 1;
        }
        assert!(
            per_child.values().all(|&n| n
                <= crate::services::inferred_edge_materialiser::DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD),
            "per-child cap holds on real reasoner output"
        );
        // Every retained edge classifies as inferred for the client channel.
        assert!(shared.edges.iter().all(edge_is_inferred));
    }
}

/// PRD-sovereign-corpus Q16: the governed ontology bundle is
/// `type: Class|Property|Individual` in `knowledge/` **only**, while the KG
/// graph ingest keeps both base paths.
#[cfg(test)]
mod sovereign_corpus_scope_tests {
    use super::*;

    /// The ontology type the ingest seam reports for a page, if any.
    fn declared_ontology_type(path: &str, content: &str) -> Option<String> {
        parse_page(content, path)
            .expect("fixture frontmatter parses")
            .map(|parsed| parsed.ontology_type)
    }

    #[test]
    fn only_knowledge_paths_are_knowledge() {
        assert!(is_knowledge_path("knowledge/pages/A.md"));
        assert!(is_knowledge_path("./knowledge/pages/A.md"));
        assert!(is_knowledge_path("/knowledge/pages/A.md"));
        assert!(!is_knowledge_path("working/pages/A.md"));
        assert!(!is_knowledge_path("working/pages/knowledge/A.md"));
    }

    #[test]
    fn a_working_twin_yields_to_its_knowledge_page_and_nothing_else_does() {
        let index = visionclaw_domain::vault::VaultIndex::from_identities(["Camera"]);
        let bases = vec!["knowledge/pages".to_string(), "working/pages".to_string()];
        let parser = KnowledgeGraphParser::new();
        let camera = parser.page_name_to_id("Camera");
        let knowledge_ids: std::collections::HashSet<u32> = [camera].into_iter().collect();
        let scope = SyncScope {
            vault: visionclaw_domain::vault::VaultContext::new(&index, &bases),
            knowledge_ids: &knowledge_ids,
            vocabulary: None,
        };
        assert!(scope.yields_to_knowledge("working/pages/Camera.md", camera));
        assert!(
            !scope.yields_to_knowledge("knowledge/pages/Camera.md", camera),
            "the knowledge page itself never yields"
        );
        assert!(
            !scope.yields_to_knowledge("working/pages/Lens.md", parser.page_name_to_id("Lens")),
            "a working page with no knowledge twin is authored in its own right"
        );
    }

    /// A public `working/` Episode — exactly the 188 podcast-evidence pages
    /// PRD-sovereign-corpus Q16 relocates out of `knowledge/`.
    fn working_episode_page() -> &'static str {
        "---\ntype: Episode\npublic: true\ntitle: Black Friday GPT\nsource: The Podcast\nepisode-date: 2026-08-01\ntier: '2'\n---\n\n# Black Friday GPT\n\nEvidence body.\n"
    }

    fn knowledge_class_page() -> &'static str {
        "---\ntype: Class\nresource: urn:ngm:class:camera\nstatus: stable\npublic: true\ntitle: Camera\n---\n\n# Camera\n"
    }

    #[test]
    fn a_public_working_episode_is_a_graph_node_but_never_an_ontology_class() {
        let episode = working_episode_page();

        // It IS admitted to the knowledge graph: `public: true` opens the §V4
        // gate, and the graph ingest keeps BOTH base paths.
        assert!(
            page_is_kg_included(episode),
            "a public working page is still a graph node"
        );

        // It is NOT part of the ontology bundle — it declares no ontology type,
        // so nothing stamps it and the assert-graph rebuild cannot see it.
        assert_eq!(
            declared_ontology_type(
                "working/pages/podcast-evidence/black-friday-gpt.md",
                episode
            ),
            None,
            "an Episode is not a Class, Property or Individual"
        );

        // …and the same page is still not ontology if it were misfiled under
        // knowledge/: the `type` is the declaration, not the directory.
        assert_eq!(
            declared_ontology_type("knowledge/pages/black-friday-gpt.md", episode),
            None
        );
    }

    #[test]
    fn only_a_knowledge_page_with_an_ontology_type_joins_the_bundle() {
        let class_page = knowledge_class_page();
        assert_eq!(
            declared_ontology_type("knowledge/pages/Camera.md", class_page),
            Some("Class".to_string())
        );
        // Leading `./` and `/` are normalised — the corpus source emits both.
        assert_eq!(
            declared_ontology_type("./knowledge/pages/Camera.md", class_page),
            Some("Class".to_string())
        );

        // Property and Individual qualify too (vocabulary.yaml `types:`).
        for t in ["Property", "Individual"] {
            let page =
                format!("---\ntype: {t}\nresource: urn:ngm:class:x\nstatus: draft\n---\n\n# X\n");
            assert_eq!(
                declared_ontology_type("knowledge/pages/X.md", &page),
                Some(t.to_string())
            );
        }

        // The SAME class page under working/ does not: Q16 scopes the governed
        // bundle to knowledge/ only, so a working-vault draft cannot smuggle a
        // class into Whelk.
        assert_eq!(
            declared_ontology_type("working/pages/Camera.md", class_page),
            None
        );

        // A knowledge page with no `type` at all is graph-only.
        assert_eq!(
            declared_ontology_type(
                "knowledge/pages/Plain.md",
                "---\npublic: true\n---\n\n# Plain\n"
            ),
            None
        );
    }
}
