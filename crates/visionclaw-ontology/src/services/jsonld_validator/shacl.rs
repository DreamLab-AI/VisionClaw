//! Shape-driven SHACL validator (PRD-022 WS-1 — genuine enforcement).
//!
//! This module replaces the hard-coded [`super::shacl_lite`] matcher as the
//! **authoritative** shape-enforcement engine. It loads the five canonical
//! `.shacl.ttl` NodeShape files that ship in `crates/visionclaw-ontology/shapes/`,
//! parses the Turtle with the workspace's existing Oxigraph dependency (no new
//! RDF crate), and validates parsed JSON-LD entries against the constraints the
//! shapes actually declare.
//!
//! ## Constraint subset implemented
//!
//! Full W3C SHACL is *not* implemented; the constructs used by our five shapes
//! *are* — verified by inspecting the shape files:
//!
//! - `sh:targetClass`      — target selection by `@type`
//! - `sh:targetSubjectsOf` — target selection by "has predicate P"
//! - `sh:property` / `sh:path`
//! - `sh:minCount` / `sh:maxCount`
//! - `sh:datatype`         (`xsd:string`, `xsd:boolean`)
//! - `sh:nodeKind`         (`sh:IRI`)
//! - `sh:pattern`          (regex over the value's lexical form)
//! - `sh:in`               (RDF-list membership)
//! - `sh:not` + `sh:pattern` (negated pattern, e.g. "must not be a stub")
//! - `sh:severity`         (`sh:Violation` blocks; `sh:Warning` is advisory)
//! - `sh:message`
//!
//! The shapes graph is parsed once and memoised ([`builtin`]).

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

use super::shacl_lite;

// ---------------------------------------------------------------------------
// Embedded shape corpus
// ---------------------------------------------------------------------------

/// The five canonical W3C SHACL shape files, embedded at compile time so the
/// validator needs no runtime filesystem access (mirrors how the canonical
/// `@context` is baked in). `(logical-name, turtle-body)`.
pub const SHAPE_FILES: &[(&str, &str)] = &[
    (
        "agent-node",
        include_str!("../../../shapes/agent-node.shacl.ttl"),
    ),
    (
        "ontology-class",
        include_str!("../../../shapes/ontology-class.shacl.ttl"),
    ),
    (
        "knowledge-node",
        include_str!("../../../shapes/knowledge-node.shacl.ttl"),
    ),
    (
        "bridge-record",
        include_str!("../../../shapes/bridge-record.shacl.ttl"),
    ),
    (
        "inferred-axiom",
        include_str!("../../../shapes/inferred-axiom.shacl.ttl"),
    ),
];

// ---------------------------------------------------------------------------
// Namespaces / vocabulary
// ---------------------------------------------------------------------------

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const SH: &str = "http://www.w3.org/ns/shacl#";

const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";

/// Known prefix → namespace bindings used to compact full IRIs back to the
/// aliases the JSON-LD blocks are authored with.
const PREFIXES: &[(&str, &str)] = &[
    ("vc", "https://narrativegoldmine.com/ns/v1#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("sh", "http://www.w3.org/ns/shacl#"),
];

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// Violation severity, mapped 1:1 from `sh:severity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaclSeverity {
    /// `sh:Violation` — blocks a write in enforcing mode.
    Violation,
    /// `sh:Warning` — always advisory (surfaced, never blocks).
    Warning,
}

impl std::fmt::Display for ShaclSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShaclSeverity::Violation => write!(f, "violation"),
            ShaclSeverity::Warning => write!(f, "warning"),
        }
    }
}

/// A single shape-constraint finding located to focus node + property path.
#[derive(Debug, Clone)]
pub struct ShapeFinding {
    /// The `@id` of the offending entry, when present.
    pub focus_node: Option<String>,
    /// The property path (compacted for display, e.g. `vc:agent_pubkey`).
    pub path: String,
    /// The SHACL constraint component that fired (e.g. `sh:minCount`).
    pub constraint: String,
    /// The shape's `sh:message` (or a generated default).
    pub message: String,
    /// Severity from `sh:severity` (default `sh:Violation`).
    pub severity: ShaclSeverity,
    /// The shape that owns the failed constraint (e.g. `AgentNodeShape`).
    pub shape: String,
}

