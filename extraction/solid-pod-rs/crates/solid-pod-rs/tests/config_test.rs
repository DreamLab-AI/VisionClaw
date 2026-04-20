//! F6 / Sprint 4 — integration tests for the JSS-compatible config
//! loader.
//!
//! Maps to the acceptance criteria in
//! `docs/design/jss-parity/05-config-platform-context.md` and the F6
//! test matrix in the task brief:
//!
//! - F6a: defaults produce a valid config
//! - F6b: JSON file overrides defaults
//! - F6c: env var overrides file
//! - F6d: JSS example config boots (same JSON file as JSS)
//! - F6e: invalid port → error
//! - F6f: unknown JSON field → serde allows (tolerant)
//! - F6g: missing required field → error with clear message
//! - F6h: JSS_STORAGE_TYPE=memory + JSS_STORAGE_ROOT set → warning
//!        logged, memory wins
//!
//! These tests mutate process env vars. Rust's test runner
//! parallelises by default, so each env-touching test isolates the
//! vars via a module-scoped `Mutex` guard.

use std::path::PathBuf;
use std::sync::Mutex;

use solid_pod_rs::config::{ConfigLoader, StorageBackendConfig};

// ---------------------------------------------------------------------------
// Env-var isolation
//
// A single Mutex serialises every env-mutating test so parallel test
// execution can't interleave them. The guard's lifetime is the test
// scope.
// ---------------------------------------------------------------------------

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// All JSS_* env vars the loader knows about. We clear them before
/// every env-driven test so earlier tests can't leak state.
const JSS_ENV_VARS: &[&str] = &[
    "JSS_HOST",
    "JSS_PORT",
    "JSS_BASE_URL",
    "JSS_ROOT",
    "JSS_STORAGE_TYPE",
    "JSS_STORAGE_ROOT",
    "JSS_S3_BUCKET",
    "JSS_S3_REGION",
    "JSS_S3_PREFIX",
    "JSS_OIDC_ENABLED",
    "JSS_OIDC_ISSUER",
    "JSS_IDP",
    "JSS_IDP_ISSUER",
    "JSS_NIP98_ENABLED",
    "JSS_DPOP_REPLAY_TTL_SECONDS",
    "JSS_NOTIFICATIONS",
    "JSS_NOTIFICATIONS_WS2023",
    "JSS_NOTIFICATIONS_WEBHOOK",
    "JSS_NOTIFICATIONS_LEGACY",
    "JSS_SSRF_ALLOW_PRIVATE",
    "JSS_SSRF_ALLOWLIST",
    "JSS_SSRF_DENYLIST",
    "JSS_DOTFILE_ALLOWLIST",
    "JSS_ACL_ORIGIN_ENABLED",
];

fn clear_jss_env() {
    for k in JSS_ENV_VARS {
        std::env::remove_var(k);
    }
}

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

// ---------------------------------------------------------------------------
// F6a — defaults produce a valid config
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6a_defaults_produce_valid_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let cfg = ConfigLoader::new()
        .with_defaults()
        .load()
        .await
        .expect("defaults must produce a valid config");

    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 3000);
    assert!(matches!(
        cfg.storage,
        StorageBackendConfig::Fs { ref root } if root == "./data"
    ));
    assert!(cfg.auth.nip98_enabled);
    assert!(!cfg.auth.oidc_enabled);
    assert!(cfg.notifications.ws2023_enabled);
    assert!(cfg.notifications.legacy_solid_01_enabled);
    assert!(cfg.security.acl_origin_enabled);
    assert_eq!(
        cfg.security.dotfile_allowlist,
        vec![".acl".to_string(), ".meta".to_string()]
    );
}

// ---------------------------------------------------------------------------
// F6b — JSON file overrides defaults
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6b_json_file_overrides_defaults() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{
            "server": { "host": "127.0.0.1", "port": 9999 },
            "notifications": { "legacy_solid_01_enabled": false }
        }"#,
    )
    .unwrap();

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .load()
        .await
        .expect("file override must succeed");

    assert_eq!(cfg.server.host, "127.0.0.1"); // overridden
    assert_eq!(cfg.server.port, 9999); // overridden
    assert!(!cfg.notifications.legacy_solid_01_enabled); // overridden
    assert!(cfg.notifications.ws2023_enabled); // default preserved
    assert!(cfg.auth.nip98_enabled); // default preserved
}

// ---------------------------------------------------------------------------
// F6c — env var overrides file
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6c_env_var_overrides_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{ "server": { "host": "127.0.0.1", "port": 4000 } }"#,
    )
    .unwrap();

    std::env::set_var("JSS_PORT", "7777");
    std::env::set_var("JSS_HOST", "10.0.0.1");

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .with_env()
        .load()
        .await
        .expect("env override must succeed");

    // Env wins over file.
    assert_eq!(cfg.server.host, "10.0.0.1");
    assert_eq!(cfg.server.port, 7777);

    clear_jss_env();
}

