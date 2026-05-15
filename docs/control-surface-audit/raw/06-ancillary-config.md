# Ancillary Configuration Sources

> Captured from research agent. Env vars, agentbox knobs, ADR-defined settings, compose orchestration.

## 1. Env vars

### MCP / Claude-flow integration
| Name | Default | Source | Read by | Gates |
|---|---|---|---|---|
| CLAUDE_FLOW_HOST | agentic-workstation | docker-compose.unified.yml:8 | `src/main.rs` MCP init | orchestrator discovery |
| MCP_HOST | agentic-workstation | docker-compose.unified.yml:9 | Vite/backend MCP client | MCP server discovery |
| MCP_TCP_PORT | 9500 | docker-compose.unified.yml:10 | MCP client init | MCP port |
| MCP_TRANSPORT | tcp | docker-compose.unified.yml:11 | MCP transport | wire protocol |
| MCP_RECONNECT_ATTEMPTS | 3 | docker-compose.unified.yml:12 | retry loop | resilience |
| MCP_RECONNECT_DELAY | 1000 | docker-compose.unified.yml:13 | backoff calc | retry backoff |
| MCP_CONNECTION_TIMEOUT | 30000 | docker-compose.unified.yml:14 | socket timeout | handshake budget |
| ORCHESTRATOR_WS_URL | ws://mcp-orchestrator:9001/ws | docker-compose.unified.yml:15 | client | task queue endpoint |
| MCP_RELAY_FALLBACK_TO_MOCK | true | docker-compose.unified.yml:16 | `src/handlers/orchestrator_handler.rs` | dev-only mock fallback |
| BOTS_ORCHESTRATOR_URL | ws://agentic-workstation:3002 | docker-compose.unified.yml:17 | client bot API | MAD compat (port 3002) |
| MANAGEMENT_API_HOST | agentic-workstation | docker-compose.unified.yml:18 | client API calls | management API host |
| MANAGEMENT_API_PORT | 9090 | docker-compose.unified.yml:19 | client HTTP | 9190 in agentbox override |

### Database
| Name | Default | Source |
|---|---|---|
| NEO4J_URI | bolt://neo4j:7687 | docker-compose.unified.yml:138 |
| NEO4J_USER | neo4j | :139 |
| NEO4J_PASSWORD | (must set) | :140 |
| NEO4J_DATABASE | neo4j | :141 |

### Build & runtime
| Name | Default | Source | Notes |
|---|---|---|---|
| CUDA_ARCH | 75 / 86 | compose:92, Dockerfile build args | A100/L40 vs RTX A6000/H100 |
| BUILD_TARGET | development | compose:93 | dev/prod multi-stage |
| NODEJS_VERSION | 22 | Dockerfile.unified L22 | pinned |
| RUST_LOG | warn,webxr=info,... | compose:111 | per-module verbosity |
| RUST_LOG_REDIRECT | true | compose:112 | stderr/stdout capture |

### Nostr / sovereign mesh
| Name | Default | Source | Notes |
|---|---|---|---|
| VISIONCLAW_NOSTR_PRIVKEY | (empty) | compose:129 | bridge identity, ADR-040 auto-gen |
| SERVER_NOSTR_PRIVKEY | fallback to VISIONCLAW_NOSTR_PRIVKEY | compose:134 | server identity (kind 30023/30100) |
| SERVER_NOSTR_AUTO_GENERATE | true | compose:135 | dev-only |
| FORUM_RELAY_URL | (empty) | compose:136 | optional |

### Access control allowlists (pubkey CSV)
| Name | Default | Source | Effect |
|---|---|---|---|
| POWER_USER_PUBKEYS | (empty) | compose:121 | unlocks PU UI affordances |
| APPROVED_PUBKEYS | (empty) | compose:122 | general auth |
| SETTINGS_SYNC_ENABLED_PUBKEYS | (empty) | compose:123 | settings PUT |
| OPENAI_ENABLED_PUBKEYS | (empty) | compose:124 | gates `/api/openai` |
| PERPLEXITY_ENABLED_PUBKEYS | (empty) | compose:125 | gates research agent |
| RAGFLOW_ENABLED_PUBKEYS | (empty) | compose:126 | gates RAG pipeline |

### External integrations / API keys
| Name | Default | Source |
|---|---|---|
| DEEPSEEK_API_KEY / DEEPSEEK_BASE_URL | empty / api.deepseek.com | .env.example |
| OPENAI_API_KEY / OPENAI_ORG_ID | empty | .env.example |
| PERPLEXITY_API_KEY / _URL / _MODEL / _TEMPERATURE | – | .env.example |
| RAGFLOW_API_KEY / _BASE_URL / _AGENT_ID | – | .env.example |
| GITHUB_TOKEN / _OWNER / _REPO / _BRANCH / _BASE_PATH | – | .env.example |

