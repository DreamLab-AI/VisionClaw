import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { agentPollingService, AgentSwarmData, PollingConfig } from '../services/AgentPollingService';
import type { BotsAgent, BotsEdge } from '../types/BotsTypes';
import { createLogger } from '../../../utils/loggerConfig';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';
import { badgeFromNodeMetadata, edgeInfoFromWire } from '../chain/chainPayments';

const logger = createLogger('useAgentPolling');

/**
 * A node is a genuine swarm agent when it carries an `agent_type` in its
 * metadata (the marker `graph_state_actor` writes on the agent→node path) or
 * its node type is `agent`/`bot` (the server population classifier's markers).
 * The poll requests `/bots/data` (the dedicated agent store), but this guard is
 * defensive: pointed at a mixed endpoint, without it every document node would
 * be mapped to a fake `specialist`/`active` agent.
 */
export function isGenuineAgentNode(node: AgentSwarmData['nodes'][number]): boolean {
  if (node.metadata?.agent_type) return true;
  return node.type === 'agent' || node.type === 'bot';
}

/** Transform the agent-population poll response into BotsAgents and edges. */
export function transformAgentData(data: AgentSwarmData): {
  agents: BotsAgent[];
  edges: BotsEdge[];
} {
  const agentNodes = (data.nodes || []).filter(isGenuineAgentNode);

  const nodeIdToAgentId = new Map<number, string>();
  agentNodes.forEach(node => {
    nodeIdToAgentId.set(node.id, node.metadataId || String(node.id));
  });

  const agents = agentNodes.map(node => {
    const agentType = node.metadata?.agent_type || node.type || 'specialist';

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
      id: node.metadataId || String(node.id),
      name: node.label || `Agent-${node.id}`,
      type: agentType as BotsAgent['type'],
      status: (node.metadata?.status || 'active') as BotsAgent['status'],
      position,
      velocity,
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
        node.metadata.capabilities.split(',').map(cap => cap.trim()).filter(cap => cap) :
        undefined,
      did_nostr: node.metadata?.did_nostr || undefined,
      chain: badgeFromNodeMetadata(node.metadata),
    } as BotsAgent;
  });

  // Only edges whose endpoints are both agents survive, mirroring the node
  // filter so a stray non-agent edge never fabricates a phantom connection.
  const edges = (data.edges || [])
    .filter(edge => nodeIdToAgentId.has(edge.source) && nodeIdToAgentId.has(edge.target))
    .map(edge => ({
      id: edge.id,
      source: nodeIdToAgentId.get(edge.source) || String(edge.source),
      target: nodeIdToAgentId.get(edge.target) || String(edge.target),
      type: edge.edgeType,
      chainPayment: edgeInfoFromWire(edge.edgeType, edge.metadata),
      dataVolume: edge.weight * 1000,
      messageCount: Math.floor(edge.weight * 10),
      lastMessageTime: Date.now()
    } as BotsEdge));

  return { agents, edges };
}

/** Whether a polled agent differs from the cached one in anything drawn. */
export function agentChanged(prev: BotsAgent, next: BotsAgent): boolean {
  return (
    prev.position?.x !== next.position?.x ||
    prev.position?.y !== next.position?.y ||
    prev.position?.z !== next.position?.z ||
    prev.status !== next.status ||
    prev.health !== next.health ||
    prev.did_nostr !== next.did_nostr ||
    // S5: a new fold height or balance, or an anchor change, must redraw the badge.
    JSON.stringify(prev.chain ?? null) !== JSON.stringify(next.chain ?? null)
  );
}

/** Whether a polled edge differs from the cached one in anything drawn. */
export function edgeChanged(prev: BotsEdge, next: BotsEdge): boolean {
  return (
    prev.dataVolume !== next.dataVolume ||
    prev.messageCount !== next.messageCount ||
    prev.type !== next.type ||
    // S5: a payment moving from unsettled to settled must redraw its label.
    JSON.stringify(prev.chainPayment ?? null) !== JSON.stringify(next.chainPayment ?? null)
  );
}

export interface UseAgentPollingOptions {
  enabled?: boolean;
  config?: Partial<PollingConfig>;
  onError?: (error: Error) => void;
}

export interface AgentPollingState {
  agents: BotsAgent[];
  edges: BotsEdge[];
  metadata?: {
    totalAgents: number;
    activeAgents: number;
    totalTasks: number;
    completedTasks: number;
    avgSuccessRate: number;
    totalTokens: number;
  };
  isPolling: boolean;
  activityLevel: 'active' | 'idle';
  lastUpdate: number;
  error: Error | null;
}


