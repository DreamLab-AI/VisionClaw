// src/services/ontology_content_analyzer.rs
//! Ontology Content Analyzer
//!
//! Provides utilities for analyzing markdown file content to extract:
//! - OntologyBlock detection
//! - Source domain from term-id prefixes (AI-, BC-, MV-, etc.)
//! - Topic extraction
//! - Relationship and class counting
//! - public:: true flag detection

use log::debug;
use once_cell::sync:::Lazy;
use regex:::Regex;
use std::collections:::HashSet;

// Domain prefixes for source domain detection
static DOMAIN_PREFIXES: &[(&str, &str)] = &[
    ("AI-", "Artificial Intelligence"),
    ("BC-", "Blockchain"),
    ("MV-", "Metaverse"),
    ("QC-", "Quantum Computing"),
    ("BIO-", "Biotechnology"),
    ("CYBER-", "Cybersecurity"),
    ("DATA-", "Data Science"),
    ("IOT-", "Internet of Things"),
    ("AR-", "Augmented Reality"),
    ("VR-", "Virtual Reality"),
    ("ML-", "Machine Learning"),
    ("NLP-", "Natural Language Processing"),
    ("CV-", "Computer Vision"),
    ("ROBOT-", "Robotics"),
    ("EDGE-", "Edge Computing"),
    ("CLOUD-", "Cloud Computing"),
];

// Regex patterns for content analysis
static TERM_ID_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"term-id::\s*([A-Z]+-[A-Z0-9_-]+)").expect("Invalid TERM_ID_PATTERN")
});

static OWL_CLASS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"owl[_:]class::\s*([a-zA-Z0-9_:/-]+)").expect("Invalid OWL_CLASS_PATTERN")
});

static OBJECT_PROPERTY_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"objectProperty::\s*([a-zA-Z0-9_:/-]+)").expect("Invalid OBJECT_PROPERTY_PATTERN")
});

static DATA_PROPERTY_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"dataProperty::\s*([a-zA-Z0-9_:/-]+)").expect("Invalid DATA_PROPERTY_PATTERN")
});

static SUBCLASS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"subClassOf::\s*([a-zA-Z0-9_:/-]+)").expect("Invalid SUBCLASS_PATTERN")
});

static TOPIC_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"topic::\s*\[\[([^\]]+)\]\]").expect("Invalid TOPIC_PATTERN")
});

/// Content analysis results
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ContentAnalysis {
    pub has_public_flag: bool,
    pub has_ontology_block: bool,
    pub source_domain: Option<String>,
    pub topics: Vec<String>,
    pub relationship_count: usize,
    pub class_count: usize,
    pub property_count: usize,
    pub term_ids: Vec<String>,
}

pub struct OntologyContentAnalyzer;

