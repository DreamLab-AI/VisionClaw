//! # JSS-compatible server config
//!
//! PRD §F6 / Sprint 4 — bounded context
//! [`docs/design/jss-parity/05-config-platform-context.md`].
//!
//! This module provides a layered config loader that mirrors JSS's
//! three-layer model (CLI > env > file > default; the CLI overlay
//! lives in the consumer binary) so that **the same JSS
//! `config.json` file boots both servers** once F7 ships the
//! `solid-pod-rs-server` binary.
//!
//! ## Layout
//!
//! - [`schema`] — [`ServerConfig`] aggregate + value objects.
//! - [`loader`] — [`ConfigLoader`] builder for layered loads.
//! - [`sources`] — [`ConfigSource`] + env/file/defaults resolvers.
//!
//! ## Typical use
//!
//! ```no_run
//! use solid_pod_rs::config::ConfigLoader;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let cfg = ConfigLoader::new()
//!     .with_defaults()                    // lowest precedence
//!     .with_file("/etc/solid-pod-rs.json")
//!     .with_env()                         // highest precedence
//!     .load()
//!     .await?;
//!
//! println!("listening on {}:{}", cfg.server.host, cfg.server.port);
//! # Ok(()) }
//! ```
//!
//! ## JSS env var parity
//!
//! See [`sources`] for the full mapping table. Headline vars:
//!
//! - `JSS_HOST`, `JSS_PORT`, `JSS_BASE_URL`
//! - `JSS_ROOT`, `JSS_STORAGE_TYPE`, `JSS_STORAGE_ROOT`
//! - `JSS_OIDC_ENABLED`, `JSS_OIDC_ISSUER` (+ `JSS_IDP`, `JSS_IDP_ISSUER` aliases)
//! - `JSS_NOTIFICATIONS`, `JSS_NOTIFICATIONS_{WS2023,WEBHOOK,LEGACY}`
//! - `JSS_SSRF_ALLOW_PRIVATE`, `JSS_SSRF_{ALLOW,DENY}LIST`
//! - `JSS_DOTFILE_ALLOWLIST`, `JSS_ACL_ORIGIN_ENABLED`
//!
//! Unknown `JSS_*` vars are ignored (forward-compat with newer JSS).

pub mod loader;
pub mod schema;
pub mod sources;

pub use loader::ConfigLoader;
pub use schema::{
    AuthConfig, NotificationsConfig, SecurityConfig, ServerConfig, ServerSection,
    StorageBackendConfig,
};
pub use sources::ConfigSource;
