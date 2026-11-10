//! Give a ForceComputeActor the saved physics settings.
//!
//! A ForceComputeActor starts with compiled-in defaults. The saved settings
//! (the SQLite `physics` row that `GET/PUT /api/settings/physics` read and
//! write) reach it through two messages, because they span two concerns:
//! `UpdateSimulationParams` (the GPU `SimParams`) and `UpdateClusteringParams`
//! (community-detector settings that do not fit `SimParams`). Boot and every
//! supervisor restart use this one path, so a restarted actor runs with the
//! same settings as the one it replaced. (The position-stream settings are not
//! the compute actor's: the client coordinator loads them itself at start.)

use actix::Addr;
use log::{info, warn};

use super::force_compute_actor::ForceComputeActor;
use crate::actors::messages::{UpdateClusteringParams, UpdateSimulationParams};
use crate::config::PhysicsSettings;
use crate::ports::settings_repository::{SettingValue, SettingsRepository};

/// Storage key of the saved physics settings (shared with the settings routes).
pub const PHYSICS_SETTINGS_KEY: &str = "physics";

/// Storage key of the legacy quality-gate settings, which held the layout mode as a string.
const QUALITY_GATES_KEY: &str = "quality_gates";

/// ADR-2140: migrate the layout mode from `qualityGates.layoutMode` into a stored physics
/// document that does not yet have `layoutMode`. A document that already has the field is left
/// alone. All seven quality-gate strings map to their own [`LayoutMode`]; an unknown string is
/// logged and leaves the document without the field, so it loads as the default. Never fails.
/// Returns the mode written, if any.
///
/// [`LayoutMode`]: visionclaw_domain::types::layout::LayoutMode
pub fn migrate_layout_mode(
    physics: &mut serde_json::Value,
    quality_gates: Option<&serde_json::Value>,
) -> Option<visionclaw_domain::types::layout::LayoutMode> {
    use visionclaw_domain::types::layout::LayoutMode;
    let doc = physics.as_object_mut()?;
    if doc.contains_key("layoutMode") || doc.contains_key("layout_mode") {
        return None;
    }
    let name = quality_gates?.get("layoutMode")?.as_str()?;
    match LayoutMode::from_quality_gate_str(name) {
        Some(mode) => {
            doc.insert("layoutMode".into(), serde_json::to_value(mode).ok()?);
            info!("migrated qualityGates.layoutMode '{name}' into physics.layoutMode ({mode})");
            Some(mode)
        }
        None => {
            warn!("qualityGates.layoutMode '{name}' is not a layout mode; physics.layoutMode defaults to forceDirected");
            None
        }
    }
}

/// Storage key under which the quality-gate row is archived, unchanged, before its
/// `ontologyStrength` is migrated into the physics document (ADR-2192).
pub const QUALITY_GATES_ARCHIVE_KEY: &str = "quality_gates.archive.adr-2192";

/// ADR-2192: migrate the retired `qualityGates.ontologyStrength` into a stored physics document
/// that does not yet have `ontologyConstraintStrength`. The old value was applied (on each
/// quality-gate save) only while `qualityGates.ontologyPhysics` was true, so only then is it
/// carried over; otherwise the strength in force was the actor's 0.6, which is the new key's
/// default. A value outside 0..=1 or not finite is not carried over. Never fails. Returns the
/// strength written, if any.
pub fn migrate_ontology_strength(
    physics: &mut serde_json::Value,
    quality_gates: Option<&serde_json::Value>,
) -> Option<f32> {
    let doc = physics.as_object_mut()?;
    if doc.contains_key("ontologyConstraintStrength")
        || doc.contains_key("ontology_constraint_strength")
    {
        return None;
    }
    let gates = quality_gates?;
    if gates
        .get("ontologyPhysics")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return None;
    }
    let strength = gates.get("ontologyStrength")?.as_f64()? as f32;
    let (lo, hi) = super::physics_bounds::ONTOLOGY_CONSTRAINT_STRENGTH;
    if !strength.is_finite() || !(lo..=hi).contains(&strength) {
        warn!("qualityGates.ontologyStrength {strength} is out of range; not migrated");
        return None;
    }
    doc.insert(
        "ontologyConstraintStrength".into(),
        serde_json::json!(strength),
    );
    info!(
        "migrated qualityGates.ontologyStrength {strength} into physics.ontologyConstraintStrength"
    );
    Some(strength)
}

