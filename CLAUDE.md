# Claude Code Configuration - Claude Flow V3

## Security Audit Sprint (2026-05-11)

A DreamLab ecosystem-wide security audit applied 14 fixes to VisionClaw.
See CHANGELOG.md `[Security Audit Sprint] - 2026-05-11` for the full
manifest. Key areas hardened: NIP-98 auth verification (P0-04, P0-05),
supervisor restart storms and heartbeat reliability (P1-21, P1-22),
dense-matrix OOM protection (P1-23), deterministic hashing via blake3
(P1-24), supervision strategy (P2-04), payment validation (P2-05,
P2-06), physics NaN/Inf rejection (P2-07), and CUDA grid lock
thresholds (P2-08).

## Agent container subsystems (2026-04-26)

VisionClaw's agent-container subsystem is in active migration:

- **`multi-agent-docker/`** — legacy; on deprecation track per [ADR-058](docs/adr/ADR-058-mad-to-agentbox-migration.md). No new features land here. Running as `agentic-workstation` on ports 9090/8080/5901/2222.
- **`agentbox/`** — git submodule (`github.com/DreamLab-AI/agentbox`); Nix-based v2 replacement. Federation/standalone adapter architecture per [agentbox ADR-005](agentbox/docs/reference/adr/ADR-005-pluggable-adapter-architecture.md). Integration contract with VisionClaw Rust substrate per [PRD-004](docs/PRD-004-agentbox-visionclaw-integration.md) and [DDD BC20](docs/ddd-agentbox-integration-context.md). Running side-by-side on ports 9190/8180/5902/2223. Feature gap analysis: [docs/gap-analysis-mad-vs-agentbox.md](docs/gap-analysis-mad-vs-agentbox.md). Agentbox docs symlinked at [docs/agentbox-docs/](docs/agentbox-docs/).

**Side-by-side port mapping:**

| Service | MAD (legacy) | Agentbox (new) |
|---------|-------------|---------------|
| Management API | 9090 | 9190 |
| Code Server | 8080 | 8180 |
| VNC Desktop | 5901 | 5902 |
| SSH | 2222 | 2223 |
| Solid Pod | — | 8484 |
| Agent Events | — | 9700 |
| Metrics | — | 9191 |

### Ecosystem AI Services (visionclaw_network)

Shared Docker network: `visionclaw_network`. Managed via `./scripts/launch.sh ecosystem`.

| Service | Container | GPU | Host Port | Internal Hostname |
|---------|-----------|-----|-----------|-------------------|
| Kokoro TTS | `kokoro-tts-container` | device=2 | 8880 | `kokoro-tts:8880` |
| Whisper WebUI | `whisper-webui-backend` | device=1 | 8000 | `whisper-webui-backend:8000` |
| Xinference | `xinference` | all | 9997 | `xinference:9997` |

Symlinked repos (in `.gitignore`): `./Kokoros`, `./Whisper-WebUI`, `./xinference`.

Launch commands: `./scripts/launch.sh ecosystem` (start all), `ecosystem-down` (stop all), `ecosystem-status` (health check). The `--with-ecosystem` flag on `up`/`restart` starts these alongside VisionFlow.

Task-routing guidance: any task touching agent-container build, supervisord, or durable-state adapters belongs in `agentbox/`; any task consuming agentbox from the VisionClaw actor mesh is a BC20 concern and belongs in `src/actors/` with ACL wiring in this repo.

## MEMORY FIRST (Reinforced)

Before ANY task: search memory. After ANY success: store pattern. Non-negotiable.

```javascript
// Search before working
mcp__claude-flow__memory_search({query: "[task keywords]", namespace: "patterns", limit: 10})
// Store after success
mcp__claude-flow__memory_store({namespace: "patterns", key: "[name]", value: "[what worked]"})
```

ONLY MCP tools: `mcp__claude-flow__memory_store/search/list/retrieve/stats`
NEVER `claude-flow memory *` CLI.

## Auto-Learning Protocol

### Before Starting Any Task
```javascript
mcp__claude-flow__memory_search({query: "[task keywords]", namespace: "patterns", limit: 10})
mcp__claude-flow__memory_search({query: "[task type]", namespace: "tasks", limit: 5})
Bash("claude-flow hooks route --task '[task description]'")
```

### After Completing Any Task
```javascript
mcp__claude-flow__memory_store({namespace: "patterns", key: "[pattern-name]", value: "[what worked]"})
Bash("claude-flow hooks post-edit --file '[main-file]' --train-neural true")
Bash("claude-flow hooks post-task --task-id '[id]' --success true --store-results true")
```

