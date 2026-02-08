//! SemanticProcessorActor - Specialized actor for semantic analysis and constraint processing
//!
//! This actor handles:
//! - Semantic analysis of graph metadata and content
//! - Dynamic semantic constraint generation and management
//! - AI feature extraction and processing
//! - Stress majorization optimization for semantic layouts
//! - Advanced semantic parameter management

use actix::dev::{MessageResponse, OneshotSender};
use actix::prelude::*;
use actix_web::web;
use futures_util::FutureExt;
use log::{debug, error, info, warn};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

// Core models and services
use crate::models::constraints::{AdvancedParams, Constraint, ConstraintSet};
use crate::models::graph::GraphData;
use crate::models::metadata::FileMetadata;
use crate::models::node::Node;
use crate::physics::stress_majorization::{OptimizationResult, StressMajorizationSolver};
use crate::services::semantic_analyzer::{
    SemanticAnalyzer, SemanticAnalyzerConfig, SemanticFeatures,
};

// Message types
use crate::actors::messages::{
    ComputeAllPairsShortestPaths, ComputeShortestPaths, GetConstraints,
    RegenerateSemanticConstraints, TriggerStressMajorization, UpdateAdvancedParams,
    UpdateConstraints,
};

// GPU semantic analyzer (conditionally compiled)
#[cfg(feature = "gpu")]
use crate::adapters::gpu_semantic_analyzer::GpuSemanticAnalyzerAdapter;
#[cfg(feature = "gpu")]
use crate::ports::gpu_semantic_analyzer::{
    GpuSemanticAnalyzer as GpuSemanticAnalyzerPort, PathfindingResult,
};

// CPU-only stubs when GPU feature is disabled
#[cfg(not(feature = "gpu"))]
pub use crate::ports::gpu_semantic_analyzer::PathfindingResult;

#[cfg(not(feature = "gpu"))]
pub struct GpuSemanticAnalyzerAdapter;

#[cfg(not(feature = "gpu"))]
impl GpuSemanticAnalyzerAdapter {
    pub fn new() -> Self {
        Self
    }

    pub async fn initialize(&self, _graph: std::sync::Arc<crate::models::graph::GraphData>) -> Result<(), String> {
        // CPU fallback: no-op initialization
        Ok(())
    }

    pub async fn compute_shortest_paths(&self, source_node_id: u32) -> Result<PathfindingResult, String> {
        // CPU fallback: return empty result
        Err(format!("GPU not available for SSSP from node {}", source_node_id))
    }

    pub async fn compute_all_pairs_shortest_paths(&self) -> Result<std::collections::HashMap<(u32, u32), Vec<u32>>, String> {
        // CPU fallback: return error
        Err("GPU not available for APSP computation".to_string())
    }
}

use crate::utils::time;

#[derive(Debug, Clone)]
pub struct SemanticProcessorConfig {
    
    pub max_constraints_per_cycle: usize,
    
    pub similarity_threshold: f32,
    
    pub enable_ai_features: bool,
    
    pub stress_convergence_threshold: f32,
    
    pub max_stress_iterations: usize,
    
    pub enable_constraint_caching: bool,
}

