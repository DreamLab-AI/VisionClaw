# ADR-084 — Cloud Infrastructure Mapping for Kit Consumers

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-012 G2, F4, F18, F19, F20, F27 |
| Companion ADRs | ADR-073, ADR-080 D7, ADR-081, ADR-082, ADR-083, ADR-085 |
| Companion PRDs | PRD-011, PRD-012 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` BC-MESH-DREAMLAB-CONSUMER |
| Affected repos | `dreamlab-ai-website` (consumer), `nostr-rust-forum` (kit upstream) |

## Context

PRD-012 G2 mandates that DreamLab's existing Cloudflare resources keep their identity — same D1 database IDs, same KV namespace IDs, same R2 bucket names, same Durable Object class, same custom domains, same wrangler routes — through the transition from `community-forum-rs/` subtree to `forum-config/` consumer package.

This is **not** a migration ADR (ADR-083 owns the cutover mechanics). This is a **mapping ADR** specifying how cloud resources flow from the legacy per-crate `community-forum-rs/crates/*/wrangler.toml` shape into the kit-consumer `forum-config/deploy/*.wrangler.toml` shape with **zero resource identity churn**.

Why it matters:
- Resource IDs (CF KV namespace `id`, D1 `database_id`) are CF-internal pointers; renaming or recreating them implies data migration. Avoiding rename = avoiding migration = avoiding cutover risk (per ADR-083 R3 HIGH).
- Custom domains (`api.dreamlab-ai.com`, `pods.dreamlab-ai.com`) are bound to existing workers; preserving them across transition requires careful wrangler route handoff.
- Secrets are stored per-worker in CF Workers Secrets; preservation requires re-binding to the renamed worker (or keeping the worker name unchanged).
- Cron triggers (the `*/5 * * * *` keep-warm on relay-worker) are worker-scoped; preservation requires the trigger declaration in the new wrangler.toml.

The QE fleet's Q4 §G1 finding ("VisionClaw has NO Rust CI workflow") + the cutover risk surface in ADR-083 + the operator's natural conservatism dictate: the kit-consumer transition MUST be fully reversible. Resource preservation is the safest reversal mechanism.

## Decision

### D1 — Resource identity preservation matrix

For every cloud resource, the consumer's wrangler manifest declares the SAME identifier as the legacy crate. No renames, no recreates.

#### D1 databases

| Legacy crate | wrangler.toml binding | Database name | Database ID | Consumer wrangler.toml |
|--------------|----------------------|---------------|-------------|------------------------|
| `community-forum-rs/crates/auth-worker/wrangler.toml` | `DB` | `dreamlab-auth` | `e3981999-...` | `forum-config/deploy/auth-worker.wrangler.toml` PRESERVED |
| `community-forum-rs/crates/relay-worker/wrangler.toml` | `DB` | `dreamlab-relay` | `97c77d23-...` | `forum-config/deploy/relay-worker.wrangler.toml` PRESERVED |

#### KV namespaces

| Legacy crate | binding | namespace ID | Used in consumer |
|--------------|---------|--------------|-------------------|
| auth-worker | `SESSIONS` | `<existing-id>` | auth-worker.wrangler.toml |
| auth-worker | `POD_META` | `<existing-id>` | auth-worker.wrangler.toml + (read-only mirror) pod-worker.wrangler.toml as `ADMIN_KV_RO` |
| auth-worker | `ADMIN_KV` | `<existing-id>` | auth-worker.wrangler.toml |
| auth-worker | `NIP98_REPLAY` | `<existing-id>` | shared by all 4 NIP-98-consuming workers (per PRD-010 F20) |
| pod-worker | `POD_META` | (same as auth) | pod-worker.wrangler.toml as `ADMIN_KV_RO` |
| pod-worker | `NIP98_REPLAY` | (same as auth) | pod-worker.wrangler.toml |
| relay-worker | `NIP98_REPLAY` | (same as auth) | relay-worker.wrangler.toml |
| search-worker | `SEARCH_CONFIG` | `<existing-id>` | search-worker.wrangler.toml |
| search-worker | `NIP98_REPLAY` | (same as auth) | search-worker.wrangler.toml |
| preview-worker | `RATE_LIMIT` | `<existing-id>` | preview-worker.wrangler.toml |

#### R2 buckets

| Legacy | bucket name | Used in consumer |
|--------|-------------|-------------------|
| auth + pod | `dreamlab-pods` | auth-worker + pod-worker (binding: `PODS`) |
| search | `dreamlab-vectors` | search-worker (binding: `VECTORS`) |

#### Durable Objects

| Legacy class | DO name | Consumer reference |
|--------------|---------|---------------------|
| `NostrRelayDO` | `RELAY` | relay-worker.wrangler.toml — **kit re-exports the same class name** so existing DO IDs continue to resolve |

The kit's `nostr-bbs-relay-worker` crate exports the relay class as `pub use crate::relay_do::NostrRelayDO;` (per ADR-085 D6) ensuring the class symbol matches the legacy crate's export — wrangler binds to the same DO namespace.

### D2 — Custom domain preservation

| Domain | Worker (legacy) | Worker (consumer) | Wrangler route declaration |
|--------|-----------------|-------------------|---------------------------|
| `api.dreamlab-ai.com/*` | dreamlab-auth-api | dreamlab-auth-api (renamed inside consumer; route preserved) | `[[routes]] pattern = "api.dreamlab-ai.com/*" zone_name = "dreamlab-ai.com"` |
| `pods.dreamlab-ai.com/*` | dreamlab-pod-api | dreamlab-pod-api | same |
| `dreamlab-nostr-relay.solitary-paper-764d.workers.dev/*` | dreamlab-nostr-relay | dreamlab-nostr-relay | (CF default workers.dev URL; preserved automatically) |
| `search.dreamlab-ai.com/*` | dreamlab-search-api | dreamlab-search-api | same |
| `dreamlab-link-preview.solitary-paper-764d.workers.dev/*` | dreamlab-link-preview | dreamlab-link-preview | (workers.dev URL; preserved) |

CF zero-downtime route migration (per Cloudflare Workers Routing docs): when the consumer worker deploys with the same `name` field as the legacy worker, CF replaces the live worker atomically. No DNS change, no route reassignment.

### D3 — Worker name preservation

The consumer wrangler manifests use the **same `name` field** as the legacy crates:

| Legacy crate | wrangler `name` | Consumer wrangler `name` |
|--------------|-----------------|--------------------------|
| auth-worker | `dreamlab-auth-api` | `dreamlab-auth-api` |
| pod-worker | `dreamlab-pod-api` | `dreamlab-pod-api` |
| relay-worker | `dreamlab-nostr-relay` | `dreamlab-nostr-relay` |
| search-worker | `dreamlab-search-api` | `dreamlab-search-api` |
| preview-worker | `dreamlab-link-preview` | `dreamlab-link-preview` |

This is what enables D2 zero-downtime route handoff and D4 secrets preservation.

### D4 — Secrets preservation

CF Workers Secrets are bound per-worker. Because D3 preserves worker names, all existing secrets transfer automatically:

| Worker | Existing secrets (preserved unchanged) |
|--------|------|
| dreamlab-auth-api | `RP_ID`, `EXPECTED_ORIGIN`, `MANAGEMENT_API_KEY`, ... |
| dreamlab-pod-api | (similarly) |
| dreamlab-nostr-relay | (similarly) |
| dreamlab-search-api | `RVF_STORE_KEY`, `ADMIN_PUBKEYS`, ... |

NEW secrets (added at X1, ahead of cutover):
- `MESH_FEDERATION_PRIVKEY` (per ADR-081 D2 Tier-2 custody) — `wrangler secret put MESH_FEDERATION_PRIVKEY` against dreamlab-nostr-relay
- `WELCOME_BOT_PRIVKEY` — against dreamlab-auth-api
- `OPERATOR_PRIVKEY` — against dreamlab-auth-api (the substrate operator key per PRD-010 F1)

### D5 — Cron trigger preservation

```toml
# forum-config/deploy/relay-worker.wrangler.toml
[triggers]
crons = ["*/5 * * * *"]   # keep-warm cron preserved from legacy
```

### D6 — Vars preservation

Existing `[vars]` declarations preserved literally per legacy wrangler.toml. New vars added (per `dreamlab.toml [features]`) are additive.

```toml
# forum-config/deploy/auth-worker.wrangler.toml — illustrative
name = "dreamlab-auth-api"
main = "../../target/wasm32-unknown-unknown/release/nostr_bbs_auth_worker.wasm"
compatibility_date = "2025-09-01"

