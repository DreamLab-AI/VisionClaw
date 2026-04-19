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

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Access modes defined by WAC.
///
/// `acl:Write` grants both `Write` and `Append` per the WAC spec
/// (see `map_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessMode {
    Read,
    Write,
    Append,
    Control,
}

/// All access modes, used for WAC-Allow header construction.
pub const ALL_MODES: &[AccessMode] = &[
    AccessMode::Read,
    AccessMode::Write,
    AccessMode::Append,
    AccessMode::Control,
];

/// A JSON-LD ACL document, as typically stored in a `.acl` sidecar.
#[derive(Debug, Deserialize)]
pub struct AclDocument {
    #[serde(rename = "@context")]
    #[allow(dead_code)]
    pub context: Option<serde_json::Value>,

    #[serde(rename = "@graph")]
    pub graph: Option<Vec<AclAuthorization>>,
}

/// One authorization entry in the `@graph` array of an `AclDocument`.
#[derive(Debug, Deserialize)]
pub struct AclAuthorization {
    #[serde(rename = "@id")]
    #[allow(dead_code)]
    pub id: Option<String>,

    #[serde(rename = "@type")]
    #[allow(dead_code)]
    pub r#type: Option<String>,

    #[serde(rename = "acl:agent")]
    pub agent: Option<IdOrIds>,

    #[serde(rename = "acl:agentClass")]
    pub agent_class: Option<IdOrIds>,

    #[serde(rename = "acl:accessTo")]
    pub access_to: Option<IdOrIds>,

    #[serde(rename = "acl:default")]
    pub default: Option<IdOrIds>,

    #[serde(rename = "acl:mode")]
    pub mode: Option<IdOrIds>,
}

/// A JSON-LD `@id` reference. May be a single object or an array.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum IdOrIds {
    Single(IdRef),
    Multiple(Vec<IdRef>),
}

/// JSON-LD `{ "@id": "..." }` reference.
#[derive(Debug, Deserialize)]
pub struct IdRef {
    #[serde(rename = "@id")]
    pub id: String,
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

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

fn agent_matches(auth: &AclAuthorization, agent_uri: Option<&str>) -> bool {
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
    false
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate whether access should be granted.
///
/// Returns `false` if `acl_doc` is `None` (deny-by-default).
pub fn evaluate_access(
    acl_doc: Option<&AclDocument>,
    agent_uri: Option<&str>,
    resource_path: &str,
    required_mode: AccessMode,
) -> bool {
    let graph = match acl_doc.and_then(|d| d.graph.as_ref()) {
        Some(g) => g,
        None => return false,
    };
    for auth in graph {
        let granted = get_modes(auth);
        if !granted.contains(&required_mode) {
            continue;
        }
        if !agent_matches(auth, agent_uri) {
            continue;
        }
        for target in get_ids(&auth.access_to) {
            if path_matches(target, resource_path, false) {
                return true;
            }
        }
        for target in get_ids(&auth.default) {
            if path_matches(target, resource_path, true) {
                return true;
            }
        }
    }
    false
}

/// Map an HTTP method to the ACL mode it requires.
pub fn method_to_mode(method: &str) -> AccessMode {
    match method.to_uppercase().as_str() {
        "GET" | "HEAD" => AccessMode::Read,
        "PUT" | "DELETE" | "PATCH" => AccessMode::Write,
        "POST" => AccessMode::Append,
        _ => AccessMode::Read,
    }
}

/// Lowercase WAC-Allow mode name.
pub fn mode_name(mode: AccessMode) -> &'static str {
    match mode {
        AccessMode::Read => "read",
        AccessMode::Write => "write",
        AccessMode::Append => "append",
        AccessMode::Control => "control",
    }
}

/// Build a `WAC-Allow` header value for the current agent and public.
pub fn wac_allow_header(
    acl_doc: Option<&AclDocument>,
    agent_uri: Option<&str>,
    resource_path: &str,
) -> String {
    let mut user_modes = Vec::new();
    let mut public_modes = Vec::new();
    for mode in ALL_MODES {
        if evaluate_access(acl_doc, agent_uri, resource_path, *mode) {
            user_modes.push(mode_name(*mode));
        }
        if evaluate_access(acl_doc, None, resource_path, *mode) {
            public_modes.push(mode_name(*mode));
        }
    }
    format!(
        "user=\"{}\", public=\"{}\"",
        user_modes.join(" "),
        public_modes.join(" ")
    )
}

// ---------------------------------------------------------------------------
// Inherited ACL resolution via Storage
// ---------------------------------------------------------------------------

/// Resolver trait for walking up the container tree to find the
/// effective ACL.
///
/// Backends can override this to use faster caching paths. The
/// default implementation walks the path using `Storage` reads.
#[async_trait]
pub trait AclResolver: Send + Sync {
    async fn find_effective_acl(
        &self,
        resource_path: &str,
    ) -> Result<Option<AclDocument>, PodError>;
}

/// Default `AclResolver` backed by a `Storage`. Walks
/// `{path}.acl` -> parent `.acl` -> ... -> `/.acl`.
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
            if let Ok((body, _meta)) = self.storage.get(&acl_key).await {
                if let Ok(doc) = serde_json::from_slice::<AclDocument>(&body) {
                    return Ok(Some(doc));
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
            access_to: Some(IdOrIds::Single(IdRef {
                id: path.into(),
            })),
            default: None,
            mode: Some(IdOrIds::Single(IdRef { id: "acl:Read".into() })),
        }
    }

    #[test]
    fn no_acl_denies_all() {
        assert!(!evaluate_access(None, None, "/foo", AccessMode::Read));
    }

    #[test]
    fn public_read_grants_anonymous() {
        let doc = make_doc(vec![public_read("/")]);
        assert!(evaluate_access(Some(&doc), None, "/", AccessMode::Read));
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
            access_to: Some(IdOrIds::Single(IdRef { id: "/".into() })),
            default: None,
            mode: Some(IdOrIds::Single(IdRef { id: "acl:Write".into() })),
        };
        let doc = make_doc(vec![auth]);
        assert!(evaluate_access(
            Some(&doc),
            Some("did:nostr:owner"),
            "/",
            AccessMode::Append
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
}
