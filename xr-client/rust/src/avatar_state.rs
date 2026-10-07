//! Per-agent avatar state: the activity machine and the gaze-attention model
//! (M3, ADR-130 Decision 4, copresence research brief §1–2).
//!
//! Two coupled pieces drive one agent's legible embodiment:
//!
//! - [`ActivityMachine`] — a discrete state machine (`idle` → `working` →
//!   `awaiting_approval` → `speaking`) advanced by [`AgentSignal`]s derived from
//!   agent-events / ACSP kinds arriving over transport. Its state chooses the
//!   avatar's colour/motion (idle bob/dim, working pulse, awaiting-approval
//!   saturated colour) — no skeleton, per the brief.
//!
//! - [`GazeAttentionModel`] — where the agent's gaze cone points. Attention is
//!   the user (mutual gaze), a referenced graph node (deixis during a task), or
//!   nobody. The model returns gaze **briefly and smoothly** so it reads as
//!   attention, not a turret: a mutual-gaze response only after the user dwells
//!   over 200 ms on the agent, a reaction latency before re-aiming, a bounded slew
//!   rate, and a settle-hold after the user looks away (hysteresis against
//!   flicker). Timing constants follow the attention-management literature
//!   (Pejsa/Andrist/Gleicher/Mutlu) cited in the brief.
//!
//! Together they produce the [`AgentPresence`] snapshot the codec
//! (`visionclaw_xr_presence::agent_presence`) puts on the wire.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use visionclaw_xr_presence::agent_presence::{
    decode_agent_presence, AgentActivity, AgentPresence, AgentPresenceBatch, AttentionTarget,
    OPCODE_AGENT_PRESENCE,
};

#[cfg(not(test))]
use godot::prelude::*;

// --- activity machine --------------------------------------------------------

/// A signal derived from an agent-event / ACSP kind that advances the activity
/// machine. The mapping from ACSP kinds (31400–31405) to these signals is a
/// transport-layer concern (Stage 2); this enum is the machine's alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSignal {
    /// Agent began executing a task (e.g. an accepted 31402 ActionRequest).
    TaskStarted,
    /// Agent finished its task and has no follow-on work.
    TaskCompleted,
    /// Agent raised a judgment for human approval (broker case queued).
    ApprovalRequested,
    /// A queued approval was granted — the agent resumes work.
    ApprovalGranted,
    /// A queued approval was denied — the agent drops the task.
    ApprovalDenied,
    /// Agent started speaking (TTS / voice channel active).
    SpeechStarted,
    /// Agent stopped speaking; it returns to whatever it was doing before.
    SpeechEnded,
    /// Explicit idle (heartbeat with no active work).
    WentIdle,
}

/// The discrete activity machine. `speaking` is an overlay: when speech ends the
/// machine returns to the activity it held before speaking began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityMachine {
    state: AgentActivity,
    /// The activity to restore when `speaking` ends.
    resume_after_speech: AgentActivity,
}

impl ActivityMachine {
    pub fn new() -> Self {
        Self {
            state: AgentActivity::Idle,
            resume_after_speech: AgentActivity::Idle,
        }
    }

    pub fn state(&self) -> AgentActivity {
        self.state
    }

    /// Apply a signal, returning the resulting state.
    pub fn apply(&mut self, signal: AgentSignal) -> AgentActivity {
        use AgentActivity::*;
        use AgentSignal::*;
        self.state = match (self.state, signal) {
            // Speech overlay: remember the underlying activity, restore on end.
            (current, SpeechStarted) => {
                if current != Speaking {
                    self.resume_after_speech = current;
                }
                Speaking
            }
            (Speaking, SpeechEnded) => self.resume_after_speech,
            (_, SpeechEnded) => self.state, // stray end while not speaking: no-op

            (_, TaskStarted) => Working,
            (_, TaskCompleted) => Idle,
            (_, WentIdle) => Idle,

            (Working, ApprovalRequested) => AwaitingApproval,
            (_, ApprovalRequested) => AwaitingApproval,
            (AwaitingApproval, ApprovalGranted) => Working,
            (_, ApprovalGranted) => self.state, // grant with nothing pending: no-op
            (AwaitingApproval, ApprovalDenied) => Idle,
            (_, ApprovalDenied) => self.state,
        };
        // Keep the resume target coherent if the underlying activity changed
        // while speaking (e.g. a task completed mid-utterance).
        if self.state == Speaking {
            // resume target already set above on entry to Speaking
        } else {
            self.resume_after_speech = self.state;
        }
        self.state
    }
}

