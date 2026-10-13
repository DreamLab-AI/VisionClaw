/**
 * StatusFlyout — the expanded body of the unified Status surface.
 *
 * Rebuilt in the modern control-centre glass language (GlassPanel + cc- tokens
 * + Tailwind), NOT by importing the legacy ControlPanel widgets. It absorbs
 * everything the old top-right cluster carried, reading its data sources
 * directly rather than via threaded props:
 *
 *   (a) Connection telemetry  — websocket / metadata / MCP status, live inbound
 *                                rate, graph-type populations, ontology rigour,
 *                                and the settings-sync telltale.
 *   (b) Agents summary         — count / links / tokens / active agents, with a
 *                                cross-link to open the Agent Ops surface.
 *   (c) SpacePilot             — connection state + device name + connect button,
 *                                and the support/secure-context guidance that
 *                                used to live in the standalone SpaceMouseStatus
 *                                banner (now folded in, so that banner dies).
 *   (d) Layout motion          — an honest morphing/settled readout (see the
 *                                server-nit note below).
 *
 * Mounts only while the surface is expanded, so its polling hooks
 * (useConstraintStats, inferred-edges refresh, the layout-motion sampler) never
 * run while the collapsed chip is at rest.
 */

import React, { useEffect, useMemo, useState } from 'react';
import {
  Activity, Wifi, Database, Server, Check, AlertCircle, Loader,
  Boxes, GitBranch, Bot, Sigma, Network, Zap, Link, Unlink,
  Puzzle, AlertTriangle, Info, Waves, ArrowUpRight,
} from 'lucide-react';
import { GlassPanel } from '../primitives/GlassPanel';
import { useBotsDataOptional } from '../../bots/contexts/BotsDataContext';
import { MultiAgentInitializationPrompt } from '../../bots/components';
import { botsWebSocketIntegration } from '../../bots/services/BotsWebSocketIntegration';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { useConstraintStats } from '../../ontology/hooks/useConstraintStats';
import { useInferredEdgesStore } from '../../ontology/store/useInferredEdgesStore';
import { useSettingsStore } from '../../../store/settingsStore';
import { graphDataManager, type GraphData } from '../../graph/managers/graphDataManager';
import { OPEN_AGENT_OPS_EVENT } from '../agents/AgentOpsSurface';
import { useLayoutMotion, type WebSocketStatus } from './useConnectionTelemetry';
import { usePhysicsState } from './usePhysicsState';
import { describePhysicsState, type PhysicsTone } from './physicsState';
import type { SpacePilotState } from './useSpacePilot';

type BotsSummary = {
  nodeCount: number;
  edgeCount: number;
  tokenCount: number;
  mcpConnected: boolean;
  dataSource: string;
  multiAgentMetrics?: { activeAgents: number };
};

export interface StatusFlyoutProps {
  websocketStatus: WebSocketStatus;
  spacePilot: SpacePilotState;
  onClose: () => void;
}

type Health = boolean | string;

function statusColor(v: Health): string {
  if (v === true || v === 'connected' || v === 'loaded') return '#22c55e';
  if (v === 'connecting' || v === 'loading') return '#f59e0b';
  return '#ef4444';
}

function StatusIcon({ v }: { v: Health }) {
  if (v === true || v === 'connected' || v === 'loaded') return <Check size={10} />;
  if (v === 'connecting' || v === 'loading') return <Loader size={10} className="animate-spin" />;
  return <AlertCircle size={10} />;
}

/** Compact connection-state tile: icon + label + state glyph. */
const StateTile: React.FC<{ icon: React.ReactNode; label: string; state: Health; testid?: string }> = ({
  icon, label, state, testid,
}) => (
  <div className="flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1" data-testid={testid}>
    <span style={{ color: statusColor(state) }} className="flex items-center">{icon}</span>
    <span className="cc-helper-text">{label}</span>
    <span className="ml-auto flex items-center" style={{ color: statusColor(state) }}>
      <StatusIcon v={state} />
    </span>
  </div>
);

/** Compact metric tile: icon + label + numeric value. */
const MetricTile: React.FC<{ icon: React.ReactNode; label: string; value: number | string; color: string }> = ({
  icon, label, value, color,
}) => (
  <div className="flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1">
    <span style={{ color }} className="flex items-center">{icon}</span>
    <span className="cc-helper-text">{label}</span>
    <span className="ml-auto font-semibold tabular-nums" style={{ color }}>
      {typeof value === 'number' ? value.toLocaleString() : value}
    </span>
  </div>
);

