# ADR-058: Deprecate multi-agent-docker in favour of agentbox

**Status:** Accepted
**Date:** 2026-04-23
**Author:** VisionClaw platform team
**Supersedes:** — (no prior formal decision; MAD accumulated organically)
**Related:** PRD-004 (agentbox ↔ VisionClaw integration), `docs/ddd-agentbox-integration-context.md`, `docs/ddd-bead-provenance-context.md`, agentbox ADR-001 (Nix flakes), agentbox ADR-002 (RuVector embedded), agentbox ADR-005 (Pluggable adapter architecture)

## Context

VisionClaw's agent-container subsystem is `multi-agent-docker/` (MAD). A 2026-04-23 architectural audit established the following:

- MAD is a 1,188-line `Dockerfile.unified` with a 2,379-line bash entrypoint, non-reproducible build (`:latest` base, 20+ `@latest` npm tags, `curl \| bash` installers, unpinned git clones), 89 GB on-disk, containing committed `node_modules/`, `target/`, `dist/`, and 85 MB of postgres dumps in-tree.
- MAD mounts `/var/run/docker.sock:rw` and adds `SYS_ADMIN`+`NET_ADMIN`+`SYS_PTRACE` capabilities — a container-escape surface that MAD's own compose file warns against (lines 158–159).
- MAD was built before VisionClaw's bounded-context model matured. It hosts BeadsService, BriefingService, Solid pod server, and ruvector-postgres inside the same container — boundaries that VisionClaw now owns via Rust actors in `src/actors/` and `src/domain/`.
- MAD's daily workflow requires a tmux-tab-6 host-shell workaround because its Docker-in-Docker setup silently breaks bind-mount path resolution when builds are issued from inside the container.

A sibling directory `agentbox/` was created at `project/agentbox/` for a clean-room redesign. Its architectural properties (summarised):

