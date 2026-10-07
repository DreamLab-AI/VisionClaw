---
id: ADR-2005
title: Hexagonal split of the webxr monolith into a thin root binary plus visionclaw crates
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: aa01c5536efd6d65c72dcc644e21472bfd2e223c
verified_paths: [Cargo.toml, src/actors, crates/visionclaw-actors/src]
owner: jjohare
review_trigger: completion of the actor extraction into crates/visionclaw-actors, or a new subsystem that does not map to an existing crate layer
repo: visionclaw
domain: BASELINE-architecture
lineage: Distils legacy ADR-090 (hexagonal crate modularisation, 2026-05-28) + parent PRD-016; ADR-090 amendment folded the planned visionclaw-server crate back into the thin root binary.
---

# ADR-2005 — Hexagonal split of the webxr monolith into a thin root binary plus visionclaw crates

## Context

The webxr backend was a single ~123k-line crate: a one-line change recompiled
everything and layer boundaries were unenforceable. Lineage: ADR-090 hexagonal
modularisation (2026-05-28) under PRD-016; the ADR-090 amendment dropped the
planned `visionclaw-server` crate, folding startup wiring back into the root
binary rather than adding a layer.

## Decision

New code lands in the crate matching its hexagonal layer —
`visionclaw-{contracts,domain,protocol,adapters,gpu,ontology,actors,xr-presence,analytics-oracle}`
— and the root binary is reduced to startup wiring. The original workspace declared the
root plus these nine `visionclaw-*` members; current additions are recorded below.
It excludes the gdext client
(`xr-client/rust`) and `agentbox/crates/headroom-napi`, which compile in their
own contexts.

## Consequences

- The compiler enforces declared crate dependencies. Intended layer direction
  and incremental-build savings need separate acceptance evidence.
- The migration is unfinished: the live server still runs from `src/`, and the
  actor layer is barely extracted. Two source-of-truth trees coexist until the
  extraction completes — a real navigation and drift cost.
- `contracts` is a deliberate leaf (no actix, no heavy deps) so it stays
  independently buildable.

## Verification

`Cargo.toml` `[workspace].members` lists `"."` plus the nine `crates/visionclaw-*`
members; `exclude` lists `xr-client/rust` and `agentbox/crates/headroom-napi`.
The extraction is measurably partial: `src/actors/*.rs` has 25 files against 4 in
`crates/visionclaw-actors/src/` (the mint plan recorded 11 — extraction has not
advanced, so `implementation: partial` is if anything sharpened). Verified at
`e0f8cd896`; re-verified at `542d63d1d` after the ADR-141 formatting sweep
reordered `pub use` re-exports in `src/actors/messages/mod.rs` — semantics
unchanged.

## Closeout extension — 2026-09-04

CP-01/03/06/08. Owner remains jjohare with crate/actor/build maintainers. Partial/live is retained. The current manifest has twelve members, adding vault-migrate and visionclaw-integration-tests to the historical root-plus-nine list. The actor crate documents root-internal dependencies that still block extraction. File counts do not prove independent responsibility or build-time improvement.

