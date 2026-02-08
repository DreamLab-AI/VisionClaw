// src/services/parsers/ontology_parser.rs
//! Enhanced Ontology Parser
//!
//! Parses markdown files containing `- ### OntologyBlock` headers to extract:
//! - Complete Tier 1 (Required) properties
//! - Complete Tier 2 (Recommended) properties
//! - Complete Tier 3 (Optional) properties
//! - All relationship types (is-subclass-of, has-part, requires, enables, bridges-to, etc.)
//! - OWL axioms in Clojure format
//! - Domain-specific extension properties
//!
//! Based on canonical-ontology-block.md specification v1.0.0

use crate::ports::ontology_repository::{AxiomType, OwlAxiom, OwlClass, OwlProperty};
use log::{debug, info};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// ============================================================================
// Regex Patterns - Compiled once at startup for performance
// ============================================================================

// Property extraction patterns
static PROPERTY_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*-\s*([a-zA-Z0-9_:-]+)::\s*(.+)$").expect("Invalid PROPERTY_PATTERN regex")
});

static WIKI_LINK_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[\[([^\]]+)\]\]").expect("Invalid WIKI_LINK_PATTERN regex")
});

static SECTION_HEADER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*-\s*(#{1,4})\s*(.+)$").expect("Invalid SECTION_HEADER_PATTERN regex")
});

static OWL_AXIOM_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)```(?:clojure|owl)\s*\n(.*?)\n\s*```").expect("Invalid OWL_AXIOM_PATTERN regex")
});

static BRIDGE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*-\s*(bridges-(?:to|from))::\s*\[\[([^\]]+)\]\]\s*via\s+(\w+)")
        .expect("Invalid BRIDGE_PATTERN regex")
});

// Domain configuration
static DOMAIN_PREFIXES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("AI-", "ai");
    m.insert("BC-", "bc");
    m.insert("RB-", "rb");
    m.insert("MV-", "mv");
    m.insert("TC-", "tc");
    m.insert("DT-", "dt");
    m
});

// ============================================================================
// Data Structures
// ============================================================================

/// Complete ontology block with all tiers of metadata
#[derive(Debug, Clone)]
pub struct OntologyBlock {
    // === File Location ===
    pub file_path: String,
    pub raw_block: String,

    // === Tier 1: Required Properties ===
    // Identification
    pub ontology: bool,
    pub term_id: Option<String>,
    pub preferred_term: Option<String>,
    pub source_domain: Option<String>,
    pub status: Option<String>,
    pub public_access: Option<bool>,
    pub last_updated: Option<String>,

    // Definition
    pub definition: Option<String>,

    // Semantic Classification
    pub owl_class: Option<String>,
    pub owl_physicality: Option<String>,
    pub owl_role: Option<String>,

    // Relationships (Tier 1)
    pub is_subclass_of: Vec<String>,

    // === Tier 2: Recommended Properties ===
    // Identification (Tier 2)
    pub alt_terms: Vec<String>,
    pub version: Option<String>,
    pub quality_score: Option<f64>,
    pub cross_domain_links: Option<i32>,

    // Definition (Tier 2)
    pub maturity: Option<String>,
    pub source: Vec<String>,
    pub authority_score: Option<f64>,
    pub scope_note: Option<String>,

    // Semantic Classification (Tier 2)
    pub owl_inferred_class: Option<String>,
    pub belongs_to_domain: Vec<String>,

    // Relationships (Tier 2)
    pub has_part: Vec<String>,
    pub is_part_of: Vec<String>,
    pub requires: Vec<String>,
    pub depends_on: Vec<String>,
    pub enables: Vec<String>,
    pub relates_to: Vec<String>,

    // === Tier 3: Optional Properties ===
    pub implemented_in_layer: Vec<String>,

    // Cross-domain bridges
    pub bridges_to: Vec<String>,
    pub bridges_from: Vec<String>,

    // OWL axioms from code blocks
    pub owl_axioms: Vec<String>,

    // Domain-specific extension properties
    pub domain_extensions: HashMap<String, String>,

    // All other relationships not explicitly modeled
    pub other_relationships: HashMap<String, Vec<String>>,
}

