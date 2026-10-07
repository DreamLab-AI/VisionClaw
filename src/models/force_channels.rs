//! Named force-channel registry (PHASE 3, mapping layer).
//!
//! # Why this exists
//!
//! The GPU [`SimParams`] struct is a flat, `repr(C)`, 212-byte record whose
//! layout is mirrored byte-for-byte by the CUDA `SimParams` (guarded by a
//! `static_assert`/`const assert` pair on both sides) and whose camelCase fields
//! are the wire contract four shipping clients already speak. Turning it into a
//! literal enum-indexed array of `{enabled, strength}` channels — the eventual
//! architecture — would touch the struct layout, every kernel that reads
//! `c_params.*`, and the settings wire simultaneously. That is too invasive for a
//! single safe pass (see the Phase 3 audit).
//!
//! This module delivers the **mapping layer** for that future refactor without
//! any of the risk: a bounded [`ForceChannel`] enum and a view/mutator that map
//! each channel to the *existing* scalar field(s) and feature-flag bit(s) on
//! `SimParams`. Nothing here changes the struct layout, the kernels, or the wire.
//! It gives the rest of the codebase one enumerable source of truth for "what
//! force channels exist, are they on, and how strong are they", reading and
//! writing through the current representation. When the real array-backed refactor
//! lands, only the bodies of [`ForceChannel::state`] / [`ForceChannel::apply`]
//! change; every caller keeps working.
//!
//! # Channel audit (force terms in the live kernels, 2026-08-30)
//!
//! Terms in `force_pass_kernel` / `force_pass_with_stability_kernel`:
//!   * Repulsion — `repel_k` (or FA2 `scaling_ratio`), gated `ENABLE_REPULSION`.
//!   * Separation — short-range hard push, `separation_radius` + `max_force`.
//!   * Springs — `spring_k` (+ `rest_length`, `spring_scale`, `sssp_alpha`,
//!     LinLog), gated `ENABLE_SPRINGS`.
//!   * Centering — `center_gravity_k`, gated `ENABLE_CENTERING`.
//!   * Constraints — ontology `ConstraintData`, gated `ENABLE_CONSTRAINTS`,
//!     ramp-capped by `constraint_max_force_per_node`.
//!   * DAG radial bias (Phase 2) — `dag_bias_k` + `dag_level_distance`, self-gated
//!     on `dag_bias_k > 0`.
//!
//! Terms in `integrate_pass_kernel`:
//!   * Boundary — soft push, `viewport_bounds` + `boundary_damping`.
//!   * Annealing — velocity jitter, `temperature` + `cooling_rate`.
//!
//! Separate kernels:
//!   * Gravity — `degree_weighted_gravity_kernel`, `gravity`.
//!   * Cluster cohesion — `cluster_cohesion_kernel`, `cluster_strength`.
//!
//! Each of those is exposed here as a [`ForceChannel`]. The registry is
//! **bounded**: adding a force term to the kernels means adding a variant here,
//! which the exhaustive `match`es make impossible to forget.

use crate::models::simulation_params::{FeatureFlags, SimParams, SimulationParams, ToSimParams};

/// A named, togglable force term in the layout engine. Bounded set — one variant
/// per force term the CUDA kernels evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForceChannel {
    /// Pairwise repulsion (classic inverse-square or FA2 degree-scaled).
    Repulsion,
    /// Short-range hard separation push (min-distance overlap resolution).
    Separation,
    /// Edge spring attraction (Hooke / LinLog).
    Spring,
    /// Pull toward the world origin (center gravity).
    Centering,
    /// Degree-weighted gravity toward the origin.
    Gravity,
    /// Cluster cohesion toward community centroids.
    ClusterCohesion,
    /// Ontology constraint forces (subclass/disjoint/position/…). READ-ONLY in the
    /// registry: its `ENABLE_CONSTRAINTS` flag is rebuilt from constraint
    /// residency (`num_constraints > 0`) every physics step, and its backing
    /// scalar is a ramp cap rather than an on/off strength, so [`apply`] is a
    /// no-op for it (see [`is_read_only`]). Enablement is owned by residency.
    Constraints,
    /// DAG radial hierarchy bias (Phase 2, radialout shells).
    DagRadialBias,
    /// Simulated-annealing velocity jitter.
    Annealing,
    /// Soft world-boundary containment push.
    Boundary,
}