impl Default for SemanticProcessorConfig {
    fn default() -> Self {
        Self {
            max_constraints_per_cycle: 1000,
            similarity_threshold: 0.7,
            enable_ai_features: true,
            stress_convergence_threshold: 0.001,
            max_stress_iterations: 500,
            enable_constraint_caching: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SemanticStats {
    pub constraints_generated: usize,
    pub constraints_active: usize,
    pub last_analysis_duration: Option<std::time::Duration>,
    pub stress_iterations: u32,
    pub stress_final_value: f32,
    pub semantic_features_cached: usize,
    pub ai_features_processed: usize,
}

impl<A, M> MessageResponse<A, M> for SemanticStats
where
    A: Actor,
    M: Message<Result = SemanticStats>,
{
    fn handle(self, _ctx: &mut A::Context, tx: Option<OneshotSender<M::Result>>) {
        if let Some(tx) = tx {
            let _ = tx.send(self);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AISemanticFeatures {
    
    pub content_embedding: Vec<f32>,
    
    pub topic_classifications: HashMap<String, f32>,
    
    pub importance_score: f32,
    
    pub conceptual_links: Vec<(u32, f32)>, 
    
    pub complexity_metrics: HashMap<String, f32>,
    
    pub sentiment_analysis: Option<HashMap<String, f32>>,
    
    pub named_entities: Vec<String>,
    
    pub cluster_assignments: Vec<String>,
}

impl Default for AISemanticFeatures {
    fn default() -> Self {
        Self {
            content_embedding: Vec::new(),
            topic_classifications: HashMap::new(),
            importance_score: 0.5,
            conceptual_links: Vec::new(),
            complexity_metrics: HashMap::new(),
            sentiment_analysis: None,
            named_entities: Vec::new(),
            cluster_assignments: Vec::new(),
        }
    }
}

pub struct SemanticProcessorActor {
    
    semantic_analyzer: Option<SemanticAnalyzer>,

    
    constraint_set: ConstraintSet,

    
    stress_solver: Option<StressMajorizationSolver>,

    
    semantic_features_cache: HashMap<String, SemanticFeatures>,

    
    ai_features_cache: HashMap<String, AISemanticFeatures>,

    
    advanced_params: AdvancedParams,

    
    config: SemanticProcessorConfig,

    
    stats: SemanticStats,

    
    graph_data: Option<Arc<GraphData>>,

    
    _last_semantic_analysis: Option<Instant>,


    _constraint_cache: HashMap<String, Vec<Constraint>>,


    _active_tasks: HashMap<String, SemanticTask>,

    
    relationship_threshold: f32,

    
    _enable_ai_processing: bool,

    
    clustering_params: SemanticClusteringParams,

    
    performance_metrics: HashMap<String, f32>,

    
    gpu_analyzer: Option<GpuSemanticAnalyzerAdapter>,
}

#[derive(Debug, Clone)]
pub struct SemanticTask {
    pub task_id: String,
    pub task_type: SemanticTaskType,
    pub status: SemanticTaskStatus,
    pub started_at: Instant,
    pub progress: f32, 
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub enum SemanticTaskType {
    ConstraintGeneration,
    StressOptimization,
    FeatureExtraction,
    ClusteringAnalysis,
    RelationshipMapping,
    AIProcessing,
}

#[derive(Debug, Clone)]
pub enum SemanticTaskStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct SemanticClusteringParams {
    pub min_cluster_size: usize,
    pub max_clusters: usize,
    pub similarity_threshold: f32,
    pub use_hierarchical: bool,
    pub enable_dynamic_clustering: bool,
}

impl Default for SemanticClusteringParams {
    fn default() -> Self {
        Self {
            min_cluster_size: 3,
            max_clusters: 50,
            similarity_threshold: 0.8,
            use_hierarchical: true,
            enable_dynamic_clustering: true,
        }
    }
}

impl SemanticProcessorActor {
    
    fn process_metadata_blocking(
        metadata_id: &str,
        metadata: &FileMetadata,
        semantic_analyzer: Option<SemanticAnalyzer>,
        config: SemanticProcessorConfig,
    ) -> Result<(), String> {
        let start_time = Instant::now();

        
        if let Some(mut analyzer) = semantic_analyzer {
            let features = analyzer.analyze_metadata(metadata);

            
            if config.enable_ai_features {
                
                let _ai_features = Self::extract_ai_features_static(metadata, &features)?;
            }
        }

        let duration = start_time.elapsed();
        debug!(
            "Processed semantic metadata for {} in thread pool: {:?}",
            metadata_id, duration
        );
        Ok(())
    }

    
    fn generate_semantic_constraints_blocking(
        graph_data: Option<Arc<GraphData>>,
        _semantic_features_cache: HashMap<String, SemanticFeatures>,
        _ai_features_cache: HashMap<String, AISemanticFeatures>,
        config: SemanticProcessorConfig,
    ) -> Result<Vec<Constraint>, String> {
        let start_time = Instant::now();

        let _graph_data = match graph_data {
            Some(data) => data,
            None => return Err("No graph data available for constraint generation".to_string()),
        };

        let mut constraints = Vec::new();

        
        

        
        constraints.truncate(config.max_constraints_per_cycle);

        let duration = start_time.elapsed();
        info!(
            "Generated {} semantic constraints in thread pool in {:?}",
            constraints.len(),
            duration
        );

        Ok(constraints)
    }

    
    fn execute_stress_optimization_blocking(
        graph_data: Option<Arc<GraphData>>,
        constraint_set: ConstraintSet,
        stress_solver: Option<StressMajorizationSolver>,
    ) -> Result<OptimizationResult, String> {
        let graph_data = match graph_data {
            Some(data) => data,
            None => return Err("No graph data available for stress optimization".to_string()),
        };

        let mut solver = match stress_solver {
            Some(solver) => solver,
            None => return Err("Stress solver not initialized".to_string()),
        };

        let start_time = Instant::now();
        let mut graph_clone = graph_data.as_ref().clone();

        let result = solver
            .optimize(&mut graph_clone, &constraint_set)
            .map_err(|e| format!("Stress optimization failed: {:?}", e))?;

        let duration = start_time.elapsed();
        info!("Completed stress optimization in thread pool: {} iterations, final stress: {:.6}, duration: {:?}",
              result.iterations, result.final_stress, duration);

        Ok(result)
    }

    
    fn extract_ai_features_static(
        metadata: &FileMetadata,
        base_features: &SemanticFeatures,
    ) -> Result<AISemanticFeatures, String> {
        let mut ai_features = AISemanticFeatures::default();

        
        ai_features.content_embedding =
            Self::generate_content_embedding_static(&metadata.file_name)?;

        
        ai_features.topic_classifications =
            Self::classify_topics_static(&metadata.file_name, base_features)?;

        
        ai_features.importance_score =
            Self::calculate_importance_score_static(metadata, base_features);

        
        ai_features.conceptual_links =
            Self::extract_conceptual_relationships_static(metadata, base_features)?;

        
        ai_features.complexity_metrics =
            Self::analyze_language_complexity_static(&metadata.file_name)?;

        
        if metadata.file_name.len() > 3 {
            ai_features.sentiment_analysis =
                Some(Self::analyze_sentiment_static(&metadata.file_name)?);
        }

        
        ai_features.named_entities = Self::extract_named_entities_static(&metadata.file_name)?;

        
        ai_features.cluster_assignments =
            Self::determine_cluster_assignments_static(metadata, base_features)?;

        Ok(ai_features)
    }

    
    fn generate_content_embedding_static(content: &str) -> Result<Vec<f32>, String> {
        
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut embedding = vec![0.0; 256]; 

        for (i, word) in words.iter().enumerate().take(100) {
            let hash = Self::simple_hash_static(word) % 256;
            embedding[hash] += 1.0 / (i as f32 + 1.0);
        }

        
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for val in &mut embedding {
                *val /= magnitude;
            }
        }

        Ok(embedding)
    }

    
    fn simple_hash_static(s: &str) -> usize {
        s.chars()
            .fold(0, |acc, c| acc.wrapping_mul(31).wrapping_add(c as usize))
    }

    
    fn classify_topics_static(
        content: &str,
        _features: &SemanticFeatures,
    ) -> Result<HashMap<String, f32>, String> {
        let mut topics = HashMap::new();
        let content_lower = content.to_lowercase();

        
        let topic_keywords = vec![
            (
                "technology",
                vec![
                    "code",
                    "software",
                    "programming",
                    "algorithm",
                    "data",
                    "computer",
                ],
            ),
            (
                "science",
                vec![
                    "research",
                    "experiment",
                    "hypothesis",
                    "analysis",
                    "theory",
                    "study",
                ],
            ),
            (
                "business",
                vec![
                    "market", "strategy", "revenue", "customer", "product", "sales",
                ],
            ),
            (
                "education",
                vec!["learn", "teach", "student", "course", "knowledge", "skill"],
            ),
            (
                "health",
                vec![
                    "medical",
                    "health",
                    "treatment",
                    "patient",
                    "disease",
                    "therapy",
                ],
            ),
            (
                "art",
                vec![
                    "creative",
                    "design",
                    "visual",
                    "artistic",
                    "aesthetic",
                    "culture",
                ],
            ),
        ];

        for (topic, keywords) in topic_keywords {
            let mut score: f32 = 0.0;
            for keyword in keywords {
                if content_lower.contains(keyword) {
                    score += 0.2;
                }
            }
            if score > 0.0 {
                topics.insert(topic.to_string(), score.min(1.0));
            }
        }

        Ok(topics)
    }

    
    fn calculate_importance_score_static(
        metadata: &FileMetadata,
        features: &SemanticFeatures,
    ) -> f32 {
        let mut score: f32 = 0.5; 

        
        score += (metadata.file_size as f32 / 10000.0).min(0.2);

        
        if metadata.last_modified.timestamp() > time::timestamp_seconds() - 86400 {
            score += 0.1; 
        }

        
        if features.structural.complexity_score > 0.0 {
            score += 0.15; 
        }

        if features.content.documentation_score > 0.5 {
            score += 0.1; 
        }

        score.min(1.0)
    }

    
    fn extract_conceptual_relationships_static(
        _metadata: &FileMetadata,
        _features: &SemanticFeatures,
    ) -> Result<Vec<(u32, f32)>, String> {
        
        Ok(Vec::new())
    }

    
    fn analyze_language_complexity_static(content: &str) -> Result<HashMap<String, f32>, String> {
        let mut metrics = HashMap::new();

        let words: Vec<&str> = content.split_whitespace().collect();
        let sentences: Vec<&str> = content.split(&['.', '!', '?'][..]).collect();

        
        if !sentences.is_empty() {
            metrics.insert(
                "avg_words_per_sentence".to_string(),
                words.len() as f32 / sentences.len() as f32,
            );
        }

        let unique_words: std::collections::HashSet<&str> = words.iter().cloned().collect();
        if !words.is_empty() {
            metrics.insert(
                "vocabulary_diversity".to_string(),
                unique_words.len() as f32 / words.len() as f32,
            );
        }

        
        if !words.is_empty() {
            let avg_word_length =
                words.iter().map(|w| w.len()).sum::<usize>() as f32 / words.len() as f32;
            metrics.insert("avg_word_length".to_string(), avg_word_length);
        }

        
        if !sentences.is_empty() && !words.is_empty() {
            let avg_sentence_length = words.len() as f32 / sentences.len() as f32;
            let readability = 206.835 - (1.015 * avg_sentence_length);
            metrics.insert(
                "readability_score".to_string(),
                readability.max(0.0).min(100.0),
            );
        }

        Ok(metrics)
    }

    
    fn analyze_sentiment_static(content: &str) -> Result<HashMap<String, f32>, String> {
        let mut sentiment = HashMap::new();
        let content_lower = content.to_lowercase();

        
        let positive_words = vec![
            "good",
            "great",
            "excellent",
            "amazing",
            "wonderful",
            "fantastic",
            "successful",
            "efficient",
        ];
        let negative_words = vec![
            "bad", "terrible", "awful", "horrible", "failed", "error", "problem", "issue",
        ];

        let mut positive_score = 0.0;
        let mut negative_score = 0.0;

        for word in positive_words {
            if content_lower.contains(word) {
                positive_score += 0.1;
            }
        }

        for word in negative_words {
            if content_lower.contains(word) {
                negative_score += 0.1;
            }
        }

        
        let total = positive_score + negative_score;
        if total > 0.0 {
            sentiment.insert("positive".to_string(), positive_score / total);
            sentiment.insert("negative".to_string(), negative_score / total);
        }

        
        let compound: f32 = positive_score - negative_score;
        sentiment.insert("compound".to_string(), compound.tanh()); 
        sentiment.insert(
            "neutral".to_string(),
            1.0 - (positive_score + negative_score).min(1.0),
        );

        Ok(sentiment)
    }

    
    fn extract_named_entities_static(content: &str) -> Result<Vec<String>, String> {
        let mut entities = Vec::new();

        
        let words: Vec<&str> = content.split_whitespace().collect();
        for word in words {
            if word.len() > 2 && word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                let clean_word = word.trim_matches(|c: char| !c.is_alphabetic());
                if clean_word.len() > 2 && !entities.contains(&clean_word.to_string()) {
                    entities.push(clean_word.to_string());
                }
            }
        }

        
        entities.truncate(50);
        Ok(entities)
    }

    
    fn determine_cluster_assignments_static(
        metadata: &FileMetadata,
        features: &SemanticFeatures,
    ) -> Result<Vec<String>, String> {
        let mut clusters = Vec::new();

        
        if let Some(extension) = std::path::Path::new(&metadata.file_name)
            .extension()
            .and_then(|e| e.to_str())
        {
            clusters.push(format!("filetype_{}", extension));
        }

        
        if features.structural.complexity_score > 0.0 {
            clusters.push("code".to_string());
        }

        if features.content.documentation_score > 0.8 {
            clusters.push("documentation".to_string());
        }

        
        let size = metadata.file_size;
        if size < 1000 {
            clusters.push("small_content".to_string());
        } else if size < 10000 {
            clusters.push("medium_content".to_string());
        } else {
            clusters.push("large_content".to_string());
        }

        Ok(clusters)
    }

    
    pub fn new(config: Option<SemanticProcessorConfig>) -> Self {
        let config = config.unwrap_or_default();
        let advanced_params = AdvancedParams::default();

        let semantic_analyzer = Some(SemanticAnalyzer::new(SemanticAnalyzerConfig::default()));

        let stress_solver = Some(StressMajorizationSolver::from_advanced_params(
            &advanced_params,
        ));

        info!(
            "Initializing SemanticProcessorActor with AI features: {}",
            config.enable_ai_features
        );

        Self {
            semantic_analyzer,
            constraint_set: ConstraintSet::default(),
            stress_solver,
            semantic_features_cache: HashMap::new(),
            ai_features_cache: HashMap::new(),
            advanced_params,
            config,
            stats: SemanticStats::default(),
            graph_data: None,
            _last_semantic_analysis: None,
            _constraint_cache: HashMap::new(),
            _active_tasks: HashMap::new(),
            relationship_threshold: 0.7,
            _enable_ai_processing: true,
            clustering_params: SemanticClusteringParams::default(),
            performance_metrics: HashMap::new(),
            gpu_analyzer: Some(GpuSemanticAnalyzerAdapter::new()),
        }
    }

    
    pub fn set_graph_data(&mut self, graph_data: Arc<GraphData>) {
        self.graph_data = Some(graph_data);
        info!("Updated graph data for semantic processing");
    }

    
    pub fn process_metadata(
        &mut self,
        metadata_id: &str,
        metadata: &FileMetadata,
    ) -> Result<(), String> {
        let start_time = Instant::now();

        
        if let Some(ref mut analyzer) = self.semantic_analyzer {
            let features = analyzer.analyze_metadata(metadata);
            self.semantic_features_cache
                .insert(metadata_id.to_string(), features.clone());

            
            if self.config.enable_ai_features {
                if let Ok(ai_features) = self.extract_ai_features(metadata, &features) {
                    self.ai_features_cache
                        .insert(metadata_id.to_string(), ai_features);
                    self.stats.ai_features_processed += 1;
                }
            }

            self.stats.semantic_features_cached += 1;
        }

        self.stats.last_analysis_duration = Some(start_time.elapsed());

        debug!(
            "Processed semantic metadata for {}: {:?}",
            metadata_id, self.stats.last_analysis_duration
        );
        Ok(())
    }

    
    fn extract_ai_features(
        &self,
        metadata: &FileMetadata,
        base_features: &SemanticFeatures,
    ) -> Result<AISemanticFeatures, String> {
        let mut ai_features = AISemanticFeatures::default();

        
        ai_features.content_embedding = self.generate_content_embedding(&metadata.file_name)?;

        
        ai_features.topic_classifications =
            self.classify_topics(&metadata.file_name, base_features)?;

        
        ai_features.importance_score = self.calculate_importance_score(metadata, base_features);

        
        ai_features.conceptual_links =
            self.extract_conceptual_relationships(metadata, base_features)?;

        
        ai_features.complexity_metrics = self.analyze_language_complexity(&metadata.file_name)?;

        
        if metadata.file_name.len() > 3 {
            ai_features.sentiment_analysis = Some(self.analyze_sentiment(&metadata.file_name)?);
        }

        
        ai_features.named_entities = self.extract_named_entities(&metadata.file_name)?;

        
        ai_features.cluster_assignments =
            self.determine_cluster_assignments(metadata, base_features)?;

        Ok(ai_features)
    }

    
    fn generate_content_embedding(&self, content: &str) -> Result<Vec<f32>, String> {
        
        
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut embedding = vec![0.0; 256]; 

        for (i, word) in words.iter().enumerate().take(100) {
            let hash = self.simple_hash(word) % 256;
            embedding[hash] += 1.0 / (i as f32 + 1.0);
        }

        
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for val in &mut embedding {
                *val /= magnitude;
            }
        }

        Ok(embedding)
    }

    
    fn simple_hash(&self, s: &str) -> usize {
        s.chars()
            .fold(0, |acc, c| acc.wrapping_mul(31).wrapping_add(c as usize))
    }

    
    fn classify_topics(
        &self,
        content: &str,
        _features: &SemanticFeatures,
    ) -> Result<HashMap<String, f32>, String> {
        let mut topics = HashMap::new();
        let content_lower = content.to_lowercase();

        
        let topic_keywords = vec![
            (
                "technology",
                vec![
                    "code",
                    "software",
                    "programming",
                    "algorithm",
                    "data",
                    "computer",
                ],
            ),
            (
                "science",
                vec![
                    "research",
                    "experiment",
                    "hypothesis",
                    "analysis",
                    "theory",
                    "study",
                ],
            ),
            (
                "business",
                vec![
                    "market", "strategy", "revenue", "customer", "product", "sales",
                ],
            ),
            (
                "education",
                vec!["learn", "teach", "student", "course", "knowledge", "skill"],
            ),
            (
                "health",
                vec![
                    "medical",
                    "health",
                    "treatment",
                    "patient",
                    "disease",
                    "therapy",
                ],
            ),
            (
                "art",
                vec![
                    "creative",
                    "design",
                    "visual",
                    "artistic",
                    "aesthetic",
                    "culture",
                ],
            ),
        ];

        for (topic, keywords) in topic_keywords {
            let mut score: f32 = 0.0;
            for keyword in keywords {
                if content_lower.contains(keyword) {
                    score += 0.2;
                }
            }
            if score > 0.0 {
                topics.insert(topic.to_string(), score.min(1.0));
            }
        }

        Ok(topics)
    }

    
    fn calculate_importance_score(
        &self,
        metadata: &FileMetadata,
        features: &SemanticFeatures,
    ) -> f32 {
        let mut score: f32 = 0.5; 

        
        score += (metadata.file_size as f32 / 10000.0).min(0.2);

        
        if metadata.last_modified.timestamp() > time::timestamp_seconds() - 86400 {
            score += 0.1; 
        }

        
        if features.structural.complexity_score > 0.0 {
            score += 0.15; 
        }

        if features.content.documentation_score > 0.5 {
            score += 0.1; 
        }

        score.min(1.0)
    }

    
    fn extract_conceptual_relationships(
        &self,
        _metadata: &FileMetadata,
        _features: &SemanticFeatures,
    ) -> Result<Vec<(u32, f32)>, String> {
        
        
        Ok(Vec::new())
    }

    
    fn analyze_language_complexity(&self, content: &str) -> Result<HashMap<String, f32>, String> {
        let mut metrics = HashMap::new();

        let words: Vec<&str> = content.split_whitespace().collect();
        let sentences: Vec<&str> = content.split(&['.', '!', '?'][..]).collect();

        
        if !sentences.is_empty() {
            metrics.insert(
                "avg_words_per_sentence".to_string(),
                words.len() as f32 / sentences.len() as f32,
            );
        }

        let unique_words: std::collections::HashSet<&str> = words.iter().cloned().collect();
        if !words.is_empty() {
            metrics.insert(
                "vocabulary_diversity".to_string(),
                unique_words.len() as f32 / words.len() as f32,
            );
        }

        
        if !words.is_empty() {
            let avg_word_length =
                words.iter().map(|w| w.len()).sum::<usize>() as f32 / words.len() as f32;
            metrics.insert("avg_word_length".to_string(), avg_word_length);
        }

        
        if !sentences.is_empty() && !words.is_empty() {
            let avg_sentence_length = words.len() as f32 / sentences.len() as f32;
            let readability = 206.835 - (1.015 * avg_sentence_length);
            metrics.insert(
                "readability_score".to_string(),
                readability.max(0.0).min(100.0),
            );
        }

        Ok(metrics)
    }

    
    fn analyze_sentiment(&self, content: &str) -> Result<HashMap<String, f32>, String> {
        let mut sentiment = HashMap::new();
        let content_lower = content.to_lowercase();

        
        let positive_words = vec![
            "good",
            "great",
            "excellent",
            "amazing",
            "wonderful",
            "fantastic",
            "successful",
            "efficient",
        ];
        let negative_words = vec![
            "bad", "terrible", "awful", "horrible", "failed", "error", "problem", "issue",
        ];

        let mut positive_score = 0.0;
        let mut negative_score = 0.0;

        for word in positive_words {
            if content_lower.contains(word) {
                positive_score += 0.1;
            }
        }

        for word in negative_words {
            if content_lower.contains(word) {
                negative_score += 0.1;
            }
        }

        
        let total = positive_score + negative_score;
        if total > 0.0 {
            sentiment.insert("positive".to_string(), positive_score / total);
            sentiment.insert("negative".to_string(), negative_score / total);
        }

        
        let compound: f32 = positive_score - negative_score;
        sentiment.insert("compound".to_string(), compound.tanh()); 
        sentiment.insert(
            "neutral".to_string(),
            1.0 - (positive_score + negative_score).min(1.0),
        );

        Ok(sentiment)
    }

    
    fn extract_named_entities(&self, content: &str) -> Result<Vec<String>, String> {
        let mut entities = Vec::new();

        
        

        
        let words: Vec<&str> = content.split_whitespace().collect();
        for word in words {
            if word.len() > 2 && word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                let clean_word = word.trim_matches(|c: char| !c.is_alphabetic());
                if clean_word.len() > 2 && !entities.contains(&clean_word.to_string()) {
                    entities.push(clean_word.to_string());
                }
            }
        }

        
        entities.truncate(50);
        Ok(entities)
    }

    
    fn determine_cluster_assignments(
        &self,
        metadata: &FileMetadata,
        features: &SemanticFeatures,
    ) -> Result<Vec<String>, String> {
        let mut clusters = Vec::new();

        
        if let Some(extension) = std::path::Path::new(&metadata.file_name)
            .extension()
            .and_then(|e| e.to_str())
        {
            clusters.push(format!("filetype_{}", extension));
        }

        
        if features.structural.complexity_score > 0.0 {
            clusters.push("code".to_string());
        }

        if features.content.documentation_score > 0.8 {
            clusters.push("documentation".to_string());
        }

        
        let size = metadata.file_size;
        if size < 1000 {
            clusters.push("small_content".to_string());
        } else if size < 10000 {
            clusters.push("medium_content".to_string());
        } else {
            clusters.push("large_content".to_string());
        }

        Ok(clusters)
    }

    
    pub fn generate_semantic_constraints(&mut self) -> Result<Vec<Constraint>, String> {
        let start_time = Instant::now();
        let graph_data = match &self.graph_data {
            Some(data) => data,
            None => return Err("No graph data available for constraint generation".to_string()),
        };

        let mut constraints = Vec::new();

        
        constraints.extend(self.generate_similarity_constraints(&graph_data)?);
        constraints.extend(self.generate_clustering_constraints(&graph_data)?);
        constraints.extend(self.generate_importance_constraints(&graph_data)?);
        constraints.extend(self.generate_topic_constraints(&graph_data)?);

        
        constraints.truncate(self.config.max_constraints_per_cycle);

        self.stats.constraints_generated = constraints.len();
        self.stats.last_analysis_duration = Some(start_time.elapsed());

        info!(
            "Generated {} semantic constraints in {:?}",
            constraints.len(),
            self.stats.last_analysis_duration
        );

        Ok(constraints)
    }

    
    fn generate_similarity_constraints(
        &self,
        graph_data: &GraphData,
    ) -> Result<Vec<Constraint>, String> {
        let mut constraints = Vec::new();

        for node_pair in self.get_node_pairs(&graph_data.nodes) {
            let (node1, node2) = node_pair;

            if let (Some(features1), Some(features2)) = (
                self.get_node_semantic_features(node1.id),
                self.get_node_semantic_features(node2.id),
            ) {
                let similarity = self.calculate_semantic_similarity(features1, features2);

                if similarity > self.config.similarity_threshold {
                    let _attraction_strength = similarity * 0.5;
                    let constraint = Constraint::separation(
                        node1.id, node2.id, 100.0, 
                    );
                    constraints.push(constraint);
                }
            }
        }

        Ok(constraints)
    }

    
    fn generate_clustering_constraints(
        &self,
        graph_data: &GraphData,
    ) -> Result<Vec<Constraint>, String> {
        let mut constraints = Vec::new();
        let clusters = self.identify_semantic_clusters(&graph_data.nodes)?;

        for (cluster_id, node_ids) in clusters {
            if node_ids.len() >= self.clustering_params.min_cluster_size {
                
                let centroid_strength = 0.3;
                let cluster_constraint =
                    Constraint::cluster(node_ids.clone(), cluster_id as f32, centroid_strength);
                constraints.push(cluster_constraint);

                
                for other_node in &graph_data.nodes {
                    if !node_ids.contains(&other_node.id) {
                        let repulsion_constraint = Constraint::separation(
                            node_ids[0], 
                            other_node.id,
                            200.0, 
                        );
                        constraints.push(repulsion_constraint);
                    }
                }
            }
        }

        Ok(constraints)
    }

    
    fn generate_importance_constraints(
        &self,
        graph_data: &GraphData,
    ) -> Result<Vec<Constraint>, String> {
        let mut constraints = Vec::new();

        for node in &graph_data.nodes {
            if let Some(ai_features) = self.ai_features_cache.get(&node.id.to_string()) {
                if ai_features.importance_score > 0.8 {
                    
                    let central_constraint = Constraint::fixed_position(
                        node.id, 0.0, 
                        0.0, 
                        0.0, 
                    );
                    constraints.push(central_constraint);
                }
            }
        }

        Ok(constraints)
    }

    
    fn generate_topic_constraints(
        &self,
        graph_data: &GraphData,
    ) -> Result<Vec<Constraint>, String> {
        let mut constraints = Vec::new();
        let mut topic_groups: HashMap<String, Vec<u32>> = HashMap::new();

        
        for node in &graph_data.nodes {
            if let Some(ai_features) = self.ai_features_cache.get(&node.id.to_string()) {
                if let Some((topic, confidence)) = ai_features
                    .topic_classifications
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                {
                    if *confidence > 0.5 {
                        topic_groups.entry(topic.clone()).or_default().push(node.id);
                    }
                }
            }
        }

        
        for (topic, node_ids) in topic_groups {
            if node_ids.len() > 1 {
                let cluster_constraint = Constraint::cluster(
                    node_ids,
                    self.simple_hash(&topic) as f32,
                    0.4, 
                );
                constraints.push(cluster_constraint);
            }
        }

        Ok(constraints)
    }

    
    fn get_node_semantic_features(&self, node_id: u32) -> Option<&SemanticFeatures> {
        self.semantic_features_cache.get(&node_id.to_string())
    }

    
    fn calculate_semantic_similarity(
        &self,
        features1: &SemanticFeatures,
        features2: &SemanticFeatures,
    ) -> f32 {
        let mut similarity = 0.0;
        let mut comparisons = 0;

        
        let _struct_sim = if features1.structural.complexity_score > 0.0
            || features2.structural.complexity_score > 0.0
        {
            let max_complexity = features1
                .structural
                .complexity_score
                .max(features2.structural.complexity_score);
            let min_complexity = features1
                .structural
                .complexity_score
                .min(features2.structural.complexity_score);
            if max_complexity > 0.0 {
                similarity += min_complexity / max_complexity;
                comparisons += 1;
            }
        };

        
        let _content_sim = if features1.content.documentation_score > 0.0
            || features2.content.documentation_score > 0.0
        {
            let max_doc_score = features1
                .content
                .documentation_score
                .max(features2.content.documentation_score);
            let min_doc_score = features1
                .content
                .documentation_score
                .min(features2.content.documentation_score);
            if max_doc_score > 0.0 {
                similarity += min_doc_score / max_doc_score;
                comparisons += 1;
            }
        };

        if comparisons > 0 {
            similarity / comparisons as f32
        } else {
            0.0
        }
    }

    
    fn get_node_pairs<'a>(&self, nodes: &'a [Node]) -> Vec<(&'a Node, &'a Node)> {
        let mut pairs = Vec::new();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                pairs.push((&nodes[i], &nodes[j]));

                
                if pairs.len() >= 1000 {
                    break;
                }
            }
            if pairs.len() >= 1000 {
                break;
            }
        }

        pairs
    }

    
    fn identify_semantic_clusters(
        &self,
        nodes: &[Node],
    ) -> Result<HashMap<usize, Vec<u32>>, String> {
        let mut clusters = HashMap::new();

        
        for node in nodes {
            if let Some(ai_features) = self.ai_features_cache.get(&node.id.to_string()) {
                for cluster in &ai_features.cluster_assignments {
                    let cluster_id = self.simple_hash(cluster) % 100; 
                    clusters
                        .entry(cluster_id)
                        .or_insert_with(Vec::new)
                        .push(node.id);
                }
            }
        }

        Ok(clusters)
    }

    
    pub fn execute_stress_optimization(&mut self) -> Result<OptimizationResult, String> {
        let graph_data = match &self.graph_data {
            Some(data) => data,
            None => return Err("No graph data available for stress optimization".to_string()),
        };

        let solver = match &mut self.stress_solver {
            Some(solver) => solver,
            None => return Err("Stress solver not initialized".to_string()),
        };

        let start_time = Instant::now();
        let mut graph_clone = graph_data.as_ref().clone();

        let result = solver
            .optimize(&mut graph_clone, &self.constraint_set)
            .map_err(|e| format!("Stress optimization failed: {:?}", e))?;

        self.stats.stress_iterations = result.iterations;
        self.stats.stress_final_value = result.final_stress;

        let duration = start_time.elapsed();
        self.performance_metrics.insert(
            "stress_optimization_ms".to_string(),
            duration.as_millis() as f32,
        );

        info!(
            "Completed stress optimization: {} iterations, final stress: {:.6}, duration: {:?}",
            result.iterations, result.final_stress, duration
        );

        Ok(result)
    }

    
    pub fn handle_constraint_update(&mut self, constraint_data: Value) -> Result<(), String> {
        let constraint_type = constraint_data
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match constraint_type {
            "semantic_similarity" => {
                if let (Some(threshold), Some(enabled)) = (
                    constraint_data.get("threshold").and_then(|v| v.as_f64()),
                    constraint_data.get("enabled").and_then(|v| v.as_bool()),
                ) {
                    self.config.similarity_threshold = threshold as f32;
                    if enabled {
                        self.regenerate_similarity_constraints()?;
                    }
                    info!(
                        "Updated semantic similarity constraints: threshold={}, enabled={}",
                        threshold, enabled
                    );
                }
            }
            "clustering" => {
                if let Some(enabled) = constraint_data.get("enabled").and_then(|v| v.as_bool()) {
                    self.constraint_set
                        .set_group_active("semantic_clustering", enabled);
                    info!("Toggled semantic clustering constraints: {}", enabled);
                }
            }
            "importance_weighting" => {
                if let Some(enabled) = constraint_data.get("enabled").and_then(|v| v.as_bool()) {
                    self.constraint_set
                        .set_group_active("importance_based", enabled);
                    info!("Toggled importance-based constraints: {}", enabled);
                }
            }
            _ => {
                warn!("Unknown constraint type: {}", constraint_type);
                return Err(format!("Unknown constraint type: {}", constraint_type));
            }
        }

        Ok(())
    }

    
    fn regenerate_similarity_constraints(&mut self) -> Result<(), String> {
        let graph_data = match &self.graph_data {
            Some(data) => data,
            None => return Err("No graph data available".to_string()),
        };

        
        self.constraint_set
            .set_group_active("semantic_similarity", false);

        
        let constraints = self.generate_similarity_constraints(graph_data)?;
        for constraint in constraints {
            self.constraint_set
                .add_to_group("semantic_similarity", constraint);
        }

        info!("Regenerated semantic similarity constraints");

        Ok(())
    }

    
    pub fn get_stats(&self) -> &SemanticStats {
        &self.stats
    }

    
    pub fn get_performance_metrics(&self) -> &HashMap<String, f32> {
        &self.performance_metrics
    }

    
    pub fn update_config(&mut self, new_config: SemanticProcessorConfig) {
        self.config = new_config;
        info!("Updated semantic processor configuration");
    }
}

