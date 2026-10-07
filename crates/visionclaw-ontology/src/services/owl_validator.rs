use crate::utils::time;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::ontology::set::SetOntology;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Failed to parse ontology: {0}")]
    ParseError(String),

    #[error("RDF processing error: {0}")]
    RdfError(String),

    #[error("Reasoning timeout after {0:?}")]
    TimeoutError(Duration),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Invalid IRI: {0}")]
    InvalidIri(String),

    #[error("Cache error: {0}")]
    CacheError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdfTriple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub is_literal: bool,
    pub datatype: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub id: String,
    pub severity: Severity,
    pub rule: String,
    pub message: String,
    pub subject: Option<String>,
    pub predicate: Option<String>,
    pub object: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Constraint summary for validation reports
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConstraintSummary {
    pub total_constraints: usize,
    pub semantic_constraints: usize,
    pub structural_constraints: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
    pub graph_signature: String,
    pub total_triples: usize,
    pub violations: Vec<Violation>,
    pub inferred_triples: Vec<RdfTriple>,
    pub statistics: ValidationStatistics,
    pub constraint_summary: ConstraintSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationStatistics {
    pub classes_checked: usize,
    pub properties_checked: usize,
    pub individuals_checked: usize,
    pub constraints_evaluated: usize,
    pub inference_rules_applied: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub labels: Vec<String>,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relationship_type: String,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
struct CachedOntology {
    ontology: SetOntology<Arc<str>>,
    loaded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    pub enable_reasoning: bool,
    pub reasoning_timeout_seconds: u64,
    pub enable_inference: bool,
    pub max_inference_depth: usize,
    pub enable_caching: bool,
    pub cache_ttl_seconds: u64,
    pub validate_cardinality: bool,
    pub validate_domains_ranges: bool,
    pub validate_disjoint_classes: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_reasoning: true,
            reasoning_timeout_seconds: 30,
            enable_inference: true,
            max_inference_depth: 3,
            enable_caching: true,
            cache_ttl_seconds: 3600,
            validate_cardinality: true,
            validate_domains_ranges: true,
            validate_disjoint_classes: true,
        }
    }
}

#[derive(Clone)]
pub struct OwlValidatorService {
    ontology_cache: Arc<DashMap<String, CachedOntology>>,
    validation_cache: Arc<DashMap<String, ValidationReport>>,
    config: ValidationConfig,
    default_namespaces: HashMap<String, String>,
    inference_rules: Vec<InferenceRule>,
}

#[derive(Debug, Clone)]
enum InferenceRule {
    Inverse { property: String, inverse: String },
    Transitive { property: String },
    Symmetric { property: String },
}

impl OwlValidatorService {
    pub fn new() -> Self {
        Self::with_config(ValidationConfig::default())
    }

    pub fn with_config(config: ValidationConfig) -> Self {
        let mut default_namespaces = HashMap::new();
        default_namespaces.insert(
            "rdf".to_string(),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
        );
        default_namespaces.insert(
            "rdfs".to_string(),
            "http://www.w3.org/2000/01/rdf-schema#".to_string(),
        );
        default_namespaces.insert(
            "owl".to_string(),
            "http://www.w3.org/2002/07/owl#".to_string(),
        );
        default_namespaces.insert(
            "xsd".to_string(),
            "http://www.w3.org/2001/XMLSchema#".to_string(),
        );
        default_namespaces.insert("foaf".to_string(), "http://xmlns.com/foaf/0.1/".to_string());

        let inference_rules = vec![
            InferenceRule::Inverse {
                property: "http://example.org/employs".to_string(),
                inverse: "http://example.org/worksFor".to_string(),
            },
            InferenceRule::Transitive {
                property: "http://example.org/partOf".to_string(),
            },
            InferenceRule::Symmetric {
                property: "http://example.org/knows".to_string(),
            },
        ];

        Self {
            ontology_cache: Arc::new(DashMap::new()),
            validation_cache: Arc::new(DashMap::new()),
            config,
            default_namespaces,
            inference_rules,
        }
    }