impl Default for ActivityMachine {
    fn default() -> Self {
        Self::new()
    }
}

// --- gaze-attention model ----------------------------------------------------

/// Attention timing constants (microseconds / radians), from the brief.
#[derive(Debug, Clone, Copy)]
pub struct AttentionConfig {
    /// User must dwell on the agent this long before it returns gaze (mutual
    /// gaze). Empirical focal fixation is ~325 ms; 200 ms is the engagement floor.
    pub mutual_gaze_dwell_us: u64,
    /// After the user looks away, hold mutual gaze this long before releasing —
    /// the hysteresis that stops the gaze snapping away like a turret.
    pub settle_hold_us: u64,
    /// Reaction latency before the gaze begins slewing to a *new* target (a
    /// saccade does not start instantly).
    pub reaction_latency_us: u64,
    /// Maximum angular slew rate of the gaze cone, radians/second — the cap that
    /// makes re-aiming glide rather than snap.
    pub max_slew_rate_rad_s: f32,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            mutual_gaze_dwell_us: 200_000,
            settle_hold_us: 700_000,
            reaction_latency_us: 150_000,
            max_slew_rate_rad_s: 6.0,
        }
    }
}

/// Per-tick inputs to the attention model.
#[derive(Debug, Clone, Copy)]
pub struct AttentionInputs {
    /// The user's (smoothed) gaze ray is currently resolving to this agent.
    pub user_gazing_at_me: bool,
    /// Unit direction from the agent toward the user (for a mutual-gaze response).
    pub dir_to_user: [f32; 3],
    /// The agent's current task deixis target, if any: `(node_id, unit dir to it)`.
    pub deixis: Option<(u32, [f32; 3])>,
    /// Microseconds since the previous tick.
    pub dt_us: u64,
}

/// Where the gaze points and how it moves there. Time-driven; call [`tick`] once
/// per frame.
///
/// [`tick`]: GazeAttentionModel::tick
#[derive(Debug, Clone, Copy)]
pub struct GazeAttentionModel {
    attention: AttentionTarget,
    gaze_dir: [f32; 3],
    user_dwell_us: u64,
    hold_remaining_us: u64,
    reaction_remaining_us: u64,
    cfg: AttentionConfig,
}

impl GazeAttentionModel {
    pub fn new() -> Self {
        Self::with_config(AttentionConfig::default())
    }

    pub fn with_config(cfg: AttentionConfig) -> Self {
        Self {
            attention: AttentionTarget::None,
            gaze_dir: [0.0, 0.0, -1.0],
            user_dwell_us: 0,
            hold_remaining_us: 0,
            reaction_remaining_us: 0,
            cfg,
        }
    }

    pub fn attention(&self) -> AttentionTarget {
        self.attention
    }

    pub fn gaze_dir(&self) -> [f32; 3] {
        self.gaze_dir
    }

    /// Advance the model. Returns the current attention target after this tick.
    pub fn tick(&mut self, input: AttentionInputs) -> AttentionTarget {
        let prev_attention = self.attention;

        // 1. Mutual-gaze engagement / hold.
        if input.user_gazing_at_me {
            self.user_dwell_us = self.user_dwell_us.saturating_add(input.dt_us);
            if self.attention == AttentionTarget::User {
                self.hold_remaining_us = self.cfg.settle_hold_us; // refresh hold
            }
        } else {
            self.user_dwell_us = 0; // engagement needs continuous dwell
            self.hold_remaining_us = self.hold_remaining_us.saturating_sub(input.dt_us);
        }

        let engaged = self.attention == AttentionTarget::User;
        if !engaged && self.user_dwell_us >= self.cfg.mutual_gaze_dwell_us {
            self.attention = AttentionTarget::User;
            self.hold_remaining_us = self.cfg.settle_hold_us;
        } else if engaged && !input.user_gazing_at_me && self.hold_remaining_us == 0 {
            // Release mutual gaze to the task deixis, or nobody.
            self.attention = deixis_target(&input);
        } else if !engaged {
            // Not in mutual gaze: follow task deixis.
            self.attention = deixis_target(&input);
        }

        // 2. A change of target incurs a reaction latency before the slew starts.
        if self.attention != prev_attention {
            self.reaction_remaining_us = self.cfg.reaction_latency_us;
        }

        // 3. Choose the desired gaze direction for the current target.
        let desired = match self.attention {
            AttentionTarget::User => normalise(input.dir_to_user),
            AttentionTarget::GraphNode(_) => input
                .deixis
                .map(|(_, d)| normalise(d))
                .unwrap_or(self.gaze_dir),
            AttentionTarget::None => self.gaze_dir, // hold last (ambient)
        };

        // 4. Slew toward the desired direction, gated by the reaction latency and
        //    the max angular rate — glide, never snap.
        if self.reaction_remaining_us > 0 {
            self.reaction_remaining_us = self.reaction_remaining_us.saturating_sub(input.dt_us);
        } else {
            let max_step = self.cfg.max_slew_rate_rad_s * (input.dt_us as f32 / 1_000_000.0);
            self.gaze_dir = rotate_toward(self.gaze_dir, desired, max_step);
        }

        self.attention
    }
}