[vars]
EXPECTED_ORIGIN = "https://dreamlab-ai.com"
RP_ID = "dreamlab-ai.com"
DREAMLAB_TOML_PATH = "../dreamlab.toml"
DEPLOYMENT_NAME = "DreamLab Community Forum"

[[d1_databases]]
binding = "DB"
database_name = "dreamlab-auth"
database_id = "<existing-id>"

[[kv_namespaces]]
binding = "SESSIONS"
id = "<existing-id>"

[[kv_namespaces]]
binding = "POD_META"
id = "<existing-id>"

[[kv_namespaces]]
binding = "ADMIN_KV"
id = "<existing-id>"

[[kv_namespaces]]
binding = "NIP98_REPLAY"
id = "<existing-id-shared>"  # shared per PRD-010 F20

[[r2_buckets]]
binding = "PODS"
bucket_name = "dreamlab-pods"

[[routes]]
pattern = "api.dreamlab-ai.com/*"
zone_name = "dreamlab-ai.com"
```

### D7 — CORS-allowed-origins preservation

Per `02-forum-surfaces.md` §10 #10: drift exists in the legacy CORS posture (single-origin auth/pod, multi-origin relay/search). PRESERVED VERBATIM in consumer wrangler manifests. ADR-082-style anti-drift lint detects unexpected CORS changes.

### D8 — `dreamlab.toml` distribution to workers

Each CF Worker boots and reads its `dreamlab.toml`. Mechanism:

**Option α — Bake-into-bundle**: `dreamlab.toml` is committed in `forum-config/`; wrangler build embeds it into each worker's WASM bundle as `include_str!("../dreamlab.toml")`.

*Selected*. Lowest runtime cost; immutable post-deploy; matches existing fixture-bundling pattern.

**Option β — Fetch from KV**: `dreamlab.toml` written to a special KV namespace; workers read at boot.

*Rejected*: introduces a runtime dependency on KV availability for boot; harder to audit since changes don't show in git.

**Option γ — Wrangler vars**: each `dreamlab.toml` field becomes a wrangler `[vars]` entry.

*Rejected*: TOML structure (nested tables, arrays) doesn't map cleanly to flat var space; lossy.

### D9 — Pre-deploy validation gate

Pre-deploy CI step (per PRD-012 F4):

```bash
# .github/workflows/workers-deploy.yml step
- name: Validate cloud resource ID alignment
  run: |
    set -euo pipefail
    
    # Compare consumer manifest IDs against live CF state
    for worker in auth pod relay search preview; do
      MANIFEST_PATH=forum-config/deploy/${worker}-worker.wrangler.toml
      
      # Extract IDs from manifest
      D1_ID=$(grep -A1 'database_name = "dreamlab' $MANIFEST_PATH | grep database_id | awk '{print $3}' | tr -d '"')
      
      # Compare against live CF
      if [ -n "$D1_ID" ]; then
        wrangler d1 list --json | jq -e ".[] | select(.uuid == \"$D1_ID\")" > /dev/null \
          || (echo "FAIL: D1 ID $D1_ID not found in live CF state for worker $worker" && exit 1)
      fi
      
      # Similar for KV namespaces
      grep '^\[\[kv_namespaces\]\]' -A2 $MANIFEST_PATH | grep '^id' | awk '{print $3}' | tr -d '"' | while read kv_id; do
        wrangler kv namespace list --json | jq -e ".[] | select(.id == \"$kv_id\")" > /dev/null \
          || (echo "FAIL: KV ID $kv_id not found" && exit 1)
      done
    done
    
    echo "PASS: all resource IDs align with live CF state"
