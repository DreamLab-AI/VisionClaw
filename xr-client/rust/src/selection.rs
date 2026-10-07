//! Selection arbiter — three resolvers into one [`SelectionEvent`]
//! (M4, ADR-130 Decision 4, copresence research brief §4).
//!
//! The three resolvers:
//! 1. **controller ray** — the existing [`crate::interaction`] ray, fired on an
//!    explicit trigger click.
//! 2. **hand pinch** — the existing pinch detection ([`crate::interaction::is_grab_active`])
//!    fed into the *same* ray path, fired on the pinch rising edge.
//! 3. **gaze dwell** — the smoothed [`crate::gaze::GazeRay`] charging a target
//!    over a configurable 400–800 ms band (600 ms default), with a target-size
//!    floor, activation hysteresis, and cancel-on-saccade — the Midas-touch
//!    mitigations from the brief.
//!
//! **Arbitration.** An explicit trigger (controller/pinch click) always beats a
//! dwell. Dwell only *arms* when no controller is tracked, or when hands-free
//! mode is set — otherwise a user holding controllers never accidentally dwells.
//!
//! The event carries the target's `did:nostr` when known. Identity is looked up
//! in a registry the client fills from presence `avatar_joined` events and from
//! the graph wire (see [`crate::binary_protocol::parse_agent_identities`] — the
//! additive `initialGraphLoad` extension and its named server-side emit point).

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;

use crate::gaze::GazeRay;
use crate::interaction::{find_target, is_grab_active, HandRay, TargetCandidate};

#[cfg(not(test))]
use godot::prelude::*;

/// Which resolver produced a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolver {
    ControllerRay,
    Pinch,
    GazeDwell,
}

impl Resolver {
    pub fn as_i32(self) -> i32 {
        match self {
            Resolver::ControllerRay => 0,
            Resolver::Pinch => 1,
            Resolver::GazeDwell => 2,
        }
    }
}

/// The output of the arbiter: a resolved selection of an agent/graph entity.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionEvent {
    pub target_entity: u32,
    /// The target's verified identity, when the client has it. `None` when the
    /// entity has no `did:nostr` in the registry yet (integration point below).
    pub did_nostr: Option<String>,
    pub resolver: Resolver,
    pub timestamp_us: u64,
}

/// A selectable entity: a graph node id, its position, and an acquisition radius
/// (metres). Small nodes are floored to [`SelectionConfig::target_radius_floor_m`]
/// so gaze can still land on them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionCandidate {
    pub node_id: u32,
    pub position: [f32; 3],
    pub radius: f32,
}

/// One controller/hand pointer this frame.
#[derive(Debug, Clone, Copy)]
pub struct PointerInput {
    /// 0 or 1 — the hand/controller slot, for rising-edge tracking.
    pub hand: u8,
    pub ray: HandRay,
    /// The controller trigger is held this frame.
    pub trigger_down: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionConfig {
    /// Default dwell duration (µs); clamped into the band below.
    pub dwell_us: u64,
    /// Configurable dwell band (µs). Research brief: 400–800 ms.
    pub dwell_min_us: u64,
    pub dwell_max_us: u64,
    /// Minimum acquisition radius for gaze dwell (target-size floor), metres.
    pub target_radius_floor_m: f32,
    /// Locked-target radius multiplier — hysteresis keeping a charging target.
    pub hysteresis_factor: f32,
    /// Angular gaze velocity (rad/s) above which the dwell charge is cancelled
    /// (cancel-on-saccade).
    pub saccade_cancel_rad_s: f32,
    /// Maximum ray/gaze reach, metres.
    pub max_distance_m: f32,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            dwell_us: 600_000,
            dwell_min_us: 400_000,
            dwell_max_us: 800_000,
            target_radius_floor_m: 0.35,
            hysteresis_factor: 1.6,
            saccade_cancel_rad_s: 4.0,
            max_distance_m: 30.0,
        }
    }
}