/// The enabled/strength view of a single channel. This is the shape the future
/// array-backed `SimParams` will store directly; today it is *derived* from the
/// flat scalar fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForceChannelState {
    /// Whether the term contributes any force this step.
    pub enabled: bool,
    /// The term's primary scalar coefficient (its meaning is channel-specific —
    /// e.g. a stiffness for `Spring`, a radius for `Separation`). See the audit.
    pub strength: f32,
}

impl ForceChannel {
    /// Every channel, in a stable order. Iterating this is the enumerable
    /// "registry" the flat struct otherwise lacks.
    pub const ALL: [ForceChannel; 10] = [
        ForceChannel::Repulsion,
        ForceChannel::Separation,
        ForceChannel::Spring,
        ForceChannel::Centering,
        ForceChannel::Gravity,
        ForceChannel::ClusterCohesion,
        ForceChannel::Constraints,
        ForceChannel::DagRadialBias,
        ForceChannel::Annealing,
        ForceChannel::Boundary,
    ];

    /// Stable, lowercase identifier for logging / diagnostics / a future
    /// registry wire surface. Not a settings key — the settings wire keeps its
    /// existing per-field camelCase names.
    pub const fn key(self) -> &'static str {
        match self {
            ForceChannel::Repulsion => "repulsion",
            ForceChannel::Separation => "separation",
            ForceChannel::Spring => "spring",
            ForceChannel::Centering => "centering",
            ForceChannel::Gravity => "gravity",
            ForceChannel::ClusterCohesion => "clusterCohesion",
            ForceChannel::Constraints => "constraints",
            ForceChannel::DagRadialBias => "dagRadialBias",
            ForceChannel::Annealing => "annealing",
            ForceChannel::Boundary => "boundary",
        }
    }

    /// The `FeatureFlags` bit that gates this channel, if it is flag-gated.
    /// Channels with no flag are gated purely by their strength being > 0.
    pub const fn feature_flag(self) -> Option<u32> {
        match self {
            ForceChannel::Repulsion => Some(FeatureFlags::ENABLE_REPULSION),
            ForceChannel::Spring => Some(FeatureFlags::ENABLE_SPRINGS),
            ForceChannel::Centering => Some(FeatureFlags::ENABLE_CENTERING),
            ForceChannel::Constraints => Some(FeatureFlags::ENABLE_CONSTRAINTS),
            ForceChannel::Separation
            | ForceChannel::Gravity
            | ForceChannel::ClusterCohesion
            | ForceChannel::DagRadialBias
            | ForceChannel::Annealing
            | ForceChannel::Boundary => None,
        }
    }

    /// Read this channel's `{enabled, strength}` view out of the flat `SimParams`.
    ///
    /// For flag-gated channels `enabled` reflects the feature-flag bit; for the
    /// rest it reflects `strength > 0` (the same test the kernels apply). This is
    /// the single place that knows which scalar backs each channel.
    pub fn state(self, p: &SimParams) -> ForceChannelState {
        let strength = self.strength_of(p);
        let enabled = match self.feature_flag() {
            Some(bit) => p.feature_flags & bit != 0,
            None => strength > 0.0,
        };
        ForceChannelState { enabled, strength }
    }

    /// The backing scalar for this channel.
    fn strength_of(self, p: &SimParams) -> f32 {
        match self {
            ForceChannel::Repulsion => p.repel_k,
            ForceChannel::Separation => p.separation_radius,
            ForceChannel::Spring => p.spring_k,
            ForceChannel::Centering => p.center_gravity_k,
            ForceChannel::Gravity => p.gravity,
            ForceChannel::ClusterCohesion => p.cluster_strength,
            ForceChannel::Constraints => p.constraint_max_force_per_node,
            ForceChannel::DagRadialBias => p.dag_bias_k,
            ForceChannel::Annealing => p.temperature,
            ForceChannel::Boundary => p.viewport_bounds,
        }
    }

    /// Write this channel's `{enabled, strength}` back into the flat `SimParams`,
    /// preserving exactly the semantics the kernels expect:
    ///
    /// * The backing scalar is set to `strength` when enabled, or forced to `0.0`
    ///   when disabled (so a non-flag-gated channel truly goes inert).
    /// * For flag-gated channels the feature-flag bit is set when
    ///   `enabled && strength > 0`, else cleared — mirroring how
    ///   `SimParams::from(&SimulationParams)` derives the flags from the scalars.
    ///
    /// Round-trips with [`state`](Self::state): reading a channel and writing it
    /// straight back leaves an equivalent `SimParams` (see the unit tests).
    pub fn apply(self, p: &mut SimParams, s: ForceChannelState) {
        // Constraints is RESIDENCY-DRIVEN and read-only here (see the variant doc
        // and `is_read_only`). The physics step rebuilds `ENABLE_CONSTRAINTS` from
        // `num_constraints > 0` on every launch (execution.rs), so any flag we set
        // is immediately overwritten; and its backing scalar
        // `constraint_max_force_per_node` is a per-node ramp CAP, not an on/off
        // strength — zeroing it would silently change constraint force semantics.
        // Enablement is therefore owned by constraint residency, not the registry.
        if self.is_read_only() {
            return;
        }
        let value = if s.enabled { s.strength } else { 0.0 };
        self.set_strength(p, value);
        if let Some(bit) = self.feature_flag() {
            if s.enabled && value > 0.0 {
                p.feature_flags |= bit;
            } else {
                p.feature_flags &= !bit;
            }
        }
    }

    /// Whether [`apply`](Self::apply) is a no-op for this channel because its
    /// enablement is owned elsewhere. Only `Constraints` is read-only: its
    /// `ENABLE_CONSTRAINTS` flag is rebuilt every physics step from constraint
    /// residency (`num_constraints > 0`), so the registry cannot meaningfully
    /// toggle it. `state()` still reports it faithfully for reading/diagnostics.
    pub const fn is_read_only(self) -> bool {
        matches!(self, ForceChannel::Constraints)
    }

    fn set_strength(self, p: &mut SimParams, value: f32) {
        match self {
            ForceChannel::Repulsion => p.repel_k = value,
            ForceChannel::Separation => p.separation_radius = value,
            ForceChannel::Spring => p.spring_k = value,
            ForceChannel::Centering => p.center_gravity_k = value,
            ForceChannel::Gravity => p.gravity = value,
            ForceChannel::ClusterCohesion => p.cluster_strength = value,
            ForceChannel::Constraints => p.constraint_max_force_per_node = value,
            ForceChannel::DagRadialBias => p.dag_bias_k = value,
            ForceChannel::Annealing => p.temperature = value,
            ForceChannel::Boundary => p.viewport_bounds = value,
        }
    }
}

