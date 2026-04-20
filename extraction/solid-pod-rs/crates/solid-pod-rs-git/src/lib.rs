//! # solid-pod-rs-git
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
//! - Git HTTP smart-protocol backend (info/refs, upload-pack,
//!   receive-pack) mounted as a pod sub-scope.
//! - Path-traversal hardening matching JSS `src/handlers/git.js`.
//! - `receive.denyCurrentBranch=updateInstead` semantics for live,
//!   single-checkout pods.
//! - `Basic nostr:<token>` client support bridging NIP-98 to git clients
//!   that speak HTTP Basic only (PARITY row 69).
//! - WAC integration so repo `.git/` trees honour the enclosing pod ACL.
//!
//! ## Parity references
//!
//! PARITY-CHECKLIST row 100 (GAP-ANALYSIS §E.1). Target ~450 LOC + 12
//! integration tests at first landing.
