//! Advanced semantic analysis service for knowledge graph enhancement

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use visionclaw_domain::models::metadata::Metadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFeatures {
    pub id: String,

    pub topics: HashMap<String, f32>,

    pub domains: Vec<KnowledgeDomain>,

    pub temporal: TemporalFeatures,

    pub structural: StructuralFeatures,

    pub content: ContentFeatures,

    pub agent_patterns: Option<AgentCommunicationPatterns>,

    pub importance_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum KnowledgeDomain {
    Mathematics,
    Physics,
    ComputerScience,
    Biology,
    Chemistry,
    Engineering,
    DataScience,
    MachineLearning,
    WebDevelopment,
    SystemsProgramming,
    DevOps,
    Security,
    Documentation,
    Configuration,
    Testing,
    UserInterface,
    Database,
    Networking,
    CloudComputing,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalFeatures {
    pub created_at: Option<DateTime<Utc>>,

    pub modified_at: Option<DateTime<Utc>>,

    pub modification_frequency: f32,

    pub co_evolution_score: f32,

    pub temporal_cluster: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralFeatures {
    pub file_type: String,

    pub directory_depth: u32,

    pub dependency_count: u32,

    pub complexity_score: f32,

    pub loc: Option<u32>,

    pub module_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFeatures {
    pub language: String,

    pub key_terms: Vec<String>,

    pub embeddings: Option<Vec<f32>>,

    pub content_hash: String,

    pub documentation_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommunicationPatterns {
    pub send_frequency: f32,

    pub receive_frequency: f32,

    pub communication_partners: HashMap<String, f32>,

    pub message_types: HashSet<String>,

    pub clustering_coefficient: f32,

    pub network_role: NetworkRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkRole {
    Hub,
    Bridge,
    Peripheral,
    Isolated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnalyzerConfig {
    pub enable_topics: bool,

    pub num_topics: usize,

    pub enable_temporal: bool,

    pub enable_agent_patterns: bool,

    pub min_term_frequency: f32,

    pub max_features: usize,

    pub enable_caching: bool,
}

impl Default for SemanticAnalyzerConfig {
    fn default() -> Self {
        Self {
            enable_topics: true,
            num_topics: 10,
            enable_temporal: true,
            enable_agent_patterns: false,
            min_term_frequency: 0.01,
            max_features: 100,
            enable_caching: true,
        }
    }
}

pub struct SemanticAnalyzer {
    config: SemanticAnalyzerConfig,
    feature_cache: HashMap<String, SemanticFeatures>,
    domain_patterns: HashMap<KnowledgeDomain, Vec<String>>,
}

impl Clone for SemanticAnalyzer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            feature_cache: self.feature_cache.clone(),
            domain_patterns: self.domain_patterns.clone(),
        }
    }
}

impl SemanticAnalyzer {
    pub fn new(config: SemanticAnalyzerConfig) -> Self {
        let mut analyzer = Self {
            config,
            feature_cache: HashMap::new(),
            domain_patterns: HashMap::new(),
        };
        analyzer.initialize_domain_patterns();
        analyzer
    }

    fn initialize_domain_patterns(&mut self) {
        self.domain_patterns.insert(
            KnowledgeDomain::Mathematics,
            vec![
                "theorem", "proof", "equation", "matrix", "vector", "calculus", "algebra",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        self.domain_patterns.insert(
            KnowledgeDomain::MachineLearning,
            vec![
                "model",
                "training",
                "neural",
                "network",
                "tensor",
                "gradient",
                "optimizer",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        self.domain_patterns.insert(
            KnowledgeDomain::WebDevelopment,
            vec![
                "react",
                "vue",
                "angular",
                "html",
                "css",
                "javascript",
                "frontend",
                "backend",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        self.domain_patterns.insert(
            KnowledgeDomain::SystemsProgramming,
            vec![
                "kernel", "memory", "pointer", "thread", "mutex", "syscall", "buffer",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        self.domain_patterns.insert(
            KnowledgeDomain::Database,
            vec![
                "sql",
                "query",
                "table",
                "index",
                "transaction",
                "schema",
                "relation",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        self.domain_patterns.insert(
            KnowledgeDomain::Security,
            vec![
                "encryption",
                "authentication",
                "vulnerability",
                "exploit",
                "firewall",
                "ssl",
                "tls",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
    }

    pub fn analyze_metadata(&mut self, metadata: &Metadata) -> SemanticFeatures {
        let id = metadata.file_name.trim_end_matches(".md").to_string();

        if self.config.enable_caching {
            if let Some(cached) = self.feature_cache.get(&id) {
                return cached.clone();
            }
        }

        let topics = self.extract_topics(metadata);
        let domains = self.classify_domains(&topics, &metadata.file_name);
        let temporal = self.extract_temporal_features(metadata);
        let structural = self.extract_structural_features(metadata);
        let content = self.extract_content_features(metadata);
        let importance_score = self.calculate_importance_score(&topics, &temporal, &structural);

        let features = SemanticFeatures {
            id: id.clone(),
            topics,
            domains,
            temporal,
            structural,
            content,
            agent_patterns: None,
            importance_score,
        };

        if self.config.enable_caching {
            self.feature_cache.insert(id, features.clone());
        }

        features
    }

    fn extract_topics(&self, metadata: &Metadata) -> HashMap<String, f32> {
        let mut topics = HashMap::new();

        if !self.config.enable_topics {
            return topics;
        }

        for (topic, &count) in &metadata.topic_counts {
            let weight = (count as f32).ln() + 1.0;
            topics.insert(topic.clone(), weight);
        }

        let total: f32 = topics.values().sum();
        if total > 0.0 {
            for weight in topics.values_mut() {
                *weight /= total;
            }
        }

        topics
    }

    fn classify_domains(&self, topics: &HashMap<String, f32>, path: &str) -> Vec<KnowledgeDomain> {
        let mut domains = Vec::new();
        let mut domain_scores: HashMap<KnowledgeDomain, f32> = HashMap::new();

        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match extension {
            "py" => domain_scores.insert(KnowledgeDomain::DataScience, 0.3),
            "rs" => domain_scores.insert(KnowledgeDomain::SystemsProgramming, 0.4),
            "js" | "jsx" | "ts" | "tsx" => {
                domain_scores.insert(KnowledgeDomain::WebDevelopment, 0.4)
            }
            "sql" => domain_scores.insert(KnowledgeDomain::Database, 0.5),
            "cu" | "cuda" => domain_scores.insert(KnowledgeDomain::Engineering, 0.4),
            _ => None,
        };

        for (domain, patterns) in &self.domain_patterns {
            let mut score = 0.0;
            for pattern in patterns {
                if let Some(&weight) = topics.get(pattern) {
                    score += weight;
                }
            }
            if score > 0.0 {
                *domain_scores.entry(domain.clone()).or_insert(0.0) += score;
            }
        }

        let mut scored_domains: Vec<_> = domain_scores.into_iter().collect();
        scored_domains.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (domain, score) in scored_domains.iter().take(3) {
            if *score > 0.1 {
                domains.push(domain.clone());
            }
        }

        if domains.is_empty() {
            domains.push(KnowledgeDomain::Other(extension.to_string()));
        }

        domains
    }

    fn extract_temporal_features(&self, _metadata: &Metadata) -> TemporalFeatures {
        TemporalFeatures {
            created_at: None,
            modified_at: None,
            modification_frequency: 1.0,
            co_evolution_score: 0.0,
            temporal_cluster: None,
        }
    }

    fn extract_structural_features(&self, metadata: &Metadata) -> StructuralFeatures {
        let path = Path::new(&metadata.file_name);
        let directory_depth = path.components().count() as u32;
        let file_type = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_string();

        StructuralFeatures {
            file_type,
            directory_depth,
            dependency_count: 0,
            complexity_score: (metadata.topic_counts.len() as f32).ln() + 1.0,
            loc: Some(metadata.file_size as u32),
            module_path: path
                .parent()
                .map(|p| p.to_string_lossy().split('/').map(String::from).collect())
                .unwrap_or_default(),
        }
    }

    fn extract_content_features(&self, metadata: &Metadata) -> ContentFeatures {
        let mut key_terms: Vec<_> = metadata.topic_counts.keys().cloned().collect();
        key_terms.sort_by_key(|k| std::cmp::Reverse(metadata.topic_counts[k]));
        key_terms.truncate(20);

        ContentFeatures {
            language: self.detect_language(&metadata.file_name),
            key_terms,
            embeddings: None,
            content_hash: metadata.sha1.clone(),
            documentation_score: self.calculate_documentation_score(metadata),
        }
    }

    fn detect_language(&self, path: &str) -> String {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match extension {
            "py" => "Python",
            "rs" => "Rust",
            "js" | "jsx" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "java" => "Java",
            "cpp" | "cc" | "cxx" => "C++",
            "c" | "h" => "C",
            "go" => "Go",
            "rb" => "Ruby",
            "php" => "PHP",
            "swift" => "Swift",
            "kt" => "Kotlin",
            "scala" => "Scala",
            "r" => "R",
            "m" => "MATLAB",
            "cu" | "cuda" => "CUDA",
            "md" => "Markdown",
            "txt" => "Text",
            "json" => "JSON",
            "yaml" | "yml" => "YAML",
            "toml" => "TOML",
            "xml" => "XML",
            "html" => "HTML",
            "css" => "CSS",
            "sql" => "SQL",
            _ => "Unknown",
        }
        .to_string()
    }

    fn calculate_documentation_score(&self, metadata: &Metadata) -> f32 {
        let mut score: f32 = 0.0;

        let doc_terms = [
            "readme",
            "doc",
            "comment",
            "description",
            "example",
            "usage",
            "api",
        ];
        for term in doc_terms {
            if metadata.topic_counts.contains_key(term) {
                score += 0.2;
            }
        }

        if metadata.file_name.ends_with(".md") {
            score += 0.3;
        }

        score.min(1.0)
    }

    fn calculate_importance_score(
        &self,
        topics: &HashMap<String, f32>,
        temporal: &TemporalFeatures,
        structural: &StructuralFeatures,
    ) -> f32 {
        let mut score = 0.0;

        let topic_entropy = -topics
            .values()
            .filter(|&&v| v > 0.0)
            .map(|&v| v * v.ln())
            .sum::<f32>();
        score += topic_entropy.min(1.0) * 0.3;

        score += temporal.modification_frequency.min(1.0) * 0.2;

        score += (structural.dependency_count as f32 / 10.0).min(1.0) * 0.3;

        score += (structural.complexity_score / 5.0).min(1.0) * 0.2;

        score.min(1.0)
    }

    pub fn analyze_agent_patterns(
        &mut self,
        agent_id: &str,
        messages: &[(String, String, DateTime<Utc>)],
    ) -> AgentCommunicationPatterns {
        let mut send_count = 0;
        let mut receive_count = 0;
        let mut partners: HashMap<String, f32> = HashMap::new();
        let mut message_types = HashSet::new();

        for (from, to, _timestamp) in messages {
            if from == agent_id {
                send_count += 1;
                *partners.entry(to.clone()).or_insert(0.0) += 1.0;
            }
            if to == agent_id {
                receive_count += 1;
                *partners.entry(from.clone()).or_insert(0.0) += 1.0;
            }

            message_types.insert("default".to_string());
        }

        let total_messages = (send_count + receive_count) as f32;
        let send_frequency = send_count as f32 / total_messages.max(1.0);
        let receive_frequency = receive_count as f32 / total_messages.max(1.0);

        let degree = partners.len();
        let network_role = if degree == 0 {
            NetworkRole::Isolated
        } else if degree > 10 {
            NetworkRole::Hub
        } else if degree > 5 {
            NetworkRole::Bridge
        } else {
            NetworkRole::Peripheral
        };

        AgentCommunicationPatterns {
            send_frequency,
            receive_frequency,
            communication_partners: partners,
            message_types,
            clustering_coefficient: 0.0,
            network_role,
        }
    }

    pub fn compute_similarity(
        &self,
        features1: &SemanticFeatures,
        features2: &SemanticFeatures,
    ) -> f32 {
        let mut similarity = 0.0;

        let topic_sim = self.cosine_similarity(&features1.topics, &features2.topics);
        similarity += topic_sim * 0.4;

        let domain_overlap = features1
            .domains
            .iter()
            .filter(|d| features2.domains.contains(d))
            .count() as f32;
        let domain_sim =
            domain_overlap / (features1.domains.len().max(features2.domains.len()) as f32).max(1.0);
        similarity += domain_sim * 0.2;

        if features1.structural.file_type == features2.structural.file_type {
            similarity += 0.1;
        }

        let depth_diff = (features1.structural.directory_depth as f32
            - features2.structural.directory_depth as f32)
            .abs();
        similarity += (1.0 / (1.0 + depth_diff)) * 0.1;

        let temporal_sim = 1.0
            / (1.0
                + (features1.temporal.modification_frequency
                    - features2.temporal.modification_frequency)
                    .abs());
        similarity += temporal_sim * 0.1;

        let importance_diff = (features1.importance_score - features2.importance_score).abs();
        similarity += (1.0 - importance_diff) * 0.1;

        similarity.min(1.0)
    }

    fn cosine_similarity(
        &self,
        topics1: &HashMap<String, f32>,
        topics2: &HashMap<String, f32>,
    ) -> f32 {
        let mut dot_product = 0.0;
        let mut norm1 = 0.0;
        let mut norm2 = 0.0;

        let all_topics: HashSet<_> = topics1.keys().chain(topics2.keys()).collect();

        for topic in all_topics {
            let v1 = topics1.get(topic.as_str()).unwrap_or(&0.0);
            let v2 = topics2.get(topic.as_str()).unwrap_or(&0.0);

            dot_product += v1 * v2;
            norm1 += v1 * v1;
            norm2 += v2 * v2;
        }

        if norm1 > 0.0 && norm2 > 0.0 {
            dot_product / (norm1.sqrt() * norm2.sqrt())
        } else {
            0.0
        }
    }

    pub fn get_cached_features(&self) -> &HashMap<String, SemanticFeatures> {
        &self.feature_cache
    }

    pub fn clear_cache(&mut self) {
        self.feature_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_analyzer_creation() {
        let config = SemanticAnalyzerConfig::default();
        let analyzer = SemanticAnalyzer::new(config);
        assert!(!analyzer.domain_patterns.is_empty());
    }

    #[test]
    fn test_domain_classification() {
        let analyzer = SemanticAnalyzer::new(SemanticAnalyzerConfig::default());

        let mut topics = HashMap::new();
        topics.insert("neural".to_string(), 0.3);
        topics.insert("network".to_string(), 0.2);
        topics.insert("training".to_string(), 0.4);

        let domains = analyzer.classify_domains(&topics, "model.py");
        assert!(
            domains.contains(&KnowledgeDomain::MachineLearning)
                || domains.contains(&KnowledgeDomain::DataScience)
        );
    }

    #[test]
    fn test_language_detection() {
        let analyzer = SemanticAnalyzer::new(SemanticAnalyzerConfig::default());

        assert_eq!(analyzer.detect_language("test.py"), "Python");
        assert_eq!(analyzer.detect_language("main.rs"), "Rust");
        assert_eq!(analyzer.detect_language("app.js"), "JavaScript");
        assert_eq!(analyzer.detect_language("kernel.cu"), "CUDA");
    }

    #[test]
    fn test_similarity_computation() {
        let analyzer = SemanticAnalyzer::new(SemanticAnalyzerConfig::default());

        let mut topics1 = HashMap::new();
        topics1.insert("test".to_string(), 0.5);
        topics1.insert("unit".to_string(), 0.5);

        let mut topics2 = HashMap::new();
        topics2.insert("test".to_string(), 0.4);
        topics2.insert("integration".to_string(), 0.6);

        let similarity = analyzer.cosine_similarity(&topics1, &topics2);
        assert!(similarity > 0.0 && similarity < 1.0);
    }
}
