import { createLogger, createErrorMetadata } from '../../../../utils/loggerConfig';
import { debugState } from '../../../../utils/clientDebugState';
import { unifiedApiClient } from '../../../../services/api/UnifiedApiClient';
import { ensureNodeHasValidPosition } from './nodeUtils';
import type { GraphData, Node, Edge } from '../graphWorkerProxy';

const logger = createLogger('GraphDataManager.restClient');

/** Shape returned by the REST `/graph/data` endpoint. */
interface RawGraphResponse {
  nodes?: unknown[];
  edges?: unknown[];
  metadata?: Record<string, unknown>;
  settlementState?: { isSettled: boolean; stableFrameCount: number; kineticEnergy: number };
}

/**
 * Server-side graph-type filter (PRD-018 WS-4). Sent as `?graph_type=` so the
 * backend returns only the requested population instead of the whole graph
 * (which the client then filtered locally — the transfer this eliminates).
 * `null` / `'all'` → no filter param → server returns the full graph.
 */
export type GraphTypeFilter = 'knowledge' | 'ontology' | 'agent' | 'all' | null;

/**
 * Fetch raw graph data from the backend REST API with up to `maxRetries`
 * attempts and exponential back-off.  Returns a validated `GraphData` object
 * with string-coerced node/edge IDs and enriched positions.
 *
 * @param graphType   Multi-graph identity (`knowledge`/`visionclaw`) — diagnostic only.
 * @param graphTypeFilter Server-side population filter → `?graph_type=`. When
 *   `null`/`'all'` the whole graph is requested (back-compat default).
 */
export async function fetchGraphData(
  graphType: string,
  graphTypeFilter: GraphTypeFilter = null,
  excludeLinkedPages: boolean = false,
): Promise<GraphData> {
  const maxRetries   = 3;
  const initialDelay = 500;

  // Build the request URL. `graph_type` scopes the population; the orthogonal
  // `exclude_linked_pages` drops the wikilink-stub nodes at source so they are
  // never transferred when the client would only hide them (mirrors the server
  // gate, shrinking the dominant payload from ~26.6MB to the rendered set).
  const params = new URLSearchParams();
  if (graphTypeFilter && graphTypeFilter !== 'all') {
    params.set('graph_type', graphTypeFilter);
  }
  if (excludeLinkedPages) {
    params.set('exclude_linked_pages', 'true');
  }
  const qs = params.toString();
  const requestUrl = qs ? `/graph/data?${qs}` : '/graph/data';

  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      if (debugState.isEnabled()) {
        logger.info(
          `Fetching initial ${graphType} graph data` +
            (graphTypeFilter && graphTypeFilter !== 'all' ? ` (graph_type=${graphTypeFilter})` : '') +
            ` (attempt ${attempt}/${maxRetries})`,
        );
      }

      const response = await unifiedApiClient.get(requestUrl, { timeout: 10000 });

      const responseData: RawGraphResponse = response.data.data || response.data;

      if (!responseData || typeof responseData !== 'object') {
        throw new Error('Invalid graph data format: data is not an object');
      }

      const rawNodes = Array.isArray(responseData.nodes) ? responseData.nodes : [];

      // One-time diagnostic for edge shape
      if (Array.isArray(responseData.edges) && responseData.edges.length > 0) {
        const rawEdge = responseData.edges[0] as Record<string, unknown>;
        logger.debug('[restClient] RAW API edge[0]:',
          'keys=', Object.keys(rawEdge),
          'source=', rawEdge.source, '(type:', typeof rawEdge.source + ')',
          'target=', rawEdge.target, '(type:', typeof rawEdge.target + ')',
          'id=', rawEdge.id,
          'full=', JSON.stringify(rawEdge).slice(0, 300));
      }

      // Normalise edge source/target to strings.
      // Rust API returns u32 numeric IDs; Edge interface expects string.
      const edges: Edge[] = Array.isArray(responseData.edges)
        ? (responseData.edges as Array<Edge & Record<string, unknown>>)
            .map(edge => {
              let source = (edge.source ?? edge.from ?? (edge as Record<string,unknown>).from_node ?? (edge as Record<string,unknown>).sourceId ?? (edge as Record<string,unknown>).source_id) as string | undefined;
              let target = (edge.target ?? edge.to ?? (edge as Record<string,unknown>).to_node   ?? (edge as Record<string,unknown>).targetId ?? (edge as Record<string,unknown>).target_id) as string | undefined;

              if (source === 'undefined' || source === 'null') source = undefined;
              if (target === 'undefined' || target === 'null') target = undefined;

              // Recover from id field ("798-861") when source/target are still missing
              if ((source == null || target == null) && edge.id && typeof edge.id === 'string') {
                const parts = edge.id.split('-');
                if (parts.length >= 2) {
                  if (source == null) source = parts[0];
                  if (target == null) target = parts.slice(1).join('-');
                }
              }

              return { ...edge, source: String(source), target: String(target) };
            })
            .filter(edge => edge.source !== 'undefined' && edge.target !== 'undefined')
        : [];

      const metadata       = responseData.metadata || {};
      const settlementState = responseData.settlementState || { isSettled: false, stableFrameCount: 0, kineticEnergy: 0 };

      if (debugState.isEnabled()) {
        logger.debug(`Settlement: settled=${settlementState.isSettled}, frames=${settlementState.stableFrameCount}, KE=${settlementState.kineticEnergy}`);
      }

      const nodes: Node[] = (rawNodes as Node[]).map(node => {
        // Normalise node id to string — API returns u32
        const normalizedNode = { ...node, id: String(node.id) };

        // Recover flat x/y/z fields into a position object when the server sends them flat
        if (!normalizedNode.position) {
          const raw = node as unknown as Record<string, unknown> & { position?: { x?: number; y?: number; z?: number } };
          normalizedNode.position = {
            x: Number(raw.x) || Number(raw.position?.x) || 0,
            y: Number(raw.y) || Number(raw.position?.y) || 0,
            z: Number(raw.z) || Number(raw.position?.z) || 0,
          };
        }

        // Attach metadata from the separate metadata map if present
        const withMeta = normalizedNode as unknown as { metadata_id?: string; metadataId?: string };
        const nodeMetadata = metadata[withMeta.metadata_id || withMeta.metadataId || ''];
        const enriched = nodeMetadata
          ? { ...normalizedNode, metadata: { ...normalizedNode.metadata, ...(nodeMetadata as object) } }
          : normalizedNode;

        return ensureNodeHasValidPosition(enriched);
      });

      if (debugState.isEnabled()) {
        logger.info(`Received ${nodes.length} nodes, ${edges.length} edges (physics settled: ${settlementState.isSettled})`);
      }

      return { nodes, edges };

    } catch (error) {
      logger.error(`Attempt ${attempt} failed:`, createErrorMetadata(error));
      if (attempt === maxRetries) {
        logger.error('All attempts to fetch initial graph data failed.');
        throw error;
      }
      const delay = initialDelay * Math.pow(2, attempt - 1);
      if (debugState.isEnabled()) logger.debug(`Retrying in ${delay}ms...`);
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }

  return { nodes: [], edges: [] };
}

