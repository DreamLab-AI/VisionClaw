---
title: System Health Monitoring
description: Use the HealthDashboard to monitor component health, physics simulation status, and MCP relay
category: how-to
tags: [monitoring, health, mcp, physics]
updated-date: 2026-04-18
---

# System Health Monitoring

This guide covers reading and acting on the System Health Monitor, including component status, physics simulation metrics, and MCP relay controls.

---

## Access the Dashboard

The Health Dashboard is rendered by `HealthDashboard.tsx` (`client/src/features/monitoring/components/`).

To reach it:

1. Open the enterprise drawer — press **Ctrl+Shift+E** (Windows/Linux) or **Cmd+Shift+E** (Mac).
2. Select the **System** tab.

The dashboard auto-refreshes every 5 seconds. Use the **Refresh** button (top-right of the card) to trigger an immediate poll.

---

## Understand the Health Status

The `useHealthService` hook polls two endpoints on mount and at a configurable interval:

| Endpoint | Data shape |
|----------|-----------|
| `GET /health` | `HealthStatus` — overall system health |
| `GET /health/physics` | `PhysicsHealth` — simulation status |

### HealthStatus shape

```typescript
interface HealthStatus {
  healthy: boolean;                    // overall system verdict
  components: Record<string, boolean>; // per-component pass/fail
  timestamp: string;                   // ISO 8601 — time of last check
  version?: string;                    // server version string
}
```

`healthy: true` means every monitored component reported healthy. `healthy: false` means at least one component failed — check the **Component Health** list to identify which one.

### Component health entries

Components reported in the `components` map:

| Key | What it monitors |
|-----|-----------------|
| `database` | Neo4j connectivity |
| `graph` | Graph data manager state |
| `physics` | Physics actor liveness |
| `websocket` | Active WebSocket service |

Each entry shows **OK** (green) or **FAILED** (red). A failed component that is not `physics` or `websocket` typically requires investigation at the backend level — check container logs.

---

## Read Physics Simulation Status

The **Physics Simulation** section reports the state of the CUDA force-directed simulation.

### PhysicsHealth shape

```typescript
interface PhysicsHealth {
  simulation_id?: string;      // UUID of the active simulation session
  running: boolean;            // true while the physics actor is stepping
  statistics?: {
    total_steps: number;                // cumulative simulation steps
    average_step_time_ms: number;       // mean wall-clock cost per step (ms)
    average_energy: number;             // mean kinetic energy of the graph
    gpu_memory_used_mb: number;         // GPU VRAM consumed by simulation (MB)
  };
}
```

**Healthy indicators:**
- `running: true`
- `average_step_time_ms` below ~16 ms (GPU-bound at 60 fps)
- `gpu_memory_used_mb` within the GPU's available VRAM

**Warning signs:**
- `running: false` with a valid `simulation_id` — physics actor may have crashed; check backend logs
- `average_step_time_ms` above 50 ms — GPU contention or node count too high for the hardware

---

## Configure Polling Behaviour

Pass options to `useHealthService` to control polling:

```typescript
import { useHealthService } from '@/features/monitoring/hooks/useHealthService';

// Default: polls every 5 000 ms
const { overallHealth, physicsHealth } = useHealthService();

// Custom interval (10 seconds)
const { overallHealth } = useHealthService({ pollHealth: true, pollInterval: 10_000 });

// Disable polling (one-time fetch only — trigger refreshHealth() manually)
const { overallHealth, refreshHealth } = useHealthService({ pollHealth: false });
```

`pollHealth` defaults to `true`. `pollInterval` defaults to `5000` ms.

---

## MCP Relay Controls

The **MCP Relay** section provides two actions:

| Button | Action | API call |
|--------|--------|---------|
| **Start Relay** | Starts the MCP relay process | `POST /health/mcp/start` |
| **View Logs** | Loads recent relay log output | `GET /health/mcp/logs` |

### Start the relay

Click **Start Relay**. On success a toast reads "MCP Relay Started". On failure the toast shows the error message from the server.

### View relay logs

Click **View Logs**. Logs appear in a monospace scrollable panel below the buttons (max height 12rem). Logs persist in local component state until the panel unmounts — clicking again replaces them with a fresh fetch.

---

## Interpret Overall Health Badges

| Badge | Meaning |
|-------|---------|
| HEALTHY (green) | All components healthy, server version shown |
| UNHEALTHY (red) | One or more components failed |

When the dashboard shows an error banner at the bottom of the card, the last health fetch itself failed (network error or 5xx). The banner includes the error message. Click **Refresh** to retry.

---

## See Also

- [Deployment Guide](../deployment-guide.md) — container health checks
- [Troubleshooting](../operations/troubleshooting.md) — common failure patterns
- [Physics & GPU Engine](../../explanation/physics-gpu-engine.md) — CUDA simulation internals
