//! # solid-pod-rs
//!
//! Rust implementation of a Solid Pod server: WAC (Web Access
//! Control), LDP (Linked Data Platform) resource/container
//! semantics, WebID profiles, NIP-98 authentication, and Solid
//! Notifications.
//!
//! The crate is framework-agnostic. Wire it into any HTTP server
//! (actix-web, axum, hyper, …) by implementing the request → storage
//! bindings yourself; see `examples/embed_in_actix.rs` for the
//! canonical embeddable pattern.
//!
//! Operators who want a turnkey binary should use the sibling crate
//! `solid-pod-rs-server`, which provides `cargo install
//! solid-pod-rs-server` — a drop-in JSS replacement wiring `PodService`
//! into actix-web with the F6 config loader. The F7 library-server
//! split (ADR-056 §D3) guarantees this library crate never mounts HTTP
//! routes itself; consumers always own the transport.
//!
//! ## Layout
//!
//! - [`storage`] — `Storage` trait and FS/Memory backends.
//! - [`wac`] — Web Access Control evaluator.
//! - [`ldp`] — LDP container/resource semantics.
//! - [`webid`] — WebID profile document helpers.
//! - [`auth`] — HTTP authentication primitives (NIP-98 in Phase 1).
//! - [`notifications`] — Solid Notifications (Phase 2 deliverable).
//! - [`error::PodError`] — crate-wide error type.
//!
//! ## Attribution
//!
//! Extracted from `community-forum-rs/crates/pod-worker`. See NOTICE
//! for provenance.

#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]

pub mod auth;
pub mod config;
pub mod error;
pub mod interop;
pub mod ldp;
pub mod metrics;
pub mod notifications;
pub mod provision;
pub mod security;
pub mod storage;
pub mod wac;
pub mod webid;

#[cfg(feature = "oidc")]
pub mod oidc;

/// Transport-agnostic HTTP / WebSocket handler drivers. Consumers wire
/// these into their HTTP framework of choice. Feature-gated; present
/// only when at least one handler is enabled. Respects the F7
/// library-server boundary — this crate never mounts routes itself.
#[cfg(feature = "legacy-notifications")]
pub mod handlers;

// Re-exports for ergonomic consumers.
pub use error::PodError;
pub use metrics::SecurityMetrics;
pub use security::{DotfileAllowlist, DotfileError, IpClass, SsrfError, SsrfPolicy};
pub use storage::{ResourceMeta, Storage, StorageEvent};
pub use wac::{
    check_origin, evaluate_access, evaluate_access_with_groups, extract_origin_patterns,
    method_to_mode, mode_name, parse_turtle_acl, serialize_turtle_acl, wac_allow_header,
    AccessMode, AclDocument, GroupMembership, Origin, OriginDecision, OriginPattern,
    StaticGroupMembership,
};
pub use ldp::{
    apply_json_patch, apply_n3_patch, apply_sparql_patch, evaluate_preconditions, link_headers,
    negotiate_format, options_for, parse_range_header, patch_dialect_from_mime,
    server_managed_triples, slice_range, ByteRange, ConditionalOutcome, ContainerRepresentation,
    Graph, OptionsResponse, PatchDialect, PatchOutcome, PreferHeader, RdfFormat, Term, Triple,
    ACCEPT_PATCH, ACCEPT_POST,
};
pub use interop::{
    dev_session, nip05_document, verify_nip05, webfinger_response, well_known_solid, DevSession,
    Nip05Document, SolidWellKnown, WebFingerJrd, WebFingerLink,
};
pub use provision::{
    check_admin_override, provision_pod, AdminOverride, ProvisionOutcome, ProvisionPlan,
    QuotaTracker,
};
pub use webid::{
    extract_oidc_issuer, generate_webid_html, generate_webid_html_with_issuer,
    validate_webid_html,
};