impl OntologyBlock {
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            raw_block: String::new(),
            ontology: false,
            term_id: None,
            preferred_term: None,
            source_domain: None,
            status: None,
            public_access: None,
            last_updated: None,
            definition: None,
            owl_class: None,
            owl_physicality: None,
            owl_role: None,
            is_subclass_of: Vec::new(),
            alt_terms: Vec::new(),
            version: None,
            quality_score: None,
            cross_domain_links: None,
            maturity: None,
            source: Vec::new(),
            authority_score: None,
            scope_note: None,
            owl_inferred_class: None,
            belongs_to_domain: Vec::new(),
            has_part: Vec::new(),
            is_part_of: Vec::new(),
            requires: Vec::new(),
            depends_on: Vec::new(),
            enables: Vec::new(),
            relates_to: Vec::new(),
            implemented_in_layer: Vec::new(),
            bridges_to: Vec::new(),
            bridges_from: Vec::new(),
            owl_axioms: Vec::new(),
            domain_extensions: HashMap::new(),
            other_relationships: HashMap::new(),
        }
    }

    /// Get domain from term-id, source-domain, or namespace
    pub fn get_domain(&self) -> Option<String> {
        // Try source-domain first
        if let Some(ref domain) = self.source_domain {
            return Some(domain.to_lowercase());
        }

        // Try term-id prefix
        if let Some(ref term_id) = self.term_id {
            for (prefix, domain) in DOMAIN_PREFIXES.iter() {
                if term_id.starts_with(prefix) {
                    return Some(domain.to_string());
                }
            }
        }

        // Try namespace in owl_class
        if let Some(ref owl_class) = self.owl_class {
            if let Some(colon_idx) = owl_class.find(':') {
                let prefix = &owl_class[..colon_idx];
                if DOMAIN_PREFIXES.values().any(|&d| d == prefix) {
                    return Some(prefix.to_string());
                }
            }
        }

        None
    }

    /// Get full IRI from owl:class property
    pub fn get_full_iri(&self) -> Option<String> {
        let owl_class = self.owl_class.as_ref()?;

        // If already a full URI, return it
        if owl_class.starts_with("http://") || owl_class.starts_with("https://") {
            return Some(owl_class.clone());
        }

        // Parse namespace:localname format
        if let Some(colon_idx) = owl_class.find(':') {
            let prefix = &owl_class[..colon_idx];
            let localname = &owl_class[colon_idx + 1..];

            // Map domain prefixes to namespaces
            let namespace = match prefix {
                "ai" => "http://narrativegoldmine.com/ai#",
                "bc" => "http://narrativegoldmine.com/blockchain#",
                "rb" => "http://narrativegoldmine.com/robotics#",
                "mv" => "http://narrativegoldmine.com/metaverse#",
                "tc" => "http://narrativegoldmine.com/telecollaboration#",
                "dt" => "http://narrativegoldmine.com/disruptive-tech#",
                "owl" => "http://www.w3.org/2002/07/owl#",
                "rdfs" => "http://www.w3.org/2000/01/rdf-schema#",
                "rdf" => "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
                "xsd" => "http://www.w3.org/2001/XMLSchema#",
                "dcterms" => "http://purl.org/dc/terms/",
                "skos" => "http://www.w3.org/2004/02/skos/core#",
                _ => return None,
            };

            return Some(format!("{}{}", namespace, localname));
        }

        None
    }

    /// Validate that all Tier 1 (required) properties are present
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Tier 1 required properties
        if self.term_id.is_none() {
            errors.push("Missing required property: term-id".to_string());
        }

        if self.preferred_term.is_none() {
            errors.push("Missing required property: preferred-term".to_string());
        }

        if self.source_domain.is_none() {
            errors.push("Missing required property: source-domain".to_string());
        }

        if self.status.is_none() {
            errors.push("Missing required property: status".to_string());
        }

        if self.public_access.is_none() {
            errors.push("Missing required property: public-access".to_string());
        }

        if self.last_updated.is_none() {
            errors.push("Missing required property: last-updated".to_string());
        }

        if self.definition.is_none() {
            errors.push("Missing required property: definition".to_string());
        }

        if self.owl_class.is_none() {
            errors.push("Missing required property: owl:class".to_string());
        }

        if self.owl_physicality.is_none() {
            errors.push("Missing required property: owl:physicality".to_string());
        }

        if self.owl_role.is_none() {
            errors.push("Missing required property: owl:role".to_string());
        }

        if self.is_subclass_of.is_empty() {
            errors.push("Missing required property: is-subclass-of (at least one parent class)".to_string());
        }

        // Validate term-id format
        if let Some(ref term_id) = self.term_id {
            if let Some(domain) = self.get_domain() {
                if let Some((&expected_prefix, _)) = DOMAIN_PREFIXES
                    .iter()
                    .find(|(_, &d)| d == domain.as_str())
                {
                    if !term_id.starts_with(expected_prefix) {
                        errors.push(format!(
                            "term-id '{}' doesn't match domain '{}' (expected {})",
                            term_id, domain, expected_prefix
                        ));
                    }
                }
            }
        }

        // Validate namespace consistency
        if let Some(ref owl_class) = self.owl_class {
            if let Some(colon_idx) = owl_class.find(':') {
                let prefix = &owl_class[..colon_idx];
                if let Some(ref domain) = self.source_domain {
                    let domain_lower = domain.to_lowercase();
                    if prefix != domain_lower && DOMAIN_PREFIXES.values().any(|&d| d == prefix) {
                        errors.push(format!(
                            "owl:class namespace '{}' doesn't match source-domain '{}'",
                            prefix, domain
                        ));
                    }
                }
            }
        }

        errors
    }
}

