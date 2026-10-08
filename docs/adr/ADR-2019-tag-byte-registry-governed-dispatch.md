---
id: ADR-2019
title: Tag byte 0 is registry-governed — it selects the codec, rejects unknown tags, and is allocated across disjoint per-socket spaces
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 31bc3d3a1990703a6ce0a433bdff77d1fe86080e
verified_paths: [src/utils/binary_protocol.rs, xr-client/rust/src/binary_protocol.rs, src/protocols/binary_settings_protocol.rs, crates/visionclaw-xr-presence/src/wire.rs, crates/visionclaw-xr-presence/src/agent_presence.rs]
owner: jjohare
review_trigger: allocation of a new opcode/version tag on any binary socket, or a proposal to share one demultiplexer across sockets
repo: visionclaw
domain: PROTOCOL-registry
lineage: "Reverses legacy ADR-061 D4 (versioning-vocabulary-removed); distils ADR-059 (0x23 agent channel) + ADR-102 (presence handshake) tag fragmentation into a single allocation authority."
---

# ADR-2019 — Tag byte 0 is registry-governed — it selects the codec, rejects unknown tags, and is allocated across disjoint per-socket spaces

## Context

Every binary frame leads with a tag byte. ADR-061 D4 had removed the versioning
vocabulary, leaving no authority over what the leading byte means or who may
allocate one; tags accreted ad hoc across channels (ADR-059 0x23 agent,
ADR-102 presence handshake). Without a rule, a receiver could reinterpret an
unknown tag as a known one and silently misparse, and there was no defined
relationship between the graph socket's tag space and the settings/presence
sockets' tag spaces, which happen to reuse the same numeric values.

## Decision

The leading tag byte **selects the codec** and receivers MUST branch on it:
graph `0x03` decodes a bare V3 body, `0x05` a V5 envelope. Removed versions
(V1/V2) **fail loud** with an explicit upgrade error, and any unknown tag is
**rejected, never reinterpreted**. Tag allocation happens only in the
PROTOCOL-registry and is **scoped per socket**: the graph space, the settings
space, and the presence space are independent, so numeric overlap between them
(e.g. graph `0x05` vs settings `0x05`) is legitimate because each socket is
demultiplexed on its own. This forecloses cross-socket tag collisions being
treated as errors and forecloses lenient reinterpretation of unknown bytes.

## Consequences

- A corrupt or downgraded frame surfaces as a named error at the boundary
  rather than a misparsed record; clients on retired versions get a clear
  upgrade signal instead of garbage.
- The same byte value carries different meanings on different sockets by
  design; anyone reading the registry must read the socket column too — reuse
  is a feature, not a clash.
- Every new tag is a registry transaction: adding one out-of-band (a raw magic
  byte in a codec) is now a defect, which is the intended governance cost.

## Verification

At e0f8cd896: `src/utils/binary_protocol.rs` `match protocol_version` returns
explicit "no longer supported / please upgrade" errors for `1` and `2`, routes
`0x03`/`5`, and ends `v => Err(format!("Unknown protocol version: {}", v))`.
Client `xr-client/rust/src/binary_protocol.rs` carries a typed
`DecodeError::BadVersion`. Disjoint reuse confirmed: graph `0x05` (the V5
branch) coexists with settings `0x05` in
`src/protocols/binary_settings_protocol.rs`, and presence uses `0x43`
(`crates/visionclaw-xr-presence/src/wire.rs`) and `0x44`
(`.../agent_presence.rs`).

## Closeout extension — 2026-09-04

Work package: **CP-01/06**. Owner remains `jjohare`, with protocol, identity and XR maintainers responsible for their respective boundaries.

Position frames reject unknown versions and misalignment. The separately dispatched 0x23 visual batch accepts complete events before truncation, so rejection guarantees must be scoped by codec.

**Acceptance condition:** Document and test each opcode's malformed/truncated-frame policy; use fixtures from the real server encoder in both browser and XR decoders and verify unknown-tag handling at each demultiplexer.

