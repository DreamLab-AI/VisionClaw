//! # Security primitives (Sprint 4 / F1 + F2)
//!
//! Two narrow, orthogonal, library-level security controls promoted
//! from the HTTP binder into `solid-pod-rs` so every consumer inherits
//! them uniformly. Closes `GAP-ANALYSIS.md` §H rank 1 and
//! `PARITY-CHECKLIST.md` rows 114 (SSRF guard) and 115 (dotfile
//! allowlist). Upstream parity with `JavaScriptSolidServer/src/utils/
//! ssrf.js:15-157` and `JavaScriptSolidServer/src/server.js:265-281`.
//!
//! Design context: `docs/design/jss-parity/01-security-primitives-context.md`.
//!
//! ## Aggregates
//!
//! - [`SsrfPolicy`] — outbound URL validator. Classifies the resolved IP
//!   of a target URL and enforces operator-configured
//!   block/allow/deny lists. Use [`SsrfPolicy::resolve_and_check`]
//!   before every server-side `fetch`.
//! - [`DotfileAllowlist`] — inbound path filter. Rejects any path whose
//!   components start with `.` unless explicitly allowlisted.
//!   Default allowlist mirrors JSS (`.acl`, `.meta`).
//!
//! ## Integration points
//!
//! The primitives define the API surface; call-site wiring lands in
//! later Sprint 4 features (F7 library-server split). Required hooks
//! per DDD:
//!
//! | Caller                         | Trigger               | Primitive                    | Sprint-4 ticket |
//! |--------------------------------|-----------------------|------------------------------|-----------------|
//! | LDP handler (pre-GET)          | inbound request       | `DotfileAllowlist::is_allowed` → 403 on deny | F7 |
//! | LDP handler (pre-PUT/POST/PATCH) | inbound write       | `DotfileAllowlist::is_allowed` → 403 on deny | F7 |
//! | OIDC JWKS fetcher              | `fetch_jwks`          | `SsrfPolicy::resolve_and_check` → 400 on deny | F5 |
//! | Webhook delivery worker        | subscription + dispatch | `SsrfPolicy::resolve_and_check` (re-resolve per dispatch — DNS rebinding guard) | F3 |
//!
//! ## DNS-rebinding resistance
//!
//! [`SsrfPolicy::resolve_and_check`] returns the resolved `IpAddr`.
//! Callers MUST pass that IP to the subsequent socket connect (for
//! `reqwest`, via the `resolve` override) so the check and the
//! connection target the same endpoint. Re-resolving at request time
//! prevents stale cache bypasses.
//!
//! ## Configuration
//!
//! All runtime policy is env-driven; see [`SsrfPolicy::from_env`] and
//! [`DotfileAllowlist::from_env`]. Defaults are fail-safe: SSRF denies
//! all private/loopback/link-local space; dotfile allowlist permits
//! only `.acl` and `.meta`.

pub mod dotfile;
pub mod ssrf;

pub use dotfile::{DotfileAllowlist, DotfileError};
pub use ssrf::{IpClass, SsrfError, SsrfPolicy};