/// Legacy output structure for backward compatibility
#[derive(Debug)]
pub struct OntologyData {
    pub classes: Vec<OwlClass>,
    pub properties: Vec<OwlProperty>,
    pub axioms: Vec<OwlAxiom>,
    pub class_hierarchy: Vec<(String, String)>,
}

// ============================================================================
// Parser Implementation
// ============================================================================

pub struct OntologyParser;

impl OntologyParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a markdown file and extract the complete ontology block
    pub fn parse_enhanced(&self, content: &str, filename: &str) -> Result<OntologyBlock, String> {
        info!("Parsing ontology file (enhanced): {}", filename);

        // Extract ontology section
        let ontology_section = self.extract_ontology_section(content)?;

        // Create block object
        let mut block = OntologyBlock::new(filename.to_string());
        block.raw_block = ontology_section.clone();

        // === Extract Tier 1 Properties ===
        self.extract_tier1_properties(&ontology_section, &mut block);

        // === Extract Tier 2 Properties ===
        self.extract_tier2_properties(&ontology_section, &mut block);

        // === Extract Tier 3 Properties ===
        self.extract_tier3_properties(&ontology_section, &mut block);

        // === Extract Relationships ===
        self.extract_relationships(&ontology_section, &mut block);

        // === Extract Cross-Domain Bridges ===
        self.extract_bridges(&ontology_section, &mut block);

        // === Extract OWL Axioms ===
        self.extract_owl_axioms(content, &mut block);

        // === Extract Domain Extensions ===
        self.extract_domain_extensions(&ontology_section, &mut block);

        debug!(
            "Parsed enhanced {}: term_id={:?}, domain={:?}, relationships={}",
            filename,
            block.term_id,
            block.get_domain(),
            block.is_subclass_of.len()
                + block.has_part.len()
                + block.requires.len()
                + block.enables.len()
        );

        Ok(block)
    }

    /// Legacy parse method for backward compatibility
    pub fn parse(&self, content: &str, filename: &str) -> Result<OntologyData, String> {
        let block = self.parse_enhanced(content, filename)?;

        // Convert to legacy format
        let classes = self.block_to_classes(&block);
        let properties = Vec::new(); // Could extract from OWL axioms if needed
        let axioms = self.block_to_axioms(&block);
        let class_hierarchy = block.is_subclass_of
            .iter()
            .filter_map(|parent| {
                block.owl_class.as_ref().map(|cls| (cls.clone(), parent.clone()))
            })
            .collect();

        Ok(OntologyData {
            classes,
            properties,
            axioms,
            class_hierarchy,
        })
    }

    // ========================================================================
    // Section Extraction
    // ========================================================================

    fn extract_ontology_section(&self, content: &str) -> Result<String, String> {
        let lines: Vec<&str> = content.lines().collect();
        let mut section_start = None;

        for (i, line) in lines.iter().enumerate() {
            if line.contains("### OntologyBlock") {
                section_start = Some(i);
                break;
            }
        }

        let start = section_start.ok_or_else(|| "No OntologyBlock found in file".to_string())?;
        let section: Vec<&str> = lines[start..].iter().copied().collect();

        Ok(section.join("\n"))
    }

    // ========================================================================
    // Tier 1 Property Extraction
    // ========================================================================

    fn extract_tier1_properties(&self, section: &str, block: &mut OntologyBlock) {
        // Identification
        if let Some(val) = self.extract_property(section, "ontology") {
            block.ontology = val.to_lowercase() == "true";
        }
        block.term_id = self.extract_property(section, "term-id");
        block.preferred_term = self.extract_property(section, "preferred-term");
        block.source_domain = self.extract_property(section, "source-domain");
        block.status = self.extract_property(section, "status");

        if let Some(val) = self.extract_property(section, "public-access") {
            block.public_access = Some(val.to_lowercase() == "true");
        }

        block.last_updated = self.extract_property(section, "last-updated");

        // Definition
        block.definition = self.extract_property(section, "definition");

        // Semantic Classification
        block.owl_class = self.extract_property(section, "owl:class");
        block.owl_physicality = self.extract_property(section, "owl:physicality");
        block.owl_role = self.extract_property(section, "owl:role");
    }

    // ========================================================================
    // Tier 2 Property Extraction
    // ========================================================================

    fn extract_tier2_properties(&self, section: &str, block: &mut OntologyBlock) {
        // Identification (Tier 2)
        block.alt_terms = self.extract_property_list(section, "alt-terms");
        block.version = self.extract_property(section, "version");

        if let Some(val) = self.extract_property(section, "quality-score") {
            block.quality_score = val.parse::<f64>().ok();
        }

        if let Some(val) = self.extract_property(section, "cross-domain-links") {
            block.cross_domain_links = val.parse::<i32>().ok();
        }

        // Definition (Tier 2)
        block.maturity = self.extract_property(section, "maturity");
        block.source = self.extract_property_list(section, "source");

        if let Some(val) = self.extract_property(section, "authority-score") {
            block.authority_score = val.parse::<f64>().ok();
        }

        block.scope_note = self.extract_property(section, "scope-note");

        // Semantic Classification (Tier 2)
        block.owl_inferred_class = self.extract_property(section, "owl:inferred-class");
        block.belongs_to_domain = self.extract_property_list(section, "belongsToDomain");
    }

    // ========================================================================
    // Tier 3 Property Extraction
    // ========================================================================

    fn extract_tier3_properties(&self, section: &str, block: &mut OntologyBlock) {
        block.implemented_in_layer = self.extract_property_list(section, "implementedInLayer");
    }

    // ========================================================================
    // Relationship Extraction
    // ========================================================================

    fn extract_relationships(&self, section: &str, block: &mut OntologyBlock) {
        // Known relationships
        block.is_subclass_of = self.extract_property_list(section, "is-subclass-of");
        block.has_part = self.extract_property_list(section, "has-part");
        block.is_part_of = self.extract_property_list(section, "is-part-of");
        block.requires = self.extract_property_list(section, "requires");
        block.depends_on = self.extract_property_list(section, "depends-on");
        block.enables = self.extract_property_list(section, "enables");
        block.relates_to = self.extract_property_list(section, "relates-to");

        // Extract other relationships from Relationships section
        let relationships_section = self.extract_relationships_section(section);

        let known_rels = vec![
            "is-subclass-of", "has-part", "is-part-of", "requires",
            "depends-on", "enables", "relates-to", "bridges-to", "bridges-from"
        ];

        for (rel_name, targets) in relationships_section {
            if !known_rels.contains(&rel_name.as_str()) {
                block.other_relationships.insert(rel_name, targets);
            }
        }
    }

    fn extract_relationships_section(&self, section: &str) -> HashMap<String, Vec<String>> {
        let mut relationships = HashMap::new();
        let mut in_relationships = false;

        for line in section.lines() {
            // Check for Relationships section header
            if line.contains("#### Relationships") {
                in_relationships = true;
                continue;
            }

            // Stop at next section
            if in_relationships && line.contains("####") && !line.contains("Relationships") {
                break;
            }

            if in_relationships {
                if let Some(caps) = PROPERTY_PATTERN.captures(line) {
                    let prop_name = caps.get(1).unwrap().as_str().to_string();
                    let value_text = caps.get(2).unwrap().as_str();

                    // Extract wiki-links
                    let wiki_links = self.extract_wiki_links(value_text);
                    if !wiki_links.is_empty() {
                        relationships.entry(prop_name).or_insert_with(Vec::new).extend(wiki_links);
                    }
                }
            }
        }

        relationships
    }

    // ========================================================================
    // Cross-Domain Bridges Extraction
    // ========================================================================

    fn extract_bridges(&self, section: &str, block: &mut OntologyBlock) {
        for line in section.lines() {
            if let Some(caps) = BRIDGE_PATTERN.captures(line) {
                let direction = caps.get(1).unwrap().as_str();
                let target = caps.get(2).unwrap().as_str();
                let via = caps.get(3).unwrap().as_str();

                let bridge_str = format!("{} via {}", target, via);

                if direction == "bridges-to" {
                    block.bridges_to.push(bridge_str);
                } else {
                    block.bridges_from.push(bridge_str);
                }
            }
        }
    }

    // ========================================================================
    // OWL Axioms Extraction
    // ========================================================================

    fn extract_owl_axioms(&self, content: &str, block: &mut OntologyBlock) {
        for caps in OWL_AXIOM_PATTERN.captures_iter(content) {
            if let Some(axiom_match) = caps.get(1) {
                block.owl_axioms.push(axiom_match.as_str().trim().to_string());
            }
        }
    }

    // ========================================================================
    // Domain Extension Properties
    // ========================================================================

    fn extract_domain_extensions(&self, section: &str, block: &mut OntologyBlock) {
        let domain = match block.get_domain() {
            Some(d) => d,
            None => return,
        };

        // Domain-specific properties based on canonical schema
        let extension_props = match domain.as_str() {
            "ai" => vec!["algorithm-type", "computational-complexity"],
            "bc" => vec!["consensus-mechanism", "decentralization-level"],
            "rb" => vec!["physicality", "autonomy-level"],
            "mv" => vec!["immersion-level", "interaction-mode"],
            "tc" => vec!["collaboration-type", "communication-mode"],
            "dt" => vec!["disruption-level", "maturity-stage"],
            _ => vec![],
        };

        for prop in extension_props {
            if let Some(val) = self.extract_property(section, prop) {
                block.domain_extensions.insert(prop.to_string(), val);
            }
        }
    }

    // ========================================================================
    // Property Extraction Utilities
    // ========================================================================

    fn extract_property(&self, section: &str, property_name: &str) -> Option<String> {
        for line in section.lines() {
            if let Some(caps) = PROPERTY_PATTERN.captures(line) {
                let prop = caps.get(1).unwrap().as_str();
                if prop == property_name {
                    let value = caps.get(2).unwrap().as_str().trim();
                    // Remove wiki-link brackets if present
                    let cleaned = if value.starts_with("[[") && value.ends_with("]]") {
                        &value[2..value.len() - 2]
                    } else {
                        value
                    };
                    return Some(cleaned.to_string());
                }
            }
        }
        None
    }

    fn extract_property_list(&self, section: &str, property_name: &str) -> Vec<String> {
        let mut result = Vec::new();

        for line in section.lines() {
            if let Some(caps) = PROPERTY_PATTERN.captures(line) {
                let prop = caps.get(1).unwrap().as_str();
                if prop == property_name {
                    let value_text = caps.get(2).unwrap().as_str();
                    let wiki_links = self.extract_wiki_links(value_text);
                    result.extend(wiki_links);
                }
            }
        }

        result
    }

    fn extract_wiki_links(&self, text: &str) -> Vec<String> {
        WIKI_LINK_PATTERN
            .captures_iter(text)
            .filter_map(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
            .collect()
    }

    // ========================================================================
    // Legacy Conversion Methods
    // ========================================================================

    fn block_to_classes(&self, block: &OntologyBlock) -> Vec<OwlClass> {
        if block.owl_class.is_none() {
            return Vec::new();
        }

        let mut properties = HashMap::new();
        properties.insert("source_file".to_string(), block.file_path.clone());

        if let Some(ref term_id) = block.term_id {
            properties.insert("term_id".to_string(), term_id.clone());
        }
        if let Some(ref status) = block.status {
            properties.insert("status".to_string(), status.clone());
        }

        vec![OwlClass {
            iri: block.owl_class.clone().unwrap_or_default(),
            term_id: block.term_id.clone(),
            preferred_term: block.preferred_term.clone(),
            label: block.preferred_term.clone(),
            description: block.definition.clone(),
            parent_classes: block.is_subclass_of.clone(),
            source_domain: block.source_domain.clone(),
            version: block.version.clone(),
            class_type: block.owl_class.clone(),
            status: block.status.clone(),
            maturity: block.maturity.clone(),
            quality_score: block.quality_score.map(|v| v as f32),
            authority_score: block.authority_score.map(|v| v as f32),
            public_access: block.public_access,
            content_status: None,
            owl_physicality: block.owl_physicality.clone(),
            owl_role: block.owl_role.clone(),
            belongs_to_domain: block.belongs_to_domain.first().cloned(),
            bridges_to_domain: None,
            source_file: Some(block.file_path.clone()),
            file_sha1: None,
            markdown_content: Some(block.raw_block.clone()),
            last_synced: None,
            properties,
            additional_metadata: None,
        }]
    }

    fn block_to_axioms(&self, block: &OntologyBlock) -> Vec<OwlAxiom> {
        let mut axioms = Vec::new();

        // SubClassOf axioms
        if let Some(ref subject) = block.owl_class {
            for parent in &block.is_subclass_of {
                axioms.push(OwlAxiom {
                    id: None,
                    axiom_type: AxiomType::SubClassOf,
                    subject: subject.clone(),
                    object: parent.clone(),
                    annotations: HashMap::new(),
                });
            }
        }

        axioms
    }
}

