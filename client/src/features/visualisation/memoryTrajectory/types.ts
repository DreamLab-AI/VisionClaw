/**
 * Types for the memory trajectory engine: an in-browser HNSW over the
 * snapshot's real vectors that records every step of a search, so the scene
 * can draw the route a query takes through memory.
 *
 * The search, tree and layout logic is adapted from the RuVector Explorer
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence).
 *
 * The local index is built over the sample, not the sidecar's own HNSW graph,
 * so a drawn route illustrates how HNSW search moves through these vectors; it
 * is not the sidecar's literal path. The UI shows the sidecar's own top-k next
 * to it and measures recall against exact search over the same sample.
 */

/** Row-major vectors, each row L2-normalised; distance is `1 - dot`. */
export interface VectorSet {
  count: number;
  dim: number;
  data: Float32Array;
}

export interface HnswParams {
  /** max links per node on layers above 0 (layer 0 allows 2 * M) */
  M: number;
  efConstruction: number;
  /** seed for level assignment, so a snapshot always builds the same graph */
  seed: number;
}

export interface HnswGraph {
  /** entry point node */
  ep: number;
  /** top layer index */
  maxL: number;
  /** level of each node */
  levels: Uint8Array;
  /** links[node][layer] = neighbour ids */
  links: number[][][];
  params: HnswParams;
}

/** One recorded search step. */
export type TraceEvent =
  | { t: 'pop'; n: number; l: number; d: number }
  | { t: 'eval'; from: number; n: number; l: number; d: number; ok: boolean; learned?: boolean };

export interface SearchResult {
  /** best k node ids, nearest first */
  top: number[];
  /** the whole final beam on layer 0, nearest first */
  beam: number[];
  /** greedy hops taken on the upper layers, top layer first */
  hops: number[];
  distanceEvals: number;
  /** empty unless tracing was requested */
  trace: TraceEvent[];
}

export interface SearchTreeNode {
  id: number;
  /** parent node id, -1 for the root */
  parent: number;
  depth: number;
  /** HNSW layer the node was reached on */
  layer: number;
  /** entered the beam (or was popped); false = evaluated and rejected */
  kept: boolean;
  /** injected by the learning loop rather than reached by a link */
  learned: boolean;
  distance: number;
  /** discovery order */
  order: number;
  /** discovery rank among kept nodes */
  rank: number;
  children: SearchTreeNode[];
  /** size of the kept subtree rooted here (1 for a leaf) */
  subtreeSize: number;
  /** horizontal leaf coordinate used by the tree and hyper layouts */
  leafX: number;
  /** centre of the angular sector owned by this node (canopy), radians */
  angle: number;
  /** deterministic [0, 1) jitter from the node id */
  hash: number;
}

export interface SearchTree {
  root: number;
  nodes: Map<number, SearchTreeNode>;
  /** nodes in discovery order */
  list: SearchTreeNode[];
  /** root → … → best result */
  path: number[];
  maxDepth: number;
  leaves: number;
  keptTotal: number;
}

export type TrajectoryView = 'space' | 'canopy' | 'tree' | 'hyper';

export type Vec3 = [number, number, number];

export interface LayoutOptions {
  view: TrajectoryView;
  /** 'space' view: cloud positions (3 * count) to place nodes at */
  spacePositions?: Float32Array | number[];
  /** overall radius of the canopy / tree / hyper layouts in scene units */
  radius: number;
  /** hyper view: geodesic step per tree depth (0.2..0.9, default 0.55) */
  hyperStep?: number;
  /** hyper view: Möbius focus point inside the unit disk, default [0, 0] */
  hyperFocus?: [number, number];
}

export interface LayoutResult {
  positions: Map<number, Vec3>;
  /** quadratic Bézier control point for the edge parent → node */
  controls: Map<number, Vec3>;
  /** hyper view only: sampled geodesic from parent to node, endpoints included */
  geodesics?: Map<number, Vec3[]>;
}

export interface LearningMemoryEntry {
  q: Float32Array;
  ids: number[];
  hits: number;
}

export interface LearningState {
  enabled: boolean;
  memory: LearningMemoryEntry[];
  /** max remembered queries */
  capacity: number;
  /** remembered queries consulted per new query */
  hintsPerQuery: number;
  targetRecall: number;
  learningRate: number;
  /** current ef (beam breadth); null until the first query */
  breadth: number | null;
  controller: 'step' | 'ewma';
  ewma: number | null;
  /** how often each node served as an upper-layer hop */
  hubCounts: Map<number, number>;
}

export interface QueryRun {
  result: SearchResult;
  tree: SearchTree;
  /** exact top-k over the same sample */
  exactTop: number[];
  /** |top ∩ exactTop| / k */
  recall: number;
  /** ef used for this query */
  breadth: number;
  hintsUsed: number[];
}
