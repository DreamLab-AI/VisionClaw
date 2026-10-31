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
            migrate_layout_mode(&mut json, load_quality_gates_json(repo).await.as_ref());
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
