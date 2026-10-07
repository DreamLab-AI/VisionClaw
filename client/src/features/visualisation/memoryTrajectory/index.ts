/**
 * Memory trajectory engine: an in-browser HNSW over the snapshot's vectors
 * that records each search step, turns it into a discovery tree, lays that
 * tree out in 3-D and learns from every query.
 *
 * Adapted from the RuVector Explorer (https://github.com/ruvnet/RuVector,
 * docs/explorer, MIT licence).
 */
export type * from './types';
export { buildHnsw, searchHnsw, exactTopK, recallAtK, HnswBuilder, MAX_LEVEL } from './hnsw';
export type { BuildOptions, SearchOptions } from './hnsw';
export { buildSearchTree, CANOPY_HALF_ANGLE } from './tree';
export { layoutTree, mobius, mobiusInverse, DEFAULT_HYPER_STEP, GEODESIC_SEGMENTS } from './layout';
export type { Complex } from './layout';
export { interpolateLayouts, easeInOutCubic } from './morph';
export type { MorphFrame, ParentSource } from './morph';
export {
  createLearningState,
  recallHints,
  learnFrom,
  serializeLearning,
  restoreLearning,
  breadthBounds,
  MAX_BREADTH,
} from './learning';
export {
  createTrajectoryEngine,
  learningStorageKey,
  DEFAULT_HNSW_PARAMS,
  DEFAULT_EF,
  DEFAULT_K,
} from './engine';
export type { TrajectoryEngine, TrajectoryEngineOptions, TrajectoryQueryOptions } from './engine';
export { packGraph, unpackGraph } from './graphCodec';
export type { PackedGraph } from './graphCodec';
