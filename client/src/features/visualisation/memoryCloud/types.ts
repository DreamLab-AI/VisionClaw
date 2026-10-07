/**
 * Wire contract for the live RuVector memory cloud (`/api/memory-cloud*`).
 *
 * The Rust structs in `crates/visionclaw-memory-cloud/src/wire.rs` serialise
 * these shapes with `#[serde(rename_all = "camelCase")]`; keep the two in
 * step. `__tests__/fixtures/wire.json` is checked against both sides (Vitest
 * `wire.test.ts`, Rust `client_fixture_round_trips`).
 *
 * The snapshot keeps the field names of the retired static
 * `embedding-cloud.json` (`count`, `positions`, `metadata`, `namespaces`,
 * `sourceTypes`) so `EmbeddingCloudLayer` reads either form.
 */

/** One sampled memory entry. Row order matches `positions` and the vectors blob. */
export interface MemoryCloudMeta {
  /** `memory_entries.id` */
  id: string;
  key: string;
  namespace: string;
  sourceType: string;
  /** `updated_at` as Unix milliseconds */
  updatedAt: number;
}

/** Per-namespace sampling outcome. */
export interface MemoryCloudStratum {
  namespace: string;
  /** rows with an embedding in the sidecar */
  total: number;
  /** rows included in this snapshot */
  sampled: number;
}

/** `GET /api/memory-cloud` */
export interface MemoryCloudSnapshot {
  version: 1;
  /** opaque id; changes whenever the sample is rebuilt */
  snapshotId: string;
  /** Unix milliseconds */
  generatedAt: number;
  /** embedding dimension (384 for bge-small-en-v1.5) */
  dim: number;
  count: number;
  /** 3 * count floats, PCA-projected and scaled to roughly [-100, 100] */
  positions: number[];
  metadata: MemoryCloudMeta[];
  /** distinct namespaces present in the sample, sorted */
  namespaces: string[];
  /** distinct source types present in the sample, sorted */
  sourceTypes: string[];
  strata: MemoryCloudStratum[];
  /** namespaces withheld by server policy (never sampled, never queried) */
  excludedNamespaces: string[];
  /**
   * Relative URL of the vectors blob for this snapshot:
   * little-endian f32, `count * dim` values, row-major, each row L2-normalised.
   * The server answers 409 when the snapshot has since been rebuilt.
   */
  vectorsUrl: string;
}

/** `POST /api/memory-cloud/query` body */
export interface MemoryCloudQueryRequest {
  text: string;
  /** 1..50, default 10 */
  k?: number;
  /** restrict the sidecar search to one namespace */
  namespace?: string;
}

/** One result from the sidecar's own HNSW search. */
export interface MemoryCloudHit {
  id: string;
  key: string;
  namespace: string;
  sourceType: string;
  /** cosine similarity, 1 = identical */
  score: number;
  /** first ~160 chars of the value, flattened to text */
  snippet: string;
  /** row index in the current snapshot, or null when the entry was not sampled */
  sampleIndex: number | null;
}

/** How the sidecar produced a result list (`wire.rs` `SearchMethod`). */
export const SEARCH_METHODS = ['hnsw', 'exact'] as const;
/**
 * `hnsw`: the sidecar's HNSW index, what an agent's `memory_search` sees.
 * `exact`: a sequential scan, used for namespace-restricted queries and for a
 * global query whose HNSW candidates were mostly excluded.
 */
export type MemoryCloudSearchMethod = (typeof SEARCH_METHODS)[number];

/** `POST /api/memory-cloud/query` response */
export interface MemoryCloudQueryResponse {
  snapshotId: string;
  embedModel: string;
  query: {
    text: string;
    /** L2-normalised query embedding, length = snapshot `dim` */
    vector: number[];
  };
  sidecar: {
    results: MemoryCloudHit[];
    tookMs: number;
    method: MemoryCloudSearchMethod;
  };
}

/** Index-versus-exact recall probe run by the server against the sidecar. */
export interface MemoryCloudRecallProbe {
  k: number;
  /** number of probe queries (sampled entries used as queries) */
  probes: number;
  /** mean recall@k of the sidecar HNSW index against an exact sequential scan */
  recall: number;
  /** mean milliseconds per query */
  indexMs: number;
  exactMs: number;
  /** Unix milliseconds */
  measuredAt: number;
}

/**
 * Why the sidecar is not serving (`wire.rs` `SidecarIssue`). A closed set:
 * driver and connection detail stays in the server log.
 */
export const SIDECAR_ISSUES = ['not_configured', 'building', 'unreachable'] as const;
export type MemoryCloudSidecarIssue = (typeof SIDECAR_ISSUES)[number];

/** `GET /api/memory-cloud/health` (power-user signed, like every endpoint) */
export interface MemoryCloudHealth {
  snapshotId: string | null;
  generatedAt: number | null;
  sidecar: {
    reachable: boolean;
    /** `pg_extension.extversion` for `ruvector` */
    extensionVersion: string | null;
    /** null while the sidecar is serving */
    error: MemoryCloudSidecarIssue | null;
  };
  embedder: {
    model: string;
    /** whether the embedder answered on the latest refresh cycle */
    reachable: boolean;
  };
  namespaces: Array<{
    namespace: string;
    total: number;
    embedded: number;
    sampled: number;
  }>;
  /** null until the first background probe completes */
  recallProbe: MemoryCloudRecallProbe | null;
}
