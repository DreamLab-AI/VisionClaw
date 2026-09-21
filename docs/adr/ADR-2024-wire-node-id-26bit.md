---
id: ADR-2024
title: Wire node-ID is an ephemeral 26-bit u32 with reserved type-flag bits, not a URN
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 997440cd0717d4c5f9341369571fc69fcf5a38d6
verified_paths: [src/utils/binary_protocol.rs]
owner: jjohare
review_trigger: node count approaching 2^26, or promotion of the debug_assert ceiling to a runtime guard
repo: visionclaw
domain: IDENTIFIER-taxonomy
lineage: legacy protocol V1/V2 records (V1 truncation bug); distils the render-plane-vs-durable-store split
---

# ADR-2024 — Wire node-ID is an ephemeral 26-bit u32 with reserved type-flag bits, not a URN

## Context

The render plane pushes node positions to clients every frame; carrying a full
`urn:visionclaw` string per node per frame is untenable. The durable identifier
(the URN) and the wire identifier are therefore distinct concerns. Protocol V1's
narrow field silently truncated IDs above its ceiling and corrupted them; V3
must reserve enough width and encode node type inline for the client. See
`docs/IDENTIFIER-taxonomy.md` for the render-plane vs durable-store split.

## Decision

On Protocol V3 a node travels as a compact sequential `u32`: bits 0-25 are the
ID (`NODE_ID_MASK = 0x03FFFFFF`, ceiling 2^26-1), bits 26-31 are type flags
(bit 31 agent, bit 30 knowledge, bits 26-28 ontology subtypes). This wire ID is
ephemeral and render-only — it is never a durable identifier and never persisted
as one. Flag setters mask the ID and OR the flag; decode strips flags via
`& NODE_ID_MASK`. The 26-bit ceiling is asserted with `debug_assert!` (fail-fast
in debug); release builds additionally emit a `log::error!` before masking an
over-range ID to its low 26 bits, so the historical V1 truncation is now **loud,
not silent**. This forecloses shipping a URN on the hot path.

## Consequences

- Per-frame payloads stay compact and the client reads node type without a
  lookup.
- The release guard logs (`error!`) but still truncates: an over-range ID is now
  loud, not silently corrupt, though truncation remains the failure mode above
  2^26. A hard reject (Result-returning setters) is the next step if node counts
  ever approach the ceiling.
- Only three type classes fit the reserved bits; a fourth top-level class needs a
  new bit and a protocol bump.

## Verification

Re-verified at `eac01130`: `src/utils/binary_protocol.rs` — flag masks and
`NODE_ID_MASK = 0x03FFFFFF`; the five `set_*_flag` setters (agent, knowledge, and
three ontology subtypes) `debug_assert!` the ceiling, then — in every build —
`if node_id > NODE_ID_MASK { error!(…) }` before `(id & NODE_ID_MASK) | FLAG`, so
the release path now logs the overflow instead of truncating silently; decode
strips via `& NODE_ID_MASK` (`clear_all_flags`, `get_actual_node_id`). The
silent-truncation gap is closed (loud, not silent); a hard reject remains the
open hardening tracked by `review_trigger`.

## Closeout extension — 2026-09-04

CP-01/02/06/08. Owner remains jjohare with protocol/graph/client maintainers. Complete/live is retained for ephemeral 26-bit IDs and typed setter behaviour. The inspected server/XR/browser masks agree. Overflow logging is specific to the five typed setters: the untyped encoder branch only debug-asserts, then forwards the unchanged ID through the identity wire helper. This source finding does not establish a live overflow or collision.

