//! # solid-pod-rs
//!
//! Rust implementation of a Solid Pod server: WAC (Web Access
//! Control), LDP (Linked Data Platform) resource/container
//! semantics, WebID profiles, NIP-98 authentication, and Solid
//! Notifications.
//!
//! The crate is framework-agnostic. Wire it into any HTTP server
//! (actix-web, axum, hyper, …) by implementing the request → storage
//! bindings yourself; see `examples/standalone.rs` for a minimal
//! actix-web integration.
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
pub mod error;
pub mod ldp;
pub mod notifications;
pub mod storage;
pub mod wac;
pub mod webid;

// Re-exports for ergonomic consumers.
pub use error::PodError;
pub use storage::{ResourceMeta, Storage, StorageEvent};
pub use wac::{
    evaluate_access, method_to_mode, mode_name, wac_allow_header, AccessMode, AclDocument,
};
