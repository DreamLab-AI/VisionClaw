---
title: VisionClaw Documentation
description: Documentation hub for VisionClaw — a governed agentic mesh for real-time 3D knowledge-graph exploration with GPU-accelerated physics, OWL 2 EL ontology reasoning, and a multi-agent control surface.
---

# VisionClaw Documentation

> [VisionClaw](../README.md) · Documentation Hub

VisionClaw is a governed agentic mesh for real-time 3D knowledge-graph exploration: a Rust backend, CUDA GPU physics, OWL 2 EL ontology reasoning over an embedded Oxigraph triple store, and a multi-agent AI control surface.

## Quick start

```bash
git clone https://github.com/DreamLab-AI/VisionClaw.git
cd VisionClaw && cp env.example .env
./scripts/launch.sh up dev
```

`./scripts/launch.sh up dev` is the canonical launcher. The explicit fallback is `docker compose -f docker-compose.unified.yml --profile dev up -d` — `docker-compose.unified.yml` is the application compose file; `docker-compose.cloudflared.yml` is an optional standalone tunnel overlay that `launch.sh` does not use.

| Service | URL | Notes |
|---------|-----|-------|
| 3D graph frontend | <http://localhost:3001> | nginx |
| REST API | <http://localhost:4000/api> | HTTP + WebSocket |
| Solid pod | <http://localhost:8484> | per-user pod storage |
| MCP TCP (agentbox) | `multi-agent-container:9500` (`MCP_TCP_PORT`) | outbound poll by `bots_client.rs` for agent-state snapshots; WebSocket cutover planned — see [ADR-2084](adr/ADR-2084-9500-load-bearing-doc-correct-plan-ws-cutover.md) |

The graph store is the embedded Oxigraph triple store backed by SQLite ([ADR-2004](adr/ADR-2004-oxigraph-sqlite-persistence.md)). Neo4j is fully removed ([ADR-132](archive/adr/ADR-132-neo4j-removal-oxigraph-adoption.md)) and there is no separate database browser UI.

## System at a glance

| Layer | Facts |
|-------|-------|
| Backend | 428 Rust files (~178K LOC); 35 Actix actors (19 service + 16 GPU; +10 WebSocket session); 44 hexser handlers (19 directive + 25 query), no CQRS bus (ADR-089); 9 ports, 12 adapters; 8 workspace crates |
| GPU physics | 82 CUDA `__global__` kernels across 9 `.cu` files (5,854 LOC); 17,147 nodes laid out live |
| Client | 465 TypeScript/TSX files (422 non-test, ~103K LOC); 16 feature modules |
| Ontology | Whelk-rs OWL 2 EL + SHACL-lite + JSON-LD validation + PROV-O provenance (PRD-022); 7 MCP ontology tools |
| Wire protocol | Full-snapshot V3 (52 B/node) / V5 (V3 body + 8-byte broadcast seq) — **delta encoding prohibited by design** (BROADCAST-001); clients tween to server targets |
| Decision record | 99 living ADR-2xxx records in [adr/](adr/README.md); the frozen legacy corpus under [archive/](archive/) holds 120 ADRs, 26 PRDs and 15 DDD context maps |

## Documentation map

Documentation follows the [Diátaxis](https://diataxis.fr/) framework — each quadrant serves a distinct need. Outside the four quadrants, `docs/` also carries the security audit, screenshots and media assets, diagram sources, gap-close evidence, estate-closeout receipts and the dream-cycle ledger.

| Category | Purpose | Start here |
|----------|---------|-----------|
| Tutorials | Learning-oriented lessons that teach VisionClaw by doing | [tutorials/](tutorials/README.md) |
| How-to | Task recipes for deployment, development, operations, and features | [how-to/](how-to/README.md) |
| Explanation | Concepts and rationale — architecture, physics, ontology, security | [explanation/](explanation/system-overview.md) |
| Reference | Exhaustive specifications — REST, WebSocket, binary protocol, schema, config | [reference/](reference/README.md) |
| Decisions | Architecture Decision Records governing every major design choice | [adr/](adr/) |
| Formal record (frozen) | Legacy Product Requirements and Domain-Driven Design context maps — rationale only, never authority | [archive/prd/](archive/prd/) · [archive/ddd/](archive/ddd/) |

### Entry points by audience

- **New user** — Start with [What is VisionClaw?](tutorials/what-is-visionclaw.md), then [Installation](tutorials/installation.md), [Your first graph](tutorials/first-graph.md), and the [navigation guide](how-to/navigation-guide.md).
- **Developer** — Building against the API: [REST API](reference/rest-api.md), [WebSocket protocol](reference/websocket-protocol.md), [Binary protocol](reference/binary-protocol.md). Understanding the internals: [System overview](explanation/system-overview.md), [Backend architecture](explanation/backend-architecture.md), [Client architecture](explanation/client-architecture.md), and [Physics & GPU engine](explanation/physics-gpu-engine.md). Contributing: [CONTRIBUTING](CONTRIBUTING.md) and the [development guide](how-to/development.md).
- **Operator** — Running it in production: [Deployment](how-to/deployment.md), the [operations runbooks](how-to/operations/README.md), [Configuration reference](reference/configuration.md), [Known issues](KNOWN_ISSUES.md), and the [pre-demo security audit (2026-08-21)](security/PRE-DEMO-SECURITY-AUDIT-2026-08-21.md).

## Known issues

Before debugging unexpected behaviour, check [KNOWN_ISSUES.md](KNOWN_ISSUES.md) — it tracks active P1/P2 bugs and their workarounds.

## agentbox subsystem

VisionClaw runs on top of **agentbox**, the sovereign agent-runtime subsystem (skills, identity mesh, pod adapters, ACSP control surfaces). agentbox is a subsystem with its own documentation set — VisionClaw links into it rather than duplicating it.

→ [agentbox documentation](../agentbox/docs/README.md)

## Contributing and history

- [Contributing](CONTRIBUTING.md) — workflow, branching conventions, code standards
- [Changelog](CHANGELOG.md) — version history and release notes

---

*Maintained by DreamLab AI — [Issues](https://github.com/DreamLab-AI/VisionClaw/issues) · [Discussions](https://github.com/DreamLab-AI/VisionClaw/discussions)*
