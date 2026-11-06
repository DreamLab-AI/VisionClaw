import React, { useState, useRef, useEffect, useCallback } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { UNIFIED_SETTINGS_CONFIG } from '../../control-center/registry/commandInputCompat';
import { ALL_PATHS } from '../../control-center/registry/settingsRegistry';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { ALL_FIELDS } from '../../control-center/registry/settingsRegistry';
import { commandRegistry } from '../../command-palette/CommandRegistry';
import { planJevCommand, type JevReview } from '../../../services/jev/commandFlow';
import { applySettingWrites, physicsWrites, type SettingWrite } from '../../control-center/state/settingWrites';
import { MACROS_BY_ID } from '../../control-center/registry/macros';
import { DEFAULT_EDGES_SETTINGS, DEFAULT_PHYSICS_SETTINGS } from '../../../api/settings/defaults';

interface CommandInputProps {
  isCollapsed: boolean;
}

interface StatusLine {
  id: number;
  text: string;
  timestamp: number;
}

// ---------------------------------------------------------------------------
// Validation & Parsing
// ---------------------------------------------------------------------------

function validateCommand(cmd: string): { valid: boolean; message: string } {
  const lower = cmd.toLowerCase();

  // Security guard against code/SQL injection. Use word boundaries so natural
  // language ("executive", "important", "required", "drop down") is not wrongly
  // rejected — those must reach the settings-aware LLM intact.
  const blockedWords = ['exec', 'eval', 'require', 'import', 'drop', 'truncate', 'sudo', 'rm'];
  const wordRe = new RegExp(`\\b(${blockedWords.join('|')})\\b`, 'i');
  if (wordRe.test(lower) || lower.includes('fetch(')) {
    return { valid: false, message: 'This interface only accepts view and graph configuration commands.' };
  }

  // Beyond the security blocklist we accept freeform language: deterministic
  // keyword matches are handled locally, everything else is interpreted by the
  // settings-aware LLM, so a rigid keyword whitelist would only reject valid
  // natural-language requests.
  return { valid: true, message: '' };
}

interface SettingsAction {
  description: string;
  /**
   * Setting changes. Applied through the one client write path
   * (applySettingWrites, ADR-2189): store + autosave queue + layout side effect,
   * exactly as if the user had moved the control in the settings panel.
   */
  writes?: SettingWrite[];
  /** Non-setting server actions only (e.g. running a clustering job). */
  endpoint?: string;
  method?: string;
  body?: Record<string, unknown>;
  localAction?: () => void | Promise<void>;
  /** When set, processCommand routes the raw command to the settings-aware LLM
   *  in agentbox instead of running a deterministic local action. */
  isLlmFallback?: boolean;
}

// Flatten UNIFIED_SETTINGS_CONFIG into a compact metadata block the agentbox LLM
// can reason over: path :: label (type) range :: current value :: description.
// This is the single source of truth for settings descriptions, sent verbatim
// so the agent can map natural language onto concrete settings paths + ranges.
//
// Fields are joined with " :: ", NOT " | ". agentbox's task sanitiser rejects
// shell-pipe patterns: a "|" followed by a command-like word (e.g. "| Node",
// which it reads as a pipe into the `node` interpreter) trips its injection
// guard. Labels such as "Node Size"/"Node Color" sit right after a separator,
// so a pipe delimiter would be rejected with HTTP 500 "suspicious shell
// patterns". "::" has no shell meaning and clears the guard cleanly.
//
// agentbox /v1/tasks rejects task strings over 10000 chars. The server prompt
// wraps this context in ~1KB of instructions, so we keep the context under a
// budget: every path/label/range/current (the essential mapping data) is always
// included; descriptions are appended only while the budget allows.
const SETTINGS_CONTEXT_BUDGET = 8000;

