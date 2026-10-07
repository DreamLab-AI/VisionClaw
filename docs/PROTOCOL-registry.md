---
title: Protocol Registry — Wire Frames, Endpoints & Version Policy
doc_id: VC-PROTOCOL
version: 0.1.4
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.1.4 (2026-10-07): ADR-2133 — /api/memory-cloud{,/vectors,/query,/health} rows; memory cloud wire types; PowerUser gate on private memory reads"
  - "0.1.3 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.1.2: 2026-09-05 remediation — ADR-2057 compile-time 52-byte and V5-seq locks (assertion language corrected); ADR-2058 header-only WS auth (?token= divergence resolved); ADR-2060 citation corrections after line drift (V5 :513→:592, 0x23 :1354/:1125-1135/:1501→:1721/:1490/:1500, 52B asserts :712,809→compile-time :93 plus tests :1077,1174); TS client V5/V2 parity recorded as open"
  - "0.1.1: correct RBAC_PUBLIC_READS citation (rbac_gate.rs/compose, not main.rs — code fails closed); reword WIRE_V3_ITEM_SIZE asserts as unit-test (not static) assertions; cite /ws/presence route registration at main.rs:996 not construction at :816"
sources:
  - src/utils/binary_protocol.rs
  - src/handlers/socket_flow_handler/http_handler.rs
  - src/handlers/socket_flow_handler/position_updates.rs
  - src/protocols/binary_settings_protocol.rs
  - src/actors/presence_actor.rs
  - src/utils/nip98.rs
  - src/main.rs
  - crates/visionclaw-xr-presence/src/wire.rs
  - crates/visionclaw-xr-presence/src/agent_presence.rs
  - xr-client/rust/src/binary_protocol.rs
  - xr-client/rust/src/presence.rs
date: 2026-08-31
---

# Protocol Registry

## Purpose

Single owning document for every live wire frame, endpoint, and version-negotiation rule
in the DreamLab estate. It supersedes the scattered protocol prose in legacy ADR-011/031/061
and is the authoritative home for the V5 broadcast envelope, which no legacy ADR owns.

## Current State

### Frame tag registry

Every binary frame leads with a single tag byte. Tags live in **two disjoint spaces** because
they travel on different sockets and are demultiplexed independently — they never collide on one
socket, so the numeric overlap between the graph-protocol space and the settings-protocol space
is safe. The registry below is the source of truth for tag allocation.

| Tag | Name | Socket / path | Direction | Owning module (file:line) |
|-----|------|---------------|-----------|---------------------------|
| `0x03` | Graph position frame **V3** | `/wss` | server→client | `src/utils/binary_protocol.rs:12,54` |
| `0x05` | Graph position frame **V5 envelope** (wraps V3) | `/wss` | server→client | `src/utils/binary_protocol.rs:592` (`PROTOCOL_V5` branch); `xr-client/rust/src/binary_protocol.rs:25` |
| `0x23` | `AGENT_ACTION` beam event | `/wss` (fanned by ClientCoordinator) | server→client | `src/utils/binary_protocol.rs:1721` (`MessageType::AgentAction`) |
| `0x43` | Avatar pose (`OPCODE_AVATAR_POSE`) | `/ws/presence` | bidirectional | `crates/visionclaw-xr-presence/src/wire.rs:9` |
| `0x44` | Agent co-presence (`OPCODE_AGENT_PRESENCE`) | `/ws/presence` (sibling channel) | bidirectional | `crates/visionclaw-xr-presence/src/agent_presence.rs:40` |
| `0x05` | Settings-protocol batch message | settings binary socket (distinct) | bidirectional | `src/protocols/binary_settings_protocol.rs:211,311` |

The trailing `0x05` row is a **separate tag space** on the settings socket and is unrelated to
the V5 graph envelope; both are recorded here so no future allocation reuses either byte on its
own socket.

### `0x03` — Graph position frame V3 (52 bytes/node)

Canonical struct `WireNodeDataItemV3` (`src/utils/binary_protocol.rs:44`). One header byte
`0x03` then N fixed **52-byte** records, little-endian throughout. Field-by-field
(the layout comment block immediately below the `WIRE_*_SIZE` constants, cross-checked
against `encode_node_data_with_types` and the client decoder at
`xr-client/rust/src/binary_protocol.rs`):

> Citations in this section were re-derived against the working tree by ADR-2060 and then
> again after ADR-2057 inserted the const assertions, which shifted every later line in
> `binary_protocol.rs`. Prefer the symbol name over the line number when they disagree.

