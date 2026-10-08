/**
 * Exhaustive field lists of the memory-cloud wire types, for the shared
 * fixture test (`__tests__/wire.test.ts`).
 *
 * They live outside `__tests__` because `tsc -p .` skips test files, and the
 * point is the compile-time check: each `Record<keyof T, true>` fails to
 * compile when a key is missing (`Record` needs every key) or extra (excess
 * property check). The test then pins these lists to `fixtures/wire.json`,
 * which `wire.rs` round-trips, so a field added or renamed on either side
 * fails somewhere. Nothing in the app imports this module.
 */
import type {
  MemoryCloudHealth,
  MemoryCloudHit,
  MemoryCloudMeta,
  MemoryCloudQueryRequest,
  MemoryCloudQueryResponse,
  MemoryCloudRecallProbe,
  MemoryCloudSearchMethod,
  MemoryCloudSidecarIssue,
  MemoryCloudSnapshot,
  MemoryCloudStratum,
} from './types';

type Keys<T> = Record<keyof T, true>;

export const WIRE_KEYS = {
  snapshot: {
    version: true, snapshotId: true, generatedAt: true, dim: true, count: true, positions: true, metadata: true,
    namespaces: true, sourceTypes: true, strata: true, excludedNamespaces: true, vectorsUrl: true,
  } satisfies Keys<MemoryCloudSnapshot>,
  meta: { id: true, key: true, namespace: true, sourceType: true, updatedAt: true } satisfies Keys<MemoryCloudMeta>,
  stratum: { namespace: true, total: true, sampled: true } satisfies Keys<MemoryCloudStratum>,
  queryRequest: { text: true, k: true, namespace: true } satisfies Keys<MemoryCloudQueryRequest>,
  queryResponse: { snapshotId: true, embedModel: true, query: true, sidecar: true } satisfies Keys<MemoryCloudQueryResponse>,
  queryEcho: { text: true, vector: true, position: true } satisfies Keys<MemoryCloudQueryResponse['query']>,
  sidecar: { results: true, tookMs: true, method: true } satisfies Keys<MemoryCloudQueryResponse['sidecar']>,
  hit: {
    id: true, key: true, namespace: true, sourceType: true, score: true, snippet: true, sampleIndex: true, position: true,
  } satisfies Keys<MemoryCloudHit>,
  health: {
    snapshotId: true, generatedAt: true, sidecar: true, embedder: true, namespaces: true, recallProbe: true,
  } satisfies Keys<MemoryCloudHealth>,
  healthSidecar: { reachable: true, extensionVersion: true, error: true } satisfies Keys<MemoryCloudHealth['sidecar']>,
  healthEmbedder: { model: true, reachable: true } satisfies Keys<MemoryCloudHealth['embedder']>,
  healthNamespace: {
    namespace: true, total: true, embedded: true, sampled: true,
  } satisfies Keys<MemoryCloudHealth['namespaces'][number]>,
  recallProbe: {
    k: true, probes: true, recall: true, indexMs: true, exactMs: true, measuredAt: true,
  } satisfies Keys<MemoryCloudRecallProbe>,
  /** serde variants of `SearchMethod` */
  searchMethod: { hnsw: true, exact: true } satisfies Record<MemoryCloudSearchMethod, true>,
  /** serde variants of `SidecarIssue` */
  sidecarIssue: { not_configured: true, building: true, unreachable: true } satisfies Record<MemoryCloudSidecarIssue, true>,
};
