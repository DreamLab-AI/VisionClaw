use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Expanded IRI of `rdfs:subClassOf`, the only predicate that makes an edge a
/// class-subsumption edge (see [`Edge::asserts_subsumption`]).
pub const RDFS_SUBCLASS_OF_IRI: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// Edge type of a synthetic domain root's edge to one of its member pages.
///
/// Membership groups pages under a domain; it says nothing about subsumption,
/// so it carries its own label rather than the `hierarchical` force category.
pub const DOMAIN_MEMBER_EDGE_TYPE: &str = "domain_member";

/// Edge type of a sidechain payment between two agent nodes (one edge per
/// sidechain transaction, payer → payee, keyed by the agents' `did:nostr`).
///
/// A payment ties two agents together the way has-part ties parts, so it uses
/// the Structural force category; it is never subsumption, so it carries its own
/// label rather than `hierarchical` or `domain_member` (ADR-2035).
pub const CHAIN_PAYMENT_EDGE_TYPE: &str = "chain_payment";

/// GPU-friendly edge type for semantic pipeline (ADR-014 Phase 2).
/// Maps relationship strings from Oxigraph/OntologyParser to a compact u8
/// discriminant suitable for GPU buffers and spring-force differentiation.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SemanticEdgeType {
    /// [[wikilink]] — standard spring
    #[default]
    ExplicitLink = 0,
    /// is-subclass-of (rdfs:subClassOf) — strong hierarchy pull
    Hierarchical = 1,
    /// has-part, is-part-of — medium clustering
    Structural = 2,
    /// requires, depends-on, enables — medium clustering
    Dependency = 3,
    /// relates-to — gentle grouping
    Associative = 4,
    /// bridges-to, bridges-from — cross-domain (weaker)
    Bridge = 5,
    /// shared namespace prefix grouping
    Namespace = 6,
    /// whelk reasoner output
    Inferred = 7,
    /// implements/implementedBy — strong coupling
    Implements = 8,
    /// enhances/enhancedBy, optimizes/optimizedBy — medium
    Enhancement = 9,
    /// secures/securedBy, validates/validatedBy — medium
    Security = 10,
    /// achievesObjective/isAchievedBy — medium
    Goal = 11,
    /// trackedOn/tracks, freezes/frozenBy — weak cross-domain
    Tracking = 12,
    /// similarTo, simulatedIn/simulates — gentle
    Similarity = 13,
    /// prov:wasAttributedTo, prov:wasDerivedFrom — metadata
    Provenance = 14,
    /// uses, supports, utilises — weaker than dependency
    Utilisation = 15,
    /// standardizedBy — governance link
    Standardisation = 16,
}

impl SemanticEdgeType {
    /// Convert an Oxigraph relation_type or OntologyParser relationship string
    /// into the corresponding SemanticEdgeType variant.
    pub fn from_relation_type(s: &str) -> Self {
        match s {
            "is_subclass_of" | "subclass_of" | "hierarchical" | "SUBCLASS_OF"
            | "equivalent_class" | "same_as" | "sub_property_of" => Self::Hierarchical,
            "has_part" | "is_part_of" | "structural" | "domain" | "range" => Self::Structural,
            "requires" | "depends_on" | "enables" | "dependency" => Self::Dependency,
            "relates_to" | "associative" | "inverse_of" | "related_to" => Self::Associative,
            "bridges_to" | "bridges_from" | "bridge" | "disjoint_with" | "contrasts_with" => {
                Self::Bridge
            }
            "namespace" => Self::Namespace,
            // Domain membership groups pages around their domain root, the way
            // has-part groups parts: structural, never subsumption.
            "domain_member" => Self::Structural,
            // Agent-to-agent sidechain payments: structural spring, own label.
            "chain_payment" => Self::Structural,
            "inferred" => Self::Inferred,
            "implements" | "implemented_by" => Self::Implements,
            "enhances" | "enhanced_by" | "optimizes" | "optimized_by" | "enhancement" => {
                Self::Enhancement
            }
            "secures" | "secured_by" | "validates" | "validated_by" | "security" => Self::Security,
            "achieves_objective" | "is_achieved_by" | "goal" => Self::Goal,
            "tracked_on" | "tracks" | "freezes" | "frozen_by" | "tracking" => Self::Tracking,
            "similar_to" | "simulated_in" | "simulates" | "similarity" => Self::Similarity,
            "provenance" | "was_attributed_to" | "was_derived_from" | "was_generated_by" => {
                Self::Provenance
            }
            "uses" | "supports" | "utilises" | "utilisation" | "enabled_by" => Self::Utilisation,
            "standardized_by" | "standardisation" => Self::Standardisation,
            _ => Self::ExplicitLink,
        }
    }

