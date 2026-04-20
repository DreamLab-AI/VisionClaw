//! Web Access Control evaluator.
//!
//! Parses JSON-LD ACL documents and evaluates whether a given agent
//! URI is granted a specific access mode on a resource path.
//!
//! Ported from `community-forum-rs/crates/pod-worker/src/acl.rs`;
//! kept deliberately free of WASM/Worker specifics.
//!
//! Reference: <https://solid.github.io/web-access-control-spec/>

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::PodError;
use crate::storage::Storage;

// F4 — `acl:origin` enforcement (WAC §4.3). Gated on `acl-origin`
// feature until the wider jss-v04 surface lands. Module is still
// compiled unconditionally so the shared types are always testable, but
// the evaluator only activates the gate when the feature is enabled.
pub mod origin;

pub use origin::{check_origin, extract_origin_patterns, Origin, OriginDecision, OriginPattern};

/// Access modes defined by WAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessMode {
    Read,
    Write,
    Append,
    Control,
}

pub const ALL_MODES: &[AccessMode] = &[
    AccessMode::Read,
    AccessMode::Write,
    AccessMode::Append,
    AccessMode::Control,
];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AclDocument {
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,

    #[serde(rename = "@graph", skip_serializing_if = "Option::is_none")]
    pub graph: Option<Vec<AclAuthorization>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AclAuthorization {
    #[serde(rename = "@id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    #[serde(rename = "acl:agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<IdOrIds>,

    #[serde(rename = "acl:agentClass", skip_serializing_if = "Option::is_none")]
    pub agent_class: Option<IdOrIds>,

    #[serde(rename = "acl:agentGroup", skip_serializing_if = "Option::is_none")]
    pub agent_group: Option<IdOrIds>,

    #[serde(rename = "acl:origin", skip_serializing_if = "Option::is_none")]
    pub origin: Option<IdOrIds>,

    #[serde(rename = "acl:accessTo", skip_serializing_if = "Option::is_none")]
    pub access_to: Option<IdOrIds>,

    #[serde(rename = "acl:default", skip_serializing_if = "Option::is_none")]
    pub default: Option<IdOrIds>,

    #[serde(rename = "acl:mode", skip_serializing_if = "Option::is_none")]
    pub mode: Option<IdOrIds>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum IdOrIds {
    Single(IdRef),
    Multiple(Vec<IdRef>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdRef {
    #[serde(rename = "@id")]
    pub id: String,
}

fn map_mode(mode_ref: &str) -> &'static [AccessMode] {
    match mode_ref {
        "acl:Read" | "http://www.w3.org/ns/auth/acl#Read" => &[AccessMode::Read],
        "acl:Write" | "http://www.w3.org/ns/auth/acl#Write" => {
            &[AccessMode::Write, AccessMode::Append]
        }
        "acl:Append" | "http://www.w3.org/ns/auth/acl#Append" => &[AccessMode::Append],
        "acl:Control" | "http://www.w3.org/ns/auth/acl#Control" => &[AccessMode::Control],
        _ => &[],
    }
}

fn get_ids(val: &Option<IdOrIds>) -> Vec<&str> {
    match val {
        None => Vec::new(),
        Some(IdOrIds::Single(r)) => vec![r.id.as_str()],
        Some(IdOrIds::Multiple(refs)) => refs.iter().map(|r| r.id.as_str()).collect(),
    }
}

fn normalize_path(path: &str) -> String {
    let stripped = path.strip_prefix("./").or_else(|| path.strip_prefix('.'));
    let base = match stripped {
        Some("") => "/".to_string(),
        Some(s) if !s.starts_with('/') => format!("/{s}"),
        Some(s) => s.to_string(),
        None => path.to_string(),
    };
    let trimmed = base.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_matches(rule_path: &str, resource_path: &str, is_default: bool) -> bool {
    let rule = normalize_path(rule_path);
    let resource = normalize_path(resource_path);
    if !is_default {
        resource == rule || resource.starts_with(&format!("{rule}/"))
    } else {
        resource.starts_with(&format!("{rule}/")) || resource == rule
    }
}

fn get_modes(auth: &AclAuthorization) -> Vec<AccessMode> {
    let mut modes = Vec::new();
    for mode_ref in get_ids(&auth.mode) {
        modes.extend_from_slice(map_mode(mode_ref));
    }
    modes
}

#[allow(dead_code)]
fn agent_matches(auth: &AclAuthorization, agent_uri: Option<&str>) -> bool {
    agent_matches_with_groups(auth, agent_uri, &NoGroupMembership)
}

fn agent_matches_with_groups(
    auth: &AclAuthorization,
    agent_uri: Option<&str>,
    groups: &dyn GroupMembership,
) -> bool {
    let agents = get_ids(&auth.agent);
    if let Some(uri) = agent_uri {
        if agents.contains(&uri) {
            return true;
        }
    }
    for cls in get_ids(&auth.agent_class) {
        if cls == "foaf:Agent" || cls == "http://xmlns.com/foaf/0.1/Agent" {
            return true;
        }
        if agent_uri.is_some()
            && (cls == "acl:AuthenticatedAgent"
                || cls == "http://www.w3.org/ns/auth/acl#AuthenticatedAgent")
        {
            return true;
        }
    }
    if let Some(uri) = agent_uri {
        for group_iri in get_ids(&auth.agent_group) {
            if groups.is_member(group_iri, uri) {
                return true;
            }
        }
    }
    false
}

/// Synchronous group membership lookup used by `evaluate_access_with_groups`.
///
/// Implementors resolve an `acl:agentGroup` IRI (typically a
/// `vcard:Group` document) against an agent WebID and return whether
/// the agent is a member. The default implementation returns `false`
/// for every call — consumer crates are expected to plug in their own
/// resolver (e.g. by fetching the group document and inspecting
/// `vcard:hasMember`).
pub trait GroupMembership {
    fn is_member(&self, group_iri: &str, agent_uri: &str) -> bool;
}

struct NoGroupMembership;
impl GroupMembership for NoGroupMembership {
    fn is_member(&self, _group_iri: &str, _agent_uri: &str) -> bool {
        false
    }
}

/// Static group-membership resolver used in tests and by pods that
/// resolve group documents eagerly into an in-memory map.
#[derive(Debug, Default, Clone)]
pub struct StaticGroupMembership {
    pub groups: std::collections::HashMap<String, Vec<String>>,
}

impl StaticGroupMembership {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, group_iri: impl Into<String>, members: Vec<String>) {
        self.groups.insert(group_iri.into(), members);
    }
}

impl GroupMembership for StaticGroupMembership {
    fn is_member(&self, group_iri: &str, agent_uri: &str) -> bool {
        self.groups
            .get(group_iri)
            .map(|m| m.iter().any(|x| x == agent_uri))
            .unwrap_or(false)
    }
}

/// Evaluate whether access should be granted.
///
/// The `request_origin` parameter carries the RFC 6454 origin from the
/// HTTP `Origin:` header; pass `None` for request paths that have no
/// origin context (e.g. server-to-server calls or tests). When the
/// `acl-origin` feature is enabled, any ACL that declares `acl:origin`
/// triples gates access on the request origin per WAC §4.3.
pub fn evaluate_access(
    acl_doc: Option<&AclDocument>,
    agent_uri: Option<&str>,
    resource_path: &str,
    required_mode: AccessMode,
    request_origin: Option<&origin::Origin>,
) -> bool {
    evaluate_access_with_groups(
        acl_doc,
        agent_uri,
        resource_path,
        required_mode,
        request_origin,
        &NoGroupMembership,
    )
}

/// Evaluate access with a caller-supplied group-membership resolver.
pub fn evaluate_access_with_groups(
    acl_doc: Option<&AclDocument>,
    agent_uri: Option<&str>,
    resource_path: &str,
    required_mode: AccessMode,
    request_origin: Option<&origin::Origin>,
    groups: &dyn GroupMembership,
) -> bool {
    let doc = match acl_doc {
        Some(d) => d,
        None => return false,
    };
    let graph = match doc.graph.as_ref() {
        Some(g) => g,
        None => return false,
    };
    let mut base_grant = false;
    for auth in graph {
        let granted = get_modes(auth);
        if !granted.contains(&required_mode) {
            continue;
        }
        if !agent_matches_with_groups(auth, agent_uri, groups) {
            continue;
        }
        for target in get_ids(&auth.access_to) {
            if path_matches(target, resource_path, false) {
                base_grant = true;
                break;
            }
        }
        if !base_grant {
            for target in get_ids(&auth.default) {
                if path_matches(target, resource_path, true) {
                    base_grant = true;
                    break;
                }
            }
        }
        if base_grant {
            break;
        }
    }
    if !base_grant {
        return false;
    }

    // WAC §4.3 invariant 4: Control mode bypasses the origin gate so
    // that an owner can always fix a mis-configured ACL from any
    // origin.
    if matches!(required_mode, AccessMode::Control) {
        return true;
    }

    // F4 — origin gate. Only active behind the `acl-origin` feature;
    // otherwise behave exactly as pre-F4 to preserve backward compat.
    #[cfg(feature = "acl-origin")]
    {
        match origin::check_origin(doc, request_origin) {
            origin::OriginDecision::NoPolicySet | origin::OriginDecision::Permitted => true,
            origin::OriginDecision::RejectedMismatch | origin::OriginDecision::RejectedNoOrigin => {
                metrics::ACL_ORIGIN_REJECTED_TOTAL
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            }
        }
    }
    #[cfg(not(feature = "acl-origin"))]
    {
        let _ = request_origin; // silence unused warning when feature off
        true
    }
}

// ---------------------------------------------------------------------------
// Lightweight metric counter for the acl-origin gate. When a proper
// metrics facade lands (F1/F2) this module will be swapped for its
// `Counter` type; for now we expose a minimal atomic compatible with
// whichever facade arrives.
// ---------------------------------------------------------------------------
#[cfg(feature = "acl-origin")]
pub mod metrics {
    use std::sync::atomic::AtomicU64;

    /// Total number of WAC evaluations denied by the `acl:origin` gate.
    pub static ACL_ORIGIN_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);
}

pub fn method_to_mode(method: &str) -> AccessMode {
    match method.to_uppercase().as_str() {
        "GET" | "HEAD" => AccessMode::Read,
        "PUT" | "DELETE" | "PATCH" => AccessMode::Write,
        "POST" => AccessMode::Append,
        _ => AccessMode::Read,
    }
}

pub fn mode_name(mode: AccessMode) -> &'static str {
    match mode {
        AccessMode::Read => "read",
        AccessMode::Write => "write",
        AccessMode::Append => "append",
        AccessMode::Control => "control",
    }
}

pub fn wac_allow_header(
    acl_doc: Option<&AclDocument>,
    agent_uri: Option<&str>,
    resource_path: &str,
) -> String {
    let mut user_modes = Vec::new();
    let mut public_modes = Vec::new();
    for mode in ALL_MODES {
        // `WAC-Allow` advertises static capabilities; the origin gate
        // is a per-request concern, so we evaluate without an origin
        // and leave any origin-gated rules to reject at request time.
        if evaluate_access(acl_doc, agent_uri, resource_path, *mode, None) {
            user_modes.push(mode_name(*mode));
        }
        if evaluate_access(acl_doc, None, resource_path, *mode, None) {
            public_modes.push(mode_name(*mode));
        }
    }
    format!(
        "user=\"{}\", public=\"{}\"",
        user_modes.join(" "),
        public_modes.join(" ")
    )
}

#[async_trait]
pub trait AclResolver: Send + Sync {
    async fn find_effective_acl(
        &self,
        resource_path: &str,
    ) -> Result<Option<AclDocument>, PodError>;
}

pub struct StorageAclResolver<S: Storage> {
    storage: std::sync::Arc<S>,
}

impl<S: Storage> StorageAclResolver<S> {
    pub fn new(storage: std::sync::Arc<S>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl<S: Storage> AclResolver for StorageAclResolver<S> {
    async fn find_effective_acl(
        &self,
        resource_path: &str,
    ) -> Result<Option<AclDocument>, PodError> {
        let mut path = resource_path.to_string();
        loop {
            let acl_key = if path == "/" {
                "/.acl".to_string()
            } else {
                format!("{}.acl", path.trim_end_matches('/'))
            };
            if let Ok((body, meta)) = self.storage.get(&acl_key).await {
                // Try JSON-LD first (preferred); fall back to Turtle.
                if let Ok(doc) = serde_json::from_slice::<AclDocument>(&body) {
                    return Ok(Some(doc));
                }
                let ct = meta.content_type.to_ascii_lowercase();
                let looks_turtle = ct.starts_with("text/turtle")
                    || ct.starts_with("application/turtle")
                    || ct.starts_with("application/x-turtle");
                let text = std::str::from_utf8(&body).unwrap_or("");
                if looks_turtle || text.contains("@prefix") || text.contains("acl:Authorization") {
                    if let Ok(doc) = parse_turtle_acl(text) {
                        return Ok(Some(doc));
                    }
                }
            }
            if path == "/" || path.is_empty() {
                break;
            }
            let trimmed = path.trim_end_matches('/');
            path = match trimmed.rfind('/') {
                Some(0) => "/".to_string(),
                Some(pos) => trimmed[..pos].to_string(),
                None => "/".to_string(),
            };
        }
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Turtle ACL parser (subset sufficient for WAC documents)
// ---------------------------------------------------------------------------

/// Parse a Turtle ACL document into the same `AclDocument` shape that
/// the JSON-LD deserialiser produces. Accepts the subset used by
/// real-world Solid ACL files: `@prefix` directives, `a` shorthand,
/// and `;`-separated predicate-object pairs terminated with `.`.
///
/// Non-recognised tokens are skipped — the parser is deliberately
/// forgiving so that odd whitespace or extra comments do not break it.
pub fn parse_turtle_acl(input: &str) -> Result<AclDocument, PodError> {
    let mut prefixes: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    prefixes.insert("acl".into(), "http://www.w3.org/ns/auth/acl#".into());
    prefixes.insert("foaf".into(), "http://xmlns.com/foaf/0.1/".into());
    prefixes.insert("vcard".into(), "http://www.w3.org/2006/vcard/ns#".into());

    // Strip comments (lines beginning with # outside IRIs).
    let cleaned = strip_turtle_comments(input);

    // Pull out @prefix directives.
    let mut body = String::new();
    for line in cleaned.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@prefix") {
            // `@prefix acl: <...> .`
            let rest = rest.trim();
            if let Some((name, iri_part)) = rest.split_once(':') {
                let name = name.trim().to_string();
                let iri_part = iri_part.trim().trim_end_matches('.').trim();
                let iri = iri_part.trim_start_matches('<').trim_end_matches('>').trim();
                prefixes.insert(name, iri.to_string());
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }

    // Split into statements separated by `.` at top level (respecting `<>` brackets).
    let statements = split_turtle_statements(&body);
    let mut graph: Vec<AclAuthorization> = Vec::new();
    for stmt in statements {
        if stmt.trim().is_empty() {
            continue;
        }
        if let Some(auth) = parse_turtle_authorization(&stmt, &prefixes) {
            graph.push(auth);
        }
    }
    Ok(AclDocument {
        context: None,
        graph: if graph.is_empty() { None } else { Some(graph) },
    })
}

fn strip_turtle_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let mut in_iri = false;
        let mut filtered = String::with_capacity(line.len());
        for c in line.chars() {
            match c {
                '<' => {
                    in_iri = true;
                    filtered.push(c);
                }
                '>' => {
                    in_iri = false;
                    filtered.push(c);
                }
                '#' if !in_iri => break,
                _ => filtered.push(c),
            }
        }
        out.push_str(&filtered);
        out.push('\n');
    }
    out
}

fn split_turtle_statements(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth_iri = 0i32;
    let mut in_str = false;
    for c in input.chars() {
        match c {
            '<' if !in_str => {
                depth_iri += 1;
                cur.push(c);
            }
            '>' if !in_str => {
                depth_iri = (depth_iri - 1).max(0);
                cur.push(c);
            }
            '"' => {
                in_str = !in_str;
                cur.push(c);
            }
            '.' if depth_iri == 0 && !in_str => {
                out.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

fn parse_turtle_authorization(
    stmt: &str,
    prefixes: &std::collections::HashMap<String, String>,
) -> Option<AclAuthorization> {
    // Only accept statements whose subject is followed by `a ... acl:Authorization`.
    let trimmed = stmt.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Expect form: SUBJECT PRED OBJ ; PRED OBJ ; ...
    let (_subject, body) = turtle_pop_term(trimmed)?;
    let mut auth = AclAuthorization {
        id: None,
        r#type: None,
        agent: None,
        agent_class: None,
        agent_group: None,
        origin: None,
        access_to: None,
        default: None,
        mode: None,
    };
    let mut any_authz = false;
    for pair in body.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (pred, rest) = turtle_pop_term(pair)?;
        let pred_expanded = expand_curie_or_iri(&pred, prefixes);
        let objects = parse_object_list(rest.trim(), prefixes);

        match pred_expanded.as_str() {
            "a"
            | "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
            | "rdf:type" => {
                // type statement
                if objects.iter().any(|o| {
                    o == "http://www.w3.org/ns/auth/acl#Authorization"
                        || o == "acl:Authorization"
                }) {
                    any_authz = true;
                }
            }
            "http://www.w3.org/ns/auth/acl#agent" | "acl:agent" => {
                auth.agent = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#agentClass" | "acl:agentClass" => {
                auth.agent_class = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#agentGroup" | "acl:agentGroup" => {
                auth.agent_group = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#origin" | "acl:origin" => {
                auth.origin = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#accessTo" | "acl:accessTo" => {
                auth.access_to = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#default" | "acl:default" => {
                auth.default = Some(ids_of(objects));
            }
            "http://www.w3.org/ns/auth/acl#mode" | "acl:mode" => {
                // Keep mode values as they were (expanded if prefixed).
                auth.mode = Some(ids_of(objects));
            }
            _ => {}
        }
    }
    if any_authz {
        Some(auth)
    } else {
        None
    }
}

fn turtle_pop_term(input: &str) -> Option<(String, String)> {
    let input = input.trim_start();
    if let Some(rest) = input.strip_prefix('<') {
        let end = rest.find('>')?;
        Some((rest[..end].to_string(), rest[end + 1..].to_string()))
    } else if input.starts_with('"') {
        // literal — not expected at subject/predicate positions we care about.
        None
    } else {
        // Identifier token terminated by whitespace.
        let end = input
            .find(|c: char| c.is_whitespace())
            .unwrap_or(input.len());
        Some((input[..end].to_string(), input[end..].to_string()))
    }
}

fn parse_object_list(
    input: &str,
    prefixes: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut remaining = input.trim().to_string();
    loop {
        let r = remaining.trim_start();
        if r.is_empty() {
            break;
        }
        let (tok, rest) = match turtle_pop_term(r) {
            Some(v) => v,
            None => break,
        };
        out.push(expand_curie_or_iri(&tok, prefixes));
        let r = rest.trim_start();
        if let Some(after_comma) = r.strip_prefix(',') {
            remaining = after_comma.to_string();
        } else {
            break;
        }
    }
    out
}

fn expand_curie_or_iri(
    tok: &str,
    prefixes: &std::collections::HashMap<String, String>,
) -> String {
    let tok = tok.trim();
    if tok == "a" {
        return "a".to_string();
    }
    if let Some((p, local)) = tok.split_once(':') {
        // curie if not already an IRI `<...>`
        if !p.starts_with('<') {
            if let Some(base) = prefixes.get(p) {
                return format!("{base}{local}");
            }
        }
    }
    tok.to_string()
}

fn ids_of(items: Vec<String>) -> IdOrIds {
    if items.len() == 1 {
        IdOrIds::Single(IdRef {
            id: items.into_iter().next().unwrap(),
        })
    } else {
        IdOrIds::Multiple(items.into_iter().map(|id| IdRef { id }).collect())
    }
}

/// Serialise an [`AclDocument`] as Turtle.
pub fn serialize_turtle_acl(doc: &AclDocument) -> String {
    let mut out = String::new();
    out.push_str("@prefix acl: <http://www.w3.org/ns/auth/acl#> .\n");
    out.push_str("@prefix foaf: <http://xmlns.com/foaf/0.1/> .\n\n");
    let graph = match &doc.graph {
        Some(g) => g,
        None => return out,
    };
    for (i, auth) in graph.iter().enumerate() {
        let subject = format!("<#rule-{i}>");
        out.push_str(&subject);
        out.push_str(" a acl:Authorization");
        fn emit_pairs(out: &mut String, pred: &str, vals: &Option<IdOrIds>) {
            if let Some(ids) = vals {
                let refs: Vec<&str> = match ids {
                    IdOrIds::Single(r) => vec![r.id.as_str()],
                    IdOrIds::Multiple(v) => v.iter().map(|r| r.id.as_str()).collect(),
                };
                if refs.is_empty() {
                    return;
                }
                out.push_str(" ;\n    ");
                out.push_str(pred);
                out.push(' ');
                let rendered: Vec<String> = refs
                    .iter()
                    .map(|r| {
                        if r.starts_with("http") {
                            format!("<{r}>")
                        } else {
                            r.to_string()
                        }
                    })
                    .collect();
                out.push_str(&rendered.join(", "));
            }
        }
        emit_pairs(&mut out, "acl:agent", &auth.agent);
        emit_pairs(&mut out, "acl:agentClass", &auth.agent_class);
        emit_pairs(&mut out, "acl:agentGroup", &auth.agent_group);
        emit_pairs(&mut out, "acl:origin", &auth.origin);
        emit_pairs(&mut out, "acl:accessTo", &auth.access_to);
        emit_pairs(&mut out, "acl:default", &auth.default);
        emit_pairs(&mut out, "acl:mode", &auth.mode);
        out.push_str(" .\n\n");
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_doc(graph: Vec<AclAuthorization>) -> AclDocument {
        AclDocument {
            context: None,
            graph: Some(graph),
        }
    }

    fn public_read(path: &str) -> AclAuthorization {
        AclAuthorization {
            id: None,
            r#type: None,
            agent: None,
            agent_class: Some(IdOrIds::Single(IdRef {
                id: "foaf:Agent".into(),
            })),
            agent_group: None,
            origin: None,
            access_to: Some(IdOrIds::Single(IdRef { id: path.into() })),
            default: None,
            mode: Some(IdOrIds::Single(IdRef { id: "acl:Read".into() })),
        }
    }

    #[test]
    fn no_acl_denies_all() {
        assert!(!evaluate_access(None, None, "/foo", AccessMode::Read, None));
    }

    #[test]
    fn public_read_grants_anonymous() {
        let doc = make_doc(vec![public_read("/")]);
        assert!(evaluate_access(Some(&doc), None, "/", AccessMode::Read, None));
    }

    #[test]
    fn write_implies_append() {
        let auth = AclAuthorization {
            id: None,
            r#type: None,
            agent: Some(IdOrIds::Single(IdRef {
                id: "did:nostr:owner".into(),
            })),
            agent_class: None,
            agent_group: None,
            origin: None,
            access_to: Some(IdOrIds::Single(IdRef { id: "/".into() })),
            default: None,
            mode: Some(IdOrIds::Single(IdRef { id: "acl:Write".into() })),
        };
        let doc = make_doc(vec![auth]);
        assert!(evaluate_access(
            Some(&doc),
            Some("did:nostr:owner"),
            "/",
            AccessMode::Append,
            None,
        ));
    }

    #[test]
    fn method_mapping() {
        assert_eq!(method_to_mode("GET"), AccessMode::Read);
        assert_eq!(method_to_mode("PUT"), AccessMode::Write);
        assert_eq!(method_to_mode("POST"), AccessMode::Append);
    }

    #[test]
    fn wac_allow_shape() {
        let doc = make_doc(vec![public_read("/")]);
        let hdr = wac_allow_header(Some(&doc), None, "/");
        assert_eq!(hdr, "user=\"read\", public=\"read\"");
    }

    #[test]
    fn turtle_acl_round_trip_parses_basic_rules() {
        let ttl = r#"
            @prefix acl: <http://www.w3.org/ns/auth/acl#> .
            @prefix foaf: <http://xmlns.com/foaf/0.1/> .

            <#public> a acl:Authorization ;
                acl:agentClass foaf:Agent ;
                acl:accessTo </> ;
                acl:mode acl:Read .
        "#;
        let doc = parse_turtle_acl(ttl).unwrap();
        assert!(evaluate_access(Some(&doc), None, "/", AccessMode::Read, None));
        assert!(!evaluate_access(Some(&doc), None, "/", AccessMode::Write, None));
    }

    #[test]
    fn turtle_acl_with_owner_grants_write() {
        let ttl = r#"
            @prefix acl: <http://www.w3.org/ns/auth/acl#> .

            <#owner> a acl:Authorization ;
                acl:agent <did:nostr:owner> ;
                acl:accessTo </> ;
                acl:default </> ;
                acl:mode acl:Write, acl:Control .
        "#;
        let doc = parse_turtle_acl(ttl).unwrap();
        assert!(evaluate_access(
            Some(&doc),
            Some("did:nostr:owner"),
            "/foo",
            AccessMode::Write,
            None,
        ));
    }

    #[test]
    fn serialize_turtle_acl_emits_prefixes_and_rules() {
        let doc = make_doc(vec![public_read("/")]);
        let out = serialize_turtle_acl(&doc);
        assert!(out.contains("@prefix acl:"));
        assert!(out.contains("acl:Authorization"));
        assert!(out.contains("acl:mode"));
    }
}