impl Default for GazeAttentionModel {
    fn default() -> Self {
        Self::new()
    }
}

fn deixis_target(input: &AttentionInputs) -> AttentionTarget {
    match input.deixis {
        Some((id, _)) => AttentionTarget::GraphNode(id),
        None => AttentionTarget::None,
    }
}

/// Rotate unit vector `cur` toward unit vector `target` by at most `max_angle`
/// radians (normalised lerp — a good small-step slerp approximation).
fn rotate_toward(cur: [f32; 3], target: [f32; 3], max_angle: f32) -> [f32; 3] {
    let c = normalise(cur);
    let t = normalise(target);
    let dot = (c[0] * t[0] + c[1] * t[1] + c[2] * t[2]).clamp(-1.0, 1.0);
    let angle = dot.acos();
    if angle <= max_angle || angle < 1e-5 {
        return t;
    }
    let frac = (max_angle / angle).clamp(0.0, 1.0);
    normalise([
        c[0] + frac * (t[0] - c[0]),
        c[1] + frac * (t[1] - c[1]),
        c[2] + frac * (t[2] - c[2]),
    ])
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-6 || !len.is_finite() {
        return [0.0, 0.0, -1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

/// The full per-agent avatar: activity + attention, producing the wire snapshot.
#[derive(Debug, Clone, Copy)]
pub struct AgentAvatar {
    pub activity: ActivityMachine,
    pub gaze: GazeAttentionModel,
}

impl AgentAvatar {
    pub fn new() -> Self {
        Self {
            activity: ActivityMachine::new(),
            gaze: GazeAttentionModel::new(),
        }
    }

    /// The presence snapshot to hand to the codec.
    pub fn presence(&self) -> AgentPresence {
        AgentPresence::new(
            self.activity.state(),
            self.gaze.gaze_dir(),
            self.gaze.attention(),
        )
    }
}

impl Default for AgentAvatar {
    fn default() -> Self {
        Self::new()
    }
}

// --- Remote co-presence consumption (ADR-2020) -------------------------------

/// Remote agents' co-presence, reconstructed from inbound `0x44` frames.
///
/// The server broadcasts *deltas*: unchanged fields are elided, so a consumer
/// cannot read one frame in isolation. This store holds the last full state per
/// transport `local_id` and folds each delta onto it, which is what makes the
/// elision safe. It is the client half of the ADR-2020 integration — the codec
/// existed on both sides, but nothing consumed a live frame.
#[derive(Debug, Clone, Default)]
pub struct RemotePresenceStore {
    agents: std::collections::HashMap<u32, AgentPresence>,
    /// Highest sequence folded in. A delta stream applied out of order silently
    /// reconstructs a state that never existed on the server, so this mirrors the
    /// ADR-2018 freshness gate on the graph socket.
    watermark: Option<u64>,
    stale_frames: u64,
}

impl RemotePresenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of remote agents currently known.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Last known presence for a transport `local_id`.
    pub fn get(&self, local_id: u32) -> Option<AgentPresence> {
        self.agents.get(&local_id).copied()
    }

    /// The graph node this remote agent is attending to, if any. This is the
    /// client side of node correlation: the id is in the same 26-bit wire space
    /// as the position stream, so the scene can resolve the node it names.
    pub fn attention_node(&self, local_id: u32) -> Option<u32> {
        match self.agents.get(&local_id)?.attention {
            AttentionTarget::GraphNode(id) => Some(id),
            _ => None,
        }
    }

    /// Every known agent's transport id, ascending.
    pub fn agent_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.agents.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Highest broadcast sequence folded in.
    pub fn watermark(&self) -> Option<u64> {
        self.watermark
    }

    /// Frames refused for arriving out of order.
    pub fn stale_frames(&self) -> u64 {
        self.stale_frames
    }

    /// Remove an agent whose co-presence the server retired (stale removal —
    /// the server announces this on the JSON room-event channel). Returns whether
    /// anything was actually removed.
    pub fn remove(&mut self, local_id: u32) -> bool {
        self.agents.remove(&local_id).is_some()
    }

    /// Fold one decoded `0x44` batch into the store.
    ///
    /// Older or duplicate sequences are refused whole. Returns the number of
    /// agents updated, or `None` when the batch was refused as stale.
    pub fn apply_batch(&mut self, batch: &AgentPresenceBatch) -> Option<usize> {
        if let Some(wm) = self.watermark {
            if batch.seq <= wm {
                self.stale_frames = self.stale_frames.saturating_add(1);
                return None;
            }
        }
        self.watermark = Some(batch.seq);
        let mut updated = 0usize;
        for delta in &batch.deltas {
            // A delta for an unknown agent folds onto a neutral base, so a client
            // that joined mid-stream still converges once a full delta arrives.
            let base = self
                .agents
                .get(&delta.local_id)
                .copied()
                .unwrap_or_else(|| {
                    AgentPresence::new(AgentActivity::Idle, [0.0, 0.0, -1.0], AttentionTarget::None)
                });
            self.agents.insert(delta.local_id, delta.apply(&base));
            updated += 1;
        }
        Some(updated)
    }

    /// Decode an inbound `0x44` frame and fold it in. Declines frames that are
    /// not agent-presence frames, so a demultiplexer can offer every binary frame
    /// to this store without it claiming a sibling opcode's bytes.
    pub fn ingest_frame(&mut self, bytes: &[u8]) -> Result<Option<usize>, String> {
        if bytes.first().copied() != Some(OPCODE_AGENT_PRESENCE) {
            return Err(format!(
                "not an agent-presence frame: opcode {:?}, expected {:#04x}",
                bytes.first(),
                OPCODE_AGENT_PRESENCE
            ));
        }
        let batch = decode_agent_presence(bytes).map_err(|e| e.to_string())?;
        Ok(self.apply_batch(&batch))
    }
}

// --- Godot node --------------------------------------------------------------

/// One agent's avatar model, driven from GDScript. The scene applies signals as
/// agent-events arrive and ticks attention each frame with the user's gaze test.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct AgentAvatarNode {
    avatar: AgentAvatar,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl AgentAvatarNode {
    /// Emitted when the discrete activity state changes (reliable wire path).
    #[signal]
    fn activity_changed(state: i32);

    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            avatar: AgentAvatar::new(),
            base,
        })
    }

    /// Apply an [`AgentSignal`] by its ordinal (see `signal_from_i32`).
    #[func]
    fn apply_signal(&mut self, signal: i32) {
        let before = self.avatar.activity.state();
        if let Some(sig) = signal_from_i32(signal) {
            let after = self.avatar.activity.apply(sig);
            if after != before {
                self.base_mut()
                    .emit_signal("activity_changed", &[Variant::from(activity_to_i32(after))]);
            }
        }
    }

    /// Advance the gaze-attention model one frame.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn tick_attention(
        &mut self,
        user_gazing_at_me: bool,
        dir_to_user: Vector3,
        has_deixis: bool,
        deixis_node: u32,
        deixis_dir: Vector3,
        dt_us: i64,
    ) {
        let deixis = if has_deixis {
            Some((deixis_node, [deixis_dir.x, deixis_dir.y, deixis_dir.z]))
        } else {
            None
        };
        self.avatar.gaze.tick(AttentionInputs {
            user_gazing_at_me,
            dir_to_user: [dir_to_user.x, dir_to_user.y, dir_to_user.z],
            deixis,
            dt_us: dt_us.max(0) as u64,
        });
    }

    #[func]
    fn activity(&self) -> i32 {
        activity_to_i32(self.avatar.activity.state())
    }

    #[func]
    fn gaze_dir(&self) -> Vector3 {
        let d = self.avatar.gaze.gaze_dir();
        Vector3::new(d[0], d[1], d[2])
    }

    /// Attention tag: 0 none, 1 user, 2 graph node.
    #[func]
    fn attention_tag(&self) -> i32 {
        match self.avatar.gaze.attention() {
            AttentionTarget::None => 0,
            AttentionTarget::User => 1,
            AttentionTarget::GraphNode(_) => 2,
        }
    }

    /// Attention graph-node id, or 0 when attention is not a node.
    #[func]
    fn attention_node(&self) -> u32 {
        match self.avatar.gaze.attention() {
            AttentionTarget::GraphNode(id) => id,
            _ => 0,
        }
    }
}

