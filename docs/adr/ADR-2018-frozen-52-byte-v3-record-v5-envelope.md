---
id: ADR-2018
title: The graph frame is a frozen 52-byte inline-analytics V3 record wrapped additively by the V5 sequence envelope
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 31bc3d3a1990703a6ce0a433bdff77d1fe86080e
verified_paths: [src/utils/binary_protocol.rs, xr-client/rust/src/binary_protocol.rs]
owner: jjohare
review_trigger: a new GPU analytics field that cannot fit an existing slot, or any need to change the 52-byte node-record layout
repo: visionclaw
domain: PROTOCOL-registry
lineage: "Retires legacy ADR-061 D1/D2 (28B forever + separate JSON analytics channel); ADR-031 (36->52B tail); ADR-102 §2 (shipped 52B WireNodeDataItemV3); ADR-137 §5 (V5 wrapper)."
---

# ADR-2018 — The graph frame is a frozen 52-byte inline-analytics V3 record wrapped additively by the V5 sequence envelope

## Context

GPU physics produces sticky per-node analytics (sssp distance/parent, cluster,
anomaly, community, centrality) every frame. The legacy plan (ADR-061 D1/D2)
kept a 28-byte record forever and shipped analytics out-of-band as JSON,
splitting one truth across two channels that could disagree. V3 instead folded
the analytics inline; the record grew 36->52 bytes (ADR-031, ADR-102 §2). V5
(ADR-137 §5) then needed monotonic ordering for delta/full broadcast dedup
without disturbing the byte layout every client already parses.

## Decision

The node record is `WireNodeDataItemV3`: a fixed **52-byte** frame carrying
nine fields — `id`/`position`/`velocity` plus the six sticky analytics values
(sssp distance/parent, cluster, anomaly, community, centrality) — and
`WIRE_V3_ITEM_SIZE == 52` is a hard,
test-asserted invariant. V5 is purely additive: `[0x05][u64 broadcast_seq
LE][52B/node V3 body]` — the decoder skips the 8-byte sequence and delegates to
the unchanged V3 codec. The V3 body is therefore **frozen**: no field may be
resized, reordered, or repurposed. New analytics extend the registry only via a
**new tag with a new documented length**, never by mutating the 52-byte record.
This forecloses silent layout drift and the two-channel analytics split.

## Consequences

- One wire truth: analytics and positions arrive in the same record, so they
  cannot desynchronise, and the size assertion fails the build loudly if anyone
  edits the struct without updating the invariant.
- The 52-byte record is a fixed budget: a tenth analytic that will not fit an
  existing slot forces a new tag/codec rather than a cheap in-place edit — the
  intended cost of freezing.
- V5's 8 bytes/frame overhead buys ordering; encoders and decoders must agree
  the sequence is skipped, not interpreted, before the V3 body.

## Verification

At e0f8cd896: `src/utils/binary_protocol.rs` — `WireNodeDataItemV3` carries
`sssp_distance/sssp_parent/cluster_id/anomaly_score/community_id/centrality`
(layout comment lines 38-51); `test_wire_format_size` asserts
`WIRE_V3_ITEM_SIZE == 52`. The decoder `match protocol_version` routes `5 =>`
through an 8-byte skip then `decode_node_data_v3(&payload[8..])`. Client
mirror `xr-client/rust/src/binary_protocol.rs` fixes `NODE_RECORD_BYTES = 52`,
`PROTOCOL_V5 = 0x05`, `V5_SEQ_BYTES = 8`.

## Closeout extension — 2026-09-04

Work package: **CP-01/06**. Owner remains `jjohare`, with protocol, identity and XR maintainers responsible for their respective boundaries.

The XR decoder skips V5 sequence bytes; a sequence envelope alone does not establish consumer ordering. All 218 XR Rust library tests pass, including the sequence-skip test.

**Acceptance condition:** Define and test consumer freshness across full/delta production and reconnects, using decreasing/duplicate sequences and a shared server/client fixture. Preserve the 52-byte record.