    /// Spring strength multiplier for force-directed layout.
    /// Higher values produce tighter springs (shorter rest length).
    pub fn spring_multiplier(&self) -> f32 {
        match self {
            Self::Hierarchical => 2.0,
            Self::Structural | Self::Dependency => 1.5,
            Self::ExplicitLink | Self::Associative => 1.0,
            Self::Inferred => 0.8,
            Self::Bridge => 0.5,
            Self::Namespace => 0.5,
            Self::Implements => 1.8,
            Self::Enhancement => 1.2,
            Self::Security => 1.3,
            Self::Goal => 1.0,
            Self::Tracking => 0.6,
            Self::Similarity => 0.8,
            Self::Provenance => 0.4,
            Self::Utilisation => 1.1,
            Self::Standardisation => 1.3,
        }
    }

    /// Convert from raw u8 discriminant (e.g. read back from GPU buffer).
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::ExplicitLink,
            1 => Self::Hierarchical,
            2 => Self::Structural,
            3 => Self::Dependency,
            4 => Self::Associative,
            5 => Self::Bridge,
            6 => Self::Namespace,
            7 => Self::Inferred,
            8 => Self::Implements,
            9 => Self::Enhancement,
            10 => Self::Security,
            11 => Self::Goal,
            12 => Self::Tracking,
            13 => Self::Similarity,
            14 => Self::Provenance,
            15 => Self::Utilisation,
            16 => Self::Standardisation,
            _ => Self::ExplicitLink,
        }
    }
}