/// Archive `gates` (the quality-gate row, unchanged) under [`QUALITY_GATES_ARCHIVE_KEY`]
/// before a migration from it is persisted (ADR-2192). Writes once: an existing archive is
/// kept, so a later boot never overwrites the original. `Err` means nothing was archived and
/// the caller must not persist the migration.
pub async fn archive_quality_gates(
    repo: &dyn SettingsRepository,
    gates: serde_json::Value,
) -> Result<(), String> {
    if let Ok(Some(_)) = repo.get_setting(QUALITY_GATES_ARCHIVE_KEY).await {
        return Ok(());
    }
    repo.set_setting(
        QUALITY_GATES_ARCHIVE_KEY,
        SettingValue::Json(gates),
        Some("Quality gates before ADR-2192 moved ontologyStrength into physics"),
    )
    .await
    .map_err(|e| e.to_string())
}

/// The stored quality-gate settings document, if there is a readable one.
pub async fn load_quality_gates_json(repo: &dyn SettingsRepository) -> Option<serde_json::Value> {
    match repo.get_setting(QUALITY_GATES_KEY).await {
        Ok(Some(SettingValue::Json(json))) => Some(json),
        _ => None,
    }
}

/// The saved physics settings, or `None` if there is no readable row (the
/// actor then keeps its defaults, which is what a fresh install has anyway).
pub async fn load_saved_physics(repo: &dyn SettingsRepository) -> Option<PhysicsSettings> {
    match repo.get_setting(PHYSICS_SETTINGS_KEY).await {
        Ok(Some(SettingValue::Json(mut json))) => {
            let gates = load_quality_gates_json(repo).await;
            migrate_layout_mode(&mut json, gates.as_ref());
            migrate_ontology_strength(&mut json, gates.as_ref());
            match serde_json::from_value(json) {
                Ok(physics) => Some(physics),
                Err(e) => {
                    warn!("saved physics settings are unreadable ({e}); keeping defaults");
                    None
                }
            }
        }
        Ok(Some(_)) => {
            warn!("saved physics settings row is not JSON; keeping defaults");
            None
        }
        Ok(None) => None,
        Err(e) => {
            warn!("could not read saved physics settings ({e}); keeping defaults");
            None
        }
    }
}

/// Send `physics` to `actor`: simulation params and clustering params.
pub fn push_physics(actor: &Addr<ForceComputeActor>, physics: &PhysicsSettings) {
    actor.do_send(UpdateSimulationParams {
        params: physics.into(),
    });
    actor.do_send(UpdateClusteringParams {
        algorithm: physics.clustering_algorithm.clone(),
        resolution: physics.clustering_resolution,
        iterations: physics.clustering_iterations,
    });
    info!(
        "Pushed saved physics to ForceComputeActor (spring_k={}, repel_k={})",
        physics.spring_k, physics.repel_k
    );
}

#[cfg(test)]
mod layout_migration_tests {
    use super::migrate_layout_mode;
    use crate::config::PhysicsSettings;
    use serde_json::json;
    use visionclaw_domain::types::layout::LayoutMode;

    fn stored() -> serde_json::Value {
        let mut doc = serde_json::to_value(PhysicsSettings::default()).unwrap();
        doc.as_object_mut().unwrap().remove("layoutMode");
        doc
    }

    #[test]
    fn all_seven_quality_gate_strings_migrate_and_load() {
        let cases = [
            ("force-directed", LayoutMode::ForceDirected),
            ("dag-topdown", LayoutMode::Hierarchical),
            ("dag-radial", LayoutMode::Radial),
            ("dag-leftright", LayoutMode::DagLeftRight),
            ("type-clustering", LayoutMode::TypeClustering),
            ("spectral", LayoutMode::Spectral),
            ("clustered", LayoutMode::Clustered),
        ];
        for (name, mode) in cases {
            let mut doc = stored();
            let gates = json!({ "layoutMode": name });
            assert_eq!(
                migrate_layout_mode(&mut doc, Some(&gates)),
                Some(mode),
                "{name}"
            );
            let loaded: PhysicsSettings = serde_json::from_value(doc).unwrap();
            assert_eq!(loaded.layout_mode, mode, "{name}");
        }
    }

    #[test]
    fn an_unknown_string_is_a_logged_default_not_a_failure() {
        let mut doc = stored();
        let gates = json!({ "layoutMode": "spiral-galaxy" });
        assert_eq!(migrate_layout_mode(&mut doc, Some(&gates)), None);
        let loaded: PhysicsSettings = serde_json::from_value(doc).unwrap();
        assert_eq!(loaded.layout_mode, LayoutMode::ForceDirected);
    }

    #[test]
    fn a_document_that_already_has_the_field_is_never_overwritten() {
        let mut doc = stored();
        doc["layoutMode"] = json!("radial");
        let gates = json!({ "layoutMode": "dag-leftright" });
        assert_eq!(migrate_layout_mode(&mut doc, Some(&gates)), None);
        let loaded: PhysicsSettings = serde_json::from_value(doc).unwrap();
        assert_eq!(loaded.layout_mode, LayoutMode::Radial);
    }

