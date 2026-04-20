//! # solid-pod-rs-nostr
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
//! - did:nostr DID Document publication at
//!   `/.well-known/did/nostr/:pubkey.json` (Tier 1 / Tier 3).
//! - did:nostr ↔ WebID resolver leveraging `alsoKnownAs` triples.
//! - Embedded Nostr relay implementing NIP-01, NIP-11, NIP-16.
//! - Integration hook with `solid-pod-rs-activitypub` for the SAND
//!   stack (AP Actor + did:nostr via `alsoKnownAs`).
//! - NIP-98 Schnorr already lives in the library core (`auth::nip98`)
//!   gated behind `nip98-schnorr`; this crate adds the relay + DID
//!   resolver surface on top.
//!
//! ## Parity references
//!
//! PARITY-CHECKLIST rows 89, 90, 101, 132 (GAP-ANALYSIS §E.4, §E.7).
//! Target 800–1,200 LOC at first landing.
