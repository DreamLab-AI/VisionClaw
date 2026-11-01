/**
 * AgentOverlay.tsx — the labels, trails, collaboration edges and payment labels of the agents
 * (ADR-2142 / ADR-2135). Replaces BotsVisualization/BotsNode/BotsEdges.
 *
 * It holds no agent position. Agents are not graph nodes: the registry (`agentTargetStore`) says which agents
 * exist, the marker pass says where each is drawn, and every part here reads that map. `/bots/data` (the
 * roster, via `BotsDataContext`) supplies names, types and the collaboration and payment edges, joined to
 * the registry by wire id.
 */
import React, { useMemo } from 'react';
import { useAgentTargetStore } from '../../../store/agentTargetStore';
import { useSettingsStore } from '../../../store/settingsStore';
import { useBotsDataOptional } from '../contexts/BotsDataContext';
import { agentMarkerColour, agentWireId } from '../virtualAgents';
import { getVisionClawColors } from './BotsShared';
import { AgentNameplate } from './AgentNameplate';
import { AgentTrail, TRAIL_DEFAULT_LENGTH } from './AgentTrail';
import { AgentCollabEdges } from './AgentCollabEdges';
import { ChainPaymentLabels } from '../chain/ChainPaymentLabels';
import { indexAgentsByWire } from '../agentSelection';
import type { BotsAgent, BotsEdge } from '../types/BotsTypes';

export const AgentOverlay: React.FC = () => {
  const settings = useSettingsStore(s => s.settings);
  const botsData = useBotsDataOptional()?.botsData ?? null;
  // The registry's agent ids, as a string so a keep-alive that changes nothing does not re-render.
  const idKey = useAgentTargetStore(s => [...s.seenAt.keys()].sort((a, b) => a - b).join(','));
  const ids = useMemo(() => (idKey ? idKey.split(',').map(Number) : []), [idKey]);

  const colours = useMemo(() => getVisionClawColors(settings as unknown as Record<string, unknown> | undefined), [settings]);
  const agentVisuals = settings?.visualisation?.graphTypeVisuals?.agent;
  const showTrails = agentVisuals?.showTrails ?? true;
  const trailLength = agentVisuals?.trailLength ?? TRAIL_DEFAULT_LENGTH;

  const byWire = useMemo(() => indexAgentsByWire(botsData?.agents), [botsData?.agents]);
  const agentMap = useMemo(() => new Map<string, BotsAgent>((botsData?.agents ?? []).map(a => [a.id, a])), [botsData?.agents]);
  const edgeMap = useMemo(() => new Map<string, BotsEdge>((botsData?.edges ?? []).map(e => [e.id, e])), [botsData?.edges]);

  return (
    <group>
      <AgentCollabEdges edges={edgeMap} agents={agentMap} color={colours.edge} />
      <ChainPaymentLabels edges={edgeMap} />
      {ids.map((id) => {
        const record = byWire.get(agentWireId(id));
        const colour = agentMarkerColour(id, record?.type, colours.getAgentColor);
        return (
          <React.Fragment key={id}>
            <AgentNameplate wireId={id} record={record} color={colour} />
            {showTrails && <AgentTrail wireId={id} color={colour} length={trailLength} />}
          </React.Fragment>
        );
      })}
    </group>
  );
};

export default AgentOverlay;
