import { useEffect, useRef, useState } from 'react';
import { botsWebSocketIntegration } from '../services/BotsWebSocketIntegration';
import { createLogger } from '../../../utils/loggerConfig';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';
import { useTelemetry } from '../../../telemetry/useTelemetry';

const logger = createLogger('useBotsWebSocketIntegration');


export function useBotsWebSocketIntegration() {
  const telemetry = useTelemetry('useBotsWebSocketIntegration');
  const [connectionStatus, setConnectionStatus] = useState({
    mcp: false,
    logseq: false,
    overall: false
  });
  const prevStatusRef = useRef({ mcp: false, logseq: false, overall: false });

  useEffect(() => {
    agentTelemetry.enable();
    logger.info('Initializing bots WebSocket integration (binary position updates only)');

    
    const unsubMcp = botsWebSocketIntegration.on('mcp-connected', ({ connected }) => {
      setConnectionStatus(prev => ({ ...prev, mcp: connected }));

      
      agentTelemetry.logAgentAction('websocket', 'mcp', connected ? 'connected' : 'disconnected');
    });

    const unsubLogseq = botsWebSocketIntegration.on('logseq-connected', ({ connected }) => {
      setConnectionStatus(prev => ({ ...prev, logseq: connected }));

      
      agentTelemetry.logAgentAction('websocket', 'logseq', connected ? 'connected' : 'disconnected');
    });

    
    const updateOverall = setInterval(() => {
      const status = botsWebSocketIntegration.getConnectionStatus();
      const prev = prevStatusRef.current;
      if (prev.mcp !== status.mcp || prev.logseq !== status.logseq || prev.overall !== status.overall) {
        prevStatusRef.current = { mcp: status.mcp, logseq: status.logseq, overall: status.overall };
        setConnectionStatus({ mcp: status.mcp, logseq: status.logseq, overall: status.overall });
      }
    }, 2000);

    
    
    logger.info('WebSocket connection ready for binary position updates. Agent metadata fetched via REST API.');
    agentTelemetry.logAgentAction('websocket', 'hook', 'initialized_position_updates');

    return () => {
      unsubMcp();
      unsubLogseq();
      clearInterval(updateOverall);

      
      agentTelemetry.logAgentAction('websocket', 'hook', 'cleanup');

      
      
    };
  }, []);

  return connectionStatus;
}