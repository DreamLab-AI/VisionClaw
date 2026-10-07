---
id: ADR-2045
title: Remove the dead ActorLifecycleManager supervision machinery
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 3b3ee7779f37c5cc9a7ec30b7928428e78e8f87a
verified_paths: [src/actors/mod.rs, src/actors/graph_service_supervisor.rs, crates/visionclaw-actors/src/supervisor.rs, tests/orchestration_improvements_test.rs]
owner: jjohare
review_trigger: a new supervision requirement that GraphServiceSupervisor cannot express
repo: visionclaw
domain: BASELINE-architecture
---

# ADR-2045 — Remove the dead ActorLifecycleManager supervision machinery

## Context

Diagrams VC-02.4/2.5/2.6/2.7 flag two complete supervision mechanisms alongside the live
`GraphServiceSupervisor` (`src/actors/graph_service_supervisor.rs`), neither of which runs.
(A) generic `SupervisorActor` (`src/actors/supervisor.rs`): `SupervisorActor::new` called only
from its own `#[cfg(test)]` module; `InitiateGracefulShutdown` never sent in `src/`; its
`SupervisionStrategy::Escalate` arm (`:344-349`) only `warn!`s and returns — it has no parent
field, so escalation cannot happen by construction. (B) `ActorLifecycleManager`
(`src/actors/lifecycle.rs:17-207`) plus `initialize_actor_system`/`shutdown_actor_system` (`:280`,
`:285`), re-exported at `src/actors/mod.rs:102-103`, never called anywhere: a complete parallel
`PhysicsOrchestratorActor`/`SemanticProcessorActor` pair with its own `SupervisionStrategy` and
health monitor. Verification found a third fact the diagrams did not record: the live
`GraphServiceSupervisor` imports `ActorFailed` and the `SupervisorActor` type itself from
`src/actors/supervisor.rs` (`:44`, `:442`, `:805`, `:1427`) for its own (also-unwired) escalation
plumbing, which this task's "do not touch `graph_service_supervisor.rs`" restriction blocks from
being cut.

## Decision

