//! `AgentBeamActor` — the server half of the agent-embodiment render
//! (ADR-059 §4, Phase 2b).
//!
//! Closes the last broadcast gap in the embodiment loop: the `/wss/agent-events`
//! ingest already parses, validates, and publishes every inbound
//! [`AgentActionEnvelope`] to the process-global [`agent_events::hub`], and the
//! browser already fully decodes the `0x23 AGENT_ACTION` binary frame (with its
//! colour map) — but **nothing was broadcasting that frame**. This actor is the
//! missing link: it subscribes to the hub, projects each envelope onto the
//! identity-blind `0x23` action via
//! [`AgentActionEnvelope::to_binary_event`], coalesces a burst into one
//! multi-action frame via
//! [`encode_agent_actions`](crate::utils::binary_protocol::encode_agent_actions)
//! (decoded browser-side by `decodeAgentActions`), and hands it to
//! [`ClientCoordinatorActor`] for fan-out to every connected client (reusing the
//! established `BroadcastNodePositions` dispatch loop via the
//! [`BroadcastAgentActionFrame`] message).
//!
//! ## Transport path
//!
//! The idiomatic actix `BroadcastStream` + `ctx.add_stream` route is unavailable
//! here: `BroadcastStream` is gated behind `tokio-stream`'s `sync` feature, which
//! is not enabled in this workspace (and the brief forbids adding a dependency
//! just for the beam). So `started` spawns a single lightweight tokio task that
//! loops on the hub receiver, coalesces each burst of envelopes into ONE
//! multi-action `0x23` frame ([`BeamCoalescer`] →
//! [`encode_agent_actions`](crate::utils::binary_protocol::encode_agent_actions)),
//! and forwards it to the coordinator with `try_send`.
//!
//! ## Backpressure (bounded coalescing)
//!
//! The coordinator mailbox is bounded, so at a high action rate a per-action
//! `try_send` floods it (`SendError::Full`), which previously dropped the beam
//! silently and spammed one error per rejected action. The coalescing task
//! instead: (a) absorbs an in-flight burst via non-blocking `try_recv` and sends
//! it as a single frame; (b) on a full mailbox HOLDS the backlog so it coalesces
//! into the next burst (retried on a short timer so the tail is not stranded);
//! and (c) only past [`MAX_PENDING_ACTIONS`] drops the OLDEST action — a
//! duration-based / last-write-wins beam values recency (ADR-059 open-question 3)
//! — counting the drop and surfacing it in a single warn rate-limited to
//! [`BACKPRESSURE_WARN_INTERVAL`]. Hub-side `broadcast` lag is still handled with
//! drop-oldest resync on `RecvError::Lagged`.
//!
//! ## ID-space contract (LANE A DECISION — Lane B must match)
//!
//! The `0x23` frame carries `source_agent_id` (agent id-space) and
//! `target_node_id` (KG id-space); the client distinguishes agent nodes by the
//! `AGENT_NODE_FLAG = 0x80000000` high bit on the id. **This actor sets that flag
//! on `source_agent_id` immediately before encode** (see [`stamp_agent_flag`]),
//! unconditionally and idempotently, so the wire frame is always
//! client-resolvable regardless of whether the upstream agentbox envelope
//! pre-flagged the id. Rationale: the binary `0x23` frame is, by design, the
//! identity-blind GPU projection (`schema.rs` docstring) and this actor is the
//! single authoritative producer of that frame on the server; centralising the
//! flag-stamp here guarantees one consistent rule rather than depending on every
//! upstream producer to remember it. The KG `target_node_id` is passed through
//! untouched — it is a plain KG-space id with no flag.

use std::time::{Duration, Instant};

use actix::prelude::*;
use log::{debug, error, info, warn};
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use crate::actors::messages::BroadcastAgentActionFrame;
use crate::actors::ClientCoordinatorActor;
use crate::agent_events;
use crate::utils::binary_protocol::{encode_agent_actions, AgentActionEvent};

/// Agent-node high-bit flag (mirrors `binary_protocol::AGENT_NODE_FLAG`). The
/// client resolves the source node of a beam as an *agent* node iff this bit is
/// set on `source_agent_id`. Kept as a local `const` rather than re-exporting the
/// private `binary_protocol` constant to avoid widening that module's surface.
const AGENT_NODE_FLAG: u32 = 0x80000000;