impl Default for OntologyParser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enhanced_complete_block() {
        let parser = OntologyParser::new();
        let content = r#"
# Large Language Models

- ### OntologyBlock
  id:: llm-ontology
  collapsed:: true

  - **Identification**
    - ontology:: true
    - term-id:: AI-0850
    - preferred-term:: Large Language Models
    - alt-terms:: [[LLM]], [[Foundation Models]]
    - source-domain:: ai
    - status:: complete
    - public-access:: true
    - version:: 1.0.0
    - last-updated:: 2025-11-21
    - quality-score:: 0.85

  - **Definition**
    - definition:: A Large Language Model (LLM) is an artificial intelligence system based on deep neural networks (typically [[Transformer]] architectures) trained on vast text corpora to understand and generate human-like text.
    - maturity:: mature
    - source:: [[OpenAI Research]], [[Stanford AI Index]]
    - authority-score:: 0.95

  - **Semantic Classification**
    - owl:class:: ai:LargeLanguageModel
    - owl:physicality:: VirtualEntity
    - owl:role:: Process
    - belongsToDomain:: [[AI-GroundedDomain]]

  - #### Relationships
    id:: llm-relationships
    - is-subclass-of:: [[Artificial Intelligence]], [[Neural Network Architecture]]
    - requires:: [[Training Data]], [[Computational Resources]]
    - enables:: [[Few-Shot Learning]], [[Text Generation]]
"#;

