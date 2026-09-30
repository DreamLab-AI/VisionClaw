---
title: Explanation
description: Concept-oriented documentation explaining how VisionClaw is built and why — architecture, domain model, the knowledge and ontology pipeline, platform surfaces, and the security model.
---

# Explanation

> [VisionClaw Docs](../README.md) · Explanation

Understanding-oriented documentation. These pages explain *how the system is shaped and why* — the architecture, the domain boundaries, the knowledge pipeline, and the trade-offs behind each decision. They are the Diátaxis explanation tier: read them to build a mental model, not to follow steps. For step-by-step learning see the [tutorials](../tutorials/README.md); for task recipes see the [how-to guides](../how-to/README.md); for exhaustive detail see the [reference](../reference/README.md).

Every page back-links to the [Architecture Decision Records](../adr/) that govern it.

---

## Architecture

How the running system is layered, from the CUDA force engine up to the browser and XR clients.

| Page | What it explains |
|------|------------------|
| [System Overview](system-overview.md) | End-to-end tour — how a vault page becomes a GPU-laid-out node in the browser and on Quest 3. The 10,000-foot map. |
| [Backend Architecture](backend-architecture.md) | The hexagonal Rust core — 8 workspace crates, 9 ports / 12 adapters, 44 hexser handlers (19 directive + 25 query), no CQRS bus. |
| [Backend CQRS Pattern](backend-cqrs-pattern.md) | The directive/query handler split and why the CQRS message bus was removed (ADR-089) in favour of direct dispatch. |
| [Actor Hierarchy](actor-hierarchy.md) | The 45 Actix actors (19 service + 16 GPU + 10 WebSocket session) and how messages flow between supervisors. |
| [Client Architecture](client-architecture.md) | The TypeScript/React client (465 files, 16 feature modules), WebGL/WebGPU rendering, and the binary position pipeline. |
| [Control Center](control-center.md) | The glass-overlay settings UI — macro dials, semantic groups, the frozen dot-path registry, and the command-palette reveal flow — that replaced the docked `IntegratedControlPanel`. |
| [Physics GPU Engine](physics-gpu-engine.md) | The 82 CUDA kernels across 9 source files; force-directed layout on the GPU. |
| [Agent–Physics Bridge](agent-physics-bridge.md) | Deep-dive — how agent nodes enter the same force graph as knowledge and ontology nodes. |
| [XR Architecture](xr-architecture.md) | Quest 3 / WebXR plus the Godot presence layer and the `visionclaw-xr-presence` crate. |
| [Deployment Topology](deployment-topology.md) | Container and network layout — API :4000, nginx :3001, Solid pod :8484, legacy MCP TCP :9500. |
| [Technology Choices](technology-choices.md) | Why Rust + Actix + CUDA + Oxigraph + React, and the trade-offs behind each pick. |

---

## Domain

The strategic domain-driven design model — where the boundaries are drawn and why.

| Page | What it explains |
|------|------------------|
| [Bounded Contexts](bounded-contexts.md) | The bounded-context map and the reasoning behind each split. |
| [DDD Bounded Contexts](ddd-bounded-contexts.md) | Strategic DDD deep-dive — context boundaries, relationships, and ubiquitous language. |
| [DDD Semantic Pipeline](ddd-semantic-pipeline.md) | The semantic ingestion and parsing context. |
| [DDD Insight Migration Context](ddd-insight-migration-context.md) | How insights are promoted from transient analysis into the persistent graph. |
| [DDD Identity Contexts](ddd-identity-contexts.md) | Identity, pod, and Nostr identity boundaries. |
| [DDD Enterprise Contexts](ddd-enterprise-contexts.md) | Enterprise-facing contexts and their integration seams. |
| [DDD Contributor Enablement Context](ddd-contributor-enablement-context.md) | The contributor-support and enablement context. |

The full per-context catalogue lives in the [DDD documents](../archive/ddd/README.md).

---

## Knowledge & Ontology

How notes become a reasoned, governed knowledge graph and how learnt insight flows back in.

| Page | What it explains |
|------|------------------|
| [Ontology Pipeline](ontology-pipeline.md) | Note → OWL — Whelk-rs OWL 2 EL reasoning, SHACL-lite and JSON-LD validation, PROV-O provenance (PRD-022), and the 7 MCP ontology tools. |
| [Feature Engineering Pipeline](feature-engineering-pipeline.md) | How graph features are derived to drive layout and clustering. |
| [Insight Migration Loop](insight-migration-loop.md) | How agent-generated insight is migrated back into the persistent graph. |
| [RuVector Integration](ruvector-integration.md) | RuVector / AgentDB memory — MiniLM-L6-v2 384-dim embeddings and HNSW semantic search. |

---

## Platform & Subsystems

VisionClaw as a coordination platform, and how it composes with the agentbox subsystem.

| Page | What it explains |
|------|------------------|
| [VisionClaw Coordination Platform](visionflow-coordination-platform.md) | VisionClaw as a multi-agent coordination surface over the graph. |
| [VisionClaw Wardley Map](visionflow-wardley-map.md) | A strategic Wardley map of the platform's value chain and evolution. |
| [Subsystems](subsystems.md) | How VisionClaw composes with the [agentbox subsystem](../../agentbox/docs/README.md) — what each owns and where the seam sits. |
| [Agent Control Surface](agent-control-surface.md) | How agents are observed and steered through the live graph. |
| [Ecosystem Convergence](ecosystem-convergence.md) | The convergence of the knowledge-graph, agent, and pod ecosystems. |
| [Solid Sidecar Architecture](solid-sidecar-architecture.md) | The Solid pod sidecar (:8484) and the URN → Solid resource mapping. |
| [User & Agent Pod Design](user-agent-pod-design.md) | Per-user and per-agent pod design and data ownership. |
| [Contributor Support Stratum](contributor-support-stratum.md) | The contributor AI-support layer — how the enablement stratum sits over the mesh (PRD-003 / ADR-057). |
| [Blender MCP Unified Architecture](blender-mcp-unified-architecture.md) | The unified Blender-MCP integration surface for 3D asset generation and scene assembly. |

---

## Security

| Page | What it explains |
|------|------------------|
| [Security Model](security-model.md) | The authentication and authorisation model, the `SETTINGS_AUTH_BYPASS` caveat, and the threat surfaces. |

---

## See also

- [Tutorials](../tutorials/README.md) · [How-To Guides](../how-to/README.md) · [Reference](../reference/README.md)
- [DDD Documents](../archive/ddd/README.md) · [Product Requirement Documents](../archive/prd/README.md)
- [agentbox subsystem documentation](../../agentbox/docs/README.md)
- Governing ADRs: [ADR-090 — Hexagonal Crate Modularisation](../archive/adr/ADR-090-hexagonal-crate-modularisation.md) · [ADR-089 — CQRS Bus Removal](../archive/adr/ADR-089-cqrs-bus-removal.md) · [ADR-112 — Ontology Augmentation Retrieval Spine](../archive/adr/ADR-112-ontology-augmentation-retrieval-spine.md) · [ADR-011 — Auth Enforcement](../archive/adr/ADR-011-auth-enforcement.md)
