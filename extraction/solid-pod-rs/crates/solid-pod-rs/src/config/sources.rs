//! Config source precedence + merge logic.
//!
//! # JSS env var mapping (canonical `JSS_*` prefix)
//!
//! The loader honours the following env vars 1:1 with their JSS
//! semantics. Where a var is listed with `[TODO verify JSS]` it means
//! solid-pod-rs introduces it to parity a Rust-side primitive that JSS
//! handles implicitly or not at all.
//!
//! | Env var | Maps to | JSS source |
//! |---|---|---|
//! | `JSS_HOST` | `server.host` | `config.js:98` |
//! | `JSS_PORT` | `server.port` | `config.js:97` |
//! | `JSS_BASE_URL` | `server.base_url` | `config.js:*` (bin/jss.js) |
//! | `JSS_ROOT` | `storage.Fs{root}` (fs kind only) | `config.js:99` |
//! | `JSS_STORAGE_TYPE` | `storage.type` (`fs`/`memory`/`s3`) | JSS uses storage adapters via `config.json`; env wrapper added here for CLI parity |
//! | `JSS_STORAGE_ROOT` | `storage.Fs{root}` | alias for `JSS_ROOT` restricted to fs backend |
//! | `JSS_S3_BUCKET` | `storage.S3{bucket}` | not in upstream JSS env (adapter config) — `[TODO verify JSS]` |
//! | `JSS_S3_REGION` | `storage.S3{region}` | `[TODO verify JSS]` |
//! | `JSS_S3_PREFIX` | `storage.S3{prefix}` | `[TODO verify JSS]` |
//! | `JSS_OIDC_ENABLED` | `auth.oidc_enabled` | JSS uses `JSS_IDP` (config.js:107); `JSS_IDP` accepted as alias |
//! | `JSS_IDP` | `auth.oidc_enabled` (alias of `JSS_OIDC_ENABLED`) | `config.js:107` |
//! | `JSS_OIDC_ISSUER` | `auth.oidc_issuer` | JSS `JSS_IDP_ISSUER` (config.js:108); `JSS_IDP_ISSUER` accepted as alias |
//! | `JSS_IDP_ISSUER` | `auth.oidc_issuer` (alias) | `config.js:108` |
//! | `JSS_DPOP_REPLAY_TTL_SECONDS` | `auth.dpop_replay_ttl_seconds` | `[TODO verify JSS]` — Rust-side DPoP cache tuning |
//! | `JSS_NOTIFICATIONS_WS2023` | `notifications.ws2023_enabled` | subset of JSS `JSS_NOTIFICATIONS` (config.js:104) |
//! | `JSS_NOTIFICATIONS_WEBHOOK` | `notifications.webhook2023_enabled` | subset of JSS `JSS_NOTIFICATIONS` |
//! | `JSS_NOTIFICATIONS_LEGACY` | `notifications.legacy_solid_01_enabled` | subset of JSS `JSS_NOTIFICATIONS` |
//! | `JSS_NOTIFICATIONS` | toggles **all three** notification channels on/off | `config.js:104` (coarse master switch) |
//! | `JSS_SSRF_ALLOW_PRIVATE` | `security.ssrf_allow_private` | `[TODO verify JSS]` — F1 security primitive |
//! | `JSS_SSRF_ALLOWLIST` | `security.ssrf_allowlist` (comma-separated) | `[TODO verify JSS]` |
//! | `JSS_SSRF_DENYLIST` | `security.ssrf_denylist` (comma-separated) | `[TODO verify JSS]` |
//! | `JSS_DOTFILE_ALLOWLIST` | `security.dotfile_allowlist` (comma-separated) | `[TODO verify JSS]` |
//! | `JSS_ACL_ORIGIN_ENABLED` | `security.acl_origin_enabled` | `[TODO verify JSS]` — F4 primitive |
//!
//! Unknown `JSS_*` vars are **ignored silently** at the sources layer
//! (warnings are a loader-level concern, see
//! [`crate::config::loader::ConfigLoader`]). This supports forward
//! compat with newer JSS releases.
//!
//! # Precedence
//!
//! ```text
//! Defaults  <  File  <  EnvVars
//! (lowest)                (highest)
//! ```
//!
//! Later sources overwrite earlier ones, matching JSS's
//! `{...defaults, ...fileConfig, ...envConfig}` model
//! (`config.js:219-224`). CLI overlay (if added later) would sit above
//! env vars.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::config::schema::ServerConfig;
use crate::error::PodError;