    pub async fn load_ontology(&self, source: &str) -> Result<String> {
        let start_time = Instant::now();

        info!(
            "Loading ontology from: {}",
            if source.len() > 100 {
                &source[..100]
            } else {
                source
            }
        );

        let ontology_content = if source.starts_with("http://") || source.starts_with("https://") {
            self.load_from_url(source).await?
        } else if std::path::Path::new(source).exists() {
            self.load_from_file(source)?
        } else {
            source.to_string()
        };

        let content_hash = self.calculate_signature(&ontology_content);
        let ontology_id = format!("ontology_{}", content_hash);

        if self.config.enable_caching {
            if let Some(cached) = self.ontology_cache.get(&ontology_id) {
                let age = time::now().signed_duration_since(cached.loaded_at);
                if age.num_seconds() < (self.config.cache_ttl_seconds as i64) {
                    debug!("Cache hit for ontology: {}", ontology_id);
                    return Ok(ontology_id);
                } else {
                    debug!("Cache expired for ontology: {}", ontology_id);
                }
            } else {
                debug!("Cache miss for ontology: {}", ontology_id);
            }
        }

        let ontology = self.parse_ontology(&ontology_content)?;
        let axiom_count = ontology.iter().count();

        info!("Parsed ontology with {} axioms", axiom_count);

        if self.config.enable_caching {
            let cached = CachedOntology {
                ontology,
                loaded_at: time::now(),
            };
            self.ontology_cache.insert(ontology_id.clone(), cached);
        }

        let duration = start_time.elapsed();
        info!("Ontology loaded in {:?}: {}", duration, ontology_id);

        Ok(ontology_id)
    }

    pub fn map_graph_to_rdf(&self, graph_data: &PropertyGraph) -> Result<Vec<RdfTriple>> {
        let mut triples = Vec::new();

        for node in &graph_data.nodes {
            for label in &node.labels {
                if self.is_iri_shaped(label) {
                    // A genuine type reference (registered CURIE, known absolute
                    // scheme, or a clean bare-local slug) → rdf:type IRI.
                    triples.push(RdfTriple {
                        subject: self.expand_iri_lenient(&node.id),
                        predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
                        object: self.expand_iri_lenient(label),
                        is_literal: false,
                        datatype: None,
                        language: None,
                    });
                } else {
                    // A human display name (whitespace, or a colon-bearing title
                    // like "ETSI Domain: Data Management + AI") is NOT a type —
                    // emit it as an rdfs:label literal. Never expand_iri it as an
                    // rd:type object, which raised "Unknown prefix" on such titles
                    // and mis-asserted display names as classes in the reasoned graph.
                    triples.push(RdfTriple {
                        subject: self.expand_iri_lenient(&node.id),
                        predicate: "http://www.w3.org/2000/01/rdf-schema#label".to_string(),
                        object: label.clone(),
                        is_literal: true,
                        datatype: None,
                        language: None,
                    });
                }
            }

            for (prop_name, prop_value) in &node.properties {
                let (object, is_literal, datatype, language) =
                    self.serialize_property_value(prop_value)?;
                triples.push(RdfTriple {
                    subject: self.expand_iri_lenient(&node.id),
                    predicate: self.expand_iri_lenient(prop_name),
                    object,
                    is_literal,
                    datatype,
                    language,
                });
            }
        }

        for edge in &graph_data.edges {
            triples.push(RdfTriple {
                subject: self.expand_iri_lenient(&edge.source),
                predicate: self.expand_iri_lenient(&edge.relationship_type),
                object: self.expand_iri_lenient(&edge.target),
                is_literal: false,
                datatype: None,
                language: None,
            });

            for (prop_name, prop_value) in &edge.properties {
                let (object, is_literal, datatype, language) =
                    self.serialize_property_value(prop_value)?;
                triples.push(RdfTriple {
                    subject: self.expand_iri_lenient(&edge.id),
                    predicate: self.expand_iri_lenient(prop_name),
                    object,
                    is_literal,
                    datatype,
                    language,
                });
            }
        }

        debug!(
            "Mapped {} nodes and {} edges to {} RDF triples",
            graph_data.nodes.len(),
            graph_data.edges.len(),
            triples.len()
        );

        Ok(triples)
    }

