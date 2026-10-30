---
id: ADR-2020
title: Agent co-presence is an additive sibling opcode, not an extension of the pose frame
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: e0f8cd896
owner: jjohare
review_trigger: a new social/co-presence field, or pressure to pack social state into the 0x43 pose transform_mask; activates when a running server and XR client exchange 0x44 social frames end to end (node correlation, stale removal, permission denial, pose independent of 0x43)
repo: visionclaw
domain: PROTOCOL-registry
lineage: "Distils ADR-102 (server-initiated presence handshake) + ADR-130 D4 (copresence AgentAvatar/Gaze/Proxemics layer)."
---

# ADR-2020 — Agent co-presence is an additive sibling opcode, not an extension of the pose frame

## Context

The avatar-pose frame (opcode `0x43`) carries skeletal transforms behind a
`transform_mask`. Co-presence work (ADR-130 D4: AgentAvatar/Gaze/Proxemics)
adds social state — activity, gaze, attention — that changes on a different
cadence from skeleton pose and for a different population (agents, not just
human avatars). Packing it into the existing `0x43` mask would couple two
independently-evolving concerns into one codec and one mask budget, so every
social-schema change would risk the pose parser (ADR-102 handshake path).

## Decision

Social/co-presence state rides a **separate additive opcode `0x44`** with its
own mask-gated codec, declared the additive sibling of `0x43`. Skeleton pose
and social state evolve independently: a change to gaze/attention encoding
touches only the `0x44` codec and its field mask, never the `0x43` pose frame,
and vice versa. This forecloses overloading the pose `transform_mask` with
non-skeletal state and forecloses one codec's schema churn destabilising the
other.

## Consequences

- The two concerns version independently; the pose parser is insulated from
  social-schema evolution and each keeps its own field mask budget.
- Once wired, a client tracking a full avatar will consume two frames (pose +
  presence) and must correlate them by node id — the deliberate cost of
  decoupling.
- `0x44` is a registry allocation on the presence socket (per ADR-2019),
  disjoint from the graph tag space; adding further social opcodes follows the
  same additive-sibling pattern rather than mask extension.

## Verification

At e0f8cd896: `crates/visionclaw-xr-presence/src/agent_presence.rs` defines
`pub const OPCODE_AGENT_PRESENCE: u8 = 0x44`, documented "Additive sibling of
0x43 (`wire::OPCODE_AVATAR_POSE`)", with a per-field mask codec and an
opcode-guarded decode (`if bytes[0] != OPCODE_AGENT_PRESENCE { ... }`). The
codec is implemented and fuzzed (`encode_agent_presence`/`decode_agent_presence`
covered by unit, property, and fuzz tests) but **not yet wired into any live
path**: `src/handlers/presence_handler.rs` and `src/actors/presence_actor.rs`
decode only `0x43` (`wire::decode`, opcode-checked), and
`xr-client/tests/unit/test_graph_agents.gd` names "the live 0x44
agent-presence wire" a pending integration point. Hence `activation_status:
staged`, not live. Sibling pose opcode `OPCODE_AVATAR_POSE = 0x43` lives in
`crates/visionclaw-xr-presence/src/wire.rs`.

## Closeout extension — 2026-09-04

Work package: **CP-06**. Owner remains `jjohare`, with protocol, identity and XR maintainers responsible for their respective boundaries.

The 0x44 codec exists, but no live server/client encode/decode integration was found in this source pass. Staged activation is retained.

**Acceptance condition:** Wire authenticated social state end to end and demonstrate node correlation, stale removal, permission denial and independent pose operation before declaring activation live.

Dependencies: CP-01 release identity and CP-04 authority where authenticated actions cross the wire. Reopen on the existing review trigger, a changed opcode or a failing freshness/visibility probe. Existing verification and activation fields retain their historical scope; this annex records source/local tests at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`, not a new live certification.

See [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-render-snapshot.json).

## Acceptance progress — 2026-09-05

**The missing integration is implemented.** The closeout found the `0x44` codec
present on both sides with no live encode/decode integration anywhere; that is
now wired end to end.

*Server* (`src/actors/presence_actor.rs`) — co-presence is a first-class channel
alongside `0x43` pose:

- `IngestAgentPresence { avatar_id, presence }` publishes an agent's social state.
  `AgentPresenceOutcome` reports `Broadcast { changed_fields }`, `Unchanged`,
  `PermissionDenied` or `InvalidNodeCorrelation { node_id }`.
- **Permission denial** — room membership is the authorisation boundary.
  Membership is established by an authenticated `JoinRoom`, so a caller that never
  joined, *or one that has since left*, is refused. Denial is a live-membership
  check, not a one-off test at join time.
- **Node correlation** — `AttentionTarget::GraphNode(id)` shares the 26-bit wire
  id space with the graph socket (ADR-2024). `GetAttentionNode` resolves an
  avatar to the node it attends; an id above the mask is refused as
  `InvalidNodeCorrelation`, because it could never be resolved by any client.
- **Stale removal** — `SweepStaleAgentPresence` retires entries not refreshed
  within `AGENT_PRESENCE_TTL` (10 s), so a crashed agent cannot leave an avatar
  permanently attentive to a node. Retirement is announced as
  `RoomEventEnvelope::AgentPresenceExpired` on the JSON event channel rather than
  as a `0x44` delta: the codec encodes *state* and has no "gone" representation,
  and reusing an idle delta would be ambiguous with an agent that genuinely went
  idle.
- **Independent pose operation** — co-presence keeps its own state map and its own
  broadcast sequence counter. Publishing presence never flushes pending poses;
  ingesting a pose never touches social state. Deltas elide unchanged fields, and
  gaze is compared at wire resolution so sub-quantum jitter generates no traffic.

*Client* (`xr-client/rust/src/avatar_state.rs`) — `RemotePresenceStore` consumes
the stream: it holds last-known state per transport `local_id` and folds deltas
onto it (which is what makes elision safe), refuses out-of-order or duplicate
batches whole, exposes `attention_node()` as the client half of node correlation,
declines sibling opcodes so a demultiplexer can offer it every binary frame, and
supports `remove()` for the server's retirement announcement.

**Tests run.**

- `cargo test --lib --no-default-features presence_actor` — 14 pass, of which 9
  are new: `0x44` opcode on the wire and decodable by the real decoder, publisher
  excluded from its own broadcast, non-member denied, departed member denied,
  attention/node correlation both ways, over-range node id refused, unchanged
  republication silent, stale retirement plus its event, presence/pose
  independence with separate sequence counters, and per-field delta elision.
- `xr-client/rust`: `cargo test --test agent_copresence_roundtrip` — 8 pass, all
  driven by the **real** `encode_agent_presence`, covering full-delta
  reconstruction, elided-field folding, out-of-order refusal, multi-agent
  tracking, stale removal, sibling-opcode refusal, exhaustive truncation and
  mid-stream convergence.

**Governed paths changed.** `src/actors/presence_actor.rs`,
`xr-client/rust/src/avatar_state.rs`,
`xr-client/rust/tests/agent_copresence_roundtrip.rs`.

**Open — staged activation is retained.** The integration is proven at the actor
and codec boundary, not over a live `/ws/presence` socket: no HTTP/WS handler
route publishes `IngestAgentPresence` from a real session yet, and no headset
rendered a remote agent's avatar from a received frame. `implementation_status`
should not move to live until a real authenticated session drives the path and a
client renders the result.