**Acceptance condition:** Test allocator plus encoder/decoder boundaries for every class, including untyped nodes, in debug and release; specify reject/remap behaviour before capacity exhaustion. Bind compact maps to graph generations and test full/delta/reconnect and stale-map retirement across clients. Reopen on allocator/capacity, class-bit allocation or mapping persistence changes. See [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#wire-identifier-overflow-coverage) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/wire-force-boundaries.json). No runtime overflow frame was exercised.

## Acceptance progress — 2026-09-05

**Implemented — the release-build gap is closed.** The finding was that the five
typed setters logged and masked overflow in release while the untyped encoder
branch only `debug_assert!`ed and then forwarded the id *unchanged*. A release
build therefore shipped an over-range id straight onto the wire with no
diagnostic, where it aliases an existing id and picks up spurious class-flag bits.

All six branches now share one helper, `enforce_wire_id_bounds(id, WireIdClass)`,
with behaviour **identical in debug and release**: remap by masking to 26 bits and
log the overflow as an error naming the class and both ids. `debug_assert!` is
retained so development builds still fail loudly at the offending call site, but
release no longer depends on it for either the bound or the diagnostic.

Remap rather than reject is the deliberate choice, and is now documented as such:
a dropped node vanishes from the layout with no trace, whereas a masked one is
visibly wrong *and* leaves a log line. `WireIdClass::Untyped` exists precisely so
the previously-silent branch names itself in that log.

`to_wire_id_v2` stays an identity function, and now says why: it runs on an
already-flagged id whose bits 26..=31 legitimately carry the class, so masking
there would strip the class off every node. The bound is enforced upstream on the
bare id.

**Testability.** The overflow branch cannot be reached through
`enforce_wire_id_bounds` in a test profile — `debug_assert!` panics first, which
is exactly how the release-only gap went unnoticed. The pure remap is therefore
split out as `remap_wire_id(id) -> (masked, overflowed)`, which *is* the code a
release build runs, and the tests target it directly.

**Tests run.** `cargo test --lib --no-default-features adr_20` — 34 pass, 6 of
them in `adr_2024_wire_id_bounds`: in-range pass-through for all six classes;
release-path remap and overflow reporting for `2^26`, `2^26+7`, `0x80000000` and
`u32::MAX`; exact boundary (`NODE_ID_MASK` valid, `+1` overflows); a remapped id
leaking no flag bit (`get_node_type` returns `Unknown`); each typed setter
stamping only its own class with the id surviving; and an end-to-end encode
proving the untyped branch emits a bounded, class-free wire id.

**Governed paths changed.** `src/utils/binary_protocol.rs`.

**Open.** No over-range id was produced by a live allocator and no rendered
collision was observed — this is boundary coverage, not evidence of a deployed
overflow. Per-generation durable-to-wire mapping, full/delta/reconnect map
consistency and stale-map retirement across clients remain unaddressed; those are
allocator and session-lifecycle concerns rather than encoder-boundary ones.
`implementation_status` is unchanged.

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

**Governed change since `eac011303`:** `src/utils/binary_protocol.rs` only.

**The 26-bit contract is unchanged.** `NODE_ID_MASK = 0x03FFFFFF` at `:29` with
the ceiling documented at `:27-28`; `AGENT_NODE_FLAG = 0x80000000` (`:18`),
`KNOWLEDGE_NODE_FLAG = 0x40000000` (`:19`), ontology subtypes on bits 26-28
(`:21`). Decode still strips flags via `& NODE_ID_MASK` (`:241`, `:253`).

**The Verification block above is now understated and is corrected here.** It
says "the five `set_*_flag` setters" carry the overflow guard. At HEAD there are
**six** guarded branches, all routed through one helper
`enforce_wire_id_bounds(node_id, WireIdClass)` (`:192-213`), whose behaviour is
identical in debug and release — `debug_assert!` at `:194-199`, then
`remap_wire_id` (`:224-226`) and an unconditional `log::error!` at `:204` naming
the class and both ids before masking:

| Branch | Site | Class |
|---|---|---|
| `set_agent_flag` | `:229` | `WireIdClass::Agent` |
| `set_knowledge_flag` | `:233` | `WireIdClass::Knowledge` |
| `set_ontology_class_flag` | `:283` | `WireIdClass::OntologyClass` |
| `set_ontology_individual_flag` | `:287` | `WireIdClass::OntologyIndividual` |
| `set_ontology_property_flag` | `:291` | `WireIdClass::OntologyProperty` |
| untyped encoder branch | `:470` | `WireIdClass::Untyped` |

The sixth row is the release-build gap the 2026-09-04 closeout identified — the
untyped branch previously `debug_assert!`ed and then forwarded an over-range id
**unchanged** onto the wire. It is closed: the untyped path now masks and logs
like every other class. `WireIdClass` is defined at `:145-165`.

**Consequences text still accurate:** the release guard logs and truncates; it
does not reject. A hard `Result`-returning setter remains the open hardening
tracked by `review_trigger`, and the node count is nowhere near 2^26.

**Commands run:** `git diff --stat eac011303..HEAD -- src/utils/binary_protocol.rs`;
`grep -n 'NODE_ID_MASK|AGENT_NODE_FLAG|KNOWLEDGE_NODE_FLAG|enforce_wire_id_bounds|
remap_wire_id|WireIdClass|fn set_.*flag' src/utils/binary_protocol.rs`;
`cargo test --lib --no-default-features binary_protocol` → **38 passed, 0
failed**, including the `remap_wire_id` all-classes suite at `:785-800`.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed change since `b0bc275f6`:** `src/utils/binary_protocol.rs` (+64) —
`encode_node_data_extended_with_sssp` gained an arm before the untyped branch:
an id whose bits 26-31 already form a recognised class pattern
(`get_node_type(id) != NodeType::Unknown`) is forwarded unchanged. Callers that
stamp the class flag themselves (`position_updates.rs`, `actor_messages.rs`)
pass empty class sets, so their ids reached the over-range check with the flag
bits set: that panicked every broadcast in debug builds (each XR client saw a
2 s "connection reset" loop, 2026-09-08) and would have stripped the class flag
in release.

**Decision refined, not contradicted.** A stamped id is not an over-range id, so
excluding it from the ceiling check is the ceiling being applied correctly.
`NODE_ID_MASK = 0x03FFFFFF` is unchanged (`:29`); a compact raw id never carries
bits 26-31, and a raw id with a high bit that is *not* a class pattern still
takes the loud path — the added test asserts `get_node_type(0x2000_0001)` is
`Unknown` and that `remap_wire_id` still reports the overflow. The
debug_assert-plus-`log::error!`-then-mask behaviour for genuine over-range ids
stands. `verified_commit` moved to the CI-repair commit.
