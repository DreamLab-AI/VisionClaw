//! Public `GET /api/server/identity` endpoint.
//!
//! Returns the server's Nostr public identity so that third parties can verify
//! server-signed events (migration approvals, bridge promotions, bead stamps,
//! audit records). Per ADR-050 §"Server-as-identity", this endpoint is
//! intentionally unauthenticated — publishing the public key is the whole
//! point.
//!
//! The secret key material is **never** exposed by this endpoint.

use actix_web::{web, HttpResponse};
use serde::Serialize;
use std::sync::Arc;

use crate::services::server_identity::ServerIdentity;

/// Payload for `GET /api/server/identity`.
#[derive(Debug, Serialize)]
pub struct ServerIdentityResponse {
    /// Hex-encoded 32-byte x-only public key (64 chars).
    pub pubkey_hex: String,
    /// Bech32 NIP-19 encoding (`npub1…`).
    pub pubkey_npub: String,
    /// Nostr event kinds this server will sign.
    pub supported_kinds: Vec<u16>,
    /// Relays to which this server broadcasts signed events.
    pub relay_urls: Vec<String>,
}

/// Handler for `GET /api/server/identity`.
pub async fn get_server_identity(
    identity: web::Data<Arc<ServerIdentity>>,
) -> HttpResponse {
    let resp = ServerIdentityResponse {
        pubkey_hex: identity.pubkey_hex(),
        pubkey_npub: identity.pubkey_npub(),
        supported_kinds: identity.supported_kinds().to_vec(),
        relay_urls: identity.relay_urls().to_vec(),
    };
    HttpResponse::Ok().json(resp)
}

/// Route wiring: mount at `/server/identity` under the `/api` scope.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/server")
            .route("/identity", web::get().to(get_server_identity)),
    );
}