```

This gate runs in `workers-deploy.yml` BEFORE `wrangler deploy`. Mismatch = block merge.

### D10 — Schema parity assertion

PRD-012 R3 HIGH risk: a schema-divergence bug could corrupt user data. Mitigation:

```bash
# .github/workflows/workers-deploy.yml step (pre-deploy)
- name: Schema parity sentinel
  run: |
    # Write known sentinel to D1 via legacy worker; read via new worker stack; assert equality
    SENTINEL=$(uuidgen)
    
    # Via legacy stack
    curl -s -X POST https://api.dreamlab-ai.com/api/_internal/sentinel/write \
      -H "X-Sentinel-Token: $SENTINEL_TOKEN" \
      -d "{\"sentinel\": \"$SENTINEL\"}"
    
    # Via new stack (staging URL)
    READ_BACK=$(curl -s https://staging-api.dreamlab-ai.com/api/_internal/sentinel/read \
      -H "X-Sentinel-Token: $SENTINEL_TOKEN")
    
    if [ "$READ_BACK" != "$SENTINEL" ]; then
      echo "FAIL: schema parity violated — staging stack returned different bytes"
      exit 1
    fi
    
    echo "PASS: schema parity confirmed"
```

This sentinel test runs in staging only; the new worker stack must include a temporary sentinel endpoint that reads from D1 directly. Removed post-cutover.

## Consequences

### Positive

- **Zero data migration**: existing user data, mod actions, profiles, sessions, ACLs all retained without copy. Cutover risk surface dramatically reduced.
- **Zero DNS churn**: existing custom domains continue to resolve without TTL drift or stale-cache risk for users.
- **Secrets preserved automatically** by D3 worker name preservation. No secret rotation triggered by the transition.
- **Resource identity is reversible**: rolling back to legacy stack at any point in T₃-T₆ continues to bind to the same resources.
- **Cost stable**: no resource provisioning during transition; existing CF billing footprint stays.

### Negative

- **Worker name re-binding has a brief race window**: when wrangler deploys to an existing name, there's a sub-second window where requests in flight may hit the old or new code. Mitigation: route updates are atomic in CF Workers; existing sessions handle the worker swap transparently.
- **Schema drift detection is sentinel-based**: the D10 sentinel test catches gross schema breaks but not subtle column-default differences. Mitigation: PRD-012 F8 Sprint Carry-Over Fixture Suite covers behavioural-level parity.
- **Anti-drift CORS lint may flag the existing legacy drift as a violation**: the inconsistent CORS posture (auth single-origin vs relay multi-origin) is preserved verbatim per D7, but a strict reviewer may push for unification. Decision: keep drift documented + flag for follow-up sprint, do NOT unify in PRD-012 (changes user-visible behaviour, out of scope).
- **TOML bake-into-bundle (D8α)** means TOML changes require redeploy. Operators making admin-pubkey-list changes can't hot-reload. Acceptable; admin changes are infrequent.

### Neutral

- **No need for CF dashboard manual changes**: D2 + D3 ensure wrangler-driven deploys handle route + name handoff. Operator does not log into CF dashboard mid-cutover.

## Alternatives Considered

### Alt-A — Provision new CF resources, migrate data
Create new D1 / KV / R2 resources with kit-friendly names; migrate data via export/import.

*Rejected*: introduces multi-hour migration window with read-only old-stack period; data-divergence risk during ramp; complete reverse of ADR-083 D3 share-storage strategy.

### Alt-B — Rename existing resources via CF dashboard
Use CF dashboard to rename `dreamlab-auth` D1 to `nostr-bbs-auth`, etc., to match kit conventions.

*Rejected*: CF doesn't support resource rename; would require recreate. Same risk as Alt-A.

### Alt-C — Run kit workers under new names; cutover via DNS
Deploy kit workers under names like `dreamlab-auth-v2`; flip DNS at cutover.

*Rejected*: DNS TTL adds cutover delay; users hit cached old DNS; not zero-downtime.

### Alt-D — Single mega-worker
Consolidate 5 workers into 1 CF Worker for the kit consumer.

*Rejected*: CF Worker bundle size limit (1 MiB free tier); 5-worker architecture is the existing sharding.

### Alt-E — Wrangler `[env]` for staging vs production
Keep one wrangler.toml with `[env.production]` and `[env.staging]` blocks.

*Considered for follow-up*. Current legacy uses separate wrangler.toml per environment. Could consolidate as a Phase 2 cleanup. Not blocking for cutover.

## Implementation notes

### Pre-cutover audit script

```bash
#!/bin/bash
# scripts/audit-cf-resource-mapping.sh
# Run pre-T₃ to confirm all resource IDs are intact.
set -euo pipefail

