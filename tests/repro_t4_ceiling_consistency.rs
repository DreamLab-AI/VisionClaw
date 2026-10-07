//! Consistency tests for T4: validation-ceiling agreement across layers.
//!
//! These tests assert that PhysicsSettings::default() values are within the
//! validation ceilings used by every layer. After the T4 fix (2026-06-03),
//! all layers read their bounds from a single source of truth,
//! `visionclaw_server::actors::gpu::physics_bounds`, so the ceilings agree and
//! the canonical defaults (repel_k 120, max_velocity 100, spring_k 12) are
//! accepted rather than clamped.
//!
//! Previously these reproduced three divergences:
//!   - repel_k default (120.0) > actor path cap (100.0)
//!   - max_velocity default (100.0) > actor path cap (50.0)
//!   - spring_k default (12.0) > actor path cap (10.0)
//!
//! Fix spec: docs/architecture/diagrams/qe-T2-T4-writepaths-ceilings.md

use visionclaw_server::actors::gpu::physics_bounds;

// ---------------------------------------------------------------------------
// Actor path-pattern ceilings — now sourced from the shared bounds module
// used by src/actors/optimized_settings_actor.rs:initialize_path_patterns().
// ---------------------------------------------------------------------------
const ACTOR_PATH_SPRING_K_MAX: f32 = physics_bounds::SPRING_K.1;
const ACTOR_PATH_REPEL_K_MAX: f32 = physics_bounds::REPEL_K.1;
const ACTOR_PATH_MAX_VELOCITY_MAX: f32 = physics_bounds::MAX_VELOCITY.1;

// ---------------------------------------------------------------------------
// Route validator ceilings — now the SAME shared bounds module used by
// src/settings/api/settings_routes.rs:validate_physics_settings().
// ---------------------------------------------------------------------------
const ROUTE_VALIDATOR_SPRING_K_MAX: f32 = physics_bounds::SPRING_K.1;
const ROUTE_VALIDATOR_MAX_VELOCITY_MAX: f32 = physics_bounds::MAX_VELOCITY.1;

// ---------------------------------------------------------------------------
// Rust backstop constant — force_compute_actor.rs:133. Held equal to the
// shared max_velocity ceiling so the backstop is only a safety net.
// ---------------------------------------------------------------------------
const RUST_BACKSTOP_MAX_VELOCITY: f32 = 1_000.0; // force_compute_actor.rs:133

#[cfg(test)]
mod t4_ceiling_consistency {
    use super::*;
    use visionclaw_domain::types::physics_config::PhysicsSettings;

    // -----------------------------------------------------------------------
    // T4(a): repel_k default MUST NOT exceed actor path cap
    // FAILS: 120.0 > 100.0
    // -----------------------------------------------------------------------
    #[test]
    fn t4a_repel_k_default_within_actor_path_ceiling() {
        let default_repel_k = PhysicsSettings::default().repel_k;
        assert!(
            default_repel_k <= ACTOR_PATH_REPEL_K_MAX,
            "REPRO T4(a): repel_k default ({}) exceeds actor path-pattern ceiling ({}). \
             Boot sends a value that the OptimizedSettingsActor path validator would reject. \
             Fix: raise actor ceiling to >= 120 OR lower default to <= 100, \
             using a single constants source for both.",
            default_repel_k,
            ACTOR_PATH_REPEL_K_MAX
        );
    }

    // -----------------------------------------------------------------------
    // T4(b): max_velocity default MUST NOT exceed actor path cap
    // FAILS: 100.0 > 50.0
    // -----------------------------------------------------------------------
    #[test]
    fn t4b_max_velocity_default_within_actor_path_ceiling() {
        let default_max_velocity = PhysicsSettings::default().max_velocity;
        assert!(
            default_max_velocity <= ACTOR_PATH_MAX_VELOCITY_MAX,
            "REPRO T4(b): max_velocity default ({}) exceeds actor path-pattern ceiling ({}). \
             The default value is rejected by the actor validator but accepted by the route \
             validator (ceiling {}). Fix: unify all ceilings from a single constants source.",
            default_max_velocity,
            ACTOR_PATH_MAX_VELOCITY_MAX,
            ROUTE_VALIDATOR_MAX_VELOCITY_MAX
        );
    }

