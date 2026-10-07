//! Proxemics solver — places N agents on a forward arc around the user
//! (M3, ADR-130 Decision 4, copresence research brief §3).
//!
//! Applies Hall's zones as radii, biased per the brief's empirical correction
//! (users stand closer to virtual agents than to humans): agents sit in the
//! **1.5–2.5 m social band**, never inside the 0.45 m intimate radius. They are
//! spread on an **arc, not a full circle** (±60° of forward by default), at
//! **equal angular spacing**, with a **minimum inter-agent chord**; the radius
//! grows (and, if the band is exhausted, the arc widens) as N grows so agents
//! never overlap.
//!
//! A pure function ([`solve`]) with deterministic output; the Godot side calls
//! it only on a membership or user-pose change ([`should_resolve`]), never
//! per-frame.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

#[cfg(not(test))]
use godot::prelude::*;

/// Placement parameters. Defaults follow ADR-130 Decision 4 / the research brief.
#[derive(Debug, Clone, Copy)]
pub struct ProxemicsConfig {
    /// Social-band radii `(min, max)` in metres. Default `(1.5, 2.5)`.
    pub band_min_m: f32,
    pub band_max_m: f32,
    /// Never place an agent inside this radius (Hall intimate zone).
    pub intimate_radius_m: f32,
    /// Half-angle of the default forward arc, radians. Default 60°.
    pub default_half_arc_rad: f32,
    /// Hard cap on the half-arc when the band is exhausted, radians. Default 85°.
    pub max_half_arc_rad: f32,
    /// Minimum distance between adjacent agents (no-overlap chord), metres.
    pub min_chord_m: f32,
    /// Vertical offset applied to every agent relative to the user position.
    pub agent_y_offset_m: f32,
    /// Re-solve position threshold: user must move this far to trigger a re-solve.
    pub resolve_pos_threshold_m: f32,
    /// Re-solve yaw threshold: user must turn this much (radians) to re-solve.
    pub resolve_yaw_threshold_rad: f32,
}

impl Default for ProxemicsConfig {
    fn default() -> Self {
        Self {
            band_min_m: 1.5,
            band_max_m: 2.5,
            intimate_radius_m: 0.45,
            default_half_arc_rad: 60.0_f32.to_radians(),
            max_half_arc_rad: 85.0_f32.to_radians(),
            min_chord_m: 0.6,
            agent_y_offset_m: 0.0,
            resolve_pos_threshold_m: 0.25,
            resolve_yaw_threshold_rad: 0.15,
        }
    }
}

/// One placement solution: agent positions plus the geometry chosen, so callers
/// (and tests) can inspect the radius and angular spread.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxemicsSolution {
    pub positions: Vec<[f32; 3]>,
    /// Radius actually used (metres), inside the band.
    pub radius_m: f32,
    /// Half-arc actually used (radians); equals the default unless widened.
    pub half_arc_rad: f32,
}