impl std::fmt::Display for SemanticEdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExplicitLink => write!(f, "explicit_link"),
            Self::Hierarchical => write!(f, "hierarchical"),
            Self::Structural => write!(f, "structural"),
            Self::Dependency => write!(f, "dependency"),
            Self::Associative => write!(f, "associative"),
            Self::Bridge => write!(f, "bridge"),
            Self::Namespace => write!(f, "namespace"),
            Self::Inferred => write!(f, "inferred"),
            Self::Implements => write!(f, "implements"),
            Self::Enhancement => write!(f, "enhancement"),
            Self::Security => write!(f, "security"),
            Self::Goal => write!(f, "goal"),
            Self::Tracking => write!(f, "tracking"),
            Self::Similarity => write!(f, "similarity"),
            Self::Provenance => write!(f, "provenance"),
            Self::Utilisation => write!(f, "utilisation"),
            Self::Standardisation => write!(f, "standardisation"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub source: u32,
    pub target: u32,
    pub weight: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub owl_property_iri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl Edge {
    pub fn new(source: u32, target: u32, weight: f32) -> Self {
        let id = format!("{}-{}", source, target);
        Self {
            id,
            source,
            target,
            weight,
            edge_type: None,
            owl_property_iri: None,
            metadata: None,
        }
    }

    pub fn with_owl_property_iri(mut self, iri: String) -> Self {
        self.owl_property_iri = Some(iri);
        self
    }

    pub fn with_edge_type(mut self, edge_type: String) -> Self {
        self.edge_type = Some(edge_type);
        self
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn add_metadata(mut self, key: String, value: String) -> Self {
        if let Some(ref mut map) = self.metadata {
            map.insert(key, value);
        } else {
            let mut map = HashMap::new();
            map.insert(key, value);
            self.metadata = Some(map);
        }
        self
    }

    /// True when this edge asserts class subsumption: `source rdfs:subClassOf
    /// target`, so `source` is the child and `target` the parent.
    ///
    /// `edge_type` is a force category, and the ingest folds several predicates
    /// into `hierarchical` (`rdfs:subClassOf`, `owl:equivalentClass`,
    /// `owl:sameAs`, `rdfs:subPropertyOf`, instance-of). The label alone cannot
    /// say which, so a `hierarchical` edge counts only when its
    /// `owl_property_iri` provenance is `rdfs:subClassOf`. The explicit
    /// subclass labels (`subclass_of` and kin) name the relation themselves.
    pub fn asserts_subsumption(&self) -> bool {
        match self.edge_type.as_deref() {
            Some("is_subclass_of" | "subclass_of" | "SUBCLASS_OF") => true,
            Some("hierarchical" | "HIERARCHICAL") => {
                self.owl_property_iri.as_deref() == Some(RDFS_SUBCLASS_OF_IRI)
            }
            _ => false,
        }
    }

    /// Derive the SemanticEdgeType from this edge's `edge_type` string field.
    /// Falls back to `ExplicitLink` when `edge_type` is None.
    pub fn semantic_edge_type(&self) -> SemanticEdgeType {
        match &self.edge_type {
            Some(t) => SemanticEdgeType::from_relation_type(t),
            None => SemanticEdgeType::ExplicitLink,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SemanticEdgeType ---

    #[test]
    fn semantic_edge_type_default_is_explicit_link() {
        assert_eq!(SemanticEdgeType::default(), SemanticEdgeType::ExplicitLink);
    }

    #[test]
    fn semantic_edge_type_from_relation_type_known_variants() {
        assert_eq!(
            SemanticEdgeType::from_relation_type("is_subclass_of"),
            SemanticEdgeType::Hierarchical
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("SUBCLASS_OF"),
            SemanticEdgeType::Hierarchical
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("has_part"),
            SemanticEdgeType::Structural
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("requires"),
            SemanticEdgeType::Dependency
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("relates_to"),
            SemanticEdgeType::Associative
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("bridges_to"),
            SemanticEdgeType::Bridge
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("namespace"),
            SemanticEdgeType::Namespace
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("inferred"),
            SemanticEdgeType::Inferred
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("implements"),
            SemanticEdgeType::Implements
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("enhances"),
            SemanticEdgeType::Enhancement
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("secures"),
            SemanticEdgeType::Security
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("goal"),
            SemanticEdgeType::Goal
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("tracks"),
            SemanticEdgeType::Tracking
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("similar_to"),
            SemanticEdgeType::Similarity
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("provenance"),
            SemanticEdgeType::Provenance
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("uses"),
            SemanticEdgeType::Utilisation
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("standardized_by"),
            SemanticEdgeType::Standardisation
        );
    }

    #[test]
    fn semantic_edge_type_from_relation_type_unknown_falls_back_to_explicit_link() {
        assert_eq!(
            SemanticEdgeType::from_relation_type(""),
            SemanticEdgeType::ExplicitLink
        );
        assert_eq!(
            SemanticEdgeType::from_relation_type("unknown_type"),
            SemanticEdgeType::ExplicitLink
        );
    }

    #[test]
    fn semantic_edge_type_from_u8_roundtrip_all_variants() {
        let variants = [
            (0u8, SemanticEdgeType::ExplicitLink),
            (1, SemanticEdgeType::Hierarchical),
            (2, SemanticEdgeType::Structural),
            (3, SemanticEdgeType::Dependency),
            (4, SemanticEdgeType::Associative),
            (5, SemanticEdgeType::Bridge),
            (6, SemanticEdgeType::Namespace),
            (7, SemanticEdgeType::Inferred),
            (8, SemanticEdgeType::Implements),
            (9, SemanticEdgeType::Enhancement),
            (10, SemanticEdgeType::Security),
            (11, SemanticEdgeType::Goal),
            (12, SemanticEdgeType::Tracking),
            (13, SemanticEdgeType::Similarity),
            (14, SemanticEdgeType::Provenance),
            (15, SemanticEdgeType::Utilisation),
            (16, SemanticEdgeType::Standardisation),
        ];
        for (byte, expected) in variants {
            assert_eq!(SemanticEdgeType::from_u8(byte), expected, "byte {}", byte);
        }
        // out-of-range falls back
        assert_eq!(
            SemanticEdgeType::from_u8(255),
            SemanticEdgeType::ExplicitLink
        );
    }

    #[test]
    fn chain_payment_is_structural_and_never_subsumption() {
        assert_eq!(
            SemanticEdgeType::from_relation_type(CHAIN_PAYMENT_EDGE_TYPE),
            SemanticEdgeType::Structural
        );
        let e = Edge::new(1, 2, 1.0).with_edge_type(CHAIN_PAYMENT_EDGE_TYPE.to_string());
        assert!(!e.asserts_subsumption());
        assert_ne!(CHAIN_PAYMENT_EDGE_TYPE, DOMAIN_MEMBER_EDGE_TYPE);
        assert_ne!(CHAIN_PAYMENT_EDGE_TYPE, "hierarchical");
    }

    #[test]
    fn semantic_edge_type_spring_multiplier_hierarchical_is_highest() {
        assert!(
            SemanticEdgeType::Hierarchical.spring_multiplier()
                > SemanticEdgeType::ExplicitLink.spring_multiplier()
        );
        assert!(
            SemanticEdgeType::Hierarchical.spring_multiplier()
                > SemanticEdgeType::Bridge.spring_multiplier()
        );
        assert!(SemanticEdgeType::Provenance.spring_multiplier() < 1.0);
    }

    #[test]
    fn semantic_edge_type_display() {
        assert_eq!(SemanticEdgeType::ExplicitLink.to_string(), "explicit_link");
        assert_eq!(SemanticEdgeType::Hierarchical.to_string(), "hierarchical");
        assert_eq!(
            SemanticEdgeType::Standardisation.to_string(),
            "standardisation"
        );
    }

    #[test]
    fn semantic_edge_type_serde_roundtrip() {
        let t = SemanticEdgeType::Implements;
        let json = serde_json::to_string(&t).unwrap();
        let back: SemanticEdgeType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    // --- Edge ---

    #[test]
    fn edge_new_sets_expected_id_format() {
        let e = Edge::new(1, 2, 0.5);
        assert_eq!(e.id, "1-2");
        assert_eq!(e.source, 1);
        assert_eq!(e.target, 2);
        assert!((e.weight - 0.5).abs() < f32::EPSILON);
        assert!(e.edge_type.is_none());
        assert!(e.owl_property_iri.is_none());
        assert!(e.metadata.is_none());
    }

    #[test]
    fn edge_builder_methods_work() {
        let mut meta = HashMap::new();
        meta.insert("k".to_string(), "v".to_string());

        let e = Edge::new(10, 20, 1.0)
            .with_edge_type("hierarchical".to_string())
            .with_owl_property_iri("http://example.org/prop".to_string())
            .with_metadata(meta);

        assert_eq!(e.edge_type.as_deref(), Some("hierarchical"));
        assert_eq!(
            e.owl_property_iri.as_deref(),
            Some("http://example.org/prop")
        );
        assert!(e.metadata.as_ref().unwrap().contains_key("k"));
    }

    #[test]
    fn edge_add_metadata_creates_map_when_none() {
        let e = Edge::new(1, 2, 1.0).add_metadata("foo".to_string(), "bar".to_string());
        let m = e.metadata.unwrap();
        assert_eq!(m["foo"], "bar");
    }

    #[test]
    fn edge_add_metadata_extends_existing_map() {
        let e = Edge::new(1, 2, 1.0)
            .add_metadata("a".to_string(), "1".to_string())
            .add_metadata("b".to_string(), "2".to_string());
        let m = e.metadata.unwrap();
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn edge_semantic_edge_type_no_edge_type_is_explicit_link() {
        let e = Edge::new(1, 2, 1.0);
        assert_eq!(e.semantic_edge_type(), SemanticEdgeType::ExplicitLink);
    }

    #[test]
    fn edge_semantic_edge_type_derives_from_string() {
        let e = Edge::new(1, 2, 1.0).with_edge_type("is_subclass_of".to_string());
        assert_eq!(e.semantic_edge_type(), SemanticEdgeType::Hierarchical);
    }

    #[test]
    fn edge_serde_roundtrip() {
        let e = Edge::new(5, 10, 2.5).with_edge_type("dependency".to_string());
        let json = serde_json::to_string(&e).unwrap();
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, 5);
        assert_eq!(back.target, 10);
        assert!((back.weight - 2.5).abs() < f32::EPSILON);
        assert_eq!(back.edge_type.as_deref(), Some("dependency"));
    }

    #[test]
    fn edge_serde_omits_none_fields() {
        let e = Edge::new(1, 2, 1.0);
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("edgeType"));
        assert!(!json.contains("owlPropertyIri"));
        assert!(!json.contains("metadata"));
    }
}
