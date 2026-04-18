use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size: usize,
    #[serde(default)]
    pub node_size: f64,
    #[serde(default)]
    pub hyperlink_count: usize,
    #[serde(default)]
    pub sha1: String,
    #[serde(default = "default_node_id")]
    pub node_id: String,
    #[serde(default = "Utc::now")]
    pub last_modified: DateTime<Utc>,
    #[serde(default)]
    pub last_content_change: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_commit: Option<DateTime<Utc>>,
    #[serde(default)]
    pub change_count: Option<u32>,
    #[serde(default)]
    pub file_blob_sha: Option<String>,
    #[serde(default)]
    pub perplexity_link: String,
    #[serde(default)]
    pub last_perplexity_process: Option<DateTime<Utc>>,
    #[serde(default)]
    pub topic_counts: HashMap<String, usize>,
    // Ontology fields from new header format
    #[serde(default)]
    pub term_id: Option<String>,
    #[serde(default)]
    pub preferred_term: Option<String>,
    #[serde(default)]
    pub source_domain: Option<String>,
    #[serde(default)]
    pub ontology_status: Option<String>,
    #[serde(default)]
    pub owl_class: Option<String>,
    #[serde(default)]
    pub owl_physicality: Option<String>,
    #[serde(default)]
    pub owl_role: Option<String>,
    #[serde(default)]
    pub quality_score: Option<f64>,
    #[serde(default)]
    pub authority_score: Option<f64>,
    #[serde(default)]
    pub belongs_to_domain: Vec<String>,
    #[serde(default)]
    pub maturity: Option<String>,
    #[serde(default)]
    pub is_subclass_of: Vec<String>,
    #[serde(default)]
    pub definition: Option<String>,
}

/// Physicality classification for OWL-annotated nodes.
///
/// Maps `owl:physicality::` Logseq property values to the integer codes expected
/// by the CUDA semantic-forces kernel (per 2026-04-18 corpus survey).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum PhysicalityCode {
    /// No physicality property present — kernel skips this dimension.
    #[default]
    None = 0,
    Abstract = 1,
    Virtual = 2,
    Conceptual = 3,
    /// Property present but value unrecognised.
    Unknown = 255,
}

impl PhysicalityCode {
    /// Parse a raw Logseq property value into a `PhysicalityCode`.
    ///
    /// Empty string → `None`; recognised value → variant; anything else → `Unknown`.
    pub fn from_logseq(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "" => Self::None,
            "abstract" => Self::Abstract,
            "virtual" | "virtualentity" => Self::Virtual,
            "conceptual" | "conceptualentity" => Self::Conceptual,
            _ => Self::Unknown,
        }
    }

    /// Signed 32-bit representation suitable for GPU buffer insertion.
    pub fn as_i32(self) -> i32 {
        self as u8 as i32
    }
}

/// Role classification for OWL-annotated nodes.
///
/// Maps `owl:role::` Logseq property values (919 Concept, 307 Object, 166 Process,
/// 17 Domain, 11 Method, 11 Agent after case-merge in the 2026-04-18 corpus).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum RoleCode {
    /// No role property present — kernel skips this dimension.
    #[default]
    None = 0,
    Concept = 1,
    Object = 2,
    Process = 3,
    Domain = 4,
    Method = 5,
    Agent = 6,
    /// Property present but value unrecognised.
    Unknown = 255,
}

impl RoleCode {
    /// Parse a raw Logseq property value into a `RoleCode`.
    pub fn from_logseq(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "" => Self::None,
            "concept" => Self::Concept,
            "object" => Self::Object,
            "process" => Self::Process,
            "domain" => Self::Domain,
            "method" => Self::Method,
            "agent" => Self::Agent,
            // "other" is an observed corpus value with no good mapping
            "other" => Self::Unknown,
            _ => Self::Unknown,
        }
    }

    /// Signed 32-bit representation suitable for GPU buffer insertion.
    pub fn as_i32(self) -> i32 {
        self as u8 as i32
    }
}