/**
 * A single assertion-version entity returned by `/ontology/state-at` (ADR-049
 * bi-temporal provenance). Governed writes and full corpus syncs (ADR-2022:
 * public class typing and class relations) carry these version entities; KG
 * pages and literals do not, so state-at is the TEMPORAL subset the client
 * overlays on the graph — it does not replace the whole graph.
 */
export interface StateAtAssertion {
  subject: string;
  predicate: string;
  /** IRI or literal. */
  object: string;
  validFrom: string;
  /** null ⇒ still valid (open interval). */
  validTo: string | null;
  generatedAt: string;
  activityUrn: string;
  /** did:nostr or IRI of the attributing agent. */
  agentIri: string;
}

/** Shape returned by the REST `/ontology/state-at` endpoint. */
interface RawStateAtResponse {
  t?: string;
  assertions?: unknown[];
  count?: number;
}

/** Coerce an unknown record into a `StateAtAssertion`, string-normalising ids. */
function normaliseAssertion(raw: Record<string, unknown>): StateAtAssertion {
  const validTo = raw.validTo;
  return {
    subject: String(raw.subject ?? ''),
    predicate: String(raw.predicate ?? ''),
    object: String(raw.object ?? ''),
    validFrom: String(raw.validFrom ?? ''),
    validTo: validTo == null ? null : String(validTo),
    generatedAt: String(raw.generatedAt ?? ''),
    activityUrn: String(raw.activityUrn ?? ''),
    agentIri: String(raw.agentIri ?? ''),
  };
}

