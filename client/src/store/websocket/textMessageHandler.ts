/**
 * textMessageHandler.ts — JSON/text WebSocket message handling
 *
 * Processes parsed JSON messages: connection_established, error frames,
 * filter_update_success, initialGraphLoad, memory_flash, etc.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { graphDataManager } from '../../features/graph/managers/graphDataManager';
import { useSettingsStore, settingsStoreUtils } from '../settingsStore';
import { useInferredEdgesStore } from '../../features/ontology/store/useInferredEdgesStore';
import { nostrAuth } from '../../services/nostrAuthService';
import { settingsApi } from '../../api/settingsApi';
import type { WebSocketMessage } from '../../types/websocketTypes';
import type { WebSocketErrorFrame, SettingsUpdatedMessage } from './types';
import { emit, notifyMessageHandlers } from './connectionManager';
import { handleErrorFrame } from './binaryProtocol';
import { isFilterResponseExpected, clearFilterResponseExpectation } from './filterSync';

const logger = createLogger('WebSocketStore');

// ADR-2047: last applied settingsUpdated timestamp per category, used to
// drop a stale/out-of-order broadcast (see handleSettingsUpdated below).
const lastAppliedSettingsTimestamp = new Map<string, number>();

// Live linkage (graphUpdated): trailing debounce so a burst of server-side
// mutation signals costs one refetch. The refetch itself is cheap when nothing
// changed — graphDataManager's topology hash turns a no-change delivery into a
// no-op — so the client can afford to trust every signal.
let graphUpdatedRefetchTimer: ReturnType<typeof setTimeout> | null = null;
const GRAPH_UPDATED_REFETCH_DEBOUNCE_MS = 750;

function scheduleGraphRefetch(revision: number | undefined, reason: string | undefined) {
  if (graphUpdatedRefetchTimer !== null) clearTimeout(graphUpdatedRefetchTimer);
  graphUpdatedRefetchTimer = setTimeout(() => {
    graphUpdatedRefetchTimer = null;
    logger.info(
      `[LiveLinkage] graphUpdated rev=${revision ?? '?'} reason=${reason ?? '?'} — refetching topology`,
    );
    graphDataManager.fetchInitialData()
      .then(() => {
        // REST returns the UNFILTERED graph; if the user has quality gates
        // active, re-assert the server-side filter so the refetch doesn't
        // silently expand a filtered view back to the full graph.
        const nf = useSettingsStore.getState().settings?.nodeFilter;
        if (nf?.enabled && (nf.filterByQuality || nf.filterByAuthority)) {
          reassertFilter(nf);
        }
      })
      .catch((error) => {
        logger.error('[LiveLinkage] topology refetch failed:', createErrorMetadata(error));
      });
    // The reasoned layer: pull fresh Whelk inferences so InferredEdges tracks
    // the evolving ontology, not just the asserted topology.
    useInferredEdgesStore.getState().refresh().catch(() => {
      /* refresh() is internally empty-safe; nothing further to do */
    });
  }, GRAPH_UPDATED_REFETCH_DEBOUNCE_MS);
}

function reassertFilter(nf: { enabled?: boolean; filterByQuality?: boolean; filterByAuthority?: boolean; qualityThreshold?: number; authorityThreshold?: number; filterMode?: string; includeLinkedPages?: boolean }) {
  import('./index').then(({ useWebSocketStore }) => {
    useWebSocketStore.getState().sendFilterUpdate({
      enabled: nf.enabled,
      qualityThreshold: nf.qualityThreshold,
      authorityThreshold: nf.authorityThreshold,
      filterByQuality: nf.filterByQuality,
      filterByAuthority: nf.filterByAuthority,
      filterMode: nf.filterMode,
      includeLinkedPages: nf.includeLinkedPages,
    });
  }).catch(() => { /* store unavailable — next user change re-syncs */ });
}

/**
 * Process a parsed JSON WebSocket message, dispatching to the appropriate
 * handler based on message.type.
 */