**Acceptance condition:** Define allowed dependency directions and module ownership, classify forwarding shims versus competing implementations, migrate callers and prove the root contains only its accepted responsibilities. Measure representative incremental changes and verify relevant feature/build combinations before retiring old modules. Reopen on new layers, dependency cycles or actor extraction completion. See [architecture review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/vision-and-architecture.md#server-extraction-and-enforceable-boundaries) and [manifest/source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/crate-supervision-snapshot.json). No build timing or complete dependency-graph validation ran.

### Re-verification 2026-09-05 (ADR-2005)

Re-checked at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` after `Cargo.toml` changed
since the previous `verified_commit` (`9423abdb`). Both frontmatter fields are
deliberately loosened for this pass — `verified_paths` is emptied and
`verified_commit` set to the current HEAD — and **both must be restored at the
landing commit** (`verified_paths: [Cargo.toml, src/actors, crates/visionclaw-actors/src]`
plus that commit's SHA) so the staleness check regains its teeth.

Claim-by-claim:

- **Workspace membership is now twelve, not root-plus-nine.** `Cargo.toml:2-15`
  lists `"."` (`:3`) plus `crates/visionclaw-contracts` (`:4`),
  `visionclaw-domain` (`:5`), `visionclaw-protocol` (`:6`), `visionclaw-adapters`
  (`:7`), `visionclaw-gpu` (`:8`), `visionclaw-ontology` (`:9`),
  `visionclaw-actors` (`:10`), `visionclaw-xr-presence` (`:11`),
  `visionclaw-analytics-oracle` (`:12`), `vault-migrate` (`:13`) and
  `visionclaw-integration-tests` (`:14`). The two additions are the ones the
  2026-09-04 closeout extension already recorded — this re-verification confirms
  them against the manifest rather than the prose.
- **Exclusions unchanged.** `Cargo.toml:19` — `exclude = ["xr-client/rust",
  "agentbox/crates/headroom-napi"]`, matching the Decision text.
- **Actor extraction is still partial, and the two counts in the Verification
  section measure different things.** `src/actors/*.rs` is **25** files, unchanged.
  `crates/visionclaw-actors/src/` holds **4** top-level `.rs` files —
  `lib.rs`, `protected_settings_actor.rs`, `supervisor.rs`, `voice_commands.rs` —
  and **11** files counted recursively, because `messages/` contributes the
  remaining seven. The Verification section's "4" is the top-level figure and the
  mint plan's "11" is the recursive one; they were never in conflict, and the
  recursive figure has not moved. Only three of those files are actor
  implementations, so the live tree still runs its actors from `src/`.
- **`contracts` remains a deliberate leaf.** `Cargo.toml` retains the comment that
  `visionclaw-contracts` is independently buildable via
  `cargo build --manifest-path crates/visionclaw-contracts/Cargo.toml`.
- **New observation, not previously recorded.** `crates/graph-cognition-extract/`
  exists on disk but is empty (no `src`, no `Cargo.toml`) and is **not** a
  workspace member — an orphan directory that the member census should either
  adopt or delete.

`implementation_status: partial` and `activation_status: live` are retained: the
manifest grew, the extraction did not.

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

**The frontmatter loosening flagged in the previous section is now reversed.**
That pass emptied `verified_paths` and pinned `verified_commit` to a
then-uncommitted tree, and said both "must be restored at the landing commit".
Done: `verified_paths` is back to `[Cargo.toml, src/actors,
crates/visionclaw-actors/src]` and `verified_commit` is the landing SHA
`b0bc275f6`, so the staleness gate has its teeth back.

**Governed changes since `b00c28a0d`:** `Cargo.toml`, sixteen files under
`src/actors`, and two message files in `crates/visionclaw-actors/src` — landed by
`346fff7af` (actor trim), `da2f5cac7` (GPU consolidation) and `35c2448a8` (dead
module removal).

**Workspace shape re-counted at HEAD.** `[workspace].members` has **12** entries:
`"."` plus nine `crates/visionclaw-*` and two others — `crates/vault-migrate` and
`crates/visionclaw-integration-tests` (`Cargo.toml:2-15`). This matches the
2026-09-04 closeout's count exactly; the Decision's "root plus these nine
`visionclaw-*` members" is still literally true, with the two non-`visionclaw-*`
additions being the "current additions recorded below". `exclude` is unchanged at
`["xr-client/rust", "agentbox/crates/headroom-napi"]` (`Cargo.toml:19`).

**The extraction ratio moved — by shrinking the root, not by extracting.** The
Verification block above cites "25 files against 4". At HEAD:

- `ls src/actors/*.rs | wc -l` → **23** (was 25). Two files were **deleted**:
  `src/actors/lifecycle.rs` and `src/actors/supervisor.rs`
  (`git diff --name-status` shows `D` for both), removed as dead supervision
  machinery under ADR-2045.
- `ls crates/visionclaw-actors/src/*.rs | wc -l` → **4**, unchanged. The only
  changes in that crate are two message files
  (`messages/analytics_messages.rs`, `messages/mod.rs`), both `M`.

So no actor was extracted this sprint. The gap narrowed from 25:4 to 23:4 purely
by deleting dead code in the root. `implementation_status: partial` is not merely
retained — it is confirmed by direct file-level evidence, and the `review_trigger`
(completion of the actor extraction) is no closer.

**Consequences text still accurate:** two source-of-truth trees still coexist, the
live server still runs from `src/`, and `contracts` remains a deliberate leaf
(`crates/visionclaw-contracts/Cargo.toml:19` still asserts it pulls no actix and
no neo4rs).

**Still open, unchanged:** allowed dependency directions are not machine-enforced,
no forwarding-shim-versus-competing-implementation classification exists, and no
incremental-build timing ran. File counts remain a proxy for extraction progress,
not proof of independent responsibility.

**Commands run:** `git diff --name-status b00c28a0d..HEAD -- src/actors
crates/visionclaw-actors/src`; `git diff --stat` on the same;
`ls src/actors/*.rs | wc -l`; `ls crates/visionclaw-actors/src/*.rs | wc -l`;
`find src/actors -name '*.rs' | wc -l` → 59 recursive; a Python parse of
`[workspace].members` → 12 entries.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: crates/visionclaw-actors messages: the never-sent `RefreshMetadata` message and its re-exports deleted (ADR-2097), plus `SupervisorActor` now the sole home of that type (ADR-2045 complete); the crate split and dependency direction are unchanged — ADR-2095 in fact relied on it, placing the typed ngm constructor in visionclaw-domain because adapters cannot depend on the server. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.


## EA-04 source qualification — 2026-09-07

At `e299114056c0bf3ff7ccc22f7a68efe65068e183`, request journalling remains in `src/services/acsp/client.rs`; the decision actor delegates its conditional application claim through `DecisionElevationStore` to `SqliteEnrichmentRepository`. No actor was moved into a crate, no workspace member or Cargo dependency changed, and no dependency-boundary enforcement was introduced. The extraction decision and partial status remain unchanged. The new persistence does not prove independent responsibility or incremental build improvement. Validation: the shared root library suite with `--no-default-features --features solid-pod-embed` passed1,369 tests with six ignored; no release-profile or build-timing claim follows.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed changes since `e29911405`:** `Cargo.toml` (+7: the
`[profile.dev-runtime]` profile, no member change);
`src/actors/decision_elevation_actor.rs` (elevation confidence `0.5` → `None`,
FR2.4/EXP-AC-002 — absence is now representable instead of fabricated);
`src/actors/voice_interface_actor.rs` (one doc comment, Kokoro → PocketTts);
`src/actors/elevation_actor.rs` (+368: pending-case rehydration and
reconciliation planning plus their unit tests), and rustfmt of that file in this
commit.

**Decision unaffected.** The decision governs *which crate new code lands in*,
not what root-crate actors do internally. No actor moved between crates, no
workspace member was added or removed, and the nine `visionclaw-*` members plus
the root binary are unchanged. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-09-22 at a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2

**Governed changes since `997440cd0`:** the workspace is now thirteen members — `crates/vault-migrate` was deleted (ADR-2112/ADR-2113) and `crates/vault-core` + `crates/vault` added at `Cargo.toml:13-14`, so the count in the 2026-09-04 review above reads "twelve" and names `vault-migrate`; both are superseded by this note. In `src/actors/`, `elevation_actor.rs` drafts frontmatter-only OKF pages instead of a `json-ld` fence, the blocked `ProcessOntologyData` message (and its `LogseqPage` import) was deleted from `messages/ontology_messages.rs`, and `optimized_settings_actor.rs` dropped the legacy `logseq` path alias (ADR-2115). `crates/visionclaw-actors/src/messages/ontology_messages.rs` lost one doc-comment line.

**Decision unaffected.** No actor crossed a crate boundary and the thin-root-plus-crates shape is intact; removing `ProcessOntologyData` deletes one of the root-internal dependencies that blocked extraction rather than adding one. `.github/workflows/ci.yml` was corrected in the same commit to build `vault-core` and `vault` in place of the deleted crate. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-09-22 at 853c4a069 (Sovereign Corpus landing)

**Governed changes since `a32abac57`:** `Cargo.toml` adds the root dependency on `crates/vault-core`; `crates/visionclaw-actors/src/messages/ontology_messages.rs` changes one comment ("Logseq-based" → "validation/report surface"). **Decision unaffected.** `vault-core` is a leaf domain crate with no dependency on server layers, so the hexagonal direction of dependencies holds. `verified_commit` moved to the landing commit. Gates at that commit: vault 294 + vault-core 111 + golden parity 15/15; server lib 1,444; corpus_local_sync 4, vault_gate_test 18, jsonld_validator_test 3; client tsc clean; fmt and clippy -D warnings clean on the crates.

## Re-verification — 2026-10-02 at 805219679 (space/Earth domain registry)

**Governed changes since `853c4a069`:** `src/actors/gpu/force_compute_actor.rs:1175` and `src/actors/gpu/gpu_resource_actor.rs:228` replace their hard-coded six-code domain → class-ID tables with `vault_core::domains::domain_class_id` (ADR-2118). The charge rule is the same (known domain 0.6 / 0.3, unknown 1.2 / 2.5). `Cargo.toml` and `crates/visionclaw-actors/src` are unchanged. **Decision unaffected.** The root already depends on `vault-core` (`Cargo.toml:51`). That crate's `[dependencies]` are serde/regex/sha2-class leaves with no server-layer crate, so the dependency direction holds. No actor crossed a crate boundary and the member count is unchanged. `verified_commit` moved to the landing commit. No build or test gate ran for this re-anchor; it is a source reading only.

## Re-verification — 2026-10-02 at 8bdece469 (DAG rank provenance, ADR-2035)

**Governed change since `805219679`:** `src/actors/gpu/force_compute_actor.rs` replaces `is_directed_hierarchy_relation` with `hierarchy_pairs`, which ranks only edges for which `Edge::asserts_subsumption` holds. **Decision unaffected, and the change follows it.** The subsumption rule is domain knowledge, so it lives on the domain model in `crates/visionclaw-domain/src/models/edge.rs` with the new `RDFS_SUBCLASS_OF_IRI` and `DOMAIN_MEMBER_EDGE_TYPE` constants; the actor in the root binary only consumes it. No crate was added or moved, and `Cargo.toml` and `crates/visionclaw-actors/src` are unchanged. Tests: `cargo test --lib -- dag_rank_tests …` (69 pass), `cargo test -p visionclaw-domain` (275 pass).

## Re-verification — 2026-10-02 at c0906ed6e201dd09b3e9baab642c0b0b67adca88

`c0906ed6e` adds actor messages (`UpdateChainPayments`, `GetChainPayments`) and their handlers inside `src/actors`, and a pure projection module in `src/services`. Nothing moves between crates, and the split this record tracks is neither advanced nor reversed. The decision holds unchanged.

## Re-verification — 2026-10-03 at fdcbc9120fda25fd93fdaee744d68d2c00713d6b

`1d3e14a30` and `fdcbc9120` change `Cargo.toml` (the solid-pod-rs pin, feature `mrc20`, a `[patch.crates-io]` for `nostr-bbs-core`; ADR-2111, S4 amendment). Workspace members are unchanged, nothing moves between crates, and `src/actors` and `crates/visionclaw-actors/src` are untouched. The decision holds unchanged.

## Re-verification — 2026-10-03 at 780eb3edb788c9cb568d5756689a3c8455db79b7

`1e55daebb` (`Cargo.toml`: the `nostr-bbs-core` git patch is removed and the crates.io pin used) leaves `[workspace].members` unchanged. In `780eb3edb`, `src/actors/elevation_actor.rs` and `decision_elevation_actor.rs` change only their panel-key lookup. The new loader is in the root `src/services/acsp/`, beside the ACSP client it serves. That adds nothing to the extraction backlog and moves nothing across a crate boundary. `implementation: partial` stands. The decision holds.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `Cargo.toml` adds the workspace member `crates/visionclaw-memory-cloud` (pure logic, no server-layer dependency), root dependencies `tokio-postgres 0.7.18`, `deadpool-postgres 0.14.2` and the new crate, and a `[profile.dev.package.visionclaw-memory-cloud] opt-level = 3` override. Line citations into `Cargo.toml` after line 15 shift by +1, after line 128 by +8 and after line 310 by +14. **Decision unaffected, and the change follows it.** The new crate holds only pure logic (serde, sha2, thiserror) and depends on no server layer; I/O stays in the root binary's service and handler. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:** `crates/visionclaw-actors/src/messages/client_messages.rs` adds `RelayToUserSessions` (actix and std only, alongside `SendToClientText`). The root re-exports it (`src/actors/messages/{client_messages,mod}.rs`), and `src/actors/client_coordinator_actor.rs` adds `ClientManager::relay_text_to_pubkey`, its handler and a probe-actor test. **Decision unaffected, and the change follows it.** The new message is domain-safe, so it lives in the domain crate. The coordinator still reaches sessions only through `ClientRecipients`, so no `handlers::*` import enters the actor layer. `implementation: partial` stands. Verified by source reading plus `cargo check --lib --tests` and `cargo test --lib -- session_relay relay_reaches_only client_coordinator` (11 pass).

## Re-verification — 2026-10-07 (089f196d6)

`client_coordinator_actor.rs` changed in how broadcasts handle a full client mailbox: the frame is skipped for that client and it stays registered, and only a closed mailbox is evicted (`settle_broadcast_result`). The crate boundaries this record governs are unchanged. The decision holds.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-actors/src/messages/analytics_messages.rs`: `SetNodeSSSP` carries a `SharedNodeSssp` alias of the same type; doc list indentation; `crates/visionclaw-actors/src/protected_settings_actor.rs`: a test builds `ProtectedSettings` with a struct literal; `crates/visionclaw-actors/src/supervisor.rs`: a needless borrow and an unused test import removed; `src/actors/agent_beam_actor.rs`: `map_or(true, ..)` becomes `is_none_or(..)`; `src/actors/agent_monitor_actor.rs`: a redundant closure removed; `src/actors/client_coordinator_actor.rs`: the V3 encoder call passes its five class-id sets as one `NodeClassIds` (same sets, same bytes); struct-literal `ClientFilter` in tests; `src/actors/client_filter.rs`: tests build `ClientFilter`/`Metadata` with struct literals; `src/actors/elevation_actor.rs`: a test helper builds `Node` with a struct literal; `src/actors/event_coordination.rs`: an unused test import removed; `src/actors/gpu/analytics_supervisor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/analytics_telemetry.rs`: `>= before + 1` becomes `> before` in a test; `src/actors/gpu/anomaly_detection_actor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/clustering_actor.rs`: coherence clamp written as `clamp` with the old NaN -> 0.1 mapping kept explicitly; test struct literals; `src/actors/gpu/constraint_actor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/context_bus.rs`: a `match` on `send` becomes `unwrap_or_default()` (0 when no receivers, as before); `src/actors/gpu/force_compute_actor.rs`: `initialize_graph` is passed borrowed slices; utilisation uses `clamp(0, 100)`; doc paragraph breaks; `src/actors/gpu/gpu_manager_actor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/gpu_resource_actor.rs`: `initialize_graph` is passed borrowed slices (same data uploaded); `src/actors/gpu/graph_analytics_supervisor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/ontology_constraint_actor.rs`: the unused report-based `apply_ontology_constraints`/`report_axioms_to_domain` pair deleted; the live `ApplyMaterializedAxioms` -> `ingest_domain_axioms` path is unchanged; `src/actors/gpu/pagerank_actor.rs`: `% 2 == 0` becomes `is_multiple_of(2)`; `src/actors/gpu/physics_supervisor.rs`: `Default` delegating to `new()` added; a needless `return` removed; `src/actors/gpu/resource_supervisor.rs`: `Default` delegating to `new()` added; `src/actors/gpu/semantic_forces_actor.rs`: `SemanticConfig`'s field-wise `Default` becomes `#[derive(Default)]` (same field defaults); `src/actors/gpu/shared.rs`: mean computation time uses `checked_div(..).unwrap_or(0)`; doc paragraph break; `src/actors/gpu/shortest_path_actor.rs`: the shared SSSP map field uses the `SharedNodeSssp` alias; `src/actors/gpu/stress_majorization_actor.rs`: `Default` delegating to `new()` added; a range check written with `contains`; `src/actors/graph_service_supervisor.rs`: `UpdateSimulationParams` is boxed in the two message enums and unboxed when forwarded; auto-trigger scheduling passes the fn pointer directly; `src/actors/graph_state_actor.rs`: `iter_mut` over `(_, v)` becomes `values_mut()`; `src/actors/messages/analytics_messages.rs`: re-exports `SharedNodeSssp`; `src/actors/messages/mod.rs`: re-exports `SharedNodeSssp`; `src/actors/messages/physics_messages.rs`: a `clone()` of a `Copy` id removed; `src/actors/mod.rs`: `PhysicsState`'s field-wise `Default` becomes `#[derive(Default)]`; `src/actors/multi_mcp_visualization_actor.rs`: two large enum fields are boxed (serde serialises the box transparently); `src/actors/ontology_actor.rs`: doc paragraph break; `src/actors/optimized_settings_actor.rs`: `if let Ok(_)` becomes `.is_ok()`; `src/actors/physics_orchestrator_actor.rs`: tests build `SimulationParams` with struct literals; `src/actors/presence_actor.rs`: a test helper's return type uses a local alias; `src/actors/semantic_processor_actor.rs`: readability uses `clamp(0, 100)`; struct-literal defaults. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `Cargo.toml`: adds `subtle = "2.6.1"` as a direct dependency for the constant-time `X-Agent-Key` comparison (`utils::agent_key`).
- `crates/visionclaw-actors/src/supervisor.rs`: deletes the uncalled `should_restart` and the never-read `ActorState.session_id` / `RestartAttempt.supervisor_name` fields.
- `src/actors/agent_beam_actor.rs`: deletes the uncalled `gluon_deferral_note`.
- `src/actors/agent_monitor_actor.rs`: deletes the never-read `last_poll` field.
- `src/actors/gpu/clustering_actor.rs`: deletes the uncalled `generate_cluster_color`.
- `src/actors/gpu/force_compute_actor.rs`: deletes the never-read `last_step_start` and two actor-address fields that were always `None`.
- `src/actors/gpu/gpu_resource_actor.rs`: deletes the unused `MAX_NODES` / `MAX_GPU_FAILURES` constants and the never-read `last_failure_reset`.
- `src/actors/gpu/pagerank_actor.rs`: deletes an uncalled private `compute_pagerank` helper; the message-handler path is unchanged.
- `src/actors/gpu/semantic_forces_actor.rs`: deletes a duplicate, uncalled FFI extern block and its private `#[repr(C)]` mirrors (call sites use `gpu::kernel_bridge`), the uncalled `config_to_gpu` / `calculate_type_centroids`, four never-read fields and the unused `TypeCentroids`; the relationship-buffer upload passes the canonical type directly instead of through an unsafe slice cast.
- `src/actors/gpu/stress_majorization_actor.rs`: deletes two uncalled private methods.
- `src/actors/graph_service_supervisor.rs`: deletes the uncalled `buffer_message` and the never-read `message_buffer_size`.
- `src/actors/messages/physics_messages.rs`: `ReloadRelationshipBuffer.buffer` carries the canonical `gpu::semantic_forces::DynamicForceConfigGPU` (identical `#[repr(C)]` layout).
- `src/actors/ontology_actor.rs`: deletes three uncalled private methods.
- `src/actors/optimized_settings_actor.rs`: deletes never-read fields, enum variants and a cache-entry hash nobody read; redis-only items are gated `#[cfg(feature = "redis")]` instead of allowed.
- `src/actors/physics_orchestrator_actor.rs`: deletes two uncalled private methods.
- `src/actors/semantic_processor_actor.rs`: deletes four never-read fields.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 (aa01c5536)

This stamp covers the merge of `chore/clippy-sweep` (299aa35bc) with `feat/xr-cloud-parity`, and the lint follow-up aa01c5536. Each branch re-verified this record against its own changes (sections above). The merge itself kept both sides; the only code it combined was test code in `client_coordinator_actor.rs`. aa01c5536 is mechanical: rustfmt, an `async-trait` patch bump, `as_chunks`, and test checks made `const`. No wire format, tag byte, settings key, pose owner, crate boundary or relay rule changed. Decision holds. Verified with `cargo test --workspace --tests` (3,244 passed), xr-client `cargo test --workspace` (516 passed) and clippy `-D warnings` clean in both.