// ---------------------------------------------------------------------------
// ConfigSource
// ---------------------------------------------------------------------------

/// One layer of the precedence stack.
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// Hard-coded defaults (always first).
    Defaults,

    /// JSON config file at the given path. Missing file is a hard
    /// error; empty or malformed JSON is a hard error; unknown fields
    /// are tolerated (serde `default` everywhere).
    File(PathBuf),

    /// Read `JSS_*` env vars from `std::env`.
    EnvVars,
}

// ---------------------------------------------------------------------------
// Resolution / merging
// ---------------------------------------------------------------------------

/// Resolve a source into a JSON value tree.
///
/// The returned value is a `serde_json::Value::Object` that is merged
/// into the accumulator by [`merge_json`] in precedence order.
pub(crate) fn resolve_source(source: &ConfigSource) -> Result<Value, PodError> {
    match source {
        ConfigSource::Defaults => {
            // Serialise the Default impl; this gives us the same
            // structure as a file-sourced config for easy merging.
            let cfg = ServerConfig::default();
            serde_json::to_value(&cfg).map_err(PodError::Json)
        }

        ConfigSource::File(path) => load_file(path),

        ConfigSource::EnvVars => Ok(load_env()),
    }
}

fn load_file(path: &Path) -> Result<Value, PodError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| PodError::Backend(format!("config file {path:?}: {e}")))?;

    let v: Value = serde_json::from_str(&content).map_err(|e| {
        PodError::Backend(format!("config file {path:?} is not valid JSON: {e}"))
    })?;

    if !v.is_object() {
        return Err(PodError::Backend(format!(
            "config file {path:?}: top-level JSON must be an object, got {}",
            type_name(&v)
        )));
    }

    // JSS accepts a flat config.json (host/port at root). Normalise
    // both flat and nested shapes into the nested ServerConfig
    // structure that ServerConfig expects.
    Ok(normalise_file_shape(v))
}

