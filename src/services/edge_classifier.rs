// src/services/edge_classifier.rs
//! Semantic Edge Classification Service
//!
//! Analyzes the context of links between nodes to determine the appropriate
//! OWL property (relationship type) that should be assigned to graph edges.

use log::{debug, info};
use std::collections::HashMap;

/// Edge classification based on contextual analysis
pub struct EdgeClassifier {
    /// Rule-based patterns for edge classification
    patterns: HashMap<String, Vec<Pattern>>,
}

/// A pattern for matching edge contexts to OWL properties
#[derive(Clone)]
struct Pattern {
    keywords: Vec<String>,
    property_iri: String,
    confidence: f32,
}

impl EdgeClassifier {
    /// Create a new EdgeClassifier with default patterns
    pub fn new() -> Self {
        let mut classifier = Self {
            patterns: HashMap::new(),
        };

        classifier.load_default_patterns();
        classifier
    }

    /// Load default classification patterns
    fn load_default_patterns(&mut self) {
        // Employment/Work relationships
        self.add_pattern(
            "worksAt",
            vec![
                "works at",
                "employed by",
                "employee of",
                "works for",
                "position at",
                "job at",
                "career at",
            ],
            "mv:worksAt",
            0.9,
        );

        // Leadership relationships
        self.add_pattern(
            "hasCEO",
            vec![
                "CEO",
                "Chief Executive Officer",
                "chief executive",
                "CEO of",
                "leads",
                "headed by",
            ],
            "mv:hasCEO",
            0.95,
        );

        self.add_pattern(
            "hasCTO",
            vec!["CTO", "Chief Technology Officer", "chief technology"],
            "mv:hasCTO",
            0.95,
        );

        self.add_pattern(
            "hasFounder",
            vec!["founded by", "founder", "co-founder", "founded"],
            "mv:hasFounder",
            0.9,
        );

        // Project relationships
        self.add_pattern(
            "contributesTo",
            vec![
                "contributes to",
                "contributor",
                "works on",
                "developing",
                "maintains",
                "maintainer of",
            ],
            "mv:contributesTo",
            0.85,
        );

        self.add_pattern(
            "usesProject",
            vec![
                "uses",
                "depends on",
                "built with",
                "powered by",
                "based on",
                "utilizes",
            ],
            "mv:usesProject",
            0.8,
        );

        // Knowledge relationships
        self.add_pattern(
            "relatedTo",
            vec![
                "related to",
                "similar to",
                "connected to",
                "associated with",
                "linked to",
                "see also",
            ],
            "mv:relatedTo",
            0.7,
        );

        self.add_pattern(
            "subConceptOf",
            vec![
                "is a",
                "type of",
                "kind of",
                "subclass of",
                "category",
                "subcategory",
            ],
            "mv:subConceptOf",
            0.85,
        );

        // Technology relationships
        self.add_pattern(
            "usesTechnology",
            vec![
                "built with",
                "technology stack",
                "uses technology",
                "implemented in",
                "written in",
            ],
            "mv:usesTechnology",
            0.85,
        );

        info!(
            "EdgeClassifier initialized with {} pattern groups",
            self.patterns.len()
        );
    }

    /// Add a classification pattern
    fn add_pattern(
        &mut self,
        name: &str,
        keywords: Vec<&str>,
        property_iri: &str,
        confidence: f32,
    ) {
        let pattern = Pattern {
            keywords: keywords.iter().map(|s| s.to_lowercase()).collect(),
            property_iri: property_iri.to_string(),
            confidence,
        };

        self.patterns
            .entry(name.to_string())
            .or_default()
            .push(pattern);
    }

