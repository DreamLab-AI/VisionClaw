# 02 — B2 BRIDGE_TO ↔ kind-30100 Fan-out (ADR-050 §server-identity + ADR-051 §audit)

**Status: CLOSED.**

## Artefact inventory

- **Service**: `src/services/bridge_edge.rs` (~640 LoC).
- **New field**: `BridgeEdgeService.server_nostr: Option<Addr<ServerNostrActor>>`
  at :195; defaults to `None`. Builder method
  `with_server_nostr(addr)` at :218-221 attaches the actor handle. The
  `None` branch keeps pre-Sprint-2 behaviour byte-for-byte (promote still
  commits to Neo4j; the audit event is a sidecar).
- **Message**: `SignBridgePromotion` from
  `src/actors/server_nostr_actor` — imported at :36. Same actor / same
  message used by existing power-user provisioning audit trail.
- **Metrics**: two new Prometheus counters
  (`src/services/metrics.rs:110-114` fields, :256-266 registration,
  :352-353 struct init):
  - `bridge_kind30100_signed_total` — successful sign+publish.
  - `bridge_kind30100_errors_total` — mailbox error OR signing error.

## Ordering invariant

The Cypher MERGE that materialises `:BRIDGE_TO` runs at
`src/services/bridge_edge.rs:391-395` with `with_context(...)?` — a
propagating error. Only **after** that `Ok` does the code reach the
audit-fan-out block at :421-472.

This means the source-of-truth in Neo4j is committed before any Nostr
event is signed, so:

- If signing succeeds: edge + event both exist.
- If signing fails: edge exists, event is missing, **error counter
  increments** — operators can reconcile from the counter + Neo4j query.
- Promote never fails because of a downstream signing error. The `Ok(true)`
  return at :474 sits outside the fan-out block, so every code path
  inside the fan-out falls through to it.

This is the ADR-050 §server-identity contract: "auditability is
additive, never blocking."

## Fan-out arm walk-through (:431-472)

```
if let Some(addr) = self.server_nostr.as_ref() {
    // Build message from the candidate's scored signals.
    let msg = SignBridgePromotion {
        from_kg:  candidate.kg_iri.clone(),
        to_owl:   candidate.owl_class_iri.clone(),
        signals:  candidate.signals.to_vec(),      // flat Vec<f64>
    };
    match addr.send(msg).await {
        Ok(Ok(event)) => {                         // actor returned signed event
            info!("signed: event_id={}", event.id.to_hex());
            prom.bridge_kind30100_signed_total.inc();
        }
        Ok(Err(e)) => {                            // signing failed inside the actor
            error!("signing failed: {}", e);
            prom.bridge_kind30100_errors_total.inc();
        }
        Err(e) => {                                // actor mailbox failed
            error!("mailbox failed: {}", e);
            prom.bridge_kind30100_errors_total.inc();
        }
    }
}
```

Both error arms increment the same counter — sum of errors is
operationally observable; discriminating cause is available in logs at
the call site (`error!` lines at :450-455 and :461-466). That is a
deliberate design choice: the counter is for alerting, the log is for
forensics.

## Test coverage (`tests/bridge_signing_fanout.rs`, 333 LoC, 5 tokio tests)

| Test | LoC | Scenario |
|------|-----|----------|
| `with_server_nostr_setter_is_additive` | :108-135 | Builder method composes with `with_prom`; default path unaffected. |
| `promote_dispatches_kind30100_and_increments_signed_counter` | :136-171 | End-to-end: actor receives `SignBridgePromotion`, counter +=1. |
| `promote_without_server_nostr_does_not_dispatch` | :172-221 | No actor attached ⇒ no mailbox send; promote still returns `Ok(true)`. |
| `promote_tolerates_actor_mailbox_failure_best_effort` | :222-270 | Actor dropped before send ⇒ `bridge_kind30100_errors_total` +=1; promote returns `Ok(true)`. |
| `promote_preserves_monotonic_invariant_with_fanout_wired` | :271-325 | Re-score with lower confidence is rejected by the Cypher `ON MATCH` clause; counter does not double-fire on a no-op. |
| `error_arm_types_compile` | :326-332 | Compile-time test that both inner error arms are reachable. |

## Verdict

Closed. The invariant "promotion ⇒ audit trail" ships with the correct
ordering, the best-effort semantics match ADR-050, and the Prometheus
counters give operators a direct handle on drift between Neo4j and the
Nostr relay. One residual observability concern: the error counter
aggregates mailbox failures with signing failures. Granular discrimination
requires the structured log, which is fine for a debt-payoff sprint but
should get per-reason labels in a later iteration (not a blocker).