/// Hard cap on buffered actions. Past this the OLDEST action is evicted
/// (`dropped_total` counts it) — a duration-based, last-write-wins beam values
/// recency over completeness. Sized so one coalesced `0x23` frame
/// (≈17 bytes/action + 3-byte header) stays far under
/// `binary_protocol::MAX_PAYLOAD_SIZE`.
const MAX_PENDING_ACTIONS: usize = 256;

/// Upper bound on how many already-queued events one loop iteration absorbs
/// before flushing — collapses a burst into a single frame without indefinitely
/// starving the dispatch.
const MAX_COALESCE_PER_FLUSH: usize = 256;

/// While a backlog is held (the coordinator mailbox was full) retry the flush at
/// least this often even if no new action arrives, so a burst's tail is not
/// stranded until the next unrelated action.
const FLUSH_RETRY_INTERVAL: Duration = Duration::from_millis(20);

/// Rate limit for the mailbox-full warn: at most one line per window, so
/// sustained backpressure yields a single heartbeat (with the running drop
/// count) rather than a per-action flood.
const BACKPRESSURE_WARN_INTERVAL: Duration = Duration::from_secs(10);

/// Stamp the agent-node flag onto an agent id-space identifier (idempotent).
#[inline]
fn stamp_agent_flag(source_agent_id: u32) -> u32 {
    source_agent_id | AGENT_NODE_FLAG
}

/// Bounded, coalescing buffer of projected `0x23` actions. Pure and actor-free so
/// the drop/coalesce policy is unit-testable in isolation (the
/// `ingest::process_frame` testability precedent).
///
/// Policy (defect: beam fan-out backpressure):
///   * [`push`](BeamCoalescer::push) appends one flag-stamped [`AgentActionEvent`].
///     Past [`MAX_PENDING_ACTIONS`] the OLDEST is evicted and counted in
///     `dropped_total` — never a silent drop-and-spam.
///   * [`encode_pending`](BeamCoalescer::encode_pending) projects the WHOLE
///     backlog onto one multi-action `0x23` frame via [`encode_agent_actions`]
///     (the browser decodes it with `decodeAgentActions`, which already yields an
///     `AgentActionEvent[]`). It is non-consuming: the caller clears only after a
///     confirmed dispatch, so a frame rejected by a full mailbox stays buffered
///     and coalesces with the next burst instead of being lost.
#[derive(Default)]
pub(crate) struct BeamCoalescer {
    pending: Vec<AgentActionEvent>,
    dropped_total: u64,
}

impl BeamCoalescer {
    fn new() -> Self {
        Self::default()
    }

    /// Buffer one already-projected (flag-stamped) action, evicting the oldest
    /// once the cap is reached. Returns `true` iff an eviction occurred.
    fn push(&mut self, event: AgentActionEvent) -> bool {
        let evicted = if self.pending.len() >= MAX_PENDING_ACTIONS {
            // Drop-oldest: recency wins for a duration-based beam.
            self.pending.remove(0);
            self.dropped_total = self.dropped_total.saturating_add(1);
            true
        } else {
            false
        };
        self.pending.push(event);
        evicted
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn len(&self) -> usize {
        self.pending.len()
    }

    fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// Encode the entire backlog as one multi-action `0x23` frame. Non-consuming;
    /// returns `None` when the backlog is empty (nothing to dispatch).
    fn encode_pending(&self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(encode_agent_actions(&self.pending))
        }
    }

    /// Clear the backlog after a confirmed dispatch.
    fn clear(&mut self) {
        self.pending.clear();
    }
}

/// Project an envelope onto the identity-blind `0x23` action and stamp the
/// agent-node flag so the client resolves the beam's source as an agent node
/// (ID-space contract, see module doc). Pure → unit-testable.
fn project_action(envelope: &agent_events::schema::AgentActionEnvelope) -> AgentActionEvent {
    let mut event = envelope.to_binary_event();
    event.source_agent_id = stamp_agent_flag(event.source_agent_id);
    event
}

/// Subscribes to the agent-action hub and broadcasts encoded `0x23` frames.
pub struct AgentBeamActor {
    /// Fan-out target. The coordinator owns the client registry and the binary
    /// `send_binary` dispatch loop; this actor never touches client state.
    coordinator: Addr<ClientCoordinatorActor>,
}

impl AgentBeamActor {
    pub fn new(coordinator: Addr<ClientCoordinatorActor>) -> Self {
        Self { coordinator }
    }
}