Both mechanisms are dead and the decision is to remove both in full. `src/actors/lifecycle.rs` is
executed now: the file, its `pub mod lifecycle;` declaration and its `pub use lifecycle::{...}`
re-export block in `src/actors/mod.rs` are deleted outright (no stub, no shim), replaced with an
ADR-2045 comment in the house style (`src/handlers/mod.rs:44-53`). `src/actors/supervisor.rs`
is **not** deleted in this change: `graph_service_supervisor.rs` — explicitly out of this task's
scope — has a real, non-test compile-time dependency on `SupervisorActor` (as the type of its
`parent_supervisor` field and `SetParentSupervisor::parent`) and on `ActorFailed` (constructed and
sent at `:805` when `parent_supervisor` is `Some`). Per the verify-before-delete rule ("if any grep
shows a real caller outside a `#[cfg(test)]` block or outside the file itself, STOP and report it
instead of deleting"), this is exactly that case, so `supervisor.rs` is left in place pending a
follow-up that touches `graph_service_supervisor.rs` (owned by vc-core, not this task) to drop the
coupling. Note for that follow-up: `parent_supervisor` is set only by `SetParentSupervisor`, which
is never sent anywhere in `src/` either — the coupling is itself provably dead at runtime, just not
at compile time, so the eventual fix is to delete `parent_supervisor`, `SetParentSupervisor`, and
the `Escalate` branch that uses them from `graph_service_supervisor.rs`, at which point
`src/actors/supervisor.rs` becomes deletable outright.

## Consequences

- `ActorLifecycleManager`, `initialize_actor_system`, `shutdown_actor_system`, the `ACTOR_SYSTEM`
  static, and `lifecycle.rs`'s own `SupervisionStrategy`/`SupervisionDecision` pair no longer exist;
  any future actor-boot code must use `GraphServiceSupervisor`, the one real supervision path.
- ~~`src/actors/supervisor.rs` … remains in the tree … recorded as outstanding work.~~
  ~~Follow-on work: remove `parent_supervisor`/`SetParentSupervisor`/the dead `Escalate`
  branch from `graph_service_supervisor.rs`, then delete `src/actors/supervisor.rs` entirely
  and its `src/actors/mod.rs:88-90` re-export, closing this ADR's implementation to
  `complete`.~~ **Superseded 2026-09-05 — the follow-on work landed.** `346fff7af` deleted
  `src/actors/supervisor.rs` (`git diff --name-status` reports `D`), the `src/actors/mod.rs`
  re-export is gone (the block at `:107-116` now records the removal instead), and
  `graph_service_supervisor.rs` carries neither `parent_supervisor` nor `SetParentSupervisor`
  — its `Escalate` arm (`:795-806`, variant at `:294`) now states plainly that it is the top
  of the tree and logs-and-stops. `implementation_status` moves to `complete` on that basis.
- `crates/visionclaw-actors/src/supervisor.rs` is now the **only** home of `SupervisorActor`
  (`grep -rn "pub struct SupervisorActor" src/ crates/` → one hit, `:124`), and the crate is
  **not** unused: `src/actors/messages/{graph,ontology,agent,analytics,client}_messages.rs`
  each re-export from `visionclaw_actors::messages::*`, and
  `tests/orchestration_improvements_test.rs:278-280` was repointed to
  `visionclaw_actors::supervisor::{InitiateGracefulShutdown, RegisterActor,
  SupervisionStrategy, SupervisorActor}` when the root copy went. The earlier description of
  it as an "apparently-unused workspace crate" holding a "byte-identical duplicate" was wrong
  at the time and is doubly wrong now — it is the surviving canonical copy.

## Verification

Greps run on the uncommitted working tree at `verified_commit` above; must be re-run at the landing
commit.

```
$ grep -rn "SupervisorActor" src/ crates/ --include=*.rs
```
Hits in `src/`: `src/actors/mod.rs:89` (re-export), `src/actors/supervisor.rs` (definition, impls,
and 2 `SupervisorActor::new(...)` calls both inside `#[cfg(test)] mod tests` at `:500`, `:524`),
`src/actors/graph_service_supervisor.rs:44,442` (import and field-type use — the live-code
dependency that blocks deletion), `src/services/speech_voice_integration.rs:54,117` (a `warn!` log
string only, not a type/value use). `crates/visionclaw-actors/src/supervisor.rs` carries the same
shape (definition + 6 `::new()` calls, all inside `#[cfg(test)]`) — a separate, out-of-scope crate.

```
$ grep -rn "InitiateGracefulShutdown" src/ crates/ --include=*.rs
```
`src/actors/supervisor.rs:119` (definition), `:219,222` (its own `impl Handler`) — no sender
anywhere in `src/`. `crates/visionclaw-actors/src/supervisor.rs` mirrors this plus one send at
`:653`, inside `#[cfg(test)]`.

```
$ grep -rn "ActorLifecycleManager\|initialize_actor_system\|shutdown_actor_system" src/ crates/ --include=*.rs
```
Before deletion: all 7 hits confined to `src/actors/lifecycle.rs` itself and its re-export at
`src/actors/mod.rs:102-103`. No hits under `crates/`. After deletion: 0 hits anywhere.

```
$ grep -rn "actors::lifecycle\|use crate::actors::lifecycle" src/ crates/ --include=*.rs
```
0 hits before and after — nothing imported `lifecycle` by path; all consumption was via the
`src/actors/mod.rs` re-export, which is now removed.

```
$ grep -rn "ACTOR_SYSTEM|Phase5SupervisionStrategy" src/ crates/ --include=*.rs   # (orphan check)
```
Before deletion: confined to `lifecycle.rs` (definition/use) and `mod.rs:104` (re-export alias).
After deletion: 0 hits.

Orphan check for `SupervisionStrategy` (declared in both `supervisor.rs` and the now-deleted
`lifecycle.rs`): `grep -rn "SupervisionStrategy" src/ crates/ --include=*.rs` shows the
`supervisor.rs` copy is used only within `supervisor.rs` itself and its `mod.rs:89` re-export —
distinct from `graph_service_supervisor.rs`'s own unrelated `GraphSupervisionStrategy` enum
(`:288`) — so it was kept as part of the still-live `supervisor.rs` file, not removed standalone.

```
$ cargo check -p visionclaw-server --lib
```
Before this change: 0 errors, 17 warnings (the task brief's stated 4-error
`owl_extractor_service.rs` baseline had already been fixed by its owning lead before this
verification ran). After deleting `lifecycle.rs` and its `mod.rs` re-export: 0 errors, 17 warnings
— identical warning set, `Finished` dev profile both times. No new error or warning introduced.

Working-tree caveat: verification ran on the uncommitted working tree above `verified_commit` and
must be re-run at the landing commit.

```
$ node scripts/adr-index-gen.js docs/adr --check
```
Exits 0 (see repo CI log for this change).

## Verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

The greps the Verification block above deferred to "the landing commit" have now
been run at that commit, and the outstanding work the Consequences recorded is
discharged — so this record moves from `partial` to `complete` and gains
`verified_paths`.

- **`src/actors/supervisor.rs` is gone.** `ls` → *No such file or directory*;
  `git diff --name-status b00c28a0d..HEAD -- src/actors` reports `D` for it and
  for `src/actors/lifecycle.rs`, deleted by `346fff7af`.
- **The re-export is gone.** `grep -n "pub mod supervisor\|pub use supervisor"
  src/actors/mod.rs` → no hit (the only `supervisor` match is the commented-out
  `// pub mod supervisor_voice;` at `:44`). The former `:88-90` re-export is
  replaced by the explanatory block at `:100-116`.
- **The last non-test coupling is gone.** `grep -n
  "parent_supervisor\|SetParentSupervisor\|Escalate"
  src/actors/graph_service_supervisor.rs` returns only the `Escalate` variant
  (`:294`), its handler (`:795-806`) and the comment at `:797-802` recording that
  `SetParentSupervisor` was never sent, so `parent_supervisor` was permanently
  `None`. No field, no message, no live escalation path.
- **`SupervisorActor` has exactly one definition.**
  `grep -rn "pub struct SupervisorActor" src/ crates/` →
  `crates/visionclaw-actors/src/supervisor.rs:124`. The type survived the
  deletion in the crate that owns the actor layer, which is the intended
  direction of travel under ADR-2005, not an orphan.
- **The test that used it was repointed, not deleted.**
  `tests/orchestration_improvements_test.rs:278-280` imports
  `visionclaw_actors::supervisor::{InitiateGracefulShutdown, RegisterActor,
  SupervisionStrategy, SupervisorActor}`, with the reason at `:276`. Landed in
  `b0bc275f6`.
- **The whole workspace builds, tests included.**
  `cargo check --workspace --all-targets` → **exit 0** (5m32s, warnings only).
  This is the check that matters for a deletion ADR: `cargo check -p
  visionclaw-server` alone does not compile test targets, so it cannot see a
  test that still references a deleted type.

## Re-verification — 2026-10-02 at c0906ed6e201dd09b3e9baab642c0b0b67adca88

`c0906ed6e` adds one forwarding handler to `graph_service_supervisor.rs` (`UpdateChainPayments` to `GraphStateActor`, the same shape as `UpdateBotsGraph`). It does not touch `parent_supervisor`, `SetParentSupervisor` or the `Escalate` branch, so the coupling this record leaves for follow-up is unchanged. The decision holds unchanged.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-actors/src/supervisor.rs`: a needless borrow and an unused test import removed; `src/actors/graph_service_supervisor.rs`: `UpdateSimulationParams` is boxed in the two message enums and unboxed when forwarded; auto-trigger scheduling passes the fn pointer directly; `src/actors/mod.rs`: `PhysicsState`'s field-wise `Default` becomes `#[derive(Default)]`; `tests/orchestration_improvements_test.rs`: `vec!` to array; `(0 + poll)` to `poll`. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `crates/visionclaw-actors/src/supervisor.rs`: deletes the uncalled `should_restart` and the never-read `ActorState.session_id` / `RestartAttempt.supervisor_name` fields.
- `src/actors/graph_service_supervisor.rs`: deletes the uncalled `buffer_message` and the never-read `message_buffer_size`.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**