    pub async fn validate(
        &self,
        ontology_id: &str,
        graph_data: &PropertyGraph,
    ) -> Result<ValidationReport> {
        let start_time = Instant::now();
        let graph_signature = self.calculate_graph_signature(graph_data);

        let cache_key = format!("{}:{}", ontology_id, graph_signature);
        if self.config.enable_caching {
            if let Some(cached_report) = self.validation_cache.get(&cache_key) {
                let age = time::now().signed_duration_since(cached_report.timestamp);
                if age.num_seconds() < (self.config.cache_ttl_seconds as i64) {
                    debug!("Using cached validation report");
                    return Ok(cached_report.clone());
                }
            }
        }

        info!(
            "Starting validation for graph with {} nodes, {} edges",
            graph_data.nodes.len(),
            graph_data.edges.len()
        );

        let cached_ontology = self.ontology_cache.get(ontology_id).ok_or_else(|| {
            ValidationError::CacheError(format!("Ontology not found: {}", ontology_id))
        })?;

        let rdf_triples = self.map_graph_to_rdf(graph_data)?;

        let mut violations = Vec::new();
        let mut statistics = ValidationStatistics {
            classes_checked: 0,
            properties_checked: 0,
            individuals_checked: 0,
            constraints_evaluated: 0,
            inference_rules_applied: 0,
            cache_hits: 0,
            cache_misses: 0,
        };

        if self.config.validate_disjoint_classes {
            violations
                .extend(self.validate_disjoint_classes(&cached_ontology.ontology, &rdf_triples)?);
            statistics.constraints_evaluated += 1;
        }

        if self.config.validate_domains_ranges {
            violations.extend(self.validate_domain_range(&cached_ontology.ontology, &rdf_triples)?);
            statistics.constraints_evaluated += 1;
        }

        if self.config.validate_cardinality {
            violations.extend(self.validate_cardinality(&cached_ontology.ontology, &rdf_triples)?);
            statistics.constraints_evaluated += 1;
        }

        let inferred_triples = if self.config.enable_inference {
            self.infer_triples(&rdf_triples, &mut statistics)?
        } else {
            Vec::new()
        };

        let duration = start_time.elapsed();

        // Calculate constraint summary from statistics
        let constraint_summary = ConstraintSummary {
            total_constraints: statistics.constraints_evaluated,
            semantic_constraints: statistics.inference_rules_applied,
            structural_constraints: statistics
                .constraints_evaluated
                .saturating_sub(statistics.inference_rules_applied),
        };

        let report = ValidationReport {
            id: Uuid::new_v4().to_string(),
            timestamp: time::now(),
            duration_ms: duration.as_millis() as u64,
            graph_signature,
            total_triples: rdf_triples.len(),
            violations,
            inferred_triples,
            statistics,
            constraint_summary,
        };

        if self.config.enable_caching {
            self.validation_cache.insert(cache_key, report.clone());
        }

        info!(
            "Validation completed in {:?}: {} violations, {} inferred triples",
            duration,
            report.violations.len(),
            report.inferred_triples.len()
        );

        Ok(report)
    }

    pub fn infer(&self, rdf_triples: &[RdfTriple]) -> Result<Vec<RdfTriple>> {
        let mut statistics = ValidationStatistics::default();
        self.infer_triples(rdf_triples, &mut statistics)
    }

    pub fn get_violations(&self, report_id: &str) -> Vec<Violation> {
        for entry in self.validation_cache.iter() {
            if entry.value().id == report_id {
                return entry.value().violations.clone();
            }
        }
        Vec::new()
    }

    pub fn clear_caches(&self) {
        self.ontology_cache.clear();
        self.validation_cache.clear();
        info!("All caches cleared");
    }

    async fn load_from_url(&self, url: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .header(
                "Accept",
                "application/rdf+xml, text/turtle, application/n-triples",
            )
            .send()
            .await
            .context("Failed to fetch ontology from URL")?;

        let content = response
            .text()
            .await
            .context("Failed to read ontology content")?;

        Ok(content)
    }

    fn load_from_file(&self, path: &str) -> Result<String> {
        std::fs::read_to_string(path).context("Failed to read ontology file")
    }

    fn parse_ontology(&self, content: &str) -> Result<SetOntology<Arc<str>>> {
        let trimmed = content.trim_start();

        debug!("Detecting ontology format...");

        if trimmed.starts_with("@prefix")
            || trimmed.starts_with("@base")
            || trimmed.contains("@prefix")
        {
            error!("Turtle format not supported. Please use OWL Functional Syntax or OWL/XML.");
            Err(ValidationError::ParseError("Turtle format not supported. Use OWL Functional Syntax (starts with 'Prefix(' or 'Ontology(') or OWL/XML.".to_string()).into())
        } else if trimmed.starts_with("<?xml")
            || (trimmed.starts_with("<") && trimmed.contains("rdf:RDF"))
        {
            error!("RDF/XML format not supported. Please use OWL Functional Syntax or OWL/XML.");
            Err(ValidationError::ParseError("RDF/XML format not supported. Use OWL Functional Syntax (starts with 'Prefix(' or 'Ontology(') or OWL/XML.".to_string()).into())
        } else if trimmed.starts_with("Prefix(") || trimmed.starts_with("Ontology(") {
            info!("Detected OWL Functional Syntax");
            self.parse_functional_syntax(content)
        } else if trimmed.starts_with("<Ontology") {
            info!("Detected OWL/XML format");
            self.parse_owx(content)
        } else if trimmed.is_empty() {
            Err(ValidationError::ParseError("Empty ontology content".to_string()).into())
        } else {
            error!("Unsupported or unrecognized ontology format");
            Err(ValidationError::ParseError("Unsupported format. Please use OWL Functional Syntax (starts with 'Prefix(' or 'Ontology(') or OWL/XML (starts with '<Ontology').".to_string()).into())
        }
    }

