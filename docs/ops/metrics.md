# VisionClaw Metrics Reference

This document enumerates the 24 Prometheus / OpenMetrics series VisionClaw
exposes on `GET /metrics` (task #18). Series are prefixed with `visionclaw_`.

Endpoint: **GET /metrics** — bare path, no auth, content-type
`application/openmetrics-text; version=1.0.0; charset=utf-8`.

Feature flag: `METRICS_ENABLED=true|false` (default `true`). When disabled,
the endpoint returns a `# metrics disabled` marker so scrapers record a
`scrape_samples_scraped=0` event but stay green.

Registry implementation: [`src/services/metrics.rs`](../../src/services/metrics.rs).

## Auth (ADR-028-ext)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_auth_nip98_success_total` | counter | — | NIP-98 Schnorr auth validated successfully |
| `visionclaw_auth_nip98_failure_total` | counter | — | NIP-98 auth rejected (malformed header or bad signature) |
| `visionclaw_auth_legacy_fallback_total` | counter | — | Fell back to `X-Nostr-Pubkey`+`X-Nostr-Token` (dev only) |
| `visionclaw_auth_anonymous_total` | counter | — | `RequireAuth::optional()` scope with no auth headers |

Use the NIP-98 success/failure ratio to watch for client regressions. A
rising `legacy_fallback_total` in production is a red flag — legacy auth
is rejected when `APP_ENV=production`.

## Pod-first ingest saga (ADR-051)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_ingest_saga_total` | counter | `outcome={complete,pending,failed}` | Terminal outcome count per batch |
| `visionclaw_ingest_saga_pending_nodes` | gauge | — | KGNodes currently carrying `saga_pending=true` |
| `visionclaw_ingest_saga_retry_total` | counter | — | Resumption task attempts on pending nodes |
| `visionclaw_ingest_saga_duration_seconds` | histogram | — | `execute_batch` wall-clock (exp buckets 1ms → ~16s) |

A growing `pending_nodes` gauge with no retry activity suggests the
resumption task is stalled; a high `failed` rate suggests Pod I/O
problems upstream.

## Ingest parser visibility (ADR-051)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_ingest_nodes_public_total` | counter | — | KGNodes ingested with `visibility=public` |
| `visionclaw_ingest_nodes_private_total` | counter | — | KGNodes ingested with `visibility=private` |
| `visionclaw_ingest_wikilink_stubs_total` | counter | — | Private stubs materialised to satisfy a WikilinkRef edge |

## Bridge edge (ADR-051 §bridge)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_bridge_candidates_surfaced_total` | counter | — | `BRIDGE_CANDIDATE` MERGE count |
| `visionclaw_bridge_promotions_total` | counter | — | `BRIDGE_TO` edges created/refreshed (monotonic) |
| `visionclaw_bridge_expired_total` | counter | — | `BRIDGE_CANDIDATE` edges auto-expired (stale + sub-threshold) |
| `visionclaw_bridge_confidence_histogram` | histogram | — | Promotion confidence scores (linear buckets 0.1 → 1.0) |

Watch the ratio `promotions_total / candidates_surfaced_total` to track
broker throughput; watch the confidence histogram tail to confirm the
sigmoid is not clipping.

## Orphan retraction (hygiene lane)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_orphan_wikilinkref_removed_total` | counter | — | Stale `WikilinkRef` edges retracted |
| `visionclaw_orphan_stubs_removed_total` | counter | — | Private stubs deleted after losing all inbound refs |

## Pod client

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_pod_put_total` | counter | `container={public,private,config}` | Pod PUT requests, partitioned by target container |
| `visionclaw_pod_put_errors_total` | counter | — | Pod PUTs returning an error |
| `visionclaw_pod_move_total` | counter | — | Pod MOVE (container re-parent) operations |

## Server-Nostr (ADR-050 §server-identity)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_server_nostr_signed_total` | counter | `kind={30023,30100,30200,30300}` | Server-signed Nostr events per kind |
| `visionclaw_server_nostr_broadcast_errors_total` | counter | — | Sign/broadcast failures (relay errors, sign errors) |

## solid-pod-rs (ADR-053)

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `visionclaw_solid_pod_rs_requests_total` | counter | — | Requests routed into the embedded solid-pod-rs subsystem |
| `visionclaw_solid_pod_rs_wac_denied_total` | counter | — | Requests denied by WAC evaluation |

Parent ADRs: [ADR-028-ext](../adr/028-nostr-auth.md),
[ADR-050](../adr/050-sovereign-schema.md),
[ADR-051](../adr/051-sovereign-transitions.md),
[ADR-053](../adr/053-solid-pod-rs.md).

## Scraping

Prometheus job example (see also `config/staging/prometheus.yml`):

```yaml
scrape_configs:
  - job_name: visionclaw
    metrics_path: /metrics
    static_configs:
      - targets: ["visionclaw:4000"]
```

The staging stack (`config/staging/docker-compose.staging.yml`) ships
Prometheus + Grafana with a provisioned
`Sovereign Mesh — Overview` dashboard covering every metric above.