Dependencies: CP-01 release identity and CP-04 authority where authenticated actions cross the wire. Reopen on the existing review trigger, a changed opcode or a failing freshness/visibility probe. Existing verification and activation fields retain their historical scope; this annex records source/local tests at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`, not a new live certification.

See [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-render-snapshot.json).

## Acceptance progress — 2026-09-05

**Policy documented.** The per-opcode malformed/truncated policy is now written
down where each decoder lives, and — importantly — the policy is *not uniform*,
which was the substance of the closeout finding. Recorded matrix:

| Opcode | Consumer | Malformed / truncated policy |
|---|---|---|
| `0x03`/`0x05` position | XR (Rust) | all-or-nothing: unknown version, misaligned body and truncated V5 sequence all reject the whole frame |
| `0x03`/`0x05` position | server decoder | all-or-nothing |
| `0x03`/`0x05` position | browser | tolerant: complete records kept, torn tail dropped (a partial record must not blank the view) |
| `0x23` agent action | XR (Rust) | tolerant prefix parse: complete events before a truncation are accepted |
| `0x23` agent action | server decoder | strict: a truncated or over-counted batch is refused whole |
| `0x23`/`0x43`/`0x44` | browser position decoder | declined outright, never size-auto-detected |

The XR/server asymmetry on `0x23` is deliberate and now stated as such: the
ingest side must not accept a partial batch into the event stream, while the
render side should not discard a whole burst of visual activity over one torn
event.

**Implemented.** One behavioural change, in the browser demultiplexer. Its
`default:` branch previously fell through to size-based auto-detection for *any*
unrecognised first byte, so a `0x23`/`0x43`/`0x44` frame whose length happened to
be a multiple of the 36-byte V2 stride would be reinterpreted as node records —
fabricating nodes at arbitrary positions from another codec's payload. A
`SIBLING_OPCODES` guard now declines known sibling opcodes before auto-detection;
genuinely unknown, non-sibling versions still auto-detect as before.

**Fixtures from the real encoder.** `crates/visionclaw-protocol/src/wire_fixtures.rs`
supplies both well-formed and malformed frames, and is pinned byte for byte to
`encode_node_data_extended_with_sssp` and `encode_agent_actions` by
`adr_2019_shared_fixture_equivalence`. The XR decoder consumes that same source
file via `#[path]`; the browser test mirrors its layout with a comment binding it
to the Rust module.

**Tests run.**

- `xr-client/rust`: `cargo test` — 227 lib + 75 integration pass. Frame-policy
  cases cover unknown versions (9 values), misaligned bodies, truncated V5
  sequence (8 lengths), empty frames, the `0x23` tolerant prefix parse,
  overstated batch counts and mutual opcode refusal between the two codecs.
- Server: `cargo test --lib --no-default-features adr_20` — 34 pass, including
  strict server-side batch rejection and unknown-version refusal.
- Browser: `./node_modules/.bin/vitest run
  src/types/__tests__/binaryProtocol.framePolicy.test.ts` — 15 pass, covering a
  256-value version sweep, exhaustive truncation sweeps asserting the decoder
  never throws, sibling-opcode refusal and V5 envelope additivity.

**Governed paths changed.** `client/src/types/binaryProtocol.ts`,
`client/src/types/__tests__/binaryProtocol.framePolicy.test.ts`,
`xr-client/rust/tests/wire_freshness_and_frame_policy.rs`,
`crates/visionclaw-protocol/src/wire_fixtures.rs`, `src/utils/binary_protocol.rs`
(tests).

**Open.** Fixtures are byte-equal to the encoder's output, but no frame was
carried over a live socket in this pass, and the browser decoder's behaviour is
verified in jsdom rather than in a running client.

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

**Governed changes since `9a2c80873`:** only the two position codecs —
`src/utils/binary_protocol.rs` and `xr-client/rust/src/binary_protocol.rs`.
`src/protocols/binary_settings_protocol.rs`,
`crates/visionclaw-xr-presence/src/wire.rs` and `.../agent_presence.rs` are
**unchanged**, so the disjoint-space half of this decision was not disturbed.

**Tag byte still selects the codec, and unknown tags are still rejected.**
`match protocol_version` at `src/utils/binary_protocol.rs:588-601`:
`1 =>` and `2 =>` return the explicit "no longer supported / please upgrade"
errors (`:589-590`), `PROTOCOL_V3` routes the bare body (`:591`), the V5 branch
skips `WIRE_V5_SEQ_SIZE` then delegates (`:598`), and the arm still ends
`v => Err(format!("Unknown protocol version: {}", v))` at `:600` — rejected,
never reinterpreted. Client-side the typed `DecodeError::BadVersion` is raised at
`xr-client/rust/src/binary_protocol.rs:428`.