/// Snapshot the full channel registry from a `SimParams` — one `{enabled,
/// strength}` per [`ForceChannel::ALL`], in that order. Useful for diagnostics
/// and as the seam the future array-backed struct will expose natively.
pub fn snapshot(p: &SimParams) -> [(ForceChannel, ForceChannelState); 10] {
    ForceChannel::ALL.map(|ch| (ch, ch.state(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_channels_have_unique_keys() {
        let mut keys: Vec<&str> = ForceChannel::ALL.iter().map(|c| c.key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "channel keys must be unique");
    }

    #[test]
    fn flag_gated_channels_report_flag_state() {
        // Default params enable repulsion/spring/centering via their positive
        // scalars (SimParams::new derives flags from PhysicsSettings defaults).
        let p = SimParams::new();
        // Repulsion is flag-gated: its enabled must track the flag bit, not just
        // the scalar sign.
        let st = ForceChannel::Repulsion.state(&p);
        assert_eq!(
            st.enabled,
            p.feature_flags & FeatureFlags::ENABLE_REPULSION != 0
        );
        assert_eq!(st.strength, p.repel_k);
    }

    #[test]
    fn non_flag_channel_enabled_tracks_positive_strength() {
        let mut p = SimParams::new();
        p.dag_bias_k = 0.0;
        assert!(!ForceChannel::DagRadialBias.state(&p).enabled);
        p.dag_bias_k = 2.5;
        let st = ForceChannel::DagRadialBias.state(&p);
        assert!(st.enabled);
        assert_eq!(st.strength, 2.5);
    }

    #[test]
    fn disabling_a_channel_zeroes_scalar_and_clears_flag() {
        let mut p = SimParams::new();
        p.repel_k = 100.0;
        p.feature_flags |= FeatureFlags::ENABLE_REPULSION;
        ForceChannel::Repulsion.apply(
            &mut p,
            ForceChannelState {
                enabled: false,
                strength: 100.0,
            },
        );
        assert_eq!(p.repel_k, 0.0, "disabled scalar must be zeroed");
        assert_eq!(
            p.feature_flags & FeatureFlags::ENABLE_REPULSION,
            0,
            "disabled flag must be cleared"
        );
    }

    #[test]
    fn enabling_a_channel_sets_scalar_and_flag() {
        let mut p = SimParams::new();
        p.spring_k = 0.0;
        p.feature_flags &= !FeatureFlags::ENABLE_SPRINGS;
        ForceChannel::Spring.apply(
            &mut p,
            ForceChannelState {
                enabled: true,
                strength: 3.0,
            },
        );
        assert_eq!(p.spring_k, 3.0);
        assert_ne!(p.feature_flags & FeatureFlags::ENABLE_SPRINGS, 0);
    }

    #[test]
    fn enabled_but_zero_strength_leaves_flag_clear() {
        // A flag-gated channel enabled with strength 0 contributes nothing, so the
        // flag stays clear — matching SimParams::from's `> 0.0` flag derivation.
        let mut p = SimParams::new();
        ForceChannel::Centering.apply(
            &mut p,
            ForceChannelState {
                enabled: true,
                strength: 0.0,
            },
        );
        assert_eq!(p.center_gravity_k, 0.0);
        assert_eq!(p.feature_flags & FeatureFlags::ENABLE_CENTERING, 0);
    }

    #[test]
    fn state_apply_roundtrip_preserves_effective_state_for_all_channels() {
        // Reading every channel and writing it straight back must preserve the
        // channel's EFFECTIVE state — the core mapping-layer invariant. `enabled`
        // must always match; `strength` must match while enabled. A DISABLED
        // channel's stored strength is immaterial to the kernels (the term is
        // gated off), and `apply` deliberately zeroes it, so it is excluded from
        // the comparison — that zeroing is the intended normalisation, not a bug.
        let original = SimParams::new();
        let mut p = original;
        for ch in ForceChannel::ALL {
            let s = ch.state(&original);
            ch.apply(&mut p, s);
        }
        for ch in ForceChannel::ALL {
            let before = ch.state(&original);
            let after = ch.state(&p);
            assert_eq!(after.enabled, before.enabled, "roundtrip flipped {:?}", ch);
            if before.enabled {
                assert_eq!(
                    after.strength, before.strength,
                    "roundtrip changed enabled strength of {:?}",
                    ch
                );
            }
        }
    }

    #[test]
    fn disabled_channel_with_positive_scalar_stays_disabled_after_roundtrip() {
        // Regression guard for the Constraints-style case: flag-off at rest but a
        // positive backing scalar. state() reports disabled; a state→apply
        // roundtrip must keep it disabled (and inert) rather than accidentally
        // enabling it.
        let mut p = SimParams::new();
        p.constraint_max_force_per_node = 50.0;
        p.feature_flags &= !FeatureFlags::ENABLE_CONSTRAINTS;
        let s = ForceChannel::Constraints.state(&p);
        assert!(!s.enabled);
        ForceChannel::Constraints.apply(&mut p, s);
        assert!(!ForceChannel::Constraints.state(&p).enabled);
        assert_eq!(p.feature_flags & FeatureFlags::ENABLE_CONSTRAINTS, 0);
    }

    #[test]
    fn only_constraints_is_read_only() {
        for ch in ForceChannel::ALL {
            assert_eq!(ch.is_read_only(), ch == ForceChannel::Constraints);
        }
    }

    #[test]
    fn constraints_apply_is_a_noop() {
        // Constraints is residency-driven: apply must not touch the scalar or flag.
        let mut p = SimParams::new();
        p.constraint_max_force_per_node = 42.0;
        p.feature_flags |= FeatureFlags::ENABLE_CONSTRAINTS;
        // Try to disable it via the registry — must have no effect.
        ForceChannel::Constraints.apply(
            &mut p,
            ForceChannelState {
                enabled: false,
                strength: 0.0,
            },
        );
        assert_eq!(
            p.constraint_max_force_per_node, 42.0,
            "scalar must be untouched"
        );
        assert_ne!(
            p.feature_flags & FeatureFlags::ENABLE_CONSTRAINTS,
            0,
            "residency flag must be untouched"
        );
    }

    #[test]
    fn snapshot_covers_every_channel_in_order() {
        let p = SimParams::new();
        let snap = snapshot(&p);
        assert_eq!(snap.len(), ForceChannel::ALL.len());
        for (i, ch) in ForceChannel::ALL.iter().enumerate() {
            assert_eq!(snap[i].0, *ch);
        }
    }
}

// ── ADR-2029: the final dispatch feature word ──────────────────────────────

/// Everything the final feature word is derived from at dispatch (ADR-2029).
///
/// Gathering these into one struct is the point: the closeout finding is that
/// the authoritative derivation lives in the physics-step wrapper, *after* the
/// converter has already produced a feature word, and immediately overwrites it.
/// Two of the inputs are not user settings at all — they are live device state
/// (`num_constraints`) and a runtime toggle (`sssp_spring_adjust_enabled`) — so
/// the word cannot be derived from `SimulationParams` alone, and any claim that
/// the converter owns force enablement is wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForceDispatchInputs {
    /// Repulsion strength from settings. `> 0` enables the term.
    pub repel_k: f32,
    /// Spring strength from settings. `> 0` enables the term.
    pub spring_k: f32,
    /// Centring strength from settings. `> 0` enables the term.
    pub center_gravity_k: f32,
    /// Settings flag requesting SSSP-adjusted springs.
    pub use_sssp_distances: bool,
    /// Runtime toggle on the compute context, independent of settings.
    pub sssp_spring_adjust_enabled: bool,
    /// **Device residency**: how many constraints are currently uploaded. This,
    /// not any setting, decides `ENABLE_CONSTRAINTS`.
    pub num_constraints: usize,
}

impl ForceDispatchInputs {
    /// Gather the inputs from a settings record plus the two pieces of live
    /// runtime state the settings do not carry.
    pub fn new(
        params: &SimulationParams,
        num_constraints: usize,
        sssp_spring_adjust_enabled: bool,
    ) -> Self {
        Self {
            repel_k: params.repel_k,
            spring_k: params.spring_k,
            center_gravity_k: params.center_gravity_k,
            use_sssp_distances: params.use_sssp_distances,
            sssp_spring_adjust_enabled,
            num_constraints,
        }
    }
}

/// Derive the feature word actually uploaded to the device (ADR-2029).
///
/// # Authority
///
/// This is the **single authoritative derivation**. The physics-step wrapper
/// calls it and assigns the result over whatever `SimulationParams::to_sim_params`
/// produced, so a converter-derived flag word never reaches the device. The actor
/// parameter mirror also holds a converter-derived word; that copy is
/// informational and is overwritten here before every execute.
///
/// # Rules
///
/// * `ENABLE_REPULSION` / `ENABLE_SPRINGS` / `ENABLE_CENTERING` — strictly
///   positive scalar. Zero and negative are both off; `-0.0` is not `> 0.0`, so
///   it is off too.
/// * `ENABLE_SSSP_SPRING_ADJUST` — the settings flag **or** the runtime toggle.
/// * `ENABLE_CONSTRAINTS` — derived from constraint *residency*
///   (`num_constraints > 0`), never from a setting. This is why
///   [`ForceChannel::Constraints`] is read-only in the registry: enablement is
///   owned by what is resident on the device, and `ForceChannel::apply` for it is
///   deliberately a no-op.
///
/// NaN is not `> 0.0`, so a poisoned scalar disables its term rather than
/// enabling it on a value the kernel cannot use.
pub fn derive_dispatch_feature_flags(inputs: ForceDispatchInputs) -> u32 {
    let mut flags = 0u32;
    if inputs.repel_k > 0.0 {
        flags |= FeatureFlags::ENABLE_REPULSION;
    }
    if inputs.spring_k > 0.0 {
        flags |= FeatureFlags::ENABLE_SPRINGS;
    }
    if inputs.center_gravity_k > 0.0 {
        flags |= FeatureFlags::ENABLE_CENTERING;
    }
    if inputs.use_sssp_distances || inputs.sssp_spring_adjust_enabled {
        flags |= FeatureFlags::ENABLE_SSSP_SPRING_ADJUST;
    }
    // KEYSTONE (ADR-098 break #1): gates the live force_pass_kernel constraint
    // loop. Without this bit the uploaded ConstraintData buffer has zero effect.
    if inputs.num_constraints > 0 {
        flags |= FeatureFlags::ENABLE_CONSTRAINTS;
    }
    flags
}

#[cfg(test)]
mod adr_2029_dispatch_authority {
    //! ADR-2029: the final device word, observed across constraint residency
    //! changes, runtime SSSP changes and scalar boundaries.
    //!
    //! These exercise the exact function the physics-step wrapper calls, so they
    //! observe the word that is actually uploaded — not the converter's word,
    //! which is overwritten before every execute.
    use super::*;

    fn base() -> ForceDispatchInputs {
        ForceDispatchInputs {
            repel_k: 1.0,
            spring_k: 1.0,
            center_gravity_k: 1.0,
            use_sssp_distances: false,
            sssp_spring_adjust_enabled: false,
            num_constraints: 0,
        }
    }

    fn has(flags: u32, bit: u32) -> bool {
        flags & bit != 0
    }

    #[test]
    fn constraint_enablement_follows_residency_through_zero_nonzero_zero() {
        // The acceptance transition: 0 -> N -> 0. The bit must track residency
        // in both directions, with no setting involved anywhere.
        let mut i = base();

        i.num_constraints = 0;
        assert!(!has(
            derive_dispatch_feature_flags(i),
            FeatureFlags::ENABLE_CONSTRAINTS
        ));

        i.num_constraints = 1;
        assert!(
            has(
                derive_dispatch_feature_flags(i),
                FeatureFlags::ENABLE_CONSTRAINTS
            ),
            "a single resident constraint must enable the kernel loop"
        );

        i.num_constraints = 4_096;
        assert!(has(
            derive_dispatch_feature_flags(i),
            FeatureFlags::ENABLE_CONSTRAINTS
        ));

        // Constraints removed: the bit must clear, or the kernel keeps walking a
        // buffer that no longer describes anything.
        i.num_constraints = 0;
        assert!(
            !has(
                derive_dispatch_feature_flags(i),
                FeatureFlags::ENABLE_CONSTRAINTS
            ),
            "removing every constraint must clear the bit"
        );
    }

    #[test]
    fn residency_alone_decides_constraints_regardless_of_settings() {
        // No settings field can turn constraints on or off. Vary every scalar
        // and the SSSP inputs; the constraint bit only ever follows residency.
        for num_constraints in [0usize, 3] {
            for (repel, spring, centre) in [(0.0, 0.0, 0.0), (5.0, 5.0, 5.0)] {
                for sssp in [false, true] {
                    let flags = derive_dispatch_feature_flags(ForceDispatchInputs {
                        repel_k: repel,
                        spring_k: spring,
                        center_gravity_k: centre,
                        use_sssp_distances: sssp,
                        sssp_spring_adjust_enabled: sssp,
                        num_constraints,
                    });
                    assert_eq!(
                        has(flags, FeatureFlags::ENABLE_CONSTRAINTS),
                        num_constraints > 0,
                        "constraint enablement must depend only on residency"
                    );
                }
            }
        }
    }

    #[test]
    fn scalar_boundaries_are_strictly_positive() {
        // Zero, negative zero, negative and NaN are all off; the smallest
        // positive value is on. A NaN scalar must not enable a term the kernel
        // cannot evaluate.
        for off in [0.0f32, -0.0, -1.0, f32::NAN, f32::NEG_INFINITY] {
            let flags = derive_dispatch_feature_flags(ForceDispatchInputs {
                repel_k: off,
                spring_k: off,
                center_gravity_k: off,
                ..base()
            });
            assert!(!has(flags, FeatureFlags::ENABLE_REPULSION), "repel {off}");
            assert!(!has(flags, FeatureFlags::ENABLE_SPRINGS), "spring {off}");
            assert!(!has(flags, FeatureFlags::ENABLE_CENTERING), "centre {off}");
        }
        for on in [f32::MIN_POSITIVE, 1e-6, 1.0, f32::INFINITY] {
            let flags = derive_dispatch_feature_flags(ForceDispatchInputs {
                repel_k: on,
                spring_k: on,
                center_gravity_k: on,
                ..base()
            });
            assert!(has(flags, FeatureFlags::ENABLE_REPULSION), "repel {on}");
            assert!(has(flags, FeatureFlags::ENABLE_SPRINGS), "spring {on}");
            assert!(has(flags, FeatureFlags::ENABLE_CENTERING), "centre {on}");
        }
    }

    #[test]
    fn each_scalar_gates_only_its_own_term() {
        let mut i = base();
        i.spring_k = 0.0;
        i.center_gravity_k = 0.0;
        let flags = derive_dispatch_feature_flags(i);
        assert!(has(flags, FeatureFlags::ENABLE_REPULSION));
        assert!(!has(flags, FeatureFlags::ENABLE_SPRINGS));
        assert!(!has(flags, FeatureFlags::ENABLE_CENTERING));
    }

    #[test]
    fn the_runtime_sssp_toggle_changes_the_word_without_a_settings_change() {
        // A runtime SSSP change must reach the device word on its own — the
        // settings flag is not the only authority.
        let mut i = base();
        assert!(!has(
            derive_dispatch_feature_flags(i),
            FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
        ));

        i.sssp_spring_adjust_enabled = true;
        assert!(has(
            derive_dispatch_feature_flags(i),
            FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
        ));

        // …and the settings flag alone also suffices (they are OR-ed).
        let j = ForceDispatchInputs {
            use_sssp_distances: true,
            sssp_spring_adjust_enabled: false,
            ..base()
        };
        assert!(has(
            derive_dispatch_feature_flags(j),
            FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
        ));

        // Turning both off clears it again.
        let k = ForceDispatchInputs {
            use_sssp_distances: false,
            sssp_spring_adjust_enabled: false,
            ..base()
        };
        assert!(!has(
            derive_dispatch_feature_flags(k),
            FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
        ));
    }

    #[test]
    fn the_dispatch_word_overrides_whatever_the_converter_produced() {
        // The authority claim, stated as a test: take a settings record, let the
        // converter build its word, then derive the dispatch word from the same
        // settings plus live residency. Where they disagree, dispatch wins —
        // which is what the physics-step wrapper does by assignment.
        let mut params = SimulationParams::new();
        params.repel_k = 0.0; // converter will not set ENABLE_REPULSION
        let converted = params.to_sim_params();
        assert!(
            !has(converted.feature_flags, FeatureFlags::ENABLE_CONSTRAINTS),
            "converter has no residency knowledge, so it cannot set this bit"
        );

        // Three constraints are resident on the device.
        let dispatch = derive_dispatch_feature_flags(ForceDispatchInputs::new(&params, 3, false));
        assert!(
            has(dispatch, FeatureFlags::ENABLE_CONSTRAINTS),
            "the dispatch word adds the residency-derived bit the converter cannot know"
        );
        assert!(!has(dispatch, FeatureFlags::ENABLE_REPULSION));

        // Applying the dispatch word is what the wrapper does; the resulting
        // SimParams carries the dispatch word, not the converted one.
        let mut final_params = converted;
        final_params.feature_flags = dispatch;
        assert_eq!(final_params.feature_flags, dispatch);
        assert_ne!(
            final_params.feature_flags, converted.feature_flags,
            "the converter's word did not survive dispatch"
        );
    }

    #[test]
    fn gathering_from_settings_matches_a_hand_built_input() {
        let mut params = SimulationParams::new();
        params.repel_k = 2.0;
        params.spring_k = 0.0;
        params.center_gravity_k = 3.0;
        params.use_sssp_distances = true;
        let gathered = ForceDispatchInputs::new(&params, 7, true);
        assert_eq!(
            gathered,
            ForceDispatchInputs {
                repel_k: 2.0,
                spring_k: 0.0,
                center_gravity_k: 3.0,
                use_sssp_distances: true,
                sssp_spring_adjust_enabled: true,
                num_constraints: 7,
            }
        );
    }

    #[test]
    fn no_bit_outside_the_declared_feature_set_is_ever_produced() {
        // The derivation must never invent a bit; anything else in the word
        // would be an undeclared contract with the kernel.
        let known = FeatureFlags::ENABLE_REPULSION
            | FeatureFlags::ENABLE_SPRINGS
            | FeatureFlags::ENABLE_CENTERING
            | FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
            | FeatureFlags::ENABLE_CONSTRAINTS;
        for num_constraints in [0usize, 1] {
            for sssp in [false, true] {
                for scalar in [0.0f32, 1.0] {
                    let flags = derive_dispatch_feature_flags(ForceDispatchInputs {
                        repel_k: scalar,
                        spring_k: scalar,
                        center_gravity_k: scalar,
                        use_sssp_distances: sssp,
                        sssp_spring_adjust_enabled: sssp,
                        num_constraints,
                    });
                    assert_eq!(flags & !known, 0, "undeclared bit in the dispatch word");
                }
            }
        }
    }
}
