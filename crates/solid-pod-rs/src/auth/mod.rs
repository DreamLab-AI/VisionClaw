//! Authentication modules.
//!
//! Phase 1 ships NIP-98 structural verification (tag layout,
//! URL/method/payload match, timestamp tolerance). Schnorr signature
//! verification is deferred to Phase 2 so the dependency footprint
//! stays small until it is needed.

pub mod nip98;