### Memory-Enhanced Development
**Check memory before**: new features, debugging, refactoring, performance work
**Store after**: bug fixes, completions, performance fixes, security discoveries

### Improvement Workers

| Trigger | Worker |
|---------|--------|
| Major refactor | `optimize` |
| New features | `testgaps` |
| Security changes | `audit` |
| API changes | `document` |
| 5+ file changes | `map` |
| Complex debug | `deepdive` |

## Intelligent Skill Selection

Before starting any task, select the optimal orchestration approach. The decision
tree below routes tasks to the right skill based on scope, complexity, and domain.

### Quick Decision Tree

```
Don't know which skill? --> /route [describe your task]  (unified dispatcher)

Single file, quick fix? --> Direct Edit (no skill needed)
Game dev (Godot/Unity/Unreal)? --> /game-dev
XR (Quest 3 native APK, Godot 4 + godot-rust + OpenXR, hand tracking, passthrough)? --> /game-dev or /rust-development (see docs/explanation/xr-architecture.md, ADR-071, PRD-008; XR project lives at xr-client/, gdext crates at crates/visionclaw-xr-{gdext,presence}/)
Bug/feature/review (single agent)? --> lazy-fetch blueprints
Multi-file feature with TDD? --> /build-with-quality
Large codebase structural analysis (call graphs, arch, diff)? --> codebase-memory
Need current library docs while coding? --> context7
Swarm (3+ agents)? --> /swarm-advanced or hive-mind
GitHub ops (PR/release/CI)? --> github-* skills
Docs/reports? --> /report-builder, /docs-alignment
Media (image/video/3D)? --> /imagemagick, /ffmpeg, /blender, /comfyui
Browser automation (GPU/WebGPU)? --> browsercontainer (agentbox/browsercontainer; `agentbox.sh browsercontainer up`; SSE at browsercontainer:8931, VNC :5903)
Browser automation (simple)? --> /playwright, /browser-automation, /qe-browser (QE-grade: typed assertions, visual-diff, injection scan)
AI/ML (PyTorch, CUDA, notebooks)? --> /pytorch-ml, /cuda, /jupyter-notebooks
Memory/AgentDB? --> /agentdb-*, /lazy-fetch
Wardley maps / strategic analysis? --> /wardley-maps, /report-builder
UI/UX design? --> /ui-ux-pro-max-skill, /bencium-*, /design-audit, /typography
Architecture review? --> /vanity-engineering-review, /renaissance-architecture, /human-architect-mindset
Research + NotebookLM? --> /notebooklm, /perplexity-research, /gemini-url-context
Deep cited research? --> /deep-research (parallel agents, provenance, verification)
Optimize a metric iteratively? --> /autoresearch (experiment loop, keep/discard)
Add source verification? --> /provenance-tracking (.provenance.md sidecar)
Security / compliance? --> /defense-security
AEC (building architecture)? --> /studio [task]
SEO / content optimisation? --> /toprank
```

Full routing with all 92 active skills: see `multi-agent-docker/skills/SKILL-DIRECTORY.md`

### Skill Capabilities Matrix

| Skill | Agents | Scope | Memory | Best For |
|-------|--------|-------|--------|----------|
| **lazy-fetch** | 1 (self) | Single session | RuVector bridge | Context discovery, plan tracking, blueprints, security scan |
| **game-dev** | 48 | Game project | Session state files | Game design, engine-specific code, team orchestration |
| **ruflo/claude-flow** | 1-15 | Multi-agent | RuVector native | Swarm coordination, complex features, cross-module work |
| **build-with-quality** | 111+ | QE pipeline | RuVector native | Testing, quality gates, coverage analysis |
| **sparc-methodology** | 5-8 | Full lifecycle | RuVector native | Specification through completion |

### Combining Skills

Skills compose. A single task may use multiple skills in sequence:

1. `lazy gather "combat system"` -- discover relevant files first
2. `/game-dev team-combat "melee attacks"` -- orchestrate the game dev team
3. `lazy check` -- validate the implementation
4. `lazy remember "combat" "melee uses hitbox detection, not raycasts"` -- persist

For ruflo swarms, each spawned agent uses worktree isolation:
- Agent spawns in worktree (`wt-add <name>` for Ruflo, `isolation: "worktree"` for Agent tool)
- Runs `lazy init` + `lazy gather` for its task scope
- Implements using lazy-fetch plan tracking
- Results merge back via ruflo coordination or git merge