    #[test]
    fn no_quality_gates_or_a_non_object_document_changes_nothing() {
        let mut doc = stored();
        assert_eq!(migrate_layout_mode(&mut doc, None), None);
        assert!(doc.get("layoutMode").is_none());
        let mut not_object = json!(3);
        assert_eq!(
            migrate_layout_mode(&mut not_object, Some(&json!({"layoutMode": "spectral"}))),
            None
        );
    }
}

/// ADR-2192: a stored physics row written before the ontology-constraint policy keys existed
/// must still load at boot (a row that fails to deserialise is replaced by defaults, wiping the
/// operator's tuning), and must load today's policy: constraints on at 0.6.
#[cfg(test)]
mod policy_key_compat_tests {
    use crate::actors::messages::SetOntologyConstraintPolicy;
    use crate::config::PhysicsSettings;

    #[test]
    fn live_physics_row_without_the_policy_keys_loads_todays_policy() {
        let row: serde_json::Value =
            serde_json::from_str(include_str!("test_fixtures/live_physics_settings.json"))
                .expect("fixture is JSON");
        assert!(row.get("ontologyConstraintsEnabled").is_none());
        assert!(row.get("ontologyConstraintStrength").is_none());
        let loaded: PhysicsSettings =
            serde_json::from_value(row.clone()).expect("the captured live row still loads");
        assert_eq!(
            SetOntologyConstraintPolicy::from_physics(&loaded),
            SetOntologyConstraintPolicy {
                enabled: true,
                strength: 0.6
            }
        );
        // The operator's own tuning in the row is kept, not replaced by defaults.
        assert_eq!(
            loaded.repel_k,
            row["repelK"].as_f64().expect("repelK is a number") as f32,
            "stored repelK survives (compared at the stored f32 width)"
        );
        assert!(crate::settings::api::settings_routes::validate_physics_settings(&loaded).is_ok());
    }
}

/// ADR-2192: the retired quality-gate strength moves into the stored physics key, once, and
/// only when it was in force.
#[cfg(test)]
mod ontology_strength_migration_tests {
    use super::migrate_ontology_strength;
    use serde_json::json;

    #[test]
    fn an_in_force_strength_moves_into_a_document_without_the_key() {
        // The captured live row, which predates the key.
        let mut doc: serde_json::Value =
            serde_json::from_str(include_str!("test_fixtures/live_physics_settings.json")).unwrap();
        let gates = json!({ "ontologyPhysics": true, "ontologyStrength": 0.25 });
        assert_eq!(
            migrate_ontology_strength(&mut doc, Some(&gates)),
            Some(0.25)
        );
        assert_eq!(doc["ontologyConstraintStrength"], json!(0.25_f32));
        let loaded: crate::config::PhysicsSettings = serde_json::from_value(doc.clone()).unwrap();
        assert_eq!(loaded.ontology_constraint_strength, 0.25);
        // Once: a document that has the key is left alone.
        let other = json!({ "ontologyPhysics": true, "ontologyStrength": 0.9 });
        assert_eq!(migrate_ontology_strength(&mut doc, Some(&other)), None);
        assert_eq!(doc["ontologyConstraintStrength"], json!(0.25_f32));
    }

    #[test]
    fn a_strength_that_was_never_in_force_or_is_out_of_range_is_not_moved() {
        for gates in [
            json!({ "ontologyPhysics": false, "ontologyStrength": 0.25 }),
            json!({ "ontologyStrength": 0.25 }),
            json!({ "ontologyPhysics": true, "ontologyStrength": 1.5 }),
            json!({ "ontologyPhysics": true }),
        ] {
            let mut doc = json!({});
            assert_eq!(
                migrate_ontology_strength(&mut doc, Some(&gates)),
                None,
                "{gates}"
            );
            assert!(doc.get("ontologyConstraintStrength").is_none());
        }
        let mut doc = json!({});
        assert_eq!(migrate_ontology_strength(&mut doc, None), None);
    }

    #[tokio::test]
    async fn the_quality_gate_row_is_archived_once_and_never_overwritten() {
        use super::{archive_quality_gates, QUALITY_GATES_ARCHIVE_KEY};
        use crate::ports::settings_repository::{SettingValue, SettingsRepository};
        let repo =
            crate::adapters::SqliteSettingsRepository::open(std::path::Path::new(":memory:"))
                .await
                .expect("repo");
        let original = json!({ "ontologyPhysics": true, "ontologyStrength": 0.25 });
        archive_quality_gates(&repo, original.clone())
            .await
            .unwrap();
        archive_quality_gates(&repo, json!({ "later": true }))
            .await
            .unwrap();
        match repo.get_setting(QUALITY_GATES_ARCHIVE_KEY).await.unwrap() {
            Some(SettingValue::Json(v)) => assert_eq!(v, original),
            other => panic!("archive missing: {other:?}"),
        }
    }
}