        let result = parser.parse_enhanced(content, "test.md");
        assert!(result.is_ok());

        let block = result.unwrap();

        // Tier 1 properties
        assert_eq!(block.term_id, Some("AI-0850".to_string()));
        assert_eq!(block.preferred_term, Some("Large Language Models".to_string()));
        assert_eq!(block.source_domain, Some("ai".to_string()));
        assert_eq!(block.status, Some("complete".to_string()));
        assert_eq!(block.public_access, Some(true));
        assert_eq!(block.owl_class, Some("ai:LargeLanguageModel".to_string()));
        assert_eq!(block.owl_physicality, Some("VirtualEntity".to_string()));
        assert_eq!(block.owl_role, Some("Process".to_string()));

        // Tier 2 properties
        assert_eq!(block.alt_terms, vec!["LLM", "Foundation Models"]);
        assert_eq!(block.version, Some("1.0.0".to_string()));
        assert_eq!(block.quality_score, Some(0.85));
        assert_eq!(block.maturity, Some("mature".to_string()));
        assert_eq!(block.authority_score, Some(0.95));

        // Relationships
        assert_eq!(block.is_subclass_of.len(), 2);
        assert!(block.is_subclass_of.contains(&"Artificial Intelligence".to_string()));
        assert_eq!(block.requires, vec!["Training Data", "Computational Resources"]);
        assert_eq!(block.enables, vec!["Few-Shot Learning", "Text Generation"]);

