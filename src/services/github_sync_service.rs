// src/services/github_sync_service.rs
//! GitHub Sync Service
//!
//! Synchronizes markdown files from GitHub repository to Neo4j.
//! - Parses public:: true pages as knowledge graph nodes (KnowledgeGraphRepository - Neo4jAdapter)
//! - Extracts OntologyBlock sections as OWL data (Neo4jOntologyRepository)
//! - Enriches graph nodes with owl_class_iri metadata via OntologyEnrichmentService
//! - Triggers OntologyPipelineService for automatic reasoning and constraint generation
//! - Uses SHA1 filtering to process only changed files (unless FORCE_FULL_SYNC=1)
//! - Batch processing (50 files) to avoid memory issues with large repositories

use crate::adapters::neo4j_ontology_repository::Neo4jOntologyRepository;
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::ports::ontology_repository::OntologyRepository;
use crate::services::github::content_enhanced::EnhancedContentAPI;
use crate::services::github::types::GitHubFileBasicMetadata;
use crate::services::parsers::{KnowledgeGraphParser, OntologyParser};
use crate::services::ontology_enrichment_service::OntologyEnrichmentService;
use crate::services::ontology_reasoner::OntologyReasoner;
use crate::services::edge_classifier::EdgeClassifier;
use crate::services::ontology_pipeline_service::OntologyPipelineService;
use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use futures::stream::{FuturesUnordered, StreamExt};
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BATCH_SIZE: usize = 50; // Save to database every 50 files

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    KnowledgeGraph,
    Ontology,
    Skip,
}

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

pub struct GitHubSyncService {
    content_api: Arc<EnhancedContentAPI>,
    kg_parser: Arc<KnowledgeGraphParser>,
    onto_parser: Arc<OntologyParser>,
    kg_repo: Arc<dyn KnowledgeGraphRepository>,
    onto_repo: Arc<Neo4jOntologyRepository>,
    enrichment_service: Arc<OntologyEnrichmentService>,
    pipeline_service: Option<Arc<OntologyPipelineService>>,
}

impl GitHubSyncService {
    pub fn new(
        content_api: Arc<EnhancedContentAPI>,
        kg_repo: Arc<dyn KnowledgeGraphRepository>,
        onto_repo: Arc<Neo4jOntologyRepository>,
    ) -> Self {
        // Initialize ontology enrichment service
        let inference_engine = Arc::new(WhelkInferenceEngine::new());
        let reasoner = Arc::new(OntologyReasoner::new(
            inference_engine,
            onto_repo.clone() as Arc<dyn OntologyRepository>,
        ));
        let classifier = Arc::new(EdgeClassifier::new());
        let enrichment_service = Arc::new(OntologyEnrichmentService::new(
            reasoner,
            classifier,
        ));

        Self {
            content_api,
            kg_parser: Arc::new(KnowledgeGraphParser::new()),
            onto_parser: Arc::new(OntologyParser::new()),
            kg_repo,
            onto_repo,
            enrichment_service,
            pipeline_service: None,
        }
    }

    /// Set the ontology pipeline service for automatic reasoning
    pub fn set_pipeline_service(&mut self, pipeline: Arc<OntologyPipelineService>) {
        info!("GitHubSyncService: Ontology pipeline service registered");
        self.pipeline_service = Some(pipeline);
    }

