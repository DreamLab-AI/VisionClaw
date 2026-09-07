---
title: Configuration Reference
description: Exhaustive reference for VisionClaw environment variables, ports, and Docker Compose options, organised by domain.
---

# Configuration Reference

> [VisionClaw Docs](../README.md) · [Reference](README.md)

VisionClaw is configured through environment variables (read at process start),
the `docker-compose.unified.yml` profiles, and a runtime settings file. This page
is the single flat reference for all three. Variables are grouped by domain:
core, networking and ports, authentication and security, GitHub sync, GPU and
CUDA, ontology, Nostr, Solid, AI services, datastores, resource limits, payments,
and logging.

Every variable name below is read by the backend (`src/`) or one of the workspace
crates (`crates/`). Defaults marked *required* abort startup if unset under
`APP_ENV=production`; defaults in `code font` are the in-process fallback.

---

## Configuration precedence

Highest to lowest:

1. Runtime API calls — `POST /api/config/update`.
2. Environment variables — `.env`, then `docker-compose.unified.yml` `environment:` blocks.
3. Settings file — `${SETTINGS_FILE_PATH}` (default `data/settings.yaml`).
4. Hardcoded defaults.

Copy `.env.example` to `.env` before first run. The Compose file loads `.env`
via `env_file:` and layers profile-specific overrides on top.

---

## Networking and ports

VisionClaw exposes four externally relevant ports. The backend API and the
WebSocket stream share one Actix server on `:4000`; nginx serves the client on
`:3001` and reverse-proxies the API.

| Port | Service | Variable | Profile |
|------|---------|----------|---------|
| `4000` | Backend API + WebSocket (Actix) | `API_PORT` / `SYSTEM_NETWORK_PORT` | dev |
| `3001` | Frontend (nginx entry, proxies API) | `DEV_NGINX_PORT` / `PROD_API_PORT` | dev + prod |
| `8484` | Solid pod (embedded `solid-pod-rs`) | — | both |
| `9500` | Legacy MCP TCP transport | `MCP_TCP_PORT` | both |
| `9090` | Management API | `MANAGEMENT_API_PORT` | both |
| `5173` | Vite dev server (internal) | `VITE_DEV_SERVER_PORT` | dev |
| `24678` | Vite HMR (internal) | `VITE_HMR_PORT` | dev |

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `API_PORT` | integer | `4000` | Backend HTTP/WebSocket port (host-published in dev). |
| `SYSTEM_NETWORK_PORT` | integer | `4000` | Internal port the Rust server binds. |
| `BIND_ADDRESS` | string | `0.0.0.0` | Interface the server binds to. |
| `DEV_NGINX_PORT` | integer | `3001` | Host port mapped to nginx in the `dev` profile. |
| `PROD_API_PORT` | integer | `3001` | Host port mapped to the production container. |
| `MCP_TCP_PORT` | integer | `9500` | Legacy MCP protocol TCP port. |
| `MANAGEMENT_API_PORT` | integer | `9090` | Management/metrics API port. |
| `MANAGEMENT_API_HOST` | string | `agentic-workstation` | Management API host. |

The dev health check probes `http://localhost:4000/api/health`; production probes
`http://localhost:3001/readyz` through nginx to verify backend readiness.

---

