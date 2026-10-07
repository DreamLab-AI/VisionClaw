//! Give a ForceComputeActor the saved physics settings.
//!
//! A ForceComputeActor starts with compiled-in defaults. The saved settings
//! (the SQLite `physics` row that `GET/PUT /api/settings/physics` read and
//! write) reach it through three messages, because they span three concerns:
//! `UpdateSimulationParams` (the GPU `SimParams`), `UpdateClusteringParams`
//! (community-detector settings that do not fit `SimParams`) and
//! `ConfigureBroadcastOptimization` (`broadcastFps`). Boot and every
//! supervisor restart use this one path, so a restarted actor runs with the
//! same settings as the one it replaced.

use actix::Addr;
use log::{info, warn};

use super::force_compute_actor::ForceComputeActor;
use crate::actors::messages::{
    ConfigureBroadcastOptimization, UpdateClusteringParams, UpdateSimulationParams,
};
use crate::config::PhysicsSettings;
use crate::ports::settings_repository::{SettingValue, SettingsRepository};

/// Storage key of the saved physics settings (shared with the settings routes).
pub const PHYSICS_SETTINGS_KEY: &str = "physics";

/// The saved physics settings, or `None` if there is no readable row (the
/// actor then keeps its defaults, which is what a fresh install has anyway).
pub async fn load_saved_physics(repo: &dyn SettingsRepository) -> Option<PhysicsSettings> {
    match repo.get_setting(PHYSICS_SETTINGS_KEY).await {
        Ok(Some(SettingValue::Json(json))) => match serde_json::from_value(json) {
            Ok(physics) => Some(physics),
            Err(e) => {
                warn!("saved physics settings are unreadable ({e}); keeping defaults");
                None
            }
        },
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

/// Send `physics` to `actor`: simulation params, clustering params and the
/// broadcast rate.
pub fn push_physics(actor: &Addr<ForceComputeActor>, physics: &PhysicsSettings) {
    actor.do_send(UpdateSimulationParams {
        params: physics.into(),
    });
    actor.do_send(UpdateClusteringParams {
        algorithm: physics.clustering_algorithm.clone(),
        resolution: physics.clustering_resolution,
        iterations: physics.clustering_iterations,
    });
    actor.do_send(ConfigureBroadcastOptimization::rate_only(
        physics.broadcast_fps,
    ));
    info!(
        "Pushed saved physics to ForceComputeActor (spring_k={}, repel_k={}, broadcast {} Hz)",
        physics.spring_k, physics.repel_k, physics.broadcast_fps
    );
}