    /// Synchronize graphs from GitHub - processes in batches with progress logging
    pub async fn sync_graphs(&self) -> Result<SyncStatistics, String> {
        info!("🔄 Starting GitHub sync (batch size: {})", BATCH_SIZE);
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

        // Fetch files
        let files = match self.fetch_all_markdown_files().await {
            Ok(files) => {
                info!("📂 Found {} markdown files", files.len());
                files
            }
            Err(e) => {
                let error_msg = format!("Failed to fetch files: {}", e);
                error!("{}", error_msg);
                stats.errors.push(error_msg);
                stats.duration = start_time.elapsed();
                return Ok(stats);
            }
        };

        stats.total_files = files.len();

        // SHA1 filtering - only process changed files (unless FORCE_FULL_SYNC is set)
        let force_full_sync = std::env::var("FORCE_FULL_SYNC")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let files_to_process = if force_full_sync {
            info!("🔄 FORCE_FULL_SYNC enabled - processing ALL {} files (bypassing SHA1 filter)", files.len());
            files.clone()
        } else {
            match self.filter_changed_files(&files).await {
                Ok(filtered) => {
                    info!("📋 Processing {} changed files ({} unchanged)",
                        filtered.len(), files.len() - filtered.len());
                    stats.skipped_files = files.len() - filtered.len();
                    filtered
                }
                Err(e) => {
                    error!("SHA1 filter failed: {}", e);
                    files.clone() // Process all if filter fails
                }
            }
        };

        // Clone files_to_process for metadata update later
        let all_files_to_process = files_to_process.clone();

        // Process in batches
        for (batch_idx, batch) in files_to_process.chunks(BATCH_SIZE).enumerate() {
            let batch_start = Instant::now();
            info!("📦 Processing batch {}/{} ({} files)",
                batch_idx + 1,
                (files_to_process.len() + BATCH_SIZE - 1) / BATCH_SIZE,
                batch.len()
            );

            match self.process_batch(batch, &mut stats).await {
                Ok(_) => {
                    info!("✅ Batch {} completed in {:?}", batch_idx + 1, batch_start.elapsed());
                }
                Err(e) => {
                    error!("❌ Batch {} failed: {}", batch_idx + 1, e);
                    stats.errors.push(format!("Batch {}: {}", batch_idx + 1, e));
                }
            }
        }

        // Update metadata
        if let Err(e) = self.update_file_metadata(&all_files_to_process).await {
            warn!("Failed to update file_metadata: {}", e);
        }

        stats.duration = start_time.elapsed();
        info!("🎉 Sync complete: {} nodes, {} edges in {:?}",
            stats.total_nodes, stats.total_edges, stats.duration);

        Ok(stats)
    }

