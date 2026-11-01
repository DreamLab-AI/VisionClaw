# Agent Swarm Polling System

## Overview

The client polling system provides real-time updates of agent swarm metadata and positions through a **hybrid architecture** combining REST polling for metadata and WebSocket binary updates for real-time position data.

## Architecture

### Components

1. **AgentPollingService** - Core polling service with configurable intervals
2. **useAgentPolling** - React hook for efficient state management
3. **AgentPollingStatus** - UI component showing real-time status
4. **BotsDataContext** - Central data provider integrating polling and WebSocket data

### Data Flow (Hybrid Architecture)

```
REST API (/api/bots/data) → AgentPollingService → useAgentPolling → BotsDataContext
                                                                            ↓
WebSocket Binary Updates → BotsWebSocketIntegration → Binary Protocol → Position Data
       (52-byte V3 format)                                                   ↓
                                                              3D Visualization (Three.js)
                                                                          ↓
                                                              GPU Force-Directed Graph
```

**Key Points:**
- **REST**: Fetches agent metadata (health, status, tokens, etc.) via polling
- **WebSocket Binary**: Real-time position updates (52-byte binary format, V3 protocol, ADR-031)
- **BotsWebSocketIntegration**: Handles ONLY binary position updates (no longer does polling)
- **BotsDataContext**: Coordinates both REST and WebSocket data sources

## Features

### 1. Configurable Polling Intervals

- **Active Mode**: 1s default (configurable down to 250ms)
- **Idle Mode**: 5s default (configurable up to 30s)
- **Smart Polling**: Automatically switches between active/idle based on:
  - Number of active agents (>20% triggers active mode)
  - Pending tasks
  - Recent data changes

### 2. Efficient State Management

- **Change Detection**: Only updates React state when data actually changes
- **Drawn position**: the overlay draws agents at the drawn-positions array, with no interpolation of its own
- **Batched Updates**: Prevents excessive re-renders
- **Memory-efficient Maps**: Uses Maps instead of arrays for O(1) lookups

### 3. Performance Monitoring

- Real-time metrics tracking
- Success/error rates
- Average poll duration
- Data freshness indicators

### 4. UI Integration

- Live status overlay with activity indicators
- Configurable polling speed controls
- Error display and retry status
- Performance metrics display

## Configuration

### Basic Usage

```typescript
import { useAgentPolling } from './features/bots/hooks/useAgentPolling';

function MyComponent() {
  const { agents, edges, metadata, pollNow } = useAgentPolling({
    enabled: true,
    config: {
      activePollingInterval: 1000,
      idlePollingInterval: 5000,
      enableSmartPolling: true
    }
  });
  
  // Use agent data in your component
  return <div>Active agents: {metadata?.activeAgents || 0}</div>;
}
```

### Polling Presets

```typescript
import { POLLING_PRESETS } from './config/pollingConfig';

// Real-time mode (500ms/2s)
agentPollingService.configure(POLLING_PRESETS.realtime);

// Standard mode (1s/5s)
agentPollingService.configure(POLLING_PRESETS.standard);

// Performance mode (2s/10s)
agentPollingService.configure(POLLING_PRESETS.performance);
```

## Integration with 3D Visualization

The polling system provides smooth real-time updates to the Three.js visualization:

1. **Position Updates**: Agent positions from GPU force-directed graph
2. **Drawn position**: Agents are drawn at the position in the drawn-positions array (`agentDrawnPosition`); the worker already tweens, so the overlay does no smoothing of its own
3. **Metadata Updates**: Health, status, CPU usage, etc.
4. **Edge Updates**: Communication patterns between agents

### Agent position

The bots overlay keeps no position of its own. `BotsNode`, `AgentTrail`, `BotsEdges` and `ChainPaymentLabels` each call `agentDrawnPosition(agent.id, out)` in their frame, which reads the same drawn-positions array the agent's gem is drawn from.

## Performance Considerations

1. **Smart Activity Detection**
   - Reduces polling frequency when system is idle
   - Increases frequency during active tasks

2. **Change Detection**
   - Hash-based comparison to detect actual data changes
   - Prevents unnecessary state updates and re-renders

3. **Error Handling**
   - Exponential backoff for failed requests
   - Automatic retry with configurable limits
   - Graceful degradation

4. **Memory Management**
   - Efficient Map-based storage
   - Automatic cleanup of stale data
   - Limited history for performance metrics

## API Endpoints

### GET /api/bots/data

Returns complete agent swarm state:

```json
{
  "nodes": [{
    "id": 1,
    "metadata_id": "agent-123",
    "label": "Coder Agent",
    "node_type": "coder",
    "data": {
      "position": {"x": 10.5, "y": 5.2, "z": -3.1},
      "velocity": {"x": 0.1, "y": 0, "z": -0.05}
    },
    "metadata": {
      "agent_type": "coder",
      "status": "active",
      "health": "95",
      "cpu_usage": "45.2",
      "memory_usage": "62.1",
      "tokens": "1523"
    }
  }],
  "edges": [{
    "id": "edge-1",
    "source": 1,
    "target": 2,
    "weight": 0.8
  }],
  "metadata": {
    "total_agents": 5,
    "active_agents": 3,
    "total_tasks": 10,
    "completed_tasks": 7
  }
}
```

## Current Architecture Status

### ✅ Implemented
- REST polling for agent metadata via `/api/bots/data`
- WebSocket binary position updates (V3 protocol, 52 bytes/node, ADR-031)
- Hybrid data coordination through BotsDataContext
- Smart polling with activity-based interval adjustment
- Change detection to prevent unnecessary re-renders

### 🚧 Deprecated/Legacy
- **REMOVED**: `PROTOCOL_V1` (34-byte) and `PROTOCOL_V2` (36-byte) support — both removed server-side; live protocol is V3 only (52 bytes/node, ADR-031)
- **DEPRECATED**: WebSocket polling methods in BotsWebSocketIntegration
  - `startBotsGraphPolling()` - use REST via BotsDataContext
  - `stopBotsGraphPolling()` - no longer needed
  - `requestInitialData()` - use REST polling
  - `restartPolling()` - handled by BotsDataContext

### ⚠️ Known Issues
- Settings system is brittle - avoid updating UX names
- Multiple WebSocket service implementations need consolidation
- Magic numbers in visualization components need extraction to config

## Future Enhancements

1. **WebSocket Service Consolidation**: Unify EnhancedWebSocketService, VoiceWebSocketService, and BotsWebSocketIntegration
2. **Differential Updates**: Send only changed metadata instead of full snapshots
3. **Binary Metadata Protocol**: Extend binary protocol to include metadata (not just positions)
4. **Predictive Prefetching**: Anticipate data needs based on user interaction patterns
5. **Worker Thread Processing**: Offload graph calculations and data transformation to Web Workers
6. **Compression**: Implement binary compression for large agent swarms (>1000 agents)