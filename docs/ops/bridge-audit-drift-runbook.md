# Bridge Audit Drift Runbook (R1)

**Risk**: `BridgeEdgeService::promote` commits Neo4j BEFORE signing kind-30100 audit event. Signing failures leave `:BRIDGE_TO` edges without corresponding relay events.

**Ref**: [ADR-055 / R1](../audits/2026-04-20-sovereign-mesh-qe-sprint2/06-new-risks.md#r1--best-effort-kind-30100-audit-drift)

---

## Symptom

Grafana dashboard: `bridge_kind30100_errors_total` counter > 0.

---

## Diagnosis Tree

**1. Nostr relays unreachable?**
- Check logs: `grep -i "nostr_service" logs/`
- Verify env: `NOSTR_RELAY_URLS` is set and URLs are valid
- Test connectivity: `curl -I $NOSTR_RELAY_URL`

**2. `ServerNostrActor` mailbox closed?**
- Check logs for `actor stopped` or `PANIC`
- Search tracing logs for actor termination
- If found, restart the server-nostr sidecar

**3. HS256 signing failure?**
- Validate env: `SERVER_NOSTR_PRIVKEY` must be set and non-empty
- Verify key format (32-byte hex)
- Check logs for cryptographic errors at `src/services/bridge_edge.rs:450-455` or `:461-466`

**4. Relay rate-limiting?**
- Search reqwest trace logs for HTTP 429 responses
- Check relay status page or ops dashboard for rate-limit metrics
- If sustained, switch relay URLs via env update + SIGHUP

---

## Remediation Procedure

**Option A: Small gap (<100 promotions)**
1. Query affected promotions:
   ```cypher
   MATCH (k:KGNode)-[r:BRIDGE_TO]->(o:OntologyClass)
   WHERE r.created_at > $incident_start_ts
   RETURN COUNT(r) AS gap_size
   ```
2. If implementable: use admin CLI `vc-cli bridge-resign --from <ts> --to <ts>` to re-emit audit events on the relay
3. Verify counter returns to 0

**Option B: Large gap (100+ promotions) or sustained errors**
1. Halt new promotions: `BRIDGE_EDGE_ENABLED=false` (env var, SIGHUP to apply)
2. Drain the promotion queue and wait for in-flight requests
3. Restart server-nostr sidecar to clear mailbox state
4. Diagnose the underlying cause (relay infra, key rotation, network partition)
5. Re-enable: `BRIDGE_EDGE_ENABLED=true`
6. Backfill via Option A if necessary

**Option C: Relay infrastructure down**
1. Verify relay reachability: `nak relay list` (Nostr admin kit)
2. Switch relay URLs: update `NOSTR_RELAY_URLS` env var and send SIGHUP
3. Monitor counter for recovery

---

## Prevention

**Alert (Grafana)**
```
rate(bridge_kind30100_errors_total[5m]) > 0.01
```
Fires when sustained at >1 error per 100 seconds.

**New Dashboard Panel**: error counter + relay reachability synthetic monitor (HTTP GET test)

**Weekly Audit** (manual or scripted): cross-reference `:BRIDGE_TO` edges (Neo4j) against broadcast kind-30100 events on canonical relay. Log discrepancies in ops runbook appendix.

---

## Acceptance

Incident closed when:
- Error rate returns to 0 for ≥1 hour AND
- Audit backfill (Option A) completed OR gap documented as known-unreconciled in runbook appendix

---

## Reference Files

- Source: `src/services/bridge_edge.rs:421-472` (promote + error arms)
- Metrics: `src/services/metrics.rs:110-114` (counter definition)
- Error logs: `error!` at lines 450-455, 461-466 include IRIs for forensics
