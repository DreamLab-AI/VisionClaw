/**
 * Semantic Service Hook
 *
 * Provides access to semantic analysis API endpoints from the Phase 5 Semantic API.
 * Integrates with /api/semantic/* endpoints for advanced graph analytics.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import { unifiedApiClient } from '@/services/api/UnifiedApiClient';
import { createLogger } from '@/utils/loggerConfig';

const logger = createLogger('useSemanticService');

export interface CommunitiesRequest {
  algorithm: 'louvain' | 'label_propagation' | 'connected_components' | 'hierarchical';
  min_cluster_size?: number;
}

export interface CommunitiesResponse {
  clusters: Record<number, number>; // node_id -> cluster_id
  cluster_sizes: Record<number, number>; // cluster_id -> size
  modularity: number;
  computation_time_ms: number;
}

export interface CentralityRequest {
  algorithm: 'pagerank' | 'betweenness' | 'closeness';
  damping?: number; // For PageRank
  max_iterations?: number;
  top_k?: number;
}

export interface CentralityResponse {
  scores: Record<number, number>; // node_id -> score
  algorithm: string;
  top_nodes: Array<[number, number]>; // [node_id, score]
}

export interface ShortestPathRequest {
  source_node_id: number;
  target_node_id?: number;
  include_path?: boolean;
}

export interface ShortestPathResponse {
  source_node: number;
  distances: Record<number, number>; // node_id -> distance
  paths: Record<number, number[]>; // node_id -> path
  computation_time_ms: number;
}

export interface GenerateConstraintsRequest {
  similarity_threshold?: number;
  enable_clustering?: boolean;
  enable_importance?: boolean;
  enable_topic?: boolean;
  max_constraints?: number;
}

export interface SemanticStatistics {
  total_analyses: number;
  average_clustering_time_ms: number;
  average_pathfinding_time_ms: number;
  cache_hit_rate: number;
  gpu_memory_used_mb: number;
}

export interface UseSemanticServiceOptions {
  /** Enable automatic statistics polling */
  pollStatistics?: boolean;
  /** Statistics polling interval in ms. Default: 5000 */
  pollInterval?: number;
}

/**
 * Hook for accessing the Semantic API (/api/semantic/*)
 *
 * Provides:
 * - Community detection (Louvain, Label Propagation, etc.)
 * - Centrality computation (PageRank, Betweenness, Closeness)
 * - Shortest path finding
 * - Semantic constraint generation
 * - Performance statistics
 */
