//! `GET /.well-known/did/nostr/{pubkey}.json` — DID document endpoint.
//!
//! Serves a Tier-1 DID document for `did:nostr:<pubkey>` identities.
//! The document follows the W3C DID Core specification and advertises
//! the Nostr public key as an `Ed25519VerificationKey2020` verification
//! method (schnorr-over-secp256k1 in practice, but the DID method
//! spec uses this key type for the JSON representation).
//!
//! Response headers:
//!   * `Content-Type: application/did+json`
//!   * `Cache-Control: public, max-age=300`
//!
//! Errors:
//!   * 400 — invalid pubkey (not 64-char hex)
//!   * 404 — pubkey not found in user storage (Neo4j)

use actix_web::{web, HttpResponse};
use serde::Serialize;

use crate::adapters::neo4j_adapter::Neo4jAdapter;
use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// DID Document shape (W3C DID Core v1.0)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DidDocument {
    #[serde(rename = "@context")]
    context: Vec<String>,
    id: String,
    verification_method: Vec<VerificationMethod>,
    authentication: Vec<String>,
    assertion_method: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationMethod {
    id: String,
    #[serde(rename = "type")]
    key_type: String,
    controller: String,
    public_key_hex: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Validate a pubkey string: must be exactly 64 lowercase hex characters.
fn is_valid_hex_pubkey(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ─────────────────────────────────────────────────────────────────────────────
// Neo4j user lookup
// ─────────────────────────────────────────────────────────────────────────────

/// Check whether a pubkey is known to the system. The server's own Nostr
/// identity is always considered "known"; for other pubkeys we probe Neo4j
/// for any node whose owner matches `did:nostr:<pubkey>`.
async fn pubkey_exists(
    neo4j: &Neo4jAdapter,
    pubkey_hex: &str,
) -> bool {
    // Query Neo4j for any node owned by this pubkey.
    let cypher = "MATCH (n) WHERE n.owner = $owner RETURN n LIMIT 1";
    let did_uri = format!("did:nostr:{}", pubkey_hex);
    let q = neo4rs::Query::new(cypher.to_string()).param("owner", did_uri);
    let result = neo4j.graph().execute(q).await;
    match result {
        Ok(mut stream) => stream.next().await.ok().flatten().is_some(),
        Err(_) => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler
// ─────────────────────────────────────────────────────────────────────────────

/// `GET /.well-known/did/nostr/{pubkey}.json`
///
/// Returns a Tier-1 DID document for the given Nostr pubkey.
pub async fn get_did_document(
    path: web::Path<String>,
    app_state: web::Data<AppState>,
    server_identity: web::Data<std::sync::Arc<crate::services::server_identity::ServerIdentity>>,
) -> HttpResponse {
    let raw_pubkey = path.into_inner();

    // Strip the `.json` suffix if present (the route captures it as part of {pubkey}).
    let pubkey = raw_pubkey
        .strip_suffix(".json")
        .unwrap_or(&raw_pubkey);

    // Validate: must be 64-char hex.
    let pubkey_lower = pubkey.to_lowercase();
    if !is_valid_hex_pubkey(&pubkey_lower) {
        return HttpResponse::BadRequest()
            .content_type("application/json")
            .body(r#"{"error":"invalid pubkey: must be 64-char hex"}"#);
    }

    // The server's own pubkey is always resolvable without a Neo4j lookup.
    let is_server_key = pubkey_lower == server_identity.pubkey_hex().to_lowercase();

    if !is_server_key && !pubkey_exists(&app_state.neo4j_adapter, &pubkey_lower).await {
        return HttpResponse::NotFound()
            .content_type("application/json")
            .body(r#"{"error":"pubkey not found"}"#);
    }

    let did_id = format!("did:nostr:{}", pubkey_lower);
    let key_id = format!("{}#keys-1", did_id);

    let doc = DidDocument {
        context: vec![
            "https://www.w3.org/ns/did/v1".to_string(),
            "https://w3id.org/security/suites/ed2020/v1".to_string(),
        ],
        id: did_id.clone(),
        verification_method: vec![VerificationMethod {
            id: key_id.clone(),
            key_type: "SchnorrSecp256k1VerificationKey2019".to_string(),
            controller: did_id.clone(),
            public_key_hex: pubkey_lower,
        }],
        authentication: vec![key_id.clone()],
        assertion_method: vec![key_id],
    };

    HttpResponse::Ok()
        .content_type("application/did+json")
        .insert_header(("Cache-Control", "public, max-age=300"))
        .json(doc)
}

// ─────────────────────────────────────────────────────────────────────────────
// Route configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Mount the DID document endpoint. Called from `main.rs` outside the `/api`
/// scope so the route lives at the server root:
///
/// ```ignore
/// .configure(webxr::handlers::configure_did_nostr_routes)
/// ```
///
/// The route pattern `/.well-known/did/nostr/{pubkey}` captures both
/// `/.../{hex}.json` (browser) and `/.../{hex}` (programmatic) forms.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/.well-known/did/nostr/{pubkey}",
        web::get().to(get_did_document),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hex_pubkeys() {
        assert!(is_valid_hex_pubkey(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        ));
        assert!(is_valid_hex_pubkey(
            "0000000000000000000000000000000000000000000000000000000000000001"
        ));
    }

    #[test]
    fn invalid_hex_pubkeys() {
        // Too short
        assert!(!is_valid_hex_pubkey("abcd"));
        // Too long
        assert!(!is_valid_hex_pubkey(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2aa"
        ));
        // Non-hex
        assert!(!is_valid_hex_pubkey(
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        ));
        // Empty
        assert!(!is_valid_hex_pubkey(""));
    }
}
