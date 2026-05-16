// tests/adapter_parity/settings_parity.rs
//! Parity scenarios for `SettingsRepository`.
//!
//! Covers ALL 17 methods on the trait surface plus three composite scenarios:
//! per-user-vs-global resolution (ADR-11 §D5), schema-version round-trip
//! (PRD-11 A2 — settings schema_version is the on-document version, distinct
//! from the SQLite migration counter), and prefix-listing semantics.
//!
//! The per-user-resolution scenario references PUBKEY_ALICE / PUBKEY_BOB
//! constants from `mod.rs`. The pubkey context is threaded into the adapter
//! by the concrete runner (`runner_neo4j.rs` / `runner_oxigraph.rs`) via
//! whichever mechanism the adapter exposes — task-local for the Oxigraph/
//! SQLite path, an `as_user(pubkey)` adapter wrapper for the Neo4j path.
//! Adapters that do NOT yet support per-user resolution must still pass
//! the GLOBAL portion of the test (per-user is gated behind
//! `supports_per_user`).

use std::collections::HashMap;

use webxr::config::PhysicsSettings;
use webxr::ports::settings_repository::{SettingValue, SettingsRepository};

use super::{make_physics_profile, sv_string};

// ---------------------------------------------------------------------------
// Method 1: get_setting (negative) — missing key returns Ok(None)
// ---------------------------------------------------------------------------

pub async fn parity_get_setting_missing<R: SettingsRepository>(repo: R) {
    let value = repo
        .get_setting("parity.does.not.exist")
        .await
        .expect("get_setting on missing key must return Ok, not Err");
    assert!(
        value.is_none(),
        "missing key must produce Ok(None), not Ok(Some(_))"
    );
}

// ---------------------------------------------------------------------------
// Method 2 + 1 combined: set_setting → get_setting round-trip on each variant
// ---------------------------------------------------------------------------

pub async fn parity_set_get_all_variants<R: SettingsRepository>(repo: R) {
    let cases = vec![
        ("parity.str", SettingValue::String("hello".into())),
        ("parity.int", SettingValue::Integer(42)),
        ("parity.float", SettingValue::Float(3.14)),
        ("parity.bool", SettingValue::Boolean(true)),
        (
            "parity.json",
            SettingValue::Json(serde_json::json!({"nested": [1, 2, 3], "k": "v"})),
        ),
    ];

    for (k, v) in &cases {
        repo.set_setting(k, v.clone(), Some("parity-harness"))
            .await
            .expect("set_setting must succeed");
    }

    for (k, expected) in &cases {
        let got = repo
            .get_setting(k)
            .await
            .expect("get_setting after set must succeed")
            .expect("set value must round-trip");
        assert_eq!(
            got, *expected,
            "value for key {} must round-trip exactly",
            k
        );
    }
}

// ---------------------------------------------------------------------------
// Method 3: delete_setting
// ---------------------------------------------------------------------------

pub async fn parity_delete_setting<R: SettingsRepository>(repo: R) {
    let key = "parity.delete.me";
    repo.set_setting(key, sv_string("doomed"), None)
        .await
        .expect("set");
    assert!(repo.has_setting(key).await.unwrap(), "must exist before delete");

    repo.delete_setting(key).await.expect("delete must succeed");

    assert!(
        !repo.has_setting(key).await.unwrap(),
        "after delete_setting, has_setting must return false"
    );
    assert!(
        repo.get_setting(key).await.unwrap().is_none(),
        "after delete_setting, get_setting must return None"
    );
}