/// Solve agent placement. Pure and deterministic: identical inputs always give
/// identical output.
///
/// `user_pos` is the user's floor/eye position; `user_forward` is their facing
/// direction (projected to the horizontal plane internally). Agents are laid out
/// on the horizontal plane at `user_pos.y + agent_y_offset_m`.
pub fn solve(
    user_pos: [f32; 3],
    user_forward: [f32; 3],
    count: usize,
    cfg: &ProxemicsConfig,
) -> ProxemicsSolution {
    let fwd = horizontal_forward(user_forward);
    let y = user_pos[1] + cfg.agent_y_offset_m;

    if count == 0 {
        return ProxemicsSolution {
            positions: Vec::new(),
            radius_m: cfg.band_min_m,
            half_arc_rad: cfg.default_half_arc_rad,
        };
    }

    if count == 1 {
        let r = base_radius(cfg);
        let p = place(user_pos, fwd, 0.0, r, y);
        return ProxemicsSolution {
            positions: vec![p],
            radius_m: r,
            half_arc_rad: cfg.default_half_arc_rad,
        };
    }

    // N >= 2. First try to satisfy the chord by growing the radius within the
    // band at the default arc; if the band is exhausted, pin the radius at the
    // band max and widen the arc (capped) instead.
    let n = count as f32;
    let default_full_arc = 2.0 * cfg.default_half_arc_rad;
    let delta_default = default_full_arc / (n - 1.0);
    let r_for_default = cfg.min_chord_m / (2.0 * (delta_default * 0.5).sin());

    let (radius, full_arc) = if r_for_default <= cfg.band_max_m {
        // Chord met within the band at the default arc.
        let r = r_for_default.max(cfg.band_min_m).min(cfg.band_max_m);
        (r, default_full_arc)
    } else {
        // Band exhausted: pin to band max, widen the arc to keep the chord.
        let r = cfg.band_max_m;
        let ratio = (cfg.min_chord_m / (2.0 * r)).clamp(-1.0, 1.0);
        let delta_req = 2.0 * ratio.asin();
        let needed_full = (delta_req * (n - 1.0)).min(2.0 * cfg.max_half_arc_rad);
        let full = needed_full.max(default_full_arc);
        (r, full)
    };

    let half_arc = full_arc * 0.5;
    let delta = full_arc / (n - 1.0);
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let angle = -half_arc + (i as f32) * delta;
        positions.push(place(user_pos, fwd, angle, radius, y));
    }

    ProxemicsSolution {
        positions,
        radius_m: radius,
        half_arc_rad: half_arc,
    }
}

/// Whether the layout should be re-solved given how far the user has moved /
/// turned and whether the agent count changed. Keeps the solver off the
/// per-frame path (research brief §3: "run only on membership/user-pose change").
pub fn should_resolve(
    prev_pos: [f32; 3],
    prev_forward: [f32; 3],
    prev_count: usize,
    new_pos: [f32; 3],
    new_forward: [f32; 3],
    new_count: usize,
    cfg: &ProxemicsConfig,
) -> bool {
    if prev_count != new_count {
        return true;
    }
    let dx = new_pos[0] - prev_pos[0];
    let dy = new_pos[1] - prev_pos[1];
    let dz = new_pos[2] - prev_pos[2];
    if (dx * dx + dy * dy + dz * dz).sqrt() > cfg.resolve_pos_threshold_m {
        return true;
    }
    let a = horizontal_forward(prev_forward);
    let b = horizontal_forward(new_forward);
    let dot = (a[0] * b[0] + a[2] * b[2]).clamp(-1.0, 1.0);
    dot.acos() > cfg.resolve_yaw_threshold_rad
}

fn base_radius(cfg: &ProxemicsConfig) -> f32 {
    (0.5 * (cfg.band_min_m + cfg.band_max_m)).clamp(cfg.band_min_m, cfg.band_max_m)
}

/// Forward vector flattened to the horizontal plane and normalised; degenerate
/// (straight up/down) falls back to `-Z`.
fn horizontal_forward(f: [f32; 3]) -> [f32; 3] {
    let len = (f[0] * f[0] + f[2] * f[2]).sqrt();
    if len < 1e-6 || !len.is_finite() {
        return [0.0, 0.0, -1.0];
    }
    [f[0] / len, 0.0, f[2] / len]
}

/// Rotate the horizontal forward by `angle` about +Y and step out `radius`.
fn place(user_pos: [f32; 3], fwd: [f32; 3], angle: f32, radius: f32, y: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    // Rotation of [fwd.x, fwd.z] about the vertical axis.
    let dir_x = fwd[0] * c + fwd[2] * s;
    let dir_z = -fwd[0] * s + fwd[2] * c;
    [
        user_pos[0] + dir_x * radius,
        y,
        user_pos[2] + dir_z * radius,
    ]
}

// --- Godot node --------------------------------------------------------------