export function handleTextMessage(
  message: WebSocketMessage,
  get: () => { forceReconnect: () => void },
  set: (partial: Record<string, unknown>) => void,
  processMessageQueueFn: () => void,
) {
  if (debugState.isDataDebugEnabled()) {
    logger.debug(`Received WebSocket message: ${message.type}`, (message as unknown as Record<string, unknown>).data);
  }

  if (message.type === 'connection_established') {
    set({ isServerReady: true });
    if (debugState.isEnabled()) {
      logger.info('Server connection established and ready');
    }
  }

  if (message.type === 'error' && (message as unknown as Record<string, unknown>).error) {
    handleErrorFrame(
      (message as unknown as Record<string, unknown>).error as WebSocketErrorFrame,
      get,
      processMessageQueueFn,
    );
    return;
  }

  if (message.type === 'filter_update_success') {
    if (debugState.isEnabled()) {
      logger.info(`Filter applied: ${message.data?.visible_nodes}/${message.data?.total_nodes} nodes visible`);
    }
    emit('filterApplied', {
      visibleNodes: message.data?.visible_nodes,
      totalNodes: message.data?.total_nodes
    });
  }

  if (message.type === 'initialGraphLoad') {
    handleInitialGraphLoad(message);
  }

  // Memory flash events -- forward to event bus for EmbeddingCloudLayer
  if (message.type === 'memory_flash' && (message as unknown as Record<string, unknown>).data) {
    emit('memoryFlash', (message as unknown as Record<string, unknown>).data);
  }

  // Live linkage: the server's reasoned graph changed shape (GitHub sync
  // reload, runtime node/edge mutation, ontology write). Refetch topology
  // (debounced) and rebroadcast on the event bus for other consumers.
  if (message.type === 'graphUpdated') {
    const m = message as unknown as { revision?: number; reason?: string };
    emit('graphUpdated', { revision: m.revision, reason: m.reason });
    scheduleGraphRefetch(m.revision, m.reason);
  }

  // ADR-2047 (vc-core): live settings-change broadcast — the actual event
  // name the server emits is "settingsUpdated" (camelCase). The snake_case
  // "settings_update" case in utils/validation.ts has no server sender; this
  // is the authoritative live path.
  if (message.type === 'settingsUpdated') {
    handleSettingsUpdated(message as unknown as SettingsUpdatedMessage);
  }

  notifyMessageHandlers(message);
}

/**
 * ADR-2047: apply a peer's settings change.
 *  - "nodeFilter" carries the full settings object — apply it directly.
 *  - every other category re-reads that category from the settings API,
 *    since the server does not send its value on the wire.
 * The writer's own echo is ignored (it already has the up-to-date state
 * locally and a round-trip could fight an in-flight edit), and a message
 * older than the last one applied for that category is dropped.
 */
function handleSettingsUpdated(message: SettingsUpdatedMessage) {
  const category = message?.category;
  if (!category || typeof category !== 'string') {
    logger.warn('[SettingsUpdated] settingsUpdated message missing category; ignoring');
    return;
  }

  const currentUser = nostrAuth.getCurrentUser();
  if (message.updatedBy && currentUser?.pubkey && message.updatedBy === currentUser.pubkey) {
    if (debugState.isEnabled()) {
      logger.info(`[SettingsUpdated] Ignoring own write echo for category=${category}`);
    }
    return;
  }

  const incomingTimestamp = typeof message.timestamp === 'number' ? message.timestamp : Date.now();
  const lastApplied = lastAppliedSettingsTimestamp.get(category);
  if (lastApplied !== undefined && incomingTimestamp <= lastApplied) {
    logger.debug(
      `[SettingsUpdated] Ignoring stale settingsUpdated for category=${category} ` +
      `(ts=${incomingTimestamp} <= last=${lastApplied})`
    );
    return;
  }
  lastAppliedSettingsTimestamp.set(category, incomingTimestamp);

  if (category === 'nodeFilter') {
    if (message.settings && typeof message.settings === 'object') {
      useSettingsStore.getState().set('nodeFilter', message.settings, true);
      logger.info('[SettingsUpdated] Applied nodeFilter settings from peer update');
    } else {
      logger.warn('[SettingsUpdated] nodeFilter settingsUpdated message missing settings payload; ignoring');
    }
    return;
  }

  refetchSettingsCategory(category);
}

/** Re-read a settings category from the server and merge it into the store. */
function refetchSettingsCategory(category: string): void {
  const paths = settingsStoreUtils.getSectionPaths(category);
  if (paths.length === 0) {
    logger.warn(`[SettingsUpdated] No known settings paths for category=${category}; cannot re-read`);
    return;
  }

  settingsApi.getSettingsByPaths(paths)
    .then((fetched) => {
      const state = useSettingsStore.getState();
      for (const path of paths) {
        const value = getNestedValueForPath(fetched as Record<string, unknown>, path);
        if (value !== undefined) {
          state.set(path, value, true);
        }
      }
      logger.info(`[SettingsUpdated] Re-read category=${category} from settings API (${paths.length} path(s))`);
    })
    .catch((error) => {
      logger.error(`[SettingsUpdated] Failed to re-read category=${category}:`, createErrorMetadata(error));
    });
}