## Core system

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `ENVIRONMENT` | string | `development` | Deployment mode label: `development`, `staging`, `production`. |
| `APP_ENV` | string | `development` | Hardening switch. `production` blocks `SETTINGS_AUTH_BYPASS` and `ALLOW_INSECURE_DEFAULTS` and refuses to start if a required secret is missing. |
| `NODE_ENV` | string | `development` | Client/Vite build mode. The `prod` profile forces `production`. |
| `DOCKER_ENV` | boolean | `true` | Set inside containers to enable container-aware paths. |
| `DEBUG_ENABLED` | boolean | `true` (dev) | Enables verbose backend debug output; `false` in `prod`. |
| `RUST_LOG` | string | see below | Per-module log filter (Rust `tracing`). |
| `RUST_LOG_REDIRECT` | boolean | `true` | Redirect Rust logs to the configured log sink. |
| `DATA_DIR` | string | `./data` (image: `/app/data`) | Root data directory. The Oxigraph dataset lives at `${DATA_DIR}/oxigraph/`. |
| `EVENT_STORE_PATH` | string | `${DATA_DIR}/events` | Append-only event/provenance store path. |
| `SETTINGS_FILE_PATH` | string | `data/settings.yaml` | Runtime settings file location. |
| `LOG_DIR` | string | `/app/logs` | Directory for log files. |
| `TELEMETRY_LOG_DIR` | string | `${LOG_DIR}` | Directory for structured telemetry output. |

The dev profile ships a tuned `RUST_LOG` default that quietens dependencies:

```
info,actix_web=warn,actix_server=warn,actix_http=warn,h2=warn,hyper=warn,
rustls=warn,reqwest=warn,oxigraph=warn,horned_owl=warn,whelk=warn,
solid_pod_rs=warn
```

Production defaults to `RUST_LOG=info`.

---

## Authentication and security

Authentication is enforced by ADR-011. Under `APP_ENV=production` the server
fails fast rather than fall back to an insecure default.

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `JWT_SECRET` | string | *required* | JWT signing secret (use `openssl rand -hex 32`). |
| `AUTH_TOKEN_EXPIRY` | integer | `86400` | Token lifetime in seconds. |
| `SETTINGS_AUTH_BYPASS` | boolean | `false` | Dev-only: skip Nostr auth on settings writes. Hard-blocked when `APP_ENV=production`. |
| `ALLOW_INSECURE_DEFAULTS` | boolean | `false` | Dev-only: permit placeholder secrets. Blocked in production — the server aborts if any secret keeps its default. |
| `CORS_ALLOWED_ORIGINS` | string (CSV) | dev origins | Allowed CORS origins. Production must include the public domain so nginx can forward the real `Origin`. |
| `ALLOWED_WS_ORIGINS` | string (CSV) | `CORS_ALLOWED_ORIGINS` | Origins accepted on the WebSocket upgrade. |
| `MANAGEMENT_API_KEY` | string | *required in prod* | Bearer key for the management API. |
| `POWER_USER_PUBKEYS` | string (CSV) | `""` | Nostr pubkeys granted elevated control-surface rights. |
| `MOCK_AGENTS` | boolean | `false` | Dev-only: serve synthetic agents. Never enable in production. |

---

## GitHub sync

The sync pipeline pulls the authored Obsidian vault from GitHub into the embedded
triple store. Only files tagged `public:: true` become page nodes; every file
with an `### OntologyBlock` contributes ontology data regardless of visibility.

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `ENABLE_GITHUB_SYNC` | boolean | `true` | Master switch for the GitHub sync service. |
| `GITHUB_TOKEN` | string | *required for sync* | Personal access token used for the GitHub API. |
| `GITHUB_OWNER` | string | *required* | Repository owner. `GITHUB_REPO_OWNER` is accepted as an alias. |
| `GITHUB_REPO` | string | *required* | Repository name. `GITHUB_REPO_NAME` is accepted as an alias. |
| `GITHUB_BRANCH` | string | `main` | Branch to sync. `GITHUB_BASE_BRANCH` is accepted as an alias. |
| `GITHUB_BASE_PATH` | string | *required* | Path prefix within the repo to ingest (e.g. `knowledge/pages,working/pages`). Changing it clears stale data. |
| `GITHUB_BASE_PATHS` | string (CSV) | `${GITHUB_BASE_PATH}` | Multiple ingest roots. |
| `GITHUB_API_VERSION` | string | `v3` | GitHub REST API version header. |
| `GITHUB_RATE_LIMIT` | boolean | `true` | Honour GitHub rate-limit headers and back off. |
| `PRIVATE_REPO_GITHUB_PAT` | string | `""` | GitHub token for the private corpus repo (`jjohare/visionGraph`). Fine-grained: Contents read/write, Pull requests read/write, Metadata read. Legacy name `LOGSEQ_PRIVATE_REPO_GITHUB` still accepted for one release. |
| `FORCE_FULL_SYNC` | boolean | `false` | Bypass SHA1 incremental filtering and reprocess every file. Reset to `false` after a one-off full reload. |