    /// Classify an edge based on context
    /// # Arguments
    /// * `source_label` - Label of source node (e.g., "Tim Cook")
    /// * `target_label` - Label of target node (e.g., "Apple Inc")
    /// * `source_class` - OWL class IRI of source (e.g., "mv:Person")
    /// * `target_class` - OWL class IRI of target (e.g., "mv:Company")
    /// * `context` - Surrounding text context (e.g., "CEO: [[Apple Inc]]")
    /// # Returns
    /// Optional OWL property IRI if classification succeeds
    pub fn classify_edge(
        &self,
        source_label: &str,
        target_label: &str,
        source_class: Option<&str>,
        target_class: Option<&str>,
        context: &str,
    ) -> Option<String> {
        let context_lower = context.to_lowercase();

        // Try pattern matching first
        let mut best_match: Option<(String, f32)> = None;

        for patterns in self.patterns.values() {
            for pattern in patterns {
                let mut score = 0.0f32;
                let mut matches = 0;

                for keyword in &pattern.keywords {
                    if context_lower.contains(keyword) {
                        matches += 1;
                        score += pattern.confidence;
                    }
                }

                if matches > 0 {
                    let avg_score = score / matches as f32;
                    if let Some((_, current_best)) = &best_match {
                        if avg_score > *current_best {
                            best_match = Some((pattern.property_iri.clone(), avg_score));
                        }
                    } else {
                        best_match = Some((pattern.property_iri.clone(), avg_score));
                    }
                }
            }
        }

        if let Some((property_iri, confidence)) = &best_match {
            debug!(
                "Classified edge {} -> {} as {} (confidence: {:.2})",
                source_label, target_label, property_iri, confidence
            );
            return Some(property_iri.clone());
        }

        // Fallback: Use class-based heuristics
        if let (Some(src_class), Some(tgt_class)) = (source_class, target_class) {
            let fallback = self.classify_by_class_pair(src_class, tgt_class);
            if let Some(ref prop) = fallback {
                debug!(
                    "Classified edge {} -> {} as {} (class-based fallback)",
                    source_label, target_label, prop
                );
            }
            return fallback;
        }

        // No classification found
        debug!(
            "Could not classify edge {} -> {} (no patterns matched)",
            source_label, target_label
        );
        None
    }

    /// Classify edge based on class pair heuristics
    fn classify_by_class_pair(&self, source_class: &str, target_class: &str) -> Option<String> {
        match (source_class, target_class) {
            // Person -> Company
            (src, tgt) if src.contains("Person") && tgt.contains("Company") => {
                Some("mv:worksAt".to_string())
            }
            // Person -> Project
            (src, tgt) if src.contains("Person") && tgt.contains("Project") => {
                Some("mv:contributesTo".to_string())
            }
            // Company -> Project
            (src, tgt) if src.contains("Company") && tgt.contains("Project") => {
                Some("mv:sponsors".to_string())
            }
            // Project -> Technology
            (src, tgt) if src.contains("Project") && tgt.contains("Technology") => {
                Some("mv:usesTechnology".to_string())
            }
            // Concept -> Concept
            (src, tgt) if src.contains("Concept") && tgt.contains("Concept") => {
                Some("mv:relatedTo".to_string())
            }
            // Default: no classification
            _ => None,
        }
    }

    /// Batch classify multiple edges
    pub fn classify_edges_batch(&self, edges: Vec<EdgeContext>) -> Vec<Option<String>> {
        edges
            .iter()
            .map(|ctx| {
                self.classify_edge(
                    &ctx.source_label,
                    &ctx.target_label,
                    ctx.source_class.as_deref(),
                    ctx.target_class.as_deref(),
                    &ctx.context,
                )
            })
            .collect()
    }
}

impl Default for EdgeClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Context information for edge classification
#[derive(Debug, Clone)]
pub struct EdgeContext {
    pub source_label: String,
    pub target_label: String,
    pub source_class: Option<String>,
    pub target_class: Option<String>,
    pub context: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ceo_classification() {
        let classifier = EdgeClassifier::new();

        let result = classifier.classify_edge(
            "Tim Cook",
            "Apple Inc",
            Some("mv:Person"),
            Some("mv:Company"),
            "Tim Cook is the CEO of [[Apple Inc]]",
        );

        assert_eq!(result, Some("mv:hasCEO".to_string()));
    }

    #[test]
    fn test_works_at_classification() {
        let classifier = EdgeClassifier::new();

        let result = classifier.classify_edge(
            "John Doe",
            "TechCorp",
            Some("mv:Person"),
            Some("mv:Company"),
            "John Doe works at [[TechCorp]] as an engineer",
        );

        assert_eq!(result, Some("mv:worksAt".to_string()));
    }

    #[test]
    fn test_class_based_fallback() {
        let classifier = EdgeClassifier::new();

        let result = classifier.classify_edge(
            "Jane Smith",
            "Project Alpha",
            Some("mv:Person"),
            Some("mv:Project"),
            "Jane Smith mentioned [[Project Alpha]]", // No clear keywords
        );

        assert_eq!(result, Some("mv:contributesTo".to_string()));
    }

    #[test]
    fn test_no_classification() {
        let classifier = EdgeClassifier::new();

        let result = classifier.classify_edge("Unknown", "Something", None, None, "Random text");

        assert!(result.is_none());
    }
}
