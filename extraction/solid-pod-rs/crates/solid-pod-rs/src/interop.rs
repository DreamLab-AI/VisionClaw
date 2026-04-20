//! Interop / discovery helpers.
//!
//! This module rounds out the crate's public Solid surface with small,
//! framework-agnostic helpers for ecosystem discovery flows:
//!
//! - **`.well-known/solid`** — Solid Protocol §4.1.2 discovery document.
//! - **WebFinger** — RFC 7033, used to map acct: URIs to WebIDs.
//! - **NIP-05 verification** — Nostr pubkey ↔ DNS name binding.
//! - **Dev-mode session bypass** — consumer-crate helper for tests.
//!
//! None of these helpers perform network I/O on their own; they return
//! response bodies and signal objects that the consumer crate wires
//! into its HTTP server.

use serde::{Deserialize, Serialize};

use crate::error::PodError;

// ---------------------------------------------------------------------------
// .well-known/solid discovery document
// ---------------------------------------------------------------------------

/// Solid Protocol `.well-known/solid` discovery document. The doc
/// advertises the OIDC issuer, the pod URL, and the Notifications
/// endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolidWellKnown {
    #[serde(rename = "@context")]
    pub context: serde_json::Value,

    pub solid_oidc_issuer: String,

    pub notification_gateway: String,

    pub storage: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webfinger: Option<String>,
}

/// Build the discovery document for a pod root.
pub fn well_known_solid(
    pod_base: &str,
    oidc_issuer: &str,
) -> SolidWellKnown {
    let base = pod_base.trim_end_matches('/');
    SolidWellKnown {
        context: serde_json::json!("https://www.w3.org/ns/solid/terms"),
        solid_oidc_issuer: oidc_issuer.trim_end_matches('/').to_string(),
        notification_gateway: format!("{base}/.notifications"),
        storage: format!("{base}/"),
        webfinger: Some(format!("{base}/.well-known/webfinger")),
    }
}

// ---------------------------------------------------------------------------
// WebFinger (RFC 7033)
// ---------------------------------------------------------------------------

/// WebFinger JRD (JSON Resource Descriptor) response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFingerJrd {
    pub subject: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<WebFingerLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFingerLink {
    pub rel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub content_type: Option<String>,
}

/// Produce a WebFinger JRD response pointing `acct:user@host` at the
/// user's WebID. Returns `None` if the resource is not recognised.
pub fn webfinger_response(
    resource: &str,
    pod_base: &str,
    webid: &str,
) -> Option<WebFingerJrd> {
    if !resource.starts_with("acct:") && !resource.starts_with("https://") {
        return None;
    }
    let base = pod_base.trim_end_matches('/');
    Some(WebFingerJrd {
        subject: resource.to_string(),
        aliases: vec![webid.to_string()],
        links: vec![
            WebFingerLink {
                rel: "http://openid.net/specs/connect/1.0/issuer".to_string(),
                href: Some(format!("{base}/")),
                content_type: None,
            },
            WebFingerLink {
                rel: "http://www.w3.org/ns/solid#webid".to_string(),
                href: Some(webid.to_string()),
                content_type: None,
            },
            WebFingerLink {
                rel: "http://www.w3.org/ns/pim/space#storage".to_string(),
                href: Some(format!("{base}/")),
                content_type: None,
            },
        ],
    })
}

// ---------------------------------------------------------------------------
// NIP-05 verification
// ---------------------------------------------------------------------------

/// NIP-05 response document (the JSON served at
/// `.well-known/nostr.json?name=<local>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nip05Document {
    pub names: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relays: Option<std::collections::HashMap<String, Vec<String>>>,
}

/// Verify a NIP-05 identifier (`local@example.com`) against a fetched
/// NIP-05 document. Returns the resolved hex pubkey on success.
pub fn verify_nip05(
    identifier: &str,
    document: &Nip05Document,
) -> Result<String, PodError> {
    let (local, _domain) = identifier
        .split_once('@')
        .ok_or_else(|| PodError::Nip98(format!("invalid NIP-05 identifier: {identifier}")))?;
    let lookup = if local.is_empty() { "_" } else { local };
    let pubkey = document
        .names
        .get(lookup)
        .ok_or_else(|| PodError::NotFound(format!("NIP-05 name not found: {lookup}")))?;
    if pubkey.len() != 64 || hex::decode(pubkey).is_err() {
        return Err(PodError::Nip98(format!(
            "NIP-05 pubkey malformed for {identifier}"
        )));
    }
    Ok(pubkey.clone())
}

/// Build the NIP-05 document structure for a pod's own hosted names.
pub fn nip05_document(
    names: impl IntoIterator<Item = (String, String)>,
) -> Nip05Document {
    Nip05Document {
        names: names.into_iter().collect(),
        relays: None,
    }
}

// ---------------------------------------------------------------------------
// Dev-mode session bypass
// ---------------------------------------------------------------------------

/// Dev-mode session — ergonomic handle a consumer crate can plug into
/// its request-processing pipeline in place of NIP-98/OIDC verification
/// during tests or local development. The bypass is only constructable
/// via explicit allow, never through a header the client supplies.
#[derive(Debug, Clone)]
pub struct DevSession {
    pub webid: String,
    pub pubkey: Option<String>,
    pub is_admin: bool,
}

/// Build a dev-session bypass. Callers are expected to gate this on a
/// top-level `ENABLE_DEV_SESSION=1` or similar environment check —
/// the helper itself will not read env to avoid accidental activation.
pub fn dev_session(webid: impl Into<String>, is_admin: bool) -> DevSession {
    DevSession {
        webid: webid.into(),
        pubkey: None,
        is_admin,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_solid_advertises_oidc_and_storage() {
        let d = well_known_solid("https://pod.example/", "https://op.example/");
        assert_eq!(d.solid_oidc_issuer, "https://op.example");
        assert!(d.notification_gateway.ends_with(".notifications"));
        assert!(d.storage.ends_with('/'));
    }

    #[test]
    fn webfinger_returns_links_for_acct() {
        let j = webfinger_response(
            "acct:alice@pod.example",
            "https://pod.example",
            "https://pod.example/profile/card#me",
        )
        .unwrap();
        assert_eq!(j.subject, "acct:alice@pod.example");
        assert!(j.links.iter().any(|l| l.rel == "http://www.w3.org/ns/solid#webid"));
    }

    #[test]
    fn webfinger_rejects_unknown_scheme() {
        assert!(webfinger_response("mailto:a@b", "https://p", "https://w").is_none());
    }

    #[test]
    fn nip05_verify_returns_pubkey() {
        let mut names = std::collections::HashMap::new();
        names.insert("alice".to_string(), "a".repeat(64));
        let doc = nip05_document(names);
        let pk = verify_nip05("alice@pod.example", &doc).unwrap();
        assert_eq!(pk, "a".repeat(64));
    }

    #[test]
    fn nip05_verify_rejects_malformed_pubkey() {
        let mut names = std::collections::HashMap::new();
        names.insert("alice".to_string(), "shortkey".to_string());
        let doc = nip05_document(names);
        assert!(verify_nip05("alice@p", &doc).is_err());
    }

    #[test]
    fn nip05_root_name_resolves_via_underscore() {
        let mut names = std::collections::HashMap::new();
        names.insert("_".to_string(), "b".repeat(64));
        let doc = nip05_document(names);
        assert!(verify_nip05("@pod.example", &doc).is_ok());
    }

    #[test]
    fn dev_session_stores_admin_flag() {
        let s = dev_session("https://me/profile#me", true);
        assert!(s.is_admin);
        assert_eq!(s.webid, "https://me/profile#me");
    }
}