impl Actor for AgentBeamActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        let coordinator = self.coordinator.clone();
        let mut rx = agent_events::hub::subscribe();

        // Single forwarding task with BOUNDED COALESCING backpressure.
        //
        // Previously each envelope fired one `try_send` at the coordinator; at
        // ~12–15 actions/sec the bounded coordinator mailbox rejected them
        // (`SendError::Full`), which silently dropped the beam and spammed one
        // error line per rejected action while the KPI hub tap kept counting.
        //
        // Now a burst is absorbed into a `BeamCoalescer` (draining any already-
        // queued events without awaiting) and flushed as ONE multi-action `0x23`
        // frame. A full mailbox does NOT drop-and-spam: the backlog is HELD and
        // coalesces into the next burst, retried on a short timer so the tail of a
        // burst is not stranded. Drop-oldest past `MAX_PENDING_ACTIONS` is the
        // hard-overflow floor, surfaced via a single rate-limited warn carrying
        // the running drop count. The task ends when the hub sender is dropped
        // (process shutdown) or the coordinator dies.
        actix::spawn(async move {
            let mut coalescer = BeamCoalescer::new();
            let mut last_warn: Option<Instant> = None;
            let mut hub_closed = false;

            loop {
                // (1) Acquire at least one action. With an empty backlog block
                // indefinitely; with a held backlog (mailbox was full) bound the
                // wait so the held frame is retried rather than stranded.
                if coalescer.is_empty() {
                    match rx.recv().await {
                        Ok(env) => {
                            coalescer.push(project_action(&env));
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            warn!(
                                "AgentBeamActor: hub lagged, {skipped} frame(s) skipped — resyncing"
                            );
                            continue;
                        }
                        Err(RecvError::Closed) => {
                            info!("AgentBeamActor: hub closed — forwarding task exiting");
                            break;
                        }
                    }
                } else {
                    match tokio::time::timeout(FLUSH_RETRY_INTERVAL, rx.recv()).await {
                        Ok(Ok(env)) => {
                            coalescer.push(project_action(&env));
                        }
                        Ok(Err(RecvError::Lagged(skipped))) => {
                            warn!(
                                "AgentBeamActor: hub lagged, {skipped} frame(s) skipped — resyncing"
                            );
                        }
                        Ok(Err(RecvError::Closed)) => {
                            hub_closed = true;
                        }
                        Err(_elapsed) => {
                            // No new action; fall through to retry the held backlog
                            // against the (hopefully drained) coordinator mailbox.
                        }
                    }
                }

                // (2) Absorb any already-queued burst without awaiting, so a spike
                // collapses into a single coalesced frame rather than N messages.
                while coalescer.len() < MAX_COALESCE_PER_FLUSH {
                    match rx.try_recv() {
                        Ok(env) => {
                            coalescer.push(project_action(&env));
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Lagged(skipped)) => {
                            warn!(
                                "AgentBeamActor: hub lagged, {skipped} frame(s) skipped — resyncing"
                            );
                        }
                        Err(TryRecvError::Closed) => {
                            hub_closed = true;
                            break;
                        }
                    }
                }

                // (3) Dispatch the whole backlog as ONE 0x23 frame. Clear only on
                // success; on a full mailbox keep it buffered to coalesce forward.
                if let Some(frame) = coalescer.encode_pending() {
                    let pending = coalescer.len();
                    match coordinator.try_send(BroadcastAgentActionFrame(frame)) {
                        Ok(()) => {
                            debug!(
                                "AgentBeamActor: dispatched coalesced 0x23 frame ({pending} action(s))"
                            );
                            coalescer.clear();

                            // GLUON (attractive force) — DEFERRED. See `gluon_deferral_note`.
                        }
                        Err(actix::prelude::SendError::Full(_)) => {
                            let now = Instant::now();
                            let due = last_warn.is_none_or(|t| {
                                now.duration_since(t) >= BACKPRESSURE_WARN_INTERVAL
                            });
                            if due {
                                warn!(
                                    "AgentBeamActor: coordinator mailbox full — holding \
                                     {pending} action(s) to coalesce into the next frame \
                                     (dropped {} over capacity so far)",
                                    coalescer.dropped_total()
                                );
                                last_warn = Some(now);
                            }
                        }
                        Err(actix::prelude::SendError::Closed(_)) => {
                            error!("AgentBeamActor: coordinator closed — forwarding task exiting");
                            break;
                        }
                    }
                }

                if hub_closed {
                    info!("AgentBeamActor: hub closed — forwarding task exiting");
                    break;
                }
            }
        });

        info!("AgentBeamActor: started — subscribed to agent-events hub (ADR-059 Phase 2b)");
    }
}

