---
id: ADR-2148
title: Enable local typed decisions and expose reviewed edge suggestions
date: 2026-10-29
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 772039bd172027af034d64befacca1ebcf0bbc04
verified_paths: [docker-compose.unified.yml, crates/visionclaw-jev/src/edge.rs, src/handlers/jev_handler.rs, crates/visionclaw-jev/src/wire.rs]
owner: operator
review_trigger: "labelled confidence calibration setting the automatic thresholds (VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE); the System One route moving off the shared agentbox proxy; any graph write or proposal approval driven by /api/jev/edge"
repo: visionclaw
---

# ADR-2148 — Enable local typed decisions and expose reviewed edge suggestions

## Context

The twelve adapters existed but the deployment supplied no flags or endpoint.
The edge meaning adapter's only host caller was an unused legacy enrichment
service, which also writes classes. Enabling that writer would widen this
rollout beyond advisory model integration.

## Decision

Both Compose application profiles enable the master switch and select the local
System One façade. The separate Codescape service opts its existing classifier
in against the same endpoint. Environment values stay overridable; no vendor key
or cloud override is introduced. Supervisord inherits the application environment.

The authenticated `POST /api/jev/edge` accepts bounded labels, classes and source
evidence, then returns a fixed-vocabulary suggestion with `authority: none`.
It uses the suggestion threshold and domain/range checks. Existing automatic
edge gap-fill retains its separate automatic threshold. Neither this route nor
the configuration creates graph writes or approves proposals. Automatic
thresholds remain unset pending labelled calibration.

## Consequences

Recreating the host services is required. Flags now reach real service processes;
model failures still fall open. The API can return human-reviewable edge advice
without enabling automatic enrichment. Confidence and accuracy must still be
measured; local health and wire conformance do not establish useful decisions.

## Verification

`cargo test -p visionclaw-jev --locked` covers all adapters plus deployment
resolution, request bounds, vocabulary/domain-range gates and independent review
versus automatic thresholds. `bash scripts/tests/test-dev-inputs.sh` checks the
host rebuild wiring. Handler tests were added for authentication and advisory responses but could
not execute here: no-default-features hits an existing physics test's GPU import;
with `gpu`, three generated `src/utils/ptx/*.ptx` inputs are absent. The server
`cargo check --lib --no-default-features --locked` passed, as did type generation.
The full ADR check still reports 26 pre-existing stale records; the index was
rendered from a temporary gitless copy without claiming that gate passed.
Runtime synthetic Choice/Score/Noul passed through the real Rust HTTP adapter
(`cargo test -p visionclaw-jev --test live_systemone -- --ignored`) in 0.16 seconds
on 2026-10-29. The reader accepts the façade’s untagged answers, while rejecting
explicit wrong types. Deployment has not been recreated.

## Amendment (2026-10-30): the shared proxy replaces the local façade as the default route

Operator decision: every System One consumer takes the TypeSafe Jev backend by default;
local routing stays an explicit opt-in for private data. Compose now merges one
`x-system-one-environment` block (the estate-wide common settings:
`AGENTBOX_SYSTEM_ONE_PROXY=1`, endpoint `http://agentbox:8094/v1/systemone`, model
`jev-latest`, `AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL=0`) into the VisionClaw, loom and
codescape services, and drops their `systemone:8097` / `laya-typed-decisions` /
`*_LOCAL=1` defaults. `VISIONCLAW_JEV_ALLOW_CLOUD`, `LOOM_JEV_ALLOW_CLOUD` and
`CODESCAPE_JEV_ALLOW_CLOUD` default to 1. The proxy is agentbox's (agentbox ADR-2139
amendment of the same date): it listens on the private docker network, never a
published port, holds the vendor key and rewrites `model` to the selected backend, so
one agentbox switch moves every consumer. Thresholds are unchanged; the outcome-changing
ones remain unset. Activation still needs the agentbox rebuild (the network listener)
and then a host `scripts/launch.sh` recreate.

Re-verified at 9373a61fb: the compose hunk is this amendment (common block merged into three services, laya defaults removed), checked with yq explode. Other governed paths unchanged.

## Close-out 2026-10-30

`staged → live`. Both activation conditions named in the 2026-10-30 amendment have happened: the agentbox rebuild
(the network listener) and the host recreate. `verified_commit` moves to `772039bd172027af034d64befacca1ebcf0bbc04`; the governed paths are unchanged.

Runtime provenance: `visionclaw_container` runs `/app/target/dev-runtime/visionclaw-server` (build stamp `features=gpu,ontology,dev-auth`, i.e. the dev profile with the dev-auth bypass compiled in); its wrapper log (`/app/logs/rust.log`) reads `[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, so the responses below come from this commit.

**The flags reach the process.** The container's environment holds the amendment's common block:
`AGENTBOX_SYSTEM_ONE_PROXY=1`, `AGENTBOX_SYSTEM_ONE_ENDPOINT=http://agentbox:8094/v1/systemone`,
`AGENTBOX_SYSTEM_ONE_MODEL=jev-latest`, `AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL=0`, `VISIONCLAW_JEV=1`,
`VISIONCLAW_JEV_ALLOW_CLOUD=1`. `VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE` is empty, so the automatic threshold stays unset
as decided.

**The proxy listens.** From inside the container, `GET http://agentbox:8094/v1/models` lists `jev-latest`
(`owned_by: typesafe`). `GET …/v1/systemone/health` is 404, so that path is not a health route.

**The route answers.** `POST /api/jev/edge` with `source_label`, `target_label`, `source_class`, `target_class` and
`evidence` returned `{"used":false,"reason":"low-confidence"}`, HTTP 200, in 0.24 s. `low-confidence` is produced only
after `client.ask(...)` has returned a model answer, either an `unsupported` choice or a domain/range rejection
(`crates/visionclaw-jev/src/edge.rs:191-204`). Transport and timeout failures carry their own tags
(`crates/visionclaw-jev/src/lib.rs:89-97`). The model was therefore reached and its advice withheld, which is the
advisory behaviour this record decides. Usefulness still awaits calibration (`review_trigger`).
