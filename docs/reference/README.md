---
title: VisionClaw Reference
description: Master index for VisionClaw's technical reference — REST and WebSocket APIs, the binary wire protocol, MCP ontology tools, graph schema, configuration, physics parameters, the agent catalogue, CLI, error codes, glossary, and benchmarks.
---

# VisionClaw Reference

> [VisionClaw Docs](../README.md) · Reference

Exhaustive, dry lookup material for the VisionClaw mesh: every endpoint, wire format, environment variable, parameter, and error code. For learning paths see [tutorials](../tutorials/README.md); for task recipes see [how-to guides](../how-to/README.md); for the concepts and rationale behind these contracts see [explanation](../explanation/system-overview.md).

Each page is self-contained and back-links to its governing ADR(s) in [../adr/](../adr/).

---

## API and protocols

| Reference | Covers |
|-----------|--------|
| [REST API](rest-api.md) | Every HTTP endpoint on the API server (`:4000`) — graph data, settings, Nostr NIP-98 auth, ontology, pathfinding, and Solid pod operations, with request/response schemas. |
| [WebSocket Protocol](websocket-protocol.md) | WebSocket endpoints, handshake, authentication, heartbeat, subprotocols, and reconnection lifecycle. |
| [Binary Protocol](binary-protocol.md) | The canonical wire format — `MessageType` frame table, V2 (36-byte) and V3 (52-byte, `BINARY_NODE_SIZE_V3`) node records, the V4 delta default, and the `AGENT_ACTION` header. |
| [MCP Tools](mcp-tools.md) | The 7 native ontology MCP tools (`discover`, `read`, `query`, `traverse`, `propose`, `validate`, `status`), their input/output contracts, and the cross-reference to the agentbox ontology-bridge tools. |

## Schema and data

| Reference | Covers |
|-----------|--------|
| [Graph Schema](graph-schema.md) | RDF/SPARQL schema for the embedded Oxigraph triple store — node types, the `u32` node-id flag-bit encoding, edge and namespace relationships, ontology axioms, provenance, and SPARQL query patterns. |
| [URN–Solid Mapping](urn-solid-mapping.md) | Binds VisionClaw's per-domain vocabulary IRIs (`bc:`, `mv:`, `rb:`, `dt:`, `ai:`) to canonical `urn:solid:` terms for ecosystem alignment (gated by `URN_SOLID_ALIGNMENT`). |

## Physics and performance

| Reference | Covers |
|-----------|--------|
| [Physics Parameters](physics-parameters.md) | Every force-directed layout parameter — type, range, default, and effect — plus the authoritative tuning table for large knowledge graphs. |
| [Performance Benchmarks](performance-benchmarks.md) | Measured figures with receipts (live graph scale, XR presence wire baseline), the harnesses that produce new ones, and operating targets. |

## Operations

| Reference | Covers |
|-----------|--------|
| [Configuration](configuration.md) | Exhaustive reference for environment variables, ports (API `:4000`, frontend `:3001`, Solid pod `:8484`, legacy MCP TCP `:9500`), and Docker Compose options, organised by domain. |
| [CLI](cli.md) | Cargo, launcher, and Docker Compose commands for building, testing, and running VisionClaw. |

## Agents and diagnostics

| Reference | Covers |
|-----------|--------|
| [Agents Catalogue](agents-catalog.md) | The 54 specialist skills across 12 domains (83+ with the full environment) plus the 7 built-in MCP ontology tools, with invocation patterns and capability descriptions. |
| [Error Codes](error-codes.md) | The full `[SYSTEM]-[SEVERITY]-[NUMBER]` hierarchy — API, database, graph/ontology, GPU/physics, and WebSocket codes — each with a solution. |
| [Glossary](glossary.md) | Definitions for the domain-specific terms used across VisionClaw documentation. |

---

## How reference fits the documentation set

Reference is one of four Diátaxis categories. Use the others when you need something other than a lookup:

| You want to… | Go to |
|--------------|-------|
| Learn by doing, from zero | [Tutorials](../tutorials/README.md) |
| Accomplish a specific task | [How-to guides](../how-to/README.md) |
| Understand a concept or decision | [Explanation](../explanation/system-overview.md) |
| Look up an exact contract or value | This section |

---

## See also

- [Documentation Hub](../README.md) — top-level entry point for all VisionClaw docs
- [System Overview](../explanation/system-overview.md) — architecture context for the contracts indexed here
- Governing ADRs: [ADR-064 Typed Graph Schema](../archive/adr/ADR-064-typed-graph-schema.md), [ADR-061 Binary Protocol Unification](../archive/adr/ADR-061-binary-protocol-unification.md), [ADR-090 Hexagonal Crate Modularisation](../archive/adr/ADR-090-hexagonal-crate-modularisation.md), [ADR-089 CQRS Bus Removal](../archive/adr/ADR-089-cqrs-bus-removal.md)