function buildSettingsContext(): string {
  const ss = useSettingsStore.getState();
  const lines: string[] = [];
  let used = 0;
  for (const section of Object.values(UNIFIED_SETTINGS_CONFIG)) {
    const fields = (section as unknown as { fields?: Array<Record<string, unknown>> }).fields ?? [];
    for (const f of fields) {
      const path = f.path as string | undefined;
      if (!path) continue;
      let current: unknown;
      try { current = ss.get(path as never); } catch { current = undefined; }
      const min = f.min as number | undefined;
      const max = f.max as number | undefined;
      const step = f.step as number | undefined;
      const range = (min !== undefined || max !== undefined)
        ? ` range=[${min ?? '-inf'}..${max ?? 'inf'}${step !== undefined ? ` step ${step}` : ''}]`
        : '';
      const base = `${path} :: ${f.label} (${f.type})${range} :: current=${JSON.stringify(current)}`;
      const withDesc = f.description ? `${base} :: ${f.description}` : base;
      // Prefer the line with its description; if it would blow the budget, fall
      // back to the description-less line so the path is still addressable.
      let line = withDesc;
      if (used + line.length + 1 > SETTINGS_CONTEXT_BUDGET) {
        line = base;
        if (used + line.length + 1 > SETTINGS_CONTEXT_BUDGET) continue;
      }
      lines.push(line);
      used += line.length + 1;
    }
  }
  return lines.join('\n');
}

// Dispatch a natural-language settings command to the agentbox LLM via the
// existing CreateTask transport (POST /api/bots/settings-command). The agent
// receives the live settings + descriptions context and applies changes back
// through the existing /api/settings/* REST API.
async function dispatchToSettingsLLM(
  command: string,
  addStatus: (text: string) => void,
): Promise<void> {
  addStatus('Asking the settings assistant to interpret your request...');
  try {
    const res = await unifiedApiClient.post<{
      data?: { taskId?: string; message?: string };
      taskId?: string;
      message?: string;
    }>('/bots/settings-command', { command, settingsContext: buildSettingsContext() });
    const envelope = res.data ?? {};
    const payload = envelope.data ?? envelope;
    if (payload.taskId) {
      addStatus(`Settings assistant working (task ${String(payload.taskId).slice(0, 8)})...`);
    } else if (payload.message) {
      addStatus(payload.message);
    }
  } catch (e) {
    const status = (e as { status?: number } | null)?.status;
    addStatus(
      status
        ? `Settings assistant unavailable (HTTP ${status})`
        : `Settings assistant error: ${e instanceof Error ? e.message : 'unknown'}`,
    );
  }
}

/** Edge Opacity slider range (registry look.ts `edgeOpacity`): 0 to 0.3. */
const EDGE_OPACITY_MAX = 0.3;
const EDGE_OPACITY_DEFAULT = DEFAULT_EDGES_SETTINGS.opacity;

/**
 * Edge opacity a chat command asks for, in the Edge Opacity setting's own
 * units, or null when the command is not about edge transparency. A stated
 * value is read as a fraction or percentage of the slider's range;
 * "transparency"/"transparent" inverts it (70% transparent = 30% opaque).
 * Without a number, fainter words halve the default and stronger words raise it.
 */
export function edgeOpacityFromCommand(lower: string): number | null {
  if (!/\b(edges?|links?|lines?|connections?)\b/.test(lower)) return null;
  const transparencyWords = /transparen|fainter|faint|subtle|lighter|dimmer|see.?through/;
  const opacityWords = /opacit|opaque|visible|stronger|bolder|solid|darker/;
  if (!transparencyWords.test(lower) && !opacityWords.test(lower)) return null;
  const round = (v: number) => Math.round(Math.min(EDGE_OPACITY_MAX, Math.max(0, v)) * 1000) / 1000;
  const match = lower.match(/(\d+\.?\d*)\s*(%|percent)?/);
  if (match) {
    let v = parseFloat(match[1]);
    if (match[2] || v > 1) v = v / 100;
    v = Math.min(1, Math.max(0, v));
    const opacity = /transparen/.test(lower) ? 1 - v : v;
    return round(opacity * EDGE_OPACITY_MAX);
  }
  const stronger = /less transparen|more opaque|more visible|stronger|bolder|solid|darker/.test(lower);
  return round(stronger ? EDGE_OPACITY_DEFAULT * 1.6 : EDGE_OPACITY_DEFAULT * 0.5);
}