function getNestedValueForPath(obj: Record<string, unknown>, path: string): unknown {
  let current: unknown = obj;
  for (const key of path.split('.')) {
    if (current === null || current === undefined || typeof current !== 'object') return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

function handleInitialGraphLoad(message: WebSocketMessage) {
  const msgData = message as unknown as { nodes?: unknown[]; edges?: unknown[] };
  const nodes = msgData.nodes || [];
  const edges = msgData.edges || [];
  logger.info(`[WebSocket] Received initialGraphLoad with ${nodes.length} nodes, ${edges.length} edges`);

  // A REST `/graph/data` load in flight is authoritative: hand the payload to
  // the manager, which holds it until REST settles (see offerServerGraphLoad).
  // The shrink-guard below only applies once no REST load is pending.
  const restInFlight = graphDataManager.isRestLoadInFlight();
  let isFilterResponse = false;
  if (restInFlight) {
    isFilterResponse = isFilterResponseExpected();
    if (isFilterResponse) clearFilterResponseExpectation();
  } else {
    const existingNodeCount = graphDataManager.nodeIdMap.size;
    if (existingNodeCount > 0 && nodes.length < existingNodeCount) {
      // Shrink-guard: the connect-time initialGraphLoad is capped (3,000 nodes)
      // and must not clobber a full REST load. BUT a smaller payload arriving
      // right after WE sent a filter_update is the server's FILTERED graph —
      // the authoritative answer to the user's quality gates — and must land.
      // Without this exception every filter response was silently discarded
      // and the quality-gate UI appeared dead.
      if (isFilterResponseExpected()) {
        clearFilterResponseExpectation();
        isFilterResponse = true;
        logger.info(
          `[WebSocket] Accepting filtered initialGraphLoad: ${nodes.length} nodes ` +
          `(down from ${existingNodeCount}) in response to filter_update`
        );
      } else {
        logger.info(
          `[WebSocket] Skipping initialGraphLoad setGraphData: REST already loaded ${existingNodeCount} nodes, ` +
          `WS only has ${nodes.length}. Positions will arrive via binary stream.`
        );
        emit('graphDataUpdated', {
          nodeCount: existingNodeCount,
          edgeCount: 0,
          source: 'websocket_filter_skipped'
        });
        return;
      }
    }
  }

  const transformedNodes = nodes.map((node: unknown) => {
    const n = node as Record<string, unknown>;
    return {
      id: String(n.id),
      label: String(n.label || n.name || n.id),
      type: (n.node_type ?? n.nodeType ?? n.type) as string | undefined,
      position: (n.position as { x: number; y: number; z: number }) || { x: Number(n.x) || 0, y: Number(n.y) || 0, z: Number(n.z) || 0 },
      metadata: {
        ...(n.metadata as Record<string, unknown>),
        quality_score: n.quality_score ?? (n.metadata as Record<string, unknown>)?.quality_score,
        authority_score: n.authority_score ?? (n.metadata as Record<string, unknown>)?.authority_score,
      },
      color: n.color as string | undefined,
      size: n.size as number | undefined,
    };
  });

  const transformedEdges = edges.map((edge: unknown) => {
    const e = edge as Record<string, unknown>;
    let source = (e.source ?? e.from ?? e.from_node ?? e.sourceId ?? e.source_id) as string | undefined;
    let target = (e.target ?? e.to ?? e.to_node ?? e.targetId ?? e.target_id) as string | undefined;

    if (source === undefined || source === 'undefined' || source === 'null') source = undefined;
    if (target === undefined || target === 'undefined' || target === 'null') target = undefined;

    const edgeId = String(e.id || '');
    if ((source == null || target == null) && edgeId) {
      const parts = edgeId.split('-');
      if (parts.length >= 2) {
        if (source == null) source = parts[0];
        if (target == null) target = parts.slice(1).join('-');
      }
    }

    return {
      id: edgeId || `${source}-${target}`,
      source: String(source),
      target: String(target),
      weight: e.weight as number | undefined,
      label: e.label as string | undefined,
      edgeType: (e.edgeType ?? e.edge_type ?? e.relation_type) as string | undefined,
      owlPropertyIri: (e.owlPropertyIri ?? e.owl_property_iri) as string | undefined,
    };
  }).filter((edge: { source: string; target: string }) => edge.source !== 'undefined' && edge.target !== 'undefined');

  graphDataManager.offerServerGraphLoad(
    { nodes: transformedNodes, edges: transformedEdges },
    { isFilterResponse },
  ).then(outcome => {
    if (outcome === 'deferred') return;
    logger.info(`[WebSocket] Graph updated with ${transformedNodes.length} nodes from server filter`);
    emit('graphDataUpdated', {
      nodeCount: transformedNodes.length,
      edgeCount: transformedEdges.length,
      source: 'websocket_filter'
    });
  }).catch(error => {
    logger.error('[WebSocket] Failed to update graph data from initialGraphLoad:', createErrorMetadata(error));
  });
}