| Offset | Bytes | Type | Field | Notes |
|--------|-------|------|-------|-------|
| `@0`  | 4  | u32 | `id` | bits 0-25 node id; bits 26-31 type flags (below) |
| `@4`  | 12 | f32×3 | `position` | x,y,z metres |
| `@16` | 12 | f32×3 | `velocity` | vx,vy,vz |
| `@28` | 4  | f32 | `sssp_distance` | defaults `f32::INFINITY` when absent (`:431`) |
| `@32` | 4  | i32 | `sssp_parent` | defaults `-1` |
| `@36` | 4  | u32 | `cluster_id` | K-means/DBSCAN, 1-based, 0=unclustered |
| `@40` | 4  | f32 | `anomaly_score` | LOF, 0.0–1.0 |
| `@44` | 4  | u32 | `community_id` | Louvain assignment |
| `@48` | 4  | f32 | `centrality` | PageRank score |

Total **52 bytes**. `WIRE_V3_ITEM_SIZE == 52` is locked at **compile time** by
`const _: () = assert!(...)` (`binary_protocol.rs:93`), so changing any `WIRE_*_SIZE`
constant fails the build rather than a test run (ADR-2057). The original unit-test
`assert_eq!`s remain as a redundant second check (`binary_protocol.rs:1077,1174`); they are
no longer the primary guard. The V5 sequence-prefix width is locked the same way
(`WIRE_V5_SEQ_SIZE == 8`, `binary_protocol.rs:102`).

**Type flag bits** in the `@0` id (`binary_protocol.rs:15-26`, mirrored client-side at
`xr-client/rust/src/binary_protocol.rs:37-43`): `NODE_ID_MASK = 0x03FF_FFFF` (26-bit id, max
67,108,863); `AGENT = 0x8000_0000`; `KNOWLEDGE = 0x4000_0000`; ontology sub-types in bits 26-28
(`CLASS 0x0400_0000`, `INDIVIDUAL 0x0800_0000`, `PROPERTY 0x1000_0000`, mask `0x1C00_0000`).

The encoder strips flag bits before SSSP/analytics map lookups (`:425`) — a real bug class if
skipped, since the maps are keyed by compact id.

### `0x05` — V5 broadcast envelope

Layout: `[0x05][u64 broadcast_seq LE][ V3 body ]` where the V3 body is the exact 52-byte-per-node
stream above (no inner `0x03` header byte). Server decode at `binary_protocol.rs:592` (the `PROTOCOL_V5` match arm) skips
the 8-byte sequence then delegates to `decode_node_data_v3`. Client constants at
`xr-client/rust/src/binary_protocol.rs:25-28` (`PROTOCOL_V5 = 0x05`, `V5_SEQ_BYTES = 8`,
`NODE_RECORD_BYTES = 52`). The envelope is **optional** and additive: a receiver distinguishes
`0x03` from `0x05` by the leading byte and unwraps accordingly. `broadcast_seq` gives clients a
monotonic ordering/drop-detection handle. **This document is the owner of the V5 envelope** — no
legacy ADR defines it.

**Client parity (ADR-2078, 2026-09-05).** All three decoders are at parity. The web client's live
path is `client/src/types/binaryProtocol.ts:198-201,410-424` — it reads the u64 LE sequence,
rejects a frame shorter than 9 bytes (the client-side equivalent of the server's
`payload.len() < WIRE_V5_SEQ_SIZE` reject at `binary_protocol.rs:594`), decodes the body from
offset 9, and surfaces the sequence as the broadcast-ack sequence at
`client/src/store/websocket/binaryProtocol.ts:416`. The web decoder now also **declines** V2 and
any unrecognised version instead of decoding them: it previously parsed 36-byte V2 records and,
in its `default` arm, re-read an unknown frame from offset 0 as 36-byte records whenever the
length divided by 36 — **fabricating nodes at arbitrary positions from arbitrary payloads. That
hazard is closed.** The decoder is now pinned against
`crates/visionclaw-protocol/src/wire_fixtures.rs` by
`client/src/types/__tests__/wireFixtures.test.ts`, closing the gap that let it drift: it was the
only one of the three decoders with no fixture-backed cross-check.

### `0x23` — AGENT_ACTION beam event