### Security / auth
| Name | Default | Source | Notes |
|---|---|---|---|
| SESSION_SECRET | dev-session-secret-not-for-production | .env.example | must rotate in prod |
| SESSION_TIMEOUT | 1800 / 3600 s | .env.example | |
| SETTINGS_AUTH_BYPASS | false | compose:115 | **dev-only — must reject prod** |
| WS_AUTH_ENABLED | true | .env.example | NIP-98 for WS |
| WS_AUTH_TOKEN | dev-ws-token | .env.example | legacy |

### Network / CORS
| Name | Default |
|---|---|
| CORS_ALLOWED_ORIGINS | http://localhost:3000,3001,... |
| CORS_ALLOWED_METHODS | GET,POST,PUT,DELETE,OPTIONS,PATCH |
| CORS_ALLOWED_HEADERS | Content-Type,Authorization,X-Requested-With |
| CLOUDFLARE_TUNNEL_TOKEN | (empty) |

### Performance / telemetry
| Name | Default | Source |
|---|---|---|
| DEBUG_ENABLED | true(dev)/false(prod) | compose:106 |
| VITE_DEV_SERVER_PORT | 5173 | :107 |
| VITE_API_PORT | 4000 | :108 |
| VITE_HMR_PORT | 24678 | :109 |
| SYSTEM_NETWORK_PORT | 4000 | :6 |
| TELEMETRY_ENABLED | true/false | .env.example |
| TELEMETRY_METRICS_INTERVAL | 1000/5000 ms | .env.example |

### Solid pod / agents
| Name | Default | Source |
|---|---|---|
| SOLID_PROXY_SECRET_KEY | dev-solid-secret | compose:231 |
| VISIONFLOW_AGENT_KEY | changeme-agent-key | compose:232 |
| POD_NAME | $HOSTNAME | computed |

## 2. Agentbox integration knobs

### Side-by-side port remapping (per ADR-058)
| Service | MAD | Agentbox | Source |
|---|---|---|---|
| Management API | 9090 | 9190 | docker-compose.override.yml:18 |
| Code Server | 8080 | 8180 | :22 |
| VNC Desktop | 5901 | 5902 | :21 |
| SSH | 2222 | 2223 | :23 |
| Agent Events | – | 9700 | :19 |
| Solid Pod | – | 8484 | :20 |
| Prometheus | – | 9191 | :24 |

### Adapter manifest knobs (`agentbox.toml`)
| Knob | Default | Notes |
|---|---|---|
| `[federation].mode` | "client" | "client" (VisionClaw master) or "standalone" |
| `adapters.beads` | "visionclaw" | / "local-sqlite" / "off" — PRD-004 P1.6 |
| `adapters.pods` | "visionclaw" | / "local-jss" / "off" — P1.7 |
| `adapters.memory` | "external-pg" (when enabled) | / "embedded" — P2.6 |
| `adapters.events` | "visionclaw" | Nostr publishing |
| `adapters.orchestrator` | "stdio-bridge" | spawn/monitor — P1.8 |
| `[integrations.ruvector_external]` enabled | false | external PG opt-in |
| RUVECTOR_PG_CONNINFO | "host=ruvector-postgres port=5432 …" | docker-compose.override.yml:29 |
| `[toolchains.cuda]` enabled | false | adds ~25 GB if true |
| `[toolchains.code_server]` | (off) | optional |
| `[toolchains.ctm]` | (off) | Telegram mirror daemon |
| `[toolchains.blender]` | (off) | |
| `[toolchains.tex]` | (off) | |
| `[sovereign_mesh]` enabled | true | |
| `[sovereign_mesh.telegram_mirror]` | false | CTM |
| `[sovereign_mesh.publish_agent_events]` | false | Nostr fan-out |