**Per-socket disjointness re-confirmed by grep at HEAD:** settings writes tag
`0x05` at `src/protocols/binary_settings_protocol.rs:234`; presence uses
`OPCODE_AVATAR_POSE = 0x43` (`crates/visionclaw-xr-presence/src/wire.rs:9`) and
`OPCODE_AGENT_PRESENCE = 0x44`
(`crates/visionclaw-xr-presence/src/agent_presence.rs:40`). Graph `0x05` and
settings `0x05` therefore still coexist legitimately on separate sockets, as the
Decision requires.

**The per-opcode policy matrix above is now backed by a shared fixture.**
`crates/visionclaw-protocol/src/wire_fixtures.rs` (exported at
`crates/visionclaw-protocol/src/lib.rs:31`) is pinned to the real server encoder
by a server-side test and included verbatim into the isolated xr-client
workspace at `xr-client/rust/tests/wire_freshness_and_frame_policy.rs:11-12`,
which discharges the "use fixtures from the real server encoder in both decoders"
half of the 2026-09-04 acceptance condition for the Rust consumers. The browser
decoder is still fixture-free — that part remains open.

**Commands run:** `git diff --stat 9a2c80873..HEAD -- <verified_paths>`;
`grep -n 'match protocol_version|no longer supported|Unknown protocol version'
src/utils/binary_protocol.rs`; `grep -rn '0x05|0x43|0x44|0x23'` over the settings
and presence codecs; `grep -rn wire_fixtures xr-client/rust --include=*.rs`;
`cargo test --lib --no-default-features binary_protocol` → **38 passed**;
`cargo test --lib --no-default-features adr_20` → **35 passed**; `cargo test` in
`xr-client/rust` → **226 lib + 83 integration passed, 0 failed**.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed changes since `4d1a698e7`:** the same two additive changes recorded
under ADR-2018 — the encoder's pre-stamped-id arm
(`src/utils/binary_protocol.rs`, +64) and three out-of-band `#[func]`s on the
client (`xr-client/rust/src/binary_protocol.rs`, +39).
`src/protocols/binary_settings_protocol.rs`,
`crates/visionclaw-xr-presence/src/wire.rs` and `.../agent_presence.rs` are
unchanged.

**Decision unaffected.** No tag was allocated, removed or reinterpreted: `0x03`
still selects the bare V3 body, `0x05` the V5 envelope, removed versions still
fail loud and unknown tags are still rejected rather than reinterpreted. The
change is inside the V3 body encoder, downstream of dispatch. `verified_commit`
moved to the CI-repair commit.

## Re-verification — 2026-09-22 at a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2

**Governed changes since `997440cd0`:** `src/protocols/binary_settings_protocol.rs` moved only by `rustfmt` (import ordering, line wrapping, trailing commas). The other four governed paths are unchanged.

**Decision unaffected.** No tag was allocated, removed or reinterpreted; the `0xFF` compressed-frame marker and the unknown-type rejection arms are byte-for-byte the same after formatting. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:** `xr-client/rust/src/binary_protocol.rs` gains `send_text` and the heat hooks only. The new `/wss` traffic (`beatClock`, `memoryRoute`, the JSON `pong` `serverTime`) is JSON text routed by `type` (ADR-2134), not tagged binary, so no tag byte is allocated and the per-socket registry is untouched. **Decision unaffected.** Unknown binary tags are still rejected. Verified with `cargo test -p visionclaw-xr-gdext` (264 + 83 pass).

## Re-verification — 2026-10-07 at 39f580e93 (merged XR parity tree)

The merge of `feat/xr-cloud` (1c03ffb2a; it carries `feat/xr-graph`) brings these into one tree with the ADR-2134 work:
- xr-cloud's memory cloud and route layers (WP6/7);
- xr-graph's palette, settings sync, hulls and node LOD (WP1/2/4).

Those branches did not move this record's `verified_commit`, so the combined state is re-verified here. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 105 integration tests, and GUT on HP Godot 4.6.1 (`--xr-mode off`) passes 149 tests.

The new traffic is JSON text routed by `type`: `settingsUpdated`, `filter_update_success`, `graphUpdated` and `memoryRoute`. No binary tag is allocated or reinterpreted, and `binary_protocol.rs` has no tag-line changes. **Decision unaffected.**

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

`binary_protocol.rs` adds no tag and changes no decode branch. **Decision unaffected.**

## Re-verification — 2026-10-07 (feat/xr-graph pack plans)

The merge of `feat/xr-graph` at e6c4b0fb5 (merge 015bd642f) changed governed files without updating this ADR, so its changes were checked against the decision. No tag byte, dispatch arm or registry entry changed; the diff is pack instrumentation only. **Still holds.**

## Re-verification — 2026-10-07 (integration merge)

