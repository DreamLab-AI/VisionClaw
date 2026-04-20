//! Layered config loader.
//!
//! Precedence (later overrides earlier):
//!
//! ```text
//! Defaults < File < EnvVars
//! ```
//!
//! Matches JSS `src/config.js:211-239` (minus the CLI overlay, which
//! sits in the `solid-pod-rs-server` binary — F7). The loader:
//!
//! 1. Walks the registered sources in order.
//! 2. Resolves each into a `serde_json::Value` tree.
//! 3. Deep-merges each overlay into the accumulator.
//! 4. Deserialises into [`ServerConfig`].
//! 5. Runs [`ServerConfig::validate`] and returns the snapshot.
//!
//! Unknown JSON fields are tolerated (every sub-struct uses
//! `#[serde(default)]`), matching the "forward-compat with newer JSS
//! releases" invariant in the bounded-context doc.

use std::path::PathBuf;

use serde_json::Value;

use crate::config::schema::ServerConfig;
use crate::config::sources::{merge_json, resolve_source, ConfigSource};
use crate::error::PodError;

// ---------------------------------------------------------------------------
// ConfigLoader
// ---------------------------------------------------------------------------

/// Builder for a layered config load.
///
/// Sources are applied in the order they were registered. The typical
/// JSS-parity invocation is:
///
/// ```no_run
/// use solid_pod_rs::config::ConfigLoader;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let cfg = ConfigLoader::new()
///     .with_defaults()
///     .with_file("config.json")
///     .with_env()
///     .load()
///     .await?;
/// # Ok(()) }
/// ```
pub struct ConfigLoader {
    sources: Vec<ConfigSource>,
    warnings: Vec<String>,
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigLoader {
    /// Empty loader — add sources explicitly. Prefer
    /// [`Self::with_defaults`] as the first call so the final snapshot
    /// is always fully populated.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Register the hard-coded defaults as the lowest-precedence
    /// layer. Idempotent — calling twice has no additional effect.
    pub fn with_defaults(mut self) -> Self {
        if !self
            .sources
            .iter()
            .any(|s| matches!(s, ConfigSource::Defaults))
        {
            self.sources.push(ConfigSource::Defaults);
        }
        self
    }

    /// Register a JSON config file source. Missing / malformed files
    /// are a hard error at load time.
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.sources.push(ConfigSource::File(path.into()));
        self
    }

    /// Register the process environment as a source. Reads `JSS_*`
    /// vars via [`std::env::var`].
    pub fn with_env(mut self) -> Self {
        self.sources.push(ConfigSource::EnvVars);
        self
    }

    /// Resolve all sources in order, merge them, deserialise, and
    /// validate.
    ///
    /// `async` for symmetry with JSS's `loadConfig` and to leave room
    /// for an eventual remote-config source (e.g. Consul, Vault)
    /// without another breaking change. No `await` points today.
    pub async fn load(mut self) -> Result<ServerConfig, PodError> {
        // If no sources were registered at all, inject Defaults so the
        // merged tree is always complete before the final deser pass.
        if self.sources.is_empty() {
            self.sources.push(ConfigSource::Defaults);
        }

        let mut tree = Value::Object(Default::default());

        for source in &self.sources {
            let overlay = resolve_source(source)?;
            merge_json(&mut tree, overlay);

            // Cross-source warning: JSS_STORAGE_TYPE=memory +
            // JSS_STORAGE_ROOT set. The env loader already dropped the
            // root value on our side, but we warn the operator.
            if let ConfigSource::EnvVars = source {
                let type_is_memory = tree
                    .get("storage")
                    .and_then(|s| s.get("type"))
                    .and_then(|t| t.as_str())
                    == Some("memory");
                let root_was_set = std::env::var("JSS_STORAGE_ROOT").is_ok()
                    || std::env::var("JSS_ROOT").is_ok();
                if type_is_memory && root_was_set {
                    self.warnings.push(
                        "JSS_STORAGE_TYPE=memory with JSS_STORAGE_ROOT/JSS_ROOT set: \
                         memory backend wins, root ignored"
                            .to_string(),
                    );
                }
            }
        }

        // Emit warnings via `tracing` if the operator has a subscriber
        // installed; no-op otherwise.
        for w in &self.warnings {
            tracing::warn!(target: "solid_pod_rs::config", "{w}");
        }

        let cfg: ServerConfig = serde_json::from_value(tree).map_err(|e| {
            PodError::Backend(format!("config merge produced invalid shape: {e}"))
        })?;

        cfg.validate().map_err(PodError::Backend)?;

        Ok(cfg)
    }

    /// Accessor for emitted warnings. Populated as a side-effect of
    /// [`Self::load`] if it is called; empty otherwise. Provided so
    /// test code can assert on warning behaviour without relying on a
    /// `tracing` subscriber.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}