impl Actor for SemanticProcessorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!(
            "SemanticProcessorActor started with AI features: {}",
            self.config.enable_ai_features
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "SemanticProcessorActor stopped. Final stats: {:?}",
            self.stats
        );
    }
}

// Message Handlers

impl Handler<UpdateConstraints> for SemanticProcessorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        debug!("Handling constraint update: {:?}", msg.constraint_data);
        self.handle_constraint_update(msg.constraint_data)
    }
}

impl Handler<GetConstraints> for SemanticProcessorActor {
    type Result = Result<ConstraintSet, String>;

    fn handle(&mut self, _msg: GetConstraints, _ctx: &mut Self::Context) -> Self::Result {
        debug!(
            "Returning constraint set with {} constraints",
            self.constraint_set.constraints.len()
        );
        Ok(self.constraint_set.clone())
    }
}

impl Handler<TriggerStressMajorization> for SemanticProcessorActor {
    type Result = actix::ResponseFuture<Result<(), String>>;

    fn handle(
        &mut self,
        _msg: TriggerStressMajorization,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("Triggering stress majorization optimization");

        let graph_data = self.graph_data.clone();
        let constraint_set = self.constraint_set.clone();
        let stress_solver = self.stress_solver.clone();

        
        let fut = web::block(move || {
            Self::execute_stress_optimization_blocking(graph_data, constraint_set, stress_solver)
        })
        .map(|result| match result {
            Ok(Ok(optimization_result)) => {
                info!(
                    "Stress optimization completed: converged={}, final_stress={:.6}",
                    optimization_result.converged, optimization_result.final_stress
                );
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Stress optimization failed: {}", e);
                Err(e)
            }
            Err(e) => Err(format!("Thread pool error: {}", e)),
        });

        Box::pin(fut)
    }
}

