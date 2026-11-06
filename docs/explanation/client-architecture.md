---
title: Client Architecture
description: Why the VisionClaw browser client is built the way it is — React 19 + Three.js (R3F), a dual WebGPU/WebGL renderer, InstancedMesh rendering, and a zero-copy SharedArrayBuffer position pipeline fed by the binary WebSocket protocol.
---

# Client Architecture

> [VisionClaw Docs](../README.md) · [Explanation](README.md)

The VisionClaw client is a React 19 single-page application that renders an interactive 3D knowledge graph in the browser. It is the only first-class viewport onto the graph: the backend computes layout on the GPU and streams positions, while the client owns presentation, interaction, and local state. This page explains the shape of that client and the reasoning behind its three load-bearing decisions — **one renderer** (three's WebGPURenderer, which falls back to its own WebGL2 backend), **InstancedMesh** geometry, and a **zero-copy SharedArrayBuffer** position pipeline driven by the binary WebSocket protocol.

The codebase is **465 TypeScript/TSX files** (**422 non-test**) totalling roughly **103K lines**, organised into **16 feature modules** under `client/src/features/`. Three.js is consumed through React Three Fiber (R3F), so the scene graph is declared as React components and reconciled like any other React tree.

## Why the client is shaped this way

A knowledge graph at scale is a hard rendering problem: tens of thousands of nodes and edges, each moving every frame while a force-directed layout converges. Three constraints follow from that, and they drive the whole design.

- **Per-object draw calls do not scale.** Issuing one draw call per node collapses the frame budget well before the graph is interesting. The client uses `InstancedMesh` so an entire node type is a single draw call regardless of count.
- **Per-frame serialisation does not scale.** Positions arrive ~60 times per second. Copying or JSON-decoding them across the worker boundary each frame would dominate the budget. The client uses a `SharedArrayBuffer` (SAB) so positions are written once and read zero-copy by both the layout worker and the render loop.
- **Browser GPU backends are not uniform.** WebGPU is faster and the strategic target, but Firefox, older Safari, and the Quest 3 Oculus Browser either lack `navigator.gpu` or fall back unpredictably. The client probes at runtime and selects a clean backend rather than trusting a hybrid path.

Everything below is a consequence of those three constraints.

## Layered structure

The client separates a thin **transport** tier (WebSocket + REST), a **state** tier (Zustand stores and the layout worker), and a **render** tier (R3F scene under the active renderer). Data flows transport → state → render; user intent flows back render → state → transport.

```mermaid
flowchart TD
    subgraph T["Transport"]
        WS["WebSocket client (/wss)"]
        REST["REST client (UnifiedApiClient)"]
    end
    subgraph S["State and compute"]
        DISP["Binary frame dispatcher (single-flight)"]
        WORKER["graph.worker.ts (layout)"]
        SAB[("SharedArrayBuffer (positions)")]
        STORES["Zustand stores (settings, graph, analytics)"]
    end
    subgraph R["Render (React Three Fiber)"]
        RF["rendererFactory (WebGPU or WebGL)"]
        GM["GraphManager (useFrame, 60Hz)"]
        INST["InstancedMesh nodes, edges, labels"]
        WASMFX["WASM scene effects"]
    end

    WS -->|"binary frames"| DISP
    WS -->|"JSON text messages"| STORES
    REST -->|"settings, metadata, analytics"| STORES
    DISP --> WORKER
    WORKER --> SAB
    SAB --> GM
    STORES --> GM
    RF --> GM
    GM --> INST
    GM --> WASMFX
    GM -->|"user interaction"| STORES
    STORES -->|"PUT settings"| REST
```

The split keeps the hot path (binary frames → SAB → render) free of React re-renders: position updates never flow through the Zustand store, so the React tree does not reconcile 60 times a second. Only structural changes (a new node, a settings edit, an analytics result) touch the stores.

## One renderer, two backends

R3F is initialised with three's `WebGPURenderer` from `rendererFactory.ts`, and only that renderer (operator decision, 2026-11-06). Where WebGPU is missing, the renderer runs on its own WebGL2 backend (`getFallback` → `WebGLBackend`). Every material is TSL, which the backend compiles to WGSL or GLSL, so the scene, the materials and the bloom pipeline (`RenderPipeline` + `BloomNode`) are the same code on both backends.

```mermaid
flowchart TD
    START["rendererFactory.createGemRenderer()"] --> FORCE{"WebGL2 forced<br/>by user toggle?"}
    FORCE -->|"yes"| GLB["WebGPURenderer<br/>(forceWebGL: true)"]
    FORCE -->|"no"| INIT["WebGPURenderer"]
    INIT --> ADAPTER{"WebGPU adapter?"}
    ADAPTER -->|"no"| FALLBACK["three's getFallback:<br/>WebGL2 backend"]
    ADAPTER -->|"yes"| TIMEOUT{"init within 5 s?"}
    TIMEOUT -->|"no"| GLB
    TIMEOUT -->|"yes"| WGPU["WebGPU backend"]
```

`rendererCapabilities.backend` (`activeBackendIsWebGPU()`) reports which backend is live. The user-facing override is the [Control Center](control-center.md)'s System & Developer "WebGPU Renderer" action, persisted in `localStorage` as `visionclaw-force-webgl`, and it maps to `forceWebGL: true`. The 5-second timeout exists because the Quest 3 Oculus Browser can hang during WebGPU adapter negotiation; on timeout the client retries on the WebGL2 backend. A layer whose material fails to build, or whose shader fails on either backend, is reported on the canvas notice (`rendering/renderFaults.ts`), never skipped silently. Immersive XR is a **separate** native binary, not this renderer — see [XR Architecture](xr-architecture.md).

## Component composition

`GraphManager` is a composition shell, not a monolith: its data-subscription, edge-buffer, event, filtering, and colour concerns are delegated to hooks under `client/src/features/graph/hooks/`. The component wires the hooks together and owns the render tree, which keeps the file within the project's 500-line limit while the per-frame work lives in focused, testable units.

```mermaid
flowchart TD
    APP["App.tsx"] --> INIT["AppInitializer (WS, settings, auth)"]
    APP --> LAYOUT["MainLayout"]
    LAYOUT --> PANEL["ControlCenter (glass overlay, settings)"]
    LAYOUT --> CANVAS["R3F Canvas"]
    CANVAS --> GM["GraphManager"]
    CANVAS --> WASMFX["WasmSceneEffects"]
    CANVAS --> POST["Post-processing (bloom)"]
    GM --> NODES["InstancedMesh nodes (per type)"]
    GM --> EDGES["GlassEdges (instanced cylinders)"]
    GM --> LABELS["InstancedLabels"]
```

Each node type renders as its own `InstancedMesh` — one draw call for all knowledge nodes, one for ontology, one for agents — so GPU cost is constant in node count. Per-instance transform and colour are written each frame via `setMatrixAt` / `setColorAt`. Edges are instanced unit-height cylinders scaled and rotated per edge; labels use a two-phase `useFrame` that patches glyph positions every frame and rebuilds layout less often.

## The data-flow pipeline: binary frame to pixels

This is the hot path and the reason the client performs. A binary frame arrives, is dispatched under a single-flight discipline, crosses into the layout worker, lands in the SharedArrayBuffer, and is read zero-copy by the render loop. No copy, no JSON, no React reconciliation on the per-frame path.

```mermaid
sequenceDiagram
    participant Server as "Backend (/wss)"
    participant WS as "WebSocket client"
    participant Disp as "binaryFrameDispatcher"
    participant Worker as "graph.worker.ts"
    participant SAB as "SharedArrayBuffer"
    participant GM as "GraphManager (useFrame)"
    participant GPU as "Active renderer"

    Server-->>WS: binary frame (V4 delta, default)
    WS->>Disp: handle(buffer)
    Note over Disp: single-flight — newest-wins,<br/>max one frame pending (ADR-03 D2)
    Disp->>Worker: processBinaryData (Comlink)
    Worker->>Worker: decode node structs, apply layout step
    Worker->>SAB: write Float32 positions (index x 3)
    loop "every frame at 60Hz"
        GM->>SAB: read positions (zero-copy Float32Array view)
        GM->>GPU: setMatrixAt() per instanced node
        GM->>GPU: recompute edge + label buffers
        GPU-->>GM: frame presented
    end
```

Two design choices make this safe under load:

- **Single-flight dispatch.** `createBinaryFrameDispatcher` processes at most one frame across the `await` that crosses the worker (Comlink) boundary. A frame arriving while one is in flight replaces the single pending slot — newest wins, stale frames are dropped and counted. This bounds memory and latency: the client always renders the freshest layout rather than draining a backlog. Each WebSocket connection gets its own dispatcher instance, so in-flight and pending state never leak across reconnects.
- **Zero-copy positions.** The worker writes positions once into the SAB; `GraphManager` reads them through a `Float32Array` view over the same memory. Nothing is serialised or copied per frame. The same view is captured once at the top of each `useFrame` tick and reused for every consumer (nodes, edges, labels) so all reads see a consistent snapshot.

## Binary wire format

Positions ride a compact binary protocol rather than JSON, for bandwidth and decode cost. Three node-payload generations exist; the client decodes all of them by stride detection:

| Version | Node payload | Role |
|---------|--------------|------|
| V2 | 36 bytes/node | Legacy — position + velocity, no analytics tail |
| V3 | 52 bytes/node (`BINARY_NODE_SIZE_V3`) | Adds the analytics tail (SSSP, cluster, anomaly, community, centrality) per ADR-031 |
| V4 | delta-encoded, **current default** | 6-byte message header, `uint32` SSSP identifiers; transmits only changed nodes |

The message envelope is a 6-byte header (`MESSAGE_HEADER_SIZE`) — `[type:u8][version:u8][payloadLength:u32]` — with a 7th byte for graph-update frames carrying a `GraphTypeFlag`. Node identifiers are sequential `u32` values whose high bits encode type (agent, knowledge, ontology); the client must coerce identifiers to `String()` before using them as map keys or in `===` comparisons, because the backend emits them numerically. The exhaustive field layout, message-type enum, and bandwidth maths live in the reference, not here:

- [Binary protocol reference](../reference/binary-protocol.md) — byte-level field layout per version.
- [WebSocket protocol reference](../reference/websocket-protocol.md) — message types, subscription handshake, control bits, heartbeats.

## State management

Application state is Zustand. The hot position path deliberately bypasses it; everything else flows through stores, the largest being `settingsStore`.

- **Settings are lazy-loaded.** Only a small set of essential paths is fetched at startup; non-essential sections are loaded on demand via `ensureLoaded(['section.*'])` inside a `useEffect`. This cuts initial settings load dramatically versus fetching the full tree up front.
- **Writes are debounced and batched.** `autoSaveManager` accumulates changes in a map and flushes the queue as a single batched API call after ~500ms of inactivity, with a `beforeunload` force-flush so edits made just before tab close are not lost.
- **Subscriptions are path-based.** Components subscribe to a specific dot-notation path rather than the whole store, so a bloom-intensity change re-renders only the bloom consumer, not the tree.

Physics settings are server-authoritative: the backend's GPU layout is the single source of truth, so on hydration server physics values are re-overlaid on top of any locally persisted overlay. The full settings read/write sequence is documented in [Physics & GPU Engine](physics-gpu-engine.md) and governed by ADR-039.

## WASM scene effects

Particle and environment effects are implemented in a Rust crate compiled to WASM and bridged into the R3F scene through a thin TypeScript layer (`scene-effects-bridge.ts` → `useWasmSceneEffects` → `WasmSceneEffects`). The bridge follows the same zero-copy discipline as the position pipeline: the Rust module exposes raw pointers via `get_*_ptr()` / `get_*_len()`, and the bridge constructs `Float32Array` views directly over `WebAssembly.Memory.buffer`. The views stay valid only while the WASM heap is not reallocated, so effect code avoids growing memory while views are held.

## Graph navigation and growth

The pipeline above draws the graph; a second, canvas-level control surface lets the
user *navigate and grow* it by direct manipulation, adopted from the Graph2VR-class
interaction programme ([ADR-139](../archive/adr/ADR-139-immersive-interaction-adoption-programme.md))
and shared, contract-for-contract, with the immersive client. All of it rides the
node/expand/relations/fold/pattern REST routes — the wire shapes live in
[reference/rest-api.md](../reference/rest-api.md), not here.

- **Click-to-focus / fit.** Selecting a node, a search hit, or a transcript line
  dispatches the `visionclaw:search` CustomEvent; `useGraphSelection` sets the
  pulsing highlight and eases the camera along `flyToTargetRef`. The coordinate-free
  bridge `cameraFocus.ts` lets HTML panels outside the R3F canvas fly the camera
  without importing Three.js internals (`focusNodeById`, and the shared
  `resolveNodeWorldPosition` SAB lookup GraphManager reuses for beam targets).
- **Page-card panel + additive expansion.** `NodeDetailPanel.tsx` shows a node's
  definition, `subClassOf` parents, typed relationships, lazily-fetched source
  page-card, and connected nodes. `NodeContextMenu.tsx` implements the
  predicate-count-first model: `GET /api/graph/node/{id}/relations` first, then a
  chosen `POST /api/graph/node/{id}/expand` **additively merges** the returned
  nodes/edges into the live graph with no reload (typed wrapper
  `client/src/api/graphExpandApi.ts`).
- **Visual query builder.** Node marks assemble triple patterns that `POST` to
  `/api/graph/query/pattern` (max 16 triples / 8 variables); matches highlight in the
  scene.
- **Fold ladder.** `GET /api/graph/fold` returns level 0–3 fold plans (groups of
  `{representativeId, memberIds, badge, kind}`) that collapse and re-expand density.
- **Agent-action beams.** `TransientBeamsLayer` renders the ephemeral `0x23
  AGENT_ACTION` beams (agent → target node) streamed over `/wss`, styled by the
  Agents settings group described in [Control Center](control-center.md).

The immersive client is a **separate native binary** (Godot + godot-rust), not this
renderer. It reaches the same visual result through a different substrate: its
per-frame position-hunt and MultiMesh packing are offloaded to a Rust **`RenderStore`**
([ADR-137](../archive/adr/ADR-137-xr-render-offload-and-runtime-quality-dials.md),
`xr-client/rust/src/render_store.rs`) — the XR analogue of this client's
SharedArrayBuffer hot path — and it decodes the same binary stream including the V5
sequenced wrapper. ADR-137's runtime quality dials (the `initialNodeLimit` settings
dial and runtime-derived instance budgets that replace hardcoded caps, plus full-3D
layout as the default) are shared settings that both clients honour. Full detail is
in [XR Architecture](xr-architecture.md).

