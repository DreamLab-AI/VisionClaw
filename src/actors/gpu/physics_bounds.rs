//! Physics validation bounds — **single source of truth**.
//!
//! Every validated physics-settings range in the backend MUST read its
//! `(MIN, MAX)` pair from this module. Before this module existed, three layers
//! disagreed on the legal ranges:
//!
//!   - `actors::optimized_settings_actor::initialize_path_patterns` had narrow
//!     path-pattern caps (repel_k <= 100, max_velocity <= 50, spring_k <= 10),
//!   - `settings::api::settings_routes::validate_physics_settings` had wider
//!     caps (max_velocity <= 1000, spring_k <= 500, repel_k unbounded),
//!   - `actors::gpu::force_compute_actor` clamps velocity to 1000 as a defensive
//!     backstop.
//!
//! The narrow actor caps were below the canonical client defaults
//! (`client/src/api/settings/defaults.ts`: repelK 120, maxVelocity 100,
//! springK 12), so boot-time defaults were silently clamped and a spurious
//! "divergence" was reported. The constants below set every `MAX` so the
//! canonical defaults sit comfortably **inside** the legal range, with headroom
//! that is still physically sane for the ~400-unit graph envelope.
//!
//! Velocity's `MAX` is deliberately kept equal to the force-compute backstop
//! constant (1000.0) so the backstop is only ever a safety net for genuinely
//! divergent frames, never a normal-path clamp.
//!
//! Resolved 2026-06-03 (T4 ceiling-consistency fix).
//!
//! Field names are the canonical snake_case `PhysicsSettings` fields from
//! `visionclaw_domain::types::physics_config::PhysicsSettings`.

/// `(MIN, MAX)` pair for a single validated physics field.
pub type Bound = (f32, f32);

// --- Core forces ---------------------------------------------------------

/// Hooke-mode spring stiffness (`spring_k`). Canonical default 12.0.
pub const SPRING_K: Bound = (0.0, 500.0);

/// Repulsion coefficient (`repel_k`). Canonical default 120.0.
pub const REPEL_K: Bound = (0.0, 500.0);

/// Maximum per-node velocity (`max_velocity`). Canonical default 100.0.
/// MAX held equal to the force-compute backstop constant so the backstop only
/// fires on genuinely divergent frames.
pub const MAX_VELOCITY: Bound = (0.1, 1000.0);

/// Maximum per-node force magnitude (`max_force`). Canonical default 150.0.
pub const MAX_FORCE: Bound = (0.1, 5000.0);

// --- Damping / integration ----------------------------------------------

/// Global velocity damping (`damping`). Canonical default 0.9.
pub const DAMPING: Bound = (0.000_001, 1.0);

/// Integration timestep (`dt`). Canonical default 0.016.
pub const DT: Bound = (0.001, 0.1);

/// Per-frame cooling rate (`cooling_rate`). Canonical default 0.001.
pub const COOLING_RATE: Bound = (0.0, 1.0);

/// Boundary reflection damping (`boundary_damping`). Canonical default 0.95.
pub const BOUNDARY_DAMPING: Bound = (0.0, 1.0);

// --- Gravity / temperature ----------------------------------------------

/// Center-gravity strength (`gravity`). Canonical default 0.002.
pub const GRAVITY: Bound = (0.0, 5.0);

/// Simulated-annealing temperature (`temperature`). Canonical default 0.0.
pub const TEMPERATURE: Bound = (0.0, 1.0);

// --- Spatial / bounds ----------------------------------------------------

/// Soft-cube containment size (`bounds_size`). Canonical default 400.0.
pub const BOUNDS_SIZE: Bound = (100.0, 2000.0);

/// Repulsion distance cutoff / spatial-hash radius (`max_repulsion_dist`).
/// Canonical default 400.0.
pub const MAX_REPULSION_DIST: Bound = (10.0, 5000.0);

// --- Clustering / SSSP ---------------------------------------------------

/// Raw cluster-force coefficient (`cluster_strength`). Canonical default 0.0 (opt-in).
pub const CLUSTER_STRENGTH: Bound = (0.0, 0.02);

/// SSSP rest-length adjustment strength (`sssp_alpha`). Canonical default 1.5.
pub const SSSP_ALPHA: Bound = (0.0, 5.0);

// --- Iterations (integer-valued, expressed as f32 for path-pattern reuse) -

/// Physics solver iterations (`iterations`). Canonical default 50.
pub const ITERATIONS: Bound = (1.0, 1000.0);

// --- Network ---------------------------------------------------------------

/// Position-broadcast rate in Hz (`broadcast_fps`). Canonical default 8
/// (`visionclaw_domain::types::physics_config::DEFAULT_BROADCAST_FPS`).
pub const BROADCAST_FPS: Bound = (1.0, 60.0);

/// `true` if `value` is within `[bound.0, bound.1]` inclusive and finite.
#[inline]
pub fn within(value: f32, bound: Bound) -> bool {
    value.is_finite() && value >= bound.0 && value <= bound.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::types::physics_config::PhysicsSettings;

    /// Every canonical `PhysicsSettings::default()` value (which mirrors
    /// `client/src/api/settings/defaults.ts`) must fall inside its bound.
    #[test]
    fn canonical_defaults_are_within_bounds() {
        let d = PhysicsSettings::default();

        assert!(
            within(d.spring_k, SPRING_K),
            "spring_k {} outside {:?}",
            d.spring_k,
            SPRING_K
        );
        assert!(
            within(d.repel_k, REPEL_K),
            "repel_k {} outside {:?}",
            d.repel_k,
            REPEL_K
        );
        assert!(
            within(d.max_velocity, MAX_VELOCITY),
            "max_velocity {} outside {:?}",
            d.max_velocity,
            MAX_VELOCITY
        );
        assert!(
            within(d.max_force, MAX_FORCE),
            "max_force {} outside {:?}",
            d.max_force,
            MAX_FORCE
        );
        assert!(
            within(d.damping, DAMPING),
            "damping {} outside {:?}",
            d.damping,
            DAMPING
        );
        assert!(within(d.dt, DT), "dt {} outside {:?}", d.dt, DT);
        assert!(
            within(d.broadcast_fps as f32, BROADCAST_FPS),
            "broadcast_fps {} outside {:?}",
            d.broadcast_fps,
            BROADCAST_FPS
        );
        assert!(
            within(d.cooling_rate, COOLING_RATE),
            "cooling_rate {} outside {:?}",
            d.cooling_rate,
            COOLING_RATE
        );
        assert!(
            within(d.boundary_damping, BOUNDARY_DAMPING),
            "boundary_damping {} outside {:?}",
            d.boundary_damping,
            BOUNDARY_DAMPING
        );
        assert!(
            within(d.gravity, GRAVITY),
            "gravity {} outside {:?}",
            d.gravity,
            GRAVITY
        );
        assert!(
            within(d.temperature, TEMPERATURE),
            "temperature {} outside {:?}",
            d.temperature,
            TEMPERATURE
        );
        assert!(
            within(d.bounds_size, BOUNDS_SIZE),
            "bounds_size {} outside {:?}",
            d.bounds_size,
            BOUNDS_SIZE
        );
        assert!(
            within(d.max_repulsion_dist, MAX_REPULSION_DIST),
            "max_repulsion_dist {} outside {:?}",
            d.max_repulsion_dist,
            MAX_REPULSION_DIST
        );
        assert!(
            within(d.cluster_strength, CLUSTER_STRENGTH),
            "cluster_strength {} outside {:?}",
            d.cluster_strength,
            CLUSTER_STRENGTH
        );
        assert!(
            within(d.sssp_alpha, SSSP_ALPHA),
            "sssp_alpha {} outside {:?}",
            d.sssp_alpha,
            SSSP_ALPHA
        );
        assert!(
            within(d.iterations as f32, ITERATIONS),
            "iterations {} outside {:?}",
            d.iterations,
            ITERATIONS
        );
    }

    /// Specifically guard the three fields that previously diverged: their MAX
    /// must accept the canonical defaults rather than clamp them.
    #[test]
    fn divergent_fields_accept_canonical_defaults() {
        assert!(
            REPEL_K.1 >= 120.0,
            "repel_k MAX must accept canonical default 120"
        );
        assert!(
            MAX_VELOCITY.1 >= 100.0,
            "max_velocity MAX must accept canonical default 100"
        );
        assert!(
            SPRING_K.1 >= 12.0,
            "spring_k MAX must accept canonical default 12"
        );
    }

    /// MIN must never exceed MAX for any bound.
    #[test]
    fn bounds_are_ordered() {
        for (name, b) in [
            ("spring_k", SPRING_K),
            ("repel_k", REPEL_K),
            ("max_velocity", MAX_VELOCITY),
            ("max_force", MAX_FORCE),
            ("damping", DAMPING),
            ("dt", DT),
            ("cooling_rate", COOLING_RATE),
            ("boundary_damping", BOUNDARY_DAMPING),
            ("gravity", GRAVITY),
            ("temperature", TEMPERATURE),
            ("bounds_size", BOUNDS_SIZE),
            ("max_repulsion_dist", MAX_REPULSION_DIST),
            ("cluster_strength", CLUSTER_STRENGTH),
            ("sssp_alpha", SSSP_ALPHA),
            ("iterations", ITERATIONS),
        ] {
            assert!(b.0 <= b.1, "{} bound min {} > max {}", name, b.0, b.1);
        }
    }
}
