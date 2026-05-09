

import React, { useState, useEffect, useRef } from 'react';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('AgentTelemetryStream');

interface TelemetryMessage {
  timestamp: number;
  agentId: string;
  agentType?: string;
  message: string;
  level: 'info' | 'warning' | 'error' | 'success';
}

export const AgentTelemetryStream: React.FC = () => {
  const [messages, setMessages] = useState<TelemetryMessage[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [activeTab, setActiveTab] = useState<'telemetry' | 'goap'>('telemetry');
  const streamRef = useRef<HTMLDivElement>(null);
  const pollIntervalRef = useRef<NodeJS.Timeout | null>(null);

  
  useEffect(() => {
    if (streamRef.current) {
      streamRef.current.scrollTop = streamRef.current.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    const pollTelemetry = async () => {
      try {
        const response = await unifiedApiClient.get('/bots/agents');

        if (response.data && response.data.agents) {
          const newMessages: TelemetryMessage[] = response.data.agents.map((agent: Record<string, unknown>) => ({
            timestamp: Date.now(),
            agentId: (agent.id as string) || 'unknown',
            agentType: (agent.type as string) || (agent.agent_type as string) || 'agent',
            message: formatAgentStatus(agent),
            level: getMessageLevel(agent)
          }));

          setMessages(prev => {
            const combined = [...prev, ...newMessages];
            
            return combined.slice(-50);
          });

          setIsConnected(true);
        }
      } catch (error) {
        logger.error('Failed to poll telemetry:', error);
        setIsConnected(false);
      }
    };

    const startId = setTimeout(pollTelemetry, 3000);
    pollIntervalRef.current = setInterval(pollTelemetry, 5000);

    return () => {
      clearTimeout(startId);
      if (pollIntervalRef.current) {
        clearInterval(pollIntervalRef.current);
      }
    };
  }, []);

  const formatAgentStatus = (agent: Record<string, unknown>): string => {
    const parts: string[] = [];

    if (agent.status) parts.push(`STS:${String(agent.status).toUpperCase()}`);
    if (agent.health !== undefined) parts.push(`HP:${Math.round(Number(agent.health))}%`);

    const cpuUsage = agent.cpuUsage ?? agent.cpu_usage;
    if (cpuUsage !== undefined) parts.push(`CPU:${Math.round(Number(cpuUsage))}%`);
    const memoryUsage = agent.memoryUsage ?? agent.memory_usage;
    if (memoryUsage !== undefined) parts.push(`MEM:${Math.round(Number(memoryUsage))}MB`);
    if (agent.workload !== undefined) parts.push(`WL:${Math.round(Number(agent.workload))}`);
    const currentTask = (agent.current_task ?? agent.currentTask) as string | undefined;
    if (currentTask) parts.push(`TSK:${currentTask.substring(0, 20)}`);

    return parts.join(' | ') || 'IDLE';
  };

  const getMessageLevel = (agent: Record<string, unknown>): 'info' | 'warning' | 'error' | 'success' => {
    const health = Number(agent.health ?? 100);
    if (agent.status === 'error' || health < 30) return 'error';
    if (agent.status === 'warning' || health < 60) return 'warning';
    if (agent.status === 'active' || agent.status === 'working') return 'success';
    return 'info';
  };

  const getLevelColor = (level: string): string => {
    switch (level) {
      case 'error': return '#ff0000';
      case 'warning': return '#ff8800';
      case 'success': return '#00ff00';
      default: return '#000000';
    }
  };

  const formatTime = (timestamp: number): string => {
    const date = new Date(timestamp);
    return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}:${String(date.getSeconds()).padStart(2, '0')}`;
  };

  return (
    <div style={{
      marginTop: '8px',
      borderTop: '1px solid rgba(255,255,255,0.15)',
      paddingTop: '8px'
    }}>
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        marginBottom: '6px',
        fontSize: '9px',
        fontWeight: '600',
        color: '#fbbf24'
      }}>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button
            onClick={() => setActiveTab('telemetry')}
            style={{
              background: activeTab === 'telemetry' ? '#ff8800' : 'transparent',
              color: activeTab === 'telemetry' ? '#000000' : '#fbbf24',
              border: '1px solid #ff8800',
              padding: '2px 6px',
              borderRadius: '3px',
              fontSize: '9px',
              fontWeight: '600',
              cursor: 'pointer',
              transition: 'all 0.2s'
            }}
          >
            TELEMETRY
          </button>
          <button
            onClick={() => setActiveTab('goap')}
            onDoubleClick={() => window.open('https://goal.ruv.io/', '_blank', 'noopener,noreferrer')}
            style={{
              background: activeTab === 'goap' ? '#ff8800' : 'transparent',
              color: activeTab === 'goap' ? '#000000' : '#fbbf24',
              border: '1px solid #ff8800',
              padding: '2px 6px',
              borderRadius: '3px',
              fontSize: '9px',
              fontWeight: '600',
              cursor: 'pointer',
              transition: 'all 0.2s'
            }}
          >
            GOAP
          </button>
        </div>
        <span style={{
          width: '8px',
          height: '8px',
          borderRadius: '50%',
          background: isConnected ? '#00ff00' : '#ff0000',
          boxShadow: isConnected ? '0 0 4px #00ff00' : '0 0 4px #ff0000'
        }} />
      </div>

      {activeTab === 'telemetry' ? (
        <div
          ref={streamRef}
          style={{
            background: '#ff8800',
            border: '2px solid #cc6600',
            borderRadius: '4px',
            padding: '8px',
            height: '200px',
            overflowY: 'auto',
            fontFamily: 'DSEG7Classic, monospace',
            fontSize: '10px',
            lineHeight: '1.4',
            color: '#000000',
            boxShadow: 'inset 0 2px 4px rgba(0,0,0,0.3)'
          }}
        >
          {messages.length === 0 ? (
            <div style={{
              textAlign: 'center',
              padding: '20px',
              color: 'rgba(0,0,0,0.5)',
              fontFamily: 'monospace',
              fontSize: '9px'
            }}>
              WAITING FOR TELEMETRY...
            </div>
          ) : (
            messages.map((msg, idx) => (
              <div key={idx} style={{
                marginBottom: '4px',
                padding: '2px 4px',
                background: 'rgba(0,0,0,0.1)',
                borderRadius: '2px',
                display: 'flex',
                gap: '8px',
                fontSize: '9px'
              }}>
                <span style={{
                  color: getLevelColor(msg.level),
                  fontWeight: 'bold',
                  minWidth: '60px'
                }}>
                  {formatTime(msg.timestamp)}
                </span>
                <span style={{
                  color: '#000000',
                  fontWeight: 'bold',
                  minWidth: '80px',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap'
                }}>
                  {msg.agentId.substring(0, 12)}
                </span>
                <span style={{
                  color: '#000000',
                  flex: 1,
                  fontFamily: 'DSEG7Classic, monospace'
                }}>
                  {msg.message}
                </span>
              </div>
            ))
          )}
        </div>
      ) : (
        <div style={{
          background: '#1a1a1a',
          border: '2px solid #ff8800',
          borderRadius: '4px',
          padding: '4px',
          height: '200px',
          overflowY: 'auto',
          fontFamily: 'monospace',
          fontSize: '9px',
          color: '#ff8800',
          boxShadow: 'inset 0 2px 4px rgba(0,0,0,0.3)'
        }}>
          <div id="goap-widget-container"></div>
          <style>{`
            #goap-widget-container * {
              font-size: 9px !important;
            }
            #goap-widget-container {
              max-width: 100%;
              margin: 0;
            }
          `}</style>
        </div>
      )}

      <style>{`
        @font-face {
          font-family: 'DSEG7Classic';
          src: url('/fonts/DSEG7Classic-Bold.woff2') format('woff2');
          font-weight: bold;
          font-style: normal;
        }

        @font-face {
          font-family: 'DSEG7Classic';
          src: url('/fonts/DSEG7Classic-Regular.woff2') format('woff2');
          font-weight: normal;
          font-style: normal;
        }
      `}</style>
    </div>
  );
};
