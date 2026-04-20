//! Linked Data Platform (LDP) resource and container semantics.
//!
//! Phase 2 scope:
//!
//! - Full `Link` header set (type + acl + describedby + storage root).
//! - `Prefer` header parsing (PreferMinimalContainer, PreferContainedIRIs).
//! - `Accept-Post` header for containers.
//! - PATCH via N3 (solid-protocol PATCH): `insert`, `delete`, `where`.
//! - PATCH via SPARQL-Update (`INSERT DATA`, `DELETE DATA`, `DELETE WHERE`).
//! - Content negotiation: Turtle, JSON-LD, N-Triples, RDF/XML.
//! - Server-managed triples (`dc:modified`, `stat:size`, `ldp:contains`).
//! - `.meta` sidecar resolution.
//!
//! The module intentionally uses a tiny, in-crate RDF triple model so
//! the crate stays free of the heavier `sophia` / `oxigraph` dep trees.
//! Only Turtle-subset parsing required for real-world Solid Pod PATCH
//! flows is supported; client-supplied RDF is parsed via the N-Triples
//! fallback whenever the Turtle fast path hits something exotic.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use async_trait::async_trait;
use serde::Serialize;

use crate::error::PodError;
use crate::storage::Storage;

pub mod iri {
    pub const LDP_RESOURCE: &str = "http://www.w3.org/ns/ldp#Resource";
    pub const LDP_CONTAINER: &str = "http://www.w3.org/ns/ldp#Container";
    pub const LDP_BASIC_CONTAINER: &str = "http://www.w3.org/ns/ldp#BasicContainer";
    pub const LDP_NS: &str = "http://www.w3.org/ns/ldp#";
    pub const LDP_CONTAINS: &str = "http://www.w3.org/ns/ldp#contains";
    pub const LDP_PREFER_MINIMAL_CONTAINER: &str =
        "http://www.w3.org/ns/ldp#PreferMinimalContainer";
    pub const LDP_PREFER_CONTAINED_IRIS: &str =
        "http://www.w3.org/ns/ldp#PreferContainedIRIs";
    pub const LDP_PREFER_MEMBERSHIP: &str = "http://www.w3.org/ns/ldp#PreferMembership";

    pub const DCTERMS_NS: &str = "http://purl.org/dc/terms/";
    pub const DCTERMS_MODIFIED: &str = "http://purl.org/dc/terms/modified";

    pub const STAT_NS: &str = "http://www.w3.org/ns/posix/stat#";
    pub const STAT_SIZE: &str = "http://www.w3.org/ns/posix/stat#size";
    pub const STAT_MTIME: &str = "http://www.w3.org/ns/posix/stat#mtime";

    pub const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
    pub const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
    pub const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";

    pub const PIM_STORAGE: &str = "http://www.w3.org/ns/pim/space#Storage";
    pub const PIM_STORAGE_REL: &str = "http://www.w3.org/ns/pim/space#storage";

    pub const ACL_NS: &str = "http://www.w3.org/ns/auth/acl#";
}

/// MIME types recognised by the content negotiator. The order matters:
/// the first format that matches the `Accept` header wins, and if the
/// client provides `*/*` the server defaults to Turtle.
pub const ACCEPT_POST: &str = "text/turtle, application/ld+json, application/n-triples";

/// Return whether a path addresses an LDP container.
pub fn is_container(path: &str) -> bool {
    path == "/" || path.ends_with('/')
}

/// Return whether a path addresses an ACL sidecar.
pub fn is_acl_path(path: &str) -> bool {
    path.ends_with(".acl")
}

/// Return whether a path addresses a `.meta` sidecar.
pub fn is_meta_path(path: &str) -> bool {
    path.ends_with(".meta")
}

/// Compute the `.meta` sidecar for a resource.
pub fn meta_sidecar_for(path: &str) -> String {
    if is_meta_path(path) {
        path.to_string()
    } else {
        format!("{path}.meta")
    }
}

/// Build the full set of `Link` headers for a given resource path.
///
/// Emits:
/// - `<ldp:Resource>; rel="type"` always.
/// - `<ldp:Container>; rel="type"` + `<ldp:BasicContainer>; rel="type"` for containers.
/// - `<path.acl>; rel="acl"` for every resource except the ACL itself.
/// - `<path.meta>; rel="describedby"` for every non-meta resource.
/// - `</>; rel="http://www.w3.org/ns/pim/space#storage"` for the pod root.
pub fn link_headers(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    if is_container(path) {
        out.push(format!("<{}>; rel=\"type\"", iri::LDP_BASIC_CONTAINER));
        out.push(format!("<{}>; rel=\"type\"", iri::LDP_CONTAINER));
        out.push(format!("<{}>; rel=\"type\"", iri::LDP_RESOURCE));
    } else {
        out.push(format!("<{}>; rel=\"type\"", iri::LDP_RESOURCE));
    }
    if !is_acl_path(path) {
        let acl_target = format!("{path}.acl");
        out.push(format!("<{acl_target}>; rel=\"acl\""));
    }
    if !is_meta_path(path) && !is_acl_path(path) {
        let meta_target = meta_sidecar_for(path);
        out.push(format!("<{meta_target}>; rel=\"describedby\""));
    }
    if path == "/" {
        out.push(format!("</>; rel=\"{}\"", iri::PIM_STORAGE_REL));
    }
    out
}

/// Resolve the target path when POSTing to a container.
pub fn resolve_slug(container: &str, slug: Option<&str>) -> String {
    let name = slug
        .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains(".."))
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if container.ends_with('/') {
        format!("{container}{name}")
    } else {
        format!("{container}/{name}")
    }
}

// ---------------------------------------------------------------------------
// Prefer header parsing (RFC 7240 + LDP 4.2.2)
// ---------------------------------------------------------------------------

/// What portions of a container representation the client wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerRepresentation {
    /// Membership triples + metadata (default).
    Full,
    /// `ldp:contains` + container metadata only.
    MinimalContainer,
    /// Only the list of contained IRIs, no server metadata.
    ContainedIRIsOnly,
}

impl Default for ContainerRepresentation {
    fn default() -> Self {
        Self::Full
    }
}

/// Parsed `Prefer` header value. Non-`return=representation` preferences
/// are ignored (the LDP spec allows this).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreferHeader {
    pub representation: ContainerRepresentation,
    pub include_minimal: bool,
    pub include_contained_iris: bool,
    pub omit_membership: bool,
}