/// Maturity classification derived from `maturity::` or `status::` Logseq properties.
///
/// Observed values are grouped as follows (2026-04-18 corpus survey):
/// - Emerging  → draft, developing, emerging
/// - Mature    → mature, established, reviewed, stable, production
/// - Declining → production-ready, deprecated
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum MaturityLevel {
    /// No maturity/status property present.
    #[default]
    None = 0,
    Emerging = 1,
    Mature = 2,
    Declining = 3,
    /// Property present but value unrecognised.
    Unknown = 255,
}

impl MaturityLevel {
    /// Parse a raw Logseq property value into a `MaturityLevel`.
    pub fn from_logseq(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "" => Self::None,
            "draft" | "developing" | "emerging" => Self::Emerging,
            "mature" | "established" | "reviewed" | "stable" | "production" => Self::Mature,
            "production-ready" | "deprecated" => Self::Declining,
            _ => Self::Unknown,
        }
    }

    /// Signed 32-bit representation suitable for GPU buffer insertion.
    pub fn as_i32(self) -> i32 {
        self as u8 as i32
    }
}

// Default function for node_id to ensure backward compatibility
fn default_node_id() -> String {

    "0".to_string()
}

pub type MetadataStore = HashMap<String, Metadata>;

pub type FileMetadata = Metadata;

#[cfg(test)]
mod tests {
    use super::*;

    // ── PhysicalityCode ──────────────────────────────────────────────────────

    #[test]
    fn physicality_empty_is_none() {
        assert_eq!(PhysicalityCode::from_logseq(""), PhysicalityCode::None);
        assert_eq!(PhysicalityCode::from_logseq("  "), PhysicalityCode::None);
    }

    #[test]
    fn physicality_abstract() {
        assert_eq!(PhysicalityCode::from_logseq("abstract"), PhysicalityCode::Abstract);
        assert_eq!(PhysicalityCode::from_logseq("Abstract"), PhysicalityCode::Abstract);
    }

    #[test]
    fn physicality_virtual() {
        assert_eq!(PhysicalityCode::from_logseq("virtual"), PhysicalityCode::Virtual);
        assert_eq!(PhysicalityCode::from_logseq("VirtualEntity"), PhysicalityCode::Virtual);
    }

    #[test]
    fn physicality_conceptual() {
        assert_eq!(PhysicalityCode::from_logseq("conceptual"), PhysicalityCode::Conceptual);
        // Observed corpus value (3 occurrences)
        assert_eq!(PhysicalityCode::from_logseq("ConceptualEntity"), PhysicalityCode::Conceptual);
    }

    #[test]
    fn physicality_unknown_for_unrecognised() {
        assert_eq!(PhysicalityCode::from_logseq("tangible"), PhysicalityCode::Unknown);
    }

    #[test]
    fn physicality_as_i32() {
        assert_eq!(PhysicalityCode::None.as_i32(), 0);
        assert_eq!(PhysicalityCode::Abstract.as_i32(), 1);
        assert_eq!(PhysicalityCode::Virtual.as_i32(), 2);
        assert_eq!(PhysicalityCode::Conceptual.as_i32(), 3);
        assert_eq!(PhysicalityCode::Unknown.as_i32(), 255);
    }

    // ── RoleCode ─────────────────────────────────────────────────────────────

    #[test]
    fn role_empty_is_none() {
        assert_eq!(RoleCode::from_logseq(""), RoleCode::None);
    }

    #[test]
    fn role_concept() {
        assert_eq!(RoleCode::from_logseq("Concept"), RoleCode::Concept);
        assert_eq!(RoleCode::from_logseq("concept"), RoleCode::Concept);
    }

    #[test]
    fn role_object() {
        assert_eq!(RoleCode::from_logseq("Object"), RoleCode::Object);
    }

    #[test]
    fn role_process() {
        assert_eq!(RoleCode::from_logseq("Process"), RoleCode::Process);
    }

    #[test]
    fn role_domain() {
        assert_eq!(RoleCode::from_logseq("Domain"), RoleCode::Domain);
    }

    #[test]
    fn role_method() {
        assert_eq!(RoleCode::from_logseq("Method"), RoleCode::Method);
    }

    #[test]
    fn role_agent() {
        assert_eq!(RoleCode::from_logseq("Agent"), RoleCode::Agent);
    }

