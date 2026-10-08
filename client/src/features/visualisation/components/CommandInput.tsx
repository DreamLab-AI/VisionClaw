import React, { useState, useRef, useEffect, useCallback } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { UNIFIED_SETTINGS_CONFIG } from '../../control-center/registry/commandInputCompat';
import { ALL_PATHS } from '../../control-center/registry/settingsRegistry';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';

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
  endpoint?: string;
  method?: string;
  body?: Record<string, unknown>;
  localAction?: () => void;
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

function parseCommandToActions(cmd: string): SettingsAction[] {
  const lower = cmd.toLowerCase();
  const actions: SettingsAction[] = [];

  // Cluster hulls
  if (lower.includes('hull') || lower.includes('cluster hull')) {
    if (lower.includes('hide') || lower.includes('disable') || lower.includes('off')) {
      actions.push({
        description: 'Disabling cluster hulls',
        localAction: () => {
          const ss = useSettingsStore.getState();
          ss?.updateSettings?.((draft: any) => {
            if (draft.visualisation?.clusterHulls) draft.visualisation.clusterHulls.enabled = false;
          });
        },
      });
    } else {
      actions.push({
        description: 'Enabling cluster hulls',
        localAction: () => {
          const ss = useSettingsStore.getState();
          ss?.updateSettings?.((draft: any) => {
            if (!draft.visualisation) draft.visualisation = {};
            if (!draft.visualisation.clusterHulls) draft.visualisation.clusterHulls = {};
            draft.visualisation.clusterHulls.enabled = true;
          });
        },
      });
    }
  }

  // Physics: repulsion. "separate"/"split" are NOT routed here — those move the
  // two graphs apart via the dual-graph disc projection below, not the repulsion
  // force constant.
  if (lower.includes('repul') || lower.includes('spread')) {
    const increase = lower.includes('increase') || lower.includes('more') || lower.includes('spread');
    const value = increase ? 400 : 100;
    actions.push({
      description: `Setting repulsion to ${value}`,
      endpoint: '/api/settings/physics',
      method: 'PUT',
      body: { repelK: value },
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
        endpoint: '/api/settings/physics',
        method: 'PUT',
        body: discBody,
      });
    }
  }

  // Physics: spring / attract / tight / compact
  if (lower.includes('spring') || lower.includes('attract') || lower.includes('tight') || lower.includes('compact')) {
    const increase = lower.includes('increase') || lower.includes('more') || lower.includes('tight') || lower.includes('compact');
    const value = increase ? 5.0 : 1.0;
    actions.push({
      description: `Setting spring strength to ${value}`,
      endpoint: '/api/settings/physics',
      method: 'PUT',
      body: { springK: value },
    });
  }

  // Physics: damping
  if (lower.includes('damp')) {
    const value = lower.includes('increase') || lower.includes('more') ? 0.8 : 0.3;
    actions.push({
      description: `Setting damping to ${value}`,
      endpoint: '/api/settings/physics',
      method: 'PUT',
      body: { damping: value },
    });
  }

  // Visibility: knowledge
  if (lower.includes('knowledge')) {
    const show = !lower.includes('hide');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} knowledge nodes`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('visualisation.graphs.knowledge.nodes.nodeTypeVisibility.knowledge', show);
      },
    });
  }

  // Visibility: ontology
  if (lower.includes('ontology') && (lower.includes('show') || lower.includes('hide'))) {
    const show = !lower.includes('hide');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} ontology nodes`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('visualisation.graphs.knowledge.nodes.nodeTypeVisibility.ontology', show);
      },
    });
  }

  // Quality filter
  if (lower.includes('quality') && (lower.match(/\d+\.?\d*/) || lower.includes('high'))) {
    const match = lower.match(/(\d+\.?\d*)/);
    const threshold = match ? parseFloat(match[1]) : 0.8;
    const normalized = threshold > 1 ? threshold / 100 : threshold;
    actions.push({
      description: `Filtering to quality >= ${normalized}`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('nodeFilter.enabled', true);
        ss?.set?.('nodeFilter.filterByQuality', true);
        ss?.set?.('nodeFilter.qualityThreshold', normalized);
      },
    });
  }

  // Orphan / isolated node visibility (degree-0 spray)
  if (lower.includes('orphan') || lower.includes('isolated') || lower.includes('unconnected') || lower.includes('disconnected')) {
    const hide = !lower.includes('show');
    actions.push({
      description: `${hide ? 'Hiding' : 'Showing'} orphan (degree-0) nodes`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('nodeFilter.minConnections', hide ? 1 : 0);
      },
    });
  }

  // Node opacity
  if (lower.includes('opacity') || lower.includes('transparent') || lower.includes('visible')) {
    const match = lower.match(/(\d+\.?\d*)/);
    let value = match ? parseFloat(match[1]) : 0.8;
    if (value > 1) value = value / 100;
    actions.push({
      description: `Setting node opacity to ${value}`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('visualisation.graphs.knowledge.nodes.opacity', value);
      },
    });
  }

  // Labels
  if (lower.includes('label')) {
    const show = !lower.includes('hide') && !lower.includes('off');
    actions.push({
      description: `${show ? 'Showing' : 'Hiding'} labels`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.set?.('visualisation.graphs.knowledge.labels.enableLabels', show);
      },
    });
  }

  // Reset to defaults. Skipped when the command is specifically about disc
  // separation/flatten ("reset the separation and flattening") — that is handled
  // by the dual-graph disc rule above and must not also reset the force constants.
  if ((lower.includes('reset') || lower.includes('default'))
    && !lower.includes('separat') && !lower.includes('flatten') && !/\bflat\b/.test(lower)) {
    actions.push({
      description: 'Resetting physics to defaults',
      endpoint: '/api/settings/physics',
      method: 'PUT',
      // Canonical defaults — mirror DEFAULT_PHYSICS_SETTINGS exactly.
      body: { repelK: 120, springK: 12.0, damping: 0.9, restLength: 50, maxVelocity: 100 },
    });
  }

  // Bloom / glow
  if (lower.includes('glow') || lower.includes('bloom')) {
    const enable = !lower.includes('off') && !lower.includes('disable');
    actions.push({
      description: `${enable ? 'Enabling' : 'Disabling'} bloom`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          if (draft.visualisation?.bloom) draft.visualisation.bloom.enabled = enable;
          if (draft.visualisation?.glow) draft.visualisation.glow.enabled = enable;
        });
      },
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
      endpoint: '/api/settings/physics',
      method: 'PUT',
      body: { clusterStrength: 0.005, clusteringAlgorithm: 'leiden', clusteringResolution: 1.0, clusteringIterations: 50 },
    });
    actions.push({
      description: 'Enabling cluster visualization',
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          if (!draft.qualityGates) draft.qualityGates = {};
          draft.qualityGates.showClusters = true;
          draft.qualityGates.semanticForces = true;
          if (!draft.visualisation) draft.visualisation = {};
          if (!draft.visualisation.clusterHulls) draft.visualisation.clusterHulls = {};
          draft.visualisation.clusterHulls.enabled = true;
        });
      },
    });
  }

  // "less overwhelming labels", "fewer labels", "reduce label clutter"
  if (lower.includes('overwhelm') || lower.includes('clutter') || lower.includes('fewer label') || lower.includes('less label')) {
    actions.push({
      description: 'Reducing label visibility for clarity',
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          const labels = draft.visualisation?.graphs?.knowledge?.labels;
          if (labels) {
            // Raw target values (no scaling) within the slider bounds.
            labels.labelDistanceThreshold = 600;
            labels.desktopFontSize = 0.25;
            labels.showMetadata = false;
          }
        });
      },
    });
  }

  // "more labels", "bigger labels", "I can't read the labels"
  if ((lower.includes('more') || lower.includes('bigger') || lower.includes('read')) && lower.includes('label')) {
    actions.push({
      description: 'Increasing label visibility',
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          const labels = draft.visualisation?.graphs?.knowledge?.labels;
          if (labels) {
            // Raw target values (no scaling) within the slider bounds.
            labels.labelDistanceThreshold = 1600;
            labels.desktopFontSize = 0.6;
            labels.enableLabels = true;
          }
        });
      },
    });
  }

  // "only knowledge and agents" / "only show knowledge graph and agents"
  if (lower.includes('only') && (lower.includes('knowledge') || lower.includes('agent'))) {
    const showKnowledge = lower.includes('knowledge');
    const showAgent = lower.includes('agent');
    const showOntology = lower.includes('ontology');
    actions.push({
      description: `Showing: ${[showKnowledge && 'knowledge', showOntology && 'ontology', showAgent && 'agents'].filter(Boolean).join(', ')}`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          const vis = draft.visualisation?.graphs?.knowledge?.nodes?.nodeTypeVisibility;
          if (vis) {
            vis.knowledge = showKnowledge;
            vis.ontology = showOntology;
            vis.agent = showAgent;
          }
        });
      },
    });
  }

  // "layout force directed" / "layout DAG" / "layout radial"
  if (lower.includes('layout') && (lower.includes('dag') || lower.includes('radial') || lower.includes('hierarchy') || lower.includes('tree'))) {
    const mode = lower.includes('radial') ? 'dag-radial' : lower.includes('left') ? 'dag-leftright' : 'dag-topdown';
    actions.push({
      description: `Switching to ${mode} layout`,
      localAction: () => {
        const ss = useSettingsStore.getState();
        ss?.updateSettings?.((draft: any) => {
          if (!draft.qualityGates) draft.qualityGates = {};
          draft.qualityGates.layoutMode = mode;
          draft.qualityGates.semanticForces = true;
        });
      },
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
            const ss = useSettingsStore.getState();
            if (view.physics) {
              await unifiedApiClient.put('/settings/physics', view.physics);
            }
            if (view.nodeTypeVisibility && ss?.updateSettings) {
              ss.updateSettings((draft: any) => {
                if (draft.visualisation?.graphs?.knowledge?.nodes) {
                  draft.visualisation.graphs.knowledge.nodes.nodeTypeVisibility = view.nodeTypeVisibility;
                }
              });
            }
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
  if (action.localAction) {
    action.localAction();
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

export const CommandInput: React.FC<CommandInputProps> = ({ isCollapsed }) => {
  const [command, setCommand] = useState('');
  const [queue, setQueue] = useState<string[]>([]);
  const [isProcessing, setIsProcessing] = useState(false);
  const [statusLines, setStatusLines] = useState<StatusLine[]>([]);
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

      for (const action of actions) {
        addStatus(action.description);
        if (action.isLlmFallback) {
          await dispatchToSettingsLLM(cmd, addStatus);
        } else {
          await executeAction(action);
        }
        await new Promise(r => setTimeout(r, 300));
      }

      addStatus('Configuration applied');
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

  if (!isCollapsed) return null;

  return (
    // cc-glass command line, anchored just above the bottom GlassDock so it
    // reads as the dock's command input (design-spec.md §5.1). z-40 matches the
    // dock / SettingsPanel / StatusCluster overlays; pointer-events restored
    // because the cc-root wrapper is pointer-events:none.
    <div
      className="cc-glass cc-glass--overlay cc-panel-transition fixed bottom-28 left-1/2 z-40 -translate-x-1/2 p-3 pointer-events-auto"
      style={{ width: 'min(92vw, 560px)' }}
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
          placeholder={isProcessing ? 'Command queued…' : 'Configure view — ask in plain language…'}
          className="w-full rounded-md border border-border/60 bg-input/40 px-3 py-1.5 font-mono text-[13px] text-foreground outline-none transition-colors placeholder:text-muted-foreground/70 focus-visible:border-transparent focus-visible:ring-2 focus-visible:ring-ring"
        />
      </form>

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