/// Error raised when a shape file cannot be parsed.
#[derive(Debug, Clone)]
pub struct ShaclLoadError(pub String);

impl std::fmt::Display for ShaclLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SHACL shape load error: {}", self.0)
    }
}

impl std::error::Error for ShaclLoadError {}

// ---------------------------------------------------------------------------
// Parsed shape model
// ---------------------------------------------------------------------------

/// How a shape selects its focus nodes.
#[derive(Debug, Clone)]
enum ShapeTarget {
    /// `sh:targetClass` — accepted `@type` tokens (full IRI + compact aliases).
    Class(Vec<String>),
    /// `sh:targetSubjectsOf` — candidate JSON keys for the trigger predicate.
    SubjectsOf(Vec<String>),
}

/// Node-kind constraint (`sh:nodeKind`). `sh:IRI` and `sh:Literal` are used
/// by our shapes; `sh:BlankNode` is evaluated too, and anything else parses
/// without enforcing, rather than silently dropping the constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Iri,
    Literal,
    BlankNode,
    Other,
}

/// One `sh:property` constraint block.
struct PropertyShape {
    /// Full path IRI (for equality checks).
    path_iri: String,
    /// Display form of the path (compact if a known prefix applies).
    path_display: String,
    /// JSON keys any of which may carry a value for this path.
    candidate_keys: Vec<String>,
    min_count: Option<usize>,
    max_count: Option<usize>,
    /// `sh:datatype` full IRI.
    datatype: Option<String>,
    node_kind: Option<NodeKind>,
    pattern: Option<Regex>,
    /// `sh:in` allowed lexical values.
    allowed_in: Option<Vec<String>>,
    /// `sh:not [ sh:pattern ... ]` — value must NOT match.
    not_pattern: Option<Regex>,
    severity: ShaclSeverity,
    message: Option<String>,
}

/// One `sh:NodeShape`.
struct NodeShape {
    /// Display name (`sh:name` or the local part of the shape IRI).
    name: String,
    target: ShapeTarget,
    properties: Vec<PropertyShape>,
}

/// A fully-parsed shapes graph ready to validate JSON-LD entries against.
pub struct ShapesGraph {
    shapes: Vec<NodeShape>,
}

impl ShapesGraph {
    /// Parse all five embedded shape files into one graph.
    pub fn from_embedded() -> Result<Self, ShaclLoadError> {
        let mut shapes = Vec::new();
        for (name, body) in SHAPE_FILES {
            shapes.extend(parse_shapes(name, body)?.into_iter().map(|h| h.0));
        }
        Ok(Self { shapes })
    }

    /// Number of `sh:NodeShape` definitions loaded.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// Validate one parsed JSON-LD entry (object) against every matching shape.
    ///
    /// Handles compact/prefixed/aliased keys and both scalar and array values.
    /// Entries that are not JSON objects yield no findings.
    pub fn validate_entry(&self, entry: &Value) -> Vec<ShapeFinding> {
        let Value::Object(map) = entry else {
            return Vec::new();
        };
        let types = shacl_lite::collect_types(entry);
        let focus = entry_id(map);
        let mut out = Vec::new();
        for shape in &self.shapes {
            if !shape.targets(&types, map) {
                continue;
            }
            for prop in &shape.properties {
                prop.evaluate(map, focus.as_deref(), &shape.name, &mut out);
            }
        }
        out
    }
}

impl NodeShape {
    fn targets(&self, types: &[String], map: &Map<String, Value>) -> bool {
        match &self.target {
            ShapeTarget::Class(tokens) => types.iter().any(|t| tokens.iter().any(|c| c == t)),
            ShapeTarget::SubjectsOf(keys) => keys.iter().any(|k| map.contains_key(k)),
        }
    }
}

