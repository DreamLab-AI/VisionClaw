
import React, { createContext, useContext, useState, useEffect, useMemo, useCallback } from 'react';
import type { BotsAgent, BotsEdge, BotsFullUpdateMessage } from '../types/BotsTypes';
import { botsWebSocketIntegration } from '../services/BotsWebSocketIntegration';
import { decodePositionFrame } from '../../../types/binaryProtocol';
import { useAgentPolling } from '../hooks/useAgentPolling';
import { agentPollingService } from '../services/AgentPollingService';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('BotsDataContext');

interface BotsData {
  nodeCount: number;
  edgeCount: number;
  tokenCount: number;
  mcpConnected: boolean;
  dataSource: string;
  
  agents: BotsAgent[];
  edges: BotsEdge[];  
  multiAgentMetrics?: {
    totalAgents: number;
    activeAgents: number;
    totalTasks: number;
    completedTasks: number;
    avgSuccessRate: number;
    totalTokens: number;
  };
  lastUpdate?: string;
}

interface BotsDataContextType {
  botsData: BotsData | null;
  updateBotsData: (data: BotsData) => void;
  updateFromFullUpdate: (update: BotsFullUpdateMessage) => void;
  pollingStatus?: {
    isPolling: boolean;
    activityLevel: 'active' | 'idle';
    lastUpdate: number;
    error: Error | null;
  };
  pollNow?: () => Promise<void>;
  configurePolling?: (config: any) => void;
}

const BotsDataContext = createContext<BotsDataContextType | undefined>(undefined);