interface GraphTypeCounts { knowledge: number; ontology: number; agent: number }

/** Bucket nodes into graph types by carried classification (ported from the
 *  legacy SystemHealthIndicator — mirrors the renderer's own detection). */
function bucketNodeTypes(nodes: unknown[] | undefined): GraphTypeCounts {
  const counts: GraphTypeCounts = { knowledge: 0, ontology: 0, agent: 0 };
  if (!Array.isArray(nodes)) return counts;
  for (const raw of nodes) {
    const n = raw as { metadata?: Record<string, unknown>; owlClassIri?: unknown };
    const meta = n?.metadata ?? {};
    const type = String(meta.type ?? meta.nodeType ?? '').toLowerCase();
    const isAgent = !!meta.agentType || meta.tokenRate !== undefined || type.startsWith('agent') || type.startsWith('bot');
    const isOntology =
      type === 'owl_class' || type === 'ontology_node' || type.startsWith('owl_') ||
      !!(n?.owlClassIri || meta.owlClassIri || meta.class_iri) ||
      meta.hierarchyDepth !== undefined;
    if (isAgent) counts.agent++;
    else if (isOntology) counts.ontology++;
    else counts.knowledge++;
  }
  return counts;
}

export const StatusFlyout: React.FC<StatusFlyoutProps> = ({ websocketStatus, spacePilot, onClose }) => {
  // Live graph topology — seed synchronously, then track topology changes only
  // (position streams do not fire this listener, so it stays quiet at settle).
  const [graphData, setGraphData] = useState<GraphData | null>(() => graphDataManager.getLastGraphData());
  useEffect(() => graphDataManager.onGraphDataChange(setGraphData), []);

  const botsCtx = useBotsDataOptional();
  const botsData = botsCtx?.botsData ?? null;

  const { stats: constraintStats } = useConstraintStats(8000);
  const inferredCount = useInferredEdgesStore((s) => s.report.count);
  const refreshInferred = useInferredEdgesStore((s) => s.refresh);
  useEffect(() => { void refreshInferred(); }, [refreshInferred]);

  const { motion, ratePerSec, feedFresh } = useLayoutMotion(true);

  const nodeCount = graphData?.nodes?.length ?? 0;
  const metadataStatus = nodeCount > 0 ? 'loaded' : 'loading';
  const mcpStatus = botsData?.mcpConnected ? 'connected' : 'disconnected';

  const typeCounts = useMemo<GraphTypeCounts>(() => {
    const bucketed = bucketNodeTypes(graphData?.nodes);
    return { ...bucketed, agent: botsData?.nodeCount ?? bucketed.agent };
  }, [graphData, botsData?.nodeCount]);

  const totalNodes = typeCounts.knowledge + typeCounts.ontology + typeCounts.agent;
  const showOntology = typeCounts.ontology > 0 || constraintStats.axiomsProcessed > 0 || inferredCount > 0;
  const isFullyConnected = websocketStatus === 'connected' && nodeCount > 0 && feedFresh;

  const physics = describePhysicsState(usePhysicsState());
  const physicsColor: Record<PhysicsTone, string> = { moving: '#f59e0b', settled: '#22c55e', paused: '#60a5fa', unknown: '#ef4444' };
  const motionColor = motion === 'morphing' ? '#f59e0b' : motion === 'stale' ? '#ef4444' : '#22c55e';
  const motionLabel = motion === 'morphing' ? 'Morphing' : motion === 'stale' ? 'Feed stale' : 'Settled';

  return (
    <GlassPanel
      elevation="overlay"
      data-testid="status-flyout"
      role="region"
      aria-label="System status detail"
      className="w-72 max-h-[72vh] overflow-y-auto p-3 text-foreground"
    >
      <div className="mb-2 flex items-center gap-1.5">
        <Activity size={13} aria-hidden="true" className={isFullyConnected ? 'text-emerald-400' : 'text-amber-400'} />
        <span className="text-sm font-semibold">System status</span>
        <span
          className="ml-auto inline-block h-2 w-2 rounded-full"
          style={{ background: statusColor(websocketStatus), boxShadow: `0 0 6px ${statusColor(websocketStatus)}` }}
          aria-hidden="true"
        />
        <button
          type="button"
          aria-label="Close system status"
          onClick={onClose}
          className="text-xs text-muted-foreground hover:text-foreground"
        >
          ✕
        </button>
      </div>

      {/* (a) Connection telemetry */}
      <section data-testid="status-connection" className="mb-2">
        <div className="grid grid-cols-2 gap-1">
          <StateTile icon={<Wifi size={11} />} label="WS" state={websocketStatus} testid="status-ws" />
          <StateTile icon={<Database size={11} />} label="Meta" state={metadataStatus} testid="status-meta" />
          <StateTile icon={<Server size={11} />} label="MCP" state={mcpStatus} testid="status-mcp" />
          <div className="flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1">
            <Activity size={11} style={{ color: feedFresh ? '#22c55e' : '#ef4444' }} />
            <span className="cc-helper-text">Rate</span>
            <span className="ml-auto tabular-nums cc-value-readout">{ratePerSec.toFixed(0)}/s</span>
          </div>
        </div>
      </section>

      {/* Graph-type populations */}
      <div className="mb-1 flex items-center gap-1.5">
        <Network size={10} className="text-muted-foreground" />
        <span className="cc-subgroup-label">Graphs</span>
        <span className="ml-auto cc-helper-text">{totalNodes.toLocaleString()} total</span>
      </div>
      <div className="mb-2 grid grid-cols-3 gap-1">
        <MetricTile icon={<Boxes size={10} />} label="Know" value={typeCounts.knowledge} color="#66BB6A" />
        <MetricTile icon={<GitBranch size={10} />} label="Onto" value={typeCounts.ontology} color="#F2C14E" />
        <MetricTile icon={<Bot size={10} />} label="Agent" value={typeCounts.agent} color="#4FC3F7" />
      </div>

      {/* Ontology rigour — only once there is ontology data to report */}
      {showOntology && (
        <>
          <div className="mb-1 flex items-center gap-1.5">
            <Sigma size={10} style={{ color: 'rgba(242,193,78,0.7)' }} />
            <span className="cc-subgroup-label">Ontology</span>
            {(constraintStats.gpuFailureCount > 0 || constraintStats.cpuFallbackCount > 0) && (
              <span
                className="ml-auto flex items-center gap-1 text-amber-400"
                title={`GPU constraint failures: ${constraintStats.gpuFailureCount}, CPU fallbacks: ${constraintStats.cpuFallbackCount}`}
              >
                <AlertCircle size={9} />
                {constraintStats.gpuFailureCount + constraintStats.cpuFallbackCount}
              </span>
            )}
          </div>
          <div className="mb-2 grid grid-cols-2 gap-1">
            <MetricTile icon={<GitBranch size={9} />} label="Classes" value={typeCounts.ontology} color="#F2C14E" />
            <MetricTile icon={<Sigma size={9} />} label="Axioms" value={constraintStats.axiomsProcessed} color="#C9A227" />
            <MetricTile icon={<Network size={9} />} label="Inferred" value={inferredCount} color="#FBBF24" />
            <MetricTile
              icon={<Zap size={9} />}
              label="Forces"
              value={constraintStats.activeConstraints}
              color={constraintStats.activeConstraints > 0 ? '#22c55e' : '#ef4444'}
            />
          </div>
        </>
      )}

      {/* Physics — the GPU actor's own settlement telemetry (GET /api/graph/settlement),
          polled only while this flyout is open. "GPU not reporting" / "Unavailable" are
          shown as such; a fallback is never presented as a measurement. */}
      <div
        data-testid="status-physics"
        className="mb-1 flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1.5"
        title={physics.detail}
      >
        <Zap size={12} style={{ color: physicsColor[physics.tone] }} />
        <span className="cc-helper-text">Physics</span>
        <span className="cc-helper-text ml-1 truncate">{physics.detail}</span>
        <span className="ml-auto font-semibold" style={{ color: physicsColor[physics.tone] }}>{physics.label}</span>
      </div>

      {/* (d) Layout motion — inferred from real inbound position-frame throughput, so it
          also proves frames are arriving; the Physics row above is the server's own
          number. (The server's settlementState was once unreliable, which is why this
          exists; it now reports measured values, so both are kept.) */}
      <div
        data-testid="status-motion"
        className="mb-2 flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1.5"
      >
        <Waves size={12} style={{ color: motionColor }} />
        <span className="cc-helper-text">Layout</span>
        <span className="ml-auto font-semibold" style={{ color: motionColor }}>{motionLabel}</span>
      </div>

      {/* Settings-sync telltale (ported from the legacy indicator) */}
      <SettingsSyncToggle />

      {/* (b) Agents summary */}
      <AgentsSection botsData={botsData} updateBotsData={botsCtx?.updateBotsData} />

      {/* (c) SpacePilot — connection, device name, connect button, and the
          support/secure-context guidance folded in from SpaceMouseStatus. */}
      <SpacePilotSection spacePilot={spacePilot} />
    </GlassPanel>
  );
};

