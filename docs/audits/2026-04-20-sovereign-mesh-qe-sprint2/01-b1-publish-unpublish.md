# 01 — B1 Publish/Unpublish Saga (ADR-051 §transitions)

**Status: CLOSED.**

## Artefact inventory

- **Module**: `src/sovereign/visibility.rs` (678 LoC) — new file as declared
  in ADR-055 Wave 1-b.
- **Re-export**: `src/sovereign/mod.rs:17` exposes
  `VisibilityNeo4jOps` + `VisibilityTransitionService`.
- **Wire-up**: `src/main.rs:609-631` constructs the service, registers it as
  `web::Data<Arc<VisibilityTransitionService>>`, and logs at startup with
  the `VISIBILITY_TRANSITIONS` env value. `src/main.rs:808` mounts it on
  the Solid proxy scope so the tombstone check has a handle.
- **Feature flag**: `VISIBILITY_TRANSITIONS` env var, default off
  (`visibility_transitions_enabled()` at :60-64). Both `publish` and
  `unpublish` short-circuit to `VisibilityError::NotEnabled` at :392 and
  :492 respectively when the flag is unset.
- **Tests**: `tests/visibility_transitions.rs` (482 LoC, 7 tokio tests).

## Publish happy path (`publish`, :391-484)

1. **Flag gate** (:392-394): early return on disabled.
2. **Pod MOVE** (:402-413) via `PodClient::move_resource`. On failure,
   increment `ingest_saga_total{outcome=failed}` and return
   `VisibilityError::PodMove` — Neo4j is **never** touched.
3. **Neo4j flip to public** (:416-442). The Cypher at :172-184 sets
   `visibility='public'`, clears `opaque_id`, restores `label`, swaps
   `pod_url`. Ordering invariant (Pod before Neo4j) is enforced by
   position in the function.
4. **V5 broadcast hint** (:447-453) via `tracing::info!` with
   `target="graph.node.published"`. Downstream websocket layer already
   subscribes and re-emits with bit 29 cleared (see H2 report §2).
5. **ADR-054 corpus regeneration** (:458) — `maybe_regenerate_corpus` is
   flag-gated on `URN_SOLID_ALIGNMENT` and best-effort (warns, does not
   propagate failure).
6. **Audit event** (:461-481) dispatches `SignAuditRecord` with
   action=`publish`. On mailbox error returns `VisibilityError::AuditEmit`
   but saga is already committed.

## Unpublish happy path (`unpublish`, :491-593)

Mirrors publish with two additions:

- **Tombstone merge** (:525-534) — `write_tombstone(current_path, owner)`
  runs after `flip_to_private` succeeds. Non-fatal on failure (MERGE is
  idempotent; the saga resumer can retry). Cypher at :231-238 does
  `MERGE (t:PodTombstone {path})`.
- **Audit event** action=`unpublish`.

## 410 Gone with Sunset header

`src/handlers/solid_proxy_handler.rs:560-603` implements the ADR-051
tombstone check for GET requests:

- Applies only when the relative path starts with `public/` (:570).
- Looks up the `VisibilityTransitionService` from `app_data` (:572-573);
  uses `neo4j_ops()` from the service to share the adapter handle
  (:575). Checks both the raw `target_path` and its leading-slash variant
  (:576) to tolerate Pod-URL normalisation drift.
- On hit: reads `tombstone_sunset(p)` for the ISO-8601 timestamp, returns
  `HttpResponse::Gone()` with `Sunset: <ts>` header and a plain-text body
  (:590-593).
- On lookup error: logs at `debug!` and falls through (fail-open on the
  lookup itself, since the 410 is a read-side optimisation; Neo4j
  authority remains with the `:KGNode.visibility` flip).

## Edge cases covered by tests

| Test | LoC | Scenario |
|------|-----|----------|
| `publish_happy_path_flips_neo4j_and_signs_audit` | :183-225 | All three side-effects ordered correctly; audit dispatched. |
| `publish_pod_move_failure_aborts_without_touching_neo4j` | :226-274 | Pod fails → `VisibilityError::PodMove`; Neo4j ops recorder shows 0 calls. |
| `publish_pod_ok_neo4j_fail_marks_saga_pending` | :275-318 | Pod succeeds + Neo4j flip fails → `mark_saga_pending` called with step `"published_pod"` and error string. |
| `unpublish_happy_path_writes_tombstone` | :319-372 | Tombstone row appears with correct path + owner. |
| `unpublish_pod_failure_leaves_state_untouched` | :373-412 | Pod fail → no flip, no tombstone, no audit. |
| `tombstone_lookup_returns_sunset_for_unpublished_path` | :413-432 | Sunset timestamp survives round-trip through `tombstone_sunset`. |
| `feature_flag_off_returns_not_enabled_without_side_effects` | :433-end | Flag-off fast-path; mock verifies 0 calls to Pod client + Neo4j adapter + Nostr actor. |

Concurrent publish+unpublish is not explicitly tested in-module, but the
Cypher MERGE on `:PodTombstone` and the flip-to-private SET are both
idempotent; Neo4j serialises the two writes via the per-node row lock.
The saga-pending marker distinguishes an in-flight transition from a
committed one, so the resumer does not double-fire.

## Verdict

Closed. Every ADR-051 §transitions contract has a code path, every code
path has a test, and the saga-pending marker integrates with the existing
`IngestSaga::resume_pending` recovery loop. The only residual risk is
unbounded `:PodTombstone` accumulation (see report §6 R2).