    fn parse_functional_syntax(&self, content: &str) -> Result<SetOntology<Arc<str>>> {
        let cursor = Cursor::new(content.as_bytes());

        match read_ofn::<Arc<str>, SetOntology<Arc<str>>, _>(cursor, Default::default()) {
            Ok((ontology, _prefixes)) => {
                debug!("Successfully parsed Functional Syntax ontology");
                Ok(ontology)
            }
            Err(e) => {
                error!("Failed to parse Functional Syntax: {}", e);
                Err(
                    ValidationError::ParseError(format!("Functional Syntax parse error: {}", e))
                        .into(),
                )
            }
        }
    }

    fn parse_owx(&self, content: &str) -> Result<SetOntology<Arc<str>>> {
        let mut cursor = Cursor::new(content.as_bytes());

        match read_owx::<Arc<str>, SetOntology<Arc<str>>, _>(&mut cursor, Default::default()) {
            Ok((ontology, _prefixes)) => {
                debug!("Successfully parsed OWL/XML ontology");
                Ok(ontology)
            }
            Err(e) => {
                error!("Failed to parse OWL/XML: {}", e);
                Err(ValidationError::ParseError(format!("OWL/XML parse error: {}", e)).into())
            }
        }
    }

    fn calculate_signature(&self, content: &str) -> String {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    fn calculate_graph_signature(&self, graph: &PropertyGraph) -> String {
        use blake3::Hasher;
        let mut hasher = Hasher::new();

        for node in &graph.nodes {
            hasher.update(node.id.as_bytes());
            for label in &node.labels {
                hasher.update(label.as_bytes());
            }
        }

        for edge in &graph.edges {
            hasher.update(edge.id.as_bytes());
            hasher.update(edge.source.as_bytes());
            hasher.update(edge.target.as_bytes());
            hasher.update(edge.relationship_type.as_bytes());
        }

        hasher.finalize().to_hex().to_string()
    }

    /// Whether a property-graph node "label" is genuinely a type IRI (→ rdf:type)
    /// rather than a human display name (→ rdfs:label literal). A type is
    /// IRI-shaped: it contains no whitespace AND `expand_iri` accepts it (a
    /// registered CURIE, a known absolute scheme, or a clean bare-local slug).
    /// Display names such as "AI Infrastructure" (whitespace) or
    /// "ETSI Domain: Data Management + AI" (colon title → expand_iri Err) are not
    /// types and must not be expanded as rd:type IRIs.
    fn is_iri_shaped(&self, label: &str) -> bool {
        !label.chars().any(char::is_whitespace) && self.expand_iri(label).is_ok()
    }

    /// Total (never-failing) IRI expansion for graph→RDF mapping. Like
    /// `expand_iri`, but an unresolvable value (a `prefix:local` whose prefix is
    /// neither a registered CURIE nor a known absolute scheme, e.g. `ai:foo`)
    /// becomes a safe opaque local IRI instead of an error. `map_graph_to_rdf`
    /// uses this for every mandatory IRI position (subject / predicate / edge
    /// endpoints) so one non-IRI value can never abort the whole mapping via `?`
    /// — otherwise the first odd string fails the entire validation and the real
    /// OWL violations are never reported. Strict `expand_iri` is retained for
    /// callers that must surface an invalid IRI.
    fn expand_iri_lenient(&self, iri: &str) -> String {
        self.expand_iri(iri)
            .unwrap_or_else(|_| format!("http://example.org/{}", iri))
    }

    /// Expand a graph identifier into an absolute IRI.
    ///
    /// Decision rule (in order):
    ///   1. Any string containing `://` is an absolute IRI (hierarchical scheme) → pass through.
    ///   2. If the substring before the first `:` is a *registered* short CURIE prefix
    ///      (rdf, rdfs, owl, xsd, foaf, …) → expand `prefix:local` to `namespace + local`.
    ///   3. Else if the string looks like an absolute IRI — a well-known non-hierarchical
    ///      scheme (urn, did, http, https, ftp, ftps, mailto, tag, file, data) OR a generic
    ///      RFC 3986 scheme followed by a multi-segment remainder (e.g. `scheme:a:b`) →
    ///      pass through unchanged.
    ///   4. If there is no `:` at all, treat it as a bare local name under the default namespace.
    ///   5. Otherwise the prefix is neither a registered CURIE nor a recognised absolute scheme
    ///      → `Unknown prefix` error.
    fn expand_iri(&self, iri: &str) -> Result<String> {
        // (1) Hierarchical absolute IRI (scheme://authority/...) — always absolute.
        if iri.contains("://") {
            return Ok(iri.to_string());
        }

        match iri.find(':') {
            Some(colon_pos) => {
                let (prefix, rest) = iri.split_at(colon_pos);
                let local = &rest[1..];

                // (2) Registered short CURIE prefix → expand.
                if let Some(namespace) = self.default_namespaces.get(prefix) {
                    return Ok(format!("{}{}", namespace, local));
                }

                // (3) Recognised absolute-IRI scheme (urn:, did:, http:, …) → pass through.
                if Self::is_absolute_iri_scheme(prefix, local) {
                    return Ok(iri.to_string());
                }

                // (5) Unknown prefix that is neither a CURIE nor an absolute IRI.
                Err(ValidationError::InvalidIri(format!("Unknown prefix: {}", prefix)).into())
            }
            // (4) Bare local name → default namespace.
            None => Ok(format!("http://example.org/{}", iri)),
        }
    }

    /// Decide whether a `prefix:local` pair (already known **not** to use a registered
    /// CURIE prefix) should be treated as an absolute IRI rather than an error.
    ///
    /// Returns `true` when either:
    ///   * `prefix` is a well-known non-hierarchical absolute-IRI scheme
    ///     (urn, did, http, https, ftp, ftps, mailto, tag, file, data), or
    ///   * `prefix` is a syntactically valid RFC 3986 scheme AND the remainder is
    ///     itself multi-segment (contains a further `:`), which is the shape of
    ///     `urn`-style absolute IRIs such as `scheme:a:b`.
    ///
    /// A bare `unregistered:thing` (single-segment remainder, unknown scheme) returns
    /// `false` so the caller can raise the existing `Unknown prefix` error.
    fn is_absolute_iri_scheme(prefix: &str, local: &str) -> bool {
        const KNOWN_ABSOLUTE_SCHEMES: &[&str] = &[
            "urn", "did", "http", "https", "ftp", "ftps", "mailto", "tag", "file", "data",
        ];

        let scheme = prefix.to_ascii_lowercase();
        if KNOWN_ABSOLUTE_SCHEMES.contains(&scheme.as_str()) {
            return true;
        }

        // Generic RFC 3986 scheme grammar: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ).
        let valid_scheme = {
            let mut chars = prefix.chars();
            match chars.next() {
                Some(c) if c.is_ascii_alphabetic() => {
                    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
                }
                _ => false,
            }
        };

        // Multi-segment remainder distinguishes an absolute IRI (`scheme:a:b`) from a
        // CURIE-shaped `prefix:local`.
        valid_scheme && local.contains(':')
    }

    fn serialize_property_value(
        &self,
        value: &serde_json::Value,
    ) -> Result<(String, bool, Option<String>, Option<String>)> {
        match value {
            serde_json::Value::String(s) => {
                if s.starts_with("http://") || s.starts_with("https://") {
                    Ok((s.clone(), false, None, None))
                } else {
                    Ok((
                        s.clone(),
                        true,
                        Some("http://www.w3.org/2001/XMLSchema#string".to_string()),
                        None,
                    ))
                }
            }
            serde_json::Value::Number(n) => {
                if n.is_i64() {
                    Ok((
                        n.to_string(),
                        true,
                        Some("http://www.w3.org/2001/XMLSchema#integer".to_string()),
                        None,
                    ))
                } else {
                    Ok((
                        n.to_string(),
                        true,
                        Some("http://www.w3.org/2001/XMLSchema#double".to_string()),
                        None,
                    ))
                }
            }
            serde_json::Value::Bool(b) => Ok((
                b.to_string(),
                true,
                Some("http://www.w3.org/2001/XMLSchema#boolean".to_string()),
                None,
            )),
            _ => Ok((
                value.to_string(),
                true,
                Some("http://www.w3.org/2001/XMLSchema#string".to_string()),
                None,
            )),
        }
    }

    fn validate_disjoint_classes(
        &self,
        _ontology: &SetOntology<Arc<str>>,
        triples: &[RdfTriple],
    ) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        let mut individual_types: HashMap<String, Vec<String>> = HashMap::new();

        for triple in triples {
            if triple.predicate == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
                && !triple.is_literal
            {
                individual_types
                    .entry(triple.subject.clone())
                    .or_default()
                    .push(triple.object.clone());
            }
        }

        let disjoint_pairs = vec![
            ("http://example.org/Person", "http://example.org/Company"),
            ("http://example.org/Animal", "http://example.org/Plant"),
        ];

        for (individual, types) in individual_types {
            for (class1, class2) in &disjoint_pairs {
                if types.contains(&class1.to_string()) && types.contains(&class2.to_string()) {
                    violations.push(Violation {
                        id: Uuid::new_v4().to_string(),
                        severity: Severity::Error,
                        rule: "DisjointClasses".to_string(),
                        message: format!(
                            "Individual {} cannot be both {} and {} (disjoint classes)",
                            individual, class1, class2
                        ),
                        subject: Some(individual.clone()),
                        predicate: Some(
                            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
                        ),
                        object: None,
                        timestamp: time::now(),
                    });
                }
            }
        }

        Ok(violations)
    }

