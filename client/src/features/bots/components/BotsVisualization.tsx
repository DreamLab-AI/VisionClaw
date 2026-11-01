/**
 * BotsVisualization.tsx
 * Thin orchestrator: wires BotsDataContext → BotsNode + BotsEdges renderers.
 *
 * It holds no agent position. Every body, edge, trail and label asks the drawn-positions
 * array for the agent's position in its own frame (`agentDrawnPosition`), so the overlay
 * sits where the gem is.
 *
 * Responsibilities:
 *   - Consume `useBotsData()` context (no WS/polling logic here)
 *   - Resolve colour palette from settings
 *   - Render loading / error / empty states
 *   - Delegate all 3-D rendering to BotsNode / BotsEdges
 */
import React, { useEffect, useState, useMemo } from 'react';
import { Html } from '@react-three/drei';
import { BotsAgent, BotsEdge, BotsState, TokenUsage } from '../types/BotsTypes';
import { createLogger } from '../../../utils/loggerConfig';
import { useTelemetry } from '../../../telemetry/useTelemetry';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';
import { useSettingsStore } from '../../../store/settingsStore';
import { useBotsData } from '../contexts/BotsDataContext';
import { getVisionClawColors } from './BotsShared';
import { agentTrustKey } from '../agentIdentity';
import { BotsNode } from './BotsNode';
import { BotsEdges } from './BotsEdges';
import { ChainPaymentLabels } from '../chain/ChainPaymentLabels';

const logger = createLogger('BotsVisualization');

// ---------------------------------------------------------------------------
// Main Visualization Component
// Note: pure rendering component. Positions are read from the drawn-positions array by
// the children; no client-side physics and no position state here.
// ---------------------------------------------------------------------------
export const BotsVisualization: React.FC = () => {
  const settings = useSettingsStore(state => state.settings);
  // Per-swarm hue tint (control-centre Agents → Behaviour). Client-only, default on.
  const swarmTint = useSettingsStore(s => s.get<boolean>('visualisation.graphTypeVisuals.agent.swarmTint')) ?? true;
  const { botsData: contextBotsData } = useBotsData();
  const telemetry = useTelemetry('BotsVisualization');

  const [botsData, setBotsData] = useState<BotsState>({
    agents: new Map(),
    edges: new Map(),
    communications: [],
    tokenUsage: { total: 0, byAgent: {} },
    lastUpdate: 0,
  });
  const [isLoading, setIsLoading]     = useState(true);
  const [error]                       = useState<string | null>(null);
  const [_mcpConnected, setMcpConnected] = useState(false);

  const colors = useMemo(
    () => getVisionClawColors(settings as unknown as Record<string, unknown> | undefined),
    [settings],
  );

  // Sync context data → local BotsState
  useEffect(() => {
    if (!contextBotsData) {
      logger.debug('[VISIONCLAW] No context data available yet');
      return;
    }

    logger.debug('[VISIONCLAW] Processing bots data from context', contextBotsData);
    setIsLoading(false);

    const agents = contextBotsData.agents || [];
    const agentMap = new Map<string, BotsAgent>();

    agents.forEach((agent) => {
      agentMap.set(agent.id, agent);

      agentTelemetry.logAgentAction(agent.id, agent.type, 'state_update', {
        status: agent.status, health: agent.health,
        cpuUsage: agent.cpuUsage, tokenRate: agent.tokenRate,
      });
    });

    const edges   = contextBotsData.edges || [];
    const edgeMap = new Map<string, BotsEdge>();
    edges.forEach((edge: BotsEdge) => edgeMap.set(edge.id, edge));

    const contextRecord = contextBotsData as unknown as Record<string, unknown>;
    setBotsData({
      agents: agentMap,
      edges:  edgeMap,
      communications: [],
      tokenUsage: (contextRecord.tokenUsage as TokenUsage | undefined) || { total: 0, byAgent: {} },
      lastUpdate: Date.now(),
    });

    setMcpConnected(agentMap.size > 0);

    agentTelemetry.logAgentAction('visualization', 'system', 'data_update', {
      agentCount: agentMap.size,
      edgeCount:  edgeMap.size,
      hasContextData: !!contextBotsData,
    });
  }, [contextBotsData]);

  // -------------------------------------------------------------------------
  // Render states
  // -------------------------------------------------------------------------
  if (error) {
    return (
      <Html center>
        <div style={{ color: '#E74C3C', padding: '20px', textAlign: 'center' }}>
          <h3>VisionClaw Error</h3>
          <p>{error}</p>
        </div>
      </Html>
    );
  }

  if (isLoading) {
    return (
      <Html center>
        <div style={{ color: '#F1C40F', padding: '20px', textAlign: 'center' }}>
          <h3>Loading VisionClaw...</h3>
          <p>Initializing hive mind visualization</p>
        </div>
      </Html>
    );
  }

  if (botsData.agents.size === 0) return null;

  // -------------------------------------------------------------------------
  // Main render
  // -------------------------------------------------------------------------
  return (
    <group>
      {/* Inter-agent collaboration edges — one THREE.LineSegments draw for the
          whole layer (see BotsEdges). Endpoints track the agents' drawn positions;
          per-edge colour/opacity/pulse ride the vertex colours. */}
      <BotsEdges
        edges={botsData.edges}
        agents={botsData.agents}
        color={colors.edge}
      />

      {/* S5: amount + short txid at each payment edge's midpoint. */}
      <ChainPaymentLabels edges={botsData.edges} />

      {/* Nodes */}
      {Array.from(botsData.agents.values()).map((node, index) => {
        const nodeColor = colors.getAgentColor
          ? colors.getAgentColor(node.type)
          : ((colors as unknown as Record<string, string>)[node.type] || colors.coordinator);

        return (
          <BotsNode
            // COM-14 / WP-1: React identity follows the agent's trust key
            // (did:nostr when carried, else the task_id fallback), not a
            // transient array index.
            key={agentTrustKey(node)}
            agent={node}
            index={index}
            color={nodeColor}
            swarmTint={swarmTint}
          />
        );
      })}
    </group>
  );
};