- Nix flake build pinned by `flake.lock`; two builds produce the same image hash.
- Manifest-gated features via `agentbox.toml`: enabling a feature pulls its Nix package set *and* emits its supervisor block, with no Dockerfile edits.
- Pluggable adapter pattern (agentbox ADR-005) for beads, pods, memory, events, and orchestrator — each with a local fallback and an external endpoint.
- 13 MB source-only (vs MAD's 89 GB); multi-arch (`x86_64-linux` + `aarch64-linux`); no docker-socket mount; `no-new-privileges:true` by default.

The deprecation is not free. MAD encodes months of hard-won workarounds (SSH setup, DinD survival, ragflow network joins, supervised `claude-zai`/`ctm`/code-server/VNC wiring) and runs services that VisionClaw currently depends on for daily work.

## Decision

VisionClaw deprecates `multi-agent-docker/` in favour of `agentbox/`. The migration is phased, gated on measurable parity, and reversible within a 30-day window post-cutover.

### Structural commitment

1. Agentbox becomes a **separate repository** at `github.com/DreamLab-AI/agentbox` when its standalone shape is stable. VisionClaw consumes it as an upstream dependency.
2. Until the split, `project/agentbox/` develops in situ, and its manifest + adapters remain generic (no VisionClaw-specific code in `agentbox/`).
3. VisionClaw-specific integration lives in `project/docs/` (this ADR, PRD-004, DDD) and in `project/docker-compose.agentbox.yml` (new, to be authored per PRD-004).

### Adapter mapping

VisionClaw wires agentbox adapters to sibling Rust actors:

| Agentbox slot | VisionClaw backend |
|---|---|
| `adapters.beads = "external"` | Rust actor implementing the bead-provenance bounded context (per `docs/ddd-bead-provenance-context.md`) — exposes the reference `bd` CLI contract over HTTP/MCP |
| `adapters.pods = "external"` | `JavaScriptSolidServer/` sibling container (or its Rust port per ADR-056 when it lands) |
| `adapters.memory = "external-pg"` | `ruvector-postgres` sidecar at `ruvector-postgres:5432` |
| `adapters.events = "external"` | Rust actor publishing to the contributor-stratum event bus (see ADR-057) + optional Nostr fan-out |
| `adapters.orchestrator = "stdio-bridge"` | Consumed by VisionClaw's `ContributorStudioSupervisor`, `AutomationOrchestratorActor`, and other actors that spawn agent work |

In VisionClaw, `agentbox.toml` defaults to `federation.mode = "client"` with all five adapters set to their external shape. Host-internal RuVector memory remains a local session cache per ADR-002.

### Deprecation gates — MAD stays live until every predicate passes

Prose gates are replaced by concrete, CI-runnable predicates. `project/scripts/deprecation-gate-check.sh` evaluates all four and emits pass/fail JSON consumed by the cutover workflow.

1. **Host Nix build, no DinD contamination** →
   - `ssh <host> 'cd /mnt/mldata/githubs/AR-AI-Knowledge-Graph/project/agentbox && nix build .#runtime'` exits 0, AND
   - The resulting image loads via `docker load < result` AND
   - `docker compose -f project/docker-compose.agentbox.yml up -d` succeeds with **no `/var/run/docker.sock` bind-mount** in the merged compose, AND
   - `docker inspect agentbox --format '{{range .HostConfig.Binds}}{{println .}}{{end}}' | grep -v docker.sock` (grep exits 1 — socket not present).

2. **Manifest-gated parity across both modes** →
   - `project/tests/parity/mad-capability-matrix.yaml` lists every MAD capability from PRD-004 §4; every row has an `agentbox_equivalent` field, AND
   - CI runs the matrix twice per row: once with `federation.mode = "client"` and once with `federation.mode = "standalone"`; both runs green, AND
   - No row is marked `explicit-ignore` without a signed justification committed.

3. **Migration guide exists and is structurally complete** →
   - `test -f project/docs/how-to/migrate-mad-to-agentbox.md` AND
   - The file contains (verified by structural lint in CI): an env-variable mapping table, a port-binding mapping table, a volume-mapping table, a Zellij-tab-replacing-tmux table, and a rollback section referencing `rollback-to-mad.sh`.

4. **User full-day sign-off** →
   - A file `project/docs/legacy/mad/rehearsal-log.md` exists, contains a day-granular activity log, AND ends with a `Signed-off-by:` trailer from the user's verified GPG key, AND
   - No unresolved `blocker:` or `regression:` entries in the log.

All four predicates are enforced by `project/scripts/deprecation-gate-check.sh`, which the cutover workflow refuses to proceed without green.

### Post-cutover

1. MAD's container is stopped but not deleted; volumes preserved for 30 days.
2. MAD's three durable documents (`SSH-SETUP.md`, `DOCKER-BUILD-NOTES.md`, `aisp.md`) move to `project/docs/legacy/mad/`.
3. MAD backups (`multi-agent-docker/backups/ruvector_backup_*.dump`) archive to cold storage + Solid pod.
4. At T+30 days, `multi-agent-docker/` is removed from VisionClaw in a single commit citing this ADR.
5. `supervisord.unified.conf`, `Dockerfile.unified`, `docker-compose.unified.yml`, and `docker-compose.unified-with-neo4j.yml` are audited: anything that references MAD is either removed or rewritten against agentbox.

### Mid-cutover rollback protocol

M5 is the only milestone where partial failure is possible — the migration rehearsal touches user workflow directly. Rollback must be deterministic, fast, and not require re-reading this ADR in a hurry.

**Pre-cutover snapshots** (executed automatically by `project/scripts/pre-cutover-snapshot.sh`):

- MAD volumes: `docker run --rm -v agentic-workstation-data:/data -v /backup:/backup alpine tar czf /backup/mad-volumes-<ts>.tgz /data`
- `ruvector-postgres` database: `pg_dump -Fc -f /backup/ruvector-<ts>.dump`
- MAD `beads` state at `/workspace/team/.beads/`: tar to backup
- Committed git SHA of VisionClaw at cutover start, recorded in `/backup/cutover-<ts>.meta`

**Rollback trigger conditions** (any one activates `project/scripts/rollback-to-mad.sh`):

- A blocker-severity entry lands in the rehearsal log.
- Four out of five `AdapterHealthMonitor` snapshots within a 10-minute window show `Failed`.
- User signs a `ROLLBACK:` trailer in the rehearsal log.

**Rollback sequence** (target: < 5 minutes, verified in CI):

1. `docker compose -f project/docker-compose.agentbox.yml down` — stop agentbox.
2. Restore MAD volumes from the `pre-cutover-snapshot` tarball.
3. `docker start agentic-workstation` — start the frozen MAD container.
4. Wait for MAD healthcheck green (30 s budget).
5. Verify agent-facing ports (9090, 5901, 8080) respond.
6. Log the rollback reason into `project/docs/legacy/mad/rehearsal-log.md` with full context.
7. Open a GitHub issue in `DreamLab-AI/VisionClaw` titled `M5 rollback 2026-XX-XX` with post-mortem owner assigned.

**Post-rollback**: the 30-day clock restarts only when M5 succeeds. During rollback window, MAD is the source of truth; any agentbox state generated during the failed cutover is preserved for forensic review but NOT replayed onto MAD (replay direction is MAD→agentbox only).

**CI verification**: `project/tests/chaos/rollback-simulation.sh` runs the full rollback sequence against test volumes on every merge to `main`. Failure blocks release tagging.

## Consequences

### Positive

- **Reproducibility.** `flake.lock` produces bit-identical images. Rollbacks become trivial.
- **Safer by default.** No `/var/run/docker.sock:rw`, no ambient `SYS_ADMIN`, gitleaks in CI.
- **Honest bounded contexts.** Beads, pods, and durable memory live in the Rust substrate they belong to; agentbox is disposable agent execution.
- **Smaller surface.** 13 MB of source vs 89 GB; 71-line entrypoint vs 2,379; one manifest file vs scattered env defaults across Dockerfile, compose, and `.env`.
- **Agentbox becomes reusable.** When split to its own repo, it's a clean product that other VisionClaw-shaped projects can consume.

### Negative

- **Migration effort.** ROM 3–5 engineering weeks + 1–2 weeks validation per PRD-004.
- **Adapter contract tests become load-bearing.** Any divergence between local and external adapters affects both VisionClaw's federated use and agentbox's standalone use.
- **Documentation split cost.** Integration story lives in `project/docs/`; standalone story in `agentbox/docs/`. Discipline required to keep them from bleeding into each other.
- **VisionClaw's CUDA workload (3DGS) becomes a build-time flag.** Teams running LichtFeld/COLMAP must opt-in via `[toolchains.cuda] = true` per manifest. Users who forget get a CUDA-less image.
- **Some MAD-era workflows (multi-user pseudo-isolation) do not map.** Agentbox uses profile isolation, not Linux-user isolation (enforced by agentbox CLAUDE.md). If multi-tenant API-key separation is ever needed, it's a separate ADR.

## Alternatives considered

**Keep MAD, layer Nix on top** (rejected): leaves the 2,379-line entrypoint, the DinD minefield, and the docker-socket mount intact. No durability win.

**Rewrite MAD in place** (rejected): the directory's name and content semantics are tied to the original monolith. A sibling directory with a clean break is cleaner to reason about, easier to roll back, and — critically — extracts cleanly into its own repo when ready.

**Adopt an off-the-shelf agent container (devcontainers, gitpod, coder)** (rejected): none of the surveyed options offered manifest-gated adapter composition with reproducible builds. Agentbox is narrower in scope than all three and purpose-built for VisionClaw's federation needs.

## Follow-ups

- Author PRD-004 (done).
- Author `docs/ddd-agentbox-integration-context.md` (pending — see task #15).
- Author `docs/how-to/migrate-mad-to-agentbox.md` (Milestone 5 of PRD-004).
- Run full QE review via the agentic-quality-engineering fleet (pending — see task #16).
- Open tracking issue "MAD deprecation" in DreamLab-AI/VisionClaw once agentbox is split to its own repo.