impl SelectionConfig {
    fn dwell_clamped_us(&self) -> u64 {
        self.dwell_us.clamp(self.dwell_min_us, self.dwell_max_us)
    }
}

/// The gaze-dwell charging state machine. Charges while gaze rests on one target
/// (within a hysteresis radius), cancels on saccade, and fires once per dwell.
#[derive(Debug, Clone)]
pub struct DwellCharger {
    target: Option<u32>,
    charge_us: u64,
    last_dir: Option<[f32; 3]>,
    fired_latch: bool,
}

impl DwellCharger {
    pub fn new() -> Self {
        Self {
            target: None,
            charge_us: 0,
            last_dir: None,
            fired_latch: false,
        }
    }

    pub fn reset(&mut self) {
        self.target = None;
        self.charge_us = 0;
        self.fired_latch = false;
        // last_dir kept so a reset-then-resume does not read as a saccade.
    }

    /// Progress toward firing, 0.0–1.0 — drives the charging reticle fill.
    pub fn charge_ratio(&self, cfg: &SelectionConfig) -> f32 {
        let dwell = cfg.dwell_clamped_us().max(1);
        (self.charge_us as f32 / dwell as f32).clamp(0.0, 1.0)
    }

    pub fn current_target(&self) -> Option<u32> {
        self.target
    }

    /// Advance one frame; returns `Some(node_id)` on the frame the dwell fires.
    pub fn tick(
        &mut self,
        gaze: &GazeRay,
        candidates: &[SelectionCandidate],
        dt_us: u64,
        cfg: &SelectionConfig,
    ) -> Option<u32> {
        let dir = normalise(gaze.dir);
        let dt_s = (dt_us as f32 / 1_000_000.0).max(1e-6);

        // cancel-on-saccade
        if let Some(prev) = self.last_dir {
            let ang = angle_between(prev, dir);
            if ang / dt_s > cfg.saccade_cancel_rad_s {
                self.target = None;
                self.charge_us = 0;
                self.fired_latch = false;
            }
        }
        self.last_dir = Some(dir);

        // Prefer keeping the currently charging target while it stays within the
        // (larger) hysteresis radius; else acquire the nearest fresh target.
        let mut chosen: Option<u32> = None;
        if let Some(t) = self.target {
            if let Some(c) = candidates.iter().find(|c| c.node_id == t) {
                let r = acquire_radius(c, cfg) * cfg.hysteresis_factor;
                if ray_hit(gaze.origin, dir, c.position, r, cfg.max_distance_m).is_some() {
                    chosen = Some(t);
                }
            }
        }
        if chosen.is_none() {
            let mut best: Option<(u32, f32)> = None;
            for c in candidates {
                let r = acquire_radius(c, cfg);
                if let Some(along) = ray_hit(gaze.origin, dir, c.position, r, cfg.max_distance_m) {
                    if best.is_none_or(|(_, d)| along < d) {
                        best = Some((c.node_id, along));
                    }
                }
            }
            chosen = best.map(|(id, _)| id);
        }

        let dwell = cfg.dwell_clamped_us();
        match chosen {
            Some(t) if self.target == Some(t) => {
                self.charge_us = (self.charge_us + dt_us).min(dwell);
            }
            Some(t) => {
                self.target = Some(t);
                self.charge_us = 0;
                self.fired_latch = false;
            }
            None => {
                self.target = None;
                self.charge_us = 0;
                self.fired_latch = false;
            }
        }

        if let Some(t) = self.target {
            if self.charge_us >= dwell && !self.fired_latch {
                self.fired_latch = true;
                return Some(t);
            }
        }
        None
    }
}

impl Default for DwellCharger {
    fn default() -> Self {
        Self::new()
    }
}

/// The arbiter: owns the resolver priority, the dwell charger, the rising-edge
/// state for explicit triggers, and the DID identity registry.
pub struct SelectionArbiter {
    cfg: SelectionConfig,
    dwell: DwellCharger,
    identities: HashMap<u32, String>,
    prev_trigger: [bool; 2],
    prev_pinch: [bool; 2],
}