Dependencies: CP-01 release identity and CP-04 authority where authenticated actions cross the wire. Reopen on the existing review trigger, a changed opcode or a failing freshness/visibility probe. Existing verification and activation fields retain their historical scope; this annex records source/local tests at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`, not a new live certification.

See [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-render-snapshot.json).

## Acceptance progress — 2026-09-05

**Implemented.** Consumer freshness now exists as an enforced contract rather than
an envelope field the consumer discarded.

- `decode_position_frame_with_sequence` returns the V5 broadcast sequence
  alongside the records; `decode_position_frame` is retained as a
  sequence-discarding wrapper. A V3 frame yields `None` — it makes no ordering
  claim at all, and that is now explicit in the type rather than implicit in a
  skipped read.
- `FreshnessGate` implements the ordering contract: increasing sequence accepted;
  equal rejected as duplicate; lower rejected as stale; unsequenced V3 accepted
  but never moving the watermark; after `reconnect()` only a `Full` frame may
  re-baseline (a producer restart legitimately moves the sequence backwards),
  and a `Delta` arriving before that snapshot is refused for want of a baseline.
  `admit_frame` decodes and gates in one step, withholding records on rejection
  so a caller cannot apply a frame the gate refused.
- Wired live: `BinaryProtocolClient` gates every inbound position frame,
  arms a resync on `Disconnected`, and exposes `freshness_counters()`
  (`[stale, duplicate, awaiting_resync, resyncs]`) so the ordering problem is
  observable on a deployment rather than only in tests.
- The 52-byte record is untouched; a fixture test asserts the frozen size and
  that the V5 envelope is purely additive over the V3 body.

**Shared server/client fixture.** `crates/visionclaw-protocol/src/wire_fixtures.rs`
is the single source of the frame bytes. It is dependency-free, so the
deliberately isolated `xr-client/rust` workspace includes the *same source file*
by `#[path]` rather than taking on the domain crate's tree. A server-side test
pins those fixtures to the real encoder byte for byte, so the fixture is a pinned
rendering of the producer, not a second implementation of the protocol.

**Tests run.**

- `cargo test --lib` and `cargo test` in `xr-client/rust` — 227 lib + 75
  integration pass, including 23 in `tests/wire_freshness_and_frame_policy.rs`
  (decreasing, duplicate, full/delta interleaving, reconnect resync, delta before
  resync, unsequenced V3, frozen record size).
- `cargo test --lib --no-default-features adr_20` in the server crate — 34 pass,
  including the fixture/encoder equivalence and V5-additivity checks.

**Governed paths changed.** `xr-client/rust/src/binary_protocol.rs`,
`xr-client/rust/tests/wire_freshness_and_frame_policy.rs`,
`crates/visionclaw-protocol/src/wire_fixtures.rs`,
`crates/visionclaw-protocol/src/lib.rs`, `src/utils/binary_protocol.rs` (tests).

**Open.** The gate is exercised against fixtures and in unit tests, not against a
live concurrent full/delta producer pair or a real reconnect to a restarted
server. `implementation_status` is unchanged: the ordering contract is
implemented and tested at the consumer, but end-to-end certification against live
production remains outstanding.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `eac011303`:** `src/utils/binary_protocol.rs` (+487/-…)
and `xr-client/rust/src/binary_protocol.rs` (+338/-…) — the freshness-gate and
shared-fixture work recorded above, landed by `1d68d8eb1` and `f21d30922`.

**The frozen record survived the change, which is the whole point of the record.**
`WIRE_V3_ITEM_SIZE` is still composed field-by-field at
`src/utils/binary_protocol.rs:75-83` and pinned by a **compile-time** assertion at
`:93-96` — `const _: () = assert!(WIRE_V3_ITEM_SIZE == 52, "ADR-2057: V3 wire
record must be exactly 52 bytes …")`. This is stronger than the
`test_wire_format_size` runtime check the Verification block above cites: it now
fails the *build*, not a test run. The nine fields and the layout comment are at
`:114-125`.

**V5 is still purely additive.** The decoder dispatch at `:588-601` routes
`PROTOCOL_V3 => decode_node_data_v3(payload)` (`:591`) and, for V5, skips
`WIRE_V5_SEQ_SIZE` before delegating to the *same unchanged* V3 codec at `:598`.

**The client mirror still fixes the same three constants:**
`xr-client/rust/src/binary_protocol.rs` — `PROTOCOL_V5 = 0x05` (`:25`),
`V5_SEQ_BYTES = 8` (`:26`), `NODE_RECORD_BYTES = 52` (`:28`), with the alignment
error naming the record size at `:51`.

**New surface, all additive.** `decode_position_frame_with_sequence` (`:400`)
returns the broadcast sequence; `decode_position_frame` (`:389`) is the
sequence-discarding wrapper, so no existing caller changes. `FreshnessGate`
(`:537`) is a consumer-side ordering contract that reads the envelope field —
it does not alter the frame. The shared fixture
`crates/visionclaw-protocol/src/wire_fixtures.rs` is exported at
`crates/visionclaw-protocol/src/lib.rs:31` and included into the isolated
xr-client workspace by `#[path]` at
`xr-client/rust/tests/wire_freshness_and_frame_policy.rs:11-12`, so both sides
test against one byte-level source.