export function useSemanticService(options: UseSemanticServiceOptions = {}) {
  const { pollStatistics = true, pollInterval = 5000 } = options;

  const [statistics, setStatistics] = useState<SemanticStatistics | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pollIntervalRef = useRef<NodeJS.Timeout | null>(null);
  const isMountedRef = useRef(true);

  // Poll statistics periodically
  useEffect(() => {
    isMountedRef.current = true;

    const fetchStatistics = async () => {
      try {
        const response = await unifiedApiClient.get<SemanticStatistics>('/semantic/statistics');
        if (isMountedRef.current) {
          setStatistics(response.data);
        }
      } catch (err: any) {
        if (isMountedRef.current) {
          logger.warn('Failed to fetch semantic statistics:', err);
        }
      }
    };

    if (pollStatistics) {
      // Initial fetch
      fetchStatistics();

      // Set up polling
      pollIntervalRef.current = setInterval(fetchStatistics, pollInterval);
    }

    return () => {
      isMountedRef.current = false;
      if (pollIntervalRef.current) {
        clearInterval(pollIntervalRef.current);
      }
    };
  }, [pollStatistics, pollInterval]);

  /**
   * Detect communities in the graph
   */
  const detectCommunities = useCallback(
    async (request: CommunitiesRequest): Promise<CommunitiesResponse> => {
      setLoading(true);
      setError(null);
      try {
        const response = await unifiedApiClient.post<CommunitiesResponse>(
          '/semantic/communities',
          request
        );

        if (isMountedRef.current) {
          setLoading(false);
          logger.info('Communities detected:', response.data);
        }

        return response.data;
      } catch (err: any) {
        if (isMountedRef.current) {
          const errorMsg = err.message || 'Failed to detect communities';
          setError(errorMsg);
          setLoading(false);
          logger.error('Failed to detect communities:', err);
        }
        throw err;
      }
    },
    []
  );

  /**
   * Compute centrality scores
   */
  const computeCentrality = useCallback(
    async (request: CentralityRequest): Promise<CentralityResponse> => {
      setLoading(true);
      setError(null);
      try {
        const response = await unifiedApiClient.post<CentralityResponse>(
          '/semantic/centrality',
          request
        );

        if (isMountedRef.current) {
          setLoading(false);
          logger.info('Centrality computed:', response.data);
        }

        return response.data;
      } catch (err: any) {
        if (isMountedRef.current) {
          const errorMsg = err.message || 'Failed to compute centrality';
          setError(errorMsg);
          setLoading(false);
          logger.error('Failed to compute centrality:', err);
        }
        throw err;
      }
    },
    []
  );

  /**
   * Compute shortest paths
   */
  const computeShortestPath = useCallback(
    async (request: ShortestPathRequest): Promise<ShortestPathResponse> => {
      setLoading(true);
      setError(null);
      try {
        const response = await unifiedApiClient.post<ShortestPathResponse>(
          '/semantic/shortest-path',
          request
        );

        if (isMountedRef.current) {
          setLoading(false);
          logger.info('Shortest path computed:', response.data);
        }

        return response.data;
      } catch (err: any) {
        if (isMountedRef.current) {
          const errorMsg = err.message || 'Failed to compute shortest path';
          setError(errorMsg);
          setLoading(false);
          logger.error('Failed to compute shortest path:', err);
        }
        throw err;
      }
    },
    []
  );

  /**
   * Generate semantic constraints
   */
  const generateConstraints = useCallback(
    async (request: GenerateConstraintsRequest): Promise<{ constraint_count: number; status: string }> => {
      setLoading(true);
      setError(null);
      try {
        const response = await unifiedApiClient.post<{ constraint_count: number; status: string }>(
          '/semantic/constraints/generate',
          request
        );

        if (isMountedRef.current) {
          setLoading(false);
          logger.info('Constraints generated:', response.data);
        }

        return response.data;
      } catch (err: any) {
        if (isMountedRef.current) {
          const errorMsg = err.message || 'Failed to generate constraints';
          setError(errorMsg);
          setLoading(false);
          logger.error('Failed to generate constraints:', err);
        }
        throw err;
      }
    },
    []
  );

  /**
   * Invalidate analysis cache
   */
  const invalidateCache = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await unifiedApiClient.post('/semantic/cache/invalidate', {});

      if (isMountedRef.current) {
        setLoading(false);
        logger.info('Semantic cache invalidated');
      }
    } catch (err: any) {
      if (isMountedRef.current) {
        const errorMsg = err.message || 'Failed to invalidate cache';
        setError(errorMsg);
        setLoading(false);
        logger.error('Failed to invalidate cache:', err);
      }
      throw err;
    }
  }, []);

  /**
   * Manually refresh statistics
   */
  const refreshStatistics = useCallback(async () => {
    try {
      const response = await unifiedApiClient.get<SemanticStatistics>('/semantic/statistics');
      if (isMountedRef.current) {
        setStatistics(response.data);
      }
    } catch (err: any) {
      if (isMountedRef.current) {
        logger.warn('Failed to refresh statistics:', err);
      }
    }
  }, []);

  return {
    // State
    statistics,
    loading,
    error,

    // Actions
    detectCommunities,
    computeCentrality,
    computeShortestPath,
    generateConstraints,
    invalidateCache,
    refreshStatistics,
  };
}
