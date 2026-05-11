use super::github::{ContentAPI, GitHubClient, GitHubConfig};
use crate::config::AppFullSettings;
use crate::models::graph::GraphData;
use crate::models::node::Node as AppNode; // Use an alias to avoid confusion
use crate::models::edge::Edge as AppEdge;
use crate::models::metadata::{Metadata, MetadataOps, MetadataStore};
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::time;
use actix_web::web;
use chrono::Utc;
use log::{debug, error, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fs;
use std::fs::File;
use std::io::Error;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use rand::Rng;

// Constants
const METADATA_PATH: &str = "/workspace/ext/data/metadata/metadata.json";
const BASE_PATH_MARKER: &str = "/workspace/ext/data/metadata/base_path.txt";
pub const MARKDOWN_DIR: &str = "/workspace/ext/data/markdown";
const GITHUB_API_DELAY: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize, Clone)]
pub struct ProcessedFile {
    pub file_name: String,
    pub content: String,
    pub is_public: bool,
    pub metadata: Metadata,
}

/// Temporary struct for extracting ontology data from markdown
#[derive(Default)]
struct OntologyData {
    term_id: Option<String>,
    preferred_term: Option<String>,
    source_domain: Option<String>,
    ontology_status: Option<String>,
    owl_class: Option<String>,
    owl_physicality: Option<String>,
    owl_role: Option<String>,
    quality_score: Option<f64>,
    authority_score: Option<f64>,
    belongs_to_domain: Vec<String>,
    maturity: Option<String>,
    is_subclass_of: Vec<String>,
    definition: Option<String>,
}

pub struct FileService {
    _settings: Arc<RwLock<AppFullSettings>>, 
    
    node_id_counter: AtomicU32,
}