impl Handler<RegenerateSemanticConstraints> for SemanticProcessorActor {
    type Result = actix::ResponseFuture<Result<(), String>>;

    fn handle(
        &mut self,
        _msg: RegenerateSemanticConstraints,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("Regenerating semantic constraints");

        
        self.constraint_set
            .set_group_active("semantic_similarity", false);
        self.constraint_set
            .set_group_active("semantic_clustering", false);
        self.constraint_set
            .set_group_active("importance_based", false);
        self.constraint_set.set_group_active("topic_based", false);

        let graph_data = self.graph_data.clone();
        let semantic_features_cache = self.semantic_features_cache.clone();
        let ai_features_cache = self.ai_features_cache.clone();
        let config = self.config.clone();

        
        let fut = web::block(move || {
            Self::generate_semantic_constraints_blocking(
                graph_data,
                semantic_features_cache,
                ai_features_cache,
                config,
            )
        })
        .map(move |result| {
            match result {
                Ok(Ok(constraints)) => {
                    
                    
                    info!(
                        "Generated {} semantic constraints in thread pool",
                        constraints.len()
                    );
                    Ok(())
                }
                Ok(Err(e)) => {
                    error!("Failed to regenerate semantic constraints: {}", e);
                    Err(e)
                }
                Err(e) => Err(format!("Thread pool error: {}", e)),
            }
        });

        Box::pin(fut)
    }
}