---

## GPU and CUDA

GPU physics is gated at build time by the `gpu` cargo feature and at runtime by
device availability. The Compose dev/prod profiles request an NVIDIA device via
`runtime: nvidia`. See ADR-070 for the CUDA integration contract.

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `ENABLE_GPU` | boolean | `false` | Enable GPU-accelerated force computation. |
| `CUDA_ARCH` | integer | `75` | Target SM architecture: `75` (RTX 20xx/T4), `86` (RTX 30xx), `89` (RTX 40xx). Set as both a build arg and a runtime env. |
| `NVIDIA_VISIBLE_DEVICES` | string | `0` | GPU device IDs exposed to the container. |
| `CUDA_VISIBLE_DEVICES` | string | inherits | CUDA-level device selection. |
| `NVIDIA_DRIVER_CAPABILITIES` | string | `compute,utility` | Driver capabilities granted to the container. |
| `VISIONCLAW_PTX_PATH` | string | image default | Override path to the compiled PTX kernels. |
| `VISIONCLAW_BLOCK_SIZE` | integer | kernel default | CUDA thread-block size for force kernels. |
| `FANOUT_NODE_THRESHOLD` | integer | kernel default | Node count above which fan-out partitioning engages. |
| `CUDA_HOME` / `CUDA_PATH` | string | `/opt/cuda` | Toolkit location (build-time; CachyOS uses `/opt/cuda`). |

The GPU path delivers roughly a 55x speedup over CPU — about 246 ms/frame (4 FPS)
falls to 4.5 ms/frame (222 FPS) at 100K nodes. See
[Physics parameters](physics-parameters.md) for layout tuning.

---

## Ontology

Ontology reasoning is embedded, not a service, so it has no network variables.
The pipeline runs Whelk-rs (OWL 2 EL classification), SHACL-lite shape
validation, JSON-LD validation, and PROV-O provenance capture in-process
(PRD-022). It is driven by ingested `### OntologyBlock` content and the runtime
settings file rather than environment variables. The augmentation retrieval spine
(ADR-112) reads the same embedded store. See the
[Ontology pipeline](../explanation/ontology-pipeline.md) explanation.

The `ontology-validation` feature flag (settings file / `POST /api/features/toggle`)
toggles SHACL enforcement; it defaults to on.

---

## Nostr

Nostr provides bead provenance publishing and the agent control-surface bridge.
Keys are scoped to the `visionclaw` service only and never shared via the common
environment block. Relay topology follows ADR-073.

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `VISIONCLAW_NOSTR_PRIVKEY` | string | `""` | Private key for the bead provenance bridge. Unset disables the bridge. |
| `NOSTR_RELAY_URL` | string | `""` | Source relay (must start `ws://` or `wss://`). |
| `FORUM_RELAY_URL` | string | `""` | Forum/destination relay for the bridge and elevation actor. |
| `ACSP_PANEL_NOSTR_PRIVKEY` | string | `""` | Key for the ACSP control-surface panel. |
| `ELEVATION_ACTOR_ENABLED` | boolean | `false` | Enable the elevation actor (requires `FORUM_RELAY_URL` and `ACSP_PANEL_NOSTR_PRIVKEY`). |

---

## Solid

