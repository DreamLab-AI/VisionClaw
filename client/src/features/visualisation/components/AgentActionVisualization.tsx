/**
 * AgentActionVisualization
 *
 * Top-level component integrating agent action events with the 3D visualization.
 * Automatically adapts to VR mode (Quest 3) with simplified geometry.
 *
 * Usage:
 *   <AgentActionVisualization />
 *
 * Add to your scene alongside AgentNodesLayer for rich agent telemetrics.
 */

import React, { useMemo } from 'react';
import { useQuest3Integration } from '@/hooks/useQuest3Integration';
import { useSettingsStore } from '@/store/settingsStore';
import { useAgentActionVisualization } from '../hooks/useAgentActionVisualization';
import { ActionConnectionsLayer, ActionConnectionsStats } from './ActionConnectionsLayer';

export interface AgentActionVisualizationProps {
  /** Show connection statistics overlay (debug) */
  showStats?: boolean;
  /** Override VR mode detection */
  forceVrMode?: boolean;
  /** Maximum concurrent connections */
  maxConnections?: number;
  /** Base animation duration (ms) */
  baseDuration?: number;
  /** Enable debug logging */
  debug?: boolean;
}

export const AgentActionVisualization: React.FC<AgentActionVisualizationProps> = ({
  showStats = false,
  forceVrMode,
  maxConnections = 50,
  baseDuration = 500,
  debug = false,
}) => {
  // Detect VR/Quest 3 mode
  const { isQuest3Detected } = useQuest3Integration({ enableAutoStart: false });
  const vrMode = forceVrMode ?? isQuest3Detected;

  // Get settings for enabling visualization (narrow selector to avoid re-render cascade)
  const enabled = useSettingsStore(
    s => ((s.settings as any)?.agents?.visualization?.show_action_connections as boolean | undefined) ?? true
  );

  // Adapt parameters for VR
  const adaptedParams = useMemo(() => ({
    maxConnections: vrMode ? Math.min(maxConnections, 25) : maxConnections,
    baseDuration: vrMode ? Math.max(baseDuration, 400) : baseDuration, // Slightly longer for VR readability
  }), [vrMode, maxConnections, baseDuration]);

  // Initialize action visualization hook
  const { connections, activeCount } = useAgentActionVisualization({
    enabled,
    maxConnections: adaptedParams.maxConnections,
    baseDuration: adaptedParams.baseDuration,
    vrMode,
    debug,
  });

  // Opacity reduction at high connection counts (performance)
  const opacity = useMemo(() => {
    if (activeCount > 40) return 0.6;
    if (activeCount > 30) return 0.8;
    return 1.0;
  }, [activeCount]);

  if (!enabled) return null;

  return (
    <>
      <ActionConnectionsLayer
        connections={connections}
        vrMode={vrMode}
        opacity={opacity}
        lineWidth={vrMode ? 1 : 2}
      />
      {showStats && <ActionConnectionsStats connections={connections} />}
    </>
  );
};

/**
 * Lightweight version for embedding in XR-only scenes.
 * Uses aggressive VR optimizations.
 */
export const AgentActionVisualizationXR: React.FC<Omit<AgentActionVisualizationProps, 'forceVrMode'>> = (props) => (
  <AgentActionVisualization {...props} forceVrMode={true} maxConnections={20} />
);

export default AgentActionVisualization;