impl SelectionArbiter {
    pub fn new() -> Self {
        Self::with_config(SelectionConfig::default())
    }

    pub fn with_config(cfg: SelectionConfig) -> Self {
        Self {
            cfg,
            dwell: DwellCharger::new(),
            identities: HashMap::new(),
            prev_trigger: [false; 2],
            prev_pinch: [false; 2],
        }
    }

    /// Register the `did:nostr` for a graph node so a resolved selection carries
    /// a verifiable identity. Populated from presence `avatar_joined` events and
    /// from [`crate::binary_protocol::parse_agent_identities`].
    pub fn register_identity(&mut self, node_id: u32, did_nostr: String) {
        self.identities.insert(node_id, did_nostr);
    }

    pub fn identity_of(&self, node_id: u32) -> Option<&String> {
        self.identities.get(&node_id)
    }

    pub fn charge_ratio(&self) -> f32 {
        self.dwell.charge_ratio(&self.cfg)
    }

    /// Resolve at most one selection for this frame. `controllers` is the set of
    /// tracked pointers, `gaze` the smoothed user gaze, `candidates` the
    /// selectable entities. Explicit clicks beat dwell; dwell only arms when no
    /// controller is tracked or `hands_free` is set.
    pub fn tick(
        &mut self,
        controllers: &[PointerInput],
        gaze: &GazeRay,
        candidates: &[SelectionCandidate],
        hands_free: bool,
        now_us: u64,
        dt_us: u64,
    ) -> Option<SelectionEvent> {
        let ic: Vec<TargetCandidate> = candidates
            .iter()
            .map(|c| TargetCandidate {
                node_id: c.node_id,
                position: c.position,
            })
            .collect();

        // 1. Explicit resolvers (rising edge of trigger or pinch), highest
        //    priority. Detect edges for present hands; reset edge state for
        //    absent hands so a reappearance reads as a fresh press.
        let mut present = [false; 2];
        let mut explicit: Option<SelectionEvent> = None;
        for p in controllers {
            let h = (p.hand as usize).min(1);
            present[h] = true;
            let trigger_edge = p.trigger_down && !self.prev_trigger[h];
            let pinch_now = is_grab_active(&p.ray);
            let pinch_edge = pinch_now && !self.prev_pinch[h];

            if explicit.is_none() && (trigger_edge || pinch_edge) {
                if let Some(hit) = find_target(&p.ray, &ic) {
                    let resolver = if trigger_edge {
                        Resolver::ControllerRay
                    } else {
                        Resolver::Pinch
                    };
                    explicit = Some(self.event(hit.node_id, resolver, now_us));
                }
            }
            self.prev_trigger[h] = p.trigger_down;
            self.prev_pinch[h] = pinch_now;
        }
        for (h, &is_present) in present.iter().enumerate() {
            if !is_present {
                self.prev_trigger[h] = false;
                self.prev_pinch[h] = false;
            }
        }

        if let Some(ev) = explicit {
            self.dwell.reset(); // an explicit pick supersedes any in-flight dwell
            return Some(ev);
        }

        // 2. Gaze dwell — only when hands-free or no controller is tracked.
        let controller_tracked = controllers.iter().any(|p| p.ray.is_tracking);
        if hands_free || !controller_tracked {
            if let Some(node) = self.dwell.tick(gaze, candidates, dt_us, &self.cfg) {
                return Some(self.event(node, Resolver::GazeDwell, now_us));
            }
        } else {
            self.dwell.reset();
        }
        None
    }

    fn event(&self, node_id: u32, resolver: Resolver, now_us: u64) -> SelectionEvent {
        SelectionEvent {
            target_entity: node_id,
            did_nostr: self.identities.get(&node_id).cloned(),
            resolver,
            timestamp_us: now_us,
        }
    }
}

impl Default for SelectionArbiter {
    fn default() -> Self {
        Self::new()
    }
}

