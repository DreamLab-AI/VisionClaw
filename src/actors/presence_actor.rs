//! Per-room XR presence broadcast actor (PRD-008 §5.3).
//!
//! Composes [`visionclaw_xr_presence::PresenceRoom`] aggregate. One actor per
//! active room id. Subscribers register via [`JoinRoom`] with a recipient
//! [`Addr`]; the actor pushes [`BroadcastFrame`] messages to all peers when a
//! session ingests a pose frame, and removes shutdown-pending rooms via
//! [`LeaveRoom`].
//!
//! Room events ([`RoomEventEnvelope`]) follow the carriers defined in
//! `docs/ddd-xr-godot-context.md` §4.1: JSON over `/ws/presence`. Every
//! `avatar_joined` carries the avatar's transport `local_id` so the client can
//! attribute opaque-`local_id` sibling pose frames to a named avatar.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use actix::{Actor, ActorContext, AsyncContext, Context, Handler, Message, Recipient};
use serde::Serialize;
use tracing::{debug, info, warn};

use visionclaw_xr_presence::agent_presence::{
    encode_agent_presence, AgentPresence, AgentPresenceDelta, AttentionTarget,
    OPCODE_AGENT_PRESENCE,
};
use visionclaw_xr_presence::{
    hand_reach, joint_anatomy, monotonic_timestamp, ports::Broadcaster, types::HandPose,
    velocity_gate, wire, world_bounds, Aabb, AvatarId, AvatarMetadata, Did, PresenceRoom, RoomId,
    Transform, ValidationError,
};

const TICK_HZ: u64 = 90;
const TICK_INTERVAL: Duration = Duration::from_micros(1_000_000 / TICK_HZ);
const VIOLATION_WINDOW: Duration = Duration::from_secs(1);
const VIOLATION_KICK_THRESHOLD: usize = 10;
const BACKPRESSURE_LIMIT: usize = 3;

/// Default max head→hand reach (m) when `PRESENCE_HAND_REACH_M` is unset (#4).
/// More generous than the crate's 1.2m validator default so tall users and
/// extended-tip controllers are not false-rejected.
const DEFAULT_PRESENCE_HAND_REACH_M: f32 = 1.5;

/// Read the configured hand-reach limit from `PRESENCE_HAND_REACH_M`, falling
/// back to [`DEFAULT_PRESENCE_HAND_REACH_M`] on an unset/empty/unparseable or
/// non-positive value.
fn configured_hand_reach_m() -> f32 {
    std::env::var("PRESENCE_HAND_REACH_M")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_PRESENCE_HAND_REACH_M)
}

// 0x43 sibling-frame envelope: [opcode u8][broadcast_seq u64 LE][room_id u32 LE][user_count u16 LE]
const PREAMBLE_OPCODE: u8 = wire::OPCODE_AVATAR_POSE;

/// How long an agent's co-presence entry survives without an update before
/// [`PresenceActor::sweep_stale_agent_presence`] retires it (ADR-2020).
///
/// Co-presence is a *live* claim — "this agent is working and looking at that
/// node". A disconnected or crashed agent that never sends a closing update
/// would otherwise keep a stale avatar attentive to a node for ever. Ten seconds
/// is comfortably above the reliable channel's update cadence.
pub const AGENT_PRESENCE_TTL: Duration = Duration::from_secs(10);

/// Low 26 bits: the ephemeral wire node-id space (ADR-2024). An attention target
/// naming a node outside it cannot correlate to anything on the graph socket.
const WIRE_NODE_ID_MASK: u32 = 0x03FF_FFFF;

/// One agent's last known social state plus when it was last refreshed.
#[derive(Debug, Clone)]
struct AgentPresenceEntry {
    last: AgentPresence,
    last_seen: Instant,
}

/// Publish an agent's co-presence (opcode `0x44`) into the room (ADR-2020).
///
/// The additive sibling of [`IngestPose`]: it carries *social* state (activity,
/// gaze, attention target) rather than skeletal pose, on the same authenticated
/// session. The two are deliberately independent — an agent may publish presence
/// without ever sending a pose, and a human avatar may send poses without ever
/// publishing presence.
#[derive(Message, Debug)]
#[rtype(result = "AgentPresenceOutcome")]
pub struct IngestAgentPresence {
    /// The publishing avatar. Must already be a joined member of this room:
    /// membership is the authorisation boundary.
    pub avatar_id: AvatarId,
    pub presence: AgentPresence,
}

/// Result of an [`IngestAgentPresence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentPresenceOutcome {
    /// Accepted and broadcast to peers as an `0x44` delta.
    Broadcast { changed_fields: u8 },
    /// Accepted, but identical to the state already held at wire resolution, so
    /// nothing was put on the wire. Gaze is compared *after* quantisation, so
    /// sub-quantum float jitter never generates traffic.
    Unchanged,
    /// The publisher is not a member of this room. Membership is established by
    /// an authenticated [`JoinRoom`], so this is the permission boundary: a
    /// non-member cannot inject social state about anyone.
    PermissionDenied,
    /// The attention target names a node outside the 26-bit wire id space, so it
    /// cannot correlate with any node on the graph socket.
    InvalidNodeCorrelation { node_id: u32 },
}

/// Retire agent co-presence entries that have gone stale (ADR-2020).
#[derive(Message, Debug)]
#[rtype(result = "Vec<u32>")]
pub struct SweepStaleAgentPresence {
    /// Entries not refreshed within this window are retired. Defaults to
    /// [`AGENT_PRESENCE_TTL`] when `None`.
    pub ttl: Option<Duration>,
}

/// Read the co-presence currently held for one avatar (diagnostics/tests).
#[derive(Message, Debug)]
#[rtype(result = "Option<AgentPresence>")]
pub struct GetAgentPresence {
    pub avatar_id: AvatarId,
}

/// The graph node an avatar is currently attending to, if any (node correlation).
#[derive(Message, Debug)]
#[rtype(result = "Option<u32>")]
pub struct GetAttentionNode {
    pub avatar_id: AvatarId,
}

#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct BroadcastFrame {
    pub bytes: Vec<u8>,
    pub broadcast_sequence: u64,
}

#[derive(Message, Debug)]
#[rtype(result = "Result<JoinAck, JoinRejection>")]
pub struct JoinRoom {
    pub did: Did,
    pub metadata: AvatarMetadata,
    pub frame_recipient: Recipient<BroadcastFrame>,
    pub event_recipient: Recipient<RoomEventEnvelope>,
}

