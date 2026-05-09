import React, { useState, useEffect } from 'react';
import { agentTelemetry } from './AgentTelemetry';

interface DebugOverlayProps {
  visible: boolean;
  onToggle: () => void;
}

export const DebugOverlay: React.FC<DebugOverlayProps> = ({ visible, onToggle }) => {
  const [debugData, setDebugData] = useState(agentTelemetry.getDebugOverlayData());
  const [selectedTab, setSelectedTab] = useState<'metrics' | 'agents' | 'websocket' | 'threejs'>('metrics');

  useEffect(() => {
    if (visible) {
      const interval = setInterval(() => {
        setDebugData(agentTelemetry.getDebugOverlayData());
      }, 1000);
      return () => clearInterval(interval);
    }
  }, [visible]);

  if (!visible) {
    return (
      <div style={{
        position: 'fixed',
        top: '10px',
        right: '10px',
        zIndex: 10000,
        backgroundColor: 'rgba(0, 0, 0, 0.8)',
        color: 'white',
        padding: '5px 10px',
        borderRadius: '3px',
        fontSize: '12px',
        cursor: 'pointer'
      }} onClick={onToggle}>
        📊 Debug
      </div>
    );
  }

  return (
    <div style={{
      position: 'fixed',
      top: '10px',
      right: '10px',
      width: '400px',
      maxHeight: '80vh',
      backgroundColor: 'rgba(0, 0, 0, 0.95)',
      color: 'white',
      borderRadius: '8px',
      zIndex: 10000,
      overflow: 'hidden',
      fontFamily: 'monospace',
      fontSize: '11px',
      border: '1px solid rgba(255, 255, 255, 0.2)'
    }}>
      {}
      <div style={{
        padding: '10px',
        borderBottom: '1px solid rgba(255, 255, 255, 0.2)',
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        backgroundColor: 'rgba(0, 0, 0, 0.5)'
      }}>
        <span style={{ fontWeight: 'bold' }}>🔍 Agent Telemetry Debug</span>
        <button
          onClick={onToggle}
          style={{
            background: 'none',
            border: 'none',
            color: 'white',
            cursor: 'pointer',
            fontSize: '16px'
          }}
        >
          ✖
        </button>
      </div>

      {}
      <div style={{
        display: 'flex',
        backgroundColor: 'rgba(0, 0, 0, 0.3)',
        borderBottom: '1px solid rgba(255, 255, 255, 0.2)'
      }}>
        {[
          { key: 'metrics', label: '📊 Metrics' },
          { key: 'agents', label: '🤖 Agents' },
          { key: 'websocket', label: '📡 WebSocket' },
          { key: 'threejs', label: '🎮 Three.js' }
        ].map(tab => (
          <button
            key={tab.key}
            onClick={() => setSelectedTab(tab.key as typeof selectedTab)}
            style={{
              flex: 1,
              padding: '8px 4px',
              background: selectedTab === tab.key ? 'rgba(255, 255, 255, 0.2)' : 'none',
              border: 'none',
              color: 'white',
              cursor: 'pointer',
              fontSize: '10px'
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {}
      <div style={{
        padding: '10px',
        maxHeight: 'calc(80vh - 120px)',
        overflow: 'auto'
      }}>
        {selectedTab === 'metrics' && (
          <div>
            <div><strong>Session:</strong> {debugData.sessionId.slice(-8)}</div>
            <div><strong>Agent Spawns:</strong> {debugData.metrics.agentSpawns}</div>
            <div><strong>WebSocket Messages:</strong> {debugData.metrics.webSocketMessages}</div>
            <div><strong>Three.js Operations:</strong> {debugData.metrics.threeJSOperations}</div>
            <div><strong>Render Cycles:</strong> {debugData.metrics.renderCycles}</div>
            <div><strong>Avg Frame Time:</strong> {debugData.metrics.averageFrameTime.toFixed(2)}ms</div>
            {debugData.metrics.memoryUsage && (
              <div><strong>Memory:</strong> {(debugData.metrics.memoryUsage / 1024 / 1024).toFixed(1)}MB</div>
            )}
            <div><strong>Errors:</strong> {debugData.metrics.errorCount}</div>

            <div style={{ marginTop: '10px', fontSize: '10px' }}>
              <strong>Recent Frame Times:</strong>
              <div style={{ backgroundColor: 'rgba(255, 255, 255, 0.1)', padding: '5px', marginTop: '3px' }}>
                {debugData.recentFrameTimes.map((time, i) => (
                  <span key={i} style={{
                    color: time > 16.67 ? '#ff6b6b' : time > 8.33 ? '#ffd93d' : '#6bcf7f',
                    marginRight: '5px'
                  }}>
                    {time.toFixed(1)}
                  </span>
                ))}
              </div>
            </div>
          </div>
        )}

        {selectedTab === 'agents' && (
          <div>
            <div style={{ marginBottom: '10px' }}>
              <strong>Recent Agent Activity:</strong>
            </div>
            {debugData.agentTelemetry.map((entry, i) => (
              <div key={i} style={{
                marginBottom: '8px',
                padding: '5px',
                backgroundColor: 'rgba(255, 255, 255, 0.05)',
                borderRadius: '3px'
              }}>
                <div style={{ color: '#4CAF50' }}>{entry.agentType}:{entry.agentId}</div>
                <div>{entry.action}</div>
                {entry.position && (
                  <div style={{ fontSize: '9px', color: '#999' }}>
                    pos: ({entry.position.x.toFixed(2)}, {entry.position.y.toFixed(2)}, {entry.position.z.toFixed(2)})
                  </div>
                )}
                <div style={{ fontSize: '9px', color: '#999' }}>
                  {new Date(entry.timestamp).toLocaleTimeString()}
                </div>
              </div>
            ))}
          </div>
        )}

        {selectedTab === 'websocket' && (
          <div>
            <div style={{ marginBottom: '10px' }}>
              <strong>Recent WebSocket Activity:</strong>
            </div>
            {debugData.webSocketTelemetry.map((entry, i) => (
              <div key={i} style={{
                marginBottom: '8px',
                padding: '5px',
                backgroundColor: 'rgba(255, 255, 255, 0.05)',
                borderRadius: '3px'
              }}>
                <div style={{
                  color: entry.direction === 'incoming' ? '#2196F3' : '#FF9800',
                  display: 'flex',
                  justifyContent: 'space-between'
                }}>
                  <span>{entry.direction === 'incoming' ? '📥' : '📤'} {entry.messageType}</span>
                  {entry.size && <span>{entry.size}B</span>}
                </div>
                <div style={{ fontSize: '9px', color: '#999' }}>
                  {new Date(entry.timestamp).toLocaleTimeString()}
                </div>
              </div>
            ))}
          </div>
        )}

        {selectedTab === 'threejs' && (
          <div>
            <div style={{ marginBottom: '10px' }}>
              <strong>Recent Three.js Operations:</strong>
            </div>
            {debugData.threeJSTelemetry.map((entry, i) => (
              <div key={i} style={{
                marginBottom: '8px',
                padding: '5px',
                backgroundColor: 'rgba(255, 255, 255, 0.05)',
                borderRadius: '3px'
              }}>
                <div style={{ color: '#9C27B0' }}>{entry.objectId}</div>
                <div>{entry.action.replace(/_/g, ' ')}</div>
                {entry.position && (
                  <div style={{ fontSize: '9px', color: '#999' }}>
                    pos: ({entry.position.x.toFixed(2)}, {entry.position.y.toFixed(2)}, {entry.position.z.toFixed(2)})
                  </div>
                )}
                <div style={{ fontSize: '9px', color: '#999' }}>
                  {new Date(entry.timestamp).toLocaleTimeString()}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {}
      <div style={{
        padding: '10px',
        borderTop: '1px solid rgba(255, 255, 255, 0.2)',
        backgroundColor: 'rgba(0, 0, 0, 0.3)',
        display: 'flex',
        gap: '10px',
        fontSize: '10px'
      }}>
        <button
          onClick={() => {
            (agentTelemetry as unknown as { uploadTelemetry?: () => void }).uploadTelemetry?.() ?? agentTelemetry.fetchAgentTelemetry();
          }}
          style={{
            padding: '5px 10px',
            backgroundColor: '#4CAF50',
            border: 'none',
            color: 'white',
            borderRadius: '3px',
            cursor: 'pointer',
            fontSize: '10px'
          }}
        >
          📤 Upload
        </button>
        <button
          onClick={() => {
            const data = agentTelemetry.getDebugOverlayData();
            navigator.clipboard.writeText(JSON.stringify(data, null, 2));
          }}
          style={{
            padding: '5px 10px',
            backgroundColor: '#2196F3',
            border: 'none',
            color: 'white',
            borderRadius: '3px',
            cursor: 'pointer',
            fontSize: '10px'
          }}
        >
          📋 Copy
        </button>
        <button
          onClick={() => {
            window.open('data:text/json;charset=utf-8,' + encodeURIComponent(JSON.stringify(agentTelemetry.getDebugOverlayData(), null, 2)), '_blank', 'noopener,noreferrer');
          }}
          style={{
            padding: '5px 10px',
            backgroundColor: '#FF9800',
            border: 'none',
            color: 'white',
            borderRadius: '3px',
            cursor: 'pointer',
            fontSize: '10px'
          }}
        >
          💾 Export
        </button>
      </div>
    </div>
  );
};


export function useDebugOverlay() {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      
      if (event.ctrlKey && event.shiftKey && event.key === 'D') {
        event.preventDefault();
        setVisible(prev => !prev);
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  const toggle = () => setVisible(prev => !prev);

  return { visible, toggle };
}