impl PropertyShape {
    fn evaluate(
        &self,
        map: &Map<String, Value>,
        focus: Option<&str>,
        shape_name: &str,
        out: &mut Vec<ShapeFinding>,
    ) {
        // Gather every value node present under any candidate key.
        let mut values: Vec<&Value> = Vec::new();
        for key in &self.candidate_keys {
            if let Some(v) = map.get(key) {
                values.extend(value_nodes(v));
            }
        }
        let count = values.len();

        let push = |constraint: &str, out: &mut Vec<ShapeFinding>| {
            out.push(ShapeFinding {
                focus_node: focus.map(str::to_string),
                path: self.path_display.clone(),
                constraint: constraint.to_string(),
                message: self.message.clone().unwrap_or_else(|| {
                    format!("{} constraint {} violated", self.path_display, constraint)
                }),
                severity: self.severity,
                shape: shape_name.to_string(),
            });
        };

        // Cardinality.
        if let Some(min) = self.min_count {
            if count < min {
                push("sh:minCount", out);
            }
        }
        if let Some(max) = self.max_count {
            if count > max {
                push("sh:maxCount", out);
            }
        }

        // Value-level constraints only apply to present values.
        if count == 0 {
            return;
        }

        if let Some(dt) = &self.datatype {
            if values.iter().any(|v| !datatype_matches(v, dt)) {
                push("sh:datatype", out);
            }
        }
        if let Some(nk) = &self.node_kind {
            let violated = match nk {
                NodeKind::Iri => values.iter().any(|v| !is_iri_node(v)),
                NodeKind::Literal => values.iter().any(|v| !is_literal_node(v)),
                NodeKind::BlankNode => values.iter().any(|v| !is_blank_node(v)),
                NodeKind::Other => false,
            };
            if violated {
                push("sh:nodeKind", out);
            }
        }
        if let Some(re) = &self.pattern {
            if values
                .iter()
                .any(|v| lexical(v).map(|s| !re.is_match(&s)).unwrap_or(true))
            {
                push("sh:pattern", out);
            }
        }
        if let Some(allowed) = &self.allowed_in {
            if values
                .iter()
                .any(|v| lexical(v).map(|s| !allowed.contains(&s)).unwrap_or(true))
            {
                push("sh:in", out);
            }
        }
        if let Some(re) = &self.not_pattern {
            if values
                .iter()
                .any(|v| lexical(v).map(|s| re.is_match(&s)).unwrap_or(false))
            {
                push("sh:not", out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Turtle → shape model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    Iri,
    Blank,
    Literal,
}

/// A flattened RDF object plus enough context to follow blank-node structure.
#[derive(Debug, Clone)]
struct ObjVal {
    kind: ObjKind,
    /// IRI string / blank id / literal lexical value.
    lexical: String,
    /// Index key that this object, if a node, is stored under
    /// (`<iri>` or `_:id`). Empty for literals.
    key: String,
}

impl ObjVal {
    fn from_term(t: &oxigraph::model::Term) -> Self {
        use oxigraph::model::Term;
        match t {
            Term::NamedNode(n) => ObjVal {
                kind: ObjKind::Iri,
                lexical: n.as_str().to_string(),
                key: format!("<{}>", n.as_str()),
            },
            Term::BlankNode(b) => ObjVal {
                kind: ObjKind::Blank,
                lexical: b.as_str().to_string(),
                key: b.to_string(),
            },
            Term::Literal(l) => ObjVal {
                kind: ObjKind::Literal,
                lexical: l.value().to_string(),
                key: String::new(),
            },
            // rdf-star triples never appear in our shape files.
            _ => ObjVal {
                kind: ObjKind::Literal,
                lexical: String::new(),
                key: String::new(),
            },
        }
    }
}

type Index = HashMap<String, Vec<(String, ObjVal)>>;

/// Parse the NodeShapes declared in one Turtle document.
pub fn parse_shapes(name: &str, ttl: &str) -> Result<Vec<NodeShapeHandle>, ShaclLoadError> {
    use oxigraph::io::{RdfFormat, RdfParser};
    use oxigraph::store::Store;

    let store = Store::new().map_err(|e| ShaclLoadError(format!("store init ({name}): {e}")))?;
    let parser = RdfParser::from_format(RdfFormat::Turtle);
    store
        .load_from_reader(parser, ttl.as_bytes())
        .map_err(|e| ShaclLoadError(format!("parse turtle '{name}': {e}")))?;

    // Build a subject → (predicate, object) index so blank-node property shapes
    // and RDF lists can be walked without repeated store queries.
    let mut idx: Index = HashMap::new();
    for q in store.quads_for_pattern(None, None, None, None) {
        let q = q.map_err(|e| ShaclLoadError(format!("scan '{name}': {e}")))?;
        let subj_key = q.subject.to_string();
        let pred = q.predicate.as_str().to_string();
        idx.entry(subj_key)
            .or_default()
            .push((pred, ObjVal::from_term(&q.object)));
    }

    // Find every sh:NodeShape subject.
    let nodeshape_iri = format!("{SH}NodeShape");
    let mut shape_keys: Vec<String> = idx
        .iter()
        .filter(|(_, preds)| {
            preds
                .iter()
                .any(|(p, o)| p == RDF_TYPE && o.kind == ObjKind::Iri && o.lexical == nodeshape_iri)
        })
        .map(|(k, _)| k.clone())
        .collect();
    shape_keys.sort(); // deterministic order across runs

    let mut shapes = Vec::new();
    for sk in shape_keys {
        if let Some(shape) = build_node_shape(&idx, &sk, name)? {
            shapes.push(NodeShapeHandle(shape));
        }
    }
    Ok(shapes)
}

/// Opaque handle so `parse_shapes` can be called from tests without exposing
/// the private [`NodeShape`] internals.
pub struct NodeShapeHandle(NodeShape);

impl NodeShapeHandle {
    /// The shape's display name.
    pub fn name(&self) -> &str {
        &self.0.name
    }
    /// Number of `sh:property` constraint blocks parsed for this shape.
    pub fn property_count(&self) -> usize {
        self.0.properties.len()
    }
}

impl From<Vec<NodeShapeHandle>> for ShapesGraph {
    fn from(v: Vec<NodeShapeHandle>) -> Self {
        ShapesGraph {
            shapes: v.into_iter().map(|h| h.0).collect(),
        }
    }
}

fn first<'a>(idx: &'a Index, subj: &str, pred: &str) -> Option<&'a ObjVal> {
    idx.get(subj)?
        .iter()
        .find(|(p, _)| p == pred)
        .map(|(_, o)| o)
}

fn build_node_shape(
    idx: &Index,
    shape_key: &str,
    file: &str,
) -> Result<Option<NodeShape>, ShaclLoadError> {
    let target_class = first(idx, shape_key, &sh("targetClass"));
    let target_subjects_of = first(idx, shape_key, &sh("targetSubjectsOf"));

    let target = if let Some(tc) = target_class {
        ShapeTarget::Class(class_tokens(&tc.lexical))
    } else if let Some(ts) = target_subjects_of {
        ShapeTarget::SubjectsOf(candidate_keys(&ts.lexical))
    } else {
        // A shape with no target we understand cannot select focus nodes.
        return Ok(None);
    };

    let name = first(idx, shape_key, &sh("name"))
        .map(|o| o.lexical.clone())
        .unwrap_or_else(|| local_name(shape_key.trim_start_matches('<').trim_end_matches('>')));

    // Collect every sh:property blank node.
    let mut properties = Vec::new();
    if let Some(preds) = idx.get(shape_key) {
        for (p, o) in preds {
            if p == &sh("property") && o.kind == ObjKind::Blank {
                if let Some(ps) = build_property_shape(idx, &o.key, file)? {
                    properties.push(ps);
                }
            }
        }
    }

    Ok(Some(NodeShape {
        name,
        target,
        properties,
    }))
}

fn build_property_shape(
    idx: &Index,
    prop_key: &str,
    file: &str,
) -> Result<Option<PropertyShape>, ShaclLoadError> {
    let path = match first(idx, prop_key, &sh("path")) {
        Some(o) if o.kind == ObjKind::Iri => o.lexical.clone(),
        // Property shapes without a plain IRI path (e.g. sh:inversePath) are
        // outside the subset our shapes use.
        _ => return Ok(None),
    };

    let min_count = first(idx, prop_key, &sh("minCount")).and_then(|o| o.lexical.parse().ok());
    let max_count = first(idx, prop_key, &sh("maxCount")).and_then(|o| o.lexical.parse().ok());
    let datatype = first(idx, prop_key, &sh("datatype")).map(|o| o.lexical.clone());
    let node_kind = first(idx, prop_key, &sh("nodeKind")).map(|o| node_kind(&o.lexical));

    let pattern = match first(idx, prop_key, &sh("pattern")) {
        Some(o) => Some(compile_regex(&o.lexical, file)?),
        None => None,
    };

    let allowed_in = match first(idx, prop_key, &sh("in")) {
        Some(o) if o.kind == ObjKind::Blank => Some(read_rdf_list(idx, &o.key)),
        _ => None,
    };

    let not_pattern = match first(idx, prop_key, &sh("not")) {
        Some(o) if o.kind == ObjKind::Blank => match first(idx, &o.key, &sh("pattern")) {
            Some(pat) => Some(compile_regex(&pat.lexical, file)?),
            None => None,
        },
        _ => None,
    };

    let severity = match first(idx, prop_key, &sh("severity")) {
        Some(o) if o.lexical == sh("Warning") => ShaclSeverity::Warning,
        // SHACL default severity is sh:Violation.
        _ => ShaclSeverity::Violation,
    };

    let message = first(idx, prop_key, &sh("message")).map(|o| o.lexical.clone());

    Ok(Some(PropertyShape {
        path_display: compact_iri(&path),
        path_iri: path,
        candidate_keys: Vec::new(), // filled below (needs path_iri)
        min_count,
        max_count,
        datatype,
        node_kind,
        pattern,
        allowed_in,
        not_pattern,
        severity,
        message,
    })
    .map(|mut ps| {
        ps.candidate_keys = candidate_keys(&ps.path_iri);
        ps
    }))
}

/// Walk an RDF collection (`rdf:first`/`rdf:rest`) into its lexical members.
fn read_rdf_list(idx: &Index, head_key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = head_key.to_string();
    // Bound the walk defensively against a malformed cyclic list.
    for _ in 0..1024 {
        let Some(entries) = idx.get(&cur) else { break };
        if let Some((_, first_obj)) = entries.iter().find(|(p, _)| p == RDF_FIRST) {
            out.push(first_obj.lexical.clone());
        }
        match entries.iter().find(|(p, _)| p == RDF_REST) {
            Some((_, rest)) if rest.kind == ObjKind::Blank => cur = rest.key.clone(),
            // rest is rdf:nil (or absent) → list terminated.
            _ => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// IRI / key helpers
// ---------------------------------------------------------------------------

fn sh(local: &str) -> String {
    format!("{SH}{local}")
}

fn compile_regex(pat: &str, file: &str) -> Result<Regex, ShaclLoadError> {
    Regex::new(pat).map_err(|e| ShaclLoadError(format!("bad sh:pattern in '{file}': {pat} ({e})")))
}

fn node_kind(iri: &str) -> NodeKind {
    match iri {
        s if s == sh("IRI") => NodeKind::Iri,
        s if s == sh("Literal") => NodeKind::Literal,
        s if s == sh("BlankNode") => NodeKind::BlankNode,
        _ => NodeKind::Other,
    }
}

/// Local name after the last `#` or `/`.
fn local_name(iri: &str) -> String {
    iri.rsplit(['#', '/', ':'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(iri)
        .to_string()
}

/// Compact a full IRI to `prefix:local` when a known prefix applies.
fn compact_iri(iri: &str) -> String {
    for (prefix, ns) in PREFIXES {
        if let Some(local) = iri.strip_prefix(ns) {
            return format!("{prefix}:{local}");
        }
    }
    iri.to_string()
}

/// Candidate JSON keys that could carry a value for a full path IRI: the full
/// IRI, the `prefix:local` compaction, the bare local name, plus known context
/// aliases.
fn candidate_keys(iri: &str) -> Vec<String> {
    let mut keys = vec![iri.to_string()];
    for (prefix, ns) in PREFIXES {
        if let Some(local) = iri.strip_prefix(ns) {
            keys.push(format!("{prefix}:{local}"));
            keys.push(local.to_string());
        }
    }
    // Context-v1 aliases (ADR-D01): rdfs:label is authored as "label" too.
    if iri == RDFS_LABEL {
        keys.push("label".to_string());
    }
    dedup(keys)
}

/// Accepted `@type` tokens for a `sh:targetClass` IRI: the generic
/// compactions plus domain aliases the corpus actually authors.
fn class_tokens(iri: &str) -> Vec<String> {
    let mut tokens = candidate_keys(iri);
    // owl:Class blocks are authored with the vc alias "OntologyClass".
    if iri == OWL_CLASS {
        tokens.push("OntologyClass".to_string());
        tokens.push("vc:OntologyClass".to_string());
        tokens.push("https://narrativegoldmine.com/ns/v1#OntologyClass".to_string());
    }
    dedup(tokens)
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

fn entry_id(map: &Map<String, Value>) -> Option<String> {
    map.get("@id")
        .or_else(|| map.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// JSON value inspection
// ---------------------------------------------------------------------------

/// Flatten a value that may be a scalar or an array into individual nodes.
fn value_nodes(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    }
}

/// Lexical form of a value node (string, `@value`, `@id`, or scalar).
fn lexical(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(map) => {
            if let Some(val) = map.get("@value") {
                lexical(val)
            } else {
                map.get("@id")
                    .or_else(|| map.get("id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            }
        }
        _ => None,
    }
}

/// Does the value represent an IRI reference (`sh:nodeKind sh:IRI`)?
fn is_iri_node(v: &Value) -> bool {
    match v {
        Value::Object(map) => {
            if map.contains_key("@value") {
                return false;
            }
            map.get("@id")
                .or_else(|| map.get("id"))
                .and_then(|v| v.as_str())
                .map(looks_like_iri)
                .unwrap_or(false)
        }
        Value::String(s) => looks_like_iri(s),
        _ => false,
    }
}

/// Minimal IRI shape check (mirrors the ingest validator's `is_valid_iri`).
fn looks_like_iri(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        && s.contains(':')
        && s.chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
}

/// Does the value satisfy the declared `sh:datatype`?
fn datatype_matches(v: &Value, datatype_iri: &str) -> bool {
    match datatype_iri {
        XSD_STRING => is_string_value(v),
        XSD_BOOLEAN => is_boolean_value(v),
        // Datatypes outside our shapes' subset are not enforced (pass).
        _ => true,
    }
}

/// Is the value an `xsd:string` literal? A language-tagged literal
/// (`{"@value": …, "@language": …}`) is `rdf:langString` under W3C SHACL and
/// JSON-LD, so it is **not** one; a shape that wants either says
/// `sh:nodeKind sh:Literal` (ADR-2147).
fn is_string_value(v: &Value) -> bool {
    match v {
        Value::String(_) => true,
        Value::Object(map) => match map.get("@value") {
            Some(Value::String(_)) if map.contains_key("@language") => false,
            Some(Value::String(_)) => match map.get("@type").and_then(|t| t.as_str()) {
                Some(t) => t.ends_with("string"),
                None => true,
            },
            _ => false,
        },
        _ => false,
    }
}

/// Does the value represent a literal of any datatype or language
/// (`sh:nodeKind sh:Literal`)? A JSON scalar is one; a JSON-LD value object
/// (`@value`) is one; a node reference (`@id`) or a nested node is not.
fn is_literal_node(v: &Value) -> bool {
    match v {
        Value::String(_) | Value::Number(_) | Value::Bool(_) => true,
        Value::Object(map) => map.contains_key("@value"),
        _ => false,
    }
}

/// Does the value represent a blank node (`sh:nodeKind sh:BlankNode`)?
fn is_blank_node(v: &Value) -> bool {
    match v {
        Value::Object(map) => map
            .get("@id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.starts_with("_:")),
        Value::String(s) => s.starts_with("_:"),
        _ => false,
    }
}

fn is_boolean_value(v: &Value) -> bool {
    match v {
        Value::Bool(_) => true,
        Value::String(s) => is_bool_lexical(s),
        Value::Object(map) => match map.get("@value") {
            Some(Value::Bool(_)) => true,
            Some(Value::String(s)) => is_bool_lexical(s),
            _ => false,
        },
        _ => false,
    }
}

fn is_bool_lexical(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "false")
}

// ---------------------------------------------------------------------------
// Memoised builtin graph
// ---------------------------------------------------------------------------

static BUILTIN: Lazy<ShapesGraph> = Lazy::new(|| match ShapesGraph::from_embedded() {
    Ok(g) => g,
    Err(e) => {
        // The regression test `all_five_shape_files_parse` guards against this
        // in CI; at runtime we log and fall back to an empty graph rather than
        // aborting the process.
        log::error!("failed to load builtin SHACL shapes: {e}");
        ShapesGraph { shapes: Vec::new() }
    }
});

/// The process-wide, memoised shapes graph parsed from the five embedded files.
pub fn builtin() -> &'static ShapesGraph {
    &BUILTIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph() -> ShapesGraph {
        ShapesGraph::from_embedded().expect("embedded shapes must parse")
    }

    #[test]
    fn all_five_shape_files_parse() {
        for (name, body) in SHAPE_FILES {
            let shapes = parse_shapes(name, body)
                .unwrap_or_else(|e| panic!("shape file '{name}' failed to parse: {e}"));
            assert!(
                !shapes.is_empty(),
                "shape file '{name}' declared no NodeShape"
            );
        }
        // The graph aggregates one NodeShape per file.
        assert_eq!(graph().shape_count(), 5, "expected exactly five NodeShapes");
    }

    #[test]
    fn agent_node_missing_pubkey_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:agent:run-x",
            "@type": "AgentNode",
            "rdfs:label": "Agent X"
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings.iter().any(|f| f.constraint == "sh:minCount"
                && f.path == "vc:agent_pubkey"
                && f.severity == ShaclSeverity::Violation),
            "missing pubkey must be a violation: {findings:?}"
        );
    }

    #[test]
    fn agent_node_bad_pubkey_pattern_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:agent:run-x",
            "@type": "vc:AgentNode",
            "vc:agent_pubkey": "NOT-HEX",
            "rdfs:label": "Agent X"
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings
                .iter()
                .any(|f| f.constraint == "sh:pattern" && f.severity == ShaclSeverity::Violation),
            "malformed pubkey must fail sh:pattern: {findings:?}"
        );
    }

    #[test]
    fn agent_node_valid_passes() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:agent:run-x",
            "@type": "AgentNode",
            "vc:agent_pubkey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "rdfs:label": "Agent X"
        });
        assert!(
            g.validate_entry(&entry).is_empty(),
            "valid agent node must pass"
        );
    }

    #[test]
    fn agent_node_missing_label_is_warning_only() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:agent:run-x",
            "@type": "AgentNode",
            "vc:agent_pubkey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        });
        let findings = g.validate_entry(&entry);
        assert_eq!(findings.len(), 1, "only the label warning: {findings:?}");
        assert_eq!(findings[0].severity, ShaclSeverity::Warning);
        assert_eq!(findings[0].path, "rdfs:label");
    }

    #[test]
    fn ontology_class_missing_label_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:orphan",
            "@type": ["OntologyClass", "owl:Class"]
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings.iter().any(|f| f.path == "rdfs:label"
                && f.constraint == "sh:minCount"
                && f.severity == ShaclSeverity::Violation),
            "class without label must be a violation: {findings:?}"
        );
    }