    /// Process a batch of files with parallel content fetching
    /// Uses FuturesUnordered to fetch file contents in parallel (the main I/O bottleneck)
    /// while processing/parsing sequentially to maintain state consistency.
    async fn process_batch(
        &self,
        files: &[GitHubFileBasicMetadata],
        stats: &mut SyncStatistics,
    ) -> Result<(), String> {
        let mut batch_nodes = std::collections::HashMap::new();
        let mut batch_edges = std::collections::HashMap::new();
        let mut public_pages = std::collections::HashSet::new();

        info!("🔍 [DEBUG] Starting batch with {} files (parallel fetch)", files.len());

        // Phase 1: Fetch all file contents in parallel
        const PARALLEL_FETCHES: usize = 8;

        // Helper to create fetch future with consistent type
        fn create_fetch_future(
            content_api: Arc<EnhancedContentAPI>,
            file: GitHubFileBasicMetadata,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = (GitHubFileBasicMetadata, Result<String, String>)> + Send>> {
            let download_url = file.download_url.clone();
            Box::pin(async move {
                let result = content_api.fetch_file_content(&download_url).await
                    .map_err(|e| format!("Failed to fetch content: {}", e));
                (file, result)
            })
        }

        let mut fetch_futures: FuturesUnordered<_> = FuturesUnordered::new();
        let mut fetched_contents: Vec<(GitHubFileBasicMetadata, Result<String, String>)> = Vec::with_capacity(files.len());
        let mut file_iter = files.iter().cloned().peekable();

        // Seed initial batch of parallel fetches
        while fetch_futures.len() < PARALLEL_FETCHES {
            if let Some(file) = file_iter.next() {
                fetch_futures.push(create_fetch_future(Arc::clone(&self.content_api), file));
            } else {
                break;
            }
        }

        // Collect all fetched contents
        while let Some((file, content_result)) = fetch_futures.next().await {
            fetched_contents.push((file, content_result));

            // Add next file to maintain parallelism
            if let Some(file) = file_iter.next() {
                fetch_futures.push(create_fetch_future(Arc::clone(&self.content_api), file));
            }
        }

        info!("🔍 [DEBUG] Fetched {} files, now processing", fetched_contents.len());

        // Phase 2: Process fetched contents sequentially (modifies shared state)
        for (idx, (file, content_result)) in fetched_contents.into_iter().enumerate() {
            if idx % 10 == 0 && idx > 0 {
                info!("  Progress: {}/{} files in batch (nodes so far: {}, edges: {})",
                    idx, files.len(), batch_nodes.len(), batch_edges.len());
            }

            match content_result {
                Ok(content) => {
                    match self.process_fetched_file(&file, &content, &mut batch_nodes, &mut batch_edges, &mut public_pages).await {
                        Ok(()) => {
                            stats.kg_files_processed += 1;
                            debug!("✓ Processed {}: {} nodes total, {} edges total",
                                file.name, batch_nodes.len(), batch_edges.len());
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

        info!("🔍 [DEBUG] After processing: {} nodes, {} edges, {} public_pages",
            batch_nodes.len(), batch_edges.len(), public_pages.len());

        // Don't filter nodes/edges - save everything to maintain graph connectivity
        // Edge cross-references between batches should be preserved
        // let nodes_before_filter = batch_nodes.len();
        // self.filter_linked_pages(&mut batch_nodes, &public_pages);
        // info!("🔍 [DEBUG] After filter_linked_pages: {} nodes (removed {})",
        //     batch_nodes.len(), nodes_before_filter - batch_nodes.len());

        // let edges_before_filter = batch_edges.len();
        // self.filter_orphan_edges(&mut batch_edges, &batch_nodes);
        // info!("🔍 [DEBUG] After filter_orphan_edges: {} edges (removed {})",
        //     batch_edges.len(), edges_before_filter - batch_edges.len());

        // Save batch to database
        if !batch_nodes.is_empty() {
            let node_vec: Vec<_> = batch_nodes.into_values().collect();
            let edge_vec: Vec<_> = batch_edges.into_values().collect();

            stats.total_nodes += node_vec.len();
            stats.total_edges += edge_vec.len();

            info!("💾 Saving batch: {} nodes, {} edges", node_vec.len(), edge_vec.len());

            let mut graph = crate::models::graph::GraphData::new();
            graph.nodes = node_vec;
            graph.edges = edge_vec;

            info!("🔍 [DEBUG] Calling save_graph() with {} nodes, {} edges",
                graph.nodes.len(), graph.edges.len());

            self.kg_repo.save_graph(&graph).await.map_err(|e| {
                error!("❌ save_graph() failed: {}", e);
                format!("Failed to save batch: {}", e)
            })?;

            info!("✅ [DEBUG] save_graph() completed successfully");
        } else {
            warn!("⚠️ [DEBUG] Batch is EMPTY after filtering - nothing to save!");
        }

        Ok(())
    }

    /// Process a file with pre-fetched content (used by parallel batch processing)
    async fn process_fetched_file(
        &self,
        file: &GitHubFileBasicMetadata,
        content: &str,
        nodes: &mut std::collections::HashMap<u32, crate::models::node::Node>,
        edges: &mut std::collections::HashMap<String, crate::models::edge::Edge>,
        public_pages: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        debug!("🔍 Processing file: {}", file.name);
        debug!("🔍 Content size: {} bytes for {}", content.len(), file.name);

        // Detect file type
        let file_type = self.detect_file_type(&content);
        debug!("🔍 Detected file type: {:?} for {}", file_type, file.name);

        let page_name = file.name.trim_end_matches(".md");

        match file_type {
            FileType::KnowledgeGraph => {
                // Process public:: true files as knowledge graph nodes
                debug!("🔍 Parsing knowledge graph from {}", file.name);
                let mut parsed = self.kg_parser.parse(&content, &file.name)
                    .map_err(|e| format!("Parse error: {}", e))?;

                info!("📊 Parsed {}: {} nodes, {} edges",
                    file.name, parsed.nodes.len(), parsed.edges.len());

                // ✅ ENRICH WITH ONTOLOGY DATA
                debug!("🦉 Enriching graph with ontology data for {}", file.name);
                match self.enrichment_service.enrich_graph(&mut parsed, &file.path, &content).await {
                    Ok((nodes_enriched, edges_enriched)) => {
                        info!("✅ Enriched {}: {} nodes with owl_class_iri, {} edges with owl_property_iri",
                            file.name, nodes_enriched, edges_enriched);
                    }
                    Err(e) => {
                        warn!("⚠️  Failed to enrich {}: {} (continuing with unenriched data)", file.name, e);
                    }
                }

                // Add to public pages
                public_pages.insert(page_name.to_string());
                debug!("✓ Added '{}' to public_pages (total: {})", page_name, public_pages.len());

                // Add nodes from KG parser
                let nodes_before = nodes.len();
                for node in parsed.nodes {
                    debug!("  → Node {}: {} (type: {:?})",
                        node.id, node.label,
                        node.metadata.get("type"));
                    nodes.insert(node.id, node);
                }
                let kg_nodes_added = nodes.len() - nodes_before;
                info!("✓ Added {} KG nodes from {} (total now: {})",
                    kg_nodes_added, file.name, nodes.len());

                // Add edges from KG parser
                let edges_before = edges.len();
                for edge in parsed.edges {
                    edges.insert(edge.id.clone(), edge);
                }
                if edges.len() > edges_before {
                    debug!("✓ Added {} edges from {}", edges.len() - edges_before, file.name);
                }

                // Also check for and parse ontology blocks in this file
                if content.contains("### OntologyBlock") {
                    debug!("🦉 Detected OntologyBlock in {}, extracting ontology data", file.name);
                    match self.onto_parser.parse(&content, &file.name) {
                        Ok(onto_data) => {
                            info!("🦉 Extracted from {}: {} classes, {} properties, {} axioms",
                                file.name,
                                onto_data.classes.len(),
                                onto_data.properties.len(),
                                onto_data.axioms.len());

                            // Save ontology data immediately
                            if let Err(e) = self.save_ontology_data(onto_data).await {
                                error!("Failed to save ontology data from {}: {}", file.name, e);
                            } else {
                                debug!("✓ Saved ontology data from {}", file.name);
                            }
                        }
                        Err(e) => {
                            debug!("Failed to parse ontology block in {}: {}", file.name, e);
                        }
                    }
                }

                Ok(())
            }
            FileType::Ontology => {
                // Process files with ontology blocks
                debug!("🦉 Processing ontology file {}", file.name);
                match self.onto_parser.parse(&content, &file.name) {
                    Ok(onto_data) => {
                        info!("🦉 Extracted from {}: {} classes, {} properties, {} axioms",
                            file.name,
                            onto_data.classes.len(),
                            onto_data.properties.len(),
                            onto_data.axioms.len());

                        // Save ontology data immediately
                        if let Err(e) = self.save_ontology_data(onto_data).await {
                            error!("Failed to save ontology data from {}: {}", file.name, e);
                        } else {
                            debug!("✓ Saved ontology data from {}", file.name);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to parse ontology file {}: {}", file.name, e);
                    }
                }
                Ok(())
            }
            FileType::Skip => {
                // Skip regular notes without public:: true or ontology blocks
                debug!("⏭️  Skipped regular note: {} (no public:: true or ontology block)", file.name);
                Ok(())
            }
        }
    }

    /// Filter linked pages
    fn filter_linked_pages(
        &self,
        nodes: &mut std::collections::HashMap<u32, crate::models::node::Node>,
        public_pages: &std::collections::HashSet<String>,
    ) {
        let before = nodes.len();
        nodes.retain(|_, node| {
            match node.metadata.get("type").map(|s| s.as_str()) {
                Some("page") => true,
                Some("linked_page") => public_pages.contains(&node.metadata_id),
                _ => true,
            }
        });
        let filtered = before - nodes.len();
        if filtered > 0 {
            info!("🔍 Filtered {} linked_page nodes", filtered);
        }
    }

    /// Filter orphan edges
    fn filter_orphan_edges(
        &self,
        edges: &mut std::collections::HashMap<String, crate::models::edge::Edge>,
        nodes: &std::collections::HashMap<u32, crate::models::node::Node>,
    ) {
        let before = edges.len();
        edges.retain(|_, edge| {
            nodes.contains_key(&edge.source) && nodes.contains_key(&edge.target)
        });
        let filtered = before - edges.len();
        if filtered > 0 {
            info!("🔍 Filtered {} orphan edges", filtered);
        }
    }

    /// SHA1-based filtering
    async fn filter_changed_files(
        &self,
        files: &[GitHubFileBasicMetadata],
    ) -> Result<Vec<GitHubFileBasicMetadata>, String> {
        let existing = self.get_existing_file_metadata().await?;

        Ok(files
            .iter()
            .filter(|file| {
                match existing.get(&file.name) {
                    Some(existing_sha) if existing_sha == &file.sha => false,
                    _ => true,
                }
            })
            .cloned()
            .collect())
    }

    // ... (rest of helper methods unchanged)
    async fn fetch_all_markdown_files(&self) -> Result<Vec<GitHubFileBasicMetadata>, String> {
        self.content_api
            .list_markdown_files("")
            .await
            .map_err(|e| format!("GitHub API error: {}", e))
    }

    async fn get_existing_file_metadata(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        use neo4rs::query;

        info!("[GitHubSync][SHA1] Querying Neo4j for existing file SHA1 hashes...");

        // Query all FileMetadata nodes for filename -> sha1 mapping
        let query_str = "MATCH (f:FileMetadata) RETURN f.filename AS filename, f.sha1 AS sha1";

        let graph = self.onto_repo.graph();
        let mut result = graph.execute(query(query_str)).await
            .map_err(|e| format!("Failed to query file metadata: {}", e))?;

        let mut metadata = std::collections::HashMap::new();
        while let Some(row) = result.next().await.map_err(|e| format!("Row iteration error: {}", e))? {
            if let (Ok(filename), Ok(sha1)) = (
                row.get::<String>("filename"),
                row.get::<String>("sha1")
            ) {
                metadata.insert(filename, sha1);
            }
        }

        info!("[GitHubSync][SHA1] Found {} existing file SHA1 hashes in Neo4j", metadata.len());
        Ok(metadata)
    }

    async fn update_file_metadata(
        &self,
        files: &[GitHubFileBasicMetadata],
    ) -> Result<(), String> {
        use neo4rs::query;

        if files.is_empty() {
            return Ok(());
        }

        info!("[GitHubSync][SHA1] Updating {} file SHA1 hashes in Neo4j...", files.len());

        let graph = self.onto_repo.graph();

        // Ensure FileMetadata index exists (idempotent)
        let index_query = "CREATE INDEX file_metadata_filename IF NOT EXISTS FOR (f:FileMetadata) ON (f.filename)";
        if let Err(e) = graph.run(query(index_query)).await {
            warn!("[GitHubSync] Failed to create FileMetadata index (may already exist): {}", e);
        }

        // MERGE each file metadata (update if exists, create if not)
        for file in files {
            let merge_query = query(
                "MERGE (f:FileMetadata {filename: $filename})
                 SET f.sha1 = $sha1, f.last_synced = datetime()"
            )
            .param("filename", file.name.clone())
            .param("sha1", file.sha.clone());

            if let Err(e) = graph.run(merge_query).await {
                warn!("[GitHubSync] Failed to update metadata for {}: {}", file.name, e);
            }
        }

        info!("[GitHubSync][SHA1] Updated {} file SHA1 hashes", files.len());
        Ok(())
    }

    fn detect_file_type(&self, content: &str) -> FileType {
        let content = content.trim_start_matches('\u{feff}');
        let lines: Vec<&str> = content.lines().take(20).collect();

        // Check for ontology blocks first (highest priority)
        if content.contains("### OntologyBlock") {
            return FileType::Ontology;
        }

        // Check for explicit public:: true (knowledge graph files)
        for line in lines.iter() {
            if line.trim() == "public:: true" {
                return FileType::KnowledgeGraph;
            }
        }

        // Default: skip regular notes without public:: true or ontology blocks
        FileType::Skip
    }

    /// Save ontology data to Neo4j and trigger reasoning pipeline
    /// This method:
    /// 1. Saves OWL classes, properties, and axioms to Neo4jOntologyRepository
    /// 2. Triggers OntologyPipelineService for automatic reasoning
    /// 3. Pipeline generates semantic constraints and uploads to GPU
    /// The reasoning pipeline runs asynchronously to avoid blocking sync.
    async fn save_ontology_data(&self, onto_data: crate::services::parsers::ontology_parser::OntologyData) -> Result<(), String> {
        use crate::ports::ontology_repository::OntologyRepository;

        // Save all ontology data to Neo4j graph database
        self.onto_repo.save_ontology(&onto_data.classes, &onto_data.properties, &onto_data.axioms).await
            .map_err(|e| format!("Failed to save ontology data: {}", e))?;

        // Log class hierarchy
        for (subclass_iri, superclass_iri) in onto_data.class_hierarchy {
            debug!("Class hierarchy: {} -> {}", subclass_iri, superclass_iri);
        }

        // 🔥 TRIGGER REASONING PIPELINE if configured
        // This spawns an async task to run CustomReasoner inference, generate
        // semantic constraints, and upload to GPU without blocking GitHub sync
        if let Some(pipeline) = &self.pipeline_service {
            info!("🔄 Triggering ontology reasoning pipeline after ontology save");

            // Convert parsed ontology data to Ontology struct for reasoning
            let mut ontology = crate::reasoning::custom_reasoner::Ontology::default();

            // Add classes
            for class in &onto_data.classes {
                use crate::reasoning::custom_reasoner::OWLClass;
                ontology.classes.insert(
                    class.iri.clone(),
                    OWLClass {
                        iri: class.iri.clone(),
                        label: class.label.clone(),
                        parent_class_iri: None, // Will be populated from axioms
                    },
                );
            }

            // Add subclass relationships
            use crate::ports::ontology_repository::AxiomType;
            for axiom in &onto_data.axioms {
                if matches!(axiom.axiom_type, AxiomType::SubClassOf) {
                    ontology.subclass_of
                        .entry(axiom.subject.clone())
                        .or_insert_with(std::collections::HashSet::new)
                        .insert(axiom.object.clone());
                }
            }

            // Trigger the pipeline asynchronously
            let ontology_id = 1; // Using default ontology ID - multi-ontology support deferred
            let pipeline_clone = Arc::clone(pipeline);

            tokio::spawn(async move {
                match pipeline_clone.on_ontology_modified(ontology_id, ontology).await {
                    Ok(stats) => {
                        info!(
                            "✅ Ontology pipeline complete: {} axioms inferred, {} constraints generated, GPU upload: {}",
                            stats.inferred_axioms_count,
                            stats.constraints_generated,
                            stats.gpu_upload_success
                        );
                    }
                    Err(e) => {
                        error!("❌ Ontology pipeline failed: {}", e);
                    }
                }
            });
        }

        Ok(())
    }
}