/**
 * Agents summary + the multi-agent lifecycle affordances ported from the
 * deleted BotsStatusPanel: Initialize (the only entry point to
 * MultiAgentInitializationPrompt) and Disconnect. Per-agent steering / New Task
 * live in the Agent Ops surface, reachable via the Manage link.
 */
const AgentsSection: React.FC<{
  botsData: BotsSummary | null;
  updateBotsData?: NonNullable<ReturnType<typeof useBotsDataOptional>>['updateBotsData'];
}> = ({ botsData, updateBotsData }) => {
  const [showInitPrompt, setShowInitPrompt] = useState(false);
  const nodeCount = botsData?.nodeCount ?? 0;

  const handleDisconnect = async () => {
    try {
      const response = await unifiedApiClient.post('/bots/disconnect-multi-agent');
      if (response.status >= 200 && response.status < 300) {
        botsWebSocketIntegration.clearAgents();
        updateBotsData?.({
          nodeCount: 0, edgeCount: 0, tokenCount: 0,
          mcpConnected: false, dataSource: 'disconnected',
          agents: [], edges: [],
        });
      }
    } catch {
      /* disconnect failed — leave the current state in place */
    }
  };

  return (
    <section data-testid="status-agents" className="mt-2 border-t border-border/40 pt-2">
      <div className="mb-1.5 flex items-center gap-1.5 text-amber-400">
        <Zap size={12} aria-hidden="true" />
        <span className="cc-rail-label font-semibold">
          VisionClaw{botsData?.dataSource ? ` (${botsData.dataSource.toUpperCase()})` : ''}
        </span>
        <button
          type="button"
          data-testid="status-agents-manage"
          aria-label="Open agent operations"
          onClick={() => window.dispatchEvent(new Event(OPEN_AGENT_OPS_EVENT))}
          className="ml-auto flex items-center gap-0.5 text-[10px] text-muted-foreground hover:text-foreground"
        >
          Manage <ArrowUpRight size={11} />
        </button>
      </div>

      {nodeCount === 0 ? (
        <div className="py-1 text-center">
          <div className="cc-helper-text mb-1.5">No active multi-agent</div>
          <button
            type="button"
            data-testid="status-agents-initialize"
            onClick={() => setShowInitPrompt(true)}
            className="rounded px-2.5 py-1 text-[10px] font-semibold text-white"
            style={{ background: 'linear-gradient(to right, #3b82f6, #2563eb)' }}
          >
            Initialize multi-agent
          </button>
        </div>
      ) : (
        <>
          <div className="grid grid-cols-3 gap-1">
            <MetricTile icon={<Bot size={10} />} label="Agents" value={nodeCount} color="#fbbf24" />
            <MetricTile icon={<Network size={10} />} label="Links" value={botsData?.edgeCount ?? 0} color="#fbbf24" />
            <MetricTile icon={<Activity size={10} />} label="Active" value={botsData?.multiAgentMetrics?.activeAgents ?? 0} color="#22c55e" />
          </div>
          {(botsData?.tokenCount ?? 0) > 0 && (
            <div className="mt-1 flex items-center gap-1.5 rounded bg-foreground/5 px-2 py-1">
              <Zap size={10} className="text-amber-500" />
              <span className="cc-helper-text">Tokens</span>
              <span className="ml-auto font-semibold tabular-nums text-amber-500">
                {(botsData?.tokenCount ?? 0).toLocaleString()}
              </span>
            </div>
          )}
          <button
            type="button"
            data-testid="status-agents-disconnect"
            onClick={handleDisconnect}
            className="mt-1 w-full rounded bg-red-500/15 px-2 py-1 text-[10px] font-semibold text-red-300 hover:bg-red-500/25"
          >
            Disconnect
          </button>
        </>
      )}

      {showInitPrompt && (
        <MultiAgentInitializationPrompt
          onClose={() => setShowInitPrompt(false)}
          onInitialized={() => setShowInitPrompt(false)}
        />
      )}
    </section>
  );
};