impl Handler<UpdateAdvancedParams> for SemanticProcessorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateAdvancedParams, _ctx: &mut Self::Context) -> Self::Result {
        info!("Updating advanced parameters for semantic processing");

        self.advanced_params = msg.params.clone();

        
        self.stress_solver = Some(StressMajorizationSolver::from_advanced_params(&msg.params));

        
        self.relationship_threshold = msg.params.semantic_force_weight * 0.1;

        info!(
            "Updated semantic processor with advanced parameters - semantic_force_weight: {}",
            msg.params.semantic_force_weight
        );

        Ok(())
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetGraphData {
    pub graph_data: Arc<GraphData>,
}

impl Handler<SetGraphData> for SemanticProcessorActor {
    type Result = ();

    fn handle(&mut self, msg: SetGraphData, _ctx: &mut Self::Context) -> Self::Result {
        info!("Setting graph data for semantic processing");
        self.set_graph_data(msg.graph_data);
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ProcessMetadata {
    pub metadata_id: String,
    pub metadata: FileMetadata,
}

impl Handler<ProcessMetadata> for SemanticProcessorActor {
    type Result = actix::ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: ProcessMetadata, _ctx: &mut Self::Context) -> Self::Result {
        debug!(
            "Processing metadata for semantic analysis: {}",
            msg.metadata_id
        );

        let metadata_id = msg.metadata_id.clone();
        let metadata = msg.metadata.clone();
        let semantic_analyzer = self.semantic_analyzer.clone();
        let config = self.config.clone();

        
        let fut = web::block(move || {
            Self::process_metadata_blocking(&metadata_id, &metadata, semantic_analyzer, config)
        })
        .map(|result| match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("Thread pool error: {}", e)),
        });