    fn validate_domain_range(
        &self,
        _ontology: &SetOntology<Arc<str>>,
        triples: &[RdfTriple],
    ) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        let constraints = vec![
            (
                "http://example.org/employs",
                "http://example.org/Organization",
                "http://example.org/Person",
            ),
            (
                "http://example.org/hasAge",
                "http://example.org/Person",
                "http://www.w3.org/2001/XMLSchema#integer",
            ),
        ];

        let mut individual_types: HashMap<String, Vec<String>> = HashMap::new();
        for triple in triples {
            if triple.predicate == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
                && !triple.is_literal
            {
                individual_types
                    .entry(triple.subject.clone())
                    .or_default()
                    .push(triple.object.clone());
            }
        }

        for triple in triples {
            for (property, domain, range) in &constraints {
                if &triple.predicate == property {
                    if let Some(subject_types) = individual_types.get(&triple.subject) {
                        if !subject_types.contains(&domain.to_string()) {
                            violations.push(Violation {
                                id: Uuid::new_v4().to_string(),
                                severity: Severity::Error,
                                rule: "DomainViolation".to_string(),
                                message: format!(
                                    "Subject {} must be of type {} for property {}",
                                    triple.subject, domain, property
                                ),
                                subject: Some(triple.subject.clone()),
                                predicate: Some(triple.predicate.clone()),
                                object: Some(triple.object.clone()),
                                timestamp: time::now(),
                            });
                        }
                    }

                    if !triple.is_literal {
                        if let Some(object_types) = individual_types.get(&triple.object) {
                            if !object_types.contains(&range.to_string()) {
                                violations.push(Violation {
                                    id: Uuid::new_v4().to_string(),
                                    severity: Severity::Error,
                                    rule: "RangeViolation".to_string(),
                                    message: format!(
                                        "Object {} must be of type {} for property {}",
                                        triple.object, range, property
                                    ),
                                    subject: Some(triple.subject.clone()),
                                    predicate: Some(triple.predicate.clone()),
                                    object: Some(triple.object.clone()),
                                    timestamp: time::now(),
                                });
                            }
                        }
                    } else if triple.is_literal
                        && triple.datatype.as_ref() != Some(&range.to_string())
                    {
                        violations.push(Violation {
                            id: Uuid::new_v4().to_string(),
                            severity: Severity::Error,
                            rule: "RangeViolation".to_string(),
                            message: format!(
                                "Literal {} must have datatype {} for property {}",
                                triple.object, range, property
                            ),
                            subject: Some(triple.subject.clone()),
                            predicate: Some(triple.predicate.clone()),
                            object: Some(triple.object.clone()),
                            timestamp: time::now(),
                        });
                    }
                }
            }
        }