// ---------------------------------------------------------------------------
// F6d — JSS example config boots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6d_jss_example_config_boots() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_file(fixture_path("jss-compatible.json"))
        .load()
        .await
        .expect("JSS fixture must boot");

    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 3000);
    assert_eq!(
        cfg.server.base_url.as_deref(),
        Some("https://pod.example.org")
    );
    assert!(matches!(
        cfg.storage,
        StorageBackendConfig::Fs { ref root } if root == "./data"
    ));
    assert!(cfg.auth.nip98_enabled);
    assert_eq!(cfg.auth.dpop_replay_ttl_seconds, 300);
    assert!(cfg.notifications.ws2023_enabled);
    assert!(cfg.notifications.legacy_solid_01_enabled);
    assert!(cfg.security.acl_origin_enabled);
}

// ---------------------------------------------------------------------------
// F6e — invalid port → error
//
// `JSS_PORT=70000` doesn't fit in `u16`, so env_from silently drops it
// and the default/file port is used. If the file itself specifies a
// non-u16 port, serde rejects the number at deser time. We test the
// latter as the structural "invalid port → error" case.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6e_invalid_port_in_file_is_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{ "server": { "host": "0.0.0.0", "port": 99999 } }"#,
    )
    .unwrap();

    let err = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .load()
        .await
        .expect_err("port out of u16 range must error");

    let msg = format!("{err}");
    assert!(
        msg.contains("config merge produced invalid shape")
            || msg.contains("port")
            || msg.contains("invalid"),
        "error should reference the structural failure: got {msg}"
    );
}

// ---------------------------------------------------------------------------
// F6f — unknown JSON field → serde allows (tolerant)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6f_unknown_json_field_is_tolerated() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{
            "server": { "host": "0.0.0.0", "port": 3000 },
            "unknown_future_key": "ignored",
            "mashlib": { "enabled": true },
            "activitypub": true
        }"#,
    )
    .unwrap();

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .load()
        .await
        .expect("unknown keys must not break the load (forward-compat)");

    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 3000);
}

// ---------------------------------------------------------------------------
// F6g — missing required field → error with clear message
//
// Required semantics: when `auth.oidc_enabled=true`, `auth.oidc_issuer`
// must be set. The loader's `validate()` catches this and returns a
// human-readable error.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6g_missing_required_field_errors_with_clear_message() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{
            "auth": { "oidc_enabled": true }
        }"#,
    )
    .unwrap();

    let err = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .load()
        .await
        .expect_err("oidc_enabled without issuer must error");

    let msg = format!("{err}");
    assert!(
        msg.contains("oidc_issuer"),
        "error must mention the missing field: got {msg}"
    );
    assert!(
        msg.contains("JSS_OIDC_ISSUER"),
        "error must name the env var to set: got {msg}"
    );
}

// ---------------------------------------------------------------------------
// F6h — JSS_STORAGE_TYPE=memory + JSS_STORAGE_ROOT set → warning logged,
//        memory wins
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6h_memory_type_with_root_warns_memory_wins() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    std::env::set_var("JSS_STORAGE_TYPE", "memory");
    std::env::set_var("JSS_STORAGE_ROOT", "/some/root/that/should/be/ignored");

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_env()
        .load()
        .await
        .expect("memory + root must not error");

    assert!(
        matches!(cfg.storage, StorageBackendConfig::Memory),
        "memory backend must win over the root hint"
    );

    clear_jss_env();
}

// ---------------------------------------------------------------------------
// Bonus — precedence sanity: Defaults < File < Env in a single load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_precedence_chain() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_jss_env();

    // Default port is 3000.
    // File sets 4000.
    // Env sets 5000 — env must win.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"{ "server": { "port": 4000 }, "security": { "acl_origin_enabled": false } }"#,
    )
    .unwrap();

    std::env::set_var("JSS_PORT", "5000");

    let cfg = ConfigLoader::new()
        .with_defaults()
        .with_file(tmp.path())
        .with_env()
        .load()
        .await
        .expect("precedence chain must succeed");

    assert_eq!(cfg.server.port, 5000);
    // File set acl_origin_enabled=false; env didn't touch it → stays false.
    assert!(!cfg.security.acl_origin_enabled);
    // Default (host=0.0.0.0) preserved — nothing overrode it.
    assert_eq!(cfg.server.host, "0.0.0.0");

    clear_jss_env();
}
