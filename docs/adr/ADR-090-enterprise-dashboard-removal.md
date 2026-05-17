# ADR-090: Enterprise Dashboard Removal — Migration to Nostr Forum

**Status:** Accepted  
**Date:** 2026-05-15  
**Deciders:** Dr John O'Hare  

## Context

VisionClaw shipped an enterprise dashboard (drawer UI, 5 panels, WASM particle effects) as a client-side overlay. The equivalent functionality has been migrated to the Nostr-native forum system (`nostr-rust-forum` kit) which provides these features via Nostr event kinds (31400-31405) and the governance/admin pages.

## Decision

Remove the enterprise dashboard from VisionClaw. The ~7,600 lines of client + server code are dead weight now that the forum handles governance, connectors, workflows, and KPIs natively.

## Feature Migration Status

| VisionClaw Feature | Lines | Forum Equivalent | Status |
|---|---|---|---|
| Broker / Judgment Cases | 488 (Rust) + BrokerWorkbench (TSX) | `governance.rs` — PanelRegistry + DecisionCanvas (kind 31400-31405) | Migrated |
| Workflow Studio | 259 (Rust) + WorkflowStudio (TSX) | `admin.rs` — deployment + config management | Migrated |
| Mesh KPI Dashboard | 50 (Rust) + MeshKpiDashboard (TSX) | `admin.rs` — stats panel + health monitoring | Migrated |
| Connector Panel | 130 (Rust) + ConnectorPanel (TSX) | `marketplace.rs` — service discovery + external integrations | Migrated |
| Policy Console | 78 (Rust) + PolicyConsole (TSX) | `admin.rs` — moderation rules + rate limits | Migrated |
| WASM Drawer FX | drawer-fx crate + bridge | N/A — visual effect only, not a feature | Removed (no equivalent needed) |

## Gaps Requiring Future Work

The following capabilities existed in the enterprise dashboard but need enhancement in the forum:

1. **KPI Sparkline Trends**: The mesh KPI dashboard had per-metric sparkline charts with time-window selectors (24h/7d/30d/90d). The forum admin stats page shows current values but lacks historical trend visualization. **Action**: Add time-series charting to `admin.rs` stats panel.

2. **Workflow Proposal Lifecycle**: The enterprise WorkflowStudio had a full proposal → review → promote → deploy lifecycle with status badges. The forum admin handles deployment but the multi-step approval workflow is simpler. **Action**: Implement proposal lifecycle as Nostr event kind transitions if needed.

3. **WASM Visual Effects**: The frosted-glass drawer with GPU-accelerated particle flow field was a UX differentiator. No equivalent in the forum. **Action**: Consider Leptos + WASM canvas effects for the forum if visual polish is prioritized.

4. **Keyboard Shortcut (Ctrl+Shift+E)**: Enterprise drawer had a global keyboard shortcut. Forum uses standard browser navigation. **Action**: Low priority — standard navigation is sufficient.

## Removed Files

### Client (TypeScript/TSX)
- `client/src/features/enterprise/` — 22 files, ~993 lines (components, store, FX, WASM)
- `client/src/enterprise-standalone.tsx` — 48 lines
- `client/src/wasm/drawer-fx/` — Rust crate source for WASM particle effects

### Client (Innovation Services — previously quarantined)
- `client/src/features/graph/innovations-dormant/` — 8 services, ~4,428 lines

### Server (Rust handlers)
- `src/handlers/api_handler/broker/` — 488 lines
- `src/handlers/api_handler/workflows/` — 259 lines
- `src/handlers/api_handler/connectors/` — 130 lines
- `src/handlers/api_handler/policy/` — 78 lines
- `src/handlers/api_handler/mesh_metrics/` — 50 lines

### Total removed: ~7,469 lines

## Consequences

- VisionClaw becomes a focused knowledge-graph + physics visualization system
- Enterprise governance moves to the Nostr-native forum where it belongs
- ~7.5K fewer lines to compile, test, and maintain
- Server binary shrinks (5 fewer handler modules)
- No enterprise route (`#/enterprise`) — users go to the forum instead
