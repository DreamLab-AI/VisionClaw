//! Canonical JSON-LD validator for VisionClaw Data Sprint (Phase D-2).
//!
//! The validator is the single source of truth for schema rejection
//! decisions, executed at two stages:
//!
//! 1. **Pre-commit** — invoked by `scripts/pre-commit-validate.sh`
//!    against staged `*.md` files. Fast failure for authors before the
//!    data leaves the editor.
//! 2. **Pre-ingest** — invoked by the parser pipeline (the parallel
//!    `jsonld_ingest` specialist's module) before any triples reach
//!    Oxigraph.
//!
//! Both call paths share `Validator` and produce `Vec<ValidationIssue>`.
//! The 11 documented [`errors::ErrorCategory`] variants form a closed
//! set mapping 1:1 to fixtures in
//! `tests/fixtures/data-model/invalid/`.
//!
//! ## Public API (the parser specialist imports these)
//!
//! ```rust,ignore
//! use crate::services::jsonld_validator::{
//!     Validator, ValidationIssue, Severity, ErrorCategory,
//!     SourceRef, OwlProfile,
//! };
//! let validator = Validator::new()?;
//! let issues = validator.validate_jsonld_block(&value, SourceRef::default());
//! ```

pub mod class_bit;
pub mod errors;
pub mod frame;
pub mod iri;
pub mod owl_el_profile;
pub mod shacl;
pub mod shacl_lite;
pub mod signature;

pub use errors::{ErrorCategory, Severity};
pub use shacl::{builtin as builtin_shapes, ShaclSeverity, ShapeFinding, ShapesGraph};

use serde_json::Value;
use std::path::{Path, PathBuf};

use oxigraph::model::Quad;

/// Source-location reference attached to every [`ValidationIssue`]. The
/// pre-commit hook prints these in `file:line:col` form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRef {
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional human-readable label for the block (e.g.
    /// "block 2 of foo.md"). Helpful when one file has several
    /// `json-ld` fences.
    pub block_label: Option<String>,
}

impl SourceRef {
    pub fn for_file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    pub fn with_block(mut self, label: impl Into<String>) -> Self {
        self.block_label = Some(label.into());
        self
    }
}

impl std::fmt::Display for SourceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self
            .path
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{}:{}:{}", path, l, c),
            (Some(l), None) => write!(f, "{}:{}", path, l),
            _ => write!(f, "{}", path),
        }
    }
}

/// A single validator finding.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub category: ErrorCategory,
    pub source: SourceRef,
    pub message: String,
    pub suggested_fix: Option<String>,
}

impl ValidationIssue {
    pub fn error(category: ErrorCategory, source: SourceRef) -> Self {
        let message = category.to_string();
        let suggested_fix = suggest(&category);
        Self {
            severity: Severity::Error,
            category,
            source,
            message,
            suggested_fix,
        }
    }

    pub fn warning(category: ErrorCategory, source: SourceRef) -> Self {
        let message = category.to_string();
        let suggested_fix = suggest(&category);
        Self {
            severity: Severity::Warning,
            category,
            source,
            message,
            suggested_fix,
        }
    }
}

fn suggest(c: &ErrorCategory) -> Option<String> {
    match c {
        ErrorCategory::SchemaVersionMissing => Some(
            "Add `\"@version\": 1.1` inside the inline `@context` object, or use \
             a string `@context` URL that already declares it."
                .to_string(),
        ),
        ErrorCategory::ContextMissing => Some(
            "Add `\"@context\": \"https://narrativegoldmine.com/context/v1.jsonld\"`".to_string(),
        ),
        ErrorCategory::ContextVersionUnknown { found: _ } => Some(format!(
            "Replace the `@context` URL with one of the accepted versions: {:?}",
            frame::ACCEPTED_CONTEXT_URLS
        )),
        ErrorCategory::RequiredFieldMissing { what } => {
            Some(format!("Add the missing required field `{}`.", what))
        }
        ErrorCategory::MalformedIri { .. } => Some(
            "IRIs must be RFC-3987-compliant: no whitespace, ASCII-alphabetic scheme, \
             at least one `:` separator. Use a `urn:visionclaw:*` or `did:nostr:*` prefix."
                .to_string(),
        ),
        ErrorCategory::BridgeTargetMustBeConcrete { .. } => Some(
            "Resolve the LinkedPage stub to a concrete page or ontology class before \
             asserting the bridge, or remove the bridge until the target exists."
                .to_string(),
        ),
        ErrorCategory::OutsideOwl2ElProfile { construct } => Some(format!(
            "`{construct}` is rejected by OWL 2 EL §3, Table 1. Model the assertion \
             as an annotation in `urn:visionclaw:graph:annotation` or refactor to \
             intersection / someValuesFrom."
        )),
        ErrorCategory::MissingCodeFenceMarker => Some(
            "Wrap the JSON-LD content in a ```json-ld fenced code block. \
             Generic ``` fences are treated as documentation and skipped."
                .to_string(),
        ),
        ErrorCategory::ClassBitMismatch { .. } => Some(
            "Either change `@type` to match the IRI scheme, or rewrite the `@id` \
             URN to use the scheme implied by the declared type."
                .to_string(),
        ),
        ErrorCategory::ProvAttributionMissing => {
            Some("Add `\"prov:wasAttributedTo\": {\"@id\": \"did:nostr:<pubkey>\"}`".to_string())
        }
        ErrorCategory::ProvTimestampMissing => Some(
            "Add `\"prov:generatedAtTime\": {\"@value\": \"<iso8601>\", \
             \"@type\": \"xsd:dateTime\"}`"
                .to_string(),
        ),
    }
}