impl OntologyContentAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze markdown content for ontology-specific metadata
    pub fn analyze_content(&self, content: &str, filename: &str) -> ContentAnalysis {
        let mut analysis = ContentAnalysis::default();

        // Check for public:: true flag (case-insensitive, flexible spacing)
        analysis.has_public_flag = content
            .lines()
            .take(20)
            .any(|line| {
                let trimmed = line.trim().to_lowercase();
                trimmed == "public:: true" || trimmed == "public::true"
            });

        // Check for OntologyBlock section
        analysis.has_ontology_block = self.detect_ontology_block(content);

        // Extract term IDs and detect source domain
        analysis.term_ids = self.extract_term_ids(content);
        analysis.source_domain = self.detect_source_domain(&analysis.term_ids);

        // Extract topics
        analysis.topics = self.extract_topics(content);

        // Count ontology elements
        if analysis.has_ontology_block {
            let ontology_section = self.extract_ontology_section(content);
            analysis.class_count = self.count_classes(&ontology_section);
            analysis.property_count = self.count_properties(&ontology_section);
            analysis.relationship_count = self.count_relationships(&ontology_section);
        }

        debug!(
            "Analyzed {}: public={}, ontology={}, domain={:?}, topics={}, classes={}, properties={}, relationships={}",
            filename,
            analysis.has_public_flag,
            analysis.has_ontology_block,
            analysis.source_domain,
            analysis.topics.len(),
            analysis.class_count,
            analysis.property_count,
            analysis.relationship_count
        );

        analysis
    }

    /// Detect if content contains an OntologyBlock section
    fn detect_ontology_block(&self, content: &str) -> bool {
        content.contains("### OntologyBlock") || content.contains("###OntologyBlock")
    }

    /// Extract the OntologyBlock section from content
    fn extract_ontology_section(&self, content: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();

        if let Some(start_idx) = lines
            .iter()
            .position(|line| line.contains("### OntologyBlock"))
        {
            // Take from OntologyBlock to end of file
            lines[start_idx..].join("\n")
        } else {
            String::new()
        }
    }

    /// Extract term IDs from content
    fn extract_term_ids(&self, content: &str) -> Vec<String> {
        TERM_ID_PATTERN
            .captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    /// Detect source domain from term ID prefixes
    fn detect_source_domain(&self, term_ids: &[String]) -> Option<String> {
        if term_ids.is_empty() {
            return None;
        }

        // Count occurrences of each domain prefix
        let mut domain_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for term_id in term_ids {
            for (prefix, domain) in DOMAIN_PREFIXES {
                if term_id.starts_with(prefix) {
                    *domain_counts.entry(domain.to_string()).or_insert(0) += 1;
                    break;
                }
            }
        }

        // Return the most common domain
        domain_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(domain, _)| domain)
    }

    /// Extract topics from content
    fn extract_topics(&self, content: &str) -> Vec<String> {
        let mut topics = HashSet::new();

        // Extract from topic:: [[...]] patterns
        for cap in TOPIC_PATTERN.captures_iter(content) {
            if let Some(topic_match) = cap.get(1) {
                let topic = topic_match.as_str().trim().to_string();
                if !topic.is_empty() {
                    topics.insert(topic);
                }
            }
        }

        // Also check for tags:: [[...]] patterns
        let tag_pattern = Regex::new(r"tags?::\s*\[\[([^\]]+)\]\]").unwrap();
        for cap in tag_pattern.captures_iter(content) {
            if let Some(tag_match) = cap.get(1) {
                let tag = tag_match.as_str().trim().to_string();
                if !tag.is_empty() {
                    topics.insert(tag);
                }
            }
        }

        topics.into_iter().collect()
    }

    /// Count OWL classes in ontology section
    fn count_classes(&self, section: &str) -> usize {
        OWL_CLASS_PATTERN.captures_iter(section).count()
    }

    /// Count OWL properties (both object and data properties)
    fn count_properties(&self, section: &str) -> usize {
        let object_props = OBJECT_PROPERTY_PATTERN.captures_iter(section).count();
        let data_props = DATA_PROPERTY_PATTERN.captures_iter(section).count();
        object_props + data_props
    }

    /// Count relationships (subClassOf, domain, range, etc.)
    fn count_relationships(&self, section: &str) -> usize {
        let subclass_rels = SUBCLASS_PATTERN.captures_iter(section).count();

        // Count domain/range relationships
        let domain_rels = section.matches("domain::").count();
        let range_rels = section.matches("range::").count();

        subclass_rels + domain_rels + range_rels
    }

    /// Quick check if content should be processed (has public or ontology block)
    pub fn should_process(&self, content: &str) -> bool {
        let has_public = content
            .lines()
            .take(20)
            .any(|line| {
                let trimmed = line.trim().to_lowercase();
                trimmed == "public:: true" || trimmed == "public::true"
            });

        let has_ontology = self.detect_ontology_block(content);

        has_public || has_ontology
    }
}

impl Default for OntologyContentAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_public_flag() {
        let analyzer = OntologyContentAnalyzer::new();

        let content = "public:: true\n# Test";
        let analysis = analyzer.analyze_content(content, "test.md");
        assert!(analysis.has_public_flag);

        let content = "public::true\n# Test";
        let analysis = analyzer.analyze_content(content, "test.md");
        assert!(analysis.has_public_flag);
    }

    #[test]
    fn test_detect_ontology_block() {
        let analyzer = OntologyContentAnalyzer::new();

        let content = "# Test\n- ### OntologyBlock\n  - owl_class:: Test";
        let analysis = analyzer.analyze_content(content, "test.md");
        assert!(analysis.has_ontology_block);
    }

    #[test]
    fn test_extract_source_domain() {
        let analyzer = OntologyContentAnalyzer::new();

        let content = r#"
term-id:: AI-001
term-id:: AI-002
term-id:: BC-001
"#;

        let analysis = analyzer.analyze_content(content, "test.md");
        assert_eq!(analysis.source_domain, Some("Artificial Intelligence".to_string()));
        assert_eq!(analysis.term_ids.len(), 3);
    }

    #[test]
    fn test_count_ontology_elements() {
        let analyzer = OntologyContentAnalyzer::new();

        let content = r#"
- ### OntologyBlock
  - owl_class:: Person
    - subClassOf:: Entity
  - owl_class:: Student
    - subClassOf:: Person
  - objectProperty:: hasParent
    - domain:: Person
    - range:: Person
  - dataProperty:: hasAge
    - domain:: Person
    - range:: xsd:integer
"#;

        let analysis = analyzer.analyze_content(content, "test.md");
        assert_eq!(analysis.class_count, 2);
        assert_eq!(analysis.property_count, 2);
        assert!(analysis.relationship_count >= 2); // At least 2 subClassOf
    }

    #[test]
    fn test_extract_topics() {
        let analyzer = OntologyContentAnalyzer::new();

        let content = r#"
topic:: [[Machine Learning]]
topic:: [[AI]]
tags:: [[Research]]
"#;

        let analysis = analyzer.analyze_content(content, "test.md");
        assert!(analysis.topics.len() >= 2);
        assert!(analysis.topics.contains(&"Machine Learning".to_string()));
    }
}