        // Validation
        let errors = block.validate();
        assert!(errors.is_empty(), "Validation errors: {:?}", errors);
    }

    #[test]
    fn test_parse_enhanced_with_bridges() {
        let parser = OntologyParser::new();
        let content = r#"
- ### OntologyBlock
  - ontology:: true
  - term-id:: AI-0001
  - preferred-term:: Test Concept
  - source-domain:: ai
  - status:: complete
  - public-access:: true
  - last-updated:: 2025-11-21
  - definition:: A test concept
  - owl:class:: ai:TestConcept
  - owl:physicality:: VirtualEntity
  - owl:role:: Concept

  - #### Relationships
    - is-subclass-of:: [[owl:Thing]]

  - #### CrossDomainBridges
    - bridges-to:: [[Blockchain Verification]] via enables
    - bridges-from:: [[Robot Control]] via requires
"#;

        let result = parser.parse_enhanced(content, "test.md");
        assert!(result.is_ok());

        let block = result.unwrap();
        assert_eq!(block.bridges_to, vec!["Blockchain Verification via enables"]);
        assert_eq!(block.bridges_from, vec!["Robot Control via requires"]);
    }

    #[test]
    fn test_parse_enhanced_with_owl_axioms() {
        let parser = OntologyParser::new();
        let content = r#"
- ### OntologyBlock
  - term-id:: AI-0002
  - preferred-term:: Test
  - source-domain:: ai
  - status:: draft
  - public-access:: true
  - last-updated:: 2025-11-21
  - definition:: Test
  - owl:class:: ai:Test
  - owl:physicality:: VirtualEntity
  - owl:role:: Concept

  - #### Relationships
    - is-subclass-of:: [[owl:Thing]]

  - #### OWL Axioms
    - ```clojure
      (Declaration (Class :TestConcept))
      (SubClassOf :TestConcept :ParentClass)
      ```
"#;

        let result = parser.parse_enhanced(content, "test.md");
        assert!(result.is_ok());

        let block = result.unwrap();
        assert_eq!(block.owl_axioms.len(), 1);
        assert!(block.owl_axioms[0].contains("Declaration"));
    }

    #[test]
    fn test_validation_missing_required() {
        let mut block = OntologyBlock::new("test.md".to_string());
        block.term_id = Some("AI-0001".to_string());
        // Missing other required fields

        let errors = block.validate();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("preferred-term")));
        assert!(errors.iter().any(|e| e.contains("definition")));
    }

    #[test]
    fn test_get_domain() {
        let mut block = OntologyBlock::new("test.md".to_string());

        // From source-domain
        block.source_domain = Some("ai".to_string());
        assert_eq!(block.get_domain(), Some("ai".to_string()));

        // From term-id
        block.source_domain = None;
        block.term_id = Some("BC-0001".to_string());
        assert_eq!(block.get_domain(), Some("bc".to_string()));
    }

    #[test]
    fn test_get_full_iri() {
        let mut block = OntologyBlock::new("test.md".to_string());
        block.owl_class = Some("ai:LargeLanguageModel".to_string());

        let iri = block.get_full_iri();
        assert_eq!(iri, Some("http://narrativegoldmine.com/ai#LargeLanguageModel".to_string()));
    }

    #[test]
    fn test_legacy_parse_backward_compatibility() {
        let parser = OntologyParser::new();
        let content = r#"
- ### OntologyBlock
  - owl:class:: ai:Test
  - preferred-term:: Test Concept
  - definition:: A test
  - owl:physicality:: VirtualEntity
  - owl:role:: Concept
  - term-id:: AI-0001
  - source-domain:: ai
  - status:: complete
  - public-access:: true
  - last-updated:: 2025-11-21

  - #### Relationships
    - is-subclass-of:: [[Parent]]
"#;

        let result = parser.parse(content, "test.md");
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.classes.len(), 1);
        assert_eq!(data.classes[0].iri, "ai:Test");
        assert_eq!(data.axioms.len(), 1);
        assert_eq!(data.class_hierarchy.len(), 1);
    }
}