## 3. ADR-defined settings
| ADR | Setting | Status |
|---|---|---|
| ADR-039 | Canonical PhysicsSettings struct | Implemented (2026-04-20) |
| ADR-039 | Field-name migration aliases (`repel_k`→`repulsion_strength`) | Active alias phase |
| ADR-040 | SERVER_NOSTR_PRIVKEY auto-gen | Implemented |
| ADR-040 | Server signing kind 30023/30100/30200/30300/30301/31400/31402 | Implemented |
| ADR-048 | :KGNode data plane (numeric IDs) | Ratified |
| ADR-048 | :OntologyClass T-Box | Ratified |
| ADR-050 | Visibility enum (Public/Private) | Ratified |
| ADR-050 | Opaque ID HMAC + rotating salt (24 hex, 48 h dual-salt) | Ratified |
| ADR-050 | Bit 29 PRIVATE_OPAQUE_FLAG in V5 binary | Ratified |
| ADR-051 | Publish/unpublish saga | Pending |
| ADR-052 | Pod default-private WAC | Ratified |
| ADR-053/056 | solid-pod-rs replaces JSS | Migrating |
| ADR-055 | Fail-closed auth in production | Implemented |
| ADR-057 | Brief + role-specific agents | Pending |
| ADR-058 | MAD deprecation phased gates | Acceptance testing |

## 4. Compose / orchestration knobs
| Knob | Default | Effect |
|---|---|---|
| Service profiles | dev + development | --profile selector |
| NEO4J_PASSWORD | required | compose validation fails if missing |
| Container Names | visionflow_container / _prod_container | DNS |
| Healthcheck path | /api/health | startup wait |
| Healthcheck retries | 5 | failure threshold |
| Volume mounts | visionflow-data / -logs / npm-cache / cargo-cache | persistence |
| HOST_PROJECT_ROOT | "." | DinD path translation |
| GPU Resource | nvidia count:1 | NVIDIA_VISIBLE_DEVICES |
| Network | visionclaw_network (external) | service discovery |
| Restart policy | unless-stopped | |
| Logging driver | json-file 10m × 3 | rotation |
| Build target | development / production | image flavour |
| Build args | CUDA_ARCH=75, BUILD_TARGET | build-time only |

## 5. Disconnects & undocumented config

### Env vars in docs/agentbox not in root .env.example
- `OLLAMA_BASE_URL`, `OLLAMA_MODEL`
- `MANAGEMENT_API_AUTH_MODE` ("hybrid"/"nip98"/"none" — undocumented)
- `LOCAL_LLM_HOST`, `LOCAL_LLM_PORT`
- `COMFYUI_API_ENDPOINT`, `COMFYUI_LOCAL_ENDPOINT`

### Settings in code, undocumented in ADRs
- `PERPLEXITY_FREQUENCY_PENALTY/PRESENCE_PENALTY/TOP_P` — no model-tuning ADR
- `OPENAI_RATE_LIMIT/TIMEOUT` — no rate-limit policy
- `PERPLEXITY_MAX_TOKENS` — no window-allocation guidance
- `DOCKER_ENV`, `VITE_DEBUG` — compose-only
- `NVIDIA_DRIVER_CAPABILITIES`, `NVIDIA_VISIBLE_DEVICES` — multi-GPU strategy undocumented

### ADR-defined but no code yet
- ADR-050 opaque-ID rotation logic (Wave 2)
- DDD BC20 LocalFallbackProbe Ed25519 verification (skeleton only)
- DDD BC20 `/v1/meta` handshake + AdapterEndpointRegistry SemVer ranges

### MAD vs Agentbox config divergence (ADR-058 in flight)
| Feature | MAD | Agentbox |
|---|---|---|
| Supervisor gating | static supervisord.conf | generated from agentbox.toml |
| Entrypoint size | 2,379 lines bash | 71 lines |
| Docker-in-docker | /var/run/docker.sock:rw | not mounted (Nix on host) |
| Capabilities | SYS_ADMIN+NET_ADMIN+SYS_PTRACE | no-new-privileges:true |
| Profile isolation | per-Linux-user | shared user + per-profile mounts |
| Durable memory | ruvector-postgres sidecar | external (P2.6) or embedded (ADR-002) |

### Reproducibility / audit gaps
- No `.env.example` default for NEO4J_PASSWORD
- RUVECTOR_PG_CONNINFO hardcoded; no secret-mgr derivation
- No rotation policy documented for SESSION_SECRET, SOLID_PROXY_SECRET_KEY, VISIONCLAW_NOSTR_PRIVKEY
- CLOUDFLARE_TUNNEL_TOKEN not in any .env template

## Summary
- 120+ env vars
- 11 agentbox adapter slots
- 5 ADR-defined settings families
- 15 compose knobs

Primary change vectors:
1. **ADR-058 agentbox migration** — port remap, federation/standalone switch, adapter contract versions reshape Q2 2026.
2. **Fragmented feature flagging** — pubkey allowlists in env, optional toolchains in agentbox manifest. No unified surface.
3. **Ad-hoc secrets management** — no rotation/escrow/audit; gitleaks pre-commit only.
4. **API contract versioning pending** — DDD BC20 §4.1a `/v1/meta` schema + SemVer ranges not yet published.
