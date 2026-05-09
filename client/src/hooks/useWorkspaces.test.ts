import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

vi.mock('../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

vi.mock('@/api/workspaceApi', () => ({
  workspaceApi: {
    fetchWorkspaces: vi.fn(),
    createWorkspace: vi.fn(),
    updateWorkspace: vi.fn(),
    deleteWorkspace: vi.fn(),
    toggleFavorite: vi.fn(),
    archiveWorkspace: vi.fn(),
  },
  WorkspaceApiError: class WorkspaceApiError extends Error {
    constructor(message: string) {
      super(message);
      this.name = 'WorkspaceApiError';
    }
  },
}));

import { useWorkspaces } from './useWorkspaces';
import { workspaceApi } from '@/api/workspaceApi';

const mockWorkspaceApi = workspaceApi as {
  fetchWorkspaces: ReturnType<typeof vi.fn>;
  createWorkspace: ReturnType<typeof vi.fn>;
  updateWorkspace: ReturnType<typeof vi.fn>;
  deleteWorkspace: ReturnType<typeof vi.fn>;
  toggleFavorite: ReturnType<typeof vi.fn>;
  archiveWorkspace: ReturnType<typeof vi.fn>;
};

const makeWorkspace = (id: string, overrides = {}) => ({
  id,
  name: `Workspace ${id}`,
  description: '',
  status: 'active' as const,
  type: 'personal' as const,
  favorite: false,
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
  ...overrides,
});

describe('useWorkspaces', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockWorkspaceApi.fetchWorkspaces.mockResolvedValue({
      workspaces: [makeWorkspace('1'), makeWorkspace('2')],
      total: 2,
      hasMore: false,
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns initial state structure', () => {
    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: false, enableRealtime: false }),
    );

    expect(result.current).toHaveProperty('workspaces');
    expect(result.current).toHaveProperty('loading');
    expect(result.current).toHaveProperty('error');
    expect(result.current).toHaveProperty('hasMore');
    expect(result.current).toHaveProperty('total');
  });

  it('fetches workspaces on mount when initialLoad is true', async () => {
    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: true, enableRealtime: false }),
    );

    await vi.waitFor(() => {
      expect(result.current.workspaces.length).toBe(2);
    });

    expect(mockWorkspaceApi.fetchWorkspaces).toHaveBeenCalled();
  });

  it('creates workspace and prepends to list', async () => {
    const newWs = makeWorkspace('3', { name: 'New WS' });
    mockWorkspaceApi.createWorkspace.mockResolvedValueOnce(newWs);

    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: false, enableRealtime: false }),
    );

    await act(async () => {
      const created = await result.current.createWorkspace({ name: 'New WS' } as any);
      expect(created.id).toBe('3');
    });

    expect(result.current.workspaces[0].id).toBe('3');
  });

  it('deletes workspace and removes from list', async () => {
    mockWorkspaceApi.deleteWorkspace.mockResolvedValueOnce(undefined);

    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: true, enableRealtime: false }),
    );

    await vi.waitFor(() => {
      expect(result.current.workspaces.length).toBe(2);
    });

    await act(async () => {
      await result.current.deleteWorkspace('1');
    });

    expect(result.current.workspaces.find((w) => w.id === '1')).toBeUndefined();
  });

  it('handles fetch error gracefully', async () => {
    mockWorkspaceApi.fetchWorkspaces.mockRejectedValue(
      new Error('Network failure'),
    );

    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: true, enableRealtime: false }),
    );

    await vi.waitFor(() => {
      expect(result.current.error).toBe('Failed to fetch workspaces');
    });
  });

  it('exposes computed filtered workspace lists', async () => {
    mockWorkspaceApi.fetchWorkspaces.mockResolvedValue({
      workspaces: [
        makeWorkspace('1', { status: 'active', favorite: true }),
        makeWorkspace('2', { status: 'archived', favorite: false }),
      ],
      total: 2,
      hasMore: false,
    });

    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: true, enableRealtime: false }),
    );

    await vi.waitFor(() => {
      expect(result.current.workspaces.length).toBe(2);
    });

    expect(result.current.activeWorkspaces.length).toBe(1);
    expect(result.current.archivedWorkspaces.length).toBe(1);
    expect(result.current.favoriteWorkspaces.length).toBe(1);
  });

  it('refresh clears cache and re-fetches', async () => {
    const { result } = renderHook(() =>
      useWorkspaces({ initialLoad: true, enableRealtime: false }),
    );

    await vi.waitFor(() => {
      expect(result.current.workspaces.length).toBe(2);
    });

    mockWorkspaceApi.fetchWorkspaces.mockResolvedValueOnce({
      workspaces: [makeWorkspace('1')],
      total: 1,
      hasMore: false,
    });

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.workspaces.length).toBe(1);
  });
});