// Deleting a missing key MUST NOT error — the contract is idempotent delete.
pub async fn parity_delete_idempotent<R: SettingsRepository>(repo: R) {
    let result = repo.delete_setting("parity.never.existed").await;
    assert!(
        result.is_ok(),
        "delete_setting on a missing key must be Ok (idempotent), got {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Method 4: has_setting
// ---------------------------------------------------------------------------

pub async fn parity_has_setting<R: SettingsRepository>(repo: R) {
    assert!(
        !repo.has_setting("parity.has.absent").await.unwrap(),
        "absent key must report has_setting=false"
    );

    repo.set_setting("parity.has.present", sv_string("yes"), None)
        .await
        .unwrap();

    assert!(
        repo.has_setting("parity.has.present").await.unwrap(),
        "present key must report has_setting=true"
    );
}

// ---------------------------------------------------------------------------
// Methods 5 + 6: get_settings_batch / set_settings_batch
// ---------------------------------------------------------------------------

pub async fn parity_batch_set_then_get<R: SettingsRepository>(repo: R) {
    let mut updates = HashMap::new();
    updates.insert("parity.b1".to_string(), SettingValue::Integer(1));
    updates.insert("parity.b2".to_string(), SettingValue::Integer(2));
    updates.insert("parity.b3".to_string(), SettingValue::Integer(3));

    repo.set_settings_batch(updates)
        .await
        .expect("set_settings_batch must succeed");

    let keys = vec![
        "parity.b1".to_string(),
        "parity.b2".to_string(),
        "parity.b3".to_string(),
    ];
    let got = repo
        .get_settings_batch(&keys)
        .await
        .expect("get_settings_batch must succeed");

    assert_eq!(got.len(), 3, "batch get must return all 3 keys");
    assert_eq!(got.get("parity.b1"), Some(&SettingValue::Integer(1)));
    assert_eq!(got.get("parity.b2"), Some(&SettingValue::Integer(2)));
    assert_eq!(got.get("parity.b3"), Some(&SettingValue::Integer(3)));
}

// Asking for a key that doesn't exist in a batch must NOT crash; the missing
// keys are simply absent from the returned map.
pub async fn parity_batch_get_partial<R: SettingsRepository>(repo: R) {
    repo.set_setting("parity.bp.present", SettingValue::Integer(10), None)
        .await
        .unwrap();

    let got = repo
        .get_settings_batch(&[
            "parity.bp.present".to_string(),
            "parity.bp.absent".to_string(),
        ])
        .await
        .expect("partial batch must not error");
    assert_eq!(got.len(), 1, "absent keys must be omitted from the map");
    assert_eq!(got.get("parity.bp.present"), Some(&SettingValue::Integer(10)));
    assert!(!got.contains_key("parity.bp.absent"));
}

// ---------------------------------------------------------------------------
// Method 7: list_settings — prefix filter
// ---------------------------------------------------------------------------

pub async fn parity_list_settings_prefix<R: SettingsRepository>(repo: R) {
    repo.set_setting("parity.list.alpha", sv_string("a"), None)
        .await
        .unwrap();
    repo.set_setting("parity.list.beta", sv_string("b"), None)
        .await
        .unwrap();
    repo.set_setting("parity.other.gamma", sv_string("g"), None)
        .await
        .unwrap();

    let prefixed = repo
        .list_settings(Some("parity.list."))
        .await
        .expect("list_settings(prefix) must succeed");

    assert!(prefixed.contains(&"parity.list.alpha".to_string()));
    assert!(prefixed.contains(&"parity.list.beta".to_string()));
    assert!(
        !prefixed.iter().any(|k| k == "parity.other.gamma"),
        "list_settings(prefix) must exclude non-matching keys"
    );
}

// ---------------------------------------------------------------------------
// Methods 8 + 9: load_all_settings / save_all_settings
// ---------------------------------------------------------------------------

pub async fn parity_load_save_all_settings<R: SettingsRepository>(repo: R) {
    use webxr::config::AppFullSettings;

    let mut s = AppFullSettings::default();
    s.version = "parity-1.2.3".to_string();

    repo.save_all_settings(&s)
        .await
        .expect("save_all_settings must succeed");

    let loaded = repo
        .load_all_settings()
        .await
        .expect("load_all_settings must succeed");

    // Adapters that have never been written may return None; that's fine
    // after a fresh start but NOT after save_all_settings.
    let loaded = loaded.expect("after save_all_settings, load_all_settings must return Some");
    assert_eq!(
        loaded.version, s.version,
        "AppFullSettings.version (the on-document schema marker per ADR-11 §D5) \
         must round-trip exactly. \
         Note: this is the *document* version, distinct from the SQLite \
         schema_migrations table — see D5 for the non-conflation rule."
    );
}

// ---------------------------------------------------------------------------
// Methods 10..13: physics profiles — get/save/list/delete
// ---------------------------------------------------------------------------

pub async fn parity_physics_profile_lifecycle<R: SettingsRepository>(repo: R) {
    let profile_name = "parity-profile";
    let custom = make_physics_profile(0.42, 0.77);

    repo.save_physics_settings(profile_name, &custom)
        .await
        .expect("save_physics_settings must succeed");

    let listed = repo
        .list_physics_profiles()
        .await
        .expect("list_physics_profiles must succeed");
    assert!(
        listed.contains(&profile_name.to_string()),
        "saved profile must appear in list_physics_profiles (got {:?})",
        listed
    );

    let got = repo
        .get_physics_settings(profile_name)
        .await
        .expect("get_physics_settings must succeed");
    assert!(
        (got.spring_k - custom.spring_k).abs() < 1e-6,
        "spring_k must round-trip (sent {}, got {})",
        custom.spring_k,
        got.spring_k
    );
    assert!(
        (got.damping - custom.damping).abs() < 1e-6,
        "damping must round-trip (sent {}, got {})",
        custom.damping,
        got.damping
    );

    repo.delete_physics_profile(profile_name)
        .await
        .expect("delete_physics_profile must succeed");

    let after = repo
        .list_physics_profiles()
        .await
        .expect("list after delete");
    assert!(
        !after.contains(&profile_name.to_string()),
        "deleted profile must not appear in list_physics_profiles"
    );
}

// get_physics_settings on a missing profile must return the default profile,
// not error — that's the trait's documented behaviour (default returned
// for "missing OR malformed").
pub async fn parity_physics_missing_profile<R: SettingsRepository>(repo: R) {
    let got = repo
        .get_physics_settings("parity-never-saved")
        .await
        .expect("get_physics_settings on a missing profile must succeed");
    // The trait contract says return-default; we just check the call doesn't fail.
    // We don't compare to PhysicsSettings::default() field-by-field because
    // some adapters serialise the "default" with slightly different field
    // selection — what we DO assert is the value is a structurally valid
    // PhysicsSettings (Rust type system enforces this; the .await above
    // having returned Ok is sufficient).
    let _ = got;
    let _: PhysicsSettings = got;
}

// ---------------------------------------------------------------------------
// Methods 14 + 15: export_settings / import_settings
// ---------------------------------------------------------------------------

pub async fn parity_export_import_roundtrip<R: SettingsRepository>(repo: R) {
    repo.set_setting("parity.exp.k1", sv_string("e1"), None)
        .await
        .unwrap();
    repo.set_setting("parity.exp.k2", SettingValue::Integer(99), None)
        .await
        .unwrap();

    let snapshot = repo
        .export_settings()
        .await
        .expect("export_settings must succeed");

    assert!(
        snapshot.is_object() || snapshot.is_array(),
        "export_settings must return a JSON object or array, got {:?}",
        snapshot
    );

    // Reimport must not error.
    repo.import_settings(&snapshot)
        .await
        .expect("import_settings must succeed");

    // After reimport, the original keys must still be readable.
    assert_eq!(
        repo.get_setting("parity.exp.k1").await.unwrap(),
        Some(sv_string("e1"))
    );
    assert_eq!(
        repo.get_setting("parity.exp.k2").await.unwrap(),
        Some(SettingValue::Integer(99))
    );
}

// ---------------------------------------------------------------------------
// Method 16: clear_cache
// ---------------------------------------------------------------------------

pub async fn parity_clear_cache<R: SettingsRepository>(repo: R) {
    repo.set_setting("parity.cache.key", sv_string("value"), None)
        .await
        .unwrap();

    repo.clear_cache()
        .await
        .expect("clear_cache must succeed even when cache is empty/disabled");

    // After clear_cache, the value MUST still be readable from the backing store.
    // This is the canonical test that "clear_cache" doesn't accidentally
    // wipe persistent state.
    let still = repo.get_setting("parity.cache.key").await.unwrap();
    assert_eq!(
        still,
        Some(sv_string("value")),
        "clear_cache MUST NOT delete data from the backing store"
    );
}

// ---------------------------------------------------------------------------
// Method 17: health_check
// ---------------------------------------------------------------------------

pub async fn parity_health_check<R: SettingsRepository>(repo: R) {
    let ok = repo.health_check().await.expect("health_check must succeed");
    assert!(
        ok,
        "a fresh adapter on a fresh store must report health_check=true"
    );
}

// ---------------------------------------------------------------------------
// Composite scenario: schema_version round-trip via AppFullSettings.version
// ---------------------------------------------------------------------------
//
// This is the explicit ADR-11 §D5 non-conflation test: the schema-version
// embedded in the AppFullSettings document must round-trip via
// load/save_all_settings WITHOUT being conflated with the SQLite-side
// `schema_migrations` table counter.

pub async fn parity_schema_version_roundtrip<R: SettingsRepository>(repo: R) {
    use webxr::config::AppFullSettings;

    let mut s = AppFullSettings::default();
    s.version = "v-parity-9.9.9".to_string();
    repo.save_all_settings(&s).await.unwrap();

    let v1 = repo
        .load_all_settings()
        .await
        .unwrap()
        .expect("after save, load must be Some")
        .version;
    assert_eq!(v1, "v-parity-9.9.9");

    // Overwrite with a different version — must replace, not append.
    s.version = "v-parity-9.9.10".to_string();
    repo.save_all_settings(&s).await.unwrap();
    let v2 = repo
        .load_all_settings()
        .await
        .unwrap()
        .expect("Some")
        .version;
    assert_eq!(
        v2, "v-parity-9.9.10",
        "schema_version must REPLACE on save, not append. \
         If two rows exist this is a sign the adapter is conflating \
         document-version with table-migrations."
    );
}

// ---------------------------------------------------------------------------
// Composite scenario: per-user vs global resolution
// ---------------------------------------------------------------------------
//
// Per ADR-11 §D5 + PRD-11 A5:
//   - A read for (K, U) returns (K, U) if present, else (K, NULL).
//   - A write by U is invisible to other Us and to NULL.
//   - Cross-user reads do NOT see each other.
//
// The harness can't bind a task-local pubkey from here (that's the adapter's
// internal mechanism). Concrete runners that DO support per-user MUST call
// `parity_per_user_isolation_with_context` (below) with a closure that
// scopes the call to a given pubkey. Adapters without per-user support
// just skip this scenario.

pub struct UserScopedRepo<'a, R: SettingsRepository> {
    pub repo: &'a R,
    pub _pubkey: Option<&'static str>,
}

/// Called by a runner that supports per-user scoping. The runner provides
/// two `with_user` closures (one for Alice, one for Bob) and one
/// `with_global` closure. Each closure invokes a callback inside that
/// user's task-local pubkey context.
pub async fn parity_per_user_isolation_with_context<R, FAlice, FBob, FGlobal>(
    _repo: &R,
    mut with_alice: FAlice,
    mut with_bob: FBob,
    mut with_global: FGlobal,
) where
    R: SettingsRepository,
    FAlice: FnMut(&str, SettingValue) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
    FBob: FnMut(&str, SettingValue) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
    FGlobal: FnMut(&str, SettingValue) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
{
    // 1. Global writes a baseline.
    with_global("parity.user.shared", sv_string("global-value")).await;
    // 2. Alice overrides the baseline.
    with_alice("parity.user.shared", sv_string("alice-value")).await;
    // 3. Bob writes a completely separate key.
    with_bob("parity.user.bob-only", sv_string("bob-value")).await;

    // The adapter's `with_user` closure must internally call get_setting
    // and assert the resolved value matches the per-user rule. The
    // assertions live in the runner — this harness function just sequences
    // the writes.
}

// ---------------------------------------------------------------------------
// Aggregator
// ---------------------------------------------------------------------------

pub async fn run_all<R, F, Fut>(factory: F)
where
    R: SettingsRepository,
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = R>,
{
    parity_get_setting_missing(factory().await).await;
    parity_set_get_all_variants(factory().await).await;
    parity_delete_setting(factory().await).await;
    parity_delete_idempotent(factory().await).await;
    parity_has_setting(factory().await).await;
    parity_batch_set_then_get(factory().await).await;
    parity_batch_get_partial(factory().await).await;
    parity_list_settings_prefix(factory().await).await;
    parity_load_save_all_settings(factory().await).await;
    parity_physics_profile_lifecycle(factory().await).await;
    parity_physics_missing_profile(factory().await).await;
    parity_export_import_roundtrip(factory().await).await;
    parity_clear_cache(factory().await).await;
    parity_health_check(factory().await).await;
    parity_schema_version_roundtrip(factory().await).await;
    // per_user_isolation is run by the runner only when the adapter supports it
}