/// GLUON (attractive force) — DEFERRED (ADR-059 §4, gluon sub-feature).
///
/// This is a documentation anchor, not dead production code. The intended visual
/// is a transient attractive edge tugging the agent node toward `target_node_id`
/// for `duration_ms`, which the existing spring kernel would naturally turn into
/// attraction. The mechanism that *would* implement it: inject a CSR edge
/// (weight > 0) between the two ids, TTL = `envelope.duration_ms`, keyed off the
/// beam, then auto-remove.
///
/// Deferred — NOT low-risk on the current GPU substrate:
///   1. GPU edges live in a PACKED CSR layout (`row_offsets` / `col_indices` /
///      `edge_weights`), uploaded wholesale by
///      `unified_gpu_compute::memory::initialize_graph` / `upload_edges_csr`.
///      There is no incremental edge-insert path — a single transient edge forces
///      a `resize_buffers` reallocation and a full re-upload of all three CSR
///      arrays.
///   2. `AddEdge` / `RemoveEdge` (`graph_state_actor.rs`) only mutate the
///      in-memory `node_map`; they do NOT propagate to the GPU until a full
///      `BuildGraphFromMetadata`-style rebuild. No clean per-edge GPU mutation
///      message exists today.
///   3. The SSSP and Louvain/community kernels read the SAME CSR buffers; a
///      mid-flight resize/re-upload would race concurrent kernels and destabilise
///      the simulation.
///   4. The stale ADR `class_charge` modulation buffer does NOT exist — only
///      `class_id` / `class_charge` / `class_mass` per-node device buffers passed to
///      `force_pass_kernel` by `UnifiedGPUCompute`, none of which is a per-edge
///      attractive force. (The `PhysicsGpuBuffers` struct this note used to cite was
///      removed with the `physics-v2` feature — ADR-2055.)
///
/// Correct fix (future increment): add an incremental
/// `UpsertTransientEdge { src, tgt, weight, ttl_ms }` GPU message that appends
/// into a SEPARATE transient-edge buffer the spring kernel sums alongside the
/// static CSR, plus a TTL sweep that zeroes expired entries — avoiding any
/// reallocation of the static CSR. Left out here so the beam broadcast lands as a
/// correct, self-contained increment (correctness over completeness). The beam
/// alone already embodies the action visually.
#[allow(dead_code)]
#[inline]
fn gluon_deferral_note() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_events::schema::AgentActionEnvelope;

    fn envelope(source_agent_id: u32, target_node_id: u32) -> AgentActionEnvelope {
        AgentActionEnvelope {
            version: 3,
            id: 1,
            source_agent_id,
            target_node_id,
            action_type: 1,
            action_type_name: "update".to_string(),
            timestamp: 1_748_500_000_000,
            duration_ms: 250,
            source_urn: None,
            target_urn: None,
            pubkey: None,
            token_count: None,
            handoff_id: None,
            verification: None,
            intent: None,
            metadata: serde_json::json!({ "note": "x" }),
        }
    }

    #[test]
    fn agent_flag_is_set_and_idempotent() {
        let flagged = stamp_agent_flag(7);
        assert_eq!(flagged, 0x80000007, "high bit set, low bits preserved");
        assert_eq!(
            flagged & AGENT_NODE_FLAG,
            AGENT_NODE_FLAG,
            "flag bit present"
        );
        assert_eq!(
            stamp_agent_flag(flagged),
            flagged,
            "stamping an already-flagged id is a no-op"
        );
    }

    #[test]
    fn flag_preserves_full_low_bits() {
        let raw: u32 = 0x7FFF_FFFF;
        assert_eq!(stamp_agent_flag(raw), 0xFFFF_FFFF);
        // Client recovers the original id by clearing the flag bit.
        assert_eq!(stamp_agent_flag(raw) & !AGENT_NODE_FLAG, raw);
    }

    #[test]
    fn project_action_stamps_agent_flag_and_passes_target_through() {
        let event = project_action(&envelope(7, 4242));
        assert_eq!(
            event.source_agent_id, 0x80000007,
            "source id is flagged as an agent node"
        );
        assert_eq!(
            event.source_agent_id & !AGENT_NODE_FLAG,
            7,
            "underlying agent id preserved"
        );
        assert_eq!(event.target_node_id, 4242, "KG target id passes through");
        assert_eq!(
            event.target_node_id & AGENT_NODE_FLAG,
            0,
            "target carries no agent flag"
        );
    }

    #[test]
    fn single_action_encodes_a_decodable_0x23_batch_of_one() {
        use crate::utils::binary_protocol::decode_agent_actions;

        let mut coalescer = BeamCoalescer::new();
        assert!(
            !coalescer.push(project_action(&envelope(7, 4242))),
            "no eviction under cap"
        );
        assert_eq!(coalescer.len(), 1);

        let frame = coalescer
            .encode_pending()
            .expect("one pending ⇒ Some frame");
        // The wire frame leads with the MessageType::AgentAction tag (0x23); the
        // browser's decodeAgentActions consumes the batch body after that byte.
        assert_eq!(frame[0], 0x23, "frame must lead with the AGENT_ACTION tag");

        let decoded = decode_agent_actions(&frame[1..]).expect("batch decodes");
        assert_eq!(decoded.len(), 1, "count=1 round-trips");
        assert_eq!(
            decoded[0].source_agent_id, 0x80000007,
            "flag survives the batch round-trip"
        );
        assert_eq!(decoded[0].target_node_id, 4242);
    }

    #[test]
    fn coalesces_a_burst_into_one_ordered_frame() {
        use crate::utils::binary_protocol::decode_agent_actions;

        let mut coalescer = BeamCoalescer::new();
        // A burst of three distinct actions collapses into ONE frame.
        for (src, tgt) in [(1u32, 10u32), (2, 20), (3, 30)] {
            coalescer.push(project_action(&envelope(src, tgt)));
        }
        assert_eq!(coalescer.len(), 3);

        let frame = coalescer.encode_pending().expect("Some frame");
        let decoded = decode_agent_actions(&frame[1..]).expect("batch decodes");

        assert_eq!(decoded.len(), 3, "all three actions ride one 0x23 frame");
        // Chronological wire order is preserved (client reverses for newest-first).
        assert_eq!(decoded[0].source_agent_id & !AGENT_NODE_FLAG, 1);
        assert_eq!(decoded[1].source_agent_id & !AGENT_NODE_FLAG, 2);
        assert_eq!(decoded[2].source_agent_id & !AGENT_NODE_FLAG, 3);
        assert_eq!(decoded[2].target_node_id, 30);
    }

    #[test]
    fn drops_oldest_past_capacity_and_counts_it() {
        use crate::utils::binary_protocol::decode_agent_actions;

        let mut coalescer = BeamCoalescer::new();
        // Push two beyond the cap with monotonically increasing source ids so we
        // can prove WHICH actions were evicted (the two oldest).
        let total = MAX_PENDING_ACTIONS + 2;
        let mut evictions = 0u64;
        for i in 0..total as u32 {
            if coalescer.push(project_action(&envelope(i, i))) {
                evictions += 1;
            }
        }

        assert_eq!(coalescer.len(), MAX_PENDING_ACTIONS, "backlog is capped");
        assert_eq!(evictions, 2, "two pushes reported an eviction");
        assert_eq!(
            coalescer.dropped_total(),
            2,
            "drop counter matches evictions"
        );

        // Drop-OLDEST: ids 0 and 1 are gone; the surviving front is id 2.
        let frame = coalescer.encode_pending().expect("Some frame");
        let decoded = decode_agent_actions(&frame[1..]).expect("batch decodes");
        assert_eq!(decoded.len(), MAX_PENDING_ACTIONS);
        assert_eq!(
            decoded[0].source_agent_id & !AGENT_NODE_FLAG,
            2,
            "oldest two (0,1) were dropped, id 2 is now the front"
        );
        assert_eq!(
            decoded.last().unwrap().source_agent_id & !AGENT_NODE_FLAG,
            total as u32 - 1,
            "newest action is retained at the tail"
        );
    }

    #[test]
    fn empty_backlog_encodes_to_none_and_clear_empties() {
        let mut coalescer = BeamCoalescer::new();
        assert!(coalescer.is_empty());
        assert!(
            coalescer.encode_pending().is_none(),
            "nothing pending ⇒ no frame"
        );

        coalescer.push(project_action(&envelope(9, 99)));
        assert!(!coalescer.is_empty());
        coalescer.clear();
        assert!(
            coalescer.is_empty(),
            "clear drains the backlog after dispatch"
        );
        assert!(coalescer.encode_pending().is_none());
    }
}