`MessageType::AgentAction = 0x23` (`binary_protocol.rs:1721`). Frame =
`[0x23]` then one or more 15-byte headers with optional variable payloads. Header
(`AgentActionEvent`, `binary_protocol.rs:1490`, `AGENT_ACTION_HEADER_SIZE = 15` at `:1500`):
`source_agent_id u32 @0`, `target_node_id u32 @4`, `action_type u8 @8`
(Query/Update/Create/Delete/Link/Transform, `:1098-1105`), `timestamp u32 @9` (ms),
`duration_ms u16 @13`, then variable `payload`. Multiple actions coalesce into one frame
(`src/actors/agent_beam_actor.rs`). Identity-blind by design — carries agent-id-space numeric ids
only.

### `0x43` — Avatar pose

`OPCODE_AVATAR_POSE = 0x43` (`crates/visionclaw-xr-presence/src/wire.rs:9`). Per-pose layout
(`wire.rs:22-34`): `[0x43][u16 frame_len LE][u8;16 room_id_hash][u8 avatar_id_len][avatar_id
utf8][u64 timestamp_us LE][u8 transform_mask]` then present transforms (28 bytes each) in
head/left/right order. `transform_mask` bits: `head 0b001`, `left 0b010`, `right 0b100`
(`wire.rs:18-20`) — a presence bitmask, not a count, so asymmetric hand presence round-trips.
Server broadcast sibling-envelope for fan-out: `[0x43][u64 broadcast_seq LE][u32 room_id LE][u16
user_count LE]` (`src/actors/presence_actor.rs:49`, client parse
`xr-client/rust/src/presence.rs:144`).

### `0x44` — Agent co-presence

`OPCODE_AGENT_PRESENCE = 0x44` (`agent_presence.rs:40`). Additive sibling of `0x43` carrying
social state, not skeleton. Layout (`agent_presence.rs:19-32`): `[0x44][u16 body_len LE][u64 seq
LE][u16 agent_count LE]` then per agent `[u32 local_id LE][u8 field_mask]` with mask-gated fields
— `bit0 state` (`u8` activity 0 idle/1 working/2 awaiting_approval/3 speaking), `bit1 gaze`
(`i16×3` quantised unit dir, scale `32767.0`), `bit2 attention` (`u8` tag 0 none/1 user/2 node,
`+u32 node_id` when tag==2). Two logical channels (reliable for state/attention, high-rate
10–20 Hz for gaze-only) share this one codec (`agent_presence.rs:6-12`).

### REST + WebSocket endpoints and auth

Routes registered in `src/main.rs:986-1016`:

| Path | Method | Auth | Notes |
|------|--------|------|-------|
| `/wss` | WS upgrade | NIP-98 Bearer **or** `?token=` query (`http_handler.rs:139-150`) | graph position stream; Origin header required (`:130`) |
| `/wss/agent-events` | WS | inherits `/wss` seam | agent-event hub (`main.rs:990,1156`) |
| `/ws/presence` | WS | XR presence registry + `NostrIdentityVerifier` (`main.rs:996` route → handler `ws_presence`; state built at `:812-816`) | `0x43`/`0x44` traffic |
| `/ws/speech`, `/ws/mcp-relay`, `/ws/client-messages` | WS | per-handler | `main.rs:991-994` |
| `/healthz`, `/readyz` | GET | none (probes) | `main.rs:986-987` |
| `/client-logs` | POST | none | `main.rs:1016` |
| `/api/memory-cloud` | GET | `PowerUser` in handler (`memory_cloud_handler.rs:44`), regardless of `RBAC_PUBLIC_READS` | live stratified snapshot JSON (ADR-2133); 503 without `RUVECTOR_PG_CONNINFO` |
| `/api/memory-cloud/vectors?snapshot=<id>` | GET | as above | `application/octet-stream`, LE f32 `count*dim`, unit rows, snapshot row order; 409 on a stale id |
| `/api/memory-cloud/query` | POST | `RbacGate` (`WriteGraph`) **and** `PowerUser` in handler | sidecar HNSW top-k (namespace-restricted = exact scan); validation 400 |
| `/api/memory-cloud/health` | GET | central `/api` read policy | always 200; aggregates only, excluded namespaces omitted, cached recall probe |

**RBAC posture (open by *deployment*, fail-closed in code).** `RbacGate` middleware enforces
Owner>Admin>Editor>Viewer on NIP-98 pubkeys. The enforcement code **fails closed**:
`public_reads_enabled()` returns `false` when `RBAC_PUBLIC_READS` is unset
(`src/middleware/rbac_gate.rs:116-128`, `unwrap_or(false)` — its own doc-comment states this), and
startup **aborts** with no Owner assigned unless `RBAC_ALLOW_OWNERLESS=1` (`main.rs:717-737`,
`RBAC_ALLOW_OWNERLESS_ENV` at `src/services/role_store.rs:33`). The permissive posture is therefore
a *deployment* choice, not a structural code default: the shipped compose sets reads default-on via
`RBAC_PUBLIC_READS: "${RBAC_PUBLIC_READS:-1}"` (`docker-compose.unified.yml:78`), and an unassigned
authenticated pubkey resolves to Editor. `RBAC_PUBLIC_READS` appears **nowhere** in `src/main.rs`.
This is a deliberate compatibility trade-off; see [VC-SECURITY] for the named profile that must gate
a public deployment. Legacy: ADR-142.