fn acquire_radius(c: &SelectionCandidate, cfg: &SelectionConfig) -> f32 {
    c.radius.max(cfg.target_radius_floor_m)
}

/// Distance-along-ray if the ray passes within `radius` of `point`, else `None`.
fn ray_hit(origin: [f32; 3], dir: [f32; 3], point: [f32; 3], radius: f32, max_dist: f32) -> Option<f32> {
    let to = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
    let along = dot(&to, &dir);
    if along <= 0.0 || along > max_dist {
        return None;
    }
    let perp_sq = sq_len(&to) - along * along;
    if perp_sq > radius * radius {
        return None;
    }
    Some(along)
}

fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sq_len(a: &[f32; 3]) -> f32 {
    dot(a, a)
}

fn angle_between(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = dot(&normalise(a), &normalise(b)).clamp(-1.0, 1.0);
    d.acos()
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let len = sq_len(&v).sqrt();
    if len < 1e-6 || !len.is_finite() {
        return [0.0, 0.0, -1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

// --- Godot node --------------------------------------------------------------

/// GDScript-facing selection arbiter. Each frame the scene pushes the tracked
/// controllers, sets the candidate set and the smoothed gaze, then calls
/// [`Self::tick`]; a resolved selection emits `selection_made`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct SelectionArbiterNode {
    arbiter: SelectionArbiter,
    controllers: Vec<PointerInput>,
    candidates: Vec<SelectionCandidate>,
    gaze: GazeRay,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl SelectionArbiterNode {
    /// Emitted when a selection resolves. `resolver`: 0 ray, 1 pinch, 2 dwell.
    #[signal]
    fn selection_made(node_id: u32, did_nostr: GString, resolver: i32);

    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            arbiter: SelectionArbiter::new(),
            controllers: Vec::new(),
            candidates: Vec::new(),
            gaze: GazeRay {
                origin: [0.0, 0.0, 0.0],
                dir: [0.0, 0.0, -1.0],
                source: crate::gaze::GazeSource::Head,
            },
            base,
        })
    }

    /// Register a node's `did:nostr` (from a presence join or the graph wire).
    #[func]
    fn register_identity(&mut self, node_id: u32, did_nostr: GString) {
        self.arbiter.register_identity(node_id, did_nostr.to_string());
    }

    /// Clear the per-frame controller set; call before pushing this frame's hands.
    #[func]
    fn begin_frame(&mut self) {
        self.controllers.clear();
    }

    #[func]
    fn push_controller(
        &mut self,
        hand: i32,
        origin: Vector3,
        direction: Vector3,
        pinch_strength: f32,
        tracking: bool,
        trigger_down: bool,
    ) {
        self.controllers.push(PointerInput {
            hand: hand.clamp(0, 1) as u8,
            ray: HandRay {
                origin: [origin.x, origin.y, origin.z],
                direction: [direction.x, direction.y, direction.z],
                pinch_strength,
                is_tracking: tracking,
            },
            trigger_down,
        });
    }

    #[func]
    fn set_candidates(
        &mut self,
        ids: PackedInt32Array,
        positions: PackedVector3Array,
        radii: PackedFloat32Array,
    ) {
        self.candidates.clear();
        let ids = ids.as_slice();
        let pos = positions.as_slice();
        let rad = radii.as_slice();
        for i in 0..ids.len().min(pos.len()) {
            self.candidates.push(SelectionCandidate {
                node_id: ids[i] as u32,
                position: [pos[i].x, pos[i].y, pos[i].z],
                radius: rad.get(i).copied().unwrap_or(0.0),
            });
        }
    }

    #[func]
    fn set_gaze(&mut self, origin: Vector3, direction: Vector3) {
        self.gaze = GazeRay {
            origin: [origin.x, origin.y, origin.z],
            dir: [direction.x, direction.y, direction.z],
            source: crate::gaze::GazeSource::Head,
        };
    }

    /// Resolve a selection for this frame. Returns the node id, or -1 for none.
    #[func]
    fn tick(&mut self, hands_free: bool, now_us: i64, dt_us: i64) -> i64 {
        let ev = self.arbiter.tick(
            &self.controllers,
            &self.gaze,
            &self.candidates,
            hands_free,
            now_us.max(0) as u64,
            dt_us.max(0) as u64,
        );
        match ev {
            Some(ev) => {
                let did = ev.did_nostr.clone().unwrap_or_default();
                self.base_mut().emit_signal(
                    "selection_made",
                    &[
                        Variant::from(ev.target_entity),
                        Variant::from(GString::from(did)),
                        Variant::from(ev.resolver.as_i32()),
                    ],
                );
                ev.target_entity as i64
            }
            None => -1,
        }
    }

    /// Dwell charge 0.0–1.0 for the radial reticle fill.
    #[func]
    fn charge_ratio(&self) -> f32 {
        self.arbiter.charge_ratio()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaze::GazeSource;

    fn gaze_forward() -> GazeRay {
        GazeRay {
            origin: [0.0, 0.0, 0.0],
            dir: [0.0, 0.0, -1.0],
            source: GazeSource::Head,
        }
    }

    fn candidate(id: u32, pos: [f32; 3]) -> SelectionCandidate {
        SelectionCandidate {
            node_id: id,
            position: pos,
            radius: 0.5,
        }
    }

    fn tracked_ray(trigger: bool, pinch: f32) -> PointerInput {
        PointerInput {
            hand: 0,
            ray: HandRay {
                origin: [0.0, 0.0, 0.0],
                direction: [0.0, 0.0, -1.0],
                pinch_strength: pinch,
                is_tracking: true,
            },
            trigger_down: trigger,
        }
    }

    #[test]
    fn controller_trigger_click_selects_and_carries_did() {
        let mut arb = SelectionArbiter::new();
        arb.register_identity(7, format!("did:nostr:{}", "a".repeat(64)));
        let cands = [candidate(7, [0.0, 0.0, -5.0])];
        // frame 1: trigger pressed (rising edge) -> selects
        let ev = arb.tick(&[tracked_ray(true, 0.0)], &gaze_forward(), &cands, false, 1000, 16_000);
        let ev = ev.expect("trigger click should select");
        assert_eq!(ev.target_entity, 7);
        assert_eq!(ev.resolver, Resolver::ControllerRay);
        assert_eq!(ev.did_nostr, Some(format!("did:nostr:{}", "a".repeat(64))));
    }

    #[test]
    fn trigger_hold_does_not_repeat() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(1, [0.0, 0.0, -5.0])];
        assert!(arb
            .tick(&[tracked_ray(true, 0.0)], &gaze_forward(), &cands, false, 0, 16_000)
            .is_some());
        // trigger still down: no new rising edge -> no repeat select
        assert!(arb
            .tick(&[tracked_ray(true, 0.0)], &gaze_forward(), &cands, false, 16_000, 16_000)
            .is_none());
    }

    #[test]
    fn pinch_rising_edge_selects_via_pinch_resolver() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(3, [0.0, 0.0, -4.0])];
        let ev = arb
            .tick(&[tracked_ray(false, 0.9)], &gaze_forward(), &cands, false, 0, 16_000)
            .expect("pinch should select");
        assert_eq!(ev.resolver, Resolver::Pinch);
        assert_eq!(ev.target_entity, 3);
    }

    #[test]
    fn dwell_disabled_while_controller_tracked() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(1, [0.0, 0.0, -5.0])];
        // Controller tracked, not hands-free, no click: dwell must NOT charge/fire.
        for _ in 0..100 {
            let ev = arb.tick(
                &[tracked_ray(false, 0.0)],
                &gaze_forward(),
                &cands,
                false,
                0,
                16_000,
            );
            assert!(ev.is_none(), "dwell fired while controller tracked");
        }
        assert_eq!(arb.charge_ratio(), 0.0);
    }

    #[test]
    fn dwell_fires_when_no_controller() {
        let mut arb = SelectionArbiter::new();
        arb.register_identity(9, "did:nostr:beef".into()); // malformed but carried verbatim
        let cands = [candidate(9, [0.0, 0.0, -5.0])];
        // No controllers: dwell arms. 600 ms default dwell in 20 ms ticks = 30 ticks.
        let mut fired = None;
        for i in 0..40 {
            if let Some(ev) = arb.tick(&[], &gaze_forward(), &cands, false, i * 20_000, 20_000) {
                fired = Some(ev);
                break;
            }
        }
        let ev = fired.expect("dwell should fire with no controller");
        assert_eq!(ev.target_entity, 9);
        assert_eq!(ev.resolver, Resolver::GazeDwell);
    }

    #[test]
    fn dwell_arms_in_hands_free_even_with_controller() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(2, [0.0, 0.0, -5.0])];
        let mut fired = false;
        for i in 0..40 {
            // controller present but hands_free=true
            if arb
                .tick(
                    &[tracked_ray(false, 0.0)],
                    &gaze_forward(),
                    &cands,
                    true,
                    i * 20_000,
                    20_000,
                )
                .is_some()
            {
                fired = true;
                break;
            }
        }
        assert!(fired, "hands-free dwell should fire despite a tracked controller");
    }

    #[test]
    fn dwell_cancels_on_saccade() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(1, [0.0, 0.0, -5.0])];
        // Charge partway.
        for i in 0..10 {
            arb.tick(&[], &gaze_forward(), &cands, false, i * 20_000, 20_000);
        }
        assert!(arb.charge_ratio() > 0.0);
        // A hard saccade: gaze swings 90° in one 20 ms tick (>> 4 rad/s).
        let saccade = GazeRay {
            origin: [0.0, 0.0, 0.0],
            dir: [1.0, 0.0, 0.0],
            source: GazeSource::Head,
        };
        arb.tick(&[], &saccade, &cands, false, 200_000, 20_000);
        assert_eq!(arb.charge_ratio(), 0.0, "saccade must cancel the charge");
    }

    #[test]
    fn dwell_does_not_fire_below_min_band() {
        let cfg = SelectionConfig::default();
        let mut arb = SelectionArbiter::with_config(cfg);
        let cands = [candidate(1, [0.0, 0.0, -5.0])];
        // 380 ms < 400 ms min band: must not fire.
        let mut fired = false;
        for i in 0..19 {
            if arb.tick(&[], &gaze_forward(), &cands, false, i * 20_000, 20_000).is_some() {
                fired = true;
            }
        }
        assert!(!fired, "dwell fired before the 400 ms band floor");
    }

    #[test]
    fn no_did_yields_none_did_field() {
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(5, [0.0, 0.0, -5.0])];
        let ev = arb
            .tick(&[tracked_ray(true, 0.0)], &gaze_forward(), &cands, false, 0, 16_000)
            .unwrap();
        assert_eq!(ev.target_entity, 5);
        assert!(ev.did_nostr.is_none(), "unknown entity must carry no DID");
    }

    #[test]
    fn explicit_click_beats_dwell() {
        // With no controller, dwell charges; when a controller click lands the
        // same frame, the explicit resolver wins and the dwell resets.
        let mut arb = SelectionArbiter::new();
        let cands = [candidate(1, [0.0, 0.0, -5.0]), candidate(2, [3.0, 0.0, -4.0])];
        for i in 0..10 {
            arb.tick(&[], &gaze_forward(), &cands, false, i * 20_000, 20_000);
        }
        assert!(arb.charge_ratio() > 0.0);
        let ev = arb
            .tick(&[tracked_ray(true, 0.0)], &gaze_forward(), &cands, false, 300_000, 20_000)
            .unwrap();
        assert_ne!(ev.resolver, Resolver::GazeDwell);
        assert_eq!(arb.charge_ratio(), 0.0, "dwell must reset after explicit pick");
    }
}
