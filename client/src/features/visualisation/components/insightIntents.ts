/**
 * Knowledge-engineer insight intents for the command box.
 *
 * A knowledge engineer asks the graph a question ("what are the main themes?",
 * "what connects blockchain and robotics?", "where are the gaps?"); the graph
 * answers through settings, layout and analytics only — deterministic, no LLM.
 * `planInsightIntent` is pure: it returns the setting writes and server calls
 * to make plus a short human answer, or null when the text is not an insight
 * question (so the existing view-command rules, focus/search and the LLM
 * fallback behave exactly as before).
 *
 * Every route and setting path emitted is listed in the allow-lists below,
 * derived from the server code, and pinned by `__tests__/insightIntents.test.ts`.
 */
import type { SettingWrite } from '../../control-center/state/settingWrites';

export interface InsightRequest {
  method: 'POST';
  /** Full `/api/...` path; CommandInput strips the `/api` prefix for unifiedApiClient. */
  endpoint: string;
  body: Record<string, unknown>;
}

export interface InsightPlan {
  id: 'communities' | 'bridges' | 'hierarchy' | 'immature' | 'anomalies' | 'hubs';
  /** What the graph will now show and how to read it. */
  answer: string;
  writes: SettingWrite[];
  requests: InsightRequest[];
  /** Bridges only: the two domains the user named (no highlight mechanism exists for them). */
  domains?: [string, string];
}

/**
 * Server routes the intents may call, as `METHOD /api/...`.
 *  - POST /api/analytics/community/detect  src/handlers/api_handler/analytics/mod.rs:207
 *  - POST /api/analytics/anomaly/detect    src/handlers/api_handler/analytics/mod.rs:209
 *  - POST /api/analytics/pagerank/compute  src/handlers/api_handler/analytics/mod.rs:237-240
 *    (analytics scope at mod.rs:196, mounted under /api via api_handler::config, src/main.rs:1183)
 *  - POST /api/layout/mode        src/handlers/layout_handler.rs:369-372 (mounted src/main.rs:1282);
 *    emitted indirectly: writing physics.layoutMode makes applySettingWrites call layoutApi.setMode
 *  - PUT  /api/settings/node-filter    src/settings/api/settings_routes.rs:2033 (nodeFilter.*)
 *  - PUT  /api/settings/quality-gates  src/settings/api/settings_routes.rs:2035 (qualityGates.showAnomalies)
 *  - PUT  /api/settings/visual         src/settings/api/settings_routes.rs:2037 (nodes.colorScheme, clusterHulls.*)
 *  - PUT  /api/settings/physics        src/settings/api/settings_routes.rs:2026 (physics.layoutMode)
 *    (scope /settings, src/main.rs:1163). The PUTs are emitted indirectly by autoSaveManager, not by the intents.
 */
export const INSIGHT_ENDPOINT_ALLOWLIST: readonly string[] = Object.freeze([
  'POST /api/analytics/community/detect',
  'POST /api/analytics/anomaly/detect',
  'POST /api/analytics/pagerank/compute',
]);

/** Store paths the intents may write (each is a registered field in control-center/registry). */
export const INSIGHT_SETTING_PATH_ALLOWLIST: readonly string[] = Object.freeze([
  'visualisation.graphs.knowledge.nodes.colorScheme',
  'visualisation.clusterHulls.enabled',
  'visualisation.clusterHulls.communityFallback',
  'visualisation.graphs.knowledge.physics.layoutMode',
  'qualityGates.showAnomalies',
  'nodeFilter.enabled',
  'nodeFilter.filterByQuality',
  'nodeFilter.qualityThreshold',
  'nodeFilter.filterMode',
  'nodeFilter.minMaturity',
]);

const ANOMALY_OVERLAY = 'qualityGates.showAnomalies';
const COLOR_SCHEME = 'visualisation.graphs.knowledge.nodes.colorScheme';
const HULLS = 'visualisation.clusterHulls.enabled';
// Hulls group by cluster_id; community detection fills community_id, so the fallback must be on.
const HULLS_COMMUNITY = 'visualisation.clusterHulls.communityFallback';
const COMMUNITY_VIEW: SettingWrite[] = [
  { path: COLOR_SCHEME, value: 'community' },
  { path: HULLS, value: true },
  { path: HULLS_COMMUNITY, value: true },
  // The red anomaly overlay outranks every scheme (nodeEncoding.ts:86-98); clear it so the answer is true.
  { path: ANOMALY_OVERLAY, value: false },
];
const LAYOUT_MODE = 'visualisation.graphs.knowledge.physics.layoutMode';

const DOMAIN = '([a-z][a-z0-9_-]*(?:\\s[a-z][a-z0-9_-]*)?)';
const BRIDGE_PAIR = new RegExp(
  `(?:between|connects?|bridges?|links?|joins?)\\s+(?:the\\s+)?${DOMAIN}\\s+(?:and|&|with|to)\\s+(?:the\\s+)?${DOMAIN}`,
);
const TRAILING = /\s+(?:domains?|areas?|fields?|concepts?|together|here)$/;
const clean = (s: string) => s.replace(TRAILING, '').trim();

const communityRequest = (): InsightRequest => ({
  method: 'POST',
  endpoint: '/api/analytics/community/detect',
  body: { algorithm: 'leiden', resolution: 1.0 },
});