For massive parallelisable work (migrations, bulk refactors), use `/batch` — it fans out to N worktree agents automatically.

### Session Start Protocol

1. Search RuVector memory for task context
2. Run `lazy read` if working in a lazy-fetch-initialised project
3. Check hooks route for skill recommendation
4. Select skill from decision tree above
5. Begin work

## Swarm Orchestration

### 3-Tier Model Routing (ADR-026)

| Tier | Handler | Latency | Cost | Use Cases |
|------|---------|---------|------|-----------|
| **1** | Agent Booster | <1ms | $0 | Simple transforms -- Skip LLM |
| **2** | Haiku | ~500ms | $0.0002 | Simple tasks, low complexity |
| **3** | Sonnet/Opus | 2-5s | $0.003-0.015 | Complex reasoning, security |

`[AGENT_BOOSTER_AVAILABLE]` -> Edit tool directly
`[TASK_MODEL_RECOMMENDATION]` -> use recommended model in Task tool

### Agent Routing

| Code | Task | Agents |
|------|------|--------|
| 1 | Bug Fix | coordinator, researcher, coder, tester |
| 3 | Feature | coordinator, architect, coder, tester, reviewer |
| 5 | Refactor | coordinator, architect, coder, reviewer |
| 7 | Performance | coordinator, perf-engineer, coder |
| 9 | Security | coordinator, security-architect, auditor |
| 11 | Docs | researcher, api-docs |

### Task Complexity

**Swarm**: 3+ files, new features, cross-module, API+tests, security, performance, DB schema
**Skip**: single file, 1-2 line fix, docs, config, questions

## Project Config

| Setting | Value |
|---------|-------|
| Topology | hierarchical-mesh |
| Max Agents | 15 |
| Strategy | specialized |
| Consensus | raft |
| Memory | hybrid |
| HNSW | Enabled |
| Neural | Enabled |

## Tool Delineation

**Task tool**: ALL execution (agents, files, code, git)
**CLI (Bash)**: `claude-flow swarm init`, `claude-flow hooks *`, `claude-flow agent spawn`
**MCP (MANDATORY)**: `mcp__claude-flow__memory_{store,search,list,retrieve}`

## RuVector PostgreSQL

See `multi-agent-docker/CLAUDE.md` for full RuVector connection details, schema,
SQL examples, and extension capabilities. Summary:

- Host: `ruvector-postgres:5432` | DB: `ruvector` | Connection: `$RUVECTOR_PG_CONNINFO`
- ALWAYS use MCP tools (`mcp__claude-flow__memory_*`), never CLI
- Tables: `memory_entries` (1.17M+), `patterns`, `reasoning_patterns`, `session_state`

## Hive-Mind

Topologies: `hierarchical`, `mesh`, `hierarchical-mesh` (recommended), `adaptive`
Strategies: `byzantine` (f<n/3), `raft` (f<n/2), `gossip`, `crdt`, `quorum`

## Session Persistence
```bash
claude-flow session restore --latest                                              # Start
claude-flow hooks session-end --generate-summary true --persist-state true        # End
```

## Architecture Patterns / Wire formats

The GPU position stream uses a single binary protocol — there are no versions.
Single-source spec: [`docs/binary-protocol.md`](docs/binary-protocol.md)
(authoritative ADR: [ADR-061](docs/adr/ADR-061-binary-protocol-unification.md);
domain model: [ddd-binary-protocol-context](docs/ddd-binary-protocol-context.md)).
The 24-byte/node frame carries position + velocity only; sticky GPU outputs
ride a separate `analytics_update` JSON message.

Two parallel URI namespaces exist by design:
- **`urn:visionclaw:*`** — Rust substrate, 6 kinds (`concept`, `kg`, `bead`, `execution`, `group`), minted in `src/uri/` (mint.rs, parse.rs, kinds.rs). Grammar: `urn:visionclaw:<kind>:<hex-pubkey>:<local>`. Owner-scoped kinds (`kg`, `bead`) use 64-char hex pubkey as scope (not bech32 npub).
- **`urn:agentbox:*`** — JS management API, 18 kinds, minted in `management-api/lib/uris.js`. Grammar: `urn:agentbox:<kind>:[<hex-pubkey>:]<local>`. Canonical ref: agentbox ADR-013.
- Both share `did:nostr:<hex-pubkey>` identity and `sha256-12-<12 hex chars>` content addressing. Hex pubkey is the canonical scope form everywhere; bech32 npub is only used at the Nostr relay wire boundary and in legacy pod filesystem paths.
- BC20 anti-corruption layer maps between them at the federation boundary (planned, see [DDD BC20](docs/ddd-agentbox-integration-context.md)).

