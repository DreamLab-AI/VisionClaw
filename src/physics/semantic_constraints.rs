//! Semantic constraint generator for knowledge graph layout optimization
//!
//! This module generates constraints based on semantic relationships, topic similarity,
//! hierarchical structures, and domain knowledge to create meaningful spatial arrangements
//! in knowledge graphs. The generator analyzes graph content and metadata to automatically
//! create constraints that improve visual coherence and understanding.
//!
//! ## Features
//!
//! - **Semantic Clustering**: Groups related nodes based on topic similarity and content analysis
//! - **Hierarchical Alignment**: Creates alignment constraints for hierarchical relationships
//! - **Separation Constraints**: Ensures weakly related nodes maintain appropriate distances
//! - **Dynamic Generation**: Adapts constraints based on graph properties and user interaction
//! - **Multi-modal Analysis**: Combines textual content, metadata, and structural information
//!
//! ## Constraint Types Generated
//!
//! - Clustering constraints for semantically similar nodes
//! - Separation constraints for unrelated or conflicting topics
//! - Alignment constraints for hierarchical relationships
//! - Boundary constraints for domain separation
//! - Fixed position constraints for important anchor nodes

use log::{debug, info, trace, warn};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::lsh::LshIndex;
use crate::models::{
    constraints::{AdvancedParams, Constraint, ConstraintSet},
    graph::GraphData,
    metadata::MetadataStore,
    node::Node,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConstraintConfig {
    pub clustering_threshold: f32,

    pub max_cluster_size: usize,

    pub min_separation_distance: f32,

    pub enable_hierarchy: bool,

    pub enable_topic_clustering: bool,

    pub enable_temporal_clustering: bool,

    pub semantic_weight: f32,

    pub structural_weight: f32,

    pub max_topics_per_node: usize,

    pub min_topic_count: usize,
}

impl Default for SemanticConstraintConfig {
    fn default() -> Self {
        Self {
            clustering_threshold: 0.6,
            max_cluster_size: 20,
            min_separation_distance: 150.0,
            enable_hierarchy: true,
            enable_topic_clustering: true,
            enable_temporal_clustering: false,
            semantic_weight: 0.7,
            structural_weight: 0.3,
            max_topics_per_node: 5,
            min_topic_count: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeSimilarity {
    pub semantic_similarity: f32,

    pub structural_similarity: f32,

    pub combined_similarity: f32,

    pub shared_topics: Vec<String>,

    pub metadata_factors: HashMap<String, f32>,
}

#[derive(Debug, Clone)]
pub struct SemanticCluster {
    pub id: String,

    pub node_ids: HashSet<u32>,

    pub primary_topics: Vec<String>,

    pub coherence: f32,

    pub centroid: Option<(f32, f32, f32)>,

    pub radius: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct HierarchicalRelation {
    pub parent_id: u32,

    pub child_id: u32,

    pub relation_type: String,

    pub strength: f32,
}

#[derive(Debug, Clone)]
pub struct ConstraintGenerationResult {
    pub clustering_constraints: Vec<Constraint>,

    pub separation_constraints: Vec<Constraint>,

    pub alignment_constraints: Vec<Constraint>,

    pub boundary_constraints: Vec<Constraint>,

    pub clusters: Vec<SemanticCluster>,

    pub hierarchical_relations: Vec<HierarchicalRelation>,

    pub stats: GenerationStats,
}

#[derive(Debug, Clone)]
pub struct GenerationStats {
    pub nodes_processed: usize,

    pub similarity_calculations: usize,

    pub clusters_created: usize,

    pub processing_time: u64,

    pub avg_cluster_coherence: f32,
}

pub struct SemanticConstraintGenerator {
    config: SemanticConstraintConfig,
    similarity_cache: HashMap<(u32, u32), NodeSimilarity>,
    topic_embeddings: HashMap<String, Vec<f32>>,
}

impl SemanticConstraintGenerator {
    pub fn new() -> Self {
        Self::with_config(SemanticConstraintConfig::default())
    }

    pub fn with_config(config: SemanticConstraintConfig) -> Self {
        Self {
            config,
            similarity_cache: HashMap::new(),
            topic_embeddings: HashMap::new(),
        }
    }

    pub fn from_advanced_params(params: &AdvancedParams) -> Self {
        let config = SemanticConstraintConfig {
            semantic_weight: params.semantic_force_weight,
            structural_weight: params.structural_force_weight,
            clustering_threshold: 0.5 + params.knowledge_force_weight * 0.3,
            min_separation_distance: params.target_edge_length * params.separation_factor,
            enable_hierarchy: params.hierarchical_mode,
            ..Default::default()
        };

        Self::with_config(config)
    }

    pub fn generate_constraints(
        &mut self,
        graph_data: &GraphData,
        metadata_store: Option<&MetadataStore>,
    ) -> Result<ConstraintGenerationResult, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        info!(
            "Generating semantic constraints for {} nodes",
            graph_data.nodes.len()
        );

        self.similarity_cache.clear();

        let similarities = self.compute_node_similarities(graph_data, metadata_store)?;
        debug!("Computed {} node similarity pairs", similarities.len());

        let clusters = self.identify_semantic_clusters(graph_data, &similarities)?;
        info!("Identified {} semantic clusters", clusters.len());

        let hierarchical_relations = if self.config.enable_hierarchy {
            self.identify_hierarchical_relations(graph_data, metadata_store)?
        } else {
            Vec::new()
        };

        let clustering_constraints = self.generate_clustering_constraints(&clusters)?;

        let separation_constraints =
            self.generate_separation_constraints(graph_data, &similarities)?;

        let alignment_constraints = self.generate_alignment_constraints(&hierarchical_relations)?;

        let boundary_constraints = self.generate_boundary_constraints(&clusters)?;

        let processing_time = start_time.elapsed().as_millis() as u64;

        let stats = GenerationStats {
            nodes_processed: graph_data.nodes.len(),
            similarity_calculations: similarities.len(),
            clusters_created: clusters.len(),
            processing_time,
            avg_cluster_coherence: clusters.iter().map(|c| c.coherence).sum::<f32>()
                / clusters.len().max(1) as f32,
        };

        let total_constraints = clustering_constraints.len()
            + separation_constraints.len()
            + alignment_constraints.len()
            + boundary_constraints.len();

        info!(
            "Generated {} semantic constraints in {}ms",
            total_constraints, processing_time
        );

        Ok(ConstraintGenerationResult {
            clustering_constraints,
            separation_constraints,
            alignment_constraints,
            boundary_constraints,
            clusters,
            hierarchical_relations,
            stats,
        })
    }

    fn compute_node_similarities(
        &mut self,
        graph_data: &GraphData,
        metadata_store: Option<&MetadataStore>,
    ) -> Result<HashMap<(u32, u32), NodeSimilarity>, Box<dyn std::error::Error>> {
        let mut similarities = HashMap::new();
        let nodes = &graph_data.nodes;

        // For small graphs, exhaustive pairwise is cheaper than building the LSH index.
        const LSH_THRESHOLD: usize = 500;

        let node_pairs: Vec<(usize, usize)> = if nodes.len() <= LSH_THRESHOLD {
            // Small graph: exhaustive O(n^2) pairwise.
            debug!(
                "Using exhaustive pairwise similarity for {} nodes (below LSH threshold {})",
                nodes.len(),
                LSH_THRESHOLD
            );
            (0..nodes.len())
                .flat_map(|i| (i + 1..nodes.len()).map(move |j| (i, j)))
                .collect()
        } else {
            // Large graph: build LSH index and only evaluate candidate pairs.
            info!("Using LSH candidate generation for {} nodes", nodes.len());

            let lsh_index = LshIndex::build_from_nodes(nodes, metadata_store);
            let candidate_pairs = lsh_index.all_candidate_pairs();

            info!(
                "LSH produced {} candidate pairs (vs {} exhaustive)",
                candidate_pairs.len(),
                (nodes.len() as u64) * (nodes.len() as u64 - 1) / 2
            );

            // Build id -> index map for efficient lookup.
            let id_to_idx: HashMap<u32, usize> = nodes
                .iter()
                .enumerate()
                .map(|(idx, node)| (node.id, idx))
                .collect();

            candidate_pairs
                .into_iter()
                .filter_map(|(id_a, id_b)| {
                    let idx_a = id_to_idx.get(&id_a)?;
                    let idx_b = id_to_idx.get(&id_b)?;
                    Some((*idx_a.min(idx_b), *idx_a.max(idx_b)))
                })
                .collect()
        };

        debug!("Computing full similarity for {} pairs", node_pairs.len());

        let computed_similarities: Vec<_> = node_pairs
            .par_iter()
            .map(|&(i, j)| {
                let node_a = &nodes[i];
                let node_b = &nodes[j];
                let similarity = self.compute_similarity_pair(node_a, node_b, metadata_store);
                ((node_a.id, node_b.id), similarity)
            })
            .collect();

        for ((id_a, id_b), similarity) in computed_similarities {
            similarities.insert((id_a.min(id_b), id_a.max(id_b)), similarity);
        }

        Ok(similarities)
    }

    fn compute_similarity_pair(
        &self,
        node_a: &Node,
        node_b: &Node,
        metadata_store: Option<&MetadataStore>,
    ) -> NodeSimilarity {
        let mut semantic_sim = 0.0;

        let mut shared_topics = Vec::new();
        let mut metadata_factors = HashMap::new();

        if let Some(store) = metadata_store {
            let meta_a = store.get(&node_a.metadata_id);
            let meta_b = store.get(&node_b.metadata_id);

            if meta_a.is_none() {
                warn!(
                    "No metadata for node {} (metadata_id: {}), using defaults",
                    node_a.id, node_a.metadata_id
                );
            }
            if meta_b.is_none() {
                warn!(
                    "No metadata for node {} (metadata_id: {}), using defaults",
                    node_b.id, node_b.metadata_id
                );
            }

            if let (Some(meta_a), Some(meta_b)) = (meta_a, meta_b) {
                semantic_sim =
                    self.compute_topic_similarity(&meta_a.topic_counts, &meta_b.topic_counts);
                shared_topics = self.find_shared_topics(&meta_a.topic_counts, &meta_b.topic_counts);

                let size_diff = (meta_a.file_size as f32 - meta_b.file_size as f32).abs();
                let size_sim = 1.0 / (1.0 + size_diff / 1000.0);
                metadata_factors.insert("file_size".to_string(), size_sim);

                if let (Some(time_a), Some(time_b)) =
                    (&meta_a.last_content_change, &meta_b.last_content_change)
                {
                    let time_diff = (time_a.timestamp() - time_b.timestamp()).abs() as f32;
                    let time_sim = 1.0 / (1.0 + time_diff / 86400.0);
                    metadata_factors.insert("temporal".to_string(), time_sim);
                }
            }
        }

        let structural_sim = self.compute_structural_similarity(node_a, node_b);

        let name_sim = self.compute_string_similarity(&node_a.label, &node_b.label);
        metadata_factors.insert("name".to_string(), name_sim);

        let combined_similarity = self.config.semantic_weight * semantic_sim
            + self.config.structural_weight * structural_sim;

        NodeSimilarity {
            semantic_similarity: semantic_sim,
            structural_similarity: structural_sim,
            combined_similarity,
            shared_topics,
            metadata_factors,
        }
    }

    fn compute_topic_similarity(
        &self,
        topics_a: &HashMap<String, usize>,
        topics_b: &HashMap<String, usize>,
    ) -> f32 {
        if topics_a.is_empty() || topics_b.is_empty() {
            return 0.0;
        }

        let all_topics: HashSet<_> = topics_a.keys().chain(topics_b.keys()).collect();

        let mut vec_a = Vec::new();
        let mut vec_b = Vec::new();

        for topic in &all_topics {
            vec_a.push(*topics_a.get(*topic).unwrap_or(&0) as f32);
            vec_b.push(*topics_b.get(*topic).unwrap_or(&0) as f32);
        }

        self.cosine_similarity(&vec_a, &vec_b)
    }

    fn find_shared_topics(
        &self,
        topics_a: &HashMap<String, usize>,
        topics_b: &HashMap<String, usize>,
    ) -> Vec<String> {
        topics_a
            .keys()
            .filter(|topic| topics_b.contains_key(*topic))
            .filter(|topic| {
                topics_a[*topic] >= self.config.min_topic_count
                    && topics_b[*topic] >= self.config.min_topic_count
            })
            .cloned()
            .collect()
    }

    fn compute_structural_similarity(&self, node_a: &Node, node_b: &Node) -> f32 {
        let pos_a = (node_a.data.x, node_a.data.y, node_a.data.z);
        let pos_b = (node_b.data.x, node_b.data.y, node_b.data.z);

        let distance = ((pos_a.0 - pos_b.0).powi(2)
            + (pos_a.1 - pos_b.1).powi(2)
            + (pos_a.2 - pos_b.2).powi(2))
        .sqrt();

        1.0 / (1.0 + distance / 100.0)
    }

    fn compute_string_similarity(&self, str_a: &str, str_b: &str) -> f32 {
        if str_a.is_empty() || str_b.is_empty() {
            return if str_a == str_b { 1.0 } else { 0.0 };
        }

        let ngrams_a: HashSet<_> = str_a
            .chars()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[0], w[1]))
            .collect();

        let ngrams_b: HashSet<_> = str_b
            .chars()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[0], w[1]))
            .collect();

        let intersection = ngrams_a.intersection(&ngrams_b).count();
        let union = ngrams_a.union(&ngrams_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    fn cosine_similarity(&self, vec_a: &[f32], vec_b: &[f32]) -> f32 {
        if vec_a.len() != vec_b.len() || vec_a.is_empty() {
            return 0.0;
        }

        let dot_product: f32 = vec_a.iter().zip(vec_b.iter()).map(|(a, b)| a * b).sum();
        let norm_a: f32 = vec_a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = vec_b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a > 0.0 && norm_b > 0.0 {
            (dot_product / (norm_a * norm_b)).max(0.0).min(1.0)
        } else {
            0.0
        }
    }

    fn identify_semantic_clusters(
        &self,
        _graph_data: &GraphData,
        similarities: &HashMap<(u32, u32), NodeSimilarity>,
    ) -> Result<Vec<SemanticCluster>, Box<dyn std::error::Error>> {
        let mut clusters = Vec::new();
        let mut processed_nodes = HashSet::new();

        let mut sorted_pairs: Vec<_> = similarities.iter().collect();
        sorted_pairs.sort_by(|a, b| {
            b.1.combined_similarity
                .partial_cmp(&a.1.combined_similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for ((id_a, id_b), similarity) in sorted_pairs {
            if similarity.combined_similarity < self.config.clustering_threshold {
                break;
            }

            if processed_nodes.contains(id_a) || processed_nodes.contains(id_b) {
                continue;
            }

            let mut cluster_nodes = HashSet::new();
            cluster_nodes.insert(*id_a);
            cluster_nodes.insert(*id_b);

            self.expand_cluster(&mut cluster_nodes, similarities, &processed_nodes)?;

            if cluster_nodes.len() <= self.config.max_cluster_size {
                let primary_topics = self.compute_cluster_topics(&cluster_nodes, similarities);
                let coherence = self.compute_cluster_coherence(&cluster_nodes, similarities);

                let cluster = SemanticCluster {
                    id: format!("cluster_{}", clusters.len()),
                    node_ids: cluster_nodes.clone(),
                    primary_topics,
                    coherence,
                    centroid: None,
                    radius: None,
                };

                let cluster_size = cluster_nodes.len();
                clusters.push(cluster);
                processed_nodes.extend(cluster_nodes);

                debug!(
                    "Created cluster with {} nodes, coherence: {:.3}",
                    cluster_size, coherence
                );
            }
        }

        Ok(clusters)
    }

    fn expand_cluster(
        &self,
        cluster_nodes: &mut HashSet<u32>,
        similarities: &HashMap<(u32, u32), NodeSimilarity>,
        processed_nodes: &HashSet<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut added_nodes = true;

        while added_nodes && cluster_nodes.len() < self.config.max_cluster_size {
            added_nodes = false;

            for &cluster_node in cluster_nodes.clone().iter() {
                for ((id_a, id_b), similarity) in similarities {
                    if similarity.combined_similarity < self.config.clustering_threshold {
                        continue;
                    }

                    let candidate = if *id_a == cluster_node
                        && !cluster_nodes.contains(id_b)
                        && !processed_nodes.contains(id_b)
                    {
                        Some(*id_b)
                    } else if *id_b == cluster_node
                        && !cluster_nodes.contains(id_a)
                        && !processed_nodes.contains(id_a)
                    {
                        Some(*id_a)
                    } else {
                        None
                    };

                    if let Some(candidate_id) = candidate {
                        cluster_nodes.insert(candidate_id);
                        added_nodes = true;

                        if cluster_nodes.len() >= self.config.max_cluster_size {
                            break;
                        }
                    }
                }

                if cluster_nodes.len() >= self.config.max_cluster_size {
                    break;
                }
            }
        }

        Ok(())
    }

    fn compute_cluster_topics(
        &self,
        cluster_nodes: &HashSet<u32>,
        similarities: &HashMap<(u32, u32), NodeSimilarity>,
    ) -> Vec<String> {
        let mut topic_counts: HashMap<String, usize> = HashMap::new();

        for &node_a in cluster_nodes {
            for &node_b in cluster_nodes {
                if node_a >= node_b {
                    continue;
                }

                if let Some(similarity) =
                    similarities.get(&(node_a.min(node_b), node_a.max(node_b)))
                {
                    for topic in &similarity.shared_topics {
                        *topic_counts.entry(topic.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        let mut sorted_topics: Vec<_> = topic_counts.into_iter().collect();
        sorted_topics.sort_by(|a, b| b.1.cmp(&a.1));

        sorted_topics
            .into_iter()
            .take(self.config.max_topics_per_node)
            .map(|(topic, _)| topic)
            .collect()
    }

    fn compute_cluster_coherence(
        &self,
        cluster_nodes: &HashSet<u32>,
        similarities: &HashMap<(u32, u32), NodeSimilarity>,
    ) -> f32 {
        if cluster_nodes.len() < 2 {
            return 1.0;
        }

        let mut similarity_sum = 0.0;
        let mut pair_count = 0;

        for &node_a in cluster_nodes {
            for &node_b in cluster_nodes {
                if node_a >= node_b {
                    continue;
                }

                if let Some(similarity) =
                    similarities.get(&(node_a.min(node_b), node_a.max(node_b)))
                {
                    similarity_sum += similarity.combined_similarity;
                    pair_count += 1;
                }
            }
        }

        if pair_count > 0 {
            similarity_sum / pair_count as f32
        } else {
            0.0
        }
    }

    fn identify_hierarchical_relations(
        &self,
        graph_data: &GraphData,
        metadata_store: Option<&MetadataStore>,
    ) -> Result<Vec<HierarchicalRelation>, Box<dyn std::error::Error>> {
        let mut relations = Vec::new();

        for edge in &graph_data.edges {
            if let (Some(source_node), Some(target_node)) = (
                graph_data.nodes.iter().find(|n| n.id == edge.source),
                graph_data.nodes.iter().find(|n| n.id == edge.target),
            ) {
                let relation_type =
                    self.infer_relation_type(source_node, target_node, metadata_store);

                if !relation_type.is_empty() {
                    let strength = self.compute_hierarchical_strength(
                        source_node,
                        target_node,
                        metadata_store,
                    );

                    if strength > 0.3 {
                        relations.push(HierarchicalRelation {
                            parent_id: edge.source,
                            child_id: edge.target,
                            relation_type,
                            strength,
                        });
                    }
                }
            }
        }

        debug!("Identified {} hierarchical relationships", relations.len());
        Ok(relations)
    }

    fn infer_relation_type(
        &self,
        source_node: &Node,
        target_node: &Node,
        _metadata_store: Option<&MetadataStore>,
    ) -> String {
        let source_label = source_node.label.to_lowercase();
        let target_label = target_node.label.to_lowercase();

        if source_label.contains("index") || source_label.contains("overview") {
            return "contains".to_string();
        }

        if target_label.contains(&source_label) || source_label.contains(&target_label) {
            return "references".to_string();
        }

        if source_label.ends_with('/') || source_label.contains("folder") {
            return "contains".to_string();
        }

        String::new()
    }

    fn compute_hierarchical_strength(
        &self,
        source_node: &Node,
        target_node: &Node,
        metadata_store: Option<&MetadataStore>,
    ) -> f32 {
        let mut strength: f32 = 0.0;

        if let Some(store) = metadata_store {
            if let (Some(source_meta), Some(target_meta)) = (
                store.get(&source_node.metadata_id),
                store.get(&target_node.metadata_id),
            ) {
                let size_ratio = source_meta.file_size as f32 / target_meta.file_size.max(1) as f32;
                if size_ratio > 1.5 {
                    strength += 0.3;
                }

                if source_meta.hyperlink_count > target_meta.hyperlink_count {
                    strength += 0.2;
                }
            }
        }

        let source_label = &source_node.label.to_lowercase();
        let target_label = &target_node.label.to_lowercase();

        if source_label.contains("overview") || source_label.contains("index") {
            strength += 0.4;
        }

        if target_label.contains(source_label) {
            strength += 0.3;
        }

        strength.min(1.0)
    }

    fn generate_clustering_constraints(
        &self,
        clusters: &[SemanticCluster],
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let mut constraints = Vec::new();

        for cluster in clusters {
            if cluster.node_ids.len() >= 2 {
                let node_indices: Vec<u32> = cluster.node_ids.iter().cloned().collect();
                let cluster_strength = cluster.coherence;

                let constraint = Constraint::cluster(
                    node_indices,
                    clusters
                        .iter()
                        .position(|c| c.id == cluster.id)
                        .expect("Expected item to be in collection") as f32,
                    cluster_strength,
                );

                constraints.push(constraint);
            }
        }

        debug!("Generated {} clustering constraints", constraints.len());
        Ok(constraints)
    }

    fn generate_separation_constraints(
        &self,
        graph_data: &GraphData,
        similarities: &HashMap<(u32, u32), NodeSimilarity>,
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let mut constraints = Vec::new();
        let separation_threshold = 0.2;

        for ((id_a, id_b), similarity) in similarities {
            if similarity.combined_similarity < separation_threshold {
                let constraint =
                    Constraint::separation(*id_a, *id_b, self.config.min_separation_distance);

                constraints.push(constraint);

                if constraints.len() >= graph_data.nodes.len() {
                    break;
                }
            }
        }

        debug!("Generated {} separation constraints", constraints.len());
        Ok(constraints)
    }

    fn generate_alignment_constraints(
        &self,
        hierarchical_relations: &[HierarchicalRelation],
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let mut constraints = Vec::new();

        let mut hierarchy_groups: HashMap<String, Vec<u32>> = HashMap::new();

        for relation in hierarchical_relations {
            hierarchy_groups
                .entry(relation.relation_type.clone())
                .or_default()
                .extend([relation.parent_id, relation.child_id]);
        }

        for (relation_type, node_ids) in hierarchy_groups {
            if node_ids.len() >= 2 {
                let mut unique_nodes: Vec<u32> = node_ids
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                unique_nodes.sort();

                if unique_nodes.len() >= 2 {
                    let constraint = match relation_type.as_str() {
                        "contains" => Constraint::align_horizontal(unique_nodes, 0.0),
                        _ => Constraint::align_horizontal(unique_nodes, 0.0),
                    };

                    constraints.push(constraint);
                }
            }
        }

        debug!("Generated {} alignment constraints", constraints.len());
        Ok(constraints)
    }

    fn generate_boundary_constraints(
        &self,
        clusters: &[SemanticCluster],
    ) -> Result<Vec<Constraint>, Box<dyn std::error::Error>> {
        let mut constraints = Vec::new();

        for cluster in clusters {
            if cluster.node_ids.len() >= 3 {
                let node_indices: Vec<u32> = cluster.node_ids.iter().cloned().collect();

                let boundary_size = 200.0 * (cluster.node_ids.len() as f32).sqrt();

                let constraint = Constraint::boundary(
                    node_indices,
                    -boundary_size,
                    boundary_size,
                    -boundary_size,
                    boundary_size,
                    -boundary_size / 2.0,
                    boundary_size / 2.0,
                );

                constraints.push(constraint);
            }
        }

        debug!("Generated {} boundary constraints", constraints.len());
        Ok(constraints)
    }

    pub fn apply_to_constraint_set(
        &self,
        constraint_set: &mut ConstraintSet,
        result: &ConstraintGenerationResult,
    ) {
        for constraint in &result.clustering_constraints {
            constraint_set.add_to_group("semantic_clustering", constraint.clone());
        }

        for constraint in &result.separation_constraints {
            constraint_set.add_to_group("semantic_separation", constraint.clone());
        }

        for constraint in &result.alignment_constraints {
            constraint_set.add_to_group("hierarchical_alignment", constraint.clone());
        }

        for constraint in &result.boundary_constraints {
            constraint_set.add_to_group("cluster_boundaries", constraint.clone());
        }

        info!(
            "Applied {} semantic constraints to constraint set",
            result.clustering_constraints.len()
                + result.separation_constraints.len()
                + result.alignment_constraints.len()
                + result.boundary_constraints.len()
        );
    }

    pub fn get_clusters(&self) -> &HashMap<(u32, u32), NodeSimilarity> {
        &self.similarity_cache
    }

    pub fn update_config(&mut self, config: SemanticConstraintConfig) {
        self.config = config;
        self.similarity_cache.clear();
        info!("Updated semantic constraint generator configuration");
    }

    pub fn clear_cache(&mut self) {
        self.similarity_cache.clear();
        self.topic_embeddings.clear();
        trace!("Cleared semantic constraint generator cache");
    }
}

impl Default for SemanticConstraintGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{edge::Edge, graph::GraphData, metadata::Metadata, node::Node};

    use std::collections::HashMap;

    fn create_test_graph_with_metadata() -> (GraphData, MetadataStore) {
        let mut graph = GraphData {
            nodes: vec![
                Node::new_with_id("ai_overview.md".to_string(), Some(1)),
                Node::new_with_id("machine_learning.md".to_string(), Some(2)),
                Node::new_with_id("deep_learning.md".to_string(), Some(3)),
                Node::new_with_id("cooking_recipes.md".to_string(), Some(4)),
            ],
            edges: vec![Edge::new(1, 2, 1.0), Edge::new(2, 3, 1.0)],
            metadata: visionclaw_domain::models::metadata::MetadataStore::new(),
            id_to_metadata: std::collections::HashMap::new(),
        };

        // Use deterministic positions for consistent test results
        // Position related nodes close together
        graph.nodes[0].label = "AI Overview".to_string();
        graph.nodes[0].data.x = 0.0;
        graph.nodes[0].data.y = 0.0;
        graph.nodes[0].data.z = 0.0;

        graph.nodes[1].label = "Machine Learning".to_string();
        graph.nodes[1].data.x = 10.0;
        graph.nodes[1].data.y = 5.0;
        graph.nodes[1].data.z = 2.0;

        graph.nodes[2].label = "Deep Learning".to_string();
        graph.nodes[2].data.x = 20.0;
        graph.nodes[2].data.y = 10.0;
        graph.nodes[2].data.z = 4.0;

        // Cooking is far from AI-related nodes
        graph.nodes[3].label = "Cooking Recipes".to_string();
        graph.nodes[3].data.x = 200.0;
        graph.nodes[3].data.y = 200.0;
        graph.nodes[3].data.z = 100.0;

        let mut metadata_store = MetadataStore::new();

        // Create topics with high overlap between related nodes
        let mut ai_topics = HashMap::new();
        ai_topics.insert("artificial_intelligence".to_string(), 20);
        ai_topics.insert("machine_learning".to_string(), 15);
        ai_topics.insert("technology".to_string(), 10);

        let mut ml_topics = HashMap::new();
        ml_topics.insert("machine_learning".to_string(), 25);
        ml_topics.insert("artificial_intelligence".to_string(), 20);
        ml_topics.insert("deep_learning".to_string(), 10);
        ml_topics.insert("algorithms".to_string(), 8);

        let mut dl_topics = HashMap::new();
        dl_topics.insert("deep_learning".to_string(), 25);
        dl_topics.insert("machine_learning".to_string(), 20);
        dl_topics.insert("artificial_intelligence".to_string(), 15);
        dl_topics.insert("neural_networks".to_string(), 12);

        // Cooking topics have no overlap with AI topics
        let mut cooking_topics = HashMap::new();
        cooking_topics.insert("cooking".to_string(), 25);
        cooking_topics.insert("recipes".to_string(), 18);
        cooking_topics.insert("food".to_string(), 12);

        metadata_store.insert(
            "ai_overview.md".to_string(),
            Metadata {
                file_name: "ai_overview.md".to_string(),
                file_size: 5000,
                topic_counts: ai_topics,
                ..Default::default()
            },
        );

        metadata_store.insert(
            "machine_learning.md".to_string(),
            Metadata {
                file_name: "machine_learning.md".to_string(),
                file_size: 8000,
                topic_counts: ml_topics,
                ..Default::default()
            },
        );

        metadata_store.insert(
            "deep_learning.md".to_string(),
            Metadata {
                file_name: "deep_learning.md".to_string(),
                file_size: 12000,
                topic_counts: dl_topics,
                ..Default::default()
            },
        );

        metadata_store.insert(
            "cooking_recipes.md".to_string(),
            Metadata {
                file_name: "cooking_recipes.md".to_string(),
                file_size: 3000,
                topic_counts: cooking_topics,
                ..Default::default()
            },
        );

        (graph, metadata_store)
    }

    #[test]
    fn test_generator_creation() {
        let generator = SemanticConstraintGenerator::new();
        assert_eq!(generator.config.clustering_threshold, 0.6);
        assert!(generator.config.enable_topic_clustering);
    }

    #[test]
    fn test_topic_similarity_computation() {
        let generator = SemanticConstraintGenerator::new();

        let mut topics_a = HashMap::new();
        topics_a.insert("ai".to_string(), 10);
        topics_a.insert("ml".to_string(), 5);

        let mut topics_b = HashMap::new();
        topics_b.insert("ai".to_string(), 8);
        topics_b.insert("deep_learning".to_string(), 12);

        let similarity = generator.compute_topic_similarity(&topics_a, &topics_b);
        assert!(similarity > 0.0 && similarity <= 1.0);
    }

    #[test]
    fn test_string_similarity() {
        let generator = SemanticConstraintGenerator::new();

        let sim1 = generator.compute_string_similarity("machine learning", "machine learning");
        assert_eq!(sim1, 1.0);

        let sim2 = generator.compute_string_similarity("machine learning", "deep learning");
        assert!(sim2 > 0.0 && sim2 < 1.0);

        let sim3 =
            generator.compute_string_similarity("artificial intelligence", "cooking recipes");
        assert!(sim3 < 0.5);
    }

    #[test]
    fn test_cosine_similarity() {
        let generator = SemanticConstraintGenerator::new();

        let vec_a = vec![1.0, 2.0, 3.0];
        let vec_b = vec![1.0, 2.0, 3.0];
        let sim1 = generator.cosine_similarity(&vec_a, &vec_b);
        assert!((sim1 - 1.0).abs() < 1e-6);

        let vec_c = vec![0.0, 0.0, 0.0];
        let sim2 = generator.cosine_similarity(&vec_a, &vec_c);
        assert_eq!(sim2, 0.0);
    }

    #[test]
    fn test_constraint_generation() {
        let mut generator = SemanticConstraintGenerator::new();
        let (graph, metadata) = create_test_graph_with_metadata();

        let result = generator
            .generate_constraints(&graph, Some(&metadata))
            .unwrap();

        assert!(result.stats.nodes_processed == 4);
        assert!(result.stats.similarity_calculations > 0);
        // processing_time may be 0 on fast hardware — measured as duration_since(Instant)
        // and the test fixture is tiny enough to complete sub-microsecond.
        // Just assert it is u64 (always >=0) and move on.
        let _ = result.stats.processing_time;

        let total_constraints = result.clustering_constraints.len()
            + result.separation_constraints.len()
            + result.alignment_constraints.len()
            + result.boundary_constraints.len();
        assert!(total_constraints > 0);

        assert!(!result.clusters.is_empty());
    }

    #[test]
    fn test_shared_topics_identification() {
        let generator = SemanticConstraintGenerator::new();

        let mut topics_a = HashMap::new();
        topics_a.insert("ai".to_string(), 10);
        topics_a.insert("ml".to_string(), 5);
        topics_a.insert("rare_topic".to_string(), 1);

        let mut topics_b = HashMap::new();
        topics_b.insert("ai".to_string(), 8);
        topics_b.insert("deep_learning".to_string(), 12);
        topics_b.insert("rare_topic".to_string(), 1);

        let shared = generator.find_shared_topics(&topics_a, &topics_b);
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0], "ai");
    }

    #[test]
    fn test_constraint_application_to_set() {
        let generator = SemanticConstraintGenerator::new();
        let mut constraint_set = ConstraintSet::default();

        let result = ConstraintGenerationResult {
            clustering_constraints: vec![Constraint::cluster(vec![1, 2], 0.0, 0.8)],
            separation_constraints: vec![Constraint::separation(3, 4, 150.0)],
            alignment_constraints: vec![],
            boundary_constraints: vec![],
            clusters: vec![],
            hierarchical_relations: vec![],
            stats: GenerationStats {
                nodes_processed: 4,
                similarity_calculations: 6,
                clusters_created: 1,
                processing_time: 100,
                avg_cluster_coherence: 0.8,
            },
        };

        generator.apply_to_constraint_set(&mut constraint_set, &result);

        assert!(constraint_set.groups.contains_key("semantic_clustering"));
        assert!(constraint_set.groups.contains_key("semantic_separation"));
        assert_eq!(constraint_set.constraints.len(), 2);
    }
}
