# Corpus Regeneration Scaling Runbook (R3)

**Risk**: `regenerate_corpus_jsonl` executes in full on every publish/unpublish transition. O(n) cost per operation where n = public-node-count. Power users with 1000+ nodes experience sustained latency spikes and Pod write saturation.

**Ref**: [ADR-054 § corpus](../adr/ADR-054-urn-solid-and-solid-apps-alignment.md#corpus.jsonl) and [Sprint-2 Audit / R3](../audits/2026-04-20-sovereign-mesh-qe-sprint2/06-new-risks.md#r3--corpusjsonl-regenerated-in-full-on-every-transition)

---

## Symptom

Monitor for any of:

- Grafana: `ingest_saga_duration_seconds` p99 > 500ms sustained
- Grafana: Pod write errors (429 Conflict) in `solid_proxy_put_errors_total`
- Logs: "corpus regen took Xms" entries > 1000ms for users with N >> 100 public nodes
- User reports: publish button latency > 2 seconds

---

## Diagnosis Tree

**1. Is corpus generation the bottleneck?**
   - Enable flamegraph profiling on ingest saga hot path
   - Check logs for `regenerate_corpus_jsonl` timing entries
   - If p99 saga time ≈ corpus regen time, proceed to mitigation

**2. What is the user's node count?**
   ```cypher
   MATCH (u:PodOwner {npub: $npub})-[:OWNS]->(n:KGNode)
   WHERE n.visibility = "public"
   RETURN COUNT(n) AS public_node_count
   ```
   - < 100 nodes: skip escalation, current cost is acceptable
   - 100–1000 nodes: trigger Level 1 (debounce)
   - 1000–10000 nodes: trigger Level 2 (incremental) or Level 3 (background)
   - 10000+ nodes: trigger Level 3 (background worker) immediately

**3. Is Pod write saturation occurring?**
   - Check Pod metrics: PUT latency, concurrency limits
   - Verify corpus.jsonl size: `wc -l corpus.jsonl` and file size
   - If size > 10MB, plan Level 4 (pagination)

---

## Mitigation Ladder

### Level 1 — Debounce (Low Lift, ~1 day)

**When**: p99 saga duration > 500ms AND public_node_count > 100

**Action**: Wrap `maybe_regenerate_corpus` in a coalescing scheduler using `tokio::sync::mpsc` with a 5-second debounce window.

- Bursts of publish calls within 5s → single corpus regen
- Trade-off: crawlers see corpus.jsonl up to 5s stale
- Implementation: `src/sovereign/visibility.rs` wraps call in `debounce_corpus_regen(user, 5s)`

**Metrics to add**:
```rust
corpus_regen_debounce_skipped_total: u64   // incremented when burst window coalesces
corpus_regen_debounce_window_seconds: f64  // configurable, default 5s
```

**Acceptance**: p99 saga duration < 500ms for users with < 1000 nodes.

---

### Level 2 — Incremental Write (Medium Lift, ~3 days)

**When**: p99 saga > 2s sustained OR Pod 429 errors present

**Action**: Switch corpus.jsonl from full-rewrite to append-and-tombstone.

**Format**:
```jsonl
{"@id":"visionclaw:owner:npub/kg/abc123","@type":"urn:solid:Note","content":"..."}
{"@id":"visionclaw:owner:npub/kg/def456","@deleted":true}
```

- Publish → append one line (50–100 bytes)
- Unpublish → append tombstone line (20 bytes)
- Compaction: consolidate weekly, rebuild cleanly
- Consumers: filter `@deleted: true` at read time

**Metrics to add**:
```rust
corpus_jsonl_size_bytes: gauge          // monitor growth before compaction
corpus_lines_total: gauge               // total lines including tombstones
corpus_compaction_duration_ms: histogram // compaction job timing
```

**Acceptance**: Pod PUT latency < 200ms even with 1000 active publishes/day.

---

### Level 3 — Background Worker (High Lift, ~1 week)

**When**: public_node_count > 10000 OR p99 saga > 2s unresolved after Level 2

**Action**: Move corpus regen out of saga hot path entirely.

- Publish saga: sets dirty flag in Neo4j (`corpus_dirty = true` on user node)
- Dedicated worker: polls every 60s, rebuilds for flagged users, clears flag
- Trade-off: corpus.jsonl staleness window = 60s (configurable)

**Implementation**:
```rust
// in ingest_saga, instead of regenerating:
user_node.set_property("corpus_dirty", true)?;

// new background task (runs in separate tokio task):
spawn_corpus_worker(interval: Duration::from_secs(60), {
    loop {
        let dirty_users = db.query("MATCH (u:PodOwner {corpus_dirty: true}) RETURN u").await?;
        for user in dirty_users {
            regenerate_corpus_jsonl(&user).await?;
            user.set_property("corpus_dirty", false)?;
        }
        tokio::time::sleep(interval).await;
    }
});
```

**Metrics to add**:
```rust
corpus_worker_iteration_duration_ms: histogram
corpus_dirty_users_count: gauge
corpus_worker_staleness_window_seconds: gauge
```

**Acceptance**: ingest saga p99 < 300ms; corpus.jsonl up-to-date within 60s.

---

### Level 4 — Pagination (If Needed)

**When**: corpus.jsonl > 50MB or single Pod PUT times out

**Action**: Split corpus.jsonl into shards.

- `corpus-0.jsonl`, `corpus-1.jsonl`, … (each ≤ 10MB)
- `corpus-index.jsonl` lists shards and their contents
- Clients fetch index first, then fetch shards as needed
- Reduces single PUT size and enables parallel uploads

**Acceptance**: no single Pod PUT > 20MB.

---

## Triggers

| Metric | Threshold | Action |
|--------|-----------|--------|
| `ingest_saga_duration_seconds` (p99) | > 500ms | Apply Level 1 |
| `ingest_saga_duration_seconds` (p99) | > 2s sustained | Apply Level 2 |
| `public_node_count` (per user) | > 10000 | Apply Level 3 |
| `corpus_jsonl_size_bytes` | > 50MB | Apply Level 4 |
| `solid_proxy_put_errors_total` | rate > 0.1/s | Immediate Level 2 or 3 |

---

## Prevention

**Alert (Grafana)**
```
histogram_quantile(0.99, rate(ingest_saga_duration_seconds_bucket[5m])) > 0.5
```

**Dashboard**: Add panels for:
- Saga duration by node_count percentile bucket
- corpus.jsonl size trend
- Debounce/regen event rate (once metrics added)
- Pod write latency (solid_proxy)

**Capacity Planning**: Forecast node growth for top users; apply Level 1 proactively at count > 500.

---

## Rollback

If any level degrades performance:

1. **Level 1**: Disable debounce via config (instant, no code change needed if configurable)
2. **Level 2**: Revert to full-rewrite; discard appended lines (requires rebuild of corpus)
3. **Level 3**: Disable worker, resume synchronous regen in saga (instant)
4. **Level 4**: Merge shards back into single file; keep index as documentation

---

## Acceptance Criteria

Incident resolved when:

- User-visible publish latency ≤ 500ms p99 (measured from client submit → response)
- Pod write error rate = 0 for ≥1 hour
- corpus.jsonl staleness ≤ 5s (Level 1) or 60s (Level 3)
- All relevant metrics in Grafana dashboard, alerting active

---

## Reference Files

- Audit finding: `docs/audits/2026-04-20-sovereign-mesh-qe-sprint2/06-new-risks.md#r3`
- ADR: `docs/adr/ADR-054-urn-solid-and-solid-apps-alignment.md#corpus.jsonl`
- Source: `src/sovereign/visibility.rs:360-379` (maybe_regenerate_corpus)
- Source: `src/services/ingest_saga.rs:669-765` (saga corpus regen call)
- Metrics: `src/services/metrics.rs` (add corpus_* metrics per levels above)
