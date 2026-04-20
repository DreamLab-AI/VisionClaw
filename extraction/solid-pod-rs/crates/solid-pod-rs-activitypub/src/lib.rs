//! # solid-pod-rs-activitypub
//!
//! Reserved for v0.5.0 implementation. See the parent workspace's
//! ADR-056 §D2 for the v0.5.0 sibling-crate strategy and
//! `docs/design/jss-parity/06-library-surface-context.md` for the
//! library-vs-server split (F7) this placeholder participates in.
//!
//! **Status: Not yet implemented. Target milestone: v0.5.0.**
//!
//! ## Planned scope
//!
//! - Actor discovery (ActivityPub §3 + NodeInfo 2.0)
//! - `POST /inbox` handling with HTTP Signature verification
//! - Outbox + federated delivery (Accept / Follow / Undo / Create)
//! - Follower / Following stores backed by `solid-pod-rs` storage
//! - NodeInfo 2.0 emission at `/.well-known/nodeinfo`
//! - Integration with `solid-pod-rs`'s WAC for per-actor authorisation
//! - SAND stack composition: AP Actor on `/profile/card` + did:nostr
//!   via `alsoKnownAs` (bundles with `solid-pod-rs-nostr`)
//!
//! ## Parity references
//!
//! PARITY-CHECKLIST rows 102–108, 131 (GAP-ANALYSIS §E.2). Target
//! ~1,200 LOC + 40 unit and 15 integration tests at first landing.