/// Translate a JSS-style flat `config.json` into solid-pod-rs's nested
/// shape. A nested config passes through untouched.
///
/// JSS flat:
/// ```json
/// { "host": "0.0.0.0", "port": 3000, "storage": { "type": "fs", "root": "./data" } }
/// ```
///
/// Nested (solid-pod-rs native):
/// ```json
/// { "server": { "host": "…", "port": 3000 }, "storage": {…} }
/// ```
fn normalise_file_shape(v: Value) -> Value {
    let obj = match v {
        Value::Object(m) => m,
        other => return other,
    };

    // If a `server` key already exists, assume nested shape — pass through.
    if obj.contains_key("server") {
        return Value::Object(obj);
    }

    let mut out = Map::new();
    let mut server = Map::new();
    let mut remaining = Map::new();

    for (k, v) in obj {
        match k.as_str() {
            "host" | "port" | "base_url" | "baseUrl" => {
                // camelCase → snake_case for baseUrl
                let key = if k == "baseUrl" {
                    "base_url".to_string()
                } else {
                    k
                };
                server.insert(key, v);
            }
            _ => {
                remaining.insert(k, v);
            }
        }
    }

    if !server.is_empty() {
        out.insert("server".to_string(), Value::Object(server));
    }
    for (k, v) in remaining {
        out.insert(k, v);
    }

    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Env var loading
// ---------------------------------------------------------------------------

/// Read the known `JSS_*` env vars and build a sparse JSON object
/// reflecting whichever were set.
///
/// Unknown `JSS_*` vars are ignored (warnings happen at the loader
/// level if requested).
fn load_env() -> Value {
    env_from(|k| std::env::var(k).ok())
}

/// Test-friendly variant that reads env via a closure.
pub(crate) fn env_from<F>(mut get: F) -> Value
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = Map::new();
    let mut server = Map::new();
    let mut storage = Map::new();
    let mut auth = Map::new();
    let mut notifications = Map::new();
    let mut security = Map::new();

    // --- server.*
    if let Some(v) = get("JSS_HOST") {
        server.insert("host".into(), Value::String(v));
    }
    if let Some(v) = get("JSS_PORT") {
        if let Ok(n) = v.parse::<u16>() {
            server.insert("port".into(), Value::Number(n.into()));
        }
    }
    if let Some(v) = get("JSS_BASE_URL") {
        server.insert("base_url".into(), Value::String(v));
    }

    // --- storage.*
    //
    // Precedence inside storage: JSS_STORAGE_TYPE > (JSS_STORAGE_ROOT | JSS_ROOT)
    // A bare JSS_ROOT implies fs backend.
    let storage_type = get("JSS_STORAGE_TYPE").map(|s| s.to_ascii_lowercase());
    let storage_root = get("JSS_STORAGE_ROOT").or_else(|| get("JSS_ROOT"));

    match storage_type.as_deref() {
        Some("memory") => {
            storage.insert("type".into(), Value::String("memory".into()));
            // JSS_STORAGE_ROOT=... while JSS_STORAGE_TYPE=memory is
            // nonsensical; loader emits a warning. Here we honour
            // memory and drop root.
        }
        Some("s3") => {
            storage.insert("type".into(), Value::String("s3".into()));
            if let Some(v) = get("JSS_S3_BUCKET") {
                storage.insert("bucket".into(), Value::String(v));
            }
            if let Some(v) = get("JSS_S3_REGION") {
                storage.insert("region".into(), Value::String(v));
            }
            if let Some(v) = get("JSS_S3_PREFIX") {
                storage.insert("prefix".into(), Value::String(v));
            }
        }
        Some("fs") | None if storage_root.is_some() => {
            storage.insert("type".into(), Value::String("fs".into()));
            if let Some(v) = storage_root {
                storage.insert("root".into(), Value::String(v));
            }
        }
        Some("fs") => {
            storage.insert("type".into(), Value::String("fs".into()));
        }
        Some(_) => {
            // Unknown storage type — leave unset; loader will flag.
        }
        None => {}
    }

    // --- auth.*
    if let Some(v) = get("JSS_OIDC_ENABLED").or_else(|| get("JSS_IDP")) {
        if let Some(b) = parse_bool(&v) {
            auth.insert("oidc_enabled".into(), Value::Bool(b));
        }
    }
    if let Some(v) = get("JSS_OIDC_ISSUER").or_else(|| get("JSS_IDP_ISSUER")) {
        auth.insert("oidc_issuer".into(), Value::String(v));
    }
    if let Some(v) = get("JSS_NIP98_ENABLED") {
        if let Some(b) = parse_bool(&v) {
            auth.insert("nip98_enabled".into(), Value::Bool(b));
        }
    }
    if let Some(v) = get("JSS_DPOP_REPLAY_TTL_SECONDS") {
        if let Ok(n) = v.parse::<u64>() {
            auth.insert("dpop_replay_ttl_seconds".into(), Value::Number(n.into()));
        }
    }

    // --- notifications.*
    // Coarse master switch — drives all three sub-toggles if individual
    // toggles aren't set.
    let master = get("JSS_NOTIFICATIONS").and_then(|v| parse_bool(&v));

    let ws = get("JSS_NOTIFICATIONS_WS2023")
        .and_then(|v| parse_bool(&v))
        .or(master);
    let webhook = get("JSS_NOTIFICATIONS_WEBHOOK")
        .and_then(|v| parse_bool(&v))
        .or(master);
    let legacy = get("JSS_NOTIFICATIONS_LEGACY")
        .and_then(|v| parse_bool(&v))
        .or(master);

    if let Some(b) = ws {
        notifications.insert("ws2023_enabled".into(), Value::Bool(b));
    }
    if let Some(b) = webhook {
        notifications.insert("webhook2023_enabled".into(), Value::Bool(b));
    }
    if let Some(b) = legacy {
        notifications.insert("legacy_solid_01_enabled".into(), Value::Bool(b));
    }

    // --- security.*
    if let Some(v) = get("JSS_SSRF_ALLOW_PRIVATE") {
        if let Some(b) = parse_bool(&v) {
            security.insert("ssrf_allow_private".into(), Value::Bool(b));
        }
    }
    if let Some(v) = get("JSS_SSRF_ALLOWLIST") {
        security.insert("ssrf_allowlist".into(), parse_csv(&v));
    }
    if let Some(v) = get("JSS_SSRF_DENYLIST") {
        security.insert("ssrf_denylist".into(), parse_csv(&v));
    }
    if let Some(v) = get("JSS_DOTFILE_ALLOWLIST") {
        security.insert("dotfile_allowlist".into(), parse_csv(&v));
    }
    if let Some(v) = get("JSS_ACL_ORIGIN_ENABLED") {
        if let Some(b) = parse_bool(&v) {
            security.insert("acl_origin_enabled".into(), Value::Bool(b));
        }
    }

    if !server.is_empty() {
        out.insert("server".into(), Value::Object(server));
    }
    if !storage.is_empty() {
        out.insert("storage".into(), Value::Object(storage));
    }
    if !auth.is_empty() {
        out.insert("auth".into(), Value::Object(auth));
    }
    if !notifications.is_empty() {
        out.insert("notifications".into(), Value::Object(notifications));
    }
    if !security.is_empty() {
        out.insert("security".into(), Value::Object(security));
    }

    Value::Object(out)
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" | "" => Some(false),
        _ => None,
    }
}