/**
 * Fetch the temporal (runtime-governed) assertion set valid at instant `t`
 * from `/ontology/state-at` (ADR-049 bi-temporal semantics: validFrom <= t AND
 * (validTo IS NULL OR t < validTo)). When `recordedAsOf` is given the server
 * additionally applies point-in-time-of-knowledge (generatedAt <= recordedAsOf).
 *
 * Read-authed under the same gate as `/ontology/query` — auth headers are
 * injected by the shared api-client interceptor (dev bypass:
 * `Bearer dev-session-token` + `X-Nostr-Pubkey`), so no per-call header setup.
 *
 * Fails gracefully: any error (network, non-2xx, malformed body) resolves to an
 * empty array rather than throwing, so the timeline overlay degrades to "no
 * runtime assertions at t" instead of breaking the graph view.
 *
 * @param t            Valid-time instant as an RFC3339 string.
 * @param recordedAsOf Optional knowledge-time cutoff (RFC3339).
 */
export async function fetchStateAt(
  t: string,
  recordedAsOf?: string,
): Promise<StateAtAssertion[]> {
  const params = new URLSearchParams();
  params.set('t', t);
  if (recordedAsOf) {
    params.set('recorded_as_of', recordedAsOf);
  }
  const requestUrl = `/ontology/state-at?${params.toString()}`;

  try {
    const response = await unifiedApiClient.get(requestUrl, { timeout: 10000 });
    const responseData: RawStateAtResponse = response.data?.data ?? response.data ?? {};
    const rawAssertions = Array.isArray(responseData.assertions) ? responseData.assertions : [];

    const assertions = rawAssertions
      .filter((a): a is Record<string, unknown> => a != null && typeof a === 'object')
      .map(normaliseAssertion);

    if (debugState.isEnabled()) {
      logger.debug(`state-at t=${t}${recordedAsOf ? ` asOf=${recordedAsOf}` : ''}: ${assertions.length} assertions`);
    }

    return assertions;
  } catch (error) {
    // Fail-open: an empty overlay is the correct degraded state (see doc above).
    logger.warn('fetchStateAt failed; returning empty assertion set:', createErrorMetadata(error));
    return [];
  }
}

/**
 * T6 auto-retry: when the initial REST fetch returns 0 nodes (empty Oxigraph
 * on first-boot / clean restart) schedule periodic re-checks every 15 s for
 * up to 20 attempts (5 minutes total) so the UI recovers without user action.
 *
 * @param attempt   Current retry number (1-based).
 * @param onSuccess Called when >0 nodes are received; caller re-runs full load.
 * @returns The timer handle so the orchestrator can cancel it via clearTimeout.
 */
export function scheduleEmptyDataRetry(
  attempt: number,
  existingTimerRef: number | null,
  onSuccess: () => Promise<unknown>,
  onReschedule: (handle: number) => void,
  graphTypeFilter: GraphTypeFilter = null,
): void {
  const MAX_ATTEMPTS = 20;
  const INTERVAL_MS  = 15_000;
  const retryUrl =
    graphTypeFilter && graphTypeFilter !== 'all'
      ? `/graph/data?graph_type=${encodeURIComponent(graphTypeFilter)}`
      : '/graph/data';

  if (attempt > MAX_ATTEMPTS) {
    logger.warn(`T6 empty-data retry: reached ${MAX_ATTEMPTS} attempts (${MAX_ATTEMPTS * INTERVAL_MS / 1000}s). Giving up.`);
    return;
  }

  if (existingTimerRef !== null) {
    clearTimeout(existingTimerRef);
  }

  const handle = window.setTimeout(async () => {
    console.info(`[GraphDataManager] T6 empty-data retry attempt ${attempt}/${MAX_ATTEMPTS}`);
    try {
      const response = await unifiedApiClient.get(retryUrl, { timeout: 10000 });
      const responseData = response.data.data || response.data;
      const nodes = Array.isArray(responseData?.nodes) ? responseData.nodes : [];
      if (nodes.length > 0) {
        console.info(`[GraphDataManager] T6 empty-data retry: received ${nodes.length} nodes on attempt ${attempt}. Triggering full load.`);
        await onSuccess();
      } else {
        scheduleEmptyDataRetry(attempt + 1, null, onSuccess, onReschedule, graphTypeFilter);
      }
    } catch (err) {
      logger.warn(`T6 empty-data retry attempt ${attempt} failed:`, createErrorMetadata(err));
      scheduleEmptyDataRetry(attempt + 1, null, onSuccess, onReschedule, graphTypeFilter);
    }
  }, INTERVAL_MS) as unknown as number;

  onReschedule(handle);
}
