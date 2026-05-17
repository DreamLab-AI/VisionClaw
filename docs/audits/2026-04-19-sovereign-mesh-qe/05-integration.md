# 05 — End-to-End Integration

Walking the sovereign-mesh golden path commit-by-commit against the wire.

## Step 1 — Pod provisioning (default-private)

Entry: user authenticates via NIP-98, `solid_proxy_handler` detects missing Pod containers and triggers provisioning. `POD_DEFAULT_PRIVATE=true` selects the ADR-052 layout. Code path: `pod_default_private_enabled()` → build root / private / public / shared / profile containers with ACL templates (`render_owner_only_acl`, `render_public_container_acl`, `render_profile_container_acl`). NIP-39 WebID card written to `./profile/card` via `render_webid_card`. **Wired.** `src/handlers/solid_proxy_handler.rs:692-751`.

## Step 2 — Power-user CLI credential seed

Entry: `vc-cli bootstrap-power-user --env .env`. Reads `GITHUB_OWNER/REPO/BRANCH/BASE_PATH/TOKEN` from the .env, signs a NIP-98 event for `PUT ./private/config/github`, writes the JSON document. Token wrapped in `Zeroizing<String>` and redacted from logs (`src/bin/vc_cli.rs:33,241-254`). Writes via `authenticated-as-owner` NIP-98 either via `SERVER_NOSTR_PRIVKEY` (server-on-behalf) or `POWER_USER_NSEC` (user-signs-once). Supports `--dry-run` and `--force`. **Wired.**

**Gap I1 (MEDIUM)**: the audit event (kind 30301 per ADR-030-ext) for bootstrap is **not visible** in the CLI code. No `sign_and_broadcast` call in `vc_cli.rs`; the `bootstrap-power-user` command writes the Pod resource but does not emit the auditable Nostr event. Compliance criterion "kind-30301 audit event emitted on bootstrap" is unmet in code.

## Step 3 — Ingest saga (github-sync → two-pass parser → Pod + Neo4j)

The saga wiring is present (`src/services/ingest_saga.rs`), parser is present (`src/services/parsers/knowledge_graph_parser.rs::parse_bundle` and `::visibility::classify_visibility`), Pod client signs per-request with NIP-98 (`src/services/pod_client.rs`). Feature flag `POD_SAGA_ENABLED` gates the whole saga entry (ingest_saga.rs:57-61).

**Gap I2 (MEDIUM)**: the github-sync integration call site — i.e. the code that constructs `Vec<NodeSagaPlan>` from the github fetch and calls `IngestSaga::execute_batch` — was not observed in this audit. `github_sync_service.rs` exists at `src/services/github_sync_service.rs` but I did not verify that it reads Pod creds via `GITHUB_CREDS_IN_POD` and feeds the saga. Worth confirming before MVP ship.

**Gap I3 (MEDIUM)**: the parser's `parse_bundle` path is gated on an env flag — the code uses `VISIBILITY_CLASSIFICATION` per the docstring at line 143 but the gate itself was not observed as a live check in `parse_bundle`. It may rely on the caller (ingest service) to branch. Confirm the gate is actually enforced.

## Step 4 — Caller-aware API query

`/api/graph/data` wrapped with `RequireAuth::optional()` via `configure_graph_routes` at `src/handlers/api_handler/graph/mod.rs:669-691`. Anonymous callers arrive with `pubkey == ""`, filtered to `None`. `visibility_allows` drops private nodes the caller does not own. Filtered edges follow filtered nodes. **Wired** — but see Privacy §P1: the drop-vs-opacify semantic differs from the ADR.

## Step 5 — BRIDGE_TO promotion via broker (manual) + server-Nostr kind 30100

`BridgeEdgeService::promote` writes the monotonic BRIDGE_TO edge. The `SignBridgePromotion` actor message in `src/actors/server_nostr_actor.rs` signs the kind-30100 event. **Gap I4 (HIGH)**: the handler that *combines* these — promotes the edge in Neo4j AND signs+broadcasts — was not observed. The promote method does not call the actor; the sign message handler does not write Neo4j. Somewhere a coordinator must fan out. If the migration broker UI lives downstream, ensure that path emits both actions atomically (or saga-style with pending-marker recovery).

## Step 6 — Publish transition (MOVE `./private/kg/` → `./public/kg/`)

**Gap I5 (HIGH)**: the publish/unpublish saga (ADR-051 §publish saga) was not observed under `src/sovereign/visibility.rs` or equivalent. ADR-051 Compliance criteria "publish saga implemented end-to-end" and "unpublish emits HTTP 410 Gone for stale public URIs" are unmet. The double-gate supports **creation** of a public resource via PUT but does not implement the **move** from private to public on an existing resource. This is an ADR-051 implementation gap.

## Step 7 — Client receives V5 binary with bit 29 cleared

Binary broadcast path (`client_coordinator_actor::broadcast_with_filter`) calls `encode_node_data_extended_with_sssp` — the **non-privacy** variant. Bit 29 is never set on the wire today. **Gap I6 (HIGH — intent)**: identical to Privacy §P2. `encode_positions_v3_with_privacy` exists but is unreachable from production. Wire the caller-context through.

## Step 8 — Prometheus scrape

`/metrics` is registered outside the `/api` scope at `src/main.rs:764-767` — no auth, no rate-limit. `MetricsRegistry::render_text` honours `METRICS_ENABLED=false` by returning a sentinel comment. Staging compose (`config/staging/docker-compose.staging.yml` + `prometheus.yml`) points a Prometheus container at `visionclaw:4000/metrics`. **Wired.** 24 metrics observed in the registry, label sets bounded, cardinality safe.

## Summary of integration seams

| # | Seam | Status |
|---|---|---|
| 1 | Pod provisioning → default-private WAC | wired |
| 2 | CLI bootstrap writes creds | wired; **I1** audit event missing |
| 3 | GitHub-sync → saga → Pod + Neo4j | partial; **I2** caller not verified, **I3** gate check not verified |
| 4 | Graph read → caller-aware filter | wired (drop semantic, not opacify — P1) |
| 5 | BRIDGE_TO promotion + kind-30100 sign | **I4** coordinator missing |
| 6 | Publish saga (MOVE public↔private) | **I5** not implemented |
| 7 | Binary broadcast with bit 29 | **I6** code present, never called with privacy set |
| 8 | Prometheus metrics endpoint | wired |

The sprint lands strong primitives (auth, parser, saga mechanics, Pod client, metrics, WAC crate, server identity) but **two critical coordinator layers** are missing (publish saga, BRIDGE_TO + kind-30100 fan-out) and **two integration pass-throughs** are incomplete (caller pubkey into broadcast, audit event on bootstrap).
