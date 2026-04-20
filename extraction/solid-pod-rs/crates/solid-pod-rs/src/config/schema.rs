//! `ServerConfig` root + value objects.
//!
//! See the bounded-context doc
//! [`docs/design/jss-parity/05-config-platform-context.md`] for the
//! aggregate model. In short: `ServerConfig` is the root, loaded by
//! [`crate::config::loader::ConfigLoader`] from a precedence-ordered
//! list of sources, and validated once at the end of the load.
//!
//! The struct shapes below are designed so **the same JSS
//! `config.json` file boots both JSS and solid-pod-rs** — field names
//! and JSON structure mirror JSS's `config.json` where semantics align.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Root aggregate
// ---------------------------------------------------------------------------

/// Fully resolved server configuration snapshot.
///
/// Construct via [`crate::config::loader::ConfigLoader`]; never mutate
/// after construction. Reload swaps in a new snapshot atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: ServerSection,

    #[serde(default)]
    pub storage: StorageBackendConfig,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub notifications: NotificationsConfig,

    #[serde(default)]
    pub security: SecurityConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection::default(),
            storage: StorageBackendConfig::default(),
            auth: AuthConfig::default(),
            notifications: NotificationsConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP binding
// ---------------------------------------------------------------------------

/// HTTP listener settings — matches JSS `host`/`port`/`baseUrl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSection {
    /// `JSS_HOST`, default `0.0.0.0` (matches JSS default).
    #[serde(default = "default_host")]
    pub host: String,

    /// `JSS_PORT`, default `3000` (matches JSS default).
    #[serde(default = "default_port")]
    pub port: u16,

    /// `JSS_BASE_URL` — optional; used for pod-URL construction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            base_url: None,
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    3000
}

// ---------------------------------------------------------------------------
// Storage backend selection
// ---------------------------------------------------------------------------

/// Tagged storage backend selector — matches JSS's
/// `{ "type": "fs"|"memory"|"s3", … }` JSON shape.
///
/// `JSS_STORAGE_TYPE` drives the variant; `JSS_STORAGE_ROOT` /
/// `JSS_ROOT` feeds the `fs` root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageBackendConfig {
    /// Filesystem backend (JSS default).
    Fs {
        #[serde(default = "default_fs_root")]
        root: String,
    },

    /// In-memory (ephemeral) backend.
    Memory,

    /// S3-compatible object store backend.
    S3 {
        bucket: String,

        region: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
}

impl Default for StorageBackendConfig {
    fn default() -> Self {
        Self::Fs {
            root: default_fs_root(),
        }
    }
}

fn default_fs_root() -> String {
    "./data".to_string()
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

/// Auth toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// NIP-98 (Nostr HTTP Auth) — default on; matches `nip98_enabled`
    /// semantics on the JSS side.
    #[serde(default = "default_true")]
    pub nip98_enabled: bool,

    /// Solid-OIDC — `JSS_OIDC_ENABLED` / JSS `idp`.
    #[serde(default)]
    pub oidc_enabled: bool,

    /// Issuer URL — `JSS_OIDC_ISSUER` / JSS `idpIssuer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_issuer: Option<String>,

    /// DPoP replay-cache TTL (seconds).
    ///
    /// `JSS_DPOP_REPLAY_TTL_SECONDS`; default 300s.
    /// [TODO verify JSS]: JSS does not currently expose this knob;
    /// we add it to parity the Rust side's DPoP replay cache.
    #[serde(default = "default_dpop_ttl")]
    pub dpop_replay_ttl_seconds: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            nip98_enabled: true,
            oidc_enabled: false,
            oidc_issuer: None,
            dpop_replay_ttl_seconds: default_dpop_ttl(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_dpop_ttl() -> u64 {
    300
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/// Solid Notifications channel toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    /// WebSocketChannel2023 — `JSS_NOTIFICATIONS_WS2023`.
    #[serde(default = "default_true")]
    pub ws2023_enabled: bool,

    /// WebhookChannel2023 — `JSS_NOTIFICATIONS_WEBHOOK`.
    #[serde(default)]
    pub webhook2023_enabled: bool,

    /// Legacy `solid-0.1` PATCH-based channel — `JSS_NOTIFICATIONS_LEGACY`.
    ///
    /// JSS sets this on by default for backwards compatibility; we mirror
    /// that for drop-in replacement.
    #[serde(default = "default_true")]
    pub legacy_solid_01_enabled: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            ws2023_enabled: true,
            webhook2023_enabled: false,
            legacy_solid_01_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

/// Security primitives — SSRF, dotfiles, ACL origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Allow outbound requests to RFC 1918 / loopback / link-local —
    /// `JSS_SSRF_ALLOW_PRIVATE`. Defaults off (production-safe).
    #[serde(default)]
    pub ssrf_allow_private: bool,

    /// Explicit allowlist of hosts/CIDRs — `JSS_SSRF_ALLOWLIST`
    /// (comma-separated in env; JSON array in file).
    #[serde(default)]
    pub ssrf_allowlist: Vec<String>,

    /// Explicit denylist — `JSS_SSRF_DENYLIST`.
    #[serde(default)]
    pub ssrf_denylist: Vec<String>,

    /// Dotfile allowlist (e.g. `.acl`, `.meta`) —
    /// `JSS_DOTFILE_ALLOWLIST`.
    #[serde(default = "default_dotfile_allowlist")]
    pub dotfile_allowlist: Vec<String>,

    /// ACL-origin lockdown toggle — `JSS_ACL_ORIGIN_ENABLED`.
    #[serde(default = "default_true")]
    pub acl_origin_enabled: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            ssrf_allow_private: false,
            ssrf_allowlist: Vec::new(),
            ssrf_denylist: Vec::new(),
            dotfile_allowlist: default_dotfile_allowlist(),
            acl_origin_enabled: true,
        }
    }
}

fn default_dotfile_allowlist() -> Vec<String> {
    vec![".acl".to_string(), ".meta".to_string()]
}

// ---------------------------------------------------------------------------
// Basic validation helpers
// ---------------------------------------------------------------------------

impl ServerConfig {
    /// Sanity-check the resolved snapshot. Called once at the end of
    /// [`crate::config::loader::ConfigLoader::load`].
    ///
    /// Returns a human-readable error; `Ok(())` means valid.
    pub fn validate(&self) -> Result<(), String> {
        // Port 0 is allowed (means "pick any free port") — don't reject.
        // But port > u16::MAX isn't representable anyway.

        if self.auth.oidc_enabled && self.auth.oidc_issuer.is_none() {
            return Err(
                "auth.oidc_enabled=true but auth.oidc_issuer is not set (set JSS_OIDC_ISSUER)"
                    .to_string(),
            );
        }

        if let StorageBackendConfig::S3 { bucket, region, .. } = &self.storage {
            if bucket.is_empty() {
                return Err("storage.type=s3 but storage.bucket is empty".to_string());
            }
            if region.is_empty() {
                return Err("storage.type=s3 but storage.region is empty".to_string());
            }
        }

        Ok(())
    }
}