export const BotsDataProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  // Track actual MCP connection status from API
  const [mcpConnectionStatus, setMcpConnectionStatus] = useState<boolean>(false);

  // Memoize to prevent a fresh reference on every BotsDataProvider render
  // which otherwise re-triggers the `configure` effect inside useAgentPolling,
  // producing stop/start spam every 2 seconds in the console.
  const pollingConfig = useMemo(
    () => ({
      activePollingInterval: 3000,
      idlePollingInterval: 15000,
      enableSmartPolling: true,
    }),
    [],
  );
  const onPollingError = useCallback((error: Error) => {
    logger.error('Polling error:', error);
  }, []);
  // OMNIBUS-FIX-6: AgentPollingService.poll() hits /graph/data every 2-15s.
  // /graph/data returns the FULL 4,519-node + 11,811-edge KG, not just agents.
  // Each poll: ~MB JSON parse + O(N) hashData (Array.map+join over node ids) +
  // setBotsData triggering full BotsDataProvider tree re-render. Overlaps with
  // binary-frame Comlink saturation → freeze. Disable until polling is gated
  // on actual agent presence (metadata.total_agents > 0) or moved to a
  // dedicated /api/bots/swarm endpoint.
  // URL override: append ?enableBotsPolling=true to test.
  const enableBotsPolling = typeof window !== 'undefined'
    && window.location.search.includes('enableBotsPolling=true');
  const pollingData = useAgentPolling({
    enabled: enableBotsPolling,
    config: pollingConfig,
    onError: onPollingError,
  });

  const [botsData, setBotsData] = useState<BotsData | null>({
    nodeCount: 0,
    edgeCount: 0,
    tokenCount: 0,
    mcpConnected: false,
    dataSource: 'live',
    agents: [],
    edges: []  
  });

  const updateBotsData = (data: BotsData) => {
    setBotsData(data);
  };

  const updateFromFullUpdate = (update: BotsFullUpdateMessage) => {
    setBotsData(prev => ({
      ...prev!,
      agents: update.agents || [],
      nodeCount: update.agents?.length || 0,
      edgeCount: 0, 
      tokenCount: update.multiAgentMetrics?.totalTokens || 0,
      mcpConnected: true,
      dataSource: 'live',
      multiAgentMetrics: update.multiAgentMetrics || {
        totalAgents: 0,
        activeAgents: 0,
        totalTasks: 0,
        completedTasks: 0,
        avgSuccessRate: 0,
        totalTokens: 0
      },
      lastUpdate: update.timestamp
    }));
  };

  
  const updateFromGraphData = (data: any) => {
    
    if (!data) {
      logger.warn('updateFromGraphData received undefined data');
      return;
    }
    
    
    const transformedAgents = (data.nodes || []).map((node: any) => {
      
      const agentType = node.metadata?.agent_type || node.type || node.node_type || node.nodeType;

      if (!agentType) {
        logger.error('Missing agent type for node:', {
          nodeId: node.id,
          metadataId: node.metadataId || node.metadata_id,
          metadata: node.metadata,
          type: node.type,
          node_type: node.node_type,
          nodeType: node.nodeType
        });
      }

      
      const position = node.data?.position || {
        x: node.data?.x || 0,
        y: node.data?.y || 0,
        z: node.data?.z || 0
      };

      const velocity = node.data?.velocity || {
        x: node.data?.vx || 0,
        y: node.data?.vy || 0,
        z: node.data?.vz || 0
      };

      return {
        
        id: node.metadataId || node.metadata_id || String(node.id),
        name: node.label || node.metadata?.name || `Agent-${node.id}`,
        type: agentType,
        status: node.metadata?.status || 'active', 
        position,
        velocity,
        force: { x: 0, y: 0, z: 0 },
        
        cpuUsage: parseFloat(node.metadata?.cpu_usage || '0'),
        memoryUsage: parseFloat(node.metadata?.memory_usage || '0'),
        health: parseFloat(node.metadata?.health || '100'),
        workload: parseFloat(node.metadata?.workload || '0'),
        tokens: parseInt(node.metadata?.tokens || '0'),
        createdAt: node.metadata?.created_at || new Date().toISOString(),
        age: parseInt(node.metadata?.age || '0'),
        
        swarmId: node.metadata?.swarm_id,
        parentQueenId: node.metadata?.parent_queen_id,
        capabilities: node.metadata?.capabilities ?
          node.metadata.capabilities.split(',').map((cap: string) => cap.trim()).filter((cap: string) => cap) :
          undefined,
        connections: [],
      };
    });

    
    
    const nodeIdToAgentId = new Map();
    data.nodes?.forEach((node: any) => {
      nodeIdToAgentId.set(node.id, node.metadataId || node.metadata_id || String(node.id));
    });

    const transformedEdges = (data.edges || []).map((edge: any) => ({
      id: edge.id,
      source: nodeIdToAgentId.get(edge.source) || String(edge.source),
      target: nodeIdToAgentId.get(edge.target) || String(edge.target),
      dataVolume: edge.weight * 1000,  
      messageCount: Math.floor(edge.weight * 10),  
    }));
    
    setBotsData(prev => ({
      ...prev!,
      agents: transformedAgents,
      edges: transformedEdges,
      nodeCount: transformedAgents.length,
      edgeCount: transformedEdges.length,
      tokenCount: transformedAgents.reduce((sum: number, agent: any) => sum + (agent.tokens || 0), 0),
      mcpConnected: true,
      dataSource: 'live',
      lastUpdate: new Date().toISOString()
    }));
  };


  const updateFromBinaryPositions = (binaryData: ArrayBuffer) => {
    try {
      // ADR-061: position frames carry raw u32 node ids — no flag bits, no
      // agent/knowledge discriminator. We look up positions for each known
      // agent by its id string (matched against the raw u32 id) and skip
      // anything not in our agent list.
      const frame = decodePositionFrame(binaryData);
      if (!frame || frame.nodes.size === 0) {
        return;
      }

      logger.debug(`Processing position frame (${frame.nodes.size} entries) for agent overlay`);

      setBotsData(prev => {
        if (!prev) return prev;

        let touched = 0;
        const updatedAgents = prev.agents.map(agent => {
          const numericId = Number(agent.id);
          if (!Number.isFinite(numericId)) return agent;
          const positionUpdate = frame.nodes.get(numericId);
          if (!positionUpdate) return agent;
          touched++;
          return {
            ...agent,
            position: positionUpdate.position,
            velocity: positionUpdate.velocity,
            lastPositionUpdate: Date.now(),
          };
        });

        if (touched === 0) {
          return prev;
        }
        return {
          ...prev,
          agents: updatedAgents,
          lastUpdate: new Date().toISOString(),
        };
      });

    } catch (error) {
      logger.error('Error processing binary position updates:', error);
    }
  };

  
  useEffect(() => {
    if (pollingData.agents.length > 0 || pollingData.edges.length > 0) {
      setBotsData({
        nodeCount: pollingData.agents.length,
        edgeCount: pollingData.edges.length,
        tokenCount: pollingData.metadata?.totalTokens || 0,
        mcpConnected: mcpConnectionStatus,  // Use actual MCP status from API
        dataSource: 'live',
        agents: pollingData.agents,
        edges: pollingData.edges,
        multiAgentMetrics: pollingData.metadata,
        lastUpdate: new Date(pollingData.lastUpdate).toISOString()
      });
    }
  }, [pollingData, mcpConnectionStatus]);

  // Update mcpConnected status even when no agents are present
  useEffect(() => {
    setBotsData(prev => {
      if (!prev) return prev;
      if (prev.mcpConnected === mcpConnectionStatus) return prev;
      return { ...prev, mcpConnected: mcpConnectionStatus };
    });
  }, [mcpConnectionStatus]);

  useEffect(() => {

    const unsubscribe = botsWebSocketIntegration.on('bots-binary-position-update', (binaryData: ArrayBuffer) => {
      updateFromBinaryPositions(binaryData);
    });

    return () => {
      unsubscribe();
    };
  }, []);

  // Poll actual MCP connection status from API
  useEffect(() => {
    const checkMcpStatus = async () => {
      try {
        const response = await unifiedApiClient.getData('/bots/status');
        // API returns { success: true, data: { connected: true, ... } }
        const connected = response?.data?.connected ?? response?.connected ?? false;
        setMcpConnectionStatus(connected);
      } catch (error) {
        logger.error('Failed to check MCP status:', error);
        setMcpConnectionStatus(false);
      }
    };

    // Defer first check so bot status doesn't compete with critical-path
    // requests (graph data, WebSocket) for browser connection slots.
    const startId = setTimeout(checkMcpStatus, 3000);

    const interval = setInterval(checkMcpStatus, 5000);

    return () => { clearTimeout(startId); clearInterval(interval); };
  }, []);

  const contextValue = useMemo(() => ({
    botsData,
    updateBotsData,
    updateFromFullUpdate,
    
    pollingStatus: {
      isPolling: pollingData.isPolling,
      activityLevel: pollingData.activityLevel,
      lastUpdate: pollingData.lastUpdate,
      error: pollingData.error
    },
    pollNow: pollingData.pollNow,
    configurePolling: pollingData.configure
  }), [botsData, pollingData]);

  return (
    <BotsDataContext.Provider value={contextValue}>
      {children}
    </BotsDataContext.Provider>
  );
};

export const useBotsData = () => {
  const context = useContext(BotsDataContext);
  if (!context) {
    throw new Error('useBotsData must be used within a BotsDataProvider');
  }
  return context;
};