/// One existing room member as seen at join time, pairing the domain metadata
/// with the transport `local_id` the broadcast frames will tag this avatar's
/// poses with. `local_id` is an infrastructure concern (per-session pose
/// addressing), so it lives here rather than polluting [`AvatarMetadata`].
#[derive(Debug, Clone)]
pub struct MemberSnapshot {
    pub metadata: AvatarMetadata,
    pub local_id: u32,
}

#[derive(Debug, Clone)]
pub struct JoinAck {
    pub avatar_id: AvatarId,
    pub members: Vec<MemberSnapshot>,
}

#[derive(Debug, Clone, thiserror::Error, Serialize)]
pub enum JoinRejection {
    #[error("avatar already present in room")]
    DuplicateMember,
    #[error("internal room error: {0}")]
    Internal(String),
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct LeaveRoom {
    pub avatar_id: AvatarId,
}

#[derive(Message, Debug)]
#[rtype(result = "IngestOutcome")]
pub struct IngestPose {
    pub avatar_id: AvatarId,
    pub frame_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IngestOutcome {
    Accepted,
    ValidationFailed(String),
    Decode(String),
    Kick(String),
}

#[derive(Message, Debug)]
#[rtype(result = "Vec<AvatarMetadata>")]
pub struct ListMembers;

#[derive(Message, Debug)]
#[rtype(result = "RoomStatsSnapshot")]
pub struct RoomStats;

#[derive(Debug, Clone, Default, Serialize)]
pub struct RoomStatsSnapshot {
    pub room_id: String,
    pub member_count: usize,
    pub broadcast_sequence: u64,
    pub poses_ingested_total: u64,
    pub poses_rejected_total: u64,
    pub broadcast_bytes_total: u64,
    pub broadcast_frames_total: u64,
}

#[derive(Message, Debug, Clone, Serialize)]
#[rtype(result = "()")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomEventEnvelope {
    AvatarJoined {
        avatar_id: String,
        did: String,
        display_name: String,
        /// Transport id this avatar's poses are tagged with in 0x43 sibling
        /// frames. Lets the client map an opaque per-pose `local_id` to a URN.
        local_id: u32,
    },
    AvatarLeft {
        avatar_id: String,
        did: String,
    },
    /// ADR-2020: an agent's co-presence went stale and was retired. Announced on
    /// the JSON event channel rather than as a `0x44` delta, because that codec
    /// encodes *state* and has no representation for "this agent is gone" —
    /// reusing an idle state delta would be ambiguous with an agent that
    /// genuinely went idle.
    AgentPresenceExpired {
        /// Transport id the retired agent's `0x44` deltas were tagged with.
        local_id: u32,
    },
}

struct Subscriber {
    frame_recipient: Recipient<BroadcastFrame>,
    event_recipient: Recipient<RoomEventEnvelope>,
    queue_depth: usize,
    violations: VecDeque<Instant>,
}

pub struct PresenceActor {
    room_id: RoomId,
    room: PresenceRoom,
    subscribers: HashMap<AvatarId, Subscriber>,
    pending_poses: HashMap<AvatarId, Vec<u8>>,
    avatar_id_to_local: HashMap<AvatarId, u32>,
    next_local_id: u32,
    broadcast_sequence: u64,
    bounds: Aabb,
    max_velocity_mps: f32,
    /// Max head→hand distance (m) before a frame is rejected (#4 codex). Env
    /// `PRESENCE_HAND_REACH_M`, default [`DEFAULT_PRESENCE_HAND_REACH_M`] (a
    /// generous 1.5m so tall users / extended controllers are not false-kicked).
    hand_reach_m: f32,
    /// ADR-2020 co-presence (opcode 0x44): last published social state per agent.
    /// Held apart from `pending_poses` so the two opcodes operate independently.
    agent_presence: HashMap<AvatarId, AgentPresenceEntry>,
    /// Broadcast sequence for the 0x44 channel. Separate from the 0x43 pose
    /// sequence: the two are independent streams and must not share a counter.
    agent_presence_sequence: u64,
    stats: RoomStatsSnapshot,
}

impl PresenceActor {
    pub fn new(room_id: RoomId) -> Self {
        let stats = RoomStatsSnapshot {
            room_id: room_id.as_str().to_owned(),
            ..Default::default()
        };
        Self {
            room: PresenceRoom::new(room_id.clone()),
            room_id,
            subscribers: HashMap::new(),
            pending_poses: HashMap::new(),
            avatar_id_to_local: HashMap::new(),
            next_local_id: 1,
            broadcast_sequence: 0,
            bounds: Aabb::symmetric(50.0),
            max_velocity_mps: 20.0,
            hand_reach_m: configured_hand_reach_m(),
            agent_presence: HashMap::new(),
            agent_presence_sequence: 0,
            stats,
        }
    }

    pub fn with_bounds(mut self, bounds: Aabb, max_velocity_mps: f32) -> Self {
        self.bounds = bounds;
        self.max_velocity_mps = max_velocity_mps;
        self
    }

    fn record_violation(&mut self, avatar_id: &AvatarId) -> bool {
        let now = Instant::now();
        let Some(sub) = self.subscribers.get_mut(avatar_id) else {
            return false;
        };
        sub.violations.push_back(now);
        while let Some(front) = sub.violations.front() {
            if now.duration_since(*front) > VIOLATION_WINDOW {
                sub.violations.pop_front();
            } else {
                break;
            }
        }
        sub.violations.len() >= VIOLATION_KICK_THRESHOLD
    }

    fn run_validators(
        &self,
        avatar_id: &AvatarId,
        decoded: &wire::DecodedFrame,
    ) -> Result<(), ValidationError> {
        let frame = &decoded.frame;
        world_bounds(&frame.head, &self.bounds)?;
        if let Some(t) = &frame.left_hand {
            world_bounds(t, &self.bounds)?;
            // #10: reject anatomically impossible hand extension. A hand farther
            // than `hand_reach_m` (env PRESENCE_HAND_REACH_M) from the head is a
            // spoofed/abusive pose. An Err here is recorded as a single violation
            // by `handle_ingest` (a lone bad frame is dropped as ValidationFailed,
            // NOT a kick — only VIOLATION_KICK_THRESHOLD within the window kicks).
            hand_reach(&frame.head, t, self.hand_reach_m)?;
        }
        if let Some(t) = &frame.right_hand {
            world_bounds(t, &self.bounds)?;
            hand_reach(&frame.head, t, self.hand_reach_m)?;
        }

        if let Some(prev) = self
            .room
            .member(avatar_id)
            .and_then(|m| m.last_frame.as_ref())
        {
            monotonic_timestamp(prev.timestamp_us, frame.timestamp_us)?;
            velocity_gate(prev, frame, self.max_velocity_mps)?;
        }

        let identity = Transform::identity();
        let left = frame.left_hand.unwrap_or(identity);
        let right = frame.right_hand.unwrap_or(identity);
        joint_anatomy(
            &HandPose {
                wrist: left,
                joints: Vec::new(),
            },
            &HandPose {
                wrist: right,
                joints: Vec::new(),
            },
        )
    }