cd $REPO_ROOT/forum-config/deploy

for worker_manifest in *.wrangler.toml; do
  echo "=== $worker_manifest ==="
  
  # D1 IDs
  echo "  D1:"
  grep '^database_id' $worker_manifest | awk '{print "    " $0}'
  
  # KV IDs
  echo "  KV:"
  grep -B1 '^id = ' $worker_manifest | grep -A1 binding | awk '{print "    " $0}'
  
  # R2 buckets
  echo "  R2:"
  grep '^bucket_name' $worker_manifest | awk '{print "    " $0}'
  
  # Routes
  echo "  Routes:"
  grep '^pattern' $worker_manifest | awk '{print "    " $0}'
done

echo "Compare against live state:"
echo "  wrangler d1 list"
echo "  wrangler kv namespace list"
echo "  wrangler r2 bucket list"
```

### Custom-domain ownership verification

Pre-T₃: confirm CF zone `dreamlab-ai.com` owns the listed routes:

```bash
wrangler route list --zone dreamlab-ai.com | grep -E 'api|pods|search'
```

Expected output matches D2 table.

### Cron trigger drift detection

```bash
# Verify cron preserved
grep -A2 '^\[triggers\]' forum-config/deploy/relay-worker.wrangler.toml
# Should show: crons = ["*/5 * * * *"]
```

### KV namespace shared NIP98_REPLAY

Per PRD-010 F20: a single shared KV namespace for replay-token storage across all 4 NIP-98-consuming workers (auth, pod, relay, search). The wrangler manifests for each worker bind to the SAME KV ID:

```toml
[[kv_namespaces]]
binding = "NIP98_REPLAY"
id = "<single-shared-id>"
```

This MUST be verified in D9 audit script — accidental divergence (each worker bound to its own NIP98_REPLAY) was the Sprint v9 STREAM-B audit gap that this enforcement closes.

### DO `RELAY` class-name continuity

Kit's `nostr-bbs-relay-worker` crate must export the DO class with the EXACT name `NostrRelayDO`:

```rust
// nostr-bbs-relay-worker/src/lib.rs
pub use crate::relay_do::NostrRelayDO;