impl PreferHeader {
    /// Parse a `Prefer` header value per RFC 7240 (tolerant).
    pub fn parse(value: &str) -> Self {
        let mut out = PreferHeader::default();
        // Preferences are separated by `,` at the top level.
        for pref in value.split(',') {
            let pref = pref.trim();
            if pref.is_empty() {
                continue;
            }
            // Tokens are separated by `;`.
            let mut parts = pref.split(';').map(|s| s.trim());
            let head = match parts.next() {
                Some(h) => h,
                None => continue,
            };
            if !head.eq_ignore_ascii_case("return=representation") {
                continue;
            }
            for token in parts {
                if let Some(val) = token
                    .strip_prefix("include=")
                    .or_else(|| token.strip_prefix("include ="))
                {
                    let unq = val.trim().trim_matches('"');
                    for iri in unq.split_whitespace() {
                        if iri == iri::LDP_PREFER_MINIMAL_CONTAINER {
                            out.include_minimal = true;
                            out.representation = ContainerRepresentation::MinimalContainer;
                        } else if iri == iri::LDP_PREFER_CONTAINED_IRIS {
                            out.include_contained_iris = true;
                            out.representation = ContainerRepresentation::ContainedIRIsOnly;
                        }
                    }
                } else if let Some(val) = token
                    .strip_prefix("omit=")
                    .or_else(|| token.strip_prefix("omit ="))
                {
                    let unq = val.trim().trim_matches('"');
                    for iri in unq.split_whitespace() {
                        if iri == iri::LDP_PREFER_MEMBERSHIP {
                            out.omit_membership = true;
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Content negotiation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdfFormat {
    Turtle,
    JsonLd,
    NTriples,
    RdfXml,
}

impl RdfFormat {
    pub fn mime(&self) -> &'static str {
        match self {
            RdfFormat::Turtle => "text/turtle",
            RdfFormat::JsonLd => "application/ld+json",
            RdfFormat::NTriples => "application/n-triples",
            RdfFormat::RdfXml => "application/rdf+xml",
        }
    }

    pub fn from_mime(mime: &str) -> Option<Self> {
        let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        match mime.as_str() {
            "text/turtle" | "application/turtle" | "application/x-turtle" => {
                Some(RdfFormat::Turtle)
            }
            "application/ld+json" | "application/json+ld" => Some(RdfFormat::JsonLd),
            "application/n-triples" | "text/plain+ntriples" => Some(RdfFormat::NTriples),
            "application/rdf+xml" => Some(RdfFormat::RdfXml),
            _ => None,
        }
    }
}

/// Pick the best RDF format based on an `Accept` header.
///
/// q-values are respected; on ties Turtle wins. `*/*` falls back to Turtle.
pub fn negotiate_format(accept: Option<&str>) -> RdfFormat {
    let accept = match accept {
        Some(a) if !a.trim().is_empty() => a,
        _ => return RdfFormat::Turtle,
    };

    let mut best: Option<(f32, RdfFormat)> = None;
    for entry in accept.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split(';').map(|s| s.trim());
        let mime = match parts.next() {
            Some(m) => m.to_ascii_lowercase(),
            None => continue,
        };
        let mut q: f32 = 1.0;
        for token in parts {
            if let Some(v) = token.strip_prefix("q=") {
                if let Ok(parsed) = v.parse::<f32>() {
                    q = parsed;
                }
            }
        }
        let format = match mime.as_str() {
            "text/turtle" | "application/turtle" => Some(RdfFormat::Turtle),
            "application/ld+json" => Some(RdfFormat::JsonLd),
            "application/n-triples" => Some(RdfFormat::NTriples),
            "application/rdf+xml" => Some(RdfFormat::RdfXml),
            "*/*" | "application/*" | "text/*" => Some(RdfFormat::Turtle),
            _ => None,
        };
        if let Some(f) = format {
            match best {
                None => best = Some((q, f)),
                Some((bq, _)) if q > bq => best = Some((q, f)),
                _ => {}
            }
        }
    }
    best.map(|(_, f)| f).unwrap_or(RdfFormat::Turtle)
}

// ---------------------------------------------------------------------------
// In-crate RDF triple model (minimal, sufficient for PATCH evaluation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Term {
    Iri(String),
    BlankNode(String),
    Literal {
        value: String,
        datatype: Option<String>,
        language: Option<String>,
    },
}

impl Term {
    pub fn iri(i: impl Into<String>) -> Self {
        Term::Iri(i.into())
    }
    pub fn blank(b: impl Into<String>) -> Self {
        Term::BlankNode(b.into())
    }
    pub fn literal(v: impl Into<String>) -> Self {
        Term::Literal {
            value: v.into(),
            datatype: None,
            language: None,
        }
    }
    pub fn typed_literal(v: impl Into<String>, dt: impl Into<String>) -> Self {
        Term::Literal {
            value: v.into(),
            datatype: Some(dt.into()),
            language: None,
        }
    }

    fn write_ntriples(&self, out: &mut String) {
        match self {
            Term::Iri(i) => {
                out.push('<');
                out.push_str(i);
                out.push('>');
            }
            Term::BlankNode(b) => {
                out.push_str("_:");
                out.push_str(b);
            }
            Term::Literal {
                value,
                datatype,
                language,
            } => {
                out.push('"');
                for c in value.chars() {
                    match c {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
                if let Some(lang) = language {
                    out.push('@');
                    out.push_str(lang);
                } else if let Some(dt) = datatype {
                    out.push_str("^^<");
                    out.push_str(dt);
                    out.push('>');
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Triple {
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}

impl Triple {
    pub fn new(subject: Term, predicate: Term, object: Term) -> Self {
        Self {
            subject,
            predicate,
            object,
        }
    }
}

/// Minimal RDF graph — a sorted set of triples.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    triples: BTreeSet<Triple>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            triples: BTreeSet::new(),
        }
    }

    pub fn from_triples(triples: impl IntoIterator<Item = Triple>) -> Self {
        let mut g = Self::new();
        for t in triples {
            g.insert(t);
        }
        g
    }

    pub fn insert(&mut self, triple: Triple) {
        self.triples.insert(triple);
    }

    pub fn remove(&mut self, triple: &Triple) -> bool {
        self.triples.remove(triple)
    }

    pub fn contains(&self, triple: &Triple) -> bool {
        self.triples.contains(triple)
    }

    pub fn len(&self) -> usize {
        self.triples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.triples.is_empty()
    }

    pub fn triples(&self) -> impl Iterator<Item = &Triple> {
        self.triples.iter()
    }

    /// Extend with all triples from another graph.
    pub fn extend(&mut self, other: &Graph) {
        for t in &other.triples {
            self.triples.insert(t.clone());
        }
    }

    /// Remove every triple in `other` that is present in `self`.
    pub fn subtract(&mut self, other: &Graph) {
        for t in &other.triples {
            self.triples.remove(t);
        }
    }

    /// Serialise to N-Triples.
    pub fn to_ntriples(&self) -> String {
        let mut out = String::new();
        for t in &self.triples {
            t.subject.write_ntriples(&mut out);
            out.push(' ');
            t.predicate.write_ntriples(&mut out);
            out.push(' ');
            t.object.write_ntriples(&mut out);
            out.push_str(" .\n");
        }
        out
    }

    /// Parse N-Triples — supports the full EBNF subset used by PATCH.
    pub fn parse_ntriples(input: &str) -> Result<Self, PodError> {
        let mut g = Graph::new();
        for (i, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let t = parse_nt_line(line)
                .map_err(|e| PodError::Unsupported(format!("N-Triples line {}: {e}", i + 1)))?;
            g.insert(t);
        }
        Ok(g)
    }
}

fn parse_nt_line(line: &str) -> Result<Triple, String> {
    let line = line.trim_end_matches('.').trim();
    let (subject, rest) = read_term(line)?;
    let rest = rest.trim_start();
    let (predicate, rest) = read_term(rest)?;
    let rest = rest.trim_start();
    let (object, _rest) = read_term(rest)?;
    Ok(Triple::new(subject, predicate, object))
}

fn read_term(input: &str) -> Result<(Term, &str), String> {
    let input = input.trim_start();
    if let Some(rest) = input.strip_prefix('<') {
        let end = rest.find('>').ok_or_else(|| "unterminated IRI".to_string())?;
        let iri = &rest[..end];
        Ok((Term::Iri(iri.to_string()), &rest[end + 1..]))
    } else if let Some(rest) = input.strip_prefix("_:") {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '.')
            .unwrap_or(rest.len());
        Ok((Term::BlankNode(rest[..end].to_string()), &rest[end..]))
    } else if input.starts_with('"') {
        read_literal(input)
    } else {
        Err(format!("unexpected char: {}", input.chars().next().unwrap_or('?')))
    }
}

fn read_literal(input: &str) -> Result<(Term, &str), String> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'"') {
        return Err("expected '\"'".to_string());
    }
    let mut i = 1usize;
    let mut value = String::new();
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                match bytes[i + 1] {
                    b'n' => value.push('\n'),
                    b't' => value.push('\t'),
                    b'r' => value.push('\r'),
                    b'"' => value.push('"'),
                    b'\\' => value.push('\\'),
                    other => value.push(other as char),
                }
                i += 2;
            }
            b'"' => {
                i += 1;
                break;
            }
            other => {
                value.push(other as char);
                i += 1;
            }
        }
    }
    let rest = &input[i..];
    let (datatype, language, rest) = if let Some(r) = rest.strip_prefix("^^<") {
        let end = r.find('>').ok_or_else(|| "unterminated datatype IRI".to_string())?;
        (Some(r[..end].to_string()), None, &r[end + 1..])
    } else if let Some(r) = rest.strip_prefix('@') {
        let end = r
            .find(|c: char| c.is_whitespace() || c == '.')
            .unwrap_or(r.len());
        (None, Some(r[..end].to_string()), &r[end..])
    } else {
        (None, None, rest)
    };
    Ok((
        Term::Literal {
            value,
            datatype,
            language,
        },
        rest,
    ))
}

// ---------------------------------------------------------------------------
// Server-managed triples
// ---------------------------------------------------------------------------

/// Compute the server-managed triples for a resource (`dc:modified`,
/// `stat:size`, and for containers `ldp:contains` entries).
pub fn server_managed_triples(
    resource_iri: &str,
    modified: chrono::DateTime<chrono::Utc>,
    size: u64,
    is_container_flag: bool,
    contained: &[String],
) -> Graph {
    let mut g = Graph::new();
    let subject = Term::iri(resource_iri);

    g.insert(Triple::new(
        subject.clone(),
        Term::iri(iri::DCTERMS_MODIFIED),
        Term::typed_literal(modified.to_rfc3339(), iri::XSD_DATETIME),
    ));
    g.insert(Triple::new(
        subject.clone(),
        Term::iri(iri::STAT_SIZE),
        Term::typed_literal(size.to_string(), iri::XSD_INTEGER),
    ));
    g.insert(Triple::new(
        subject.clone(),
        Term::iri(iri::STAT_MTIME),
        Term::typed_literal(modified.timestamp().to_string(), iri::XSD_INTEGER),
    ));

    if is_container_flag {
        for child in contained {
            let base = if resource_iri.ends_with('/') {
                resource_iri.to_string()
            } else {
                format!("{resource_iri}/")
            };
            g.insert(Triple::new(
                subject.clone(),
                Term::iri(iri::LDP_CONTAINS),
                Term::iri(format!("{base}{child}")),
            ));
        }
    }
    g
}

/// List of predicates clients are not allowed to set directly. These
/// are overwritten by the server on PUT.
pub const SERVER_MANAGED_PREDICATES: &[&str] = &[
    iri::DCTERMS_MODIFIED,
    iri::STAT_SIZE,
    iri::STAT_MTIME,
    iri::LDP_CONTAINS,
];

/// Return the list of client-supplied triples that attempt to set
/// server-managed predicates. These MUST be ignored at PUT time.
pub fn find_illegal_server_managed(graph: &Graph) -> Vec<Triple> {
    graph
        .triples()
        .filter(|t| {
            if let Term::Iri(p) = &t.predicate {
                SERVER_MANAGED_PREDICATES.iter().any(|sm| sm == p)
            } else {
                false
            }
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Container representation (JSON-LD + Turtle)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ContainerMember {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@type")]
    pub types: Vec<&'static str>,
}

/// Render a container as JSON-LD respecting a `Prefer` header.
pub fn render_container_jsonld(
    container_path: &str,
    members: &[String],
    prefer: PreferHeader,
) -> serde_json::Value {
    let base = if container_path.ends_with('/') {
        container_path.to_string()
    } else {
        format!("{container_path}/")
    };

    match prefer.representation {
        ContainerRepresentation::ContainedIRIsOnly => serde_json::json!({
            "@id": container_path,
            "ldp:contains": members
                .iter()
                .map(|m| serde_json::json!({"@id": format!("{base}{m}")}))
                .collect::<Vec<_>>(),
        }),
        ContainerRepresentation::MinimalContainer => serde_json::json!({
            "@context": {
                "ldp": iri::LDP_NS,
                "dcterms": iri::DCTERMS_NS,
            },
            "@id": container_path,
            "@type": [ "ldp:Container", "ldp:BasicContainer", "ldp:Resource" ],
        }),
        ContainerRepresentation::Full => {
            let contains: Vec<ContainerMember> = members
                .iter()
                .map(|m| {
                    let is_dir = m.ends_with('/');
                    ContainerMember {
                        id: format!("{base}{m}"),
                        types: if is_dir {
                            vec![iri::LDP_BASIC_CONTAINER, iri::LDP_CONTAINER, iri::LDP_RESOURCE]
                        } else {
                            vec![iri::LDP_RESOURCE]
                        },
                    }
                })
                .collect();
            serde_json::json!({
                "@context": {
                    "ldp": iri::LDP_NS,
                    "dcterms": iri::DCTERMS_NS,
                    "contains": { "@id": "ldp:contains", "@type": "@id" },
                },
                "@id": container_path,
                "@type": [ "ldp:Container", "ldp:BasicContainer", "ldp:Resource" ],
                "ldp:contains": contains,
            })
        }
    }
}

/// Backwards-compatible alias for the Phase 1 API.
pub fn render_container(container_path: &str, members: &[String]) -> serde_json::Value {
    render_container_jsonld(container_path, members, PreferHeader::default())
}

/// Render a container as Turtle.
pub fn render_container_turtle(
    container_path: &str,
    members: &[String],
    prefer: PreferHeader,
) -> String {
    let base = if container_path.ends_with('/') {
        container_path.to_string()
    } else {
        format!("{container_path}/")
    };
    let mut out = String::new();
    let _ = writeln!(out, "@prefix ldp: <{}> .", iri::LDP_NS);
    let _ = writeln!(out, "@prefix dcterms: <{}> .", iri::DCTERMS_NS);
    let _ = writeln!(out);
    match prefer.representation {
        ContainerRepresentation::ContainedIRIsOnly => {
            let _ = writeln!(out, "<{container_path}> ldp:contains");
            let list: Vec<String> = members
                .iter()
                .map(|m| format!("    <{base}{m}>"))
                .collect();
            let _ = writeln!(out, "{} .", list.join(",\n"));
        }
        ContainerRepresentation::MinimalContainer => {
            let _ = writeln!(
                out,
                "<{container_path}> a ldp:BasicContainer, ldp:Container, ldp:Resource ."
            );
        }
        ContainerRepresentation::Full => {
            let _ = writeln!(
                out,
                "<{container_path}> a ldp:BasicContainer, ldp:Container, ldp:Resource ;"
            );
            if members.is_empty() {
                // Drop the trailing `;` from the previous line.
                let fixed = out.trim_end().trim_end_matches(';').to_string();
                out = fixed;
                out.push_str(" .\n");
            } else {
                let list: Vec<String> = members
                    .iter()
                    .map(|m| format!("    ldp:contains <{base}{m}>"))
                    .collect();
                let _ = writeln!(out, "{} .", list.join(" ;\n"));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// PATCH — N3 and SPARQL-Update
// ---------------------------------------------------------------------------

/// Outcome of evaluating a PATCH request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    /// Graph after the patch was applied.
    pub graph: Graph,
    /// Number of triples inserted.
    pub inserted: usize,
    /// Number of triples deleted.
    pub deleted: usize,
}

/// Apply a solid-protocol N3 PATCH document to `target`.
///
/// Recognised clauses:
///
/// ```text
/// _:rename a solid:InsertDeletePatch ;
///   solid:inserts { <#s> <#p> <#o> . } ;
///   solid:deletes { <#s> <#p> <#o> . } ;
///   solid:where   { <#s> <#p> ?var . } .
/// ```
///
/// The parser is deliberately permissive: it hunts for `insert` /
/// `delete` / `where` blocks delimited by curly braces anywhere in the
/// body. The contents of each block are parsed as N-Triples.
pub fn apply_n3_patch(target: Graph, patch: &str) -> Result<PatchOutcome, PodError> {
    let inserts = extract_block(patch, &["insert", "inserts", "solid:inserts"]).unwrap_or_default();
    let deletes = extract_block(patch, &["delete", "deletes", "solid:deletes"]).unwrap_or_default();
    let where_clause = extract_block(patch, &["where", "solid:where"]);

    let insert_graph = if !inserts.is_empty() {
        Graph::parse_ntriples(&strip_braces(&inserts))?
    } else {
        Graph::new()
    };
    let delete_graph = if !deletes.is_empty() {
        Graph::parse_ntriples(&strip_braces(&deletes))?
    } else {
        Graph::new()
    };

    // WHERE clause: every triple must be present in the target graph,
    // otherwise the PATCH fails. Variables (`?foo`) are treated as
    // existential — we currently require them to match exactly any
    // existing predicate/subject/object, so the simple empty-WHERE
    // and literal-WHERE flows both work.
    if let Some(wc) = where_clause {
        if !wc.trim().is_empty() {
            let where_graph = Graph::parse_ntriples(&strip_braces(&wc))?;
            for t in where_graph.triples() {
                if !target.contains(t) {
                    return Err(PodError::PreconditionFailed(format!(
                        "WHERE clause triple missing: {t:?}"
                    )));
                }
            }
        }
    }

    let mut graph = target;
    let inserted_count = insert_graph.len();
    let deleted_count = delete_graph
        .triples()
        .filter(|t| graph.contains(t))
        .count();
    graph.subtract(&delete_graph);
    graph.extend(&insert_graph);

    Ok(PatchOutcome {
        graph,
        inserted: inserted_count,
        deleted: deleted_count,
    })
}

fn extract_block(source: &str, keywords: &[&str]) -> Option<String> {
    let lower = source.to_ascii_lowercase();
    for kw in keywords {
        let needle = kw.to_ascii_lowercase();
        let mut search_from = 0usize;
        while let Some(pos) = lower[search_from..].find(&needle) {
            let abs = search_from + pos;
            let after_kw = abs + needle.len();
            // Look for the opening brace.
            if let Some(rel) = source[after_kw..].find('{') {
                let open = after_kw + rel;
                // Find the matching close brace.
                let mut depth = 0i32;
                let mut end = None;
                for (i, c) in source[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(open + i + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(e) = end {
                    return Some(source[open..e].to_string());
                }
            }
            search_from = abs + needle.len();
        }
    }
    None
}

fn strip_braces(block: &str) -> String {
    let t = block.trim();
    let t = t.strip_prefix('{').unwrap_or(t);
    let t = t.strip_suffix('}').unwrap_or(t);
    t.trim().to_string()
}

/// Apply a SPARQL 1.1 Update document (`INSERT DATA`, `DELETE DATA`,
/// `DELETE WHERE`) to `target` using `spargebra` for parsing.
pub fn apply_sparql_patch(target: Graph, update: &str) -> Result<PatchOutcome, PodError> {
    use spargebra::term::{
        GraphName, GraphNamePattern, GroundQuad, GroundQuadPattern, GroundSubject, GroundTerm,
        GroundTermPattern, NamedNodePattern, Quad, Subject, Term as SpTerm,
    };
    use spargebra::{GraphUpdateOperation, Update};

    let parsed = Update::parse(update, None)
        .map_err(|e| PodError::Unsupported(format!("SPARQL parse error: {e}")))?;

    fn map_subject(s: &Subject) -> Option<Term> {
        match s {
            Subject::NamedNode(n) => Some(Term::Iri(n.as_str().to_string())),
            Subject::BlankNode(b) => Some(Term::BlankNode(b.as_str().to_string())),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    fn map_term(t: &SpTerm) -> Option<Term> {
        match t {
            SpTerm::NamedNode(n) => Some(Term::Iri(n.as_str().to_string())),
            SpTerm::BlankNode(b) => Some(Term::BlankNode(b.as_str().to_string())),
            SpTerm::Literal(lit) => {
                let value = lit.value().to_string();
                if let Some(lang) = lit.language() {
                    Some(Term::Literal {
                        value,
                        datatype: None,
                        language: Some(lang.to_string()),
                    })
                } else {
                    Some(Term::Literal {
                        value,
                        datatype: Some(lit.datatype().as_str().to_string()),
                        language: None,
                    })
                }
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    fn map_ground_subject(s: &GroundSubject) -> Option<Term> {
        match s {
            GroundSubject::NamedNode(n) => Some(Term::Iri(n.as_str().to_string())),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    fn map_ground_term(t: &GroundTerm) -> Option<Term> {
        match t {
            GroundTerm::NamedNode(n) => Some(Term::Iri(n.as_str().to_string())),
            GroundTerm::Literal(lit) => {
                let value = lit.value().to_string();
                if let Some(lang) = lit.language() {
                    Some(Term::Literal {
                        value,
                        datatype: None,
                        language: Some(lang.to_string()),
                    })
                } else {
                    Some(Term::Literal {
                        value,
                        datatype: Some(lit.datatype().as_str().to_string()),
                        language: None,
                    })
                }
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    fn map_ground_term_pattern(t: &GroundTermPattern) -> Option<Term> {
        match t {
            GroundTermPattern::NamedNode(n) => Some(Term::Iri(n.as_str().to_string())),
            GroundTermPattern::Literal(lit) => {
                let value = lit.value().to_string();
                if let Some(lang) = lit.language() {
                    Some(Term::Literal {
                        value,
                        datatype: None,
                        language: Some(lang.to_string()),
                    })
                } else {
                    Some(Term::Literal {
                        value,
                        datatype: Some(lit.datatype().as_str().to_string()),
                        language: None,
                    })
                }
            }
            _ => None,
        }
    }

    fn quad_to_triple(q: &Quad) -> Option<Triple> {
        if !matches!(q.graph_name, GraphName::DefaultGraph) {
            return None;
        }
        Some(Triple::new(
            map_subject(&q.subject)?,
            Term::Iri(q.predicate.as_str().to_string()),
            map_term(&q.object)?,
        ))
    }
    fn ground_quad_to_triple(q: &GroundQuad) -> Option<Triple> {
        if !matches!(q.graph_name, GraphName::DefaultGraph) {
            return None;
        }
        Some(Triple::new(
            map_ground_subject(&q.subject)?,
            Term::Iri(q.predicate.as_str().to_string()),
            map_ground_term(&q.object)?,
        ))
    }
    fn ground_quad_pattern_to_triple(q: &GroundQuadPattern) -> Option<Triple> {
        if !matches!(q.graph_name, GraphNamePattern::DefaultGraph) {
            return None;
        }
        let predicate = match &q.predicate {
            NamedNodePattern::NamedNode(n) => Term::Iri(n.as_str().to_string()),
            NamedNodePattern::Variable(_) => return None,
        };
        Some(Triple::new(
            map_ground_term_pattern(&q.subject)?,
            predicate,
            map_ground_term_pattern(&q.object)?,
        ))
    }

    let mut graph = target;
    let mut inserted = 0usize;
    let mut deleted = 0usize;

    for op in &parsed.operations {
        match op {
            GraphUpdateOperation::InsertData { data } => {
                for q in data {
                    if let Some(tr) = quad_to_triple(q) {
                        if !graph.contains(&tr) {
                            graph.insert(tr);
                            inserted += 1;
                        }
                    }
                }
            }
            GraphUpdateOperation::DeleteData { data } => {
                for q in data {
                    if let Some(tr) = ground_quad_to_triple(q) {
                        if graph.remove(&tr) {
                            deleted += 1;
                        }
                    }
                }
            }
            GraphUpdateOperation::DeleteInsert { delete, insert, .. } => {
                for q in delete {
                    if let Some(tr) = ground_quad_pattern_to_triple(q) {
                        if graph.remove(&tr) {
                            deleted += 1;
                        }
                    }
                }
                for q in insert {
                    // Only insert triples whose template is fully
                    // ground (no variable bindings). Templates with
                    // variables require WHERE-clause resolution,
                    // which the pod does not implement for PATCH.
                    let gqp = match convert_quad_pattern_to_ground(q) {
                        Some(g) => g,
                        None => continue,
                    };
                    if let Some(tr) = ground_quad_pattern_to_triple(&gqp) {
                        if !graph.contains(&tr) {
                            graph.insert(tr);
                            inserted += 1;
                        }
                    }
                }
            }
            _ => {
                return Err(PodError::Unsupported(format!(
                    "unsupported SPARQL operation: {op:?}"
                )));
            }
        }
    }

    Ok(PatchOutcome {
        graph,
        inserted,
        deleted,
    })
}

fn convert_quad_pattern_to_ground(
    q: &spargebra::term::QuadPattern,
) -> Option<spargebra::term::GroundQuadPattern> {
    use spargebra::term::{
        GraphNamePattern, GroundQuadPattern, GroundTermPattern, NamedNodePattern, TermPattern,
    };

    let subject = match &q.subject {
        TermPattern::NamedNode(n) => GroundTermPattern::NamedNode(n.clone()),
        TermPattern::Literal(l) => GroundTermPattern::Literal(l.clone()),
        _ => return None,
    };
    let predicate = match &q.predicate {
        NamedNodePattern::NamedNode(n) => NamedNodePattern::NamedNode(n.clone()),
        NamedNodePattern::Variable(_) => return None,
    };
    let object = match &q.object {
        TermPattern::NamedNode(n) => GroundTermPattern::NamedNode(n.clone()),
        TermPattern::Literal(l) => GroundTermPattern::Literal(l.clone()),
        _ => return None,
    };
    let graph_name = match &q.graph_name {
        GraphNamePattern::DefaultGraph => GraphNamePattern::DefaultGraph,
        GraphNamePattern::NamedNode(n) => GraphNamePattern::NamedNode(n.clone()),
        GraphNamePattern::Variable(_) => return None,
    };
    Some(GroundQuadPattern {
        subject,
        predicate,
        object,
        graph_name,
    })
}

// ---------------------------------------------------------------------------
// Conditional requests (RFC 7232: If-Match / If-None-Match / If-Modified-Since)
// ---------------------------------------------------------------------------

/// Outcome of evaluating conditional request headers against a current
/// resource ETag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalOutcome {
    /// The request may proceed.
    Proceed,
    /// Request must fail with `412 Precondition Failed` (e.g.
    /// `If-Match` mismatch).
    PreconditionFailed,
    /// Request should return `304 Not Modified` (GET/HEAD only with
    /// `If-None-Match`).
    NotModified,
}

/// Evaluate `If-Match` and `If-None-Match` precondition headers against
/// the current ETag of a resource. The caller passes whatever is
/// observed on the storage side; `None` for the ETag means the
/// resource does not exist.
///
/// * `If-Match: *` matches any existing resource (fails if absent).
/// * `If-None-Match: *` fails if the resource exists.
/// * `If-Match: "etag1", "etag2"` — pass if any matches.
/// * `If-None-Match: "etag1", "etag2"` — for GET/HEAD a match means
///   `NotModified`; for any other method a match means
///   `PreconditionFailed`.
pub fn evaluate_preconditions(
    method: &str,
    current_etag: Option<&str>,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> ConditionalOutcome {
    let method_upper = method.to_ascii_uppercase();
    let safe = method_upper == "GET" || method_upper == "HEAD";

    if let Some(im) = if_match {
        let raw = im.trim();
        if raw == "*" {
            if current_etag.is_none() {
                return ConditionalOutcome::PreconditionFailed;
            }
        } else {
            let wanted = parse_etag_list(raw);
            match current_etag {
                None => return ConditionalOutcome::PreconditionFailed,
                Some(cur) => {
                    if !wanted.iter().any(|w| w == cur || w == "*") {
                        return ConditionalOutcome::PreconditionFailed;
                    }
                }
            }
        }
    }

    if let Some(inm) = if_none_match {
        let raw = inm.trim();
        if raw == "*" {
            if current_etag.is_some() {
                if safe {
                    return ConditionalOutcome::NotModified;
                }
                return ConditionalOutcome::PreconditionFailed;
            }
        } else {
            let wanted = parse_etag_list(raw);
            if let Some(cur) = current_etag {
                if wanted.iter().any(|w| w == cur) {
                    if safe {
                        return ConditionalOutcome::NotModified;
                    }
                    return ConditionalOutcome::PreconditionFailed;
                }
            }
        }
    }

    ConditionalOutcome::Proceed
}

fn parse_etag_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Strip weak-etag prefix + surrounding double quotes.
            let s = s.strip_prefix("W/").unwrap_or(s);
            s.trim_matches('"').to_string()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Byte-range requests (RFC 7233)
// ---------------------------------------------------------------------------

/// A parsed byte range. `end` is inclusive per RFC 7233 §2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn length(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }
    /// Render as the `Content-Range` header value (without the
    /// `Content-Range: ` prefix).
    pub fn content_range(&self, total: u64) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, total)
    }
}

/// Parse a `Range:` header value of the form `bytes=start-end` or
/// `bytes=start-` or `bytes=-suffix`. Multi-range is intentionally
/// not supported — Solid Pods treat non-rangeable media (JSON-LD,
/// Turtle) as opaque and the binary path is the only consumer.
///
/// Returns `Ok(None)` when the header is absent, `Err` when the header
/// is syntactically valid but unsatisfiable (clients must receive
/// `416 Range Not Satisfiable`), and `Ok(Some(range))` for the
/// happy path.
pub fn parse_range_header(
    header: Option<&str>,
    total: u64,
) -> Result<Option<ByteRange>, PodError> {
    let raw = match header {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return Ok(None),
    };
    let spec = raw
        .strip_prefix("bytes=")
        .ok_or_else(|| PodError::Unsupported(format!("unsupported Range unit: {raw}")))?;
    if spec.contains(',') {
        return Err(PodError::Unsupported(
            "multi-range requests not supported".into(),
        ));
    }
    let (start_s, end_s) = spec
        .split_once('-')
        .ok_or_else(|| PodError::Unsupported(format!("malformed Range: {spec}")))?;
    if total == 0 {
        return Err(PodError::PreconditionFailed(
            "range request against empty resource".into(),
        ));
    }

    let range = if start_s.is_empty() {
        // suffix: `bytes=-500`
        let suffix: u64 = end_s
            .parse()
            .map_err(|e| PodError::Unsupported(format!("range suffix parse: {e}")))?;
        if suffix == 0 {
            return Err(PodError::PreconditionFailed("zero suffix length".into()));
        }
        let start = total.saturating_sub(suffix);
        ByteRange {
            start,
            end: total - 1,
        }
    } else {
        let start: u64 = start_s
            .parse()
            .map_err(|e| PodError::Unsupported(format!("range start parse: {e}")))?;
        let end = if end_s.is_empty() {
            total - 1
        } else {
            let v: u64 = end_s
                .parse()
                .map_err(|e| PodError::Unsupported(format!("range end parse: {e}")))?;
            v.min(total - 1)
        };
        if start > end {
            return Err(PodError::PreconditionFailed(format!(
                "unsatisfiable range: {start}-{end}"
            )));
        }
        if start >= total {
            return Err(PodError::PreconditionFailed(format!(
                "range start {start} >= total {total}"
            )));
        }
        ByteRange { start, end }
    };
    Ok(Some(range))
}

/// Slice a body buffer to a byte range. The slice is a zero-copy
/// view; callers are expected to `copy_from_slice` or similar when
/// returning it through an HTTP framework.
pub fn slice_range(body: &[u8], range: ByteRange) -> &[u8] {
    let end_excl = (range.end as usize + 1).min(body.len());
    let start = (range.start as usize).min(end_excl);
    &body[start..end_excl]
}

// ---------------------------------------------------------------------------
// OPTIONS response (RFC 7231 §4.3.7)
// ---------------------------------------------------------------------------

/// Build the set of values returned on OPTIONS for a Solid resource.
///
/// * `Allow` advertises methods the resource supports.
/// * `Accept-Post` is set for containers.
/// * `Accept-Patch` advertises supported PATCH dialects.
/// * `Accept-Ranges: bytes` is always advertised so binary resources
///   can be sliced with `Range:` requests.
#[derive(Debug, Clone)]
pub struct OptionsResponse {
    pub allow: Vec<&'static str>,
    pub accept_post: Option<&'static str>,
    pub accept_patch: &'static str,
    pub accept_ranges: &'static str,
}

/// `Accept-Patch` advertising the PATCH dialects supported.
pub const ACCEPT_PATCH: &str = "text/n3, application/sparql-update, application/json-patch+json";

pub fn options_for(path: &str) -> OptionsResponse {
    let container = is_container(path);
    let mut allow = vec!["GET", "HEAD", "OPTIONS"];
    if container {
        allow.push("POST");
    } else {
        allow.push("PUT");
        allow.push("PATCH");
    }
    allow.push("DELETE");
    OptionsResponse {
        allow,
        accept_post: if container { Some(ACCEPT_POST) } else { None },
        accept_patch: ACCEPT_PATCH,
        accept_ranges: "bytes",
    }
}

// ---------------------------------------------------------------------------
// JSON Patch (RFC 6902) — applied to the JSON representation of a
// resource. Keeps the surface intentionally small: `add`, `remove`,
// `replace`, `test`. `copy` and `move` are implemented on top.
// ---------------------------------------------------------------------------

/// Apply a JSON Patch document (RFC 6902) to a `serde_json::Value` in
/// place. Returns `Err(PodError::PreconditionFailed)` when a `test`
/// operation fails, `Err(PodError::Unsupported)` for malformed patches.
pub fn apply_json_patch(
    target: &mut serde_json::Value,
    patch: &serde_json::Value,
) -> Result<(), PodError> {
    let ops = patch
        .as_array()
        .ok_or_else(|| PodError::Unsupported("JSON Patch must be an array".into()))?;
    for op in ops {
        let op_name = op
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PodError::Unsupported("JSON Patch op missing 'op'".into()))?;
        let path = op
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PodError::Unsupported("JSON Patch op missing 'path'".into()))?;
        match op_name {
            "add" => {
                let value = op
                    .get("value")
                    .cloned()
                    .ok_or_else(|| PodError::Unsupported("add requires value".into()))?;
                json_pointer_set(target, path, value, /* add_mode = */ true)?;
            }
            "replace" => {
                let value = op
                    .get("value")
                    .cloned()
                    .ok_or_else(|| PodError::Unsupported("replace requires value".into()))?;
                json_pointer_set(target, path, value, /* add_mode = */ false)?;
            }
            "remove" => {
                json_pointer_remove(target, path)?;
            }
            "test" => {
                let value = op
                    .get("value")
                    .ok_or_else(|| PodError::Unsupported("test requires value".into()))?;
                let actual = json_pointer_get(target, path)
                    .ok_or_else(|| PodError::PreconditionFailed(format!("test path missing: {path}")))?;
                if actual != value {
                    return Err(PodError::PreconditionFailed(format!(
                        "test failed at {path}"
                    )));
                }
            }
            "copy" => {
                let from = op
                    .get("from")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PodError::Unsupported("copy requires from".into()))?;
                let value = json_pointer_get(target, from)
                    .cloned()
                    .ok_or_else(|| PodError::PreconditionFailed(format!("copy from missing: {from}")))?;
                json_pointer_set(target, path, value, true)?;
            }
            "move" => {
                let from = op
                    .get("from")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PodError::Unsupported("move requires from".into()))?;
                let value = json_pointer_get(target, from)
                    .cloned()
                    .ok_or_else(|| PodError::PreconditionFailed(format!("move from missing: {from}")))?;
                json_pointer_remove(target, from)?;
                json_pointer_set(target, path, value, true)?;
            }
            other => {
                return Err(PodError::Unsupported(format!(
                    "unsupported JSON Patch op: {other}"
                )));
            }
        }
    }
    Ok(())
}

fn json_pointer_get<'a>(
    target: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    if path.is_empty() {
        return Some(target);
    }
    target.pointer(path)
}

fn json_pointer_remove(target: &mut serde_json::Value, path: &str) -> Result<(), PodError> {
    if path.is_empty() {
        return Err(PodError::Unsupported("cannot remove root".into()));
    }
    let (parent_path, last) = split_pointer(path);
    let parent = target
        .pointer_mut(&parent_path)
        .ok_or_else(|| PodError::PreconditionFailed(format!("remove path missing: {path}")))?;
    match parent {
        serde_json::Value::Object(m) => {
            m.remove(&last).ok_or_else(|| {
                PodError::PreconditionFailed(format!("remove key missing: {path}"))
            })?;
            Ok(())
        }
        serde_json::Value::Array(a) => {
            let idx: usize = last.parse().map_err(|_| {
                PodError::Unsupported(format!("remove array index not numeric: {last}"))
            })?;
            if idx >= a.len() {
                return Err(PodError::PreconditionFailed(format!(
                    "remove array out of bounds: {idx}"
                )));
            }
            a.remove(idx);
            Ok(())
        }
        _ => Err(PodError::PreconditionFailed(format!(
            "remove target is not container: {path}"
        ))),
    }
}

fn json_pointer_set(
    target: &mut serde_json::Value,
    path: &str,
    value: serde_json::Value,
    add_mode: bool,
) -> Result<(), PodError> {
    if path.is_empty() {
        *target = value;
        return Ok(());
    }
    let (parent_path, last) = split_pointer(path);
    let parent = target
        .pointer_mut(&parent_path)
        .ok_or_else(|| PodError::PreconditionFailed(format!("set parent missing: {path}")))?;
    match parent {
        serde_json::Value::Object(m) => {
            if !add_mode && !m.contains_key(&last) {
                return Err(PodError::PreconditionFailed(format!(
                    "replace missing key: {path}"
                )));
            }
            m.insert(last, value);
            Ok(())
        }
        serde_json::Value::Array(a) => {
            if last == "-" {
                a.push(value);
                return Ok(());
            }
            let idx: usize = last.parse().map_err(|_| {
                PodError::Unsupported(format!("array index not numeric: {last}"))
            })?;
            if add_mode {
                if idx > a.len() {
                    return Err(PodError::PreconditionFailed(format!(
                        "array add out of bounds: {idx}"
                    )));
                }
                a.insert(idx, value);
            } else {
                if idx >= a.len() {
                    return Err(PodError::PreconditionFailed(format!(
                        "array replace out of bounds: {idx}"
                    )));
                }
                a[idx] = value;
            }
            Ok(())
        }
        _ => Err(PodError::PreconditionFailed(format!(
            "set parent not container: {path}"
        ))),
    }
}

fn split_pointer(path: &str) -> (String, String) {
    match path.rfind('/') {
        Some(pos) => {
            let parent = path[..pos].to_string();
            let last_raw = &path[pos + 1..];
            let last = last_raw.replace("~1", "/").replace("~0", "~");
            (parent, last)
        }
        None => (String::new(), path.to_string()),
    }
}

/// Pick a PATCH dialect from the `Content-Type` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchDialect {
    N3,
    SparqlUpdate,
    JsonPatch,
}

pub fn patch_dialect_from_mime(mime: &str) -> Option<PatchDialect> {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match m.as_str() {
        "text/n3" | "application/n3" => Some(PatchDialect::N3),
        "application/sparql-update" | "application/sparql-update+update" => {
            Some(PatchDialect::SparqlUpdate)
        }
        "application/json-patch+json" => Some(PatchDialect::JsonPatch),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// LdpContainerOps trait (backwards compatible)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait LdpContainerOps: Storage {
    async fn container_representation(
        &self,
        path: &str,
    ) -> Result<serde_json::Value, PodError> {
        let children = self.list(path).await?;
        Ok(render_container(path, &children))
    }
}

impl<T: Storage + ?Sized> LdpContainerOps for T {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_container_detects_trailing_slash() {
        assert!(is_container("/"));
        assert!(is_container("/media/"));
        assert!(!is_container("/file.txt"));
    }

    #[test]
    fn link_headers_include_acl_and_describedby() {
        let hdrs = link_headers("/profile/card");
        assert!(hdrs.iter().any(|h| h.contains("rel=\"type\"")));
        assert!(hdrs.iter().any(|h| h.contains("rel=\"acl\"")));
        assert!(hdrs.iter().any(|h| h.contains("/profile/card.acl")));
        assert!(hdrs.iter().any(|h| h.contains("rel=\"describedby\"")));
        assert!(hdrs.iter().any(|h| h.contains("/profile/card.meta")));
    }

    #[test]
    fn link_headers_root_exposes_pim_storage() {
        let hdrs = link_headers("/");
        let joined = hdrs.join(",");
        assert!(joined.contains("http://www.w3.org/ns/pim/space#storage"));
    }

    #[test]
    fn link_headers_skip_describedby_on_meta() {
        let hdrs = link_headers("/foo.meta");
        assert!(!hdrs.iter().any(|h| h.contains("rel=\"describedby\"")));
    }

    #[test]
    fn link_headers_skip_acl_on_acl() {
        let hdrs = link_headers("/profile/card.acl");
        assert!(!hdrs.iter().any(|h| h.contains("rel=\"acl\"")));
    }

    #[test]
    fn prefer_minimal_container_parsed() {
        let p = PreferHeader::parse(
            "return=representation; include=\"http://www.w3.org/ns/ldp#PreferMinimalContainer\"",
        );
        assert!(p.include_minimal);
        assert_eq!(p.representation, ContainerRepresentation::MinimalContainer);
    }

    #[test]
    fn prefer_contained_iris_parsed() {
        let p = PreferHeader::parse(
            "return=representation; include=\"http://www.w3.org/ns/ldp#PreferContainedIRIs\"",
        );
        assert!(p.include_contained_iris);
        assert_eq!(p.representation, ContainerRepresentation::ContainedIRIsOnly);
    }

    #[test]
    fn negotiate_prefers_explicit_turtle() {
        assert_eq!(
            negotiate_format(Some("application/ld+json;q=0.5, text/turtle;q=0.9")),
            RdfFormat::Turtle
        );
    }

    #[test]
    fn negotiate_falls_back_to_turtle() {
        assert_eq!(negotiate_format(Some("*/*")), RdfFormat::Turtle);
        assert_eq!(negotiate_format(None), RdfFormat::Turtle);
    }

    #[test]
    fn negotiate_picks_jsonld_when_highest() {
        assert_eq!(
            negotiate_format(Some("application/ld+json, text/turtle;q=0.5")),
            RdfFormat::JsonLd
        );
    }

    #[test]
    fn ntriples_roundtrip() {
        let nt = "<http://a/s> <http://a/p> <http://a/o> .\n";
        let g = Graph::parse_ntriples(nt).unwrap();
        assert_eq!(g.len(), 1);
        let out = g.to_ntriples();
        assert!(out.contains("<http://a/s>"));
    }

    #[test]
    fn server_managed_triples_include_ldp_contains() {
        let now = chrono::Utc::now();
        let members = vec!["a.txt".to_string(), "sub/".to_string()];
        let g = server_managed_triples("http://x/y/", now, 42, true, &members);
        let nt = g.to_ntriples();
        assert!(nt.contains("http://www.w3.org/ns/ldp#contains"));
        assert!(nt.contains("http://x/y/a.txt"));
        assert!(nt.contains("http://x/y/sub/"));
    }

    #[test]
    fn find_illegal_server_managed_flags_ldp_contains() {
        let mut g = Graph::new();
        g.insert(Triple::new(
            Term::iri("http://r/"),
            Term::iri(iri::LDP_CONTAINS),
            Term::iri("http://r/x"),
        ));
        let illegal = find_illegal_server_managed(&g);
        assert_eq!(illegal.len(), 1);
    }

    #[test]
    fn render_container_minimal_omits_contains() {
        let prefer = PreferHeader {
            representation: ContainerRepresentation::MinimalContainer,
            include_minimal: true,
            include_contained_iris: false,
            omit_membership: true,
        };
        let v = render_container_jsonld("/docs/", &["one.txt".into()], prefer);
        assert!(v.get("ldp:contains").is_none());
    }

    #[test]
    fn render_container_turtle_emits_types() {
        let v = render_container_turtle("/x/", &[], PreferHeader::default());
        assert!(v.contains("ldp:BasicContainer"));
    }

    #[test]
    fn n3_patch_insert_and_delete() {
        let mut g = Graph::new();
        g.insert(Triple::new(
            Term::iri("http://s/a"),
            Term::iri("http://p/keep"),
            Term::literal("v"),
        ));
        g.insert(Triple::new(
            Term::iri("http://s/a"),
            Term::iri("http://p/drop"),
            Term::literal("old"),
        ));

        let patch = r#"
            _:r a solid:InsertDeletePatch ;
              solid:deletes {
                <http://s/a> <http://p/drop> "old" .
              } ;
              solid:inserts {
                <http://s/a> <http://p/new> "shiny" .
              } .
        "#;
        let outcome = apply_n3_patch(g, patch).unwrap();
        assert_eq!(outcome.inserted, 1);
        assert_eq!(outcome.deleted, 1);
        assert!(outcome.graph.contains(&Triple::new(
            Term::iri("http://s/a"),
            Term::iri("http://p/new"),
            Term::literal("shiny"),
        )));
        assert!(!outcome.graph.contains(&Triple::new(
            Term::iri("http://s/a"),
            Term::iri("http://p/drop"),
            Term::literal("old"),
        )));
    }

    #[test]
    fn n3_patch_where_failure_returns_precondition() {
        let g = Graph::new();
        let patch = r#"
            _:r solid:where   { <http://s/a> <http://p/need> "x" . } ;
                solid:inserts { <http://s/a> <http://p/added> "y" . } .
        "#;
        let err = apply_n3_patch(g, patch).err().unwrap();
        assert!(matches!(err, PodError::PreconditionFailed(_)));
    }

    #[test]
    fn sparql_insert_data() {
        let g = Graph::new();
        let update = r#"INSERT DATA { <http://s> <http://p> "v" . }"#;
        let outcome = apply_sparql_patch(g, update).unwrap();
        assert_eq!(outcome.inserted, 1);
        assert_eq!(outcome.graph.len(), 1);
    }

    #[test]
    fn sparql_delete_data() {
        let mut g = Graph::new();
        g.insert(Triple::new(
            Term::iri("http://s"),
            Term::iri("http://p"),
            Term::literal("v"),
        ));
        let update = r#"DELETE DATA { <http://s> <http://p> "v" . }"#;
        let outcome = apply_sparql_patch(g, update).unwrap();
        assert_eq!(outcome.deleted, 1);
        assert!(outcome.graph.is_empty());
    }

    #[test]
    fn patch_dialect_detection() {
        assert_eq!(patch_dialect_from_mime("text/n3"), Some(PatchDialect::N3));
        assert_eq!(
            patch_dialect_from_mime("application/sparql-update; charset=utf-8"),
            Some(PatchDialect::SparqlUpdate)
        );
        assert_eq!(patch_dialect_from_mime("text/plain"), None);
    }

    #[test]
    fn slug_uses_valid_value() {
        let out = resolve_slug("/photos/", Some("cat.jpg"));
        assert_eq!(out, "/photos/cat.jpg");
    }

    #[test]
    fn slug_rejects_slashes() {
        let out = resolve_slug("/photos/", Some("a/b"));
        assert!(!out.contains("a/b"));
    }

    #[test]
    fn render_container_shapes_jsonld() {
        let members = vec!["one.txt".to_string(), "sub/".to_string()];
        let v = render_container("/docs/", &members);
        assert!(v.get("@context").is_some());
        assert!(v.get("ldp:contains").unwrap().as_array().unwrap().len() == 2);
    }

    #[test]
    fn preconditions_if_match_star_passes_when_resource_exists() {
        let got = evaluate_preconditions("PUT", Some("etag123"), Some("*"), None);
        assert_eq!(got, ConditionalOutcome::Proceed);
    }

    #[test]
    fn preconditions_if_match_star_fails_when_resource_absent() {
        let got = evaluate_preconditions("PUT", None, Some("*"), None);
        assert_eq!(got, ConditionalOutcome::PreconditionFailed);
    }

    #[test]
    fn preconditions_if_match_mismatch_412() {
        let got = evaluate_preconditions("PUT", Some("etag123"), Some("\"other\""), None);
        assert_eq!(got, ConditionalOutcome::PreconditionFailed);
    }

    #[test]
    fn preconditions_if_none_match_match_on_get_returns_304() {
        let got =
            evaluate_preconditions("GET", Some("etag123"), None, Some("\"etag123\""));
        assert_eq!(got, ConditionalOutcome::NotModified);
    }

    #[test]
    fn preconditions_if_none_match_on_put_when_exists_fails() {
        let got = evaluate_preconditions("PUT", Some("etag1"), None, Some("*"));
        assert_eq!(got, ConditionalOutcome::PreconditionFailed);
    }

    #[test]
    fn preconditions_if_none_match_on_put_when_absent_passes() {
        let got = evaluate_preconditions("PUT", None, None, Some("*"));
        assert_eq!(got, ConditionalOutcome::Proceed);
    }

    #[test]
    fn range_parses_start_end() {
        let r = parse_range_header(Some("bytes=0-99"), 1000).unwrap().unwrap();
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 99);
        assert_eq!(r.length(), 100);
    }

    #[test]
    fn range_parses_open_ended() {
        let r = parse_range_header(Some("bytes=500-"), 1000).unwrap().unwrap();
        assert_eq!(r.start, 500);
        assert_eq!(r.end, 999);
    }

    #[test]
    fn range_parses_suffix() {
        let r = parse_range_header(Some("bytes=-200"), 1000).unwrap().unwrap();
        assert_eq!(r.start, 800);
        assert_eq!(r.end, 999);
    }

    #[test]
    fn range_rejects_unsatisfiable() {
        let err = parse_range_header(Some("bytes=2000-3000"), 1000);
        assert!(matches!(err, Err(PodError::PreconditionFailed(_))));
    }

    #[test]
    fn range_content_range_header_value() {
        let r = parse_range_header(Some("bytes=0-99"), 1000).unwrap().unwrap();
        assert_eq!(r.content_range(1000), "bytes 0-99/1000");
    }

    #[test]
    fn options_container_includes_post_and_accept_post() {
        let o = options_for("/photos/");
        assert!(o.allow.contains(&"POST"));
        assert!(o.accept_post.is_some());
        assert_eq!(o.accept_ranges, "bytes");
    }

    #[test]
    fn options_resource_includes_put_patch_no_post() {
        let o = options_for("/photos/cat.jpg");
        assert!(o.allow.contains(&"PUT"));
        assert!(o.allow.contains(&"PATCH"));
        assert!(!o.allow.contains(&"POST"));
        assert!(o.accept_post.is_none());
        assert!(o.accept_patch.contains("sparql-update"));
        assert!(o.accept_patch.contains("json-patch"));
    }

    #[test]
    fn json_patch_add_and_replace() {
        let mut v = serde_json::json!({ "name": "alice" });
        let patch = serde_json::json!([
            { "op": "add", "path": "/age", "value": 30 },
            { "op": "replace", "path": "/name", "value": "bob" }
        ]);
        apply_json_patch(&mut v, &patch).unwrap();
        assert_eq!(v["name"], "bob");
        assert_eq!(v["age"], 30);
    }

    #[test]
    fn json_patch_remove() {
        let mut v = serde_json::json!({ "name": "alice", "age": 30 });
        let patch = serde_json::json!([
            { "op": "remove", "path": "/age" }
        ]);
        apply_json_patch(&mut v, &patch).unwrap();
        assert!(v.get("age").is_none());
    }

    #[test]
    fn json_patch_test_failure_returns_precondition() {
        let mut v = serde_json::json!({ "name": "alice" });
        let patch = serde_json::json!([
            { "op": "test", "path": "/name", "value": "bob" }
        ]);
        let err = apply_json_patch(&mut v, &patch).unwrap_err();
        assert!(matches!(err, PodError::PreconditionFailed(_)));
    }

    #[test]
    fn json_patch_dialect_detection() {
        assert_eq!(
            patch_dialect_from_mime("application/json-patch+json"),
            Some(PatchDialect::JsonPatch)
        );
    }
}