## See also

- [System Overview](system-overview.md) — how the client sits within the whole VisionClaw system.
- [Backend Architecture](backend-architecture.md) — the Actix backend and hexagonal ports that feed the client.
- [Actor Hierarchy](actor-hierarchy.md) — the actors that compute layout and broadcast positions.
- [Physics & GPU Engine](physics-gpu-engine.md) — CUDA force computation and the settings pipeline behind the positions.
- [Control Center](control-center.md) — the settings UI that writes into that pipeline.
- [XR Architecture](xr-architecture.md) — the separate native immersive client.
- [Binary protocol reference](../reference/binary-protocol.md) and [WebSocket protocol reference](../reference/websocket-protocol.md) — wire-level detail.
- [Control Center](control-center.md) — the settings overlay and the desktop graph-interaction trio in full.
- [REST API reference](../reference/rest-api.md) — the node/expand/relations/fold/pattern and layout routes the navigation surfaces call.
- Governing ADRs: [ADR-012 WebSocket store decomposition](../archive/adr/ADR-012-websocket-store-decomposition.md), [ADR-013 render performance](../archive/adr/ADR-013-render-performance.md), [ADR-031 GPU analytics correctness and wiring](../archive/adr/ADR-031-gpu-analytics-correctness-and-wiring.md), [ADR-039 settings consolidation](../archive/adr/ADR-039-settings-consolidation.md), [ADR-061 binary protocol unification](../archive/adr/ADR-061-binary-protocol-unification.md).
- Interaction and layout programme ADRs: [ADR-137 XR render offload + runtime quality dials](../archive/adr/ADR-137-xr-render-offload-and-runtime-quality-dials.md), [ADR-138 GPU force-channel registry](../archive/adr/ADR-138-gpu-force-channel-registry.md), [ADR-139 immersive interaction adoption](../archive/adr/ADR-139-immersive-interaction-adoption-programme.md), [ADR-140 XR agent-swarm visualisation](../archive/adr/ADR-140-xr-agent-swarm-visualisation.md), [ADR-141 constrained-layout engine programme](../archive/adr/ADR-141-constrained-layout-engine-programme.md).