/// The OWL profile boundary used by the validator. Currently only EL
/// is implemented; the enum exists so the validator constructor reads
/// naturally and so DL/QL/RL paths can be added without renaming the
/// public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OwlProfile {
    #[default]
    El,
}

/// Canonical `@context` representation. Owns the parsed JSON of
/// `context-v1.jsonld` so the validator can answer questions like
/// "is `label` defined?" without re-parsing.
#[derive(Debug, Clone)]
pub struct CanonicalContext {
    pub url: String,
    pub document: Value,
}

impl CanonicalContext {
    pub fn v1() -> Result<Self, ValidatorInitError> {
        // Bake the canonical context file in at build time so the
        // validator does not require runtime file IO.
        const V1_BYTES: &str = include_str!("context-v1.jsonld");
        let document = serde_json::from_str(V1_BYTES)
            .map_err(|e| ValidatorInitError::ContextParse(e.to_string()))?;
        Ok(Self {
            url: "https://narrativegoldmine.com/context/v1.jsonld".to_string(),
            document,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatorInitError {
    #[error("failed to parse canonical context: {0}")]
    ContextParse(String),
    #[error("failed to read fixture file: {0}")]
    Io(String),
}

/// The main validator entry point. Holds the canonical context and the
/// active OWL profile; the inner check functions are pure.
#[derive(Debug, Clone)]
pub struct Validator {
    pub context: CanonicalContext,
    pub profile: OwlProfile,
}

impl Validator {
    /// Construct a validator with the canonical v1 context and the
    /// EL profile.
    pub fn new() -> Result<Self, ValidatorInitError> {
        Ok(Self {
            context: CanonicalContext::v1()?,
            profile: OwlProfile::default(),
        })
    }

    /// Read a markdown file, extract every ```json-ld fenced block,
    /// and run the full validator suite against each. Empty result
    /// means the file passes.
    pub fn validate_markdown_file(&self, path: &Path) -> Vec<ValidationIssue> {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                return vec![ValidationIssue::error(
                    ErrorCategory::RequiredFieldMissing {
                        what: format!("readable file ({})", e),
                    },
                    SourceRef::for_file(path),
                )];
            }
        };
        self.validate_markdown_source(&source, path)
    }

    /// Validate already-loaded markdown source. Used by the binary and
    /// by tests that want to avoid disk IO.
    pub fn validate_markdown_source(&self, markdown: &str, path: &Path) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let blocks = extract_jsonld_blocks(markdown);

        // 107 — MissingCodeFenceMarker: a file declared invalid but
        // emitting no json-ld blocks tripwires this category. The
        // heuristic: if the file has a `json-ld` mention but no
        // properly-fenced block, OR if the file contains a JSON
        // object literal (`{ "@context" or ` outside any fenced
        // ```json-ld block, we report it.
        if blocks.is_empty() && contains_unfenced_jsonld(markdown) {
            issues.push(ValidationIssue::error(
                ErrorCategory::MissingCodeFenceMarker,
                SourceRef::for_file(path),
            ));
        }

        for (idx, (block_src, line_no)) in blocks.iter().enumerate() {
            let source_ref = SourceRef::for_file(path)
                .with_line(*line_no)
                .with_block(format!("block {}", idx + 1));
            match serde_json::from_str::<Value>(block_src) {
                Ok(value) => {
                    issues.extend(self.validate_jsonld_block(&value, source_ref));
                }
                Err(e) => {
                    // Malformed JSON: surface as ContextMissing with
                    // a parser-detail suggestion. (No invalid fixture
                    // covers this exact path; pre-commit catches it
                    // anyway because Error-severity.)
                    let mut issue =
                        ValidationIssue::error(ErrorCategory::ContextMissing, source_ref);
                    issue.message = format!("invalid JSON in fenced block: {}", e);
                    issues.push(issue);
                }
            }
        }
        issues
    }

    /// Validate a single parsed JSON-LD block. Composable; this is
    /// the entry the parser pipeline calls.
    pub fn validate_jsonld_block(&self, block: &Value, source: SourceRef) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        // Frame-level checks (context / id / version / prov).
        let frame_result = frame::validate_block_frame(block);
        for category in frame_result.categories {
            issues.push(ValidationIssue::error(category, source.clone()));
        }
        // Walk every assertion in the block (handling `@graph`).
        for entry in collect_entries(block) {
            for c in shacl_lite::validate_entry_shape(entry) {
                issues.push(ValidationIssue::error(c, source.clone()));
            }
            for c in owl_el_profile::validate_entry_profile(entry) {
                issues.push(ValidationIssue::error(c, source.clone()));
            }
            for c in class_bit::validate_class_bit_consistency(entry) {
                issues.push(ValidationIssue::error(c, source.clone()));
            }
        }
        issues
    }

