//! Dotfile allowlist (F2).
//!
//! Rejects any inbound request whose path contains a component starting
//! with `.` unless that component is explicitly allowlisted. Default
//! allowlist mirrors JSS: `.acl` and `.meta` — the standard Solid
//! metadata sidecars.
//!
//! Upstream parity: `JavaScriptSolidServer/src/server.js:265-281`.
//! Design context: `docs/design/jss-parity/01-security-primitives-context.md`.

use std::path::{Component, Path};

use thiserror::Error;

use crate::metrics::SecurityMetrics;

/// Environment variable: comma-separated dotfile names permitted by the
/// allowlist. Each entry may or may not include the leading `.`; the
/// allowlist stores them normalised (leading `.` present).
pub const ENV_DOTFILE_ALLOWLIST: &str = "DOTFILE_ALLOWLIST";

/// Default allowlist entries. Matches JSS behaviour for standard Solid
/// metadata sidecars.
pub const DEFAULT_ALLOWED: &[&str] = &[".acl", ".meta"];

/// Reason a path was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum DotfileError {
    /// Path contained a dotfile component not on the allowlist.
    #[error("dotfile path component is not on the allowlist")]
    NotAllowed,
}

/// Dotfile allowlist (aggregate root).
///
/// Immutable after construction. Matching is by exact component
/// equality (case-sensitive, as Solid paths are case-sensitive).
#[derive(Debug, Clone)]
pub struct DotfileAllowlist {
    allowed: Vec<String>,
    metrics: Option<SecurityMetrics>,
}

impl DotfileAllowlist {
    /// Load from `DOTFILE_ALLOWLIST` (comma-separated). Falls back to
    /// the default allowlist (`.acl`, `.meta`) when unset or empty.
    pub fn from_env() -> Self {
        match std::env::var(ENV_DOTFILE_ALLOWLIST) {
            Ok(raw) => {
                let parsed = parse_csv(&raw);
                if parsed.is_empty() {
                    Self::with_defaults()
                } else {
                    Self {
                        allowed: parsed,
                        metrics: None,
                    }
                }
            }
            Err(_) => Self::with_defaults(),
        }
    }

    /// Construct the default allowlist: `.acl`, `.meta`.
    pub fn with_defaults() -> Self {
        Self {
            allowed: DEFAULT_ALLOWED.iter().map(|s| (*s).to_string()).collect(),
            metrics: None,
        }
    }

    /// Construct with an explicit allowlist. Each entry is normalised
    /// to include the leading `.`.
    pub fn new(entries: Vec<String>) -> Self {
        let allowed = entries
            .into_iter()
            .map(|e| normalise_entry(&e))
            .filter(|e| !e.is_empty() && e != ".")
            .collect();
        Self {
            allowed,
            metrics: None,
        }
    }

    /// Attach a metrics sink; counter is incremented on every deny.
    pub fn with_metrics(mut self, metrics: SecurityMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Return the current allowlist entries (normalised; each begins
    /// with `.`).
    pub fn entries(&self) -> &[String] {
        &self.allowed
    }

    /// Returns `false` if ANY path component starts with `.` AND is
    /// not on the allowlist. Returns `true` if the path is free of
    /// dotfile components, or if every dotfile component present is
    /// on the allowlist.
    ///
    /// `.` and `..` navigation components are always rejected
    /// (callers MUST normalise paths before reaching this primitive,
    /// but we defend in depth).
    pub fn is_allowed(&self, path: &Path) -> bool {
        for component in path.components() {
            match component {
                Component::Normal(os) => {
                    let s = match os.to_str() {
                        Some(s) => s,
                        // Non-UTF-8: refuse (Solid paths are UTF-8).
                        None => {
                            self.record_deny();
                            return false;
                        }
                    };
                    if s.starts_with('.') && !self.allowed.iter().any(|a| a == s) {
                        self.record_deny();
                        return false;
                    }
                }
                Component::CurDir | Component::ParentDir => {
                    // Defensive: reject navigation components even
                    // though callers should have normalised the path.
                    self.record_deny();
                    return false;
                }
                Component::Prefix(_) | Component::RootDir => {
                    // Scheme prefix / leading `/`: no dotfile concern.
                }
            }
        }
        true
    }

    fn record_deny(&self) {
        if let Some(m) = &self.metrics {
            m.record_dotfile_deny();
        }
    }
}

impl Default for DotfileAllowlist {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// --- helpers -------------------------------------------------------------

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalise_entry)
        .filter(|s| !s.is_empty() && s != ".")
        .collect()
}

fn normalise_entry(entry: &str) -> String {
    let trimmed = entry.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('.') {
        trimmed.to_string()
    } else {
        format!(".{trimmed}")
    }
}

// --- unit tests ----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_permits_acl_and_meta() {
        let al = DotfileAllowlist::default();
        assert!(al.is_allowed(&PathBuf::from("/resource/.acl")));
        assert!(al.is_allowed(&PathBuf::from("/resource/.meta")));
    }

    #[test]
    fn default_blocks_env() {
        let al = DotfileAllowlist::default();
        assert!(!al.is_allowed(&PathBuf::from("/.env")));
        assert!(!al.is_allowed(&PathBuf::from("/x/y/.env")));
    }

    #[test]
    fn explicit_allowlist_accepts_listed_entries() {
        let al = DotfileAllowlist::new(vec![".env".into(), ".config".into()]);
        assert!(al.is_allowed(&PathBuf::from("/.env")));
        assert!(al.is_allowed(&PathBuf::from("/.config")));
        assert!(!al.is_allowed(&PathBuf::from("/.secret")));
    }

    #[test]
    fn entry_without_dot_prefix_is_normalised() {
        let al = DotfileAllowlist::new(vec!["notifications".into()]);
        assert!(al.is_allowed(&PathBuf::from("/.notifications")));
    }

    #[test]
    fn nested_dotfile_rejected() {
        let al = DotfileAllowlist::default();
        assert!(!al.is_allowed(&PathBuf::from("foo/.secret/bar")));
    }

    #[test]
    fn path_without_dotfiles_accepted() {
        let al = DotfileAllowlist::default();
        assert!(al.is_allowed(&PathBuf::from("/a/b/c/file.ttl")));
    }

    #[test]
    fn parent_dir_rejected() {
        let al = DotfileAllowlist::default();
        assert!(!al.is_allowed(&PathBuf::from("foo/..")));
    }
}