Solid pod storage is provided by the embedded `solid-pod-rs` crate
(ADR-032, ADR-053, ADR-096) — the former JSS sidecar is removed. The pod is
served on `:8484`; the backend reaches it over an internal proxy URL.

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `ENABLE_SOLID` | boolean | `false` | Enable Solid/LDP integration. |
| `SOLID_DATA_ROOT` | string | `/data/solid` | Root path for pod storage. |
| `SOLID_INTERNAL_URL` | string | `http://127.0.0.1:4001/api/solid` | Internal proxy endpoint for the embedded pod. |
| `SOLID_ALLOW_ANONYMOUS` | boolean | `false` | Permit anonymous reads of public resources. |
| `SOLID_PROXY_SECRET_KEY` | string | *required when enabled* | Shared secret for the Solid proxy layer. |
| `VISIONCLAW_AGENT_KEY` | string | *required in prod* | Agent key for the pod-backed pipeline. Production aborts if unset. |

See [URN ↔ Solid mapping](urn-solid-mapping.md) and the
[Solid sidecar architecture](../explanation/solid-sidecar-architecture.md).

---

## AI services and orchestration

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `PRIMARY_PROVIDER` | string | `anthropic` | Default LLM provider selector. |
| `OPENAI_API_KEY` | string | `""` | OpenAI API key. |
| `PERPLEXITY_API_KEY` | string | `""` | **Not the Perplexity endpoint config.** Read only by the boot readiness report (`app_state.rs:1516`) to decide whether `PerplexityService` was *expected* to be present. See the correction note below. |
| `PERPLEXITY_ENABLED_PUBKEYS` | string (csv) | `""` | Per-pubkey feature gate for Perplexity access (`feature_access.rs:20`). Gates *who* may call it, not *where* it calls. |
| `COMFYUI_URL` | string | `http://comfyui:8188` | ComfyUI image-generation endpoint. |
| `COMFYUI_SALAD_URL` | string | `http://comfyui:3000` | Alternate ComfyUI/Salad endpoint. |
| `RAGFLOW_API_KEY` | string | `""` | RAGFlow API key. |
| `RAGFLOW_API_BASE_URL` | string | `""` | RAGFlow base URL. |
| `RAGFLOW_AGENT_ID` | string | `""` | RAGFlow agent identifier. |
| `MCP_HOST` | string | `agentic-workstation` | MCP server host. |
| `MCP_TRANSPORT` | string | `tcp` | MCP transport: `tcp` or `stdio`. |
| `MCP_RECONNECT_ATTEMPTS` | integer | `3` | MCP reconnect attempts. |
| `MCP_RECONNECT_DELAY` | integer | `1000` | Reconnect delay (ms). |
| `MCP_CONNECTION_TIMEOUT` | integer | `30000` | Connection timeout (ms). |
| `ORCHESTRATOR_WS_URL` | string | `ws://mcp-orchestrator:9001/ws` | Orchestrator WebSocket URL. |
| `BOTS_ORCHESTRATOR_URL` | string | `ws://agentic-workstation:3002` | Agent/bots orchestrator URL. |
| `CLAUDE_FLOW_HOST` | string | `agentic-workstation` | Claude Flow coordination host. |
| `RUV_SWARM_HOST` / `RUV_SWARM_PORT` | string / integer | `agentic-workstation` / — | Swarm coordination endpoint. |
| `DAA_HOST` / `DAA_PORT` | string / integer | — | Decentralised autonomous agent endpoint. |

**Correction (2026-09-05) — Perplexity is settings-sourced, not env-sourced.** This
table previously listed `PERPLEXITY_API_KEY` as if it configured the Perplexity
endpoint. It does not. `PerplexityService` reads `AppFullSettings.perplexity` and
fails with "Perplexity settings not configured" when that section is absent; the
`api_url`, `api_key` and `model` it uses all come from settings
(`src/services/perplexity_service.rs:99-120`), which is why the section is one of
the three `SECRET_BEARING_SECTIONS` redacted from full-settings GETs
(`settings_handler/routes.rs:229`). The only two Perplexity environment variables
the code reads are `PERPLEXITY_ENABLED_PUBKEYS`, which gates *who* may use the
feature (`src/config/feature_access.rs:20`), and `PERPLEXITY_API_KEY`, read solely
by `AppState`'s readiness report to decide whether the service was expected to be
constructed (`src/app_state.rs:1516`) — setting it configures nothing. The MCP
skill layer is separate and genuinely does take `PERPLEXITY_API_KEY` from the
environment; see [agents catalog](agents-catalog.md) §Skill Configuration. Raised by
diagram note VC-28.3 (`docs/diagrams/visionclaw/28-external-services.md` in the VisionFlow estate tree (see `docs/diagrams/README.md`)).

