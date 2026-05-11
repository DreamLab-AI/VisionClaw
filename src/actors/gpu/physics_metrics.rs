//! Physics metrics extraction and GPU utilization helpers.
//!
//! Extracted from `force_compute_actor.rs` (P3-05 decomposition) to reduce the
//! 3210-line monolith.  These are pure computation functions that take
//! pre-fetched data and return metric values — they hold no state and perform
//! no GPU mutations, making them independently testable.

use crate::models::simulation_params::SimulationParams;
use crate::utils::unified_gpu_compute::ComputeMode;

use super::force_compute_actor::PhysicsStats;
use super::shared::GPUState;

/// Calculate GPU utilization as a percentage of the target 60fps frame budget
/// (16.67 ms per frame).
///
/// Returns a value clamped to `[0.0, 100.0]`.
pub fn calculate_gpu_utilization(execution_time_ms: f64) -> f32 {
    const TARGET_FRAME_TIME_MS: f64 = 16.67;
    let utilization_percent = (execution_time_ms / TARGET_FRAME_TIME_MS * 100.0) as f32;
    utilization_percent.clamp(0.0, 100.0)
}

/// Estimate physics metrics when GPU data is not available (e.g. mutex busy).
///
/// Returns `(average_velocity, kinetic_energy, total_forces)`.
pub fn estimate_physics_metrics(
    sim_params: &SimulationParams,
    num_nodes: u32,
    num_edges: u32,
) -> (f32, f32, f32) {
    let estimated_velocity = sim_params.max_velocity * 0.3;
    let estimated_kinetic_energy = 0.5 * (num_nodes as f32) * estimated_velocity.powi(2);
    let estimated_total_forces = sim_params.spring_k * (num_edges as f32) * 0.5;
    (estimated_velocity, estimated_kinetic_energy, estimated_total_forces)
}

/// Compute physics metrics from downloaded GPU velocity buffers.
///
/// `vel_x`, `vel_y`, `vel_z` must be parallel arrays of length `num_nodes`.
///
/// Returns `(average_velocity, kinetic_energy, total_forces)`.
pub fn compute_velocity_metrics(
    vel_x: &[f32],
    vel_y: &[f32],
    vel_z: &[f32],
    damping: f32,
) -> (f32, f32, f32) {
    let num_nodes = vel_x.len();
    if num_nodes == 0 {
        return (0.0, 0.0, 0.0);
    }

    let total_velocity: f32 = vel_x
        .iter()
        .zip(vel_y)
        .zip(vel_z)
        .map(|((vx, vy), vz)| (vx * vx + vy * vy + vz * vz).sqrt())
        .sum();

    let average_velocity = total_velocity / num_nodes as f32;

    let kinetic_energy: f32 = vel_x
        .iter()
        .zip(vel_y)
        .zip(vel_z)
        .map(|((vx, vy), vz)| 0.5 * (vx * vx + vy * vy + vz * vz))
        .sum();

    let estimated_total_forces = total_velocity * damping * num_nodes as f32;

    (average_velocity, kinetic_energy, estimated_total_forces)
}

/// Build a `PhysicsStats` snapshot from the current actor state.
///
/// This is a pure data-assembly function with no side effects.
pub fn build_physics_stats(
    gpu_state: &GPUState,
    sim_params: &SimulationParams,
    compute_mode: &ComputeMode,
    last_step_duration_ms: f32,
    average_velocity: f32,
    kinetic_energy: f32,
    total_forces: f32,
) -> PhysicsStats {
    let fps = if last_step_duration_ms > 0.0 {
        1000.0 / last_step_duration_ms
    } else {
        0.0
    };

    PhysicsStats {
        iteration_count: gpu_state.iteration_count,
        gpu_failure_count: gpu_state.gpu_failure_count,
        current_params: sim_params.clone(),
        compute_mode: compute_mode.clone(),
        nodes_count: gpu_state.num_nodes,
        edges_count: gpu_state.num_edges,
        average_velocity,
        kinetic_energy,
        total_forces,
        last_step_duration_ms,
        fps,
        num_edges: gpu_state.num_edges,
        total_force_calculations: gpu_state.iteration_count * gpu_state.num_nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_utilization_zero_time() {
        assert_eq!(calculate_gpu_utilization(0.0), 0.0);
    }

    #[test]
    fn gpu_utilization_half_budget() {
        let util = calculate_gpu_utilization(8.335);
        assert!((util - 50.0).abs() < 0.5);
    }

    #[test]
    fn gpu_utilization_clamped_at_100() {
        assert_eq!(calculate_gpu_utilization(100.0), 100.0);
    }

    #[test]
    fn gpu_utilization_negative_clamped_at_zero() {
        assert_eq!(calculate_gpu_utilization(-5.0), 0.0);
    }

    #[test]
    fn velocity_metrics_empty() {
        let (avg, ke, tf) = compute_velocity_metrics(&[], &[], &[], 0.5);
        assert_eq!(avg, 0.0);
        assert_eq!(ke, 0.0);
        assert_eq!(tf, 0.0);
    }

    #[test]
    fn velocity_metrics_single_node() {
        // v = (3, 4, 0) => |v| = 5
        let (avg, ke, _tf) = compute_velocity_metrics(&[3.0], &[4.0], &[0.0], 0.1);
        assert!((avg - 5.0).abs() < 0.001);
        // KE = 0.5 * (9 + 16 + 0) = 12.5
        assert!((ke - 12.5).abs() < 0.001);
    }

    #[test]
    fn estimate_produces_nonzero_with_nodes() {
        let params = SimulationParams::default();
        let (v, ke, tf) = estimate_physics_metrics(&params, 100, 200);
        assert!(v > 0.0);
        assert!(ke > 0.0);
        assert!(tf > 0.0);
    }

    #[test]
    fn estimate_zero_nodes_zero_metrics() {
        let params = SimulationParams::default();
        let (_v, ke, _tf) = estimate_physics_metrics(&params, 0, 0);
        // KE = 0.5 * 0 * v^2 = 0
        assert_eq!(ke, 0.0);
    }
}