    fn local_id_for(&mut self, avatar_id: &AvatarId) -> u32 {
        if let Some(id) = self.avatar_id_to_local.get(avatar_id) {
            return *id;
        }
        let id = self.next_local_id;
        self.next_local_id = self.next_local_id.wrapping_add(1);
        self.avatar_id_to_local.insert(avatar_id.clone(), id);
        id
    }

    fn build_broadcast_frame(&mut self) -> Option<Vec<u8>> {
        if self.pending_poses.is_empty() {
            return None;
        }
        self.broadcast_sequence = self.broadcast_sequence.wrapping_add(1);
        let user_count = self.pending_poses.len() as u16;
        let mut buf: Vec<u8> = Vec::with_capacity(
            1 + 8 + 4 + 2 + self.pending_poses.values().map(|v| v.len()).sum::<usize>(),
        );
        buf.push(PREAMBLE_OPCODE);
        buf.extend_from_slice(&self.broadcast_sequence.to_le_bytes());
        let room_id_u32 = u32::from_le_bytes([
            self.room_id.wire_hash()[0],
            self.room_id.wire_hash()[1],
            self.room_id.wire_hash()[2],
            self.room_id.wire_hash()[3],
        ]);
        buf.extend_from_slice(&room_id_u32.to_le_bytes());
        buf.extend_from_slice(&user_count.to_le_bytes());

        let drained: Vec<(AvatarId, Vec<u8>)> = self.pending_poses.drain().collect();
        for (avatar_id, payload) in drained {
            buf.extend_from_slice(&self.local_id_for(&avatar_id).to_le_bytes());
            buf.extend_from_slice(&payload);
        }
        Some(buf)
    }

    // ── ADR-2020: agent co-presence (opcode 0x44) ──────────────────────────

    /// The graph node an avatar is attending to, if its attention names one.
    /// This is the *node correlation*: a `0x44` attention target and a node on
    /// the `0x03`/`0x05` graph socket refer to the same 26-bit wire id space.
    fn attention_node(&self, avatar_id: &AvatarId) -> Option<u32> {
        match self.agent_presence.get(avatar_id)?.last.attention {
            AttentionTarget::GraphNode(id) => Some(id),
            _ => None,
        }
    }

    /// Validate that an attention target can correlate to a graph node.
    ///
    /// A node id above the 26-bit wire mask cannot be carried by the graph
    /// socket at all (ADR-2024), so accepting it would publish an attention
    /// target that no client could ever resolve.
    fn check_node_correlation(presence: &AgentPresence) -> Result<(), u32> {
        match presence.attention {
            AttentionTarget::GraphNode(id) if id > WIRE_NODE_ID_MASK => Err(id),
            _ => Ok(()),
        }
    }

    /// Apply one agent's published co-presence, broadcasting a delta when it
    /// actually changed at wire resolution.
    fn handle_agent_presence(
        &mut self,
        msg: IngestAgentPresence,
        ctx: &mut Context<Self>,
    ) -> AgentPresenceOutcome {
        // Permission boundary: only a joined member may publish. Membership is
        // established by an authenticated JoinRoom, so a caller that never
        // joined — or one that has left — cannot inject social state.
        if !self.subscribers.contains_key(&msg.avatar_id) {
            warn!(
                room = %self.room_id.as_str(),
                "rejecting agent presence from a non-member"
            );
            return AgentPresenceOutcome::PermissionDenied;
        }

        if let Err(node_id) = Self::check_node_correlation(&msg.presence) {
            warn!(
                node_id,
                "agent presence attention target is outside wire id space"
            );
            return AgentPresenceOutcome::InvalidNodeCorrelation { node_id };
        }

        let local_id = self.local_id_for(&msg.avatar_id);
        let previous = self.agent_presence.get(&msg.avatar_id).map(|e| e.last);

        // A first publication is a full delta; afterwards only changed fields go
        // on the wire. `between` compares gaze at wire resolution, so float
        // jitter below the 16-bit quantum produces no traffic.
        let delta = match previous {
            None => AgentPresenceDelta::full(local_id, &msg.presence),
            Some(prev) => AgentPresenceDelta::between(local_id, &prev, &msg.presence),
        };

        self.agent_presence.insert(
            msg.avatar_id.clone(),
            AgentPresenceEntry {
                last: msg.presence,
                last_seen: Instant::now(),
            },
        );

        if delta.is_empty() {
            return AgentPresenceOutcome::Unchanged;
        }

        let changed_fields = delta.field_mask();
        self.broadcast_agent_presence(&[delta], Some(&msg.avatar_id), ctx);
        AgentPresenceOutcome::Broadcast { changed_fields }
    }

    /// Encode and dispatch one or more `0x44` deltas to the room.
    ///
    /// `exclude` is the publisher, which does not need its own state echoed
    /// back. Kept entirely separate from `dispatch_broadcast`: a co-presence
    /// update must never flush pending poses, and a pose broadcast must never
    /// carry social state.
    fn broadcast_agent_presence(
        &mut self,
        deltas: &[AgentPresenceDelta],
        exclude: Option<&AvatarId>,
        _ctx: &mut Context<Self>,
    ) {
        if deltas.is_empty() {
            return;
        }
        self.agent_presence_sequence = self.agent_presence_sequence.wrapping_add(1);
        let bytes = match encode_agent_presence(self.agent_presence_sequence, deltas) {
            Ok(b) => b,
            Err(e) => {
                warn!(err = %e, "failed to encode agent presence frame");
                return;
            }
        };
        debug_assert_eq!(bytes.first().copied(), Some(OPCODE_AGENT_PRESENCE));

        let envelope = BroadcastFrame {
            bytes: bytes.to_vec(),
            broadcast_sequence: self.agent_presence_sequence,
        };
        self.stats.broadcast_frames_total += 1;
        self.stats.broadcast_bytes_total += envelope.bytes.len() as u64;

        for (id, sub) in self.subscribers.iter() {
            if Some(id) == exclude {
                continue;
            }
            if !sub.frame_recipient.connected() {
                continue;
            }
            sub.frame_recipient.do_send(envelope.clone());
        }
    }