---

## Datastores

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `REDIS_URL` | string | `redis://redis:6379` | Redis connection URL (caching/coordination). |
| `POSTGRES_HOST` | string | `postgres` | PostgreSQL host (RuVector memory backend). |
| `POSTGRES_PORT` | integer | `5432` | PostgreSQL port. |
| `POSTGRES_DB` | string | `visionclaw` | Database name. |
| `POSTGRES_USER` | string | `visionclaw` | Database user. |
| `POSTGRES_PASSWORD` | string | *required* | Database password. |

The primary graph store is an **embedded Oxigraph RDF triple store backed by
SQLite** (ADR-101); it runs in-process with no container, network endpoint, or
password. **Neo4j is fully removed** ([ADR-132](../archive/adr/ADR-132-neo4j-removal-oxigraph-adoption.md))
— `NEO4J_*` variables are no longer read.
Reset the graph by stopping the server and running `rm -rf ${DATA_DIR}/oxigraph`.
See [Graph schema](graph-schema.md).

---

## Resource limits

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `MEMORY_LIMIT` | string | `16g` | Container memory limit (Compose `deploy.resources`). |
| `CPU_LIMIT` | float | `8.0` | Maximum CPU cores. |
| `CPU_RESERVATION` | float | `4.0` | Reserved CPU cores. |
| `MAX_CONCURRENT_TASKS` | integer | `20` | Simultaneous tasks across all agents (`TaskOrchestratorActor`). |
| `SHARED_MEMORY_SIZE` | string | `2g` | `shm_size` for GPU shared memory. |

Production limits in `docker-compose.unified.yml` cap the container at 8 GB memory
and 4 CPUs with a 2 GB / 1 CPU reservation.

---

## Payments (pay402)

The pay402 ledger meters per-call costs in sats (loopback ledger by default).

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `PAY_ENABLED` | boolean | `false` | Enable per-call payment metering. |
| `PAY_LEDGER_DIR` | string | `${DATA_DIR}/ledger` | Ledger storage directory. |
| `PAY_COST_SATS` | integer | `0` | Default per-call cost in sats. |
| `PAY_INFERENCE_COST_SATS` | integer | `${PAY_COST_SATS}` | Inference call cost. |
| `PAY_IMAGE_GEN_COST_SATS` | integer | `${PAY_COST_SATS}` | Image-generation call cost. |
| `PAY_ANALYTICS_COST_SATS` | integer | `${PAY_COST_SATS}` | Analytics call cost. |

---