export function useAgentPolling(options: UseAgentPollingOptions = {}) {
  const { enabled = true, config, onError } = options;
  
  
  const agentsMapRef = useRef<Map<string, BotsAgent>>(new Map());
  const edgesMapRef = useRef<Map<string, BotsEdge>>(new Map());
  const lastUpdateRef = useRef<number>(0);
  
  const [state, setState] = useState<AgentPollingState>({
    agents: [],
    edges: [],
    metadata: undefined,
    isPolling: false,
    activityLevel: 'idle',
    lastUpdate: 0,
    error: null
  });


  const updateStateRef = useRef<(data: AgentSwarmData) => void>(undefined);
  updateStateRef.current = (data: AgentSwarmData) => {
    const { agents, edges } = transformAgentData(data);
    const now = Date.now();

    
    let hasAgentChanges = false;
    agents.forEach(agent => {
      const existing = agentsMapRef.current.get(agent.id);
      if (!existing || agentChanged(existing, agent)) {
        hasAgentChanges = true;
        agentsMapRef.current.set(agent.id, agent);
      }
    });

    
    let hasEdgeChanges = false;
    const newEdgeIds = new Set<string>();
    edges.forEach(edge => {
      newEdgeIds.add(edge.id);
      const existing = edgesMapRef.current.get(edge.id);
      if (!existing || edgeChanged(existing, edge)) {
        hasEdgeChanges = true;
        edgesMapRef.current.set(edge.id, edge);
      }
    });

    
    edgesMapRef.current.forEach((edge, id) => {
      if (!newEdgeIds.has(id)) {
        edgesMapRef.current.delete(id);
        hasEdgeChanges = true;
      }
    });

    
    if (hasAgentChanges || hasEdgeChanges || now - lastUpdateRef.current > 5000) {
      lastUpdateRef.current = now;

      setState(prev => ({
        ...prev,
        agents: Array.from(agentsMapRef.current.values()),
        edges: Array.from(edgesMapRef.current.values()),
        metadata: data.metadata ? {
          totalAgents: data.metadata.total_agents,
          activeAgents: data.metadata.active_agents,
          totalTasks: data.metadata.total_tasks,
          completedTasks: data.metadata.completed_tasks,
          avgSuccessRate: data.metadata.avg_success_rate,
          totalTokens: data.metadata.total_tokens
        } : prev.metadata,
        lastUpdate: now,
        error: null
      }));

      
      if (hasAgentChanges) {
        agentTelemetry.logAgentAction('polling', 'update', 'agents_changed', {
          agentCount: agents.length,
          activeCount: agents.filter(a => a.status === 'active').length
        });
      }
    }
  };

  
  const updateState = useCallback((data: AgentSwarmData) => {
    updateStateRef.current?.(data);
  }, []);

  
  const handleErrorRef = useRef<(error: Error) => void>(undefined);
  handleErrorRef.current = (error: Error) => {
    logger.error('Polling error:', error);
    setState(prev => ({ ...prev, error }));
    onError?.(error);
  };

  const handleError = useCallback((error: Error) => {
    handleErrorRef.current?.(error);
  }, []);

  
  useEffect(() => {
    if (config) {
      agentPollingService.configure(config);
    }
  }, [config]);

  
  useEffect(() => {
    if (!enabled) {
      agentPollingService.stop();
      setState(prev => ({ ...prev, isPolling: false }));
      return;
    }

    
    const unsubscribe = agentPollingService.subscribe(updateState, handleError);

    
    agentPollingService.start();
    setState(prev => ({ ...prev, isPolling: true }));

    
    const statusInterval = setInterval(() => {
      const status = agentPollingService.getStatus();
      setState(prev => {
        
        if (prev.isPolling === status.isPolling && prev.activityLevel === status.activityLevel) {
          return prev;
        }
        return {
          ...prev,
          isPolling: status.isPolling,
          activityLevel: status.activityLevel
        };
      });
    }, 2000);

    return () => {
      unsubscribe();
      agentPollingService.stop();
      clearInterval(statusInterval);
    };
  }, [enabled]); 

  
  const result = useMemo(() => ({
    agents: state.agents,
    edges: state.edges,
    metadata: state.metadata,
    isPolling: state.isPolling,
    activityLevel: state.activityLevel,
    lastUpdate: state.lastUpdate,
    error: state.error,
    
    pollNow: () => agentPollingService.pollNow(),
    getStatus: () => agentPollingService.getStatus(),
    configure: (newConfig: Partial<PollingConfig>) => agentPollingService.configure(newConfig)
  }), [state]);

  return result;
}