impl FileService {
    pub fn new(_settings: Arc<RwLock<AppFullSettings>>) -> Self {
        
        
        let service = Self {
            _settings, 
            node_id_counter: AtomicU32::new(1),
        };

        
        if let Ok(metadata) = Self::load_or_create_metadata() {
            let max_id = metadata.get_max_node_id();
            if max_id > 0 {
                
                service.node_id_counter.store(max_id + 1, Ordering::SeqCst);
                info!(
                    "Initialized node ID counter to {} based on existing metadata",
                    max_id + 1
                );
            }
        }

        service
    }

    
    fn get_next_node_id(&self) -> u32 {
        self.node_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    
    fn update_node_ids(&self, processed_files: &mut Vec<ProcessedFile>) {
        for processed_file in processed_files {
            if processed_file.metadata.node_id == "0" {
                processed_file.metadata.node_id = self.get_next_node_id().to_string();
            }
        }
    }

    
    pub async fn process_file_upload(&self, payload: web::Bytes) -> Result<GraphData, Error> {
        let content = String::from_utf8(payload.to_vec())
            .map_err(|e| Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        let metadata = Self::load_or_create_metadata()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        let mut graph_data = GraphData::new();

        
        let temp_filename = format!("temp_{}.md", time::timestamp_seconds());
        let temp_path = format!("{}/{}", MARKDOWN_DIR, temp_filename);
        if let Err(e) = fs::write(&temp_path, &content) {
            return Err(Error::new(std::io::ErrorKind::Other, e.to_string()));
        }

        
        let valid_nodes: Vec<String> = metadata
            .keys()
            .map(|name| name.trim_end_matches(".md").to_string())
            .collect();

        let references = Self::extract_references(&content, &valid_nodes);
        let topic_counts = Self::convert_references_to_topic_counts(references);

        // Create metadata with ontology fields extracted
        let mut file_metadata = Self::create_metadata_with_ontology(
            temp_filename.clone(),
            &content,
            self.get_next_node_id().to_string(),
            time::now(),
            None,
        );
        file_metadata.topic_counts = topic_counts;
        file_metadata.change_count = Some(1);

        
        graph_data
            .metadata
            .insert(temp_filename.clone(), file_metadata);

        
        if let Err(e) = fs::remove_file(&temp_path) {
            error!("Failed to remove temporary file: {}", e);
        }

        Ok(graph_data)
    }

    
    pub async fn list_files(&self) -> Result<Vec<String>, Error> {
        let metadata = Self::load_or_create_metadata()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        Ok(metadata.keys().cloned().collect())
    }

    
    pub async fn load_file(&self, filename: &str) -> Result<GraphData, Error> {
        let file_path = format!("{}/{}", MARKDOWN_DIR, filename);
        if !Path::new(&file_path).exists() {
            return Err(Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", filename),
            ));
        }

        let content = fs::read_to_string(&file_path)
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let metadata = Self::load_or_create_metadata()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        let mut graph_data = GraphData::new();

        
        let valid_nodes: Vec<String> = metadata
            .keys()
            .map(|name| name.trim_end_matches(".md").to_string())
            .collect();

        let references = Self::extract_references(&content, &valid_nodes);
        let topic_counts = Self::convert_references_to_topic_counts(references);

        // Create metadata with ontology fields extracted
        let mut file_metadata = Self::create_metadata_with_ontology(
            filename.to_string(),
            &content,
            self.get_next_node_id().to_string(),
            time::now(),
            None,
        );
        file_metadata.topic_counts = topic_counts;

        
        graph_data
            .metadata
            .insert(filename.to_string(), file_metadata);

        Ok(graph_data)
    }

    
    pub fn load_or_create_metadata() -> Result<MetadataStore, String> {
        // Use the correct metadata path constant
        let metadata_dir = Path::new(METADATA_PATH).parent().unwrap_or(Path::new("/workspace/ext/data/metadata"));
        std::fs::create_dir_all(metadata_dir)
            .map_err(|e| format!("Failed to create metadata directory: {}", e))?;

        let metadata_path = METADATA_PATH;

        match File::open(metadata_path) {
            Ok(file) => {
                info!("Loading existing metadata from {}", metadata_path);
                serde_json::from_reader(file)
                    .map_err(|e| format!("Failed to parse metadata: {}", e))
            }
            _ => {
                info!("Creating new metadata file at {}", metadata_path);
                let empty_store = MetadataStore::default();
                let file = File::create(metadata_path)
                    .map_err(|e| format!("Failed to create metadata file: {}", e))?;

                serde_json::to_writer_pretty(file, &empty_store)
                    .map_err(|e| format!("Failed to write metadata: {}", e))?;

                
                let metadata = std::fs::metadata(metadata_path)
                    .map_err(|e| format!("Failed to verify metadata file: {}", e))?;

                if !metadata.is_file() {
                    return Err("Metadata file was not created properly".to_string());
                }

                Ok(empty_store)
            }
        }
    }

    
    pub fn load_graph_data() -> Result<Option<GraphData>, String> {
        // Use metadata directory path for graph.json
        let metadata_dir = Path::new(METADATA_PATH).parent().unwrap_or(Path::new("/workspace/ext/data/metadata"));
        let graph_path = metadata_dir.join("graph.json");

        match File::open(&graph_path) {
            Ok(file) => {
                info!("Loading existing graph data from {:?}", graph_path);
                match serde_json::from_reader(file) {
                    Ok(graph) => {
                        info!("Successfully loaded graph data with positions");
                        Ok(Some(graph))
                    }
                    Err(e) => {
                        error!("Failed to parse graph.json: {}", e);
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                info!(
                    "No existing graph.json found: {}. Will generate positions.",
                    e
                );
                Ok(None)
            }
        }
    }

    
    fn calculate_node_size(file_size: usize) -> f64 {
        const BASE_SIZE: f64 = 1000.0; 
        const MIN_SIZE: f64 = 5.0; 
        const MAX_SIZE: f64 = 50.0; 

        let size = (file_size as f64 / BASE_SIZE).min(5.0);
        MIN_SIZE + (size * (MAX_SIZE - MIN_SIZE) / 5.0)
    }

    
    fn extract_references(content: &str, valid_nodes: &[String]) -> Vec<String> {
        let mut references = Vec::new();
        let content_lower = content.to_lowercase();

        for node_name in valid_nodes {
            let node_name_lower = node_name.to_lowercase();

            
            let pattern = format!(r"\b{}\b", regex::escape(&node_name_lower));
            if let Ok(re) = Regex::new(&pattern) {
                
                let count = re.find_iter(&content_lower).count();

                
                if count > 0 {
                    debug!("Found {} references to {} in content", count, node_name);
                    
                    for _ in 0..count {
                        references.push(node_name.clone());
                    }
                }
            }
        }

        references
    }

    fn convert_references_to_topic_counts(references: Vec<String>) -> HashMap<String, usize> {
        let mut topic_counts = HashMap::new();
        for reference in references {
            *topic_counts.entry(reference).or_insert(0) += 1;
        }
        topic_counts
    }

    
    pub async fn initialize_local_storage(
        settings: Arc<RwLock<AppFullSettings>>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {

        let github_config =
            GitHubConfig::from_env().map_err(|e| Box::new(e) as Box<dyn StdError + Send + Sync>)?;

        // Detect base path change — wipe local cache if target directory changed
        let current_base_path = github_config.base_path.clone();
        if Self::base_path_changed(&current_base_path) {
            info!("🔄 GITHUB_BASE_PATH changed to '{}' — clearing local file cache for fresh ingest", current_base_path);
            Self::clear_local_cache();
            Self::save_base_path_marker(&current_base_path);
        }

        let github = GitHubClient::new(github_config, Arc::clone(&settings)).await?;
        let content_api = ContentAPI::new(Arc::new(github));


        if Self::has_valid_local_setup() {
            info!("Valid local setup found, skipping initialization");
            return Ok(());
        }

        info!("Initializing local storage with files from GitHub");

        
        Self::ensure_directories()?;

        
        let basic_github_files = content_api.list_markdown_files("").await?;
        info!(
            "Found {} markdown files in GitHub",
            basic_github_files.len()
        );

        let mut metadata_store = MetadataStore::new();

        
        const BATCH_SIZE: usize = 5;
        for chunk in basic_github_files.chunks(BATCH_SIZE) {
            let mut futures = Vec::new();

            for file_basic_meta in chunk {
                let file_basic_meta = file_basic_meta.clone();
                let content_api = content_api.clone();

                futures.push(async move {
                    
                    let file_extended_meta = match content_api
                        .get_file_metadata_extended(&file_basic_meta.path)
                        .await
                    {
                        Ok(meta) => meta,
                        Err(e) => {
                            error!(
                                "Failed to get extended metadata for {}: {}",
                                file_basic_meta.name, e
                            );
                            return Err(e);
                        }
                    };

                    
                    match content_api
                        .fetch_file_content(&file_extended_meta.download_url)
                        .await
                    {
                        Ok(content) => {
                            // Check for public-access:: true (new) or public:: true (legacy)
                            let is_public = Self::is_public_file(&content);

                            if !is_public {
                                debug!(
                                    "Skipping file without public marker: {}",
                                    file_basic_meta.name
                                );
                                return Ok(None);
                            }

                            let file_path = format!("{}/{}", MARKDOWN_DIR, file_extended_meta.name);
                            if let Err(e) = fs::write(&file_path, &content) {
                                error!("Failed to write file {}: {}", file_path, e);
                                return Err(e.into());
                            }

                            info!(
                                "fetch_and_process_files: Successfully wrote {} to {}",
                                file_extended_meta.name, file_path
                            );

                            Ok(Some((file_extended_meta, content)))
                        }
                        Err(e) => {
                            error!(
                                "Failed to fetch content for {}: {}",
                                file_extended_meta.name, e
                            );
                            Err(e)
                        }
                    }
                });
            }

            
            let results = futures::future::join_all(futures).await;

            for result in results {
                match result {
                    Ok(Some((file_extended_meta, content))) => {
                        // Create metadata with ontology fields extracted
                        let metadata = Self::create_metadata_with_ontology(
                            file_extended_meta.name.clone(),
                            &content,
                            "0".to_string(), // Will be assigned later
                            file_extended_meta.last_content_modified,
                            Some(file_extended_meta.sha.clone()),
                        );

                        metadata_store.insert(file_extended_meta.name, metadata);
                    }
                    Ok(None) => continue, 
                    Err(e) => {
                        error!("Failed to process file in batch: {}", e);
                    }
                }
            }

            sleep(GITHUB_API_DELAY).await;
        }

        
        Self::update_topic_counts(&mut metadata_store)?;

        
        info!("Saving metadata for {} public files", metadata_store.len());
        Self::save_metadata(&metadata_store)?;

        // Record current base path for future change detection
        Self::save_base_path_marker(&current_base_path);

        info!(
            "Initialization complete. Processed {} public files",
            metadata_store.len()
        );
        Ok(())
    }

    
    fn update_topic_counts(metadata_store: &mut MetadataStore) -> Result<(), Error> {
        let valid_nodes: Vec<String> = metadata_store
            .keys()
            .map(|name| name.trim_end_matches(".md").to_string())
            .collect();

        for file_name in metadata_store.keys().cloned().collect::<Vec<_>>() {
            let file_path = format!("{}/{}", MARKDOWN_DIR, file_name);
            if let Ok(content) = fs::read_to_string(&file_path) {
                let references = Self::extract_references(&content, &valid_nodes);
                let topic_counts = Self::convert_references_to_topic_counts(references);

                if let Some(metadata) = metadata_store.get_mut(&file_name) {
                    metadata.topic_counts = topic_counts;
                }
            }
        }

        Ok(())
    }

    
    fn has_valid_local_setup() -> bool {
        if let Ok(metadata_content) = fs::read_to_string(METADATA_PATH) {
            if metadata_content.trim().is_empty() {
                return false;
            }

            if let Ok(metadata) = serde_json::from_str::<MetadataStore>(&metadata_content) {
                return metadata.validate_files(MARKDOWN_DIR);
            }
        }
        false
    }

    /// Check if GITHUB_BASE_PATH changed since last successful sync
    fn base_path_changed(current_base_path: &str) -> bool {
        match fs::read_to_string(BASE_PATH_MARKER) {
            Ok(stored) => stored.trim() != current_base_path.trim(),
            Err(_) => false, // No marker = first run, not a "change"
        }
    }

    /// Record the current base path for future change detection
    fn save_base_path_marker(base_path: &str) {
        if let Some(parent) = Path::new(BASE_PATH_MARKER).parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(BASE_PATH_MARKER, base_path) {
            warn!("Failed to write base path marker: {}", e);
        }
    }

    /// Clear local markdown files and metadata for a fresh ingest
    fn clear_local_cache() {
        // Remove metadata.json
        if let Err(e) = fs::remove_file(METADATA_PATH) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove {}: {}", METADATA_PATH, e);
            }
        }

        // Remove all .md files from the markdown directory
        if let Ok(entries) = fs::read_dir(MARKDOWN_DIR) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "md") {
                    if let Err(e) = fs::remove_file(&path) {
                        warn!("Failed to remove {}: {}", path.display(), e);
                    }
                }
            }
        }
        info!("Local file cache cleared (metadata.json + markdown files)");
    }

    
    fn ensure_directories() -> Result<(), Error> {
        let markdown_dir = Path::new(MARKDOWN_DIR);
        let metadata_path = Path::new(METADATA_PATH);

        info!("Ensuring directories exist...");
        info!("MARKDOWN_DIR (absolute): {:?}", fs::canonicalize(markdown_dir.parent().unwrap_or(Path::new("/"))).unwrap_or_else(|_| markdown_dir.to_path_buf()));
        info!("METADATA_PATH (absolute): {:?}", fs::canonicalize(metadata_path.parent().unwrap_or(Path::new("/"))).unwrap_or_else(|_| metadata_path.to_path_buf()));

        if !markdown_dir.exists() {
            info!("Creating markdown directory at {:?}", markdown_dir);
            fs::create_dir_all(markdown_dir).map_err(|e| {
                Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create markdown directory: {}", e),
                )
            })?;
            
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(markdown_dir, fs::Permissions::from_mode(0o777)).map_err(
                    |e| {
                        Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to set markdown directory permissions: {}", e),
                        )
                    },
                )?;
            }
        }

        
        let metadata_dir = Path::new(METADATA_PATH).parent()
            .expect("METADATA_PATH constant has a known parent directory");
        if !metadata_dir.exists() {
            info!("Creating metadata directory at {:?}", metadata_dir);
            fs::create_dir_all(metadata_dir).map_err(|e| {
                Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create metadata directory: {}", e),
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(metadata_dir, fs::Permissions::from_mode(0o777)).map_err(
                    |e| {
                        Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to set metadata directory permissions: {}", e),
                        )
                    },
                )?;
            }
        }

        
        let test_file = format!("{}/test_permissions", MARKDOWN_DIR);
        match fs::write(&test_file, "test") {
            Ok(_) => {
                info!("Successfully wrote test file to {}", test_file);
                fs::remove_file(&test_file).map_err(|e| {
                    Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to remove test file: {}", e),
                    )
                })?;
                info!("Successfully removed test file");
                info!("Directory permissions verified");
                Ok(())
            }
            Err(e) => {
                // EROFS (30) is expected — source pages are mounted read-only by design.
                // Demote to debug so we don't flood the log on every tick.
                if e.raw_os_error() == Some(30) {
                    debug!(
                        "Markdown dir {} is read-only (EROFS) — expected for bind-mounted source",
                        MARKDOWN_DIR
                    );
                } else {
                    error!("Failed to verify directory permissions: {}", e);
                    if let Ok(current_dir) = std::env::current_dir() {
                        error!("Current directory: {:?}", current_dir);
                    }
                    if let Ok(dir_contents) = fs::read_dir(MARKDOWN_DIR) {
                        error!("Directory contents: {:?}", dir_contents);
                    }
                }
                Err(Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Failed to verify directory permissions: {}", e),
                ))
            }
        }
    }

    
    pub fn save_metadata(metadata: &MetadataStore) -> Result<(), Error> {
        let json = crate::utils::json::to_json_pretty(metadata)
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        fs::write(METADATA_PATH, json)
            .map_err(|e| Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok(())
    }

    /// Scan local markdown files and create metadata from them
    /// This is used as a fallback when GitHub sync fails or when local files exist
    pub fn scan_local_files_to_metadata() -> Result<MetadataStore, String> {
        info!("Scanning local markdown files from {}", MARKDOWN_DIR);

        let markdown_dir = Path::new(MARKDOWN_DIR);
        if !markdown_dir.exists() {
            return Err(format!("Markdown directory does not exist: {}", MARKDOWN_DIR));
        }

        let mut metadata_store = MetadataStore::new();
        let mut node_id_counter: u32 = 1;

        // Read all .md files from the directory
        let entries = fs::read_dir(markdown_dir)
            .map_err(|e| format!("Failed to read markdown directory: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(name) => name.to_string(),
                    None => continue,
                };

                // Read file content
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to read file {}: {}", file_name, e);
                        continue;
                    }
                };

                // COMMENTED OUT: Include ALL files regardless of public status
                // if !Self::is_public_file(&content) {
                //     debug!("Skipping non-public file: {}", file_name);
                //     continue;
                // }

                debug!("Processing file: {}", file_name);

                // Create metadata with ontology fields
                let metadata = Self::create_metadata_with_ontology(
                    file_name.clone(),
                    &content,
                    node_id_counter.to_string(),
                    Utc::now(),
                    None, // No blob SHA for local files
                );

                metadata_store.insert(file_name, metadata);
                node_id_counter += 1;
            }
        }

        // Update topic counts (cross-references between files)
        let valid_nodes: Vec<String> = metadata_store
            .keys()
            .map(|name| name.trim_end_matches(".md").to_string())
            .collect();

        for file_name in metadata_store.keys().cloned().collect::<Vec<_>>() {
            let file_path = format!("{}/{}", MARKDOWN_DIR, file_name);
            if let Ok(content) = fs::read_to_string(&file_path) {
                let references = Self::extract_references(&content, &valid_nodes);
                let topic_counts = Self::convert_references_to_topic_counts(references);

                if let Some(metadata) = metadata_store.get_mut(&file_name) {
                    metadata.topic_counts = topic_counts;
                }
            }
        }

        info!(
            "Local scan complete: Found {} markdown files",
            metadata_store.len()
        );

        // Save metadata to disk
        if !metadata_store.is_empty() {
            Self::save_metadata(&metadata_store)
                .map_err(|e| format!("Failed to save metadata: {}", e))?;
        }

        Ok(metadata_store)
    }


    fn calculate_sha1(content: &str) -> String {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }


    fn count_hyperlinks(content: &str) -> usize {
        let re = Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").expect("Invalid regex pattern");
        re.find_iter(content).count()
    }

    /// Check if file has public-access:: true (new format)
    /// or public:: true anywhere in the content (legacy format)
    fn is_public_file(content: &str) -> bool {
        // Check new format: public-access:: true anywhere in content
        if content.contains("public-access:: true") {
            return true;
        }
        // Legacy format: public:: true anywhere in content (can be on any line)
        // Look for the pattern as a standalone line or Logseq property
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('-').trim();
            if trimmed == "public:: true" {
                return true;
            }
        }
        false
    }

    /// Extract ontology data from markdown content with new header format
    fn extract_ontology_data(content: &str) -> OntologyData {
        let mut data = OntologyData::default();

        // Parse key-value pairs from ontology block
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('-').trim();

            if let Some((key, value)) = trimmed.split_once("::") {
                let key = key.trim();
                let value = value.trim();

                match key {
                    "term-id" => data.term_id = Some(value.to_string()),
                    "preferred-term" => data.preferred_term = Some(value.to_string()),
                    "source-domain" => data.source_domain = Some(value.to_string()),
                    "status" => data.ontology_status = Some(value.to_string()),
                    "owl:class" => data.owl_class = Some(value.to_string()),
                    "owl:physicality" => data.owl_physicality = Some(value.to_string()),
                    "owl:role" => data.owl_role = Some(value.to_string()),
                    "quality-score" => data.quality_score = value.parse().ok(),
                    "authority-score" => data.authority_score = value.parse().ok(),
                    "maturity" => data.maturity = Some(value.to_string()),
                    "definition" => data.definition = Some(value.to_string()),
                    "belongsToDomain" => {
                        // Parse [[Domain1]], [[Domain2]] format
                        let domains: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().trim_start_matches("[[").trim_end_matches("]]").to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        data.belongs_to_domain = domains;
                    }
                    "is-subclass-of" => {
                        // Parse [[Class]] format, accumulate multiple
                        let class = value.trim().trim_start_matches("[[").trim_end_matches("]]").to_string();
                        if !class.is_empty() {
                            data.is_subclass_of.push(class);
                        }
                    }
                    _ => {}
                }
            }
        }

        data
    }

    /// Create Metadata with ontology fields populated
    fn create_metadata_with_ontology(
        file_name: String,
        content: &str,
        node_id: String,
        last_modified: chrono::DateTime<Utc>,
        file_blob_sha: Option<String>,
    ) -> Metadata {
        let file_size = content.len();
        let node_size = Self::calculate_node_size(file_size);
        let ontology = Self::extract_ontology_data(content);

        Metadata {
            file_name,
            file_size,
            node_size,
            node_id,
            hyperlink_count: Self::count_hyperlinks(content),
            sha1: Self::calculate_sha1(content),
            last_modified,
            last_content_change: Some(last_modified),
            last_commit: Some(last_modified),
            change_count: None,
            file_blob_sha,
            perplexity_link: String::new(),
            last_perplexity_process: None,
            topic_counts: HashMap::new(),
            // Ontology fields
            term_id: ontology.term_id,
            preferred_term: ontology.preferred_term,
            source_domain: ontology.source_domain,
            ontology_status: ontology.ontology_status,
            owl_class: ontology.owl_class,
            owl_physicality: ontology.owl_physicality,
            owl_role: ontology.owl_role,
            quality_score: ontology.quality_score,
            authority_score: ontology.authority_score,
            belongs_to_domain: ontology.belongs_to_domain,
            maturity: ontology.maturity,
            is_subclass_of: ontology.is_subclass_of,
            definition: ontology.definition,
        }
    }

    
    #[allow(dead_code)]
    async fn should_process_file(
        &self,
        file_name: &str,
        github_blob_sha: &str,
        content_api: &ContentAPI,
        download_url: &str,
        metadata_store: &MetadataStore,
    ) -> Result<bool, Box<dyn StdError + Send + Sync>> {
        
        if let Some(existing_metadata) = metadata_store.get(file_name) {
            
            if let Some(stored_sha) = &existing_metadata.file_blob_sha {
                if stored_sha == github_blob_sha {
                    info!(
                        "should_process_file: File {} has unchanged SHA, skipping",
                        file_name
                    );
                    return Ok(false);
                } else {
                    info!(
                        "should_process_file: File {} SHA changed (old: {}, new: {})",
                        file_name, stored_sha, github_blob_sha
                    );
                }
            } else {
                info!(
                    "should_process_file: File {} has no stored SHA, will check content",
                    file_name
                );
            }
        } else {
            info!(
                "should_process_file: File {} is new, will check content",
                file_name
            );
        }

        
        info!(
            "should_process_file: Downloading content for {} to check public tag",
            file_name
        );
        match content_api.fetch_file_content(download_url).await {
            Ok(content) => {
                // Check for public-access:: true (new) or public:: true (legacy)
                let is_public = Self::is_public_file(&content);
                if !is_public {
                    info!("should_process_file: File {} does not have public marker, skipping", file_name);
                } else {
                    info!(
                        "should_process_file: File {} has public marker, will process",
                        file_name
                    );
                }
                Ok(is_public)
            }
            Err(e) => {
                error!("Failed to fetch content for {}: {}", file_name, e);
                Err(Box::new(e))
            }
        }
    }

    
    pub async fn fetch_and_process_files(
        &self,
        content_api: Arc<ContentAPI>,
        _settings: Arc<RwLock<AppFullSettings>>, 
        metadata_store: &mut MetadataStore,
    ) -> Result<Vec<ProcessedFile>, Box<dyn StdError + Send + Sync>> {
        info!("fetch_and_process_files: Starting GitHub file fetch process");
        debug!("Attempting to fetch and process files from GitHub repository.");
        let mut processed_files = Vec::new();

        
        info!("fetch_and_process_files: Calling list_markdown_files...");
        let basic_github_files = match content_api.list_markdown_files("").await {
            Ok(files) => {
                info!(
                    "fetch_and_process_files: Successfully retrieved {} file entries from GitHub",
                    files.len()
                );
                debug!(
                    "GitHub API returned {} potential markdown files.",
                    files.len()
                );
                if files.is_empty() {
                    warn!("fetch_and_process_files: No markdown files found in GitHub repository");
                    warn!("fetch_and_process_files: Check GITHUB_OWNER, GITHUB_REPO, and GITHUB_BASE_PATH in .env");
                }
                files
            }
            Err(e) => {
                error!(
                    "fetch_and_process_files: Failed to list markdown files from GitHub: {}",
                    e
                );
                return Err(Box::new(e));
            }
        };

        info!(
            "fetch_and_process_files: Processing {} markdown files from GitHub",
            basic_github_files.len()
        );

        
        const BATCH_SIZE: usize = 5;
        let total_batches = (basic_github_files.len() + BATCH_SIZE - 1) / BATCH_SIZE;
        info!(
            "fetch_and_process_files: Processing files in {} batches of up to {} files each",
            total_batches, BATCH_SIZE
        );

        for (batch_idx, chunk) in basic_github_files.chunks(BATCH_SIZE).enumerate() {
            info!(
                "fetch_and_process_files: Processing batch {}/{} with {} files",
                batch_idx + 1,
                total_batches,
                chunk.len()
            );
            let mut futures = Vec::new();

            for file_basic_meta in chunk {
                let file_basic_meta = file_basic_meta.clone();
                let content_api = content_api.clone();
                let metadata_store_clone = metadata_store.clone();

                info!(
                    "fetch_and_process_files: Checking file: {}",
                    file_basic_meta.name
                );

                futures.push(async move {
                    
                    let file_extended_meta = match content_api.get_file_metadata_extended(&file_basic_meta.path).await {
                        Ok(meta) => meta,
                        Err(e) => {
                            error!("Failed to get extended metadata for {}: {}", file_basic_meta.name, e);
                            return Err(e);
                        }
                    };

                    
                    let needs_download = if let Some(existing_metadata) = metadata_store_clone.get(&file_extended_meta.name) {
                        if let Some(stored_sha) = &existing_metadata.file_blob_sha {
                            if stored_sha == &file_extended_meta.sha {
                                info!("fetch_and_process_files: File {} has unchanged SHA, skipping download", file_extended_meta.name);
                                false
                            } else {
                                info!("fetch_and_process_files: File {} SHA changed (old: {}, new: {})",
                                     file_extended_meta.name, stored_sha, file_extended_meta.sha);
                                true
                            }
                        } else {
                            info!("fetch_and_process_files: File {} has no stored SHA, will download", file_extended_meta.name);
                            true
                        }
                    } else {
                        info!("fetch_and_process_files: File {} is new, will download", file_extended_meta.name);
                        true
                    };

                    if !needs_download {
                        return Ok(None);
                    }

                    
                    match content_api.fetch_file_content(&file_extended_meta.download_url).await {
                        Ok(content) => {
                            // Check for public-access:: true (new) or public:: true (legacy)
                            let is_public = Self::is_public_file(&content);

                            if !is_public {
                                info!("fetch_and_process_files: File {} does not have public marker",
                                     file_extended_meta.name);
                                return Ok(None);
                            }

                            info!("fetch_and_process_files: File {} is marked as public, writing to disk", file_extended_meta.name);

                            let file_path = format!("{}/{}", MARKDOWN_DIR, file_extended_meta.name);
                            if let Err(e) = fs::write(&file_path, &content) {
                                error!("Failed to write file {}: {}", file_path, e);
                                return Err(e.into());
                            }

                            info!("fetch_and_process_files: Successfully wrote {} to {}", file_extended_meta.name, file_path);

                            // Create metadata with ontology fields extracted
                            let metadata = Self::create_metadata_with_ontology(
                                file_extended_meta.name.clone(),
                                &content,
                                "0".to_string(), // Will be assigned later
                                file_extended_meta.last_content_modified,
                                Some(file_extended_meta.sha.clone()),
                            );

                            Ok(Some(ProcessedFile {
                                file_name: file_extended_meta.name.clone(),
                                content,
                                is_public: true,
                                metadata,
                            }))
                        }
                        Err(e) => {
                            error!("Failed to fetch content for {}: {}", file_basic_meta.name, e);
                            Err(e)
                        }
                    }
                });
            }

            
            let results = futures::future::join_all(futures).await;

            for result in results {
                match result {
                    Ok(Some(processed_file)) => {
                        processed_files.push(processed_file);
                    }
                    Ok(None) => continue, 
                    Err(e) => {
                        error!("Failed to process file in batch: {}", e);
                    }
                }
            }

            sleep(GITHUB_API_DELAY).await;
        }

        
        self.update_node_ids(&mut processed_files);

        
        for processed_file in &processed_files {
            metadata_store.insert(
                processed_file.file_name.clone(),
                processed_file.metadata.clone(),
            );
        }

        
        Self::update_topic_counts(metadata_store)?;

        Ok(processed_files)
    }

    /// Load complete graph from local markdown files into Neo4j database
    pub async fn load_graph_from_files_into_neo4j(
        neo4j_adapter: &Arc<crate::adapters::neo4j_adapter::Neo4jAdapter>,
    ) -> Result<(), String> {
        info!("Starting to load graph from local files into Neo4j...");

        let metadata = Self::load_or_create_metadata()?;
        if metadata.is_empty() {
            warn!("metadata.json is empty. No data to load into Neo4j.");
            return Ok(());
        }

        let mut graph_data = GraphData::new();

        // Deterministic hash ID from page name — must match KGParser::page_name_to_id exactly.
        // Uses BLAKE3 for cross-Rust-version stability (P1-24).
        let page_name_to_id = |page_name: &str| -> u32 {
            let hash = blake3::hash(page_name.as_bytes());
            let bytes = hash.as_bytes();
            let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            (raw & 0xFFFF_FFFE) + 1
        };

        // Strip OntologyBlock section before wikilink extraction to avoid hub-node explosion
        fn strip_ontology_block(content: &str) -> &str {
            if let Some(pos) = content.find("### OntologyBlock") {
                &content[..pos]
            } else {
                content
            }
        }

        // Phase 1: Create nodes and collect file contents + actual IDs.
        let mut term_to_id: HashMap<String, u32> = HashMap::new();
        let mut file_contents: Vec<(String, u32)> = Vec::new(); // (content, actual_node_id)

        for (filename, meta) in metadata.iter() {
            let file_path = Path::new(MARKDOWN_DIR).join(filename);
            let content = match fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to read file {}: {}. Skipping.", file_path.display(), e);
                    continue;
                }
            };

            let page_name = meta.file_name.trim_end_matches(".md");
            let hash_id = page_name_to_id(page_name);

            let mut node = AppNode::new_with_id(filename.clone(), Some(hash_id));
            node.id = hash_id; // ensure ID matches hash regardless of NEXT_NODE_ID
            node.label = page_name.to_string();
            node.node_type = Some("page".to_string());
            node.ensure_kind_id();
            node.size = Some(meta.node_size as f32);
            node.color = Some("#888888".to_string());
            let mut rng = rand::thread_rng();
            node.data.x = rng.gen_range(-100.0..100.0);
            node.data.y = rng.gen_range(-100.0..100.0);
            node.data.z = rng.gen_range(-100.0..100.0);

            // Map page name and preferred term → hash ID for wikilink resolution
            term_to_id.insert(page_name.to_lowercase(), hash_id);
            if let Some(ref term) = meta.preferred_term {
                term_to_id.insert(term.to_lowercase(), hash_id);
            }

            graph_data.nodes.push(node);
            file_contents.push((content, hash_id));
        }

        info!(
            "Phase 1: Created {} nodes, preferred_term mapping has {} entries",
            graph_data.nodes.len(), term_to_id.len()
        );

        // Phase 2: Extract edges from wikilinks, excluding OntologyBlock content.
        let wikilink_re = Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")
            .expect("Invalid wikilink regex");
        let mut seen_edges = std::collections::HashSet::new();

        for (content, source_id) in &file_contents {
            let kg_content = strip_ontology_block(content);
            for cap in wikilink_re.captures_iter(kg_content) {
                if let Some(link_match) = cap.get(1) {
                    let target = link_match.as_str().trim().to_lowercase();
                    if let Some(&target_id) = term_to_id.get(&target) {
                        let edge_key = (*source_id, target_id);
                        if target_id != *source_id && seen_edges.insert(edge_key) {
                            graph_data.edges.push(AppEdge::new(*source_id, target_id, 1.0));
                        }
                    }
                }
            }
        }

        info!(
            "Phase 2: Extracted {} edges from wikilinks across {} files.",
            graph_data.edges.len(), file_contents.len()
        );
        info!(
            "Total: {} nodes and {} edges ready for Neo4j.",
            graph_data.nodes.len(),
            graph_data.edges.len()
        );

        // Collect metadata_ids of all nodes we're about to upsert
        let current_metadata_ids: Vec<String> = graph_data
            .nodes
            .iter()
            .map(|n| n.metadata_id.clone())
            .collect();

        // Remove only nodes whose source files no longer exist (not in current set)
        // This preserves nodes from prior syncs that still have valid source files
        info!("Removing stale nodes from Neo4j (nodes whose source files were removed)...");
        {
            use neo4rs::BoltType;
            let id_list: Vec<BoltType> = current_metadata_ids
                .iter()
                .map(|s| BoltType::from(s.clone()))
                .collect();
            let mut params = HashMap::new();
            params.insert("current_ids".to_string(), BoltType::from(id_list));

            match neo4j_adapter.execute_cypher_safe(
                "MATCH (n:KGNode)
                 WHERE n.metadata_id IS NOT NULL
                   AND NOT n.metadata_id IN $current_ids
                 WITH n, n.metadata_id AS mid
                 DETACH DELETE n
                 RETURN count(*) AS removed, collect(mid) AS removed_ids",
                params,
            ).await {
                Ok(rows) => {
                    if let Some(row) = rows.first() {
                        let removed = row.get("removed")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if removed > 0 {
                            let removed_ids: Vec<&str> = row.get("removed_ids")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .take(10)
                                    .collect())
                                .unwrap_or_default();
                            info!(
                                "Removed {} stale nodes from Neo4j: {:?}",
                                removed, removed_ids
                            );
                        } else {
                            info!("No stale nodes to remove.");
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to remove stale nodes: {}. Continuing with upsert.", e);
                }
            }
        }

        info!("Upserting {} nodes into Neo4j (preserving existing physics state)...", graph_data.nodes.len());
        if let Err(e) = neo4j_adapter.save_graph(&graph_data).await {
            return Err(format!("Failed to save graph to Neo4j: {}", e));
        }

        info!(
            "✅ Successfully synced Neo4j: {} nodes upserted from local files.",
            graph_data.nodes.len()
        );
        Ok(())
    }
}