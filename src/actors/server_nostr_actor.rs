//! ServerNostrActor — serialises server-owned Nostr signing & broadcast.
//!
//! The actor wraps `Arc<ServerIdentity>` behind actix message handlers so that
//! other actors (saga, bead lifecycle, bridge, WAC audit) can request signed
//! server events without colocating nostr-sdk state or relay bookkeeping.
//!
//! Four message types cover the four event kinds defined by ADR-050
//! §"Server-as-identity":
//! - [`SignMigrationApproval`]  → kind 30023
//! - [`SignBridgePromotion`]    → kind 30100
//! - [`SignBeadStamp`]          → kind 30200
//! - [`SignAuditRecord`]        → kind 30300
//!
//! Each handler returns a `ResponseFuture<anyhow::Result<Event>>` so that
//! `sign_and_broadcast` can run I/O off the actor thread without blocking the
//! mailbox. Failures from the underlying broadcast are swallowed inside
//! `ServerIdentity::sign_and_broadcast` — the actor only surfaces signing
//! errors (invalid key material, malformed tags, etc.).

use actix::prelude::*;
use anyhow::Result;
use nostr_sdk::prelude::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::services::server_identity::{
    ServerIdentity, KIND_AUDIT_RECORD, KIND_BEAD_STAMP, KIND_BRIDGE_PROMOTION,
    KIND_MIGRATION_APPROVAL,
};

/// Actor handle for server-owned Nostr signing.
pub struct ServerNostrActor {
    identity: Arc<ServerIdentity>,
}

impl ServerNostrActor {
    /// Construct with an already-loaded [`ServerIdentity`].
    pub fn new(identity: Arc<ServerIdentity>) -> Self {
        Self { identity }
    }
}

impl Actor for ServerNostrActor {
    type Context = Context<Self>;
}

// -----------------------------------------------------------------------------
// Messages
// -----------------------------------------------------------------------------

/// Request a signed migration approval (kind 30023).
///
/// Content body: `{"migration_id": "...", "bridge_iri": "...", "confidence": f64}`
/// Tags: `d=migration_id`, `e=bridge_iri`.
#[derive(Message)]
#[rtype(result = "Result<Event>")]
pub struct SignMigrationApproval {
    pub migration_id: Uuid,
    pub bridge_iri: String,
    pub confidence: f64,
}

/// Request a signed bridge-to promotion event (kind 30100).
///
/// Content body: `{"from_kg": "...", "to_owl": "...", "signals": [f64...]}`
/// Tags: `from=from_kg`, `to=to_owl`.
#[derive(Message)]
#[rtype(result = "Result<Event>")]
pub struct SignBridgePromotion {
    pub from_kg: String,
    pub to_owl: String,
    pub signals: Vec<f64>,
}

/// Request a server-witnessed bead provenance stamp (kind 30200).
///
/// Content body: `{"bead_id": "...", "payload_hash": "..."}`
/// Tags: `bead=bead_id`, `sha256=payload_hash`.
#[derive(Message)]
#[rtype(result = "Result<Event>")]
pub struct SignBeadStamp {
    pub bead_id: String,
    pub payload_hash: String,
}

/// Request an opaque audit record (kind 30300).
///
/// Content body: `{"action": "...", "details": <Value>}`
/// Tags: `action=action`, `actor=actor_pubkey.unwrap_or("")`.
#[derive(Message)]
#[rtype(result = "Result<Event>")]
pub struct SignAuditRecord {
    pub action: String,
    pub actor_pubkey: Option<String>,
    pub details: serde_json::Value,
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

impl Handler<SignMigrationApproval> for ServerNostrActor {
    type Result = ResponseFuture<Result<Event>>;

    fn handle(
        &mut self,
        msg: SignMigrationApproval,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let identity = self.identity.clone();
        Box::pin(async move {
            let mig_id = msg.migration_id.to_string();
            let content = json!({
                "migration_id": mig_id,
                "bridge_iri": msg.bridge_iri,
                "confidence": msg.confidence,
            })
            .to_string();
            let tags = vec![
                Tag::custom(TagKind::Custom("d".into()), vec![mig_id.clone()]),
                Tag::custom(
                    TagKind::Custom("e".into()),
                    vec![msg.bridge_iri.clone()],
                ),
            ];
            identity
                .sign_and_broadcast(KIND_MIGRATION_APPROVAL, content, tags)
                .await
        })
    }
}

impl Handler<SignBridgePromotion> for ServerNostrActor {
    type Result = ResponseFuture<Result<Event>>;

    fn handle(
        &mut self,
        msg: SignBridgePromotion,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let identity = self.identity.clone();
        Box::pin(async move {
            let content = json!({
                "from_kg": msg.from_kg,
                "to_owl": msg.to_owl,
                "signals": msg.signals,
            })
            .to_string();
            let tags = vec![
                Tag::custom(
                    TagKind::Custom("from".into()),
                    vec![msg.from_kg.clone()],
                ),
                Tag::custom(
                    TagKind::Custom("to".into()),
                    vec![msg.to_owl.clone()],
                ),
            ];
            identity
                .sign_and_broadcast(KIND_BRIDGE_PROMOTION, content, tags)
                .await
        })
    }
}

impl Handler<SignBeadStamp> for ServerNostrActor {
    type Result = ResponseFuture<Result<Event>>;

    fn handle(&mut self, msg: SignBeadStamp, _ctx: &mut Self::Context) -> Self::Result {
        let identity = self.identity.clone();
        Box::pin(async move {
            let content = json!({
                "bead_id": msg.bead_id,
                "payload_hash": msg.payload_hash,
            })
            .to_string();
            let tags = vec![
                Tag::custom(
                    TagKind::Custom("bead".into()),
                    vec![msg.bead_id.clone()],
                ),
                Tag::custom(
                    TagKind::Custom("sha256".into()),
                    vec![msg.payload_hash.clone()],
                ),
            ];
            identity
                .sign_and_broadcast(KIND_BEAD_STAMP, content, tags)
                .await
        })
    }
}

impl Handler<SignAuditRecord> for ServerNostrActor {
    type Result = ResponseFuture<Result<Event>>;

    fn handle(&mut self, msg: SignAuditRecord, _ctx: &mut Self::Context) -> Self::Result {
        let identity = self.identity.clone();
        Box::pin(async move {
            let content = json!({
                "action": msg.action,
                "details": msg.details,
            })
            .to_string();
            let actor_pubkey = msg.actor_pubkey.clone().unwrap_or_default();
            let tags = vec![
                Tag::custom(
                    TagKind::Custom("action".into()),
                    vec![msg.action.clone()],
                ),
                Tag::custom(TagKind::Custom("actor".into()), vec![actor_pubkey]),
            ];
            identity
                .sign_and_broadcast(KIND_AUDIT_RECORD, content, tags)
                .await
        })
    }
}
