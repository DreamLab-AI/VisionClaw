// Custom hook for workspace data management with caching and real-time updates
import { useState, useEffect, useCallback, useRef } from 'react';
import { workspaceApi, Workspace, CreateWorkspaceRequest, UpdateWorkspaceRequest, WorkspaceApiError } from '@/api/workspaceApi';
import { createLogger } from '@/utils/loggerConfig';

const logger = createLogger('useWorkspaces');

export interface UseWorkspacesState {
  workspaces: Workspace[];
  loading: boolean;
  error: string | null;
  hasMore: boolean;
  total: number;
}

export interface UseWorkspacesFilters {
  status?: 'active' | 'archived';
  type?: 'personal' | 'team' | 'public';
  favorite?: boolean;
}

export interface UseWorkspacesOptions {
  initialLoad?: boolean;
  pageSize?: number;
  enableRealtime?: boolean;
  cacheTimeout?: number;
}

interface WorkspaceCache {
  data: Workspace[];
  timestamp: number;
  filters: UseWorkspacesFilters;
}

export const useWorkspaces = (options: UseWorkspacesOptions = {}) => {
  const {
    initialLoad = true,
    pageSize = 20,
    enableRealtime = true,
    cacheTimeout = 5 * 60 * 1000, 
  } = options;

  
  const [state, setState] = useState<UseWorkspacesState>({
    workspaces: [],
    loading: false,
    error: null,
    hasMore: true,
    total: 0,
  });

  
  const [filters, setFilters] = useState<UseWorkspacesFilters>({});
  const [currentPage, setCurrentPage] = useState(1);


  const abortControllerRef = useRef<AbortController | null>(null);
  const cacheRef = useRef<WorkspaceCache | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const hasFetchedRef = useRef(false);
  const wsReconnectAttemptsRef = useRef(0);

  
  const isValidCache = useCallback((cache: WorkspaceCache | null): boolean => {
    if (!cache) return false;

    const isExpired = Date.now() - cache.timestamp > cacheTimeout;
    const filtersMatch = JSON.stringify(cache.filters) === JSON.stringify(filters);

    return !isExpired && filtersMatch;
  }, [filters, cacheTimeout]);

  
  const fetchWorkspaces = useCallback(async (
    page: number = 1,
    appendToExisting: boolean = false
  ) => {
    
    if (!appendToExisting && isValidCache(cacheRef.current)) {
      logger.info('Using cached workspace data');
      setState(prev => ({
        ...prev,
        workspaces: cacheRef.current!.data,
        loading: false,
        error: null,
      }));
      return;
    }

    
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }

    abortControllerRef.current = new AbortController();

    setState(prev => ({
      ...prev,
      loading: true,
      error: null,
    }));

    try {
      logger.info('Fetching workspaces', { page, filters, appendToExisting });

      const response = await workspaceApi.fetchWorkspaces({
        page,
        limit: pageSize,
        ...filters,
      });

      setState(prev => ({
        ...prev,
        workspaces: appendToExisting
          ? [...prev.workspaces, ...response.workspaces]
          : response.workspaces,
        loading: false,
        error: null,
        hasMore: response.hasMore,
        total: response.total,
      }));

      
      if (!appendToExisting) {
        cacheRef.current = {
          data: response.workspaces,
          timestamp: Date.now(),
          filters: { ...filters },
        };
      }

      logger.info('Workspaces fetched successfully', {
        count: response.workspaces.length,
        total: response.total,
        hasMore: response.hasMore,
      });

    } catch (error) {
      if ((error as Error).name === 'AbortError') return;

      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to fetch workspaces';

      logger.error('Failed to fetch workspaces', { error: errorMessage, page, filters });

      setState(prev => ({
        ...prev,
        loading: false,
        error: errorMessage,
      }));
    }
  }, [filters, pageSize, isValidCache]);

  
  const loadMore = useCallback(async () => {
    if (state.loading || !state.hasMore) return;

    const nextPage = currentPage + 1;
    setCurrentPage(nextPage);
    await fetchWorkspaces(nextPage, true);
  }, [state.loading, state.hasMore, currentPage, fetchWorkspaces]);

  
  const refresh = useCallback(async () => {
    cacheRef.current = null;
    setCurrentPage(1);
    await fetchWorkspaces(1, false);
  }, [fetchWorkspaces]);

  
  const updateFilters = useCallback(async (newFilters: UseWorkspacesFilters) => {
    logger.info('Updating workspace filters', { newFilters });
    setFilters(newFilters);
    setCurrentPage(1);
    cacheRef.current = null; 
  }, []);

  
  const optimisticallyUpdateWorkspace = useCallback((
    workspaceId: string,
    updates: Partial<Workspace> | UpdateWorkspaceRequest
  ) => {
    setState(prev => ({
      ...prev,
      workspaces: prev.workspaces.map(workspace =>
        workspace.id === workspaceId
          ? { ...workspace, ...updates } as Workspace
          : workspace
      ),
    }));


    if (cacheRef.current) {
      cacheRef.current.data = cacheRef.current.data.map(workspace =>
        workspace.id === workspaceId
          ? { ...workspace, ...updates } as Workspace
          : workspace
      );
    }
  }, []);

  
  const createWorkspace = useCallback(async (data: CreateWorkspaceRequest): Promise<Workspace> => {
    try {
      logger.info('Creating workspace', { name: data.name });

      const newWorkspace = await workspaceApi.createWorkspace(data);

      
      setState(prev => ({
        ...prev,
        workspaces: [newWorkspace, ...prev.workspaces],
        total: prev.total + 1,
      }));

      
      if (cacheRef.current) {
        cacheRef.current.data = [newWorkspace, ...cacheRef.current.data];
      }

      logger.info('Workspace created successfully', { id: newWorkspace.id });
      return newWorkspace;

    } catch (error) {
      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to create workspace';

      logger.error('Failed to create workspace', { error: errorMessage, data });
      throw new Error(errorMessage);
    }
  }, []);

  
  const updateWorkspace = useCallback(async (
    id: string,
    data: UpdateWorkspaceRequest
  ): Promise<Workspace> => {
    
    optimisticallyUpdateWorkspace(id, data);

    try {
      logger.info('Updating workspace', { id, updates: Object.keys(data) });

      const updatedWorkspace = await workspaceApi.updateWorkspace(id, data);

      
      optimisticallyUpdateWorkspace(id, updatedWorkspace);

      logger.info('Workspace updated successfully', { id });
      return updatedWorkspace;

    } catch (error) {
      
      await refresh();

      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to update workspace';

      logger.error('Failed to update workspace', { error: errorMessage, id, data });
      throw new Error(errorMessage);
    }
  }, [optimisticallyUpdateWorkspace, refresh]);

  
  const deleteWorkspace = useCallback(async (id: string): Promise<void> => {
    try {
      logger.info('Deleting workspace', { id });

      await workspaceApi.deleteWorkspace(id);

      
      setState(prev => ({
        ...prev,
        workspaces: prev.workspaces.filter(workspace => workspace.id !== id),
        total: prev.total - 1,
      }));

      
      if (cacheRef.current) {
        cacheRef.current.data = cacheRef.current.data.filter(workspace => workspace.id !== id);
      }

      logger.info('Workspace deleted successfully', { id });

    } catch (error) {
      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to delete workspace';

      logger.error('Failed to delete workspace', { error: errorMessage, id });
      throw new Error(errorMessage);
    }
  }, []);

  
  const toggleFavorite = useCallback(async (id: string): Promise<void> => {
    const workspace = state.workspaces.find(w => w.id === id);
    if (!workspace) return;

    
    optimisticallyUpdateWorkspace(id, { favorite: !workspace.favorite });

    try {
      logger.info('Toggling workspace favorite', { id, currentFavorite: workspace.favorite });

      const updatedWorkspace = await workspaceApi.toggleFavorite(id);

      
      optimisticallyUpdateWorkspace(id, { favorite: updatedWorkspace.favorite });

      logger.info('Workspace favorite toggled successfully', { id, favorite: updatedWorkspace.favorite });

    } catch (error) {
      
      optimisticallyUpdateWorkspace(id, { favorite: workspace.favorite });

      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to toggle favorite';

      logger.error('Failed to toggle workspace favorite', { error: errorMessage, id });
      throw new Error(errorMessage);
    }
  }, [state.workspaces, optimisticallyUpdateWorkspace]);

  
  const archiveWorkspace = useCallback(async (id: string, archive: boolean = true): Promise<void> => {
    
    optimisticallyUpdateWorkspace(id, { status: archive ? 'archived' : 'active' });

    try {
      logger.info('Updating workspace archive status', { id, archive });

      const updatedWorkspace = await workspaceApi.archiveWorkspace(id, archive);

      
      optimisticallyUpdateWorkspace(id, { status: updatedWorkspace.status });

      logger.info('Workspace archive status updated', { id, status: updatedWorkspace.status });

    } catch (error) {
      
      await refresh();

      const errorMessage = error instanceof WorkspaceApiError
        ? error.message
        : 'Failed to update archive status';

      logger.error('Failed to update workspace archive status', { error: errorMessage, id, archive });
      throw new Error(errorMessage);
    }
  }, [optimisticallyUpdateWorkspace, refresh]);

  
  useEffect(() => {
    if (!enableRealtime) return;

    const connectWebSocket = () => {
      try {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/workspaces`;

        logger.info('Connecting to workspace WebSocket', { url: wsUrl });

        wsRef.current = new WebSocket(wsUrl);

        wsRef.current.onopen = () => {
          logger.info('Workspace WebSocket connected');
          wsReconnectAttemptsRef.current = 0;
        };

        wsRef.current.onmessage = (event) => {
          try {
            const message = JSON.parse(event.data);
            logger.info('Received workspace update', { type: message.type, data: message.data });

            switch (message.type) {
              case 'workspace_updated':
                optimisticallyUpdateWorkspace(message.data.id, message.data);
                break;
              case 'workspace_created':
                setState(prev => ({
                  ...prev,
                  workspaces: [message.data, ...prev.workspaces],
                  total: prev.total + 1,
                }));
                break;
              case 'workspace_deleted':
                setState(prev => ({
                  ...prev,
                  workspaces: prev.workspaces.filter(w => w.id !== message.data.id),
                  total: prev.total - 1,
                }));
                break;
              default:
                logger.warn('Unknown workspace update type', { type: message.type });
            }
          } catch (error) {
            logger.error('Failed to parse WebSocket message', { error: (error as Error).message });
          }
        };

        wsRef.current.onclose = (event) => {
          logger.info('Workspace WebSocket disconnected', { code: event.code, reason: event.reason });

          const maxRetries = 5;
          if (event.code !== 1000 && wsReconnectAttemptsRef.current < maxRetries) {
            const attempt = wsReconnectAttemptsRef.current;
            wsReconnectAttemptsRef.current = attempt + 1;
            const delay = Math.min(5000 * Math.pow(2, attempt), 40000);
            setTimeout(connectWebSocket, delay);
          }
        };

        wsRef.current.onerror = (error) => {
          logger.error('Workspace WebSocket error', { error });
        };

      } catch (error) {
        logger.error('Failed to connect workspace WebSocket', { error: (error as Error).message });
      }
    };

    connectWebSocket();

    return () => {
      if (wsRef.current) {
        wsRef.current.close(1000, 'Component unmounting');
        wsRef.current = null;
      }
    };
  }, [enableRealtime, optimisticallyUpdateWorkspace]);


  useEffect(() => {
    if (initialLoad && !hasFetchedRef.current) {
      hasFetchedRef.current = true;
      fetchWorkspaces();
    }
  }, [initialLoad, fetchWorkspaces]);

  
  useEffect(() => {
    if (filters) {
      fetchWorkspaces();
    }
  }, [filters, fetchWorkspaces]);

  
  useEffect(() => {
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  return {
    
    ...state,
    filters,
    currentPage,

    
    fetchWorkspaces,
    loadMore,
    refresh,
    updateFilters,

    
    createWorkspace,
    updateWorkspace,
    deleteWorkspace,
    toggleFavorite,
    archiveWorkspace,

    
    activeWorkspaces: state.workspaces.filter(w => w.status === 'active'),
    archivedWorkspaces: state.workspaces.filter(w => w.status === 'archived'),
    favoriteWorkspaces: state.workspaces.filter(w => w.favorite),
  };
};

export default useWorkspaces;