        Ok(violations)
    }

    fn validate_cardinality(
        &self,
        _ontology: &SetOntology<Arc<str>>,
        triples: &[RdfTriple],
    ) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        let cardinality_constraints = vec![
            ("http://example.org/hasSSN", 1, Some(1)),
            ("http://example.org/hasChild", 0, None),
        ];

        let mut property_counts: HashMap<(String, String), usize> = HashMap::new();

        for triple in triples {
            let key = (triple.subject.clone(), triple.predicate.clone());
            *property_counts.entry(key).or_insert(0) += 1;
        }

        for (property, min_card, max_card) in cardinality_constraints {
            let subjects_using_property: HashSet<String> = triples
                .iter()
                .filter(|t| t.predicate == property)
                .map(|t| t.subject.clone())
                .collect();

            for subject in subjects_using_property {
                let count = property_counts
                    .get(&(subject.clone(), property.to_string()))
                    .unwrap_or(&0);

                if *count < min_card {
                    violations.push(Violation {
                        id: Uuid::new_v4().to_string(),
                        severity: Severity::Error,
                        rule: "MinCardinalityViolation".to_string(),
                        message: format!(
                            "Subject {} must have at least {} values for property {} (found {})",
                            subject, min_card, property, count
                        ),
                        subject: Some(subject.clone()),
                        predicate: Some(property.to_string()),
                        object: None,
                        timestamp: time::now(),
                    });
                }

                if let Some(max_card) = max_card {
                    if *count > max_card {
                        violations.push(Violation {
                            id: Uuid::new_v4().to_string(),
                            severity: Severity::Error,
                            rule: "MaxCardinalityViolation".to_string(),
                            message: format!(
                                "Subject {} must have at most {} values for property {} (found {})",
                                subject, max_card, property, count
                            ),
                            subject: Some(subject.clone()),
                            predicate: Some(property.to_string()),
                            object: None,
                            timestamp: time::now(),
                        });
                    }
                }
            }
        }

        Ok(violations)
    }

    fn infer_triples(
        &self,
        original_triples: &[RdfTriple],
        statistics: &mut ValidationStatistics,
    ) -> Result<Vec<RdfTriple>> {
        let mut inferred = Vec::new();
        let timeout = Duration::from_secs(self.config.reasoning_timeout_seconds);
        let start_time = Instant::now();

        for rule in &self.inference_rules {
            if start_time.elapsed() > timeout {
                return Err(ValidationError::TimeoutError(timeout).into());
            }

            let new_triples = match rule {
                InferenceRule::Inverse { property, inverse } => {
                    self.apply_inverse_property_rule(original_triples, property, inverse)
                }
                InferenceRule::Transitive { property } => {
                    self.apply_transitive_property_rule(original_triples, property)
                }
                InferenceRule::Symmetric { property } => {
                    self.apply_symmetric_property_rule(original_triples, property)
                }
            };

            inferred.extend(new_triples);
            statistics.inference_rules_applied += 1;
        }

        Ok(inferred)
    }

    fn apply_inverse_property_rule(
        &self,
        triples: &[RdfTriple],
        property: &str,
        inverse: &str,
    ) -> Vec<RdfTriple> {
        let mut inferred = Vec::new();

        for triple in triples {
            if triple.predicate == property && !triple.is_literal {
                inferred.push(RdfTriple {
                    subject: triple.object.clone(),
                    predicate: inverse.to_string(),
                    object: triple.subject.clone(),
                    is_literal: false,
                    datatype: None,
                    language: None,
                });
            }
        }

        inferred
    }

    fn apply_transitive_property_rule(
        &self,
        triples: &[RdfTriple],
        property: &str,
    ) -> Vec<RdfTriple> {
        let mut inferred = Vec::new();

        let property_triples: Vec<_> = triples
            .iter()
            .filter(|t| t.predicate == property && !t.is_literal)
            .collect();

        for triple1 in &property_triples {
            for triple2 in &property_triples {
                if triple1.object == triple2.subject && triple1.subject != triple2.object {
                    inferred.push(RdfTriple {
                        subject: triple1.subject.clone(),
                        predicate: property.to_string(),
                        object: triple2.object.clone(),
                        is_literal: false,
                        datatype: None,
                        language: None,
                    });
                }
            }
        }

        inferred
    }

    fn apply_symmetric_property_rule(
        &self,
        triples: &[RdfTriple],
        property: &str,
    ) -> Vec<RdfTriple> {
        let mut inferred = Vec::new();

        for triple in triples {
            if triple.predicate == property && !triple.is_literal {
                inferred.push(RdfTriple {
                    subject: triple.object.clone(),
                    predicate: property.to_string(),
                    object: triple.subject.clone(),
                    is_literal: false,
                    datatype: None,
                    language: None,
                });
            }
        }

        inferred
    }
}