    /// Quad-stream validation (the pre-ingest tail check). Confirms
    /// no forbidden predicate landed in the asserted ontology graph.
    ///
    /// Gated behind `persistence-oxigraph` since `Quad` is an Oxigraph
    /// type and the pre-commit binary does not need it.
    pub fn validate_quads(&self, quads: &[Quad]) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        for q in quads {
            let predicate = q.predicate.as_str();
            for forbidden in owl_el_profile::FORBIDDEN_CONSTRUCTS {
                let owl_iri = format!(
                    "http://www.w3.org/2002/07/owl#{}",
                    forbidden.trim_start_matches("owl:")
                );
                if predicate == owl_iri {
                    issues.push(ValidationIssue::error(
                        ErrorCategory::OutsideOwl2ElProfile {
                            construct: (*forbidden).to_string(),
                        },
                        SourceRef::default(),
                    ));
                }
            }
        }
        issues
    }
}

/// Walk a JSON-LD document and return references to every assertion
/// entry. Handles the `@graph` array form per ADR-D01 §D7.
fn collect_entries(block: &Value) -> Vec<&Value> {
    let Value::Object(map) = block else {
        return vec![block];
    };
    if let Some(g) = map.get("@graph").or_else(|| map.get("graph")) {
        match g {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        }
    } else {
        vec![block]
    }
}

/// Extract every ```json-ld fenced code block from the markdown
/// source. Returns `(content, line_number_of_opening_fence)` pairs.
///
/// The recogniser is intentionally narrow:
/// - Fence start: `^```json-ld` (whitespace before the backticks is
///   allowed; nothing else after the language tag).
/// - Fence end: a line consisting of `^```$` only.
fn extract_jsonld_blocks(markdown: &str) -> Vec<(String, usize)> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines().enumerate().peekable();
    while let Some((line_no, line)) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```json-ld") {
            let mut buf = String::new();
            for (_, body_line) in lines.by_ref() {
                if body_line.trim() == "```" {
                    break;
                }
                buf.push_str(body_line);
                buf.push('\n');
            }
            blocks.push((buf, line_no + 1));
        }
    }
    blocks
}

/// Heuristic for fixture 107: file contains JSON-LD-shaped content
/// (an `@context` or `@id` key) but no `json-ld` language tag on any
/// fence. This is the "bare JSON without block marker" failure.
fn contains_unfenced_jsonld(markdown: &str) -> bool {
    // Strip out properly-fenced ```json-ld blocks; whatever's left
    // is fair game for the heuristic.
    let mut buf = String::with_capacity(markdown.len());
    let mut in_jsonld = false;
    for line in markdown.lines() {
        let t = line.trim_start();
        if t.starts_with("```json-ld") {
            in_jsonld = true;
            continue;
        }
        if in_jsonld && t == "```" {
            in_jsonld = false;
            continue;
        }
        if !in_jsonld {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    buf.contains("@context") || buf.contains("@id")
}
