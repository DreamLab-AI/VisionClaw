# PRD-004: Agentbox integration with VisionClaw (MAD replacement)

**Status:** Draft v4 — 10/10 open questions resolved, architecture ratified, ready for ADR + DDD + QE review
**Date:** 2026-04-23
**Author:** VisionClaw platform team
**Supersedes:** — (MAD's original deployment is legacy, no prior PRD)
**Related (VisionClaw):** ADR-058 (MAD→agentbox migration, incoming), `docs/ddd-agentbox-integration-context.md` (incoming), `docs/ddd-bead-provenance-context.md`, `docs/prd-bead-provenance-upgrade.md`, ADR-056 (JSS parity migration), ADR-057 (contributor enablement)
**Related (agentbox-internal):** `agentbox/docs/prd/PRD-001-capabilities-and-adapters.md` (standalone product spec), agentbox ADR-001–005

> **Scope note.** This doc owns the VisionClaw↔agentbox integration story: how agentbox joins VisionClaw's docker-compose mesh, which sibling services it speaks to, and how MAD (at `multi-agent-docker/`) is deprecated in favour of agentbox (at `agentbox/`). Agentbox as a **standalone product** is specified separately in `agentbox/docs/prd/PRD-001-capabilities-and-adapters.md` — this doc treats agentbox as an upstream dependency and consumes its adapter contract. When agentbox is pushed to its own repo at [github.com/DreamLab-AI/agentbox](https://github.com/DreamLab-AI/agentbox), the standalone doc goes with it; this integration doc stays in VisionClaw.

**v2 changelog (2026-04-23):** Folded user decisions on Q1 (Hyprland/Wayland), Q2 (ragflow env switch), Q3 (swap Gemini Flow for official `@google/gemini-cli`), Q4 (ontology tools port default-off), Q6 (CUDA build flag default-off), Q7 (built-in + external ComfyUI dual switches), Q8 (single /projects), Q9 (keep Zellij + port MAD tab layout + add alias), Q10 (Solid/Nostr confirmed core ecosystem — D.6 inverted).

**v3 changelog (2026-04-23 later same day):** Architectural reframe — Beads, Solid pods, and durable memory are VisionClaw Rust-side bounded contexts; agentbox is a **federation client**, not a sovereignty host. Adapter pattern introduced. P1.6/P1.7 rewritten as client-adapter rows. New P1.8 stdio agent spawn/monitor channel for VisionClaw actor consumption. New P2.6 external RuVector PG backend.

**v4 changelog (2026-04-23 later still):** Document split. This file (PRD-004) moved from `agentbox/docs/prd/` to `project/docs/` because its content is VisionClaw-specific. A new lean, generic, standalone PRD-001 is created at `agentbox/docs/prd/PRD-001-capabilities-and-adapters.md` — it describes agentbox as a reusable container product and does not reference VisionClaw except in one history sentence. When agentbox's subfolder is pushed to its standalone repo at `DreamLab-AI/agentbox`, only the standalone doc travels with it. MAD-vs-VisionClaw specifics (ragflow, ontology tools, briefing roles, MAD deprecation) all live here.

## 1. Summary

Replace `multi-agent-docker/` with `agentbox/` as VisionClaw's agent-container subsystem. Parity plus three structural upgrades: (a) reproducible Nix build replaces the 1,188-line Dockerfile, (b) manifest-gated features replace the 2,379-line bash entrypoint, (c) adapter-based integration with sibling Rust actors (beads, pods, memory, events) replaces MAD's in-container hosting of those services. Agentbox's daily-driver shape inside VisionClaw is **federated mode** with VisionClaw Rust as the source of truth; its **standalone mode** (PRD-001 at agentbox/docs) is the public reusable product and is not this doc's concern.

## 2. Goals

1. Every MAD capability the user depends on daily survives migration — with equal or better UX.
2. Every port is a manifest-gated `agentbox.toml` feature, never a Dockerfile edit.
3. Build remains cryptographically reproducible (flake.lock).
4. Final image stays multi-arch (`x86_64-linux` + `aarch64-linux`) where the opt-in toolchain allows it.
5. Secret-scanning and backup/restore are closed as parity *gaps in both repos* — agentbox becomes strictly safer than MAD.
6. **Federated mode** (VisionClaw integration) is the **primary** deliverable. Agentbox speaks to the multi-container suite cleanly — beads, pods, durable memory, agent events all flow through typed adapters to VisionClaw Rust.
7. **Standalone mode** (native in-container fallbacks) is a **first-class** deliverable, shipped alongside federated — **just-barely-secondary** in priority, never a stub. Use cases: offline hacking, single-operator sessions, agentbox-as-MAD drop-in when VisionClaw isn't the host, reusable agent container for unrelated projects. Standalone must be credible as a production replacement on its own.

## 3. Non-goals

1. Not porting MAD's DinD host-bridge workflow. Agentbox builds via Nix, not via a Docker daemon inside a container — the bind-mount minefield that motivated `tmux send-keys -t 6` simply does not exist. Removing a workaround for a problem that no longer exists is a **win**, not a regression.
2. Not reintroducing Linux pseudo-user isolation. Profile isolation is the ratified model (CLAUDE.md §"Important Rules For Changes").
3. Not shipping a 2,379-line bash entrypoint. Supervisor blocks are generated from `agentbox.toml` by `flake.nix`.
4. Not coupling comfyui to VisionFlow unless explicitly gated on.
5. Not replacing Zellij with tmux.
6. **Not hosting Beads state or Solid pod storage inside agentbox (v3).** Those are VisionClaw Rust's responsibility in the federated deployment. Agentbox ships client adapters only. Local standalone fallback is a convenience, not the default — see §3a.
7. Not owning durable agent memory as a service. Durable memory belongs to external RuVector PG (P2.6 when enabled) or VisionClaw's Rust memory layer. Agentbox's embedded RuVector is a local-session cache, not a source of truth.

## 3a. Architectural principle: federation client, standalone-capable

Agentbox runs in two shapes without recompilation:

- **Federated mode** (default target): VisionClaw Rust is the source of truth for beads, Solid pods, and durable memory. Agentbox speaks to it over defined IPC (stdio, HTTP, MCP). Nostr identity is the cross-container auth substrate. Agentbox is a fast, disposable agent execution pool.
- **Standalone mode** (drop-in MAD replacement, offline hacking): VisionClaw absent. Agentbox fires up its own minimal fallbacks — local `bd`-equivalent sqlite, local Solid server, local JSONL agent-event sink. Enough to run a single-operator session without external infra. Not feature-complete vs federated.

The mode switch lives in `agentbox.toml`:

```toml
[federation]
mode = "client"          # "client" (federate with VisionClaw) | "standalone" (local-fallbacks)
visionclaw_url = ""      # set in client mode; e.g. http://visionclaw:7070 or stdio://
beads_adapter = "visionclaw"   # "visionclaw" | "local-sqlite" | "off"
pod_adapter = "visionclaw"     # "visionclaw" | "local-jss" | "off"
```

Derived rules:

- All three values default to `standalone` / `local-*` / `local-*` when `federation.mode = "standalone"`.
- `visionclaw` adapter values are rejected by `agentbox config validate` unless `mode = "client"` and `visionclaw_url` is set.
- Rows in §4 referencing beads/pods assume the adapter abstraction — the Nix package set differs per adapter (e.g. `local-jss` pulls the JS Solid server; `visionclaw` pulls none).

This preserves ADR-001 (Nix declarative), ADR-002 (embedded RuVector as local retrieval), and opens a clean story for the MAD deprecation: agentbox in `standalone` mode **is** the MAD replacement, without VisionClaw required.

## 4. Scope — Port In (in priority order)

### P0: Correctness & safety gaps (fix before any other work)

| # | Capability | MAD source | Agentbox target |
|---|---|---|---|
| P0.1 | Compose healthchecks + `service_healthy` gating | `docker-compose.unified.yml` L312-317, L75-77 | Add `healthcheck:` to `agentbox` service (`curl -f :9090/health`); order `ollama → agentbox` via `depends_on.condition` |
| P0.2 | Wire `https-bridge` into supervisord | MAD `supervisord.unified.conf` L453-462 | Add `[program:https-bridge]` block to generator in `flake.nix`, gated `[sovereign_mesh] https_bridge = true` |
| P0.3 | Secret-scanning in CI | Absent in both | Pre-commit + GitHub Action: `gitleaks` against staged files; fail PR on hit. New `.github/workflows/secret-scan.yml` |
| P0.4 | Scripted backup/restore | MAD has manual `backups/ruvector_backup_*.dump` | Add `agentbox.sh backup` / `restore` verbs; archive Solid pod + RuVector volume + `agentbox.toml` + generated supervisor; document in `docs/guides/backup-restore.md` |
| P0.5 | Bootable-with-zero-config defaults review | MAD needs 4 mandatory placeholders; agentbox has bad-sentinel mgmt key | Replace `change-this-secret-key` default with first-run auto-generated key persisted to `/workspace/profiles/<stack>/mgmt-key`; log once on startup |

### P1: Developer ergonomics (user feels these daily)

| # | Capability | MAD source | Agentbox target |
|---|---|---|---|
| P1.1 | VNC desktop — **Hyprland/Wayland** (Q1 resolved) | MAD baked-in (xvfb + x11vnc + openbox + tint2, port 5901) | `[desktop] enabled = false, stack = "hyprland-wayland"`. Nix: hyprland + waybar + wofi + foot + wayvnc (instead of x11vnc). Port 5901 exposed only when true. Compositor autostart via generated supervisor block. Document carefully in `docs/guides/desktop-hyprland.md` — VNC clients must speak WebSocket-capable VNC (e.g. TigerVNC 1.13+, noVNC); some corporate VNC clients lack Wayland support |
| P1.2 | code-server | MAD L990 + supervisord block | New `[toolchains.code_server] enabled = false` manifest key; Nix package + `[program:code-server]` block; port 8080 exposed only when true |
| P1.3 | CTM (Claude Telegram Mirror) | MAD supervised `ctm` daemon | New `[sovereign_mesh] telegram_mirror = false` gate; install `ctm` binary via Nix derivation built from `/home/devuser/workspace/claude-telegram-mirror-rs/`; generate supervisor block. Config at `~/.config/claude-telegram-mirror/config.json` — persist via profile-local mount |
| P1.4 | Zellij layout + tmux-compat alias (Q9 resolved) | MAD tmux 11-window layout | **Keep Zellij as the primary multiplexer.** Port MAD's tmux windows into Zellij tabs. Write `config/zellij/layouts/agentbox.kdl` with tabs: `claude`, `ruflo`, `qe`, `docs`, `build`, `logs`, `vcs`, `memory`, `llm`, `agents`, `host-shell` (tab 11 = the ex-"tmux tab 6" host-build tab, preserved even though DinD is gone — useful for out-of-band shell work). Bind `alias z='zellij'` in the default shell rc; also alias `zattach`, `zls`, `zkill` to `zellij attach/list-sessions/kill-session` for muscle memory |
| P1.5 | `.devcontainer/` for VS Code Remote | MAD has `.devcontainer/devcontainer.json` | New `agentbox/.devcontainer/devcontainer.json` that does `nix build .#runtime` in `onCreateCommand` and attaches via Zellij |
| P1.6 | **BeadsClient adapter** (Q5 resolved — client, not host, per §3a) | MAD `services/beads-service.js` (228 lines, wraps external `bd` CLI) | Rewrite as `agentbox/management-api/services/beads-client.js` with a pluggable backend interface. **`beads_adapter = "visionclaw"`** (default when federated): methods `createEpic/createChild/claim/close/...` forward over stdio or HTTP to VisionClaw Rust. **`beads_adapter = "local-sqlite"`** (standalone fallback): agentbox-internal sqlite schema implementing the same interface — enough for offline work, no git-sync, no multi-machine consensus. **`beads_adapter = "off"`**: BriefingService silently skips bead creation. User attribution via `userContext.pubkey` unchanged. `bd` binary is NOT packaged into agentbox — it stays in VisionClaw Rust. The 228-line MAD BeadsService becomes the `local-sqlite` adapter's reference implementation |
| P1.7 | **BriefingService + `/v1/briefs` routes** (Q5 resolved — port, adapter-aware) | MAD `services/briefing-service.js` (246 lines) + `routes/briefs.js` (392 lines) | Port the route handlers and role-guidance table verbatim. Storage backend pluggable: in `federation.mode = "client"`, brief files live in VisionClaw's pod store via `pod_adapter = "visionclaw"`; in standalone mode, fall back to local filesystem `/workspace/team/humans|roles/` (MAD layout). Manifest: `[management_api.briefs] enabled = true`. Role guidance (L231-241) moves to `config/briefing-roles.toml` (user-extensible per profile). BriefingService's `processManager.spawnTask` uses P1.8 stdio channel for agent spawning regardless of storage backend |
| P1.8 | **Agent spawn/monitor stdio channel for VisionClaw** (new, from 2026-04-23 clarification) | Implicit in MAD via `routes/tasks.js` + `routes/agent-events.js` + `utils/agent-event-publisher.js` + `utils/agent-event-bridge.js` + `hooks/agent-action-hooks.js` | Agentbox exposes an explicit contract: (a) `docker exec -i agentbox agentbox-agent-spawn <role> <prompt>` spawns an agent, streams stdout, maps signals. (b) `docker exec -i agentbox agentbox-agent-events --follow --format jsonl` streams agent lifecycle events (start, progress, tool-use, completion) as JSONL. (c) Each stdio event also published to `/v1/agent-events` HTTP endpoint and to the Nostr bridge (optional, as kind 30078 parameterised replaceable events, under `[sovereign_mesh] publish_agent_events = false`). VisionClaw can connect via either channel. Port from MAD: `routes/agent-events.js`, `routes/tasks.js`, both event utils, `hooks/agent-action-hooks.js` — all 5 files need auditing against agentbox's hybrid NIP-98 auth |

### P2: Integration & ecosystem

| # | Capability | MAD source | Agentbox target |
|---|---|---|---|
| P2.1 | External `visionclaw_network` network join (Q2 resolved) | MAD compose L105-110, L335-338 | `[integrations.ragflow] enabled = false, network = "visionclaw_network", aliases = ["agentbox"]`. `.env.template` ships with `RAGFLOW_NETWORK_ENABLED=false` (documented that the current DreamLab context needs it ON; OCI/cloud contexts OFF). `scripts/start-agentbox.sh` TUI auto-detects `docker network ls \| grep -q visionclaw_network` and offers to enable. Generator emits the compose `networks:` block only when true |
| P2.2 | AISP 5.1 Platinum spec bundle | MAD ships `aisp.md` (19 KB) | Copy to `agentbox/docs/aisp-5.1-spec.md`. Reference doc only — runtime `aisp/` code is already shared |
| P2.3 | **Google Gemini CLI** (replaces Gemini Flow, Q3 resolved) | MAD supervisord `gemini-flow` block | New `[toolchains.gemini_cli] enabled = false, version = "0.38.2"`. Nix fetches `@google/gemini-cli@0.38.2` (April 2026 official release — 1M context, Chapters narrative flow, Context Compression Service, Dynamic Sandbox + worktree support). Supervisor block runs daemon mode if configured. Zellij alias `zgemini`. Keep `GEMINI_API_KEY` + `GOOGLE_APPLICATION_CREDENTIALS` envs for Vertex AI path. **Do not port** MAD's `gemini-flow` — it's not a Google product and its replacement is first-party |
| P2.4 | claude-zai model upgrade | MAD: `glm-5` / "GLM 5 Coding Plan" + pinned `claude-code@2.1.47` | Update `claude-zai/claude-config.json` to GLM-5; pin `@anthropic-ai/claude-code` to the same version MAD uses; add the digest-pin SECURITY comment |
| P2.5 | Ontology MCP tools (7 × `ontology_*`) (Q4 resolved) | MAD `mcp-server.js` +186 lines | Port as `[skills.ontology] enabled = false` (default off). Nix derivation fetches the 7 tool definitions + ontology-core skill; generator registers them in MCP gateway only when enabled. Supports the Logseq OWL2 DL TBox workflow behind a flag |
| P2.6 | **External RuVector PostgreSQL backend** (new, from 2026-04-23 clarification) | MAD uses `ruvector-postgres:5432` sidecar in `docker-compose.unified.yml` + `RUVECTOR_PG_CONNINFO` env var pinned into `mcp.json` | Opt-in integration that coexists with ADR-002 (embedded remains default). Manifest: `[integrations.ruvector_external] enabled = false, conninfo = "postgresql://ruvector@ruvector-postgres:5432/ruvector"`. When enabled: (a) generator sets `RUVECTOR_PG_CONNINFO` + `RUVECTOR_PG_USE_EXTERNAL=true` in supervised processes, (b) `memory_store/search/retrieve` MCP tools route to external PG instead of embedded sql.js, (c) agentbox joins the `visionclaw_network` network (P2.1) to reach it, (d) startup healthcheck `pg_isready -h ruvector-postgres -p 5432` blocks service start. `.env.template` ships OFF; current DreamLab context flips ON. **ADR-005 required** documenting the "embedded OR external" dual-backend invariant |

### P3: Optional heavy toolchains

| # | Capability | MAD source | Agentbox target |
|---|---|---|---|
| P3.1 | NVIDIA CUDA stack (Q6 resolved) | MAD compose `runtime: nvidia`, L86-102 | `[toolchains.cuda] enabled = false, version = "13.1"`. Build-time flag: when true, Nix pulls cudaPackages_13_1 + cudnn + cuTensor; generator emits `runtime: nvidia` + `deploy.resources.reservations.devices`. Default off keeps base image lean; opt-in image expected to hit 25 GB compressed. Drives D.2 `[gpu] backend = "local-cuda"` |
| P3.2 | LichtFeld Studio + COLMAP + METIS (3DGS pipeline) | MAD built from source L234-302 | New `[skills.spatial_and_3d] gaussian_splatting = false`; pulls Nix derivations for COLMAP, METIS, LichtFeld. Requires `[toolchains.cuda] = true` (validator D.4 enforces) |
| P3.3 | Blender 5.0.1 | Already in `agentbox.toml [skills.spatial_and_3d] blender` (default false) | Verify Nix package resolves to 5.0.1; if not, add overlay |
| P3.4 | TeX Live full | Already in `[skills.docs] latex = true` | Verify Nix `texliveFull` attr is referenced, not a minimal subset |
| P3.5 | ComfyUI — built-in + external switches (Q7 resolved) | MAD v1.1.0 delegates to VisionFlow; agentbox has standalone v1.0.0 | Two independent manifest keys: **(a) `[skills.media.comfyui_builtin] enabled = false, version = "latest"`** — Nix packages the current online ComfyUI release inside agentbox; supervisor block on port 8188. **(b) `[integrations.comfyui_external] enabled = false, url = "http://comfyui.external:8188"`** — points skills at an external ComfyUI instance. Mutually exclusive: validator (D.4) rejects both enabled. The "upgrade standalone v1.0.0" question is resolved by always tracking current online ComfyUI release through nixpkgs |

## 4b. Scope — Agentbox design changes (not parity, improvements)

These are changes to agentbox's own design that the comparison surfaced as weak spots. Independent of MAD parity — shipping regardless.

| # | Change | Rationale | Effort |
|---|---|---|---|
| D.1 | Generate `docker-compose.yml` from `agentbox.toml` via `flake.nix`, same pattern as supervisord | Single source of truth. Right now supervisor is manifest-gated but compose is static — every new integration needs two edits in two places | 1 day |
| D.2 | Unify GPU story as `[gpu] backend = "ollama-rocm" \| "ollama-cuda" \| "local-cuda" \| "none"` | Replaces the implicit "ollama sidecar only" default with an explicit structural choice. Drives device mounts, `runtime:`, Nix packages from one key. Obsoletes P3.1/P3.2 as separate rows — they become derived facts | 2 days |
| D.3 | Promote API-key management from `.env` to `[providers.<name>]` manifest sections | 53 bare env vars → 2–3 enabled providers with their keys. Better defaults, better validation, fewer placeholders | 1 day |
| D.4 | Add `agentbox config validate` + JSON Schema for `agentbox.toml` | Surface semantic errors (e.g. `gaussian_splatting = true` without `gpu.backend = "local-cuda"`) before an 8-minute Nix build fails. TUI `start-agentbox.sh` consumes the schema | 2 days |
| D.5 | Extend `agentbox.sh` with local lifecycle verbs: `up`, `down`, `build`, `rebuild`, `logs`, `shell`, `health` | It's currently remote-OCI-shaped. As the daily local driver it needs local verbs. Keep all remote verbs intact | 1 day |
| D.6 | **Sovereign-mesh reshaped as client-first, per §3a** (Q10 resolved, v3 refined) | Solid + Nostr remain the federation substrate, but agentbox is a CLIENT, not a host. Servers (bd, JSS, beads store) live in VisionClaw Rust. Work: (a) `[sovereign_mesh] enabled = true` default — Nostr identity generation + NIP-98 auth + Nostr event publishing remain in-agentbox (client concerns); (b) **remove Solid-pod-server hosting from the default build** — port 8484 now only exposed when `pod_adapter = "local-jss"` (standalone fallback); (c) flesh out `nostr-bridge.js` beyond its 31-line stub as a CLIENT (connects to external relays, doesn't relay); (d) inter-agent messages get Nostr event kinds (30078 parameterised replaceable for agent state; NIP-33 addressable for briefs/beads refs); (e) document NIP-98 hybrid auth + the federated-vs-standalone split in `docs/guides/sovereign-mesh.md` and `docs/guides/federation-modes.md`; (f) `scripts/sovereign-bootstrap.py` trimmed to identity generation only (Nostr keys), pod provisioning moved to `scripts/standalone-bootstrap.py` for the offline fallback path | 3–5 days |
| D.7 | Keep Zellij + port MAD tmux layout + add aliases (Q9 resolved) | Ratified. Port the 11-window tmux layout into `config/zellij/layouts/agentbox.kdl`. Shell aliases: `z=zellij`, `zattach=zellij attach`, `zls=zellij ls`, `zkill=zellij kill-session`, plus compat `tmux-attach`, `tmux-ls` that route to zellij equivalents. Keeps muscle memory for users who drop into agentbox from a tmux-first environment | 0.5 day |
| D.8 | Delete `config/supervisord.conf` (marked legacy in CLAUDE.md) | Empty ambiguity. Either it's live and documented, or it's gone | 10 min |
| D.9 | Skills as content-addressed Nix derivations fetched from a single upstream git repo, not vendored in `skills/` | Per-skill versioning, reproducibility, and `nix flake lock --update-input skills` upgrade flow. Removes "is this skill current?" ambiguity | 1–2 days |
| D.10 | Consolidate `config/entrypoint-unified.sh` + `scripts/skills-entrypoint.sh` | Two overlapping scripts; unify or clearly bound with comments. Unclear-boundary tech debt | 0.5 day |
| D.11 | Make OCI provisioner pluggable: `scripts/provision-<target>.sh` for `oci`, `fly`, `hetzner`, `bare`; `agentbox.sh provision --target <x>` | OCI is hardcoded today in both the provisioning script and `.env.template`. Deploying anywhere else currently requires a rewrite | 2–3 days |
| D.12 | Auto-generate `SKILL-DIRECTORY.md` from enabled skills in `agentbox.toml`, or remove the empty file | Empty doc files erode trust. If it's an index, make it reflect manifest state | 0.5 day |
| D.13 | Finish `mcp/servers/nostr-bridge.js` (currently 31 lines — stub) or mark it experimental and exclude from default supervisor | Half-built features in default paths are landmines. Either ship or mark | Depends on scope — TBD |

## 5. Scope — Reject (with rationale)

| Capability | Rejection rationale |
|---|---|
| 2,379-line bash entrypoint | Contradicts manifest-gated supervisor generation. Explicitly out per CLAUDE.md §"Important Rules For Changes". |
| SSH + tmux tab 6 host-build workflow | Workaround for MAD's broken DinD bind-mounts. Agentbox builds via `nix build` on the host shell directly — the trap does not exist. |
| `SigLevel = Never` pacman bypass | Non-applicable. Nix does not use pacman; supply chain is content-addressed via `flake.lock`. |
| CachyOS `:latest` base | Replaced by `nixos-unstable` pinned by git revision. Determinism is non-negotiable (ADR-001). |
| Dockerfile.cachyos variant | Flake handles multi-arch and feature gates declaratively. |
| Hard-coded `/home/devuser/.claude/skills/...` paths in supervisord | Generated from manifest; paths computed per-profile. |
| Committed `node_modules/`, `dist/`, `target/`, `backups/*.dump` in-tree | Agentbox `.gitignore` (122 lines) already forbids this. Enforce via pre-commit. |
| Linux 4-user isolation (`devuser` / `zai-user` / `gemini-user` / `local-private`) | Explicitly forbidden by CLAUDE.md. Profile isolation is the chosen model. |
| `ruvector-postgres` sidecar | Superseded by embedded RuVector (ADR-002). PG is legacy. |
| tmux as primary terminal multiplexer | Zellij is the chosen replacement. Keep tmux available as a Nix package for user preference only. |
| `SYS_ADMIN` + `NET_ADMIN` + `SYS_PTRACE` caps + `/var/run/docker.sock:rw` mount | Container-escape surface. MAD self-warns against it (docker-compose.unified.yml L158-159). Agentbox keeps `no-new-privileges:true` and adds no caps. If a future feature genuinely needs docker socket access, raise it as a new ADR. |
| AppArmor + seccomp unconfined | Same rationale as above. |

## 6. Open questions

Nine of ten resolved. Resolutions captured in §4/§4b rows. Summary:

| Q | Topic | Resolution |
|---|---|---|
| Q1 | Desktop stack | **Hyprland/Wayland** — future-proof; document carefully (P1.1) |
| Q2 | ragflow network | **Env-configured switch**, off in template, TUI auto-detects (P2.1) |
| Q3 | Gemini orchestrator | **Swap Gemini Flow for official `@google/gemini-cli` v0.38.2** (P2.3) |
| Q4 | Ontology MCP tools | **Port, default off** under `[skills.ontology]` (P2.5) |
| Q6 | CUDA toolchain | **Build-time flag, default off** (P3.1 / D.2) |
| Q7 | ComfyUI | **Two switches: built-in (nixpkgs current) + external URL**, mutually exclusive, both default off (P3.5) |
| Q8 | Multi-project mounts | **Single `/projects` is fine** for now — revisit if need emerges |
| Q9 | Terminal multiplexer | **Keep Zellij**, port MAD tmux 11-tab layout to Zellij, add shell aliases (D.7) |
| Q10 | Sovereign-mesh commitment | **Core ecosystem — default ON, first-class**. Inverts D.6. Agentbox becomes the reference test environment for the JSS Rust port (D.6) |

**Q5 — RESOLVED 2026-04-23 (port both, promoted to P1)**
Direct reading of MAD source (`services/beads-service.js` 228 lines, `services/briefing-service.js` 246 lines, `routes/briefs.js` 392 lines) confirmed the contract:

- **BeadsService** wraps an external `bd` CLI for agent-work receipts: epic/child hierarchy, atomic claim (compare-and-swap), blocks/blocked-by dependencies, JSONL-to-git sync, user attribution via Nostr pubkey tags (already sovereign-mesh-native — `BEADS_ACTOR: visionflow/{display_name}`, `pubkey:{hex[:8]}`).
- **BriefingService** orchestrates brief → execute-per-role → debrief in a `team/humans/` and `team/roles/` tree; spawns role-specific agents (architect, dev, ciso, designer, dpo, devops, appsec, advocate) via `processManager.spawnTask`; creates one epic bead per brief, one child bead per role.
- **Why port**: user confirms VisionClaw depends on beads for long-running multi-input agent actions. BriefingService requires BeadsService (constructor dependency). Both promoted to P1.6/P1.7. Stdio channel in P1.8 is how VisionClaw orchestrates the work.
- **Open sub-question (Q5a, not blocking)**: `bd` CLI lives in VisionClaw Rust (not in agentbox). Agentbox writes a thin client adapter. Still useful: confirm the VisionClaw-side bead API shape — HTTP REST? stdio JSON-RPC? MCP? — so the adapter targets the right transport.
- **Open sub-question (Q5b, not blocking)**: VisionClaw dir not found on this container (only `visioninglab/Campaigns/popupview_*`). Is it a separate Rust repo on your side, a remote service, or an in-progress rename of `visioninglab`? Answer shapes the stdio channel wiring — `docker exec -i` (same-host) vs TCP/WS (remote) vs MCP (structured). Point me at the repo when convenient and I can draft the adapter interface against its real API.

## 7. Milestones

Exit criteria for every milestone are **passable/failable predicates** — no prose. Every predicate either runs green in CI or doesn't; no subjective interpretation.

| Milestone | Scope | Duration | Exit predicates (ALL must pass) |
|---|---|---|---|
| M1 — Safety floor | All P0 + D.8 + D.12 | 3–5 days | (a) `nix build .#runtime` cold-cache produces identical `sha256` across two consecutive runs; (b) `curl -f http://localhost:9090/health` → HTTP 200 within 30 s of `docker compose up`; (c) CI refuses a PR containing canary secret `AWS_SECRET_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE`; (d) `agentbox.sh backup && docker volume rm $(docker volume ls -q -f name=agentbox) && agentbox.sh restore` yields byte-identical SQLite + Solid-pod tree (checksum verified); (e) `test ! -e agentbox/config/supervisord.conf` (file gone); (f) contract-test harness skeleton exists at `agentbox/tests/contract/<slot>.contract.spec.js` × 5 with ≥1 passing assertion per suite; (g) `agentbox/tests/reproducibility/nix-build-hash.sh` exits 0 on host |
| M2 — Daily ergonomics | P1.1–P1.8 + D.5 | 2–3 weeks | (a) `wayvnc` serves 5901 with Hyprland compositor; noVNC handshake green from reference client; (b) `POST /v1/briefs` creates one epic bead + N child beads — `bd list --epic <id>` count = role count; (c) `docker exec -i agentbox agentbox-agent-spawn <role> <prompt>` emits ≥1 `spawn` + ≥1 `completion` JSONL event within 60 s; (d) `zellij --layout agentbox.kdl attach` loads all 11 tabs error-free; (e) `code-server` responds on 8080 with HTTP 200; (f) `ctm status` reports daemon running when `[sovereign_mesh].telegram_mirror = true`; (g) contract harness green for `beads` + `orchestrator` local-* impls |
| M3 — Ecosystem | P2.1–P2.6 + D.1/D.2/D.3/D.4/D.6 | 2–3 weeks | (a) `agentbox config validate` exits non-zero for each of the ~16 semantic rules with a rule-specific stderr regex; (b) `[integrations.ragflow].enabled = true` → `getent hosts ragflow` resolves from inside agentbox; (c) `[adapters.memory] = "external-pg"` → `memory_store` writes verifiable via `psql -c "SELECT count(*) FROM memory_entries"`; (d) `GET /v1/meta` returns the five adapter contract versions in the shape declared in ADR-005; (e) LocalFallbackProbe succeeds against all five external endpoints in the CI integration run; (f) `@google/gemini-cli` v0.38.2 responds to `--help` via the daemon block; (g) contract harness green across all three implementation classes per slot |
| M4 — Heavy toolchains | P3.1–P3.5 | 1–2 weeks | (a) `[toolchains.cuda] = true` builds a CUDA-13.1 image that passes `nvidia-smi` inside; (b) `[skills.spatial_and_3d].gaussian_splatting = true` builds COLMAP + METIS + LichtFeld and passes a single-scene reconstruction smoke test; (c) `[toolchains.cuda] = false` compressed image stays under 4 GB (measured via `docker image inspect`); (d) `agentbox config validate` rejects `gaussian_splatting = true` without `gpu.backend = "local-cuda"` with the specific error message |
| M5 — Parity sign-off | Migration rehearsal | 3–5 days | (a) `project/tests/parity/mad-capability-matrix.yaml` shows 100% rows green or explicit-ignore with signed justification per ignored row; (b) user runs a full working day on agentbox — rehearsal log committed with a signed-off-by trailer; (c) rollback script `project/scripts/rollback-to-mad.sh` restores MAD from the frozen-container snapshot in under 5 minutes (timed in CI); (d) aarch64 matrix build of `.#runtime` variant produces a loadable image (CUDA opt-in legitimately skipped on aarch64); (e) MAD-state replay completeness check: every MAD `bd` epic and every MAD brief file has a corresponding entry in the post-migration agentbox state |

**Total engineering estimate:** 3–5 weeks + 1 week validation, matching the Lineage agent's ROM in the comparison report.

## 8. Success criteria

1. `nix build .#runtime` on the host shell produces a deterministic image — `flake.lock` unchanged between two runs means identical image hash.
2. `agentbox.sh status` reports all supervised services healthy; no manual `supervisorctl` required to diagnose first boot.
3. `agentbox.sh backup` → destructive rebuild → `agentbox.sh restore` recovers Solid pod + RuVector + profile state with zero data loss.
4. A secret committed to a test branch is caught by gitleaks before merge.
5. User can stop `agentic-workstation` container and run full daily workflow on `agentbox` for one week without unresolved regressions.
6. Final `agentbox/` on-disk size stays under 50 MB source (excluding any vendored fixtures strictly required by tests); MAD's 89 GB is not a ceiling to approach.
7. CUDA opt-in image stays under 25 GB compressed; base runtime stays under 4 GB compressed.

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Nix build fails on the CachyOS host due to exotic kernel/driver | Validate M1 on the actual host first, not in CI. If fails, raise as ADR-005 before proceeding to M2. |
| CTM Rust binary fails to package cleanly via Nix | Fall back to `buildRustPackage` fetching the source repo pinned to a commit; if that fails, ship as opt-in external systemd mount. |
| ragflow network drift — peer names/ports change between MAD build and agentbox migration | Lock peer contract in `agentbox.toml [integrations.ragflow]`; document each peer name/port the user actually calls. |
| 3DGS toolchain (LichtFeld/COLMAP) needs specific CUDA/cuDNN versions not in nixpkgs | Author nixpkgs overlay, upstream where possible; pin overlay commit in `flake.lock`. |
| **Hyprland/Wayland VNC incompatibility with existing VNC clients** | Document tested client matrix in `docs/guides/desktop-hyprland.md` (TigerVNC 1.13+, noVNC confirmed). Ship fallback: `[desktop] stack = "x11-openbox"` option using xvfb+openbox+tint2 (the MAD stack) for users stuck on older clients. Flip default only after 1-month validation window. |
| **Gemini CLI 0.38.2 churn** — it's active April 2026 release, next update could break config schema | Pin version in `flake.lock`; subscribe to release notes; ship a version-pin bump as its own PR with migration notes. Don't auto-follow `@latest`. |
| **ComfyUI built-in vs external mutual exclusion confusion** | `agentbox config validate` (D.4) rejects both-enabled at manifest parse time with clear error. TUI flow offers radio-button choice, not two checkboxes. |
| **JSS Rust port lag** — D.6 includes `[sovereign_mesh] jss_rust_backend = false` gate; if the Rust port slips, sovereign-mesh stays on JS JSS | Keep JS JSS as default backend; flip to Rust when the port hits parity. No blocker for agentbox deployment. |
| User workflow regressions we didn't predict | M5 rehearsal week; keep MAD container frozen for one-command rollback (`docker start agentic-workstation`) for 30 days post-migration. |

## 10. Deprecation plan for multi-agent-docker

After M5 sign-off:
1. Freeze MAD — stop making changes, keep container stopped, preserve compose file and named volumes for 30 days.
2. Extract MAD's three durable documents — `SSH-SETUP.md`, `DOCKER-BUILD-NOTES.md`, `aisp.md` — into `agentbox/docs/legacy/`.
3. Archive MAD backups (`ruvector_backup_*.dump`) to cold storage (Solid pod + off-container copy).
4. Remove `multi-agent-docker/` from the repo at T+30 days with a single commit citing this PRD.
5. `ADR-005` documents the deprecation decision and the one-command rollback runbook.

## 11. References

- `agentbox/docs/adr/ADR-001-nixos-flakes.md`
- `agentbox/docs/adr/ADR-002-ruvector-standalone.md`
- `agentbox/docs/adr/ADR-003-guidance-control-plane.md`
- `agentbox/docs/adr/ADR-004-upstream-sync.md`
- `agentbox/CLAUDE.md` (architectural constraints)
- RuVector memory namespace `agentbox-comparison` (6 keys — full evidence base from the 2026-04-23 comparison swarm)