**Commands run:** `git diff --stat eac011303..HEAD -- src/utils/binary_protocol.rs
xr-client/rust/src/binary_protocol.rs`; `grep -n` over both files for
`WIRE_V3_ITEM_SIZE|NODE_RECORD_BYTES|PROTOCOL_V5|V5_SEQ_BYTES|match protocol_version|
decode_node_data_v3|FreshnessGate`; `ls`/`grep` for `wire_fixtures`; `cargo test
--lib --no-default-features binary_protocol` → **38 passed, 0 failed**;
`cargo test --lib --no-default-features adr_20` → **35 passed, 0 failed**
(fixture/encoder equivalence and V5-additivity).

**Client-side receipt at this commit:** `cargo test` in `xr-client/rust` →
**226 lib + 83 integration passed, 0 failed** across 11 integration binaries,
including **23 in `tests/wire_freshness_and_frame_policy.rs`** (decreasing,
duplicate, full/delta interleaving, reconnect resync, delta-before-resync,
unsequenced V3, frozen record size). The acceptance section's "227 lib + 75
integration" was that session's count; the suite has since been re-partitioned —
the 23-case freshness figure it names is exact.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed changes since `4d1a698e7`:** both files additive only.
`src/utils/binary_protocol.rs` (+64) added the pre-stamped-id pass-through arm
of the encoder's class-flag branch plus its regression test;
`xr-client/rust/src/binary_protocol.rs` (+39) added three `#[func]`s
(`set_agent_anchors`, `retire_agents`, `server_clock_ms`) for ADR-2109.

**Decision unaffected.** The record size is untouched: the static assertion
`WIRE_V3_ITEM_SIZE == 52` still stands (`src/utils/binary_protocol.rs:94`), the
client still pins `NODE_RECORD_BYTES: usize = 52`
(`xr-client/rust/src/binary_protocol.rs:28`) with `record_is_52_bytes` at
`:1785`, and no field was added to the record or the V5 envelope. The new
`#[func]`s carry anchors and lifecycle out of band, not on the wire.
`verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:** `xr-client/rust/src/binary_protocol.rs` adds three `BinaryProtocolClient` methods (`send_text`, `set_attention_heat`, `get_node_heat`) and advances the render store's heat clock in `poll()`. **Decision unaffected.** No wire constant, record size or decoder branch changed: `NODE_RECORD_BYTES = 52`, `V5_SEQ_BYTES = 8`, and the V3/V5 decode paths are byte-identical. `cargo test -p visionclaw-xr-gdext` passes 264 library and 83 integration tests, including the frozen-layout and V5 fixtures.

## Re-verification — 2026-10-07 at 39f580e93 (merged XR parity tree)

The merge of `feat/xr-cloud` (1c03ffb2a; it carries `feat/xr-graph`) brings these into one tree with the ADR-2134 work:
- xr-cloud's memory cloud and route layers (WP6/7);
- xr-graph's palette, settings sync, hulls and node LOD (WP1/2/4).

Those branches did not move this record's `verified_commit`, so the combined state is re-verified here. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 105 integration tests, and GUT on HP Godot 4.6.1 (`--xr-mode off`) passes 149 tests.

In `xr-client/rust/src/binary_protocol.rs` the merged changes add settings-sync, refetch and hull state to `BinaryProtocolClient`. No wire constant or decode branch changed (`git diff b6fbe772d..HEAD` shows no `NODE_RECORD_BYTES`, `V5_SEQ_BYTES`, `PROTOCOL_V*` or tag line). **Decision unaffected.**

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

`binary_protocol.rs` gains 30 lines of LOD buffer accessors (`build_edge_buffer_lod`, ribbon buffers). There is no wire constant, record-size or decode change. **Decision unaffected.**

## Re-verification — 2026-10-07 (feat/xr-graph pack plans)

The merge of `feat/xr-graph` at e6c4b0fb5 (merge 015bd642f) changed governed files without updating this ADR, so its changes were checked against the decision. `binary_protocol.rs` gains only pack-timing fields and a `last_pack_ms` getter; the hand-off now moves the packed `&[f32]` into the Godot array. Record sizes, the v5 envelope and every decode path are untouched. **Still holds.**

## Re-verification — 2026-10-07 (integration merge)

At 8f375c132, which merges `feat/xr-graph` (f1ef384dc) and `feat/xr-pulse` into the memory-cloud-explorer integration branch. `git diff 015bd642f..HEAD -- xr-client/rust/src/binary_protocol.rs` is +48/-10. The change switches the LOD pack timing to a thread-CPU stopwatch (`crate::thread_cpu::CpuStopwatch`, with fields `last_node_pack_cpu`/`last_edge_pack_cpu`), adds three `#[func]`s (`graph_layer_triangles` at `:1631`, `last_pack_cpu_ms` at `:1656` and `thread_cpu_ms` at `:1663`) and reorders the heat-clock fields in the struct. Nothing touches the wire. `PROTOCOL_V5 = 0x05` (`:25`), `V5_SEQ_BYTES = 8` (`:26`) and `NODE_RECORD_BYTES = 52` (`:28`) are unchanged, as are `decode_position_frame_with_sequence` (`:449`) and `FreshnessGate` (`:586`). `record_is_52_bytes` (`:2198`) passes. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext --offline` passes 363 library + 118 integration tests across 17 integration binaries, 0 failed, including `wire_freshness_and_frame_policy.rs`. GUT was not re-run in this pass; the HUD and FrameBudget GUT receipts are those recorded on the sprint branches (f65c69e24, 5f53cba68). **Decision unaffected.**