/** Settings-sync telltale — whether physics/analytics changes reach the server. */
const SettingsSyncToggle: React.FC = () => {
  const syncEnabled = useSettingsStore((s) => s.settingsSyncEnabled);
  const setSyncEnabled = useSettingsStore((s) => s.setSettingsSyncEnabled);
  return (
    <button
      type="button"
      data-testid="status-sync-toggle"
      aria-pressed={syncEnabled}
      onClick={() => setSyncEnabled(!syncEnabled)}
      title={syncEnabled
        ? 'Settings sync ON — your changes update the shared server state. Click for local-only.'
        : 'Settings sync OFF — changes are local to this browser session. Click to re-enable sync.'}
      className="flex w-full items-center gap-1.5 rounded px-2 py-1"
      style={{
        background: syncEnabled ? 'rgba(34,197,94,0.1)' : 'rgba(239,68,68,0.1)',
        border: `1px solid ${syncEnabled ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
      }}
    >
      {syncEnabled
        ? <Link size={11} style={{ color: '#22c55e' }} />
        : <Unlink size={11} style={{ color: '#ef4444' }} />}
      <span style={{ color: syncEnabled ? '#22c55e' : '#ef4444' }} className="text-[10px] font-medium">
        {syncEnabled ? 'Sync' : 'Local'}
      </span>
      <span
        className="ml-auto inline-block h-1.5 w-1.5 rounded-full"
        style={{ background: syncEnabled ? '#22c55e' : '#ef4444' }}
      />
    </button>
  );
};

const SpacePilotSection: React.FC<{ spacePilot: SpacePilotState }> = ({ spacePilot }) => {
  const { isSupported, isSecureContext, isLocalhost, connected, deviceName, connect } = spacePilot;

  return (
    <section data-testid="status-spacepilot" className="mt-2 border-t border-border/40 pt-2">
      <div className="mb-1 flex items-center gap-1.5">
        <Puzzle size={12} aria-hidden="true" />
        <span className="cc-rail-label font-semibold">SpacePilot</span>
        {connected && (
          <span className="ml-auto flex items-center gap-1 text-[10px] text-emerald-400">
            <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: '#22c55e', boxShadow: '0 0 4px rgba(34,197,94,0.6)' }} />
            Connected
          </span>
        )}
      </div>

      {connected ? (
        <div className="cc-helper-text truncate" title={deviceName}>
          {deviceName ? `Device: ${deviceName}` : 'Device connected'}
        </div>
      ) : !isSupported ? (
        <Guidance
          testid="status-spacepilot-guidance"
          tone="info"
          icon={<Info size={12} />}
          title="WebHID not supported"
          body="SpacePilot needs Chrome or Edge — this browser can't open HID devices."
        />
      ) : !isSecureContext ? (
        <Guidance
          testid="status-spacepilot-guidance"
          tone="warn"
          icon={<AlertTriangle size={12} />}
          title="Secure context required"
          body={`WebHID needs HTTPS${isLocalhost ? '' : ' or localhost'}. Use localhost or enable insecure origins in chrome://flags.`}
        />
      ) : (
        <div className="flex items-center gap-2">
          <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: '#f87171', boxShadow: '0 0 4px rgba(248,113,113,0.6)' }} />
          <button
            type="button"
            data-testid="status-spacepilot-connect"
            onClick={connect}
            className="rounded px-2.5 py-1 text-[10px] font-semibold text-white"
            style={{ background: 'linear-gradient(to right, #3b82f6, #2563eb)' }}
          >
            Connect
          </button>
        </div>
      )}
    </section>
  );
};

const Guidance: React.FC<{
  testid: string;
  tone: 'warn' | 'info';
  icon: React.ReactNode;
  title: string;
  body: string;
}> = ({ testid, tone, icon, title, body }) => (
  <div
    data-testid={testid}
    className="flex items-start gap-2 rounded px-2 py-1.5"
    style={{
      background: tone === 'warn' ? 'rgba(245,158,11,0.12)' : 'rgba(59,130,246,0.12)',
      border: `1px solid ${tone === 'warn' ? 'rgba(245,158,11,0.3)' : 'rgba(59,130,246,0.3)'}`,
      color: tone === 'warn' ? '#fcd34d' : '#93c5fd',
    }}
  >
    <span className="mt-0.5 flex-shrink-0">{icon}</span>
    <div className="flex-1">
      <div className="text-[10px] font-semibold">{title}</div>
      <div className="mt-0.5 text-[9px] opacity-80">{body}</div>
    </div>
  </div>
);

StatusFlyout.displayName = 'StatusFlyout';

export default StatusFlyout;