**NIP-98 token validation** (`src/utils/nip98.rs`): ±60 s age window (`TOKEN_MAX_AGE_SECONDS = 60`,
`:168`) **plus** a process-wide single-use replay cache keyed on event id (`:161-184`) — replay
inside the 60 s window is now closed (landing 2026-08-31).

**Visibility filter** `PUBKEY_VISIBILITY_FILTER` now defaults **ON / fail-closed**
(`position_updates.rs:34-43`): private nodes without an owner match are dropped from the wire for
everyone. Enabling requires only absence of a falsy token. Legacy ADR-050/060.

### Version negotiation & compatibility policy

Legacy ADR-061 mandated "one binary protocol, no versioning". That doctrine is retired: the wire
already versions itself via the leading tag byte, and the decoder branches on it
(`binary_protocol.rs:506-522`). The live, enforced policy is:

1. **Tag byte is the version.** Every graph frame's first byte selects the codec: `0x03` (bare
   V3) or `0x05` (V5 envelope + V3 body). Receivers MUST branch on byte 0 and MUST reject unknown
   tags with an explicit error, never a silent reinterpret (`:521`, client `BadVersion` at
   `xr-client/rust/src/binary_protocol.rs:49`).
2. **Additive envelopes only.** A new envelope (like V5) wraps an existing body rather than
   re-laying fields. The inner body layout is frozen once shipped; new per-node analytics extend
   the record only by a new tag with a new documented length.
3. **Fixed record length is a hard invariant.** Payload length not divisible by the record size
   is a decode error (`:532`, client `Misaligned` at `xr-client/.../binary_protocol.rs:51`), so a
   length change cannot be silently absorbed.
4. **Removed versions fail loud.** V1 and V2 return explicit "upgrade client" errors
   (`:510-511`); they are not silently accepted.
5. **Registry-gated allocation.** New tag bytes are allocated only in this document, per socket,
   to prevent cross-feature collision.

### Deprecation process

- Mark the tag *deprecated* in this registry with the replacement and a removal date; keep the
  decoder branch returning data for one release.
- Next release: replace the decoder branch with an explicit "upgrade client" error (the V1/V2
  pattern at `:510-511`), never a silent drop.
- Only after telemetry shows zero live senders may the tag byte be freed — and it stays reserved
  in this table so it is never reused for a different meaning.

## Known divergences & open items

- **28B and 48B figures are retired.** Legacy ADR-061 ("28B forever") and ADR-031 ("48B") are both
  stale prose. 28 bytes survives only as the *internal* server-side `BinaryNodeData` struct,
  never on the wire; 48 was an interim analytics count. The wire is **52 bytes/node**, now locked
  at compile time — *Resolved — ADR-2057 (2026-09-05)*.
- **V5 envelope had no owning ADR** — *Resolved — ADR-2057 (2026-09-05)*, which is now the owning
  record for the envelope layout and the `broadcast_seq` contract. This document remains the
  registry of record for the tag allocation itself.
- **`?token=` query auth contradicts legacy ADR-011** — *Resolved — ADR-2058 (2026-09-05)*. The
  `Authorization` header is now the only accepted carrier in a release build; the query-string
  path is compiled out of release entirely and survives only behind the dev-auth gate, logging a
  `SECURITY:` warning when used. A release build that receives `token=` logs a rejection warning.
- **Two `0x05` allocations** (V5 graph envelope vs settings-protocol message) coexist on distinct
  sockets. Safe today; the registry records both so neither is reused on its own socket.
  **Caution (ADR-2060):** this hazard has already caused one documentation error —
  `BASELINE-architecture.md` cited the *settings* `0x05` as the graph V5 envelope. When citing
  `0x05`, always name the socket.
- **Settings binary protocol** (`src/protocols/binary_settings_protocol.rs`) is a separate framed
  protocol not yet fully enumerated here; fold it into this registry in a future revision.
  **Open** — unchanged by the 2026-09-05 remediation.