/// GDScript-facing proxemics solver. The scene calls [`Self::solve`] on join /
/// leave / recentre and reads back the flattened positions.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct ProxemicsSolver {
    cfg: ProxemicsConfig,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl ProxemicsSolver {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            cfg: ProxemicsConfig::default(),
            base,
        })
    }

    /// Override the social band (metres). Values are ordered defensively.
    #[func]
    fn set_band(&mut self, min_m: f32, max_m: f32) {
        self.cfg.band_min_m = min_m.min(max_m);
        self.cfg.band_max_m = min_m.max(max_m);
    }

    /// Solve placement for `count` agents around the user; returns one Vector3
    /// per agent in scene space.
    #[func]
    fn solve(&self, user_pos: Vector3, user_forward: Vector3, count: i64) -> PackedVector3Array {
        let sol = solve(
            [user_pos.x, user_pos.y, user_pos.z],
            [user_forward.x, user_forward.y, user_forward.z],
            count.max(0) as usize,
            &self.cfg,
        );
        let out: Vec<Vector3> = sol
            .positions
            .iter()
            .map(|p| Vector3::new(p[0], p[1], p[2]))
            .collect();
        PackedVector3Array::from(out.as_slice())
    }

    /// Whether a re-solve is warranted for the given pose/membership change.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn should_resolve(
        &self,
        prev_pos: Vector3,
        prev_forward: Vector3,
        prev_count: i64,
        new_pos: Vector3,
        new_forward: Vector3,
        new_count: i64,
    ) -> bool {
        should_resolve(
            [prev_pos.x, prev_pos.y, prev_pos.z],
            [prev_forward.x, prev_forward.y, prev_forward.z],
            prev_count.max(0) as usize,
            [new_pos.x, new_pos.y, new_pos.z],
            [new_forward.x, new_forward.y, new_forward.z],
            new_count.max(0) as usize,
            &self.cfg,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn horiz_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dx = a[0] - b[0];
        let dz = a[2] - b[2];
        (dx * dx + dz * dz).sqrt()
    }

    fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        let dz = a[2] - b[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    #[test]
    fn single_agent_dead_ahead() {
        let cfg = ProxemicsConfig::default();
        let sol = solve([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 1, &cfg);
        assert_eq!(sol.positions.len(), 1);
        let p = sol.positions[0];
        // Directly in front (−Z), no lateral offset.
        assert!(p[0].abs() < 1e-4, "x should be ~0, got {}", p[0]);
        assert!(p[2] < 0.0, "should be in front (−Z), got {}", p[2]);
        assert!(horiz_dist([0.0; 3], p) >= cfg.band_min_m - 1e-4);
    }

    #[test]
    fn empty_is_empty() {
        let cfg = ProxemicsConfig::default();
        assert!(solve([0.0; 3], [0.0, 0.0, -1.0], 0, &cfg)
            .positions
            .is_empty());
    }

    #[test]
    fn all_in_band_and_outside_intimate() {
        let cfg = ProxemicsConfig::default();
        for n in 1..=10 {
            let sol = solve([1.0, 1.6, 2.0], [0.0, 0.0, -1.0], n, &cfg);
            for p in &sol.positions {
                let d = horiz_dist([1.0, 1.6, 2.0], *p);
                assert!(
                    d >= cfg.band_min_m - 1e-3,
                    "n={n} radius {d} below band min"
                );
                assert!(
                    d <= cfg.band_max_m + 1e-3,
                    "n={n} radius {d} above band max"
                );
                assert!(
                    d > cfg.intimate_radius_m,
                    "n={n} agent inside intimate zone"
                );
            }
        }
    }

    #[test]
    fn no_adjacent_overlap() {
        let cfg = ProxemicsConfig::default();
        for n in 2..=10 {
            let sol = solve([0.0; 3], [0.0, 0.0, -1.0], n, &cfg);
            for w in sol.positions.windows(2) {
                let chord = dist3(w[0], w[1]);
                assert!(
                    chord >= cfg.min_chord_m - 1e-3,
                    "n={n} adjacent chord {chord} below min {}",
                    cfg.min_chord_m
                );
            }
        }
    }

    #[test]
    fn agents_stay_within_arc() {
        let cfg = ProxemicsConfig::default();
        let user = [0.0, 0.0, 0.0];
        let fwd = [0.0, 0.0, -1.0];
        for n in 1..=10 {
            let sol = solve(user, fwd, n, &cfg);
            for p in &sol.positions {
                // angle of the agent direction from forward, in the horizontal plane
                let dir = [p[0] - user[0], p[2] - user[2]];
                let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
                let cos = (-dir[1] / len).clamp(-1.0, 1.0); // forward is −Z
                let ang = cos.acos();
                assert!(
                    ang <= sol.half_arc_rad + 1e-3,
                    "n={n} agent angle {ang} exceeds half-arc {}",
                    sol.half_arc_rad
                );
            }
        }
    }

    #[test]
    fn deterministic() {
        let cfg = ProxemicsConfig::default();
        let a = solve([0.3, 1.7, -0.2], [0.2, 0.1, -0.9], 6, &cfg);
        let b = solve([0.3, 1.7, -0.2], [0.2, 0.1, -0.9], 6, &cfg);
        assert_eq!(a, b);
    }

    #[test]
    fn symmetric_about_forward_for_even_split() {
        // Two agents should sit mirror-image about the forward axis.
        let cfg = ProxemicsConfig::default();
        let sol = solve([0.0; 3], [0.0, 0.0, -1.0], 2, &cfg);
        let (l, r) = (sol.positions[0], sol.positions[1]);
        assert!(
            (l[0] + r[0]).abs() < 1e-3,
            "x not mirrored: {} vs {}",
            l[0],
            r[0]
        );
        assert!(
            (l[2] - r[2]).abs() < 1e-3,
            "z should match: {} vs {}",
            l[2],
            r[2]
        );
    }

    #[test]
    fn radius_grows_with_agent_count() {
        let cfg = ProxemicsConfig::default();
        let r2 = solve([0.0; 3], [0.0, 0.0, -1.0], 2, &cfg).radius_m;
        let r8 = solve([0.0; 3], [0.0, 0.0, -1.0], 8, &cfg).radius_m;
        assert!(
            r8 >= r2,
            "radius should not shrink as N grows: {r2} -> {r8}"
        );
    }

    #[test]
    fn should_resolve_on_count_change() {
        let cfg = ProxemicsConfig::default();
        assert!(should_resolve(
            [0.0; 3],
            [0.0, 0.0, -1.0],
            3,
            [0.0; 3],
            [0.0, 0.0, -1.0],
            4,
            &cfg
        ));
    }

    #[test]
    fn should_not_resolve_on_tiny_move() {
        let cfg = ProxemicsConfig::default();
        assert!(!should_resolve(
            [0.0; 3],
            [0.0, 0.0, -1.0],
            3,
            [0.05, 0.0, 0.05],
            [0.0, 0.0, -1.0],
            3,
            &cfg
        ));
    }

    #[test]
    fn should_resolve_on_large_turn() {
        let cfg = ProxemicsConfig::default();
        assert!(should_resolve(
            [0.0; 3],
            [0.0, 0.0, -1.0],
            3,
            [0.0; 3],
            [1.0, 0.0, 0.0], // turned 90°
            3,
            &cfg
        ));
    }

    #[test]
    fn placement_follows_user_facing() {
        // Facing +X, the single agent should be along +X, not −Z.
        let cfg = ProxemicsConfig::default();
        let sol = solve([0.0; 3], [1.0, 0.0, 0.0], 1, &cfg);
        let p = sol.positions[0];
        assert!(p[0] > 0.0, "agent should be along +X facing, got {p:?}");
        assert!(p[2].abs() < 1e-3, "no Z offset expected, got {}", p[2]);
    }
}
