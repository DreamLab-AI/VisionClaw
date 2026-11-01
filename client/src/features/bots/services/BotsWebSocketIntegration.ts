import { createLogger } from '../../../utils/loggerConfig';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';
import { webSocketService } from '../../../store/websocketStore';
import { webSocketEventBus } from '../../../services/WebSocketEventBus';

const logger = createLogger('BotsWebSocketIntegration');


export class BotsWebSocketIntegration {
  private static instance: BotsWebSocketIntegration;
  private knowledgeConnected = false;
  private listeners: Map<string, Set<(data: any) => void>> = new Map();

  private constructor() {
    this.initializeConnections();
  }

  static getInstance(): BotsWebSocketIntegration {
    if (!BotsWebSocketIntegration.instance) {
      BotsWebSocketIntegration.instance = new BotsWebSocketIntegration();
    }
    return BotsWebSocketIntegration.instance;
  }

  private async initializeConnections() {
    logger.info('Initializing WebSocket connection for graph data');

    
    this.initializeKnowledgeConnection();
  }

  private initializeKnowledgeConnection() {
    
    webSocketService.onConnectionStatusChange((connected) => {
      logger.info(`Knowledge WebSocket connection status: ${connected}`);
      this.knowledgeConnected = connected;
      this.emit('knowledge-connected', { connected });

      
      agentTelemetry.logWebSocketMessage('connection_status_change', 'incoming', { connected });
      
      
      
      logger.info(`WebSocket connection ${connected ? 'established' : 'lost'} - position updates ${connected ? 'enabled' : 'disabled'}`);
    });

    
    // Cast to any to handle Bots-specific message types not in standard WS types
    webSocketService.onMessage((message: any) => {
      agentTelemetry.logWebSocketMessage(message.type || 'unknown', 'incoming', message.data);

      // Emit to cross-service event bus
      webSocketEventBus.emit('message:bots', { data: message });

      if (message.type === 'graph-update') {
        logger.debug('Received Knowledge graph update', message.data);
        this.emit('knowledge-graph-update', message.data);
      } else if (message.type === 'botsGraphUpdate') {
        
        const nodeCount = message.data?.nodes?.length || 0;
        const edgeCount = message.data?.edges?.length || 0;
        logger.debug('Received bots graph update with', nodeCount, 'nodes and', edgeCount, 'edges');

        
        agentTelemetry.logWebSocketMessage('botsGraphUpdate', 'incoming', {
          nodeCount,
          edgeCount,
          hasData: !!message.data
        });

        
        if (message.data?.nodes) {
          logger.debug(`[NODES] Received ${message.data.nodes.length} nodes with positions`);
        }

        
        if (!message.data) {
          logger.warn('botsGraphUpdate message has no data field:', message);
        }
        this.emit('bots-graph-update', message.data);
      } else if (message.type === 'bots-full-update') {
        const agentCount = message.agents?.length || 0;
        logger.debug('Received bots full update with', agentCount, 'agents');

        
        agentTelemetry.logWebSocketMessage('bots-full-update', 'incoming', {
          agentCount,
          hasMultiAgentMetrics: !!message.multiAgentMetrics
        });

        this.emit('bots-full-update', message);
        
        this.processBotsUpdate({
          agents: message.agents,
          multiAgentMetrics: message.multiAgentMetrics,
          timestamp: message.timestamp
        });
      }
    });

    
    webSocketService.onBinaryMessage((data) => {
      logger.debug(`Received Knowledge binary update: ${data.byteLength} bytes`);

      
      agentTelemetry.logWebSocketMessage('binary_position_update', 'incoming', undefined, data.byteLength);

      this.emit('knowledge-binary-update', data);
    });
  }

  private processBotsUpdate(data: any) {
    
    if (data.agents) {
      this.emit('bots-agents-update', data.agents);
    }

    if (data.edges) {
      this.emit('bots-edges-update', data.edges);
    }

    if (data.communications) {
      this.emit('bots-communications-update', data.communications);
    }

    if (data.tokenUsage) {
      this.emit('bots-token-usage', data.tokenUsage);
    }

    
    this.emit('bots-update', data);
  }

  // MCP connectivity is not tracked by this service (it only owns the Knowledge
  // websocket). The real MCP status is polled from `/bots/status` in
  // BotsDataContext, so no `mcp` field is exposed here rather than a stub value.
  getConnectionStatus() {
    return {
      knowledge: this.knowledgeConnected,
      overall: this.knowledgeConnected
    };
  }

  
  on(event: string, callback: (data: any) => void) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback);

    return () => {
      this.listeners.get(event)?.delete(callback);
    };
  }

  private emit(event: string, data: any) {
    this.listeners.get(event)?.forEach(callback => {
      try {
        callback(data);
      } catch (error) {
        logger.error(`Error in event listener for ${event}:`, error);
      }
    });
  }

  
  clearAgents() {
    logger.info('Clearing all agents data');
    
    
    this.emit('bots-graph-update', {
      nodes: [],
      edges: [],
      metadata: {}
    });
  }

  disconnect() {
    logger.info('Disconnecting WebSocket services');
    this.clearAgents();
    
    webSocketService.close();
    this.knowledgeConnected = false;
  }
}

// Export singleton instance
export const botsWebSocketIntegration = BotsWebSocketIntegration.getInstance();