- **TypeScript client wire parity.** The web client (`client/src/services/binaryProtocol/`) has no
  `0x05`/V5 handling at all and still advertises `PROTOCOL_V2` in `SUPPORTED_PROTOCOLS`, which the
  server rejects outright. The XR Rust client parses V5 correctly. Spec routed to the vc-clients
  lead under ADR-2057; **open** until that lands. Root cause: the TS decoder is the only one of the
  three with no fixture-backed cross-check against
  `crates/visionclaw-protocol/src/wire_fixtures.rs`.

## Invariants (must not silently change)

- Byte 0 of every binary frame is the tag/version; unknown tags are rejected, not reinterpreted.
- V3 record is exactly 52 bytes with the field offsets above; `WIRE_V3_ITEM_SIZE == 52` is
  enforced by a compile-time `const _: () = assert!(...)` (`binary_protocol.rs:93`), so a width
  change fails the build (ADR-2057). The V5 envelope's 8-byte `broadcast_seq` prefix is locked
  the same way (`binary_protocol.rs:102`).
- `NODE_ID_MASK = 0x03FF_FFFF`; flag bits 26-31 are stripped before analytics/SSSP map lookups.
- All multi-byte fields are little-endian.
- Tag allocation happens only in this registry, scoped per socket.
- The visibility filter default is fail-closed (ON).
- Memory-cloud wire types (`client/src/features/visualisation/memoryCloud/types.ts`) change only
  together with `crates/visionclaw-memory-cloud/src/wire.rs`, whose field-name test pins them; the
  vectors blob is little-endian f32 with no header (ADR-2133).
- Namespaces in `MEMORY_CLOUD_EXCLUDE_NAMESPACES` (default `personal-context`) never appear in any
  memory-cloud response, and the private memory-cloud reads never drop below `PowerUser`.

## Change process

This is a living ground-truth document, not an ADR. Amend it in the same PR that changes any
wire-facing code, cite `file:line`, and bump `version`. Any new frame tag, field, offset, or
endpoint auth change MUST update the tables here before merge. Removals follow the deprecation
process above. Ratification: reviewed against the code at the recorded `verified_commit`.

## Estate closeout qualification — 2026-09-04

See the [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) and [source/test receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-render-snapshot.json). XR discards V5 sequence values; envelope compatibility is not consumer ordering enforcement. Its 0x23 parser accepts complete events before truncation, whereas malformed position framing is rejected. Codec-specific rejection and freshness policies need explicit tests. The 0x44 codec remains staged without a located live integration. Agent-events transport authenticates a session, but frame processing does not receive that session identity; structural provenance alone does not bind the claimed agent to the sender.

## Remediation — 2026-09-05

- **ADR-2057** — The 52-byte V3 record and the 8-byte V5 sequence prefix are locked at compile
  time with `const _: () = assert!(...)`; the former test-only assertions remain as a redundant
  check. This ADR is the owning record for the V5 envelope layout and the `broadcast_seq`
  contract. `PROTOCOL_V5` and `WIRE_V5_SEQ_SIZE` replace bare literals in the decode branch.
- **ADR-2078** (vc-clients) — The web TypeScript decoder fails closed on V2 and on unknown
  versions, the size-autodetect node-fabrication hazard is removed, and the decoder is
  fixture-pinned against `wire_fixtures.rs`. Amends ADR-2057, whose Finding 1 ("the TS client has
  no V5 support") was wrong — the live path always had it, including the short-payload guard.
- **ADR-2058** — WebSocket bearer tokens are accepted only from the `Authorization` header in a
  release build. The `?token=` query path is compiled out of release and survives only behind
  the dev-auth gate with a `SECURITY:` warning; a release build receiving `token=` logs a
  rejection.
- **ADR-2060** — Citation corrections after line drift in `binary_protocol.rs`, and the
  `0x05` caution above (a mis-citation of the settings tag as the graph envelope had already
  occurred in BASELINE).
- **ADR-2099** (vc-clients) — The bare-tag `[0x23][count:u16]…` decoder is the sole
  agent-action path in the web client. The framed-header branch was unreachable — `parseHeader`
  reads its type byte from offset 0, the same byte the tag check has already consumed — so it is
  deleted rather than kept as a fallback, and the suite now asserts `extractPayload` is never
  reached for a 0x23 frame. Unknown-version fail-closed behaviour (ADR-2078) is unchanged.
- **Open** — TypeScript client V5 parity and V2 de-advertisement, routed to the vc-clients
  lead under ADR-2057; and the settings binary protocol is still not enumerated here.