    // -----------------------------------------------------------------------
    // T4(b) corollary: max_velocity default MUST be within route validator range
    // PASSES on current code (100.0 <= 1000.0) — informational consistency check.
    // -----------------------------------------------------------------------
    #[test]
    fn t4b_max_velocity_default_within_route_validator_ceiling() {
        let default_max_velocity = PhysicsSettings::default().max_velocity;
        assert!(
            default_max_velocity <= ROUTE_VALIDATOR_MAX_VELOCITY_MAX,
            "max_velocity default ({}) exceeds route validator ceiling ({})",
            default_max_velocity,
            ROUTE_VALIDATOR_MAX_VELOCITY_MAX
        );
    }

    // -----------------------------------------------------------------------
    // T4(b) corollary: route validator ceiling MUST equal Rust backstop
    // so the backstop is only a safety net, never a normal-path clamp.
    // PASSES on current code (both are 1000.0).
    // -----------------------------------------------------------------------
    #[test]
    fn t4b_route_validator_max_velocity_equals_rust_backstop() {
        assert_eq!(
            ROUTE_VALIDATOR_MAX_VELOCITY_MAX, RUST_BACKSTOP_MAX_VELOCITY,
            "Route validator ceiling ({}) differs from Rust backstop ({}). \
             When they differ, the backstop fires on healthy frames whose velocity \
             was allowed by the validator but exceeds the backstop constant.",
            ROUTE_VALIDATOR_MAX_VELOCITY_MAX, RUST_BACKSTOP_MAX_VELOCITY
        );
    }

    // -----------------------------------------------------------------------
    // spring_k: actor path cap (10.0) vs default (12.0)
    // FAILS: 12.0 > 10.0 — same pattern as repel_k / max_velocity
    // -----------------------------------------------------------------------
    #[test]
    fn t4_spring_k_default_within_actor_path_ceiling() {
        let default_spring_k = PhysicsSettings::default().spring_k;
        assert!(
            default_spring_k <= ACTOR_PATH_SPRING_K_MAX,
            "REPRO T4: spring_k default ({}) exceeds actor path-pattern ceiling ({}). \
             This follows the same ceiling-mismatch pattern as repel_k and max_velocity.",
            default_spring_k,
            ACTOR_PATH_SPRING_K_MAX
        );
    }

    // -----------------------------------------------------------------------
    // spring_k: default within route validator ceiling
    // PASSES (12.0 <= 500.0) — informational.
    // -----------------------------------------------------------------------
    #[test]
    fn t4_spring_k_default_within_route_validator_ceiling() {
        let default_spring_k = PhysicsSettings::default().spring_k;
        assert!(
            default_spring_k <= ROUTE_VALIDATOR_SPRING_K_MAX,
            "spring_k default ({}) exceeds route validator ceiling ({})",
            default_spring_k,
            ROUTE_VALIDATOR_SPRING_K_MAX
        );
    }

    // -----------------------------------------------------------------------
    // T4 fix invariant: actor path ceiling MUST equal route validator ceiling
    // for every field that both layers bound. After the single-source-of-truth
    // refactor these are literally the same `physics_bounds` constant, so a
    // future divergence can only be reintroduced by breaking the shared module.
    // -----------------------------------------------------------------------
    #[test]
    fn t4_actor_and_route_ceilings_agree() {
        assert_eq!(
            ACTOR_PATH_SPRING_K_MAX, ROUTE_VALIDATOR_SPRING_K_MAX,
            "spring_k ceilings diverge: actor {} vs route {}",
            ACTOR_PATH_SPRING_K_MAX, ROUTE_VALIDATOR_SPRING_K_MAX
        );
        assert_eq!(
            ACTOR_PATH_MAX_VELOCITY_MAX, ROUTE_VALIDATOR_MAX_VELOCITY_MAX,
            "max_velocity ceilings diverge: actor {} vs route {}",
            ACTOR_PATH_MAX_VELOCITY_MAX, ROUTE_VALIDATOR_MAX_VELOCITY_MAX
        );
        // repel_k is now bounded by both layers via the same shared constant.
        assert_eq!(
            ACTOR_PATH_REPEL_K_MAX,
            physics_bounds::REPEL_K.1,
            "repel_k actor ceiling diverges from shared bounds source"
        );
    }

    // -----------------------------------------------------------------------
    // T4 fix invariant: every canonical default is accepted (not clamped) by
    // the unified ceilings. This is the user-visible outcome of the fix.
    // -----------------------------------------------------------------------
    #[test]
    fn t4_canonical_defaults_accepted_by_unified_ceilings() {
        let d = PhysicsSettings::default();
        assert!(
            d.repel_k <= ACTOR_PATH_REPEL_K_MAX,
            "repel_k {} clamped",
            d.repel_k
        );
        assert!(
            d.max_velocity <= ACTOR_PATH_MAX_VELOCITY_MAX,
            "max_velocity {} clamped",
            d.max_velocity
        );
        assert!(
            d.spring_k <= ACTOR_PATH_SPRING_K_MAX,
            "spring_k {} clamped",
            d.spring_k
        );
        // velocity ceiling held equal to the backstop so the backstop never
        // fires on healthy default frames.
        assert_eq!(ROUTE_VALIDATOR_MAX_VELOCITY_MAX, RUST_BACKSTOP_MAX_VELOCITY);
    }

    // -----------------------------------------------------------------------
    // ANTI-TAUTOLOGY: drive the REAL route validator with the canonical client
    // defaults (defaults.ts: repelK 120, maxVelocity 100, springK 12,
    // maxForce 150 — mirrored by PhysicsSettings::default()) and assert it
    // ACCEPTS them unclamped. Comparing physics_bounds constants to themselves
    // proves nothing; calling validate_physics_settings exercises the production
    // code path that boot-time settings actually traverse.
    // -----------------------------------------------------------------------
    #[test]
    fn t4_real_validator_accepts_canonical_defaults() {
        use visionclaw_server::settings::api::settings_routes::validate_physics_settings;

        let d = PhysicsSettings::default();
        // Pin the canonical default magnitudes so a silent default change can't
        // make this test vacuously pass on weaker values.
        assert_eq!(d.repel_k, 120.0, "canonical repelK default drifted");
        assert_eq!(
            d.max_velocity, 100.0,
            "canonical maxVelocity default drifted"
        );
        assert_eq!(d.spring_k, 12.0, "canonical springK default drifted");
        assert_eq!(d.max_force, 150.0, "canonical maxForce default drifted");

        let result = validate_physics_settings(&d);
        assert!(
            result.is_ok(),
            "T4: the real route validator REJECTED the canonical defaults \
             (repelK 120 / maxVelocity 100 / springK 12 / maxForce 150): {:?}. \
             Boot-time settings would be clamped — the ceiling divergence is back.",
            result.err()
        );
    }

    // -----------------------------------------------------------------------
    // ANTI-TAUTOLOGY (liveness): prove the validator is not a no-op. A value
    // ABOVE the shared ceiling MUST be rejected. Without this, a validator that
    // returns Ok(()) unconditionally would pass the acceptance test above.
    // -----------------------------------------------------------------------
    #[test]
    fn t4_real_validator_rejects_above_ceiling() {
        use visionclaw_server::settings::api::settings_routes::validate_physics_settings;

        let d = PhysicsSettings {
            max_velocity: physics_bounds::MAX_VELOCITY.1 + 1.0, // one past the ceiling
            ..Default::default()
        };
        let result = validate_physics_settings(&d);
        assert!(
            result.is_err(),
            "T4: validator accepted max_velocity {} > ceiling {} — validator is a no-op, \
             so the acceptance test proves nothing.",
            d.max_velocity,
            physics_bounds::MAX_VELOCITY.1
        );
    }
}