impl Default for OwlValidatorService {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: `validation_report_to_reasoning_report` was removed in ADR-090 Phase A4.
// It depended on `physics::ontology_constraints::{OntologyReasoningReport, …}`
// which are webxr-internal types that cannot move to this crate yet.
// The function had zero callers outside owl_validator itself.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_validation() {
        let validator = OwlValidatorService::new();

        let graph = PropertyGraph {
            nodes: vec![GraphNode {
                id: "person1".to_string(),
                labels: vec!["Person".to_string()],
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "name".to_string(),
                        serde_json::Value::String("John".to_string()),
                    );
                    props.insert(
                        "age".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(30)),
                    );
                    props
                },
            }],
            edges: vec![],
            metadata: HashMap::new(),
        };

        let triples = validator.map_graph_to_rdf(&graph).unwrap();
        assert!(!triples.is_empty());

        let _inferred = validator.infer(&triples).unwrap();
    }

    #[test]
    fn colon_and_space_labels_map_to_rdfs_label_literal_not_rdf_type() {
        let validator = OwlValidatorService::new();
        let graph = PropertyGraph {
            nodes: vec![GraphNode {
                id: "urn:ngm:class:etsi-domain-data-management-ai".to_string(),
                // A colon-bearing display title (the corpus page title) + a
                // whitespace label + a genuine IRI-shaped type slug.
                labels: vec![
                    "ETSI Domain: Data Management + AI".to_string(),
                    "AI Infrastructure".to_string(),
                    "clean-slug-type".to_string(),
                ],
                properties: HashMap::new(),
            }],
            edges: vec![],
            metadata: HashMap::new(),
        };
        // Must NOT error (previously raised "Unknown prefix: ETSI Domain").
        let triples = validator.map_graph_to_rdf(&graph).unwrap();

        let rdfs_label = "http://www.w3.org/2000/01/rdf-schema#label";
        let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

        // The colon title and the whitespace label → rdfs:label literals.
        let label_literals: Vec<&RdfTriple> = triples
            .iter()
            .filter(|t| t.predicate == rdfs_label && t.is_literal)
            .collect();
        assert!(label_literals
            .iter()
            .any(|t| t.object == "ETSI Domain: Data Management + AI"));
        assert!(label_literals
            .iter()
            .any(|t| t.object == "AI Infrastructure"));

        // The clean slug is IRI-shaped → an rd:type IRI object (not a literal).
        assert!(triples.iter().any(|t| t.predicate == rdf_type
            && !t.is_literal
            && t.object.contains("clean-slug-type")));
        // And no rd:type triple carries a whitespace/colon display name.
        assert!(!triples
            .iter()
            .any(|t| t.predicate == rdf_type && t.object.contains("ETSI Domain")));
    }

    #[test]
    fn test_iri_expansion() {
        let validator = OwlValidatorService::new();

        // Registered CURIE prefixes still expand to their full namespace IRI.
        let expanded = validator.expand_iri("foaf:Person").unwrap();
        assert_eq!(expanded, "http://xmlns.com/foaf/0.1/Person");

        let expanded = validator.expand_iri("owl:Class").unwrap();
        assert_eq!(expanded, "http://www.w3.org/2002/07/owl#Class");

        let expanded = validator.expand_iri("rdfs:subClassOf").unwrap();
        assert_eq!(expanded, "http://www.w3.org/2000/01/rdf-schema#subClassOf");

        // Hierarchical absolute IRIs pass through unchanged.
        let full_iri = "http://example.org/Person";
        assert_eq!(validator.expand_iri(full_iri).unwrap(), full_iri);
    }

    #[test]
    fn test_urn_iris_pass_through_as_absolute() {
        // Regression: real KG/corpus data is full of urn:ngm:* and urn:agentbox:* IRIs.
        // These are absolute IRIs, NOT CURIEs, and must never trigger "Unknown prefix".
        let validator = OwlValidatorService::new();

        for iri in [
            "urn:ngm:class:foo",
            "urn:ngm:class:x",
            "urn:agentbox:decision:y",
            "urn:agentbox:decision:2026-08-08:abc",
        ] {
            let expanded = validator.expand_iri(iri).unwrap();
            assert_eq!(expanded, iri, "urn IRI must pass through unchanged: {iri}");
        }
    }

    #[test]
    fn test_did_and_http_iris_pass_through_as_absolute() {
        let validator = OwlValidatorService::new();

        for iri in [
            "did:nostr:abc",
            "http://example.org/z",
            "https://example.org/z",
        ] {
            let expanded = validator.expand_iri(iri).unwrap();
            assert_eq!(
                expanded, iri,
                "absolute IRI must pass through unchanged: {iri}"
            );
        }
    }

    #[test]
    fn test_unregistered_curie_prefix_still_errors() {
        // A single-segment, unknown-scheme value like `bogus:thing` is neither a
        // registered CURIE prefix nor an absolute-IRI shape, so it still errors.
        let validator = OwlValidatorService::new();

        assert!(
            validator.expand_iri("bogus:thing").is_err(),
            "unregistered bare prefix must still error"
        );
    }

    #[test]
    fn test_generic_multi_segment_scheme_is_absolute() {
        // An unknown but syntactically valid scheme with a multi-segment remainder
        // (urn-style `scheme:a:b`) is treated as an absolute IRI, not a CURIE.
        let validator = OwlValidatorService::new();

        let iri = "myscheme:a:b";
        assert_eq!(validator.expand_iri(iri).unwrap(), iri);
    }

    #[test]
    fn test_property_value_serialization() {
        let validator = OwlValidatorService::new();

        let string_val = serde_json::Value::String("test".to_string());
        let (_object, is_literal, datatype, _) =
            validator.serialize_property_value(&string_val).unwrap();
        assert!(is_literal);
        assert_eq!(
            datatype,
            Some("http://www.w3.org/2001/XMLSchema#string".to_string())
        );

        let int_val = serde_json::Value::Number(serde_json::Number::from(42));
        let (_object, is_literal, datatype, _) =
            validator.serialize_property_value(&int_val).unwrap();
        assert!(is_literal);
        assert_eq!(
            datatype,
            Some("http://www.w3.org/2001/XMLSchema#integer".to_string())
        );
    }
}