#[durable_object]
pub struct NostrRelayDO { ... }
```

Wrangler binding:

```toml
[[durable_objects.bindings]]
name = "RELAY"
class_name = "NostrRelayDO"   # EXACT MATCH to kit export

[[migrations]]
tag = "v1"
new_classes = ["NostrRelayDO"]
```

Kit must NOT rename the class to `BbsRelayDO` or similar; existing DO IDs are bound to the class name.

## References

- PRD-012 — DreamLab Website Kit Adoption (G2, F4, F18, F19, F20, F27)
- PRD-011 — VisionFlow Forum Kit Extraction
- PRD-010 — DID:Nostr Mesh Federation (F20 shared NIP98_REPLAY)
- ADR-073 — Mesh topology
- ADR-080 — Forum kit deployment topology (D7 downstream-consumer pattern)
- ADR-081 — Federation key custody & rotation
- ADR-082 — Cross-substrate test fixture sharing
- ADR-083 — Cutover migration pattern (R3 mitigation)
- ADR-085 — `forum-config/` Package Architecture (companion)
- `docs/integration-research/02-forum-surfaces.md` §9 (current cloud stack)
- Cloudflare Workers Routing docs: https://developers.cloudflare.com/workers/runtime-apis/routes-and-domains/
- Cloudflare DO migrations docs: https://developers.cloudflare.com/durable-objects/reference/durable-objects-migrations/
- GitHub repos:
  - https://github.com/DreamLab-AI/dreamlab-ai-website (consumer)
  - https://github.com/DreamLab-AI/nostr-rust-forum (kit upstream — must export DO class with stable name)