## Codebase Memory MCP (ACTIVE — USE FIRST)

`codebase-memory-mcp` is indexed for this project (`home-devuser-workspace-project`, 48,159 nodes, 95,766 edges). Use these tools BEFORE Grep/Glob/Read for all structural queries.

| Query Type | Tool | Instead of |
|---|---|---|
| Who calls function X? | `trace_path(function_name, direction="callers")` | Grep |
| Module/route architecture | `get_architecture(project, aspects=[...])` | Manual file reading |
| Find struct/fn by name | `search_graph(project, query)` | Glob |
| Git diff impact + risk score | `detect_changes(project)` | Manual inspection |
| Fetch function source | `get_code_snippet(qualified_name)` | Read |
| Check if index is current | `index_status(project)` | — |
| Re-index after large changes | `index_repository(repo_path)` | — |

**Project ID**: `home-devuser-workspace-project`
**Session start**: run `index_status` — if `needs_reindex: true`, run `index_repository`.
**Token savings**: ~99.2% vs grep (3,400 vs 412,000 tokens for 5 structural queries).

## Ecosystem & Federation (mega-sprint 2026-05-07)

VisionClaw (this monorepo) is the **integration substrate** for a 5-project ecosystem federated via did:nostr. Master spec lives here; consuming substrates pull spec via cross-repo references.

### Five-substrate landscape