#[cfg(not(test))]
fn signal_from_i32(v: i32) -> Option<AgentSignal> {
    Some(match v {
        0 => AgentSignal::TaskStarted,
        1 => AgentSignal::TaskCompleted,
        2 => AgentSignal::ApprovalRequested,
        3 => AgentSignal::ApprovalGranted,
        4 => AgentSignal::ApprovalDenied,
        5 => AgentSignal::SpeechStarted,
        6 => AgentSignal::SpeechEnded,
        7 => AgentSignal::WentIdle,
        _ => return None,
    })
}

#[cfg(not(test))]
fn activity_to_i32(a: AgentActivity) -> i32 {
    a.as_u8() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle() {
        let m = ActivityMachine::new();
        assert_eq!(m.state(), AgentActivity::Idle);
    }

    #[test]
    fn task_started_goes_working() {
        let mut m = ActivityMachine::new();
        assert_eq!(m.apply(AgentSignal::TaskStarted), AgentActivity::Working);
    }

    #[test]
    fn approval_flow() {
        let mut m = ActivityMachine::new();
        m.apply(AgentSignal::TaskStarted);
        assert_eq!(
            m.apply(AgentSignal::ApprovalRequested),
            AgentActivity::AwaitingApproval
        );
        assert_eq!(
            m.apply(AgentSignal::ApprovalGranted),
            AgentActivity::Working
        );
    }

    #[test]
    fn approval_denied_goes_idle() {
        let mut m = ActivityMachine::new();
        m.apply(AgentSignal::TaskStarted);
        m.apply(AgentSignal::ApprovalRequested);
        assert_eq!(m.apply(AgentSignal::ApprovalDenied), AgentActivity::Idle);
    }

    #[test]
    fn speech_overlay_restores_prior_activity() {
        let mut m = ActivityMachine::new();
        m.apply(AgentSignal::TaskStarted); // Working
        assert_eq!(m.apply(AgentSignal::SpeechStarted), AgentActivity::Speaking);
        // Speech ends -> back to Working, not Idle.
        assert_eq!(m.apply(AgentSignal::SpeechEnded), AgentActivity::Working);
    }

    #[test]
    fn speech_overlay_restores_awaiting_approval() {
        let mut m = ActivityMachine::new();
        m.apply(AgentSignal::TaskStarted);
        m.apply(AgentSignal::ApprovalRequested); // AwaitingApproval
        m.apply(AgentSignal::SpeechStarted); // Speaking
        assert_eq!(
            m.apply(AgentSignal::SpeechEnded),
            AgentActivity::AwaitingApproval
        );
    }

    #[test]
    fn stray_speech_end_is_noop() {
        let mut m = ActivityMachine::new();
        m.apply(AgentSignal::TaskStarted);
        assert_eq!(m.apply(AgentSignal::SpeechEnded), AgentActivity::Working);
    }

    // --- gaze-attention ---

    fn gazing(dt_us: u64) -> AttentionInputs {
        AttentionInputs {
            user_gazing_at_me: true,
            dir_to_user: [0.0, 0.0, 1.0],
            deixis: None,
            dt_us,
        }
    }

    fn not_gazing(dt_us: u64) -> AttentionInputs {
        AttentionInputs {
            user_gazing_at_me: false,
            dir_to_user: [0.0, 0.0, 1.0],
            deixis: None,
            dt_us,
        }
    }

    #[test]
    fn mutual_gaze_requires_dwell_threshold() {
        let mut g = GazeAttentionModel::new();
        // 199 ms of dwell in ~11 ms ticks: must NOT engage.
        for _ in 0..18 {
            g.tick(gazing(11_000));
        }
        assert_eq!(g.attention(), AttentionTarget::None, "engaged too early");
        // Cross 200 ms.
        for _ in 0..4 {
            g.tick(gazing(11_000));
        }
        assert_eq!(
            g.attention(),
            AttentionTarget::User,
            "should engage after 200ms"
        );
    }

    #[test]
    fn dwell_must_be_continuous() {
        let mut g = GazeAttentionModel::new();
        for _ in 0..15 {
            g.tick(gazing(11_000));
        }
        // Break gaze once — accumulator resets.
        g.tick(not_gazing(11_000));
        for _ in 0..15 {
            g.tick(gazing(11_000));
        }
        // 15 continuous ticks = 165 ms < 200 ms: still not engaged.
        assert_eq!(g.attention(), AttentionTarget::None);
    }

    #[test]
    fn mutual_gaze_holds_through_brief_lookaway() {
        let mut g = GazeAttentionModel::new();
        for _ in 0..30 {
            g.tick(gazing(11_000)); // engage
        }
        assert_eq!(g.attention(), AttentionTarget::User);
        // Look away briefly (300 ms < 700 ms hold) — must stay engaged.
        for _ in 0..27 {
            g.tick(not_gazing(11_000));
        }
        assert_eq!(g.attention(), AttentionTarget::User, "dropped during hold");
    }

    #[test]
    fn mutual_gaze_releases_after_hold_expires() {
        let mut g = GazeAttentionModel::new();
        for _ in 0..30 {
            g.tick(gazing(11_000));
        }
        assert_eq!(g.attention(), AttentionTarget::User);
        // Look away for > 700 ms hold.
        for _ in 0..80 {
            g.tick(not_gazing(11_000));
        }
        assert_eq!(
            g.attention(),
            AttentionTarget::None,
            "should release after hold"
        );
    }

    #[test]
    fn deixis_drives_attention_when_no_mutual_gaze() {
        let mut g = GazeAttentionModel::new();
        let input = AttentionInputs {
            user_gazing_at_me: false,
            dir_to_user: [0.0, 0.0, 1.0],
            deixis: Some((77, [1.0, 0.0, 0.0])),
            dt_us: 11_000,
        };
        g.tick(input);
        assert_eq!(g.attention(), AttentionTarget::GraphNode(77));
    }

    #[test]
    fn gaze_slews_not_snaps() {
        let mut g = GazeAttentionModel::new();
        // Target the user directly behind (dir +Z) from the default −Z gaze:
        // a 180° change. After the reaction latency, the first slewing tick must
        // move only a bounded step, never jump straight to the target.
        let input = AttentionInputs {
            user_gazing_at_me: false,
            dir_to_user: [0.0, 0.0, 1.0],
            deixis: Some((1, [0.0, 0.0, 1.0])),
            dt_us: 16_000,
        };
        // Burn the reaction latency.
        for _ in 0..12 {
            g.tick(input);
        }
        let after = g.gaze_dir();
        // It must have started turning but not completed the 180°.
        assert!(
            after[2] < 1.0 - 1e-3,
            "gaze snapped to target instantly: {after:?}"
        );
    }

    #[test]
    fn gaze_converges_to_target_over_time() {
        let mut g = GazeAttentionModel::new();
        let input = AttentionInputs {
            user_gazing_at_me: false,
            dir_to_user: [0.0, 0.0, 1.0],
            deixis: Some((1, [1.0, 0.0, 0.0])),
            dt_us: 16_000,
        };
        for _ in 0..400 {
            g.tick(input);
        }
        let d = g.gaze_dir();
        assert!(
            (d[0] - 1.0).abs() < 1e-2,
            "gaze did not converge to +X: {d:?}"
        );
    }

    #[test]
    fn avatar_presence_snapshot_reflects_state() {
        let mut a = AgentAvatar::new();
        a.activity.apply(AgentSignal::TaskStarted);
        let p = a.presence();
        assert_eq!(p.state, AgentActivity::Working);
    }
}
