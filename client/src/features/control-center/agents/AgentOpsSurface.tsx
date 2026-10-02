// D2 / D8 steering + observability surface (PRD-023 WP-3).
//
// Mounts `AgentDetailPanel` (per-agent steering: submit-task / interrupt) behind
// a node selection, alongside the `SwarmObservabilityPanel` aggregate (D8). The
// panel opens when an agent node is selected (`visionclaw:node-selected`
// resolves to a known agent), and an always-present ambient pill toggles it open
// too so the surface is reachable when agent-node raycasting is not wired.
//
// This closes the WP-3 falsification: `AgentDetailPanel` is mounted (not merely
// exported), and its submit-task / interrupt controls invoke the live routes
// from a mounted panel (each fires `CANARY-VC-D2-STEER` server-side).

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Bot } from 'lucide-react';
import { useBotsDataOptional } from '../../bots/contexts/BotsDataContext';
import { AgentDetailPanel } from '../../bots/components/AgentDetailPanel';
import { SwarmObservabilityPanel } from '../../bots/components/SwarmObservabilityPanel';
import { ChainPaymentsPanel } from '../../bots/chain/ChainPaymentsPanel';
import { resolveSelectedAgentId } from '../../bots/agentSelection';
import { GlassPanel } from '../primitives/GlassPanel';

/** Dock/other surfaces can summon the AgentOps panel with this event. */
export const OPEN_AGENT_OPS_EVENT = 'visionclaw:open-agent-ops';

export const AgentOpsSurface: React.FC = () => {
  const botsData = useBotsDataOptional()?.botsData ?? null;
  const [open, setOpen] = useState(false);
  const [selectedAgentId, setSelectedAgentId] = useState<string | undefined>(undefined);
  const agents = botsData?.agents;

  // Open behind an agent-node selection.
  useEffect(() => {
    const onSelected = (event: Event) => {
      const detail = (event as CustomEvent).detail as
        | { nodeId?: string; label?: string; metadata?: Record<string, unknown> | null }
        | null;
      const agentId = resolveSelectedAgentId(detail, agents);
      if (agentId) {
        setSelectedAgentId(agentId);
        setOpen(true);
      }
    };
    window.addEventListener('visionclaw:node-selected', onSelected);
    return () => window.removeEventListener('visionclaw:node-selected', onSelected);
  }, [agents]);

  // Open on the explicit summon event (the dock affordance / ambient pill).
  useEffect(() => {
    const onOpen = () => setOpen(true);
    window.addEventListener(OPEN_AGENT_OPS_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_AGENT_OPS_EVENT, onOpen);
  }, []);

  const agentCount = agents?.length ?? 0;
  const handleAgentSelect = useCallback((id: string) => setSelectedAgentId(id), []);

  return (
    <div className="fixed bottom-6 left-4 z-40 flex flex-col items-start gap-2" style={{ pointerEvents: 'auto' }}>
      {open && (
        <GlassPanel
          elevation="overlay"
          data-testid="agent-ops-panel"
          role="region"
          aria-label="Agent operations"
          className="w-80 max-h-[70vh] overflow-y-auto p-3 text-foreground"
        >
          <div className="flex items-center justify-between mb-2">
            <span className="text-sm font-semibold">Agent Ops</span>
            <button
              type="button"
              aria-label="Close agent operations"
              onClick={() => setOpen(false)}
              className="text-xs text-muted-foreground hover:text-foreground"
            >
              ✕
            </button>
          </div>

          <SwarmObservabilityPanel className="mb-3" />

          {/* S5: sidechain payments between agents, with mirror links. */}
          <ChainPaymentsPanel className="mb-3 pt-2 border-t border-white/10" />

          <div className="pt-2 border-t border-white/10">
            <AgentDetailPanel
              selectedAgentId={selectedAgentId}
              onAgentSelect={handleAgentSelect}
            />
          </div>
        </GlassPanel>
      )}

      <button
        type="button"
        data-testid="agent-ops-toggle"
        aria-label="Agent operations"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="cc-glass flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Bot size={13} aria-hidden="true" />
        Agents
        <span className="cc-value-readout">{agentCount}</span>
      </button>
    </div>
  );
};

AgentOpsSurface.displayName = 'AgentOpsSurface';

export default AgentOpsSurface;