| Substrate | GitHub | Local path | Role |
|-----------|--------|------------|------|
| **VisionClaw** (this repo) | `https://github.com/DreamLab-AI/VisionClaw` | `/home/devuser/workspace/project/` | Integration substrate; master fixture host (per ADR-082); knowledge-graph + XR; mesh peer |
| **nostr-rust-forum** (kit upstream; product `nostr-bbs-rs`; internal brand "VisionFlow forum") | `https://github.com/DreamLab-AI/nostr-rust-forum` | `/home/devuser/workspace/nostr-rust-forum/` | Generic configurable forum kit; consumed by N operators |
| **dreamlab-ai-website** (kit's flagship downstream consumer) | `https://github.com/DreamLab-AI/dreamlab-ai-website` | `/home/devuser/workspace/dreamlab-ai-website/` | DreamLab's branded forum deployment; will gain `forum-config/` package per PRD-012 |
| **agentbox** (sovereign agent container; mesh peer) | `https://github.com/DreamLab-AI/agentbox` | `/home/devuser/workspace/project/agentbox/` (submodule) | Nix-based; pod-bridge + nostr-rs-relay + mesh peer + skill provider |
| **solid-pod-rs** (foundation library) | `https://github.com/DreamLab-AI/solid-pod-rs` | `/home/devuser/workspace/solid-pod-rs/` | LDP / WAC / WebID / NIP-98 / DID Tier-3 — consumed by all other substrates |

### Spec stack (this monorepo's `docs/`)

3 PRDs, 13 ADRs, 1 DDD context map, ~12,000 lines of cross-substrate research.

| Artefact | Purpose |
|----------|---------|
| `docs/PRD-010-did-nostr-mesh-federation.md` | Master spec for cross-substrate did:nostr mesh federation |
| `docs/PRD-011-visionflow-forum-kit-extraction.md` | Extracting the kit from `dreamlab-ai-website/community-forum-rs/` |
| `docs/PRD-012-dreamlab-ai-website-kit-adoption.md` | DreamLab website becoming a downstream consumer |
| `docs/adr/ADR-073-private-nostr-relay-mesh-topology.md` | Mesh topology + NIP-42 AUTH gate |
| `docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md` | DID:Nostr canonicalisation + NIP-26 trust pivot |
| `docs/adr/ADR-075-is-envelope-message-contract.md` | IS-Envelope v1 — cross-system message contract |
| `docs/adr/ADR-076-nostr-core-absorption-into-upstream.md` | Forum nostr-core → upstream `nostr` crate (rust-nostr.org) |
| `docs/adr/ADR-077-ecosystem-qe-policy.md` | 10 QE policies (P1-P10) governing all substrates |
| `docs/adr/ADR-078-cross-substrate-library-convergence.md` | Library convergence registry |
| `docs/adr/ADR-079-forum-setup-skill-provider-abstraction.md` | AI configurator with provider-abstracted backend |
| `docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md` | 6 canonical deployment topologies |
| `docs/adr/ADR-081-federation-key-custody-rotation.md` | Federation key custody (Tier-1/2/3) + rotation protocol |
| `docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md` | Single source of truth for reference vectors |
| `docs/adr/ADR-083-dreamlab-ai-website-cutover-migration.md` | Cutover mechanics (feature-flag + dual-deploy) |
| `docs/adr/ADR-084-cloud-infrastructure-mapping-for-kit-consumers.md` | CF resource ID preservation for kit consumers |
| `docs/adr/ADR-085-forum-config-package-architecture.md` | `forum-config/` consumer package shape |
| `docs/ddd-mesh-federation-context.md` | DDD bounded-context map (V1: 4 substrates; V2: 5-substrate kit; V13: consumer aggregates; V14: full ecosystem) |
| `docs/specs/fixtures/` | 13 cross-substrate reference vector fixtures (paulmillr/nip44, BIP-340, RFC 8785, etc.) |
| `docs/integration-research/` | 12,000-line audit corpus (6 specialist research + 5-agent QE fleet + 3 validators) |

### Cross-system identity

`did:nostr:<64-lowercase-hex>` is the universal identity primitive (per ADR-074 D1). Every substrate emits DID Documents with `verificationMethod.type = SchnorrSecp256k1VerificationKey2019` and `@context` including `https://w3id.org/security/suites/secp256k1-2019/v1`. NIP-26 delegation is the cross-system trust pivot.

### IS-Envelope v1 (cross-system message contract)

Per ADR-075: 7 envelope kinds (chat / tool_invoke / tool_result / knowledge_link / moderation / mesh_ping / unknown), JCS-canonicalised, NIP-59 gift-wrap on the wire, AS2 LDN mapping at the Solid pod inbox boundary.

### Mega-sprint memory keys (RuVector `project-state` namespace)

For context recovery in future sessions:
- `mega-sprint-2026-05-07-phase-0-charter` / `phase-0-final-report`
- `mega-sprint-2026-05-07-phase-1-charter` / `phase-1-final-report`
- `mega-sprint-2026-05-07-phase-1-charter-addendum-cargo-check`
- `mega-sprint-2026-05-07-cargo-check-matrix`
- `mega-sprint-2026-05-07-commit-batches-final` (4-repo branch list + commit hashes)
- `mega-sprint-phase-2-kit-extraction-charter` / `phase-2-kit-extraction-final-report`
- `prd-012-adr-084-085-ddd-v13-summary`
- `qe-fleet-comprehensive-findings-2026-05-07`
- `prd-010-mesh-federation-summary`
- `deferred-task-reasoningbank-revisit-2026-05-10` (cron set 2026-05-10T10:37 local)
- `hybrid-workflow-checkpoint-adr-080-pre-snapshot` / `post-verification` (adr-architect agent eval)

### What's done (2026-05-07 mega-sprint)

Phase 0 — gating crypto fixes (5 critical bugs C1-C5) + sync infra + Rust CI workflow + F1 identity unification + F26 canary spike crate. 13/13 deliverables. 4 cross-repo branches (`mega-sprint/2026-05-07`) with 13 logical commits, NOT pushed.

Phase 1 — vector vendoring (10 of 13 fixtures completing 13/13 corpus) + L1 reference-vector test scaffolds in 4 substrates + block_level_parser:209 URN drift fix. 14/14 deliverables.

Phase 2 (in flight at time of writing) — kit extraction X0: import C1+C5 fixes + F26 canary + L1 tests + scripts into newly-cloned nostr-rust-forum.

### What's next (Phase 3+ roadmap)

- Phase 3 — full Sprint v9-v11 feature absorption into kit (NIP-98 replay store, profiles backfill, username reservations, mesh service-list scaffolding, etc.) — the full PRD-011 X1 workspace restructure
- Phase 4 — kit GA (`v3.0.0`): crates.io publish + ADR-077 P1-P10 compliance + Sprint Carry-Over Fixture Suite green
- Phase 5 — `dreamlab-ai-website` `forum-config/` consumer package per PRD-012 X1
- Phase 6 — production cutover per ADR-083 (14-day window with traffic split + dual-deploy + parity monitoring)
- Phase 7 — cleanup: `community-forum-rs/` deletion T₇+7
- Cross-cutting: 2026-05-10 reminder to revisit V3 ReasoningBank + adr-architect re-engineering

Total estimate per PRD-010: ~5 sprints @ 1 FTE post-Phase-2 to reach P5 + cutover.