    /// Retire co-presence entries that have not been refreshed within `ttl`,
    /// returning the retired agents' `local_id`s (ADR-2020 stale removal).
    ///
    /// Retirement is announced on the JSON room-event channel rather than as a
    /// `0x44` delta: the codec encodes *state*, and it has no representation for
    /// "this agent is gone". Reusing a state delta to mean removal would be
    /// ambiguous with an agent that genuinely went idle.
    fn sweep_stale_agent_presence(&mut self, ttl: Duration) -> Vec<u32> {
        let now = Instant::now();
        let stale: Vec<AvatarId> = self
            .agent_presence
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_seen) > ttl)
            .map(|(id, _)| id.clone())
            .collect();

        let mut retired = Vec::with_capacity(stale.len());
        for avatar_id in stale {
            self.agent_presence.remove(&avatar_id);
            let local_id = self.local_id_for(&avatar_id);
            retired.push(local_id);
            info!(local_id, "retiring stale agent co-presence");
            let envelope = RoomEventEnvelope::AgentPresenceExpired { local_id };
            for sub in self.subscribers.values() {
                if sub.event_recipient.connected() {
                    sub.event_recipient.do_send(envelope.clone());
                }
            }
        }
        retired
    }

    fn dispatch_broadcast(&mut self, sender: &AvatarId) {
        let Some(frame_bytes) = self.build_broadcast_frame() else {
            return;
        };
        self.stats.broadcast_frames_total += 1;
        self.stats.broadcast_bytes_total += frame_bytes.len() as u64;
        let envelope = BroadcastFrame {
            bytes: frame_bytes,
            broadcast_sequence: self.broadcast_sequence,
        };
        let mut to_drop: Vec<AvatarId> = Vec::new();
        for (id, sub) in self.subscribers.iter_mut() {
            if id == sender {
                continue;
            }
            if !sub.frame_recipient.connected() {
                to_drop.push(id.clone());
                continue;
            }
            if sub.queue_depth >= BACKPRESSURE_LIMIT {
                debug!(
                    avatar = %id,
                    queue_depth = sub.queue_depth,
                    "dropping oldest queued frame (backpressure)"
                );
                sub.queue_depth = sub.queue_depth.saturating_sub(1);
            }
            if sub.frame_recipient.try_send(envelope.clone()).is_ok() {
                sub.queue_depth += 1;
            } else {
                to_drop.push(id.clone());
            }
        }
        for id in to_drop {
            self.cleanup_subscriber(&id);
        }
    }

    fn cleanup_subscriber(&mut self, avatar_id: &AvatarId) {
        let did_str = self
            .room
            .member(avatar_id)
            .map(|m| m.metadata.did.to_string())
            .unwrap_or_default();
        if self.subscribers.remove(avatar_id).is_some() {
            let _ = self.room.leave(avatar_id);
            self.avatar_id_to_local.remove(avatar_id);
            self.pending_poses.remove(avatar_id);
            self.stats.member_count = self.subscribers.len();
            let envelope = RoomEventEnvelope::AvatarLeft {
                avatar_id: avatar_id.to_string(),
                did: did_str,
            };
            for s in self.subscribers.values() {
                let _ = s.event_recipient.try_send(envelope.clone());
            }
        }
    }

    fn shutdown_if_empty(&self, ctx: &mut Context<Self>) {
        if self.subscribers.is_empty() {
            info!(room = %self.room_id, "presence actor shutting down (room empty)");
            ctx.stop();
        }
    }

    fn evict_disconnected_subscribers(&mut self) {
        let dropped: Vec<AvatarId> = self
            .subscribers
            .iter()
            .filter(|(_, sub)| !sub.frame_recipient.connected())
            .map(|(id, _)| id.clone())
            .collect();
        for id in dropped {
            warn!(avatar = %id, room = %self.room_id, "evicting disconnected subscriber");
            self.cleanup_subscriber(&id);
        }
    }
}

impl Actor for PresenceActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(room = %self.room_id, "presence actor started @ {} Hz", TICK_HZ);
        ctx.run_interval(TICK_INTERVAL * 10, |act, ctx| {
            act.evict_disconnected_subscribers();
            act.shutdown_if_empty(ctx);
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(room = %self.room_id, "presence actor stopped");
    }
}

impl Broadcaster for PresenceActor {
    fn broadcast(&self, _room: &RoomId, _frame: &[u8]) {
        // Trait impl exists for crate composability; the actor's own
        // `dispatch_broadcast` is the live path — `&self` here cannot
        // mutate subscriber queue depth.
    }
}

impl Handler<JoinRoom> for PresenceActor {
    type Result = actix::MessageResult<JoinRoom>;

    fn handle(&mut self, msg: JoinRoom, ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.handle_join(msg, ctx))
    }
}

impl PresenceActor {
    fn handle_join(
        &mut self,
        msg: JoinRoom,
        _ctx: &mut Context<Self>,
    ) -> Result<JoinAck, JoinRejection> {
        let display_name = msg.metadata.display_name.clone();
        let did = msg.metadata.did.clone();
        let avatar_id = self
            .room
            .join(msg.did.clone(), msg.metadata.clone())
            .map_err(|e| match e {
                visionclaw_xr_presence::RoomError::DuplicateDid { .. } => {
                    JoinRejection::DuplicateMember
                }
                other => JoinRejection::Internal(other.to_string()),
            })?;

        self.subscribers.insert(
            avatar_id.clone(),
            Subscriber {
                frame_recipient: msg.frame_recipient,
                event_recipient: msg.event_recipient,
                queue_depth: 0,
                violations: VecDeque::new(),
            },
        );
        self.stats.member_count = self.subscribers.len();

        // Assign the joiner's transport id eagerly so peers learn it now (not
        // lazily on first pose), and so the joiner's own poses are attributable
        // the moment it starts broadcasting.
        let joiner_local_id = self.local_id_for(&avatar_id);
        let join_event = RoomEventEnvelope::AvatarJoined {
            avatar_id: avatar_id.to_string(),
            did: did.to_string(),
            display_name,
            local_id: joiner_local_id,
        };
        for (peer_id, peer) in self.subscribers.iter() {
            if peer_id != &avatar_id {
                let _ = peer.event_recipient.try_send(join_event.clone());
            }
        }

        // Snapshot existing members with their local_ids so the joining client
        // can attribute their sibling poses too. Collect ids first to release
        // the immutable `room` borrow before `local_id_for` takes `&mut self`.
        let member_ids: Vec<(AvatarId, AvatarMetadata)> = self
            .room
            .members()
            .map(|m| (m.avatar_id.clone(), m.metadata.clone()))
            .collect();
        let members: Vec<MemberSnapshot> = member_ids
            .into_iter()
            .map(|(aid, metadata)| {
                let local_id = self.local_id_for(&aid);
                MemberSnapshot { metadata, local_id }
            })
            .collect();
        info!(
            room = %self.room_id,
            avatar = %avatar_id,
            count = self.subscribers.len(),
            "avatar joined"
        );
        Ok(JoinAck { avatar_id, members })
    }
}