        Box::pin(fut)
    }
}

#[derive(Message)]
#[rtype(result = "SemanticStats")]
pub struct GetSemanticStats;

impl Handler<GetSemanticStats> for SemanticProcessorActor {
    type Result = SemanticStats;

    fn handle(&mut self, _msg: GetSemanticStats, _ctx: &mut Self::Context) -> Self::Result {
        self.stats.clone()
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateSemanticConfig {
    pub config: SemanticProcessorConfig,
}

impl Handler<UpdateSemanticConfig> for SemanticProcessorActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateSemanticConfig, _ctx: &mut Self::Context) -> Self::Result {
        info!("Updating semantic processor configuration");
        self.update_config(msg.config);
    }
}

impl Handler<ComputeShortestPaths> for SemanticProcessorActor {
    type Result = actix::ResponseFuture<Result<PathfindingResult, String>>;

    fn handle(&mut self, msg: ComputeShortestPaths, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Computing shortest paths from node {} using GPU",
            msg.source_node_id
        );

        let graph_data = self.graph_data.clone();
        let mut gpu_analyzer = match self.gpu_analyzer.take() {
            Some(analyzer) => analyzer,
            None => {
                return Box::pin(async move { Err("GPU analyzer not available".to_string()) });
            }
        };

        Box::pin(async move {
            
            if let Some(graph) = graph_data {
                if let Err(e) = gpu_analyzer.initialize(graph).await {
                    return Err(format!("Failed to initialize GPU analyzer: {:?}", e));
                }
            } else {
                return Err("No graph data available for pathfinding".to_string());
            }

            
            match gpu_analyzer
                .compute_shortest_paths(msg.source_node_id)
                .await
            {
                Ok(result) => {
                    info!(
                        "GPU SSSP completed: {} reachable nodes in {:.2}ms",
                        result.distances.len(),
                        result.computation_time_ms
                    );
                    Ok(result)
                }
                Err(e) => {
                    error!("GPU SSSP failed: {:?}", e);
                    Err(format!("Pathfinding failed: {:?}", e))
                }
            }
        })
    }
}

