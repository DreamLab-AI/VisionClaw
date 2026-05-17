# JSS → solid-pod-rs Cutover Runbook

> **STATUS: COMPLETED 2026-04-20.** JSS sidecar retired. `solid-pod-rs`
> is now an external crate (`github.com/DreamLab-AI/solid-pod-rs`,
> pinned at `v0.4.0-alpha.1`) and the only Solid Pod backend. The
> `SOLID_IMPL` env var, the shadow comparator, and the
> `solid_proxy_handler` module have all been removed. This document
> is preserved as a historical record of the phased cutover; operators
> should ignore the `SOLID_IMPL` guidance below — there is no flag to
> flip any more.

**Authoritative reference:** [ADR-053](../adr/ADR-053-solid-pod-rs-crate-extraction.md)
§"Phase 3 Integration" + §"Post-parity extraction".

This runbook walks the Pod backend from the legacy JavaScript Solid
Server (JSS) Node sidecar to the in-process `solid-pod-rs` native
backend. The cutover is gated by the `SOLID_IMPL` feature flag:

| Value | Behaviour |
|-------|-----------|
| `jss` (default) | Legacy `solid_proxy_handler` proxies `/solid/*` to JSS. Unchanged from pre-cutover. |
| `native`        | `solid_pod_handler::NativeSolidService` serves `/solid/*` from the in-process `solid-pod-rs` crate. JSS sidecar is still running but idle. |
| `shadow`        | JSS serves the client; the native backend runs the same request in parallel. Diffs are journalled to `docs/audits/YYYY-MM-DD-jss-native-shadow.jsonl`. Client never sees the native response. |

Unrecognised values default to `jss` with a `WARN` log.

## Phase-by-phase plan

### Phase 1 — Baseline (both environments)

- `SOLID_IMPL=jss` is the default everywhere.
- Shadow / native are never enabled before the deploy that lands ADR-053
  Phase 3 code. `solid_pod_handler` is compiled in, but its routes are
  not mounted until `SOLID_IMPL` is flipped.
- **Exit criterion:** image with the dispatcher code deployed to staging
  and prod; no behaviour change observed.

### Phase 2 — Staging shadow (72h)

- Set `SOLID_IMPL=shadow` on staging via
  `config/staging/docker-compose.staging.yml` (committed default) or
  `.env.staging` (override).
- Monitor:
  - `docs/audits/YYYY-MM-DD-jss-native-shadow.jsonl`
    — one JSONL row per request with `status_match`, `content_type_match`,
      `link_match`, `body_match`, `body_diff_bytes`.
  - Grafana panel (staging) — add counters for `shadow_body_mismatch_total`
    and `shadow_link_mismatch_total` (future task, not required to start
    this phase).
- Bug-fix loop: mismatches are investigated and patched in `solid-pod-rs`;
  staging is redeployed.
- **Exit criteria:**
  - Zero body mismatches on public endpoints
    (everything under `/public/*`, `/profile/card`, and the pod root).
  - < 1% header mismatches on LDP `Link` values (JSS emits slightly
    different `Link` orderings; this is acceptable).
  - 72 hours with the above holding continuously.

### Phase 3 — Staging native (48h)

- Flip staging to `SOLID_IMPL=native`. JSS sidecar stays running for
  instant rollback.
- Watch for client-visible errors: 5xx rate, Solid client compat tests,
  sovereign-mesh integration tests.
- **Exit criteria:**
  - 48 hours with no new 5xx regression attributable to `/solid/*`.
  - Rollback plan verified: flip `SOLID_IMPL=jss`, restart visionclaw,
    confirm JSS serves traffic again within one minute.

### Phase 4 — Production shadow (72h)

- Promote the staging image to prod with `SOLID_IMPL=shadow` set via
  the prod compose overlay.
- Same monitoring as Phase 2; the audit file is rotated daily and
  captured by the log-shipping sidecar.
- **Exit criteria:** same as Phase 2 (zero body mismatches on public
  endpoints, < 1% header mismatches, 72 hours).

### Phase 5 — Production native (7-day rollback window)

- Flip prod to `SOLID_IMPL=native`.
- JSS sidecar stays running in prod for **7 days** as a rollback lane.
- Rollback: set `SOLID_IMPL=jss`, restart visionclaw. No container
  image change is required.
- **Exit criteria:** 7 days with no JSS rollback triggered, no
  Solid-spec regression reported by downstream clients.

### Phase 6 — JSS decommission

- Remove the `jss` service from `docker-compose.staging.yml`,
  `docker-compose.unified.yml`, and any prod overlays.
- Remove `solid_proxy_handler.rs` / `solid_proxy_migration.rs` and
  reduce `SolidImpl` to `{ Native, Shadow }` (or drop the enum; only
  needed while both paths coexist).
- Tag a release and annotate in `CHANGELOG.md`.
- Post-decommission, per ADR-053 §"Post-Parity extraction", subtree-split
  `crates/solid-pod-rs` into its own GitHub repo and publish `0.1.0` to
  crates.io.

## Operational details

### Shadow audit file

- Path: `docs/audits/YYYY-MM-DD-jss-native-shadow.jsonl`.
- Format: JSON-lines, one diff per request.
- Rotation: per UTC day; written append-only.
- Schema:

  ```json
  {
    "ts": "2026-04-20T12:34:56.789Z",
    "path": "/api/solid/alice/public/profile",
    "method": "GET",
    "status_match": true,
    "jss_status": 200,
    "native_status": 200,
    "content_type_match": true,
    "link_match": true,
    "body_match": true,
    "body_diff_bytes": 0
  }
  ```

- Comparator normalises Turtle whitespace before diffing bodies — a
  whitespace-only difference is not flagged as a mismatch.

### Pass criteria (per phase)

All phases share the same numerical gate:

| Metric                                    | Gate       |
|-------------------------------------------|------------|
| Body mismatches on public endpoints       | `== 0`     |
| Body mismatches on private endpoints      | `== 0`     |
| `Link` header order/content mismatches    | `< 1%`     |
| Non-2xx regression vs. the prior phase    | `0`        |
| Rollback dry-run (shadow → jss, timed)    | `<= 60s`   |

Private endpoints are `{/inbox, /shared, /private}` plus anything gated
by a non-permissive ACL. They MUST be 100% byte-equal — drift here
indicates a WAC divergence and is blocking.

### Rollback

Any phase can be rolled back by flipping `SOLID_IMPL` and restarting the
`visionclaw` container:

```bash
# Staging
docker compose -f config/staging/docker-compose.staging.yml \
  up -d --force-recreate visionclaw   # picks up new SOLID_IMPL

# Prod (example; adjust compose path)
docker compose -f docker-compose.production.yml \
  up -d --force-recreate visionclaw
```

No data migration or schema change is required — the FS backend's root
(`POD_DATA_ROOT`, default `/app/data/solid-pod-rs`) is independent of
the JSS data volume, so both backends can coexist indefinitely.

### Environment variables

| Name              | Default                         | Notes |
|-------------------|---------------------------------|-------|
| `SOLID_IMPL`      | `jss`                           | `jss` \| `native` \| `shadow` |
| `POD_DATA_ROOT`   | `/app/data/solid-pod-rs`        | FS root for the native backend |
| `POD_BASE_URL`    | `https://pods.visionclaw.org`   | Public base URL for WebIDs / Link headers |
| `JSS_URL`         | `http://jss:3030`               | JSS upstream (used by the legacy proxy) |

## Compliance

This runbook addresses ADR-053 Compliance Criteria item
`[ ] Shadow-testing harness in staging; Phase 3 operational`.