## Re-verification — 2026-10-07 (35bd7c6bc)

The merge of `feat/xr-cloud-parity` (48e1bf327) adds one `#[func]`, `graph_robust_bounds` (`xr-client/rust/src/binary_protocol.rs:1778`), to `binary_protocol.rs` (+12/-0). It returns the robust bounds `[cx, cy, cz, radius]` of the stored positions so that the memory cloud can be framed on the graph, and it reads `RenderStore::robust_bounds` (`render_store.rs:1936`). It is a local getter and nothing touches the wire. `PROTOCOL_V5 = 0x05` (`:25`), `V5_SEQ_BYTES = 8` (`:26`) and `NODE_RECORD_BYTES = 52` (`:28`) are unchanged, as are `decode_position_frame_with_sequence` (`:449`) and `record_is_52_bytes` (`:2210`). Suite at 35bd7c6bc: `cargo test --offline` in `xr-client/rust` passes 384 library + 118 integration tests, 0 failed. **Decision unaffected.**

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/utils/binary_protocol.rs`: `encode_node_data_extended_with_sssp` takes the five class-id sets as one `NodeClassIds` struct; the flag precedence and encoded bytes are unchanged; `xr-client/rust/src/binary_protocol.rs`: `chunks_exact` becomes `as_chunks` (same 52-byte records); a module-level allow for gdext's generated `CallError` closures. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record); `cargo test --workspace` in `xr-client/rust`: 495 passed, 0 failed, before and after. **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `src/utils/binary_protocol.rs`: deletes the unused V2 item-size and alias constants; the V3 52-byte `const` assertion and all encoders/decoders are untouched.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 (aa01c5536)

This stamp covers the merge of `chore/clippy-sweep` (299aa35bc) with `feat/xr-cloud-parity`, and the lint follow-up aa01c5536. Each branch re-verified this record against its own changes (sections above). The merge itself kept both sides; the only code it combined was test code in `client_coordinator_actor.rs`. aa01c5536 is mechanical: rustfmt, an `async-trait` patch bump, `as_chunks`, and test checks made `const`. No wire format, tag byte, settings key, pose owner, crate boundary or relay rule changed. Decision holds. Verified with `cargo test --workspace --tests` (3,244 passed), xr-client `cargo test --workspace` (516 passed) and clippy `-D warnings` clean in both.

## Re-verification — 2026-10-07 (b63d35f8a)

Merging fix/xr-held-above-route touches this record's paths only with a benchmark-only `#[func] pin_thread_to_l3` (no wire, tag, record-size or decode change) and aim-ray/priority wiring in `graph_scene.gd` (no settings key, physics write or pose-owner change). Decision holds.

## Re-verification — 2026-10-07 (ADR-2135: f275173a3, 08a3e2a41, 96d9c426d)

ADR-2135 changes `xr-client/rust/src/binary_protocol.rs` in three ways. `graph_robust_bounds(separation)` folds the graph bounds for the memory cloud. Applied `0x23` actions also feed the render-store agent drift. Three `#[func]`s are added (`record_memory_flash`, `step_agent_drift`, `agent_drift_offset`). The 52-byte V3 record, the V5 envelope and every decoder are untouched. Decision holds. Verified at 96d9c426d on f95dc554f: server `cargo test --lib` 1,565 passed, 0 failed, 6 ignored; `cargo test -p visionclaw-tri-layout` 22 + 2 doc; xr-client `cargo test --workspace` 527 passed; client vitest 1,264; clippy `-D warnings` and fmt clean on the server lib, the new crate and the xr-client workspace.

## Re-verification — 2026-10-08 at 31bc3d3a1 (ADR-2135 amendment, ADR-2136)

`xr-client/rust/src/binary_protocol.rs` changes only two Godot methods: `agent_drift_offset(agent_id)` and `graph_robust_bounds()` lose their separation argument. The 52-byte V3 record and the V5 envelope are untouched. Decision holds. xr-client workspace: 549 passed.
