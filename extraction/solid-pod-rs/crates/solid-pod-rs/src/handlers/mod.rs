//! HTTP / WebSocket handler adapters.
//!
//! Following the F7 library-server boundary, this crate does not mount
//! itself into an HTTP router. The handlers in this module are
//! transport-agnostic driver loops that consumers wire into the HTTP
//! framework of their choice (actix-web, axum, hyper, …). See each
//! submodule for mounting guidance.

#[cfg(feature = "legacy-notifications")]
pub mod legacy_notifications;