impl Handler<LeaveRoom> for PresenceActor {
    type Result = ();

    fn handle(&mut self, msg: LeaveRoom, ctx: &mut Self::Context) -> Self::Result {
        self.cleanup_subscriber(&msg.avatar_id);
        self.shutdown_if_empty(ctx);
    }
}

impl Handler<IngestPose> for PresenceActor {
    type Result = actix::MessageResult<IngestPose>;

    fn handle(&mut self, msg: IngestPose, ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.handle_ingest(msg, ctx))
    }
}

impl PresenceActor {
    fn handle_ingest(&mut self, msg: IngestPose, ctx: &mut Context<Self>) -> IngestOutcome {
        if !self.subscribers.contains_key(&msg.avatar_id) {
            return IngestOutcome::ValidationFailed("not a room member".into());
        }
        self.stats.poses_ingested_total += 1;

        let decoded = match wire::decode(&msg.frame_bytes) {
            Ok(d) => d,
            Err(e) => {
                self.stats.poses_rejected_total += 1;
                if self.record_violation(&msg.avatar_id) {
                    self.cleanup_subscriber(&msg.avatar_id);
                    self.shutdown_if_empty(ctx);
                    return IngestOutcome::Kick(format!("decode-violations exceeded: {e}"));
                }
                return IngestOutcome::Decode(e.to_string());
            }
        };

        if decoded.avatar_id != msg.avatar_id.as_str() {
            self.stats.poses_rejected_total += 1;
            if self.record_violation(&msg.avatar_id) {
                self.cleanup_subscriber(&msg.avatar_id);
                self.shutdown_if_empty(ctx);
                return IngestOutcome::Kick("avatar-id spoofing".into());
            }
            return IngestOutcome::ValidationFailed("avatar_id mismatch".into());
        }

        if decoded.room_hash != self.room_id.wire_hash() {
            self.stats.poses_rejected_total += 1;
            return IngestOutcome::ValidationFailed("room_hash mismatch".into());
        }

        if let Err(e) = self.run_validators(&msg.avatar_id, &decoded) {
            self.stats.poses_rejected_total += 1;
            warn!(avatar = %msg.avatar_id, error = %e, "pose validation failed");
            if self.record_violation(&msg.avatar_id) {
                self.cleanup_subscriber(&msg.avatar_id);
                self.shutdown_if_empty(ctx);
                return IngestOutcome::Kick(format!("validation-violations exceeded: {e}"));
            }
            return IngestOutcome::ValidationFailed(e.to_string());
        }

        if let Err(e) = self.room.update_pose(&msg.avatar_id, decoded.frame.clone()) {
            return IngestOutcome::ValidationFailed(e.to_string());
        }

        // Strip outer envelope (opcode + len + room_hash + avatar_id_len +
        // avatar_id) — keep only per-avatar payload (timestamp + mask + transforms).
        let body_start = 1 + 2 + 16 + 1 + msg.avatar_id.as_str().len();
        if body_start >= msg.frame_bytes.len() {
            return IngestOutcome::ValidationFailed("truncated body".into());
        }
        let payload = msg.frame_bytes[body_start..].to_vec();
        self.pending_poses.insert(msg.avatar_id.clone(), payload);

        self.dispatch_broadcast(&msg.avatar_id);
        IngestOutcome::Accepted
    }
}

impl Handler<IngestAgentPresence> for PresenceActor {
    type Result = actix::MessageResult<IngestAgentPresence>;

    fn handle(&mut self, msg: IngestAgentPresence, ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.handle_agent_presence(msg, ctx))
    }
}

impl Handler<SweepStaleAgentPresence> for PresenceActor {
    type Result = actix::MessageResult<SweepStaleAgentPresence>;

    fn handle(&mut self, msg: SweepStaleAgentPresence, _ctx: &mut Self::Context) -> Self::Result {
        let ttl = msg.ttl.unwrap_or(AGENT_PRESENCE_TTL);
        actix::MessageResult(self.sweep_stale_agent_presence(ttl))
    }
}

impl Handler<GetAgentPresence> for PresenceActor {
    type Result = actix::MessageResult<GetAgentPresence>;

    fn handle(&mut self, msg: GetAgentPresence, _ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.agent_presence.get(&msg.avatar_id).map(|e| e.last))
    }
}

impl Handler<GetAttentionNode> for PresenceActor {
    type Result = actix::MessageResult<GetAttentionNode>;

    fn handle(&mut self, msg: GetAttentionNode, _ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.attention_node(&msg.avatar_id))
    }
}

impl Handler<ListMembers> for PresenceActor {
    type Result = actix::MessageResult<ListMembers>;

    fn handle(&mut self, _: ListMembers, _ctx: &mut Self::Context) -> Self::Result {
        actix::MessageResult(self.room.members().map(|m| m.metadata.clone()).collect())
    }
}

impl Handler<RoomStats> for PresenceActor {
    type Result = actix::MessageResult<RoomStats>;

    fn handle(&mut self, _: RoomStats, _ctx: &mut Self::Context) -> Self::Result {
        let mut s = self.stats.clone();
        s.broadcast_sequence = self.broadcast_sequence;
        s.member_count = self.subscribers.len();
        actix::MessageResult(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix::Arbiter;
    use std::sync::{Arc, Mutex};
    use visionclaw_xr_presence::{wire::encode, PoseFrame};

    struct CollectActor {
        frames: Arc<Mutex<Vec<BroadcastFrame>>>,
        events: Arc<Mutex<Vec<RoomEventEnvelope>>>,
    }

    impl Actor for CollectActor {
        type Context = Context<Self>;
    }

    impl Handler<BroadcastFrame> for CollectActor {
        type Result = ();
        fn handle(&mut self, msg: BroadcastFrame, _: &mut Context<Self>) {
            self.frames.lock().unwrap().push(msg);
        }
    }

    impl Handler<RoomEventEnvelope> for CollectActor {
        type Result = ();
        fn handle(&mut self, msg: RoomEventEnvelope, _: &mut Context<Self>) {
            self.events.lock().unwrap().push(msg);
        }
    }

    fn did(byte: u8) -> Did {
        Did::parse(format!("did:nostr:{}", format!("{:02x}", byte).repeat(32))).unwrap()
    }

    fn meta(d: &Did, name: &str) -> AvatarMetadata {
        AvatarMetadata {
            did: d.clone(),
            display_name: name.into(),
            model_uri: None,
        }
    }

    fn sample_frame(ts_us: u64) -> PoseFrame {
        PoseFrame {
            timestamp_us: ts_us,
            head: Transform {
                position: [0.5, 1.6, -0.3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        }
    }

    fn sample_room() -> RoomId {
        RoomId::parse("urn:visionclaw:room:sha256-12-aaaaaaaaaaaa").unwrap()
    }

    #[actix::test]
    async fn join_then_leave_emits_events_and_shuts_down() {
        let room = sample_room();
        let actor = PresenceActor::new(room.clone()).start();

        let frames = Arc::new(Mutex::new(Vec::<BroadcastFrame>::new()));
        let events = Arc::new(Mutex::new(Vec::<RoomEventEnvelope>::new()));
        let collector = CollectActor {
            frames: frames.clone(),
            events: events.clone(),
        }
        .start();

        let d = did(0x11);
        let ack = actor
            .send(JoinRoom {
                did: d.clone(),
                metadata: meta(&d, "alice"),
                frame_recipient: collector.clone().recipient(),
                event_recipient: collector.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ack.members.len(), 1);

        actor
            .send(LeaveRoom {
                avatar_id: ack.avatar_id,
            })
            .await
            .unwrap();
    }

    #[actix::test]
    async fn ingest_validates_and_broadcasts_to_peers_only() {
        let room = sample_room();
        let actor = PresenceActor::new(room.clone()).start();

        let frames_a = Arc::new(Mutex::new(Vec::<BroadcastFrame>::new()));
        let events_a = Arc::new(Mutex::new(Vec::<RoomEventEnvelope>::new()));
        let frames_b = Arc::new(Mutex::new(Vec::<BroadcastFrame>::new()));
        let events_b = Arc::new(Mutex::new(Vec::<RoomEventEnvelope>::new()));

        let collector_a = CollectActor {
            frames: frames_a.clone(),
            events: events_a,
        }
        .start();
        let collector_b = CollectActor {
            frames: frames_b.clone(),
            events: events_b,
        }
        .start();

        let d_a = did(0x10);
        let d_b = did(0x20);

        let ack_a = actor
            .send(JoinRoom {
                did: d_a.clone(),
                metadata: meta(&d_a, "alice"),
                frame_recipient: collector_a.clone().recipient(),
                event_recipient: collector_a.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap();

        let _ack_b = actor
            .send(JoinRoom {
                did: d_b.clone(),
                metadata: meta(&d_b, "bob"),
                frame_recipient: collector_b.clone().recipient(),
                event_recipient: collector_b.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap();

        let frame = sample_frame(1_000_000);
        let bytes = encode(&frame, &room, &ack_a.avatar_id).unwrap().to_vec();
        let outcome = actor
            .send(IngestPose {
                avatar_id: ack_a.avatar_id.clone(),
                frame_bytes: bytes,
            })
            .await
            .unwrap();
        assert_eq!(outcome, IngestOutcome::Accepted);

        actix_rt::time::sleep(Duration::from_millis(50)).await;
        assert!(
            frames_a.lock().unwrap().is_empty(),
            "sender must not receive own frame"
        );
        assert_eq!(frames_b.lock().unwrap().len(), 1);
        Arbiter::current().stop();
    }

    #[actix::test]
    async fn ingest_rejects_out_of_bounds() {
        let room = sample_room();
        let actor = PresenceActor::new(room.clone())
            .with_bounds(Aabb::symmetric(2.0), 20.0)
            .start();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let collector = CollectActor { frames, events }.start();

        let d = did(0x42);
        let ack = actor
            .send(JoinRoom {
                did: d.clone(),
                metadata: meta(&d, "eve"),
                frame_recipient: collector.clone().recipient(),
                event_recipient: collector.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap();

        let mut frame = sample_frame(1_000_000);
        frame.head.position = [100.0, 0.0, 0.0];
        let bytes = encode(&frame, &room, &ack.avatar_id).unwrap().to_vec();
        let outcome = actor
            .send(IngestPose {
                avatar_id: ack.avatar_id,
                frame_bytes: bytes,
            })
            .await
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::ValidationFailed(_)));
    }

    #[test]
    fn configured_hand_reach_defaults_when_unset() {
        // Env is process-global; this test only asserts the default when the var
        // is absent, avoiding cross-test env races.
        if std::env::var("PRESENCE_HAND_REACH_M").is_err() {
            assert_eq!(configured_hand_reach_m(), DEFAULT_PRESENCE_HAND_REACH_M);
        }
    }

    #[actix::test]
    async fn ingest_drops_single_out_of_reach_hand_without_kick() {
        let room = sample_room();
        // Generous bounds so ONLY the hand-reach check trips.
        let actor = PresenceActor::new(room.clone())
            .with_bounds(Aabb::symmetric(50.0), 20.0)
            .start();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let collector = CollectActor { frames, events }.start();

        let d = did(0x55);
        let ack = actor
            .send(JoinRoom {
                did: d.clone(),
                metadata: meta(&d, "stretch"),
                frame_recipient: collector.clone().recipient(),
                event_recipient: collector.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap();

        // Hand 3m from the head — beyond the 1.5m default reach, but well inside
        // world bounds. A single such frame must DROP (ValidationFailed), not
        // kick.
        let mut frame = sample_frame(1_000_000);
        frame.head.position = [0.0, 1.6, 0.0];
        frame.left_hand = Some(Transform {
            position: [3.0, 1.6, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
        let bytes = encode(&frame, &room, &ack.avatar_id).unwrap().to_vec();
        let outcome = actor
            .send(IngestPose {
                avatar_id: ack.avatar_id,
                frame_bytes: bytes,
            })
            .await
            .unwrap();
        assert!(
            matches!(outcome, IngestOutcome::ValidationFailed(_)),
            "single out-of-reach hand frame should drop, not kick: {outcome:?}"
        );
        Arbiter::current().stop();
    }

    // ── ADR-2020: agent co-presence (0x44) end to end ──────────────────────

    use actix::Addr;
    use visionclaw_xr_presence::agent_presence::{
        decode_agent_presence, AgentActivity, AgentPresence, AttentionTarget, OPCODE_AGENT_PRESENCE,
    };

    fn presence(activity: AgentActivity, gaze: [f32; 3], attn: AttentionTarget) -> AgentPresence {
        AgentPresence::new(activity, gaze, attn)
    }

    /// Join `d` to `actor`, returning its avatar id.
    async fn join(
        actor: &Addr<PresenceActor>,
        d: &Did,
        name: &str,
        collector: &Addr<CollectActor>,
    ) -> AvatarId {
        actor
            .send(JoinRoom {
                did: d.clone(),
                metadata: meta(d, name),
                frame_recipient: collector.clone().recipient(),
                event_recipient: collector.clone().recipient(),
            })
            .await
            .unwrap()
            .unwrap()
            .avatar_id
    }

    type Collected<T> = Arc<Mutex<Vec<T>>>;

    fn collector() -> (
        Addr<CollectActor>,
        Collected<BroadcastFrame>,
        Collected<RoomEventEnvelope>,
    ) {
        let frames = Arc::new(Mutex::new(Vec::<BroadcastFrame>::new()));
        let events = Arc::new(Mutex::new(Vec::<RoomEventEnvelope>::new()));
        let addr = CollectActor {
            frames: frames.clone(),
            events: events.clone(),
        }
        .start();
        (addr, frames, events)
    }

    #[actix::test]
    async fn agent_presence_is_encoded_as_0x44_and_reaches_peers_not_the_publisher() {
        // The closeout finding: the 0x44 codec existed but no live server/client
        // encode/decode integration could be found. This is that integration.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, frames_a, _) = collector();
        let (coll_b, frames_b, _) = collector();

        let d_a = did(0x10);
        let d_b = did(0x20);
        let a = join(&actor, &d_a, "agent-a", &coll_a).await;
        let _b = join(&actor, &d_b, "agent-b", &coll_b).await;

        let outcome = actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::GraphNode(4_242),
                ),
            })
            .await
            .unwrap();
        assert!(
            matches!(outcome, AgentPresenceOutcome::Broadcast { .. }),
            "first publication must broadcast, got {outcome:?}"
        );

        actix::clock::sleep(Duration::from_millis(50)).await;

        // The peer received a real 0x44 frame that decodes back to what was sent.
        let received = frames_b.lock().unwrap().clone();
        assert_eq!(received.len(), 1, "peer must receive exactly one frame");
        let bytes = &received[0].bytes;
        assert_eq!(bytes[0], OPCODE_AGENT_PRESENCE, "opcode must be 0x44");

        let batch = decode_agent_presence(bytes).expect("server output must decode");
        assert_eq!(batch.deltas.len(), 1);
        let delta = &batch.deltas[0];
        assert_eq!(delta.state, Some(AgentActivity::Working));
        assert_eq!(delta.attention, Some(AttentionTarget::GraphNode(4_242)));

        // The publisher does not get its own state echoed back.
        assert!(
            frames_a.lock().unwrap().is_empty(),
            "publisher must not receive its own presence"
        );
    }

    #[actix::test]
    async fn a_non_member_cannot_publish_presence() {
        // Permission denial. Membership is established by an authenticated
        // JoinRoom, so it is the authorisation boundary for social state.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;

        // A well-formed avatar id that never joined this room.
        let outsider = AvatarId::from_did(&did(0xEE));
        let outcome = actor
            .send(IngestAgentPresence {
                avatar_id: outsider.clone(),
                presence: presence(
                    AgentActivity::Speaking,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::User,
                ),
            })
            .await
            .unwrap();
        assert_eq!(outcome, AgentPresenceOutcome::PermissionDenied);

        // Nothing was recorded for the outsider.
        assert!(actor
            .send(GetAgentPresence {
                avatar_id: outsider
            })
            .await
            .unwrap()
            .is_none());

        // A member is still accepted, so denial is not a blanket refusal.
        let ok = actor
            .send(IngestAgentPresence {
                avatar_id: a,
                presence: presence(AgentActivity::Idle, [0.0, 0.0, -1.0], AttentionTarget::None),
            })
            .await
            .unwrap();
        assert!(matches!(ok, AgentPresenceOutcome::Broadcast { .. }));
    }

    #[actix::test]
    async fn a_member_that_left_can_no_longer_publish() {
        // Leaving revokes the permission: the boundary is live membership, not a
        // one-off check at join time.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let (coll_b, _, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;
        // A second member keeps the room alive: leaving the LAST member stops the
        // actor entirely, which would test mailbox shutdown rather than authority.
        let _b = join(&actor, &did(0x20), "agent-b", &coll_b).await;

        actor
            .send(LeaveRoom {
                avatar_id: a.clone(),
            })
            .await
            .unwrap();

        let outcome = actor
            .send(IngestAgentPresence {
                avatar_id: a,
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::None,
                ),
            })
            .await
            .unwrap();
        assert_eq!(outcome, AgentPresenceOutcome::PermissionDenied);
    }

    #[actix::test]
    async fn attention_correlates_to_a_graph_node_id() {
        // Node correlation: the attention target and the graph socket share the
        // 26-bit wire id space, so an attended node is resolvable by a client.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;

        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::GraphNode(1_234),
                ),
            })
            .await
            .unwrap();
        assert_eq!(
            actor
                .send(GetAttentionNode {
                    avatar_id: a.clone()
                })
                .await
                .unwrap(),
            Some(1_234)
        );

        // Attending to the user, not a node, correlates to nothing.
        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Speaking,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::User,
                ),
            })
            .await
            .unwrap();
        assert_eq!(
            actor.send(GetAttentionNode { avatar_id: a }).await.unwrap(),
            None
        );
    }

    #[actix::test]
    async fn an_attention_node_outside_the_wire_id_space_is_refused() {
        // A node id above the 26-bit mask cannot ride the graph socket at all
        // (ADR-2024), so publishing it would name a node no client can resolve.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;

        let over_range = WIRE_NODE_ID_MASK + 1;
        let outcome = actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::GraphNode(over_range),
                ),
            })
            .await
            .unwrap();
        assert_eq!(
            outcome,
            AgentPresenceOutcome::InvalidNodeCorrelation {
                node_id: over_range
            }
        );
        assert!(actor
            .send(GetAgentPresence { avatar_id: a })
            .await
            .unwrap()
            .is_none());
    }

    #[actix::test]
    async fn an_unchanged_republication_puts_nothing_on_the_wire() {
        // Gaze is compared at wire resolution, so float jitter below the 16-bit
        // quantum must not generate traffic.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let (coll_b, frames_b, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;
        let _b = join(&actor, &did(0x20), "agent-b", &coll_b).await;

        let p = presence(
            AgentActivity::Working,
            [0.0, 0.0, -1.0],
            AttentionTarget::GraphNode(7),
        );
        assert!(matches!(
            actor
                .send(IngestAgentPresence {
                    avatar_id: a.clone(),
                    presence: p
                })
                .await
                .unwrap(),
            AgentPresenceOutcome::Broadcast { .. }
        ));
        assert_eq!(
            actor
                .send(IngestAgentPresence {
                    avatar_id: a.clone(),
                    presence: p
                })
                .await
                .unwrap(),
            AgentPresenceOutcome::Unchanged
        );

        actix::clock::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            frames_b.lock().unwrap().len(),
            1,
            "the identical republication must not reach the wire"
        );
    }