## Logging and telemetry

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `RUST_LOG` | string | see [Core](#core-system) | Per-module log filter. |
| `LOG_DIR` | string | `/app/logs` | Log file directory. |
| `TELEMETRY_LOG_DIR` | string | `${LOG_DIR}` | Structured telemetry directory. |
| `AUDIT_LOG_PATH` | string | `/app/logs/audit.log` | Persistent audit log (auth events, admin actions). |

The Compose `json-file` driver caps logs at 10 MB × 3 files per service. See
[Telemetry and logging](../how-to/operations/telemetry-logging.md).

---

## Docker Compose

`docker-compose.unified.yml` defines three services across two profile pairs.

| Service | Profiles | Purpose |
|---------|----------|---------|
| `visionclaw` | `development`, `dev` | Hot-reload dev build; mounts host source read-only and the Docker socket. |
| `visionclaw-production` | `production`, `prod` | Pre-compiled prod image; data volumes only, no source mounts, no Docker socket. |
| `cloudflared` | `production`, `prod` | Cloudflare tunnel fronting the prod container. |

Select a profile with `--profile`:

```bash
docker compose -f docker-compose.unified.yml --profile dev up -d --build
docker compose -f docker-compose.unified.yml --profile prod up -d
```

### Profile differences

| Aspect | dev (`visionclaw`) | prod (`visionclaw-production`) |
|--------|--------------------|-------------------------------|
| Dockerfile | `Dockerfile.unified` (`target: development`) | `Dockerfile.production` |
| Published ports | `3001:3001`, `4000:4000` | `3001:3001` |
| Source mounts | host `src/`, `crates/`, `client/` read-only | none |
| Docker socket | mounted read-only (host root risk) | not mounted |
| Health check | `:4000/api/health` | `:3001/health` |
| Resource caps | shared GPU reservation | 8 GB / 4 CPU limit |

The dev profile mounts host source for hot reload. In Docker-in-Docker setups
these bind mounts resolve against the host filesystem via `HOST_PROJECT_ROOT`
(default `.`); see [Deployment](../how-to/deployment.md) for the correct rebuild
path.

### GPU resources

Both runtime services share a GPU reservation and `runtime: nvidia`:

```yaml
deploy:
  resources:
    reservations:
      devices:
        - driver: nvidia
          count: 1
          capabilities: [gpu, compute, utility]
runtime: nvidia
```

### Network and volumes

The stack joins an **external** Docker network (`visionclaw_network`) that must
exist before `up`:

```bash
docker network create visionclaw_network
```

Named volumes persist data and build caches:

| Volume | Mount | Purpose |
|--------|-------|---------|
| `visionclaw-data` | `/app/data` | Graph store, settings, ledger, events. |
| `visionclaw-logs` | `/app/logs` | Logs and audit trail. |
| `cargo-target-cache` | `/app/target` | Rust incremental build cache. |
| `cargo-cache` / `cargo-git-cache` | `/root/.cargo/*` | Cargo registry/git caches. |
| `npm-cache` | `/root/.npm` | Client dependency cache. |

### Deployment sizing

| Scale | Settings |
|-------|----------|
| Small (< 10K nodes) | `MEMORY_LIMIT=8g CPU_LIMIT=4.0 ENABLE_GPU=false` |
| Medium (10K–100K) | `MEMORY_LIMIT=16g CPU_LIMIT=8.0 ENABLE_GPU=true GPU_MEMORY_LIMIT=8g` |
| Large (100K+) | `MEMORY_LIMIT=32g CPU_LIMIT=16.0 ENABLE_GPU=true CUDA_ARCH=89` |

---

## See also

- [Deployment](../how-to/deployment.md) — build, rebuild, and run the stack.
- [Operations: configuration](../how-to/operations/configuration.md) — runtime change procedures.
- [Operations: security](../how-to/operations/security.md) — production hardening checklist.
- [Security model](../explanation/security-model.md) — auth and trust boundaries.
- [Deployment topology](../explanation/deployment-topology.md) — service layout and networks.
- [Ontology pipeline](../explanation/ontology-pipeline.md) — embedded reasoning stack.
- [REST API](rest-api.md) · [WebSocket protocol](websocket-protocol.md) · [Binary protocol](binary-protocol.md)
- Governing ADRs: [ADR-011 Auth enforcement](../archive/adr/ADR-011-auth-enforcement.md), [ADR-070 CUDA integration hardening](../archive/adr/ADR-070-cuda-integration-hardening.md), [ADR-090 Hexagonal crate modularisation](../archive/adr/ADR-090-hexagonal-crate-modularisation.md), [ADR-101 Triple-store migration framework](../archive/adr/ADR-101-triple-store-migration-framework.md).
