# Feature Gap Analysis: multi-agent-docker → agentbox

**Date**: 2026-04-26
**Purpose**: Identify features present in the live MAD container that are missing from agentbox, to inform the parity PRD for migration.
**Method**: Four parallel research agents mapped MAD directory, agentbox directory, live container state, and VisionClaw integration points.

---

## Executive Summary

Agentbox has already surpassed MAD in architecture (Nix reproducibility, adapter slots, sovereign identity, linked-data federation, runtime hardening). The gaps are **operational** — services, integrations, and tooling that the live MAD container provides day-to-day that agentbox hasn't wired up yet. Most gaps are straightforward to close because agentbox's manifest-driven design means adding a feature is a TOML gate + Nix derivation.

---

## Gap Table

### P0 — Blocks migration (must have before side-by-side testing)

| # | Feature | MAD Implementation | Agentbox Status | Gap | Migration Path |
|---|---------|-------------------|-----------------|-----|---------------|
| 1 | **RuVector PostgreSQL (external)** | Always-on connection to `ruvector-postgres:5432`; 112 SQL functions; 1.17M+ entries | `adapters.memory = "embedded-ruvector"` (sql.js); `external-pg` adapter exists but untested in production | Adapter exists, needs production validation + data migration path | Set `adapters.memory = "external-pg"` in agentbox.toml; validate HNSW search parity; write migration script for sql.js → PG |
| 2 | **Docker network integration** | On `visionclaw_network` network; discovers ruvector-postgres, Neo4j, ComfyUI, RAGFlow, MinIO, Redis, ES | Compose doesn't join external networks by default | No `networks:` block in generated compose | Add `[integrations.docker_network]` gate in agentbox.toml; flake.nix compose generator emits `networks: { visionclaw_network: { external: true } }` |
| 3 | **Management API feature parity** | Port 9090: task CRUD, agent spawn, ComfyUI proxy, health, metrics | Port 9090: task CRUD, adapter dispatch, health, metrics, URI resolver, linked-data viewer | Missing: ComfyUI proxy route, agent spawn verb (uses orchestrator adapter instead) | Wire `/v1/agents/spawn` route through orchestrator adapter; add ComfyUI proxy if `skills.media.comfyui_builtin = true` |
| 4 | **610 agent templates** | Mounted at `/home/devuser/agents/` (610 .md files from ChrisRoyse/610ClaudeSubagents) | 1 template (`agents/auto-consultant.md`) | 609 missing | Git submodule or Nix fetchFromGitHub into `/opt/agentbox/agents/`; gate with `[toolchains.agent_templates] enabled = true` |
| 5 | **Claude Code settings bridge** | `~/.claude/settings.json` with full MCP server registry, hooks, model routing | Per-profile `settings.json` generated at provision time | Settings are profile-scoped (correct), but need parity with MAD's MCP registry | Template the settings.json generator to include all skill MCPs; validate against MAD's live config |
| 6 | **SSH server** | sshd on port 2222→22; host key mount from `~/.ssh` | Not in supervisord config | No SSH access | Add `[services.sshd] enabled = true, port = 22` gate; Nix adds openssh-server; supervisor block generated |

### P1 — Required for daily-driver parity