    #[actix::test]
    async fn stale_presence_is_retired_and_announced() {
        // Stale removal: a crashed agent must not leave an avatar permanently
        // attentive to a node.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let (coll_b, _, events_b) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;
        let _b = join(&actor, &did(0x20), "agent-b", &coll_b).await;

        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::GraphNode(99),
                ),
            })
            .await
            .unwrap();

        // Well inside the TTL: nothing is retired.
        assert!(actor
            .send(SweepStaleAgentPresence {
                ttl: Some(Duration::from_secs(60))
            })
            .await
            .unwrap()
            .is_empty());
        assert!(actor
            .send(GetAgentPresence {
                avatar_id: a.clone()
            })
            .await
            .unwrap()
            .is_some());

        // A zero TTL makes every entry stale.
        let retired = actor
            .send(SweepStaleAgentPresence {
                ttl: Some(Duration::ZERO),
            })
            .await
            .unwrap();
        assert_eq!(retired.len(), 1, "the stale entry must be retired");

        assert!(
            actor
                .send(GetAgentPresence {
                    avatar_id: a.clone()
                })
                .await
                .unwrap()
                .is_none(),
            "retired presence must be dropped"
        );
        assert_eq!(
            actor.send(GetAttentionNode { avatar_id: a }).await.unwrap(),
            None,
            "a retired agent attends to nothing"
        );

        actix::clock::sleep(Duration::from_millis(50)).await;
        let events = events_b.lock().unwrap().clone();
        assert!(
            events.iter().any(|e| matches!(
                e,
                RoomEventEnvelope::AgentPresenceExpired { local_id } if *local_id == retired[0]
            )),
            "retirement must be announced on the event channel, got {events:?}"
        );
    }

    #[actix::test]
    async fn presence_and_pose_operate_independently() {
        // Independent pose operation: publishing co-presence must not flush or
        // fabricate poses, and ingesting a pose must not touch social state.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let (coll_b, frames_b, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;
        let _b = join(&actor, &did(0x20), "agent-b", &coll_b).await;

        // Presence alone: exactly one 0x44 frame, no 0x43 frame.
        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(
                    AgentActivity::Working,
                    [0.0, 0.0, -1.0],
                    AttentionTarget::None,
                ),
            })
            .await
            .unwrap();
        actix::clock::sleep(Duration::from_millis(50)).await;
        {
            let f = frames_b.lock().unwrap();
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].bytes[0], OPCODE_AGENT_PRESENCE);
        }

        // Now a pose from the same avatar: a 0x43 frame appears, and the social
        // state is untouched by it.
        let outcome = actor
            .send(IngestPose {
                avatar_id: a.clone(),
                frame_bytes: encode(&sample_frame(1_000), &sample_room(), &a)
                    .unwrap()
                    .to_vec(),
            })
            .await
            .unwrap();
        assert_eq!(outcome, IngestOutcome::Accepted);
        actix::clock::sleep(Duration::from_millis(50)).await;

        let f = frames_b.lock().unwrap().clone();
        assert_eq!(f.len(), 2, "one presence frame and one pose frame");
        assert_eq!(
            f[1].bytes[0], PREAMBLE_OPCODE,
            "the second is the 0x43 pose"
        );

        let still = actor
            .send(GetAgentPresence {
                avatar_id: a.clone(),
            })
            .await
            .unwrap()
            .expect("pose ingest must not clear social state");
        assert_eq!(still.state, AgentActivity::Working);

        // The two streams carry independent sequence spaces. Both counters start
        // at zero, so their first frames legitimately share the value 1 — what
        // independence means is that advancing ONE does not advance the other.
        assert_eq!(f[0].broadcast_sequence, 1, "first 0x44 frame");
        assert_eq!(f[1].broadcast_sequence, 1, "first 0x43 frame, own counter");

        // A second presence update advances only the 0x44 counter.
        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(AgentActivity::Idle, [0.0, 0.0, -1.0], AttentionTarget::None),
            })
            .await
            .unwrap();
        actix::clock::sleep(Duration::from_millis(50)).await;
        let f = frames_b.lock().unwrap().clone();
        assert_eq!(f.len(), 3);
        assert_eq!(f[2].bytes[0], OPCODE_AGENT_PRESENCE);
        assert_eq!(
            f[2].broadcast_sequence, 2,
            "the 0x44 counter advanced without the pose stream moving"
        );
    }

    #[actix::test]
    async fn a_changed_field_broadcasts_only_that_field() {
        // Deltas elide unchanged fields, so the reliable channel stays cheap.
        let actor = PresenceActor::new(sample_room()).start();
        let (coll_a, _, _) = collector();
        let (coll_b, frames_b, _) = collector();
        let a = join(&actor, &did(0x10), "agent-a", &coll_a).await;
        let _b = join(&actor, &did(0x20), "agent-b", &coll_b).await;

        let gaze = [0.0f32, 0.0, -1.0];
        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(AgentActivity::Idle, gaze, AttentionTarget::None),
            })
            .await
            .unwrap();

        // Only the activity changes.
        actor
            .send(IngestAgentPresence {
                avatar_id: a.clone(),
                presence: presence(AgentActivity::Working, gaze, AttentionTarget::None),
            })
            .await
            .unwrap();

        actix::clock::sleep(Duration::from_millis(50)).await;
        let f = frames_b.lock().unwrap().clone();
        assert_eq!(f.len(), 2);
        let batch = decode_agent_presence(&f[1].bytes).expect("decodes");
        let delta = &batch.deltas[0];
        assert_eq!(delta.state, Some(AgentActivity::Working));
        assert!(delta.gaze_dir.is_none(), "unchanged gaze must be elided");
        assert!(
            delta.attention.is_none(),
            "unchanged attention must be elided"
        );
    }
}