    #[test]
    fn role_other_maps_to_unknown() {
        // "Other" is an observed corpus value; maps to Unknown
        assert_eq!(RoleCode::from_logseq("Other"), RoleCode::Unknown);
    }

    #[test]
    fn role_unknown_for_unrecognised() {
        assert_eq!(RoleCode::from_logseq("gadget"), RoleCode::Unknown);
    }

    #[test]
    fn role_as_i32() {
        assert_eq!(RoleCode::None.as_i32(), 0);
        assert_eq!(RoleCode::Concept.as_i32(), 1);
        assert_eq!(RoleCode::Object.as_i32(), 2);
        assert_eq!(RoleCode::Process.as_i32(), 3);
        assert_eq!(RoleCode::Domain.as_i32(), 4);
        assert_eq!(RoleCode::Method.as_i32(), 5);
        assert_eq!(RoleCode::Agent.as_i32(), 6);
        assert_eq!(RoleCode::Unknown.as_i32(), 255);
    }

    // ── MaturityLevel ────────────────────────────────────────────────────────

    #[test]
    fn maturity_empty_is_none() {
        assert_eq!(MaturityLevel::from_logseq(""), MaturityLevel::None);
    }

    #[test]
    fn maturity_emerging_group() {
        assert_eq!(MaturityLevel::from_logseq("draft"), MaturityLevel::Emerging);
        assert_eq!(MaturityLevel::from_logseq("developing"), MaturityLevel::Emerging);
        assert_eq!(MaturityLevel::from_logseq("emerging"), MaturityLevel::Emerging);
    }

    #[test]
    fn maturity_mature_group() {
        assert_eq!(MaturityLevel::from_logseq("mature"), MaturityLevel::Mature);
        assert_eq!(MaturityLevel::from_logseq("established"), MaturityLevel::Mature);
        assert_eq!(MaturityLevel::from_logseq("reviewed"), MaturityLevel::Mature);
        assert_eq!(MaturityLevel::from_logseq("stable"), MaturityLevel::Mature);
        assert_eq!(MaturityLevel::from_logseq("production"), MaturityLevel::Mature);
    }

    #[test]
    fn maturity_declining_group() {
        assert_eq!(MaturityLevel::from_logseq("production-ready"), MaturityLevel::Declining);
        assert_eq!(MaturityLevel::from_logseq("deprecated"), MaturityLevel::Declining);
    }

    #[test]
    fn maturity_unknown_for_unrecognised() {
        assert_eq!(MaturityLevel::from_logseq("experimental"), MaturityLevel::Unknown);
    }

    #[test]
    fn maturity_case_insensitive() {
        assert_eq!(MaturityLevel::from_logseq("DRAFT"), MaturityLevel::Emerging);
        assert_eq!(MaturityLevel::from_logseq("Mature"), MaturityLevel::Mature);
    }

    #[test]
    fn maturity_as_i32() {
        assert_eq!(MaturityLevel::None.as_i32(), 0);
        assert_eq!(MaturityLevel::Emerging.as_i32(), 1);
        assert_eq!(MaturityLevel::Mature.as_i32(), 2);
        assert_eq!(MaturityLevel::Declining.as_i32(), 3);
        assert_eq!(MaturityLevel::Unknown.as_i32(), 255);
    }
}

// Implement helper methods directly on HashMap<String, Metadata>
pub trait MetadataOps {
    fn validate_files(&self, markdown_dir: &str) -> bool;
    fn get_max_node_id(&self) -> u32;
}

impl MetadataOps for MetadataStore {
    fn get_max_node_id(&self) -> u32 {
        
        self.values()
            .map(|m| m.node_id.parse::<u32>().unwrap_or(0))
            .max()
            .unwrap_or(0)
    }

    fn validate_files(&self, markdown_dir: &str) -> bool {
        if self.is_empty() {
            return false;
        }

        
        for filename in self.keys() {
            let file_path = format!("{}/{}", markdown_dir, filename);
            if !std::path::Path::new(&file_path).exists() {
                return false;
            }
        }

        true
    }
}