fn parse_csv(s: &str) -> Value {
    Value::Array(
        s.split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| Value::String(p.to_string()))
            .collect(),
    )
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Merge logic
// ---------------------------------------------------------------------------

/// Recursively deep-merge `overlay` into `base`. Objects are merged
/// key-by-key; non-object leaves are replaced wholesale.
///
/// This matches JSS's shallow-spread behaviour at the top level
/// (`{...defaults, ...fileConfig, ...envConfig}` — `config.js:219`)
/// but extends it to nested objects so a partial `server` override
/// doesn't wipe unset siblings.
pub(crate) fn merge_json(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => merge_json(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, overlay) => {
            *slot = overlay;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_nested_objects_preserves_siblings() {
        let mut base = serde_json::json!({
            "server": { "host": "0.0.0.0", "port": 3000 },
            "auth":   { "oidc_enabled": false }
        });
        let overlay = serde_json::json!({
            "server": { "port": 8080 }
        });

        merge_json(&mut base, overlay);

        assert_eq!(base["server"]["host"], "0.0.0.0");
        assert_eq!(base["server"]["port"], 8080);
        assert_eq!(base["auth"]["oidc_enabled"], false);
    }

    #[test]
    fn env_host_port() {
        let v = env_from(|k| match k {
            "JSS_HOST" => Some("127.0.0.1".into()),
            "JSS_PORT" => Some("4242".into()),
            _ => None,
        });
        assert_eq!(v["server"]["host"], "127.0.0.1");
        assert_eq!(v["server"]["port"], 4242);
    }

    #[test]
    fn env_memory_storage_ignores_root() {
        let v = env_from(|k| match k {
            "JSS_STORAGE_TYPE" => Some("memory".into()),
            "JSS_STORAGE_ROOT" => Some("/ignored".into()),
            _ => None,
        });
        assert_eq!(v["storage"]["type"], "memory");
        assert!(v["storage"].get("root").is_none());
    }

    #[test]
    fn env_fs_storage_from_jss_root_alias() {
        let v = env_from(|k| match k {
            "JSS_ROOT" => Some("/pods".into()),
            _ => None,
        });
        assert_eq!(v["storage"]["type"], "fs");
        assert_eq!(v["storage"]["root"], "/pods");
    }

    #[test]
    fn env_csv_parses_to_array() {
        let v = env_from(|k| match k {
            "JSS_SSRF_ALLOWLIST" => Some("10.0.0.0/8, 192.168.1.5".into()),
            _ => None,
        });
        assert_eq!(
            v["security"]["ssrf_allowlist"],
            serde_json::json!(["10.0.0.0/8", "192.168.1.5"])
        );
    }

    #[test]
    fn flat_file_shape_normalised_to_nested() {
        let flat = serde_json::json!({
            "host": "0.0.0.0",
            "port": 3000,
            "baseUrl": "https://example.org",
            "storage": { "type": "fs", "root": "./data" }
        });
        let nested = normalise_file_shape(flat);

        assert_eq!(nested["server"]["host"], "0.0.0.0");
        assert_eq!(nested["server"]["port"], 3000);
        assert_eq!(nested["server"]["base_url"], "https://example.org");
        assert_eq!(nested["storage"]["type"], "fs");
    }

    #[test]
    fn nested_file_shape_passes_through() {
        let nested = serde_json::json!({
            "server": { "host": "0.0.0.0", "port": 3000 }
        });
        let out = normalise_file_shape(nested.clone());
        assert_eq!(out, nested);
    }
}