impl Handler<ComputeAllPairsShortestPaths> for SemanticProcessorActor {
    type Result = actix::ResponseFuture<Result<HashMap<(u32, u32), Vec<u32>>, String>>;

    fn handle(
        &mut self,
        _msg: ComputeAllPairsShortestPaths,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("Computing all-pairs shortest paths using GPU landmark approximation");

        let graph_data = self.graph_data.clone();
        let mut gpu_analyzer = match self.gpu_analyzer.take() {
            Some(analyzer) => analyzer,
            None => {
                return Box::pin(async move { Err("GPU analyzer not available".to_string()) });
            }
        };

        Box::pin(async move {
            
            if let Some(graph) = graph_data {
                if let Err(e) = gpu_analyzer.initialize(graph).await {
                    return Err(format!("Failed to initialize GPU analyzer: {:?}", e));
                }
            } else {
                return Err("No graph data available for pathfinding".to_string());
            }

            
            match gpu_analyzer.compute_all_pairs_shortest_paths().await {
                Ok(paths) => {
                    info!(
                        "GPU landmark APSP completed: {} path pairs computed",
                        paths.len()
                    );
                    Ok(paths)
                }
                Err(e) => {
                    error!("GPU APSP failed: {:?}", e);
                    Err(format!("APSP failed: {:?}", e))
                }
            }
        })
    }
}