export function planInsightIntent(cmd: string): InsightPlan | null {
  const lower = cmd.toLowerCase().replace(/[’]/g, "'").trim();
  if (!lower) return null;
  // Negations ("hide the communities", "turn off anomalies") and explicit layout commands
  // belong to the existing view rules (or the LLM path), never to an insight question.
  if (/\b(?:hide|off|disable|stop|remove|clear|reset)\b|\blayout\b/.test(lower)) return null;

  // 5. Outliers / anomalies — checked first: "what doesn't fit" must not read as a bridge.
  if (/\boutliers?\b|\banomal|(?:doesn'?t|does not|don'?t|do not|didn'?t)\s+fit|\bmisfits?\b|\bodd one/.test(lower)) {
    return {
      id: 'anomalies',
      answer:
        'Running structural anomaly detection (LOF). Anomalous nodes are drawn in red, brighter the stranger they are: ' +
        'concepts whose links look unlike their neighbours, often mis-filed or missing relations.',
      writes: [{ path: ANOMALY_OVERLAY, value: true }],
      requests: [{ method: 'POST', endpoint: '/api/analytics/anomaly/detect', body: { method: 'lof', kNeighbors: 10 } }],
    };
  }

  // 2. Bridges between two named domains.
  const pair = lower.match(BRIDGE_PAIR);
  if (pair && !/\bhull/.test(lower)) {
    const a = clean(pair[1]);
    const b = clean(pair[2]);
    return {
      id: 'bridges',
      answer:
        `Coloured by community with hulls on. Concepts linking ${a} and ${b} are the nodes sitting inside the other ` +
        `domain's colour, or on a hull boundary with edges crossing it. The two domains are ` +
        'lit with a search-hit halo.',
      writes: [...COMMUNITY_VIEW],
      requests: [communityRequest()],
      domains: [a, b],
    };
  }
  if (/\bbridg(?:e|es|ing)\b|cross-?domain/.test(lower)) {
    return {
      id: 'bridges',
      answer:
        'Coloured by community with hulls on. Cross-domain bridges are nodes inside another community\'s colour, ' +
        'or sitting on a hull boundary with edges crossing it.',
      writes: [...COMMUNITY_VIEW],
      requests: [communityRequest()],
    };
  }

  // 3. Hierarchy / backbone.
  if (/\bhierarch|\bbackbone\b|\btaxonom|\bsubclass|\bis\b.*\borgani[sz]ed\b|\bhow\b.*\borgani[sz]ed\b|\bclass tree\b/.test(lower)) {
    return {
      id: 'hierarchy',
      answer:
        'Switched to the hierarchical layout: rdfs:subClassOf edges run top-down, so broad parent classes sit above ' +
        'their children. Depth is distance from the root; a wide row is a family of siblings.',
      writes: [{ path: LAYOUT_MODE, value: 'hierarchical' }],
      requests: [],
    };
  }

  // 4. Immature / low-quality / gaps.
  if (/\bimmature\b|\bemerging\b|low[- ]quality|(?<!between nodes |node )\bgaps?\b(?!\s+between|\s+(?:size|spacing))|\b(?:weak|weakest)\s+(?:concepts?|nodes?|areas?|parts?)\b|\bdrafts?\b|\bunder-?developed\b|\bneeds? (?:more )?work\b/.test(lower)) {
    return {
      id: 'immature',
      answer:
        'Node filter on: nodes below "established" maturity or under 0.6 quality are greyed out, nothing is removed. ' +
        'The greyed nodes are your gaps; coloured nodes are the settled core. Pages with no maturity field are unaffected.',
      writes: [
        { path: 'nodeFilter.enabled', value: true },
        { path: 'nodeFilter.filterByQuality', value: true },
        { path: 'nodeFilter.qualityThreshold', value: 0.6 },
        { path: 'nodeFilter.filterMode', value: 'and' },
        { path: 'nodeFilter.minMaturity', value: 'established' },
      ],
      requests: [],
    };
  }

  // 6. Hubs / most important.
  if (/\bhubs?\b|most (?:important|central|connected|influential)|\bcentral (?:concepts?|nodes?)\b|\bkey (?:concepts?|nodes?)\b|\bcore concepts?\b|\binfluential\b/.test(lower)) {
    return {
      id: 'hubs',
      answer:
        'Running PageRank and colouring by centrality: warmer nodes are more central. Labels are ranked by ' +
        'centrality once it arrives, so the hubs are the names you can read first.',
      writes: [
        { path: COLOR_SCHEME, value: 'centrality' },
        { path: ANOMALY_OVERLAY, value: false },
      ],
      requests: [{ method: 'POST', endpoint: '/api/analytics/pagerank/compute', body: { dampingFactor: 0.85 } }],
    };
  }

  // 1. Communities / themes. Existing view commands (hulls, kmeans/louvain, "group by",
  // "semantic cluster") are not questions about themes and keep their own rules.
  if (
    /\bcommunit(?:y|ies)\b|(?<!dark |light |colou?r |ui )\bthemes?\b(?!\s+(?:colou?rs?|mode))|(?:main|major|big|top)\s+(?:clusters?|topics|areas|groups)|\bwhat (?:are )?the (?:clusters|topics|groups)\b/.test(lower) &&
    !/\bhull|kmeans|louvain|spectral|group by|semantic cluster/.test(lower)
  ) {
    return {
      id: 'communities',
      answer:
        'Coloured by community with hulls on: each colour is a theme found by Leiden community detection, ' +
        'and a hull outlines its extent. Dense, same-coloured groups are the main topics; thin links between hulls are the seams.',
      writes: [...COMMUNITY_VIEW],
      requests: [communityRequest()],
    };
  }

  return null;
}