| # | Feature | MAD Implementation | Agentbox Status | Gap | Migration Path |
|---|---------|-------------------|-----------------|-----|---------------|
| 7 | **MCP Gateway (TCP/WS bridge)** | `mcp-gateway` service: TCP port 9500, WebSocket port 3002; bridges stdio MCP servers to network clients | Not present | No TCP/WS bridge for MCP servers | Port `mcp-gateway.js` + `mcp-tcp-server.js` + `mcp-ws-relay.js` to agentbox npm-services; gate with `[services.mcp_gateway]` |
| 8 | **Z.AI service** | Dedicated `claude-zai` service on port 9600; runs as `zai-user`; worker_pool=4 | ZAI is a consultant (`consultants.zai`), not a dedicated service | Consultant invocation ≠ always-on service proxy | Add `[services.zai_proxy]` option that runs the Z.AI bridge as a supervised service; consultant mode remains for on-demand use |
| 9 | **Local LLM proxy** | `local-llm-proxy` service; Nemotron 120B via llama.cpp; Anthropic→OpenAI translation; port 3100 | Not present | No local LLM fallback | Add `[integrations.local_llm]` with host/port/model config; Nix derivation for proxy script; supervisor block |
| 10 | **HTTPS bridge** | Always-on HTTPS proxy service | Gated via `sovereign_mesh.https_bridge = true` but implementation incomplete | Stub exists, needs implementation | Complete the HTTPS bridge supervisor block in flake.nix; test with sovereign mesh endpoints |
| 11 | **Telegram mirror (ctm)** | Rust binary `/usr/local/bin/ctm`; hooks in settings.json; mirrors all sessions to Telegram forum topics | Not present | No Telegram mirroring | fetchFromGitHub the ctm binary; add `[services.telegram_mirror]` gate; wire hooks |
| 12 | **Code Server** | Always-on, port 8080, no auth | Gated `[toolchains.code_server] enabled = true` | Present but off by default; needs auth option | Enable by default in development profile; add optional auth config |
| 13 | **VNC desktop** | Xvfb :1 (2048x2048) + x11vnc (5901) + Openbox + tint2 + 7 kitty terminals | `[desktop] enabled = true, stack = "hyprland-wayland"` | Different stack (Wayland vs X11); may break X11-dependent tools (Playwright, QGIS) | Ensure Xwayland compatibility for X11 tools; test Playwright + QGIS under Hyprland; add X11 fallback gate |
| 14 | **Multi-user isolation** | 4 OS users: devuser, gemini-user, openai-user, zai-user; separate workspaces | Profile-based isolation (shared user, separate configs) | No OS-level user separation for LLM provider processes | Add `[security.user_isolation]` gate; Nix creates users; supervisor runs consultant processes as dedicated users |
| 15 | **ComfyUI integration** | External container on visionclaw_network; API at 192.168.2.48:3001 / 172.18.0.10:8188; MCP server bridges | Builtin option (skills.media.comfyui_builtin) OR external (`integrations.comfyui_external`) | External integration needs docker network (see #2); builtin needs GPU validation | Validate both paths: builtin with GPU, external via visionclaw_network network |
| 16 | **Neo4j graph DB** | visionflow-neo4j container; ports 7474/7687; used by VisionFlow actors | Not integrated (optional external) | No Neo4j adapter or integration | Add `[integrations.neo4j]` config block; not an adapter slot — it's a VisionClaw dependency, consumed via BC20 federation |
| 17 | **tmux → Zellij migration** | tmux with 11-window layout; deeply embedded in workflows | Zellij with tab layout | Different multiplexer; muscle memory break | Ship tmux as fallback (`[toolchains.tmux_compat] enabled = true`); provide Zellij layout matching MAD's 11 windows; alias `tmux` → `zellij` optionally |

### P2 — Nice to have / specialist tools

| # | Feature | MAD Implementation | Agentbox Status | Gap | Migration Path |
|---|---------|-------------------|-----------------|-----|---------------|
| 18 | **AISP 5.1** | `/usr/local/bin/aisp`; neuro-symbolic reasoning; validate/binding commands | Not present | Missing entirely | fetchurl the binary; add `[toolchains.aisp]` gate |
| 19 | **Claude Cowork** | `/home/devuser/.local/bin/cowork`; manages Claude Desktop on VNC Display :1 | Not present | Missing entirely | fetchurl the binary; requires desktop mode; add `[toolchains.cowork]` gate |
| 20 | **KiCAD + ngspice** | Installed via pacman; MCP servers for EDA/circuit design | Not present | Missing entirely | Add `[skills.engineering.kicad]` and `[skills.engineering.ngspice]` gates; Nix packages exist |
| 21 | **PBR Renderer** | MCP server on socket 9878 | Not present | Missing entirely | Port PBR skill + MCP; add `[skills.spatial_and_3d.pbr]` gate |
| 22 | **Arnis (Minecraft world gen)** | Installed in Dockerfile phase 2.6 | Not present | Missing entirely | Add `[skills.spatial_and_3d.arnis]` gate; Nix derivation from GitHub |
| 23 | **Google Antigravity IDE** | Installed in Dockerfile phase 14 | Not present | Not critical | Low priority; evaluate if anyone uses it |
| 24 | **Elasticsearch** | docker-es01-1 container; port 1200 | Not integrated | Consumed by RAGFlow, not directly by agent container | External service; add `[integrations.elasticsearch]` config if needed |
| 25 | **MinIO (S3)** | docker-minio-1; ports 9000-9001 | Solid pods serve similar role for agent data | Different paradigm | Not a direct gap — Solid pods are the agentbox equivalent for agent-scoped storage; MinIO is infrastructure |
| 26 | **Redis/Valkey** | docker-redis-1; port 6379 | Not integrated | Used by RAGFlow + potential event bus | External service; add `[integrations.redis]` config if needed for event bus (DDD-BC20 §9.1) |
| 27 | **Multiple project mounts** | PROJECT_DIR_2 through PROJECT_DIR_10 (up to 10 projects) | Single project mount | Only 1 project mount in compose | Add `[workspace.project_mounts]` array in agentbox.toml; compose generator emits N bind mounts |
| 28 | **Resource limits** | Explicit: 64GB mem, 32 cores, 32GB shm | Not in compose | No resource constraints | Add `[resources]` section to agentbox.toml; compose generator emits deploy.resources block |
| 29 | **Gemini Flow MCP** | Always-on supervised MCP server | Gemini is a consultant, not a dedicated MCP | Consultant ≠ persistent MCP | Add optional `[services.gemini_flow]` for persistent Gemini bridge; consultant mode covers most use cases |
| 30 | **Flow Nexus MCP** | Running in live container | Not present | Cloud platform integration | Add `[integrations.flow_nexus]` gate if needed; may not be required for standalone |
| 31 | **Ruflo V4 plugins** | 6 plugins installed in Dockerfile | Ruflo included but plugins not explicitly listed | Plugin parity unclear | Audit which plugins are installed; add to Nix npm-cli derivation |
| 32 | **CLAUDE.md dedup cron** | Daily 03:00 UTC via supervisord | Not present | No dedup maintenance | Add as supervisor cron block; gate with `[maintenance.claude_md_dedup]` |
| 33 | **Godot / Unreal Engine** | Skills present; engines partially installed | Skills present; engines gated | Verify Nix packages match MAD's installed versions | Test `[skills.spatial_and_3d.godot]` and `[skills.spatial_and_3d.unreal_engine]` gates |

---

## Agentbox Advantages (features MAD lacks)

These are capabilities agentbox already has that MAD does not. They represent forward progress, not gaps.

| Feature | Agentbox | MAD Equivalent |
|---------|----------|---------------|
| **Nix reproducible builds** | Hermetic, declarative, multi-arch | 1188-line imperative Dockerfile |
| **Five-slot adapter architecture** | Pluggable beads/pods/memory/events/orchestrator | Hardcoded integrations |
| **Solid Protocol server** (solid-pod-rs) | First-class Rust server, WAC 2.0, LDP | JSS (JavaScript) or none |
| **Embedded Nostr relay** | Sovereign messaging, NIP-42 auth | None |
| **did:nostr identity** | Decentralised agent identity | None |
| **Privacy filter** | PII redaction middleware (1.5B MoE) | None |
| **JSON-LD federation** | 11 linked-data surfaces | None |
| **Canonical URI grammar** | `did:nostr:` + `urn:agentbox:` resolver | None |
| **Multi-arch images** | amd64 + arm64 | x86_64 only |
| **Immutable bootstrap** | No package manager at runtime | apt/pip/npm at runtime |
| **Runtime hardening** | cap_drop, read_only, no-new-privileges | Full privileges |
| **Contract testing** | Adapter contract test suite | None |
| **Cloud provisioning** | OCI, Fly, Hetzner, bare-metal scripts | Docker only |
| **Manifest-driven config** | Single agentbox.toml drives everything | Scattered env vars + Dockerfile |
| **Consultant tier** | Structured multi-LLM consultation | Ad-hoc per-user services |

---

## Migration Priority Order

```
Phase 1 (P0 — side-by-side testing gate):
  #2 Docker network → #1 RuVector PG → #5 Settings bridge → #4 Agent templates → #6 SSH → #3 API parity

Phase 2 (P1 — daily-driver):
  #12 Code Server → #13 Desktop → #17 tmux compat → #15 ComfyUI → #7 MCP Gateway → #8 Z.AI → #11 CTM

Phase 3 (P2 — specialist):
  #18-33 as needed per user workflow

Phase 4 (cutover):
  ADR-058 deprecation gates → snapshot → switch → 30-day rollback window
```

---

## Counts

| Metric | Value |
|--------|-------|
| Total gaps identified | 33 |
| P0 (migration blockers) | 6 |
| P1 (daily-driver parity) | 11 |
| P2 (specialist/optional) | 16 |
| Agentbox advantages over MAD | 15 |
| Estimated agentbox.toml additions | ~18 new gate keys |
| Estimated new Nix derivations | ~8 |
| Estimated new supervisor blocks | ~10 |
