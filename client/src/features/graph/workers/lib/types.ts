/**
 * Shared type definitions for the graph physics worker.
 * Keep this file free of runtime code — pure type declarations only.
 */

/**
 * Typed metadata for graph nodes.
 *
 * The index signature `[key: string]: any` preserves full backward compatibility —
 * existing code can access arbitrary fields from the backend without casts.
 * The named optional fields provide autocomplete, documentation, and a canonical
 * reference of known metadata shapes used across the codebase.
 *
 * Known node archetypes (discriminated by `type`):
 *  - `'agent'`    — agentType, health, status, workload, tokenRate, currentTask, etc.
 *  - `'knowledge'`— quality, authority, source_domain, page_url, file_path, etc.
 *  - `'ontology'` — hierarchyDepth, classIri, violations, constraintValid, etc.
 */
export interface NodeMetadata {
  // --- Discriminator ---
  type?: string;
  nodeType?: string;

  // --- Common ---
  // Note: quality, quality_score, qualityScore, authority, authority_score,
  // authorityScore, and instanceCount are intentionally omitted from named
  // fields because they arrive from the backend as mixed string/number/unknown
  // types and are consumed inconsistently (parseInt, parseFloat, ?? 0, etc.).
  // They remain fully accessible via the [key: string]: any index signature.
  size?: number;
  depth?: number;
  lastModified?: string | number;
  last_modified?: string | number;
  updated_at?: string | number;
  updatedAt?: string | number;
  color?: string;
  name?: string;
  velocity?: { x: number; y: number; z: number };

  // --- Domain / clustering ---
  source_domain?: string;
  domain?: string;
  cluster?: string;

  // --- Ontology / hierarchy ---
  classIri?: string;
  hierarchyDepth?: number;
  violations?: number;
  constraintValid?: boolean;

  // --- Agent ---
  agentType?: string;
  agent_type?: string;
  health?: number;
  status?: string;
  workload?: number;
  tokenRate?: number;
  currentTask?: string;
  tasksActive?: number;
  tasks?: number;

  // --- Resource metrics ---
  cpu_usage?: string;
  memory_usage?: string;
  tokens?: string;
  created_at?: string;
  age?: string;
  swarm_id?: string;
  parent_queen_id?: string;
  capabilities?: string;

  // --- Navigation ---
  page_url?: string;
  pageUrl?: string;
  url?: string;
  file_path?: string;
  filePath?: string;
  path?: string;

  // --- Content metrics ---
  fileSize?: string;
  role?: string;

  // Any additional untyped fields from the backend
  [key: string]: any;
}

export interface Node {
  id: string;
  label: string;
  position: {
    x: number;
    y: number;
    z: number;
  };
  metadata?: NodeMetadata;
}

export interface Edge {
  id: string;
  source: string;
  target: string;
  label?: string;
  weight?: number;
  edgeType?: string;
  metadata?: Record<string, any>;
}

export interface GraphData {
  nodes: Node[];
  edges: Edge[];
}

// Force-directed physics settings — retained for API compatibility.
// Client-side force simulation is REMOVED: the server (Rust/CUDA GPU physics)
// is the single source of truth for all graph types. The client only performs
// optimistic interpolation/tweening toward server-provided target positions.
export interface ForcePhysicsSettings {
  repulsionStrength: number;
  attractionStrength: number;
  centerGravity: number;
  damping: number;
  maxVelocity: number;
  idealEdgeLength: number;
  theta: number;
  enabled: boolean;
  alpha: number;
  alphaDecay: number;
  alphaMin: number;
  clusterStrength: number;
  enableClustering: boolean;
}

export interface TweenSettings {
  enabled: boolean;
  /** Chase time constant in milliseconds; lower is faster. */
  responseMs: number;
  snapThreshold: number;
  maxDivergence: number;
}

export interface PhysicsSettings {
  springStrength: number;
  damping: number;
  maxVelocity: number;
  updateThreshold: number;
}
