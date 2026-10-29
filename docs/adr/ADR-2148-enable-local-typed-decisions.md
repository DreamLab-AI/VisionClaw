---
id: ADR-2148
title: Enable local typed decisions and expose reviewed edge suggestions
date: 2026-10-29
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 0d01be4a67da89fbc978648e58934106a633b167
verified_paths: [docker-compose.unified.yml, crates/visionclaw-jev/src/edge.rs, src/handlers/jev_handler.rs, crates/visionclaw-jev/src/wire.rs]
owner: operator
review_trigger: First host rebuild and labelled confidence calibration
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