function parseCommandToActions(cmd: string): SettingsAction[] {
  const lower = cmd.toLowerCase();
  const actions: SettingsAction[] = [];

  // Cluster hulls
  if (lower.includes('hull') || lower.includes('cluster hull')) {
    if (lower.includes('hide') || lower.includes('disable') || lower.includes('off')) {
      actions.push({
        description: 'Disabling cluster hulls',
        writes: [{ path: 'visualisation.clusterHulls.enabled', value: false }],
      });
    } else {
      actions.push({
        description: 'Enabling cluster hulls',
        writes: [{ path: 'visualisation.clusterHulls.enabled', value: true }],
      });
    }
  }

  // Physics: repulsion. "separate"/"split" are NOT routed here — those move the
  // two graphs apart via the dual-graph disc projection below, not the repulsion
  // force constant.
  if (lower.includes('repul') || lower.includes('spread')) {
    const increase = lower.includes('increase') || lower.includes('more') || lower.includes('spread');
    // The Spread dial owns these numbers: the same transfer function the dock uses.
    const t = increase ? 0.9 : 0.15;
    actions.push({
      description: `Setting Spread to ${Math.round(t * 100)}%`,
      writes: MACROS_BY_ID.density.apply(t),
    });
  }

  // Disc shape (ADR-2135). The knowledge graph, the ontology and the memory cloud are
  // always apart at a fixed separation (not a setting); axisCompressionZ (1.0 = fully
  // 3D blobs .. 0.05 = flat discs) thins each graph into a disc facing the centre. This
  // acts on the display projection, NOT the force constants, so it is kept distinct
  // from the repulsion rule above. Handles e.g. "flatten the graphs" and
  // "reset the flattening".
  {
    const toZero = lower.includes('zero') || lower.includes('reset')
      || lower.includes('off') || lower.includes('remove') || lower.includes('un-');
    const discBody: Record<string, number> = {};

    if (lower.includes('flatten') || /\bflat\b/.test(lower) || lower.includes('facing disc')
      || lower.includes('co-planar') || lower.includes('coplanar')) {
      discBody.axisCompressionZ = toZero ? 1.0 : 0.05;
    } else if (lower.includes('unflatten') || lower.includes('un-flatten')
      || lower.includes('blob') || (lower.includes('3d') && lower.includes('graph'))) {
      discBody.axisCompressionZ = 1.0;
    }

    if (Object.keys(discBody).length > 0) {
      const parts: string[] = [];
      if ('axisCompressionZ' in discBody) parts.push(`flatten→${discBody.axisCompressionZ}`);
      actions.push({
        description: `Disc layout: ${parts.join(', ')}`,
        writes: physicsWrites(discBody),
      });
    }
  }

  // Physics: spring / attract / tight / compact
  if (lower.includes('spring') || lower.includes('attract') || lower.includes('tight') || lower.includes('compact')) {
    const increase = lower.includes('increase') || lower.includes('more') || lower.includes('tight') || lower.includes('compact');
    const value = increase ? 5.0 : 1.0;
    actions.push({
      description: `Setting spring strength to ${value}`,
      writes: physicsWrites({ springK: value }),
    });
  }

  // Physics: damping
  if (lower.includes('damp')) {
    const value = lower.includes('increase') || lower.includes('more') ? 0.8 : 0.3;
    actions.push({
      description: `Setting damping to ${value}`,
      writes: physicsWrites({ damping: value }),
    });
  }

  // Visibility: knowledge
  if (lower.includes('knowledge')) {
    const show = !lower.includes('hide');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} knowledge nodes`,
      writes: [{ path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.knowledge', value: show }],
    });
  }

  // Visibility: ontology
  if (lower.includes('ontology') && (lower.includes('show') || lower.includes('hide'))) {
    const show = !lower.includes('hide');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} ontology nodes`,
      writes: [{ path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.ontology', value: show }],
    });
  }

  // Quality filter
  if (lower.includes('quality') && (lower.match(/\d+\.?\d*/) || lower.includes('high'))) {
    const match = lower.match(/(\d+\.?\d*)/);
    const threshold = match ? parseFloat(match[1]) : 0.8;
    const normalized = threshold > 1 ? threshold / 100 : threshold;
    actions.push({
      description: `Filtering to quality >= ${normalized}`,
      writes: [
        { path: 'nodeFilter.enabled', value: true },
        { path: 'nodeFilter.filterByQuality', value: true },
        { path: 'nodeFilter.qualityThreshold', value: normalized },
      ],
    });
  }

  // Orphan / isolated node visibility (degree-0 spray)
  if (lower.includes('orphan') || lower.includes('isolated') || lower.includes('unconnected') || lower.includes('disconnected')) {
    const hide = !lower.includes('show');
    actions.push({
      description: `${hide ? 'Hiding' : 'Showing'} orphan (degree-0) nodes`,
      writes: [{ path: 'nodeFilter.minConnections', value: hide ? 1 : 0 }],
    });
  }

  // Edge opacity: "edge transparency 70%", "make the links fainter". Must come
  // before the node rule, which used to swallow every opacity request.
  const edgeOpacityValue = edgeOpacityFromCommand(lower);
  if (edgeOpacityValue !== null) {
    actions.push({
      description: `Setting edge opacity to ${edgeOpacityValue}`,
      writes: [{ path: 'visualisation.graphs.knowledge.edges.opacity', value: edgeOpacityValue }],
    });
  } else if (lower.includes('opacity') || lower.includes('transparent') || lower.includes('visible')) {
    // Node opacity
    const match = lower.match(/(\d+\.?\d*)/);
    let value = match ? parseFloat(match[1]) : 0.8;
    if (value > 1) value = value / 100;
    actions.push({
      description: `Setting node opacity to ${value}`,
      writes: [{ path: 'visualisation.graphs.knowledge.nodes.opacity', value }],
    });
  }

  // Labels
  if (lower.includes('label')) {
    const show = !lower.includes('hide') && !lower.includes('off');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} labels`,
      writes: [{ path: 'visualisation.graphs.knowledge.labels.enableLabels', value: show }],
    });
  }

  // Reset to defaults. Skipped when the command is specifically about disc
  // separation/flatten ("reset the separation and flattening") — that is handled
  // by the dual-graph disc rule above and must not also reset the force constants.
  if ((lower.includes('reset') || lower.includes('default'))
    && !lower.includes('separat') && !lower.includes('flatten') && !/\bflat\b/.test(lower)) {
    actions.push({
      description: 'Resetting forces to defaults',
      // Read from the shipped defaults, never restated here.
      writes: physicsWrites({
        repelK: DEFAULT_PHYSICS_SETTINGS.repelK,
        springK: DEFAULT_PHYSICS_SETTINGS.springK,
        damping: DEFAULT_PHYSICS_SETTINGS.damping,
        restLength: DEFAULT_PHYSICS_SETTINGS.restLength,
        maxVelocity: DEFAULT_PHYSICS_SETTINGS.maxVelocity,
      }),
    });
  }

  // Bloom / glow
  if (lower.includes('glow') || lower.includes('bloom')) {
    const enable = !lower.includes('off') && !lower.includes('disable');
    actions.push({
      description: `${enable ? 'Enabling' : 'Disabling'} emphasis glow`,
      writes: [
        { path: 'visualisation.bloom.enabled', value: enable },
        { path: 'visualisation.glow.enabled', value: enable },
      ],
    });
  }

  // Grouping — spatial K-means vs topological communities (unified run endpoint)
  if (lower.includes('kmeans') || lower.includes('louvain') || lower.includes('spectral')) {
    const isKmeans = lower.includes('kmeans');
    actions.push({
      description: isKmeans ? 'Running K-means grouping' : 'Running community grouping',
      endpoint: '/api/analytics/clustering/run',
      method: 'POST',
      body: isKmeans
        ? { method: 'kmeans', params: { numClusters: 6 } }
        : { method: 'communities', params: { resolution: 1.0 } },
    });
  }

  // Semantic clusters — "create semantic clusters", "group by topic", "cluster"
  if ((lower.includes('semantic') && lower.includes('cluster')) || lower.includes('group by')) {
    actions.push({
      description: 'Enabling community cohesion + Leiden detection',
      writes: physicsWrites({ clusterStrength: 0.005, clusteringAlgorithm: 'leiden', clusteringResolution: 1.0, clusteringIterations: 50 }),
    });
    actions.push({
      description: 'Enabling cluster visualization',
      // (The retired qualityGates.semanticForces write is gone: ADR-2140 moved it into physics.semantic.)
      writes: [
        // ADR-2189: "show clusters" is Explore › Colour by = Cluster (the old toggle
        // coloured nothing; the colour scheme's cluster branch skips knowledge nodes).
        { path: 'visualisation.graphView.explore.colorBy', value: 'cluster' },
        { path: 'visualisation.clusterHulls.enabled', value: true },
      ],
    });
  }

  // "less overwhelming labels", "fewer labels", "reduce label clutter"
  if (lower.includes('overwhelm') || lower.includes('clutter') || lower.includes('fewer label') || lower.includes('less label')) {
    actions.push({
      description: 'Fewer labels: lowering the overview label budget',
      // The Labels dial owns the number (ADR-2186 budget).
      writes: [...MACROS_BY_ID.focus.apply(0.15), { path: 'visualisation.graphs.knowledge.labels.showMetadata', value: false }],
    });
  }

  // "more labels", "bigger labels", "I can't read the labels"
  if ((lower.includes('more') || lower.includes('bigger') || lower.includes('read')) && lower.includes('label')) {
    actions.push({
      description: 'More labels: raising the overview label budget',
      writes: [...MACROS_BY_ID.focus.apply(0.8), { path: 'visualisation.graphs.knowledge.labels.enableLabels', value: true }],
    });
  }

  // "only knowledge and agents" / "only show knowledge graph and agents"
  if (lower.includes('only') && (lower.includes('knowledge') || lower.includes('agent'))) {
    const showKnowledge = lower.includes('knowledge');
    const showAgent = lower.includes('agent');
    const showOntology = lower.includes('ontology');
    actions.push({
      description: `Showing: ${[showKnowledge && 'knowledge', showOntology && 'ontology', showAgent && 'agents'].filter(Boolean).join(', ')}`,
      writes: [
        { path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.knowledge', value: showKnowledge },
        { path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.ontology', value: showOntology },
        { path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.agent', value: showAgent },
      ],
    });
  }

  // "layout force directed" / "layout DAG" / "layout radial"
  if (lower.includes('layout') && (lower.includes('dag') || lower.includes('radial') || lower.includes('hierarchy') || lower.includes('tree'))) {
    const mode = lower.includes('radial') ? 'radial' : lower.includes('left') ? 'dagLeftRight' : 'hierarchical';
    actions.push({
      description: `Switching to ${mode} layout`,
      // The layout mode is the stored physics.layoutMode (ADR-2140); the write path
      // also asks the server for the transition, as the Layout › Mode select does.
      writes: [{ path: 'visualisation.graphs.knowledge.physics.layoutMode', value: mode }],
    });
  }

  // "focus on X" / "find X" — zoom to node by label search
  if (lower.includes('focus') || lower.includes('find') || lower.includes('search')) {
    const searchTerm = cmd.replace(/^(focus|find|search)\s*(on|for)?\s*/i, '').trim();
    if (searchTerm.length > 1) {
      actions.push({
        description: `Searching for "${searchTerm}" — use the graph to navigate`,
        localAction: () => {
          // Dispatch a custom event that the graph can listen for
          window.dispatchEvent(new CustomEvent('visionclaw:search', { detail: { query: searchTerm } }));
        },
      });
    }
  }

  // Save/Load graph views to Solid Pod
  if (lower.includes('save') && lower.includes('view')) {
    const viewName = cmd.replace(/^save\s+view\s*(as\s+)?/i, '').trim() || 'default';
    actions.push({
      description: `Saving graph view "${viewName}" to your Pod`,
      localAction: async () => {
        try {
          const solidPod = (await import('../../../services/SolidPodService')).default;
          const ss = useSettingsStore.getState();
          const settings = ss?.settings;
          const viewData = {
            filters: settings?.nodeFilter,
            physics: {
              repelK: settings?.visualisation?.graphs?.knowledge?.physics?.repelK,
              springK: settings?.visualisation?.graphs?.knowledge?.physics?.springK,
              restLength: settings?.visualisation?.graphs?.knowledge?.physics?.restLength,
              centerGravityK: settings?.visualisation?.graphs?.knowledge?.physics?.centerGravityK,
              damping: settings?.visualisation?.graphs?.knowledge?.physics?.damping,
            },
            clusters: settings?.qualityGates,
            nodeTypeVisibility: settings?.visualisation?.graphs?.knowledge?.nodes?.nodeTypeVisibility,
          };
          await solidPod.saveGraphView(viewName, viewData);
        } catch (e) {
          console.error('Failed to save view:', e);
        }
      },
    });
  }

  if (lower.includes('load') && lower.includes('view')) {
    const viewName = cmd.replace(/^load\s+view\s*/i, '').trim() || 'default';
    actions.push({
      description: `Loading graph view "${viewName}" from your Pod`,
      localAction: async () => {
        try {
          const solidPod = (await import('../../../services/SolidPodService')).default;
          const view = await solidPod.loadGraphView(viewName);
          if (view) {
            const writes: SettingWrite[] = [];
            if (view.physics) {
              // Keep only the stored values: a saved view may carry unset (undefined) keys.
              const physics = Object.fromEntries(
                Object.entries(view.physics as Record<string, unknown>).filter(([, v]) => v !== undefined),
              );
              writes.push(...physicsWrites(physics));
            }
            if (view.nodeTypeVisibility) {
              writes.push({ path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility', value: view.nodeTypeVisibility });
            }
            applySettingWrites(writes);
          }
        } catch (e) {
          console.error('Failed to load view:', e);
        }
      },
    });
  }

  if (lower.includes('list') && lower.includes('view')) {
    actions.push({
      description: 'Listing saved graph views from your Pod',
      localAction: async () => {
        try {
          const solidPod = (await import('../../../services/SolidPodService')).default;
          const views = await solidPod.listGraphViews();
          if (views.length > 0) {
            window.dispatchEvent(new CustomEvent('visionclaw:status', {
              detail: { message: `Saved views: ${views.join(', ')}` }
            }));
          }
        } catch (e) {
          console.error('Failed to list views:', e);
        }
      },
    });
  }

  // "show agents" / "inject agents" / "show swarm" — inject mock claude-flow agents
  if ((lower.includes('inject') || lower.includes('show')) && (lower.includes('agent') || lower.includes('swarm'))) {
    // Don't conflict with visibility-only commands like "show agent nodes"
    if (!lower.includes('node') && !lower.includes('only')) {
      actions.push({
        description: 'Injecting mock claude-flow swarm agents into graph',
        localAction: async () => {
          try {
            const res = await unifiedApiClient.post<{ injected?: number }>('/bots/mock-agents', {
              agents: [
                { id: 'claude-opus', label: 'Claude Opus 4.6', type: 'coordinator', status: 'active' },
                { id: 'coder-1', label: 'Coder Agent', type: 'coder', status: 'active' },
                { id: 'reviewer-1', label: 'QE Reviewer', type: 'reviewer', status: 'thinking' },
                { id: 'researcher-1', label: 'Research Agent', type: 'researcher', status: 'active' },
                { id: 'memory-1', label: 'RuVector Memory', type: 'memory', status: 'idle' },
              ],
            });
            window.dispatchEvent(new CustomEvent('visionclaw:status', {
              detail: { message: `Injected ${res.data?.injected ?? 0} agents into graph` },
            }));
          } catch (e) {
            console.error('Failed to inject mock agents:', e);
          }
        },
      });
    }
  }

  // No deterministic match — hand the raw command to the settings-aware LLM.
  if (actions.length === 0) {
    actions.push({
      description: 'Interpreting with the settings assistant',
      isLlmFallback: true,
    });
  }

  return actions;
}

async function executeAction(action: SettingsAction): Promise<void> {
  if (action.writes?.length) {
    applySettingWrites(action.writes);
  }
  if (action.localAction) {
    await action.localAction();
  }
  if (action.endpoint) {
    // Route through unifiedApiClient so the auth interceptor injects credentials
    // (dev token / NIP-98). A raw fetch sends no auth and gets 401.
    // unifiedApiClient's baseURL is already '/api', so strip the leading '/api'.
    const path = action.endpoint.replace(/^\/api(?=\/)/, '');
    await unifiedApiClient.request(action.method || 'GET', path, action.body);
  }
}

// ---------------------------------------------------------------------------
// StatusText — retro green scanline fade
// ---------------------------------------------------------------------------

const StatusText: React.FC<{ text: string; timestamp: number }> = ({ text, timestamp }) => {
  const [opacity, setOpacity] = useState(1);

  useEffect(() => {
    const fadeStart = setTimeout(() => {
      const interval = setInterval(() => {
        setOpacity(prev => {
          const next = prev - 0.02;
          if (next <= 0) {
            clearInterval(interval);
            return 0;
          }
          return next;
        });
      }, 50);
      return () => clearInterval(interval);
    }, 2000);
    return () => clearTimeout(fadeStart);
  }, [timestamp]);

  if (opacity <= 0) return null;

  return (
    <div style={{
      color: `rgba(0, 255, 65, ${opacity})`,
      fontSize: '11px',
      fontFamily: '"Courier New", monospace',
      padding: '2px 4px',
      textShadow: `0 0 4px rgba(0, 255, 65, ${opacity * 0.5})`,
      backgroundImage: opacity > 0.5
        ? 'repeating-linear-gradient(0deg, transparent, transparent 1px, rgba(0,0,0,0.1) 1px, rgba(0,0,0,0.1) 2px)'
        : 'none',
      transition: 'opacity 0.1s linear',
    }}>
      {text}
    </div>
  );
};

// ---------------------------------------------------------------------------
// CommandInput — main exported component
// ---------------------------------------------------------------------------

// Registry fields with their current values, for the Jev option's shortlist and
// patch assembly. Protected settings are filtered again in shortlistSettings.
function jevFields() {
  const ss = useSettingsStore.getState();
  return ALL_FIELDS.filter((f) => f.path).map((f) => {
    let current: unknown;
    try { current = ss.get(f.path as never); } catch { current = undefined; }
    return { ...f, current };
  });
}

export const CommandInput: React.FC<CommandInputProps> = ({ isCollapsed }) => {
  const [command, setCommand] = useState('');
  const [queue, setQueue] = useState<string[]>([]);
  const [isProcessing, setIsProcessing] = useState(false);
  const [statusLines, setStatusLines] = useState<StatusLine[]>([]);
  // Jev option (off by default): a suggestion awaiting the user's decision.
  const [jevReview, setJevReview] = useState<{ review: JevReview; command: string } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const statusIdRef = useRef(0);
  // One-time hydration guard: the LLM context (buildSettingsContext) reads
  // live values off the store, but most of the 168 paths sit outside the
  // ~30 ESSENTIAL_PATHS loaded at boot (design-spec.md §4.2). Load them all
  // the first time the user actually engages this input, not at app boot.
  const hasEnsuredAllSettingsRef = useRef(false);

  const handleInputFocus = useCallback(() => {
    if (hasEnsuredAllSettingsRef.current) return;
    hasEnsuredAllSettingsRef.current = true;
    void useSettingsStore.getState().ensureLoaded(ALL_PATHS);
  }, []);

  const addStatus = useCallback((text: string) => {
    const id = ++statusIdRef.current;
    setStatusLines(prev => [...prev.slice(-4), { id, text, timestamp: Date.now() }]);
  }, []);

  const processCommand = useCallback(async (cmd: string) => {
    setIsProcessing(true);
    addStatus(`Processing: ${cmd}`);

    try {
      const validationResult = validateCommand(cmd);
      if (!validationResult.valid) {
        addStatus(validationResult.message);
        setIsProcessing(false);
        return;
      }

      const actions = parseCommandToActions(cmd);

      let heldForReview = false;
      for (const action of actions) {
        addStatus(action.description);
        if (action.isLlmFallback) {
          // Jev option: only here, where the pre-change path would hand the raw
          // text to the settings assistant. null (flag off, no endpoint, error,
          // timeout, malformed, low confidence) ⇒ exactly the pre-change call.
          const review = await planJevCommand(cmd, jevFields());
          if (review) {
            setJevReview({ review, command: cmd });
            heldForReview = true;
          } else {
            await dispatchToSettingsLLM(cmd, addStatus);
          }
        } else {
          await executeAction(action);
        }
        await new Promise(r => setTimeout(r, 300));
      }

      addStatus(heldForReview ? 'Review the suggestion below' : 'Configuration applied');
    } catch (error) {
      addStatus(`Error: ${error instanceof Error ? error.message : 'Unknown error'}`);
    }

    setIsProcessing(false);
    setCommand('');
  }, [addStatus]);

  // Process queue when current command finishes
  useEffect(() => {
    if (!isProcessing && queue.length > 0) {
      const next = queue[0];
      setQueue(prev => prev.slice(1));
      processCommand(next);
    }
  }, [isProcessing, queue, processCommand]);

  const handleSubmit = useCallback((e: React.FormEvent) => {
    e.preventDefault();
    if (!command.trim()) return;

    if (isProcessing) {
      setQueue(prev => [...prev, command.trim()]);
      addStatus(`Queued: ${command.trim()}`);
      setCommand('');
    } else {
      processCommand(command.trim());
    }
  }, [command, isProcessing, addStatus, processCommand]);

  // Jev review actions. Applying a patch uses the existing settings store, which
  // syncs through the authenticated settings path; Jev itself writes nothing.
  const applyJevPatch = useCallback(() => {
    if (!jevReview || jevReview.review.kind !== 'patch') return;
    const { patch } = jevReview.review;
    try {
      applySettingWrites([{ path: patch.path, value: patch.proposed }]);
      addStatus(`Applied ${patch.label} = ${String(patch.proposed)}`);
    } catch (e) {
      addStatus(`Could not apply: ${e instanceof Error ? e.message : 'unknown error'}`);
    }
    setJevReview(null);
    setCommand('');
  }, [jevReview, addStatus]);

  const sendToAssistant = useCallback(async () => {
    if (!jevReview) return;
    const cmd = jevReview.command;
    setJevReview(null);
    await dispatchToSettingsLLM(cmd, addStatus);
    setCommand('');
  }, [jevReview, addStatus]);

  const runJevCommand = useCallback(async () => {
    if (!jevReview || jevReview.review.kind !== 'intent' || !jevReview.review.commandId) return;
    const cmd = commandRegistry.getCommand(jevReview.review.commandId);
    setJevReview(null);
    if (cmd && cmd.enabled !== false) await cmd.handler();
    setCommand('');
  }, [jevReview]);

  if (!isCollapsed) return null;

  return (
    // cc-glass command line, anchored just above the bottom GlassDock so it
    // reads as the dock's command input (design-spec.md §5.1). z-40 matches the
    // dock / SettingsPanel / StatusCluster overlays; pointer-events restored
    // because the cc-root wrapper is pointer-events:none.
    <div
      className="cc-glass cc-glass--overlay cc-panel-transition fixed left-1/2 -translate-x-1/2 p-3 pointer-events-auto"
      style={{ width: 'min(92vw, 560px)', bottom: 'var(--cc-dock-clearance, 7rem)', zIndex: 'var(--cc-z-popover, 45)' as unknown as number }}
    >
      <form onSubmit={handleSubmit} className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="select-none font-mono text-sm leading-none text-primary/80"
        >
          &rsaquo;
        </span>
        <input
          ref={inputRef}
          type="text"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onFocus={handleInputFocus}
          aria-label="Ask: describe the view you want in plain language"
          data-testid="command-input-field"
          placeholder={isProcessing ? 'Command queued…' : 'Configure view — ask in plain language…'}
          className="w-full rounded-md border border-border/60 bg-input/40 px-3 py-1.5 font-mono text-[13px] text-foreground outline-none transition-colors placeholder:text-muted-foreground/70 focus-visible:border-transparent focus-visible:ring-2 focus-visible:ring-ring"
        />
      </form>

      {jevReview && (
        <div role="group" aria-label="Jev suggestion" className="mt-2 rounded-md border border-border/60 bg-input/30 p-2 text-[12px] font-mono">
          {jevReview.review.kind === 'patch' ? (
            <>
              <div>
                Proposed change: <strong>{jevReview.review.patch.label}</strong>{' '}
                <span className="text-muted-foreground">({jevReview.review.patch.path})</span>
              </div>
              <div>
                {JSON.stringify(jevReview.review.patch.current)} &rarr; {JSON.stringify(jevReview.review.patch.proposed)}
                {jevReview.review.patch.clamped ? ' (clamped to the allowed range)' : ''}
              </div>
            </>
          ) : (
            <div>{jevReview.review.note}</div>
          )}
          <div className="mt-1 flex gap-2">
            {jevReview.review.kind === 'patch' && (
              <button type="button" className="underline" onClick={applyJevPatch}>Apply</button>
            )}
            {jevReview.review.kind === 'intent' && jevReview.review.commandId && (
              <button type="button" className="underline" onClick={runJevCommand}>Run</button>
            )}
            <button type="button" className="underline" onClick={sendToAssistant}>Ask the settings assistant instead</button>
            <button type="button" className="underline" onClick={() => setJevReview(null)}>Dismiss</button>
          </div>
        </div>
      )}

      {statusLines.length > 0 && (
        <div className="mt-2 min-h-[20px] space-y-0.5">
          {statusLines.map(line => (
            <StatusText key={line.id} text={line.text} timestamp={line.timestamp} />
          ))}
        </div>
      )}
    </div>
  );
};

export default CommandInput;