    #[test]
    fn ontology_class_language_tagged_label_conforms() {
        // ADR-2147: every published class carries `"…"@en`, which W3C SHACL
        // types as rdf:langString. The shape must accept it.
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:foo",
            "@type": "owl:Class",
            "rdfs:label": {"@value": "1Hive", "@language": "en"}
        });
        let findings = g.validate_entry(&entry);
        assert!(
            !findings.iter().any(|f| f.path == "rdfs:label"),
            "a language-tagged label must conform: {findings:?}"
        );
    }

    #[test]
    fn ontology_class_label_that_is_a_node_reference_is_nodekind_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:foo",
            "@type": "owl:Class",
            "rdfs:label": {"@id": "urn:visionclaw:owl:class:bar"}
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings.iter().any(|f| f.path == "rdfs:label"
                && f.constraint == "sh:nodeKind"
                && f.severity == ShaclSeverity::Violation),
            "an IRI label must fail sh:nodeKind sh:Literal: {findings:?}"
        );
    }

    #[test]
    fn a_language_tagged_literal_is_not_an_xsd_string() {
        // W3C SHACL: `"x"@en` has datatype rdf:langString. The in-house
        // evaluator used to pass it as xsd:string, which hid the shape defect
        // ADR-2147 records.
        assert!(!is_string_value(&json!({"@value": "x", "@language": "en"})));
        assert!(is_string_value(&json!({"@value": "x"})));
        assert!(is_string_value(&json!("x")));
        assert!(is_literal_node(&json!({"@value": "x", "@language": "en"})));
        assert!(is_literal_node(&json!(true)));
        assert!(!is_literal_node(&json!({"@id": "urn:x"})));
        assert!(is_blank_node(&json!({"@id": "_:b0"})));
    }

    #[test]
    fn ontology_class_extra_domain_is_maxcount_warning() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:foo",
            "@type": "owl:Class",
            "rdfs:label": "Foo",
            "vc:domain": ["a", "b"]
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings
                .iter()
                .any(|f| f.constraint == "sh:maxCount" && f.severity == ShaclSeverity::Warning),
            "two domains must trip the maxCount warning: {findings:?}"
        );
    }

    #[test]
    fn knowledge_node_missing_required_fields_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:page:foo",
            "@type": "KnowledgeNode"
        });
        let findings = g.validate_entry(&entry);
        let violations: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == ShaclSeverity::Violation)
            .collect();
        assert!(
            violations.iter().any(|f| f.path == "vc:source_file"),
            "missing source_file must be a violation: {findings:?}"
        );
        assert!(
            violations.iter().any(|f| f.path == "vc:public"),
            "missing public must be a violation: {findings:?}"
        );
    }

    #[test]
    fn knowledge_node_public_wrong_datatype_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:page:foo",
            "@type": "vc:KnowledgeNode",
            "vc:source_file": "pages/foo.md",
            "vc:public": "yes",
            "rdfs:label": "Foo"
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings
                .iter()
                .any(|f| f.constraint == "sh:datatype" && f.path == "vc:public"),
            "'yes' is not xsd:boolean: {findings:?}"
        );
    }

    #[test]
    fn knowledge_node_valid_passes() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:page:foo",
            "@type": "KnowledgeNode",
            "vc:source_file": "pages/foo.md",
            "vc:public": true,
            "rdfs:label": "Foo"
        });
        assert!(
            g.validate_entry(&entry).is_empty(),
            "valid knowledge node must pass"
        );
    }

    #[test]
    fn bridge_to_stub_is_violation() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:bridge:abc",
            "@type": "BridgeRecord",
            "vc:bridgeTo": { "@id": "urn:visionclaw:linked:tempietto" }
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings
                .iter()
                .any(|f| f.constraint == "sh:not" && f.severity == ShaclSeverity::Violation),
            "bridge to a linked stub must be a violation: {findings:?}"
        );
    }

    #[test]
    fn bridge_to_concrete_iri_passes() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:bridge:abc",
            "vc:bridgeTo": { "@id": "urn:visionclaw:owl:class:tempietto" }
        });
        assert!(
            g.validate_entry(&entry).is_empty(),
            "bridge to a concrete IRI must pass"
        );
    }

    #[test]
    fn bridge_to_literal_fails_nodekind() {
        let g = graph();
        // A subject carrying vc:bridgeTo is targeted via sh:targetSubjectsOf.
        let entry = json!({
            "@id": "urn:visionclaw:bridge:abc",
            "vc:bridgeTo": "just a label, not an IRI ref"
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings.iter().any(|f| f.constraint == "sh:nodeKind"),
            "non-IRI bridgeTo must fail sh:nodeKind: {findings:?}"
        );
    }

    #[test]
    fn inferred_axiom_requires_prov_and_valid_derivation() {
        let g = graph();
        // Targeted via sh:targetSubjectsOf vc:derivation.
        let entry = json!({
            "@id": "urn:visionclaw:owl:axiom:xyz",
            "vc:derivation": "hallucinated"
        });
        let findings = g.validate_entry(&entry);
        assert!(
            findings
                .iter()
                .any(|f| f.path == "prov:wasGeneratedBy" && f.constraint == "sh:minCount"),
            "missing prov:wasGeneratedBy must be a violation: {findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.constraint == "sh:in"),
            "derivation not in (inferred, proposed) must fail sh:in: {findings:?}"
        );
    }

    #[test]
    fn inferred_axiom_valid_passes() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:owl:axiom:xyz",
            "vc:derivation": "inferred",
            "prov:wasGeneratedBy": { "@id": "urn:visionclaw:agent:reasoner-run-1" }
        });
        assert!(
            g.validate_entry(&entry).is_empty(),
            "valid inferred axiom must pass: {:?}",
            g.validate_entry(&entry)
        );
    }

    #[test]
    fn non_targeted_entry_yields_nothing() {
        let g = graph();
        let entry = json!({
            "@id": "urn:visionclaw:page:plain",
            "@type": "Page",
            "vc:slug": "plain"
        });
        assert!(g.validate_entry(&entry).is_empty());
    }
}
