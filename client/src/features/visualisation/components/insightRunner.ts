/**
 * Client-side follow-ups for the insight intents (the impure half of
 * `insightAnalysis`): read the graph the client already holds, write emphasis on
 * its own `emph:insight` source, and return the truthful answer line.
 */
import { graphDataManager } from '../../graph/managers/graphDataManager';
import { writeEmphasis, EMPHASIS_PREFIX } from '../../graph/explore/filterRegistry';
import { HIGHLIGHT_SEARCH_HIT } from '../../graph/state/highlightMask';
import { nodeAnalyticsStore } from '../../analytics/store/nodeAnalyticsStore';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import type { InsightPlan } from './insightIntents';
import {
  anomalyAnswer, authoredHubs, bridgeSet, gapConcepts, hierarchyNeighbourhood,
  type AnalysisEdge, type AnalysisNode,
} from './insightAnalysis';

export const INSIGHT_EMPHASIS_SOURCE = `${EMPHASIS_PREFIX}insight`;
const HUB_COUNT = 12;
const names = (xs: ReadonlyArray<{ label: string }>, n = 4) => xs.slice(0, n).map((x) => x.label).join(', ');
const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Responses arrive either as the body or wrapped in `{ data }` by the API client. */
const body = <T>(res: unknown, key: string): T | undefined => {
  const r = res as Record<string, unknown> | undefined;
  return (r && key in r ? r : (r?.data as Record<string, unknown> | undefined)) as T | undefined;
};

async function waitForCentrality(nodes: readonly AnalysisNode[], timeoutMs = 6000): Promise<boolean> {
  const has = () => nodes.some((n) => nodeAnalyticsStore.centralityOf(String(n.id)) > 0);
  for (let waited = 0; waited <= timeoutMs; waited += 500) {
    if (has()) return true;
    if (waited >= timeoutMs) break;
    await delay(500);
  }
  return false;
}

/** Run the follow-up for `plan`; returns the answer line to show, or undefined when there is none. */
export async function runInsightFollowUp(plan: InsightPlan, response?: unknown): Promise<string | undefined> {
  const data = await graphDataManager.getGraphData();
  const nodes = data.nodes as unknown as AnalysisNode[];
  const edges = data.edges as unknown as AnalysisEdge[];
  const centrality = (id: string) => nodeAnalyticsStore.centralityOf(id);

  switch (plan.followUp) {
    case 'bridges': {
      if (!plan.domains) return undefined;
      const [a, b] = plan.domains;
      const br = bridgeSet(nodes, edges, plan.domains);
      writeEmphasis(INSIGHT_EMPHASIS_SOURCE, HIGHLIGHT_SEARCH_HIT, br.ids.length ? br.ids : null);
      if (br.crossEdges === 0) return `No edges cross between ${a} and ${b}: nothing connects them directly.`;
      return `${br.crossEdges} edges cross between ${a} and ${b}, through ${br.ids.length} concepts. ` +
        `Most connected: ${names(br.top)}. They carry a halo.`;
    }
    case 'hubs': {
      await waitForCentrality(nodes);
      const hubs = authoredHubs(nodes, centrality, HUB_COUNT);
      writeEmphasis(INSIGHT_EMPHASIS_SOURCE, HIGHLIGHT_SEARCH_HIT, hubs.length ? hubs.map((h) => h.id) : null);
      return hubs.length
        ? `Top authored hubs: ${names(hubs, 5)}. Domain roots are system-made and left out.`
        : 'PageRank has not reported centrality yet; try again in a moment.';
    }
    case 'gaps': {
      if (!(await waitForCentrality(nodes, 0))) {
        await unifiedApiClient.post('/analytics/pagerank/compute', { dampingFactor: 0.85 });
        await waitForCentrality(nodes);
      }
      const gaps = gapConcepts(nodes, centrality);
      writeEmphasis(INSIGHT_EMPHASIS_SOURCE, HIGHLIGHT_SEARCH_HIT, gaps.length ? gaps.map((g) => g.id) : null);
      return gaps.length
        ? `${gaps.length} important concepts are still draft or emerging: ${names(gaps, 5)}.`
        : 'No top-decile concept is draft or emerging, or centrality is not available yet.';
    }
    case 'hierarchy': {
      const h = plan.concept ? hierarchyNeighbourhood(nodes, edges, plan.concept) : null;
      writeEmphasis(INSIGHT_EMPHASIS_SOURCE, HIGHLIGHT_SEARCH_HIT, h ? h.ids : null);
      if (!plan.concept) return undefined;
      if (!h) return `No concept matching "${plan.concept}" was found.`;
      return `${h.focus.label}: ${h.parents.length} parents (${names(h.parents) || 'none'}), ` +
        `${h.children.length} children (${names(h.children) || 'none'}). The layout keeps its physics.`;
    }
    case 'anomalies':
      return anomalyAnswer(body(response, 'anomalies'), nodes);
    default:
      return undefined;
  }
}