At 8f375c132, which merges `feat/xr-graph` (f1ef384dc) and `feat/xr-pulse` into the memory-cloud-explorer integration branch. The only governed change is `xr-client/rust/src/binary_protocol.rs` (+48/-10 since 015bd642f). That change is pack-timing instrumentation and the `graph_layer_triangles` FrameBudget accessor (`:1631`), recorded under ADR-2018. No tag byte, dispatch arm or registry entry changed. `DecodeError::BadVersion` is still raised for an unknown version (`:477`), and the freshness path still refuses it (`:763`). The server codec, the settings codec and both presence codecs are untouched. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext --offline` passes 363 library + 118 integration tests across 17 integration binaries, 0 failed, including `wire_freshness_and_frame_policy.rs`. GUT was not re-run in this pass; the HUD and FrameBudget GUT receipts are those recorded on the sprint branches (f65c69e24, 5f53cba68). **Decision unaffected.**

## Re-verification — 2026-10-07 (35bd7c6bc)

`xr-client/rust/src/binary_protocol.rs` changed at 48e1bf327 (+12/-0) only by adding the `graph_robust_bounds` `#[func]` (`:1778`), a getter over the render store that frames the memory cloud. No tag is allocated, matched or dispatched differently, and the version bytes are unchanged (`PROTOCOL_V5 = 0x05`, `:25`). The new `memoryRoute` fields `sidecarTotal`/`sidecarAgree` are JSON text frames on `/wss`, outside the tag-byte space (recorded under ADR-2134). Suite at 35bd7c6bc: `cargo test --offline` in `xr-client/rust` passes 384 library + 118 integration tests, 0 failed. **Decision unaffected.**

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-xr-presence/src/wire.rs`: `chunks_exact(4)` becomes `as_chunks::<4>()` (same chunks, remainder still dropped); `src/utils/binary_protocol.rs`: `encode_node_data_extended_with_sssp` takes the five class-id sets as one `NodeClassIds` struct; the flag precedence and encoded bytes are unchanged; `xr-client/rust/src/binary_protocol.rs`: `chunks_exact` becomes `as_chunks` (same 52-byte records); a module-level allow for gdext's generated `CallError` closures. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record); `cargo test --workspace` in `xr-client/rust`: 495 passed, 0 failed, before and after. **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `src/utils/binary_protocol.rs`: deletes the unused V2 item-size and alias constants; the V3 52-byte `const` assertion and all encoders/decoders are untouched.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 (aa01c5536)

This stamp covers the merge of `chore/clippy-sweep` (299aa35bc) with `feat/xr-cloud-parity`, and the lint follow-up aa01c5536. Each branch re-verified this record against its own changes (sections above). The merge itself kept both sides; the only code it combined was test code in `client_coordinator_actor.rs`. aa01c5536 is mechanical: rustfmt, an `async-trait` patch bump, `as_chunks`, and test checks made `const`. No wire format, tag byte, settings key, pose owner, crate boundary or relay rule changed. Decision holds. Verified with `cargo test --workspace --tests` (3,244 passed), xr-client `cargo test --workspace` (516 passed) and clippy `-D warnings` clean in both.

## Re-verification — 2026-10-07 (b63d35f8a)

Merging fix/xr-held-above-route touches this record's paths only with a benchmark-only `#[func] pin_thread_to_l3` (no wire, tag, record-size or decode change) and aim-ray/priority wiring in `graph_scene.gd` (no settings key, physics write or pose-owner change). Decision holds.

## Re-verification — 2026-10-07 (ADR-2135: f275173a3, 08a3e2a41, 96d9c426d)

ADR-2135 changes `xr-client/rust/src/binary_protocol.rs` only through GDScript-facing methods and an extra call after an applied `0x23` action. No tag byte or dispatch arm changed. Decision holds. Verified at 96d9c426d on f95dc554f: server `cargo test --lib` 1,565 passed, 0 failed, 6 ignored; `cargo test -p visionclaw-tri-layout` 22 + 2 doc; xr-client `cargo test --workspace` 527 passed; client vitest 1,264; clippy `-D warnings` and fmt clean on the server lib, the new crate and the xr-client workspace.

## Re-verification — 2026-10-08 at 31bc3d3a1 (ADR-2135 amendment, ADR-2136)

`xr-client/rust/src/binary_protocol.rs` changes only two Godot methods (`agent_drift_offset`, `graph_robust_bounds` lose their separation argument). Tag dispatch is untouched. Decision holds. xr-client workspace: 549 passed.
