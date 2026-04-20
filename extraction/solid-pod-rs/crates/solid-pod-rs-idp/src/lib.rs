//! # solid-pod-rs-idp
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
//! - Solid-OIDC provider: `/auth`, `/token`, `/me`, `/reg`, `/session`
//!   endpoints matching JSS `src/idp/index.js`.
//! - OIDC discovery + JWKS publication.
//! - Dynamic client registration (PARITY row 75 IdP side).
//! - Client Identifier Documents (fetch + cache).
//! - Credentials flow (email + password, rate-limited).
//! - Passkeys / WebAuthn via a host-app integration trait.
//! - Schnorr SSO (NIP-07 handshake) bridging Nostr identities into
//!   Solid-OIDC sessions.
//! - HTML login / register / consent pages behind a templating trait
//!   so consumers pick their own view layer.
//!
//! ## Parity references
//!
//! PARITY-CHECKLIST rows 74–82 (GAP-ANALYSIS §E.3). Target ~3,500 LOC
//! plus templates, shipped on an independent release cycle from the
//! library core.
