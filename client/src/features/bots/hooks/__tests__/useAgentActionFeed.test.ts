/**
 * useAgentActionFeed Hook Tests
 *
 * Exercises the bounded, newest-first ring buffer that backs the live agent
 * action transcript. Uses low-level createRoot + React.act rather than
 * @testing-library's renderHook (React 19 compatibility, matching the pattern
 * in telemetry/__tests__ and visualisation/hooks/__tests__).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import React from 'react';
import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { AgentActionType, type AgentActionEvent } from '@/services/BinaryWebSocketProtocol';
import {
  useAgentActionFeed,
  type UseAgentActionFeedReturn,
} from '../useAgentActionFeed';

// Capture the 'agent-action' handler the hook registers, and the unsubscribe it
// returns, via a hoisted bridge so the vi.mock factory can reference them.
const ws = vi.hoisted(() => {
  let handler: ((data: unknown) => void) | null = null;
  const unsubscribe = vi.fn();
  const on = vi.fn((_event: string, cb: (data: unknown) => void) => {
    handler = cb;
    return unsubscribe;
  });
  return {
    on,
    unsubscribe,
    emit: (data: unknown) => handler?.(data),
    reset: () => {
      handler = null;
    },
  };
});

vi.mock('@/store/websocketStore', () => ({
  useWebSocketStore: (selector: (s: { on: typeof ws.on }) => unknown) => selector({ on: ws.on }),
}));

function makeEvent(overrides: Partial<AgentActionEvent> = {}): AgentActionEvent {
  return {
    sourceAgentId: 1,
    targetNodeId: 100,
    actionType: AgentActionType.Query,
    timestamp: Date.now(),
    durationMs: 500,
    ...overrides,
  };
}

let feedRef: UseAgentActionFeedReturn | null = null;

function FeedTestComponent({ limit }: { limit?: number }) {
  feedRef = useAgentActionFeed({ limit });
  return null;
}

describe('useAgentActionFeed', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    ws.reset();
    feedRef = null;
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    if (root) {
      act(() => root.unmount());
    }
    if (container) {
      document.body.removeChild(container);
    }
  });

  async function render(limit?: number): Promise<UseAgentActionFeedReturn> {
    root = createRoot(container);
    await act(async () => {
      root.render(React.createElement(FeedTestComponent, { limit }));
    });
    if (!feedRef) throw new Error('Hook did not render');
    return feedRef;
  }

  it('subscribes to agent-action on mount', async () => {
    await render();
    expect(ws.on).toHaveBeenCalledTimes(1);
    expect(ws.on).toHaveBeenCalledWith('agent-action', expect.any(Function));
  });

  it('unsubscribes on unmount', async () => {
    await render();
    expect(ws.unsubscribe).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    // Prevent afterEach from unmounting an already-unmounted root.
    root = undefined as unknown as Root;
    expect(ws.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('collects actions newest-first', async () => {
    await render();
    await act(async () => {
      ws.emit([
        makeEvent({ sourceAgentId: 1, timestamp: 1000 }),
        makeEvent({ sourceAgentId: 2, timestamp: 2000 }),
        makeEvent({ sourceAgentId: 3, timestamp: 3000 }),
      ]);
    });

    expect(feedRef!.entries).toHaveLength(3);
    expect(feedRef!.entries.map(e => e.sourceAgentId)).toEqual([3, 2, 1]);
  });

  it('keeps a later batch ahead of an earlier one', async () => {
    await render();
    await act(async () => ws.emit([makeEvent({ sourceAgentId: 10, timestamp: 1000 })]));
    await act(async () => ws.emit([makeEvent({ sourceAgentId: 20, timestamp: 2000 })]));

    expect(feedRef!.entries.map(e => e.sourceAgentId)).toEqual([20, 10]);
  });

  it('caps the ring buffer at the configured limit, dropping oldest', async () => {
    await render(5);
    await act(async () => {
      const batch: AgentActionEvent[] = [];
      for (let i = 0; i < 12; i++) {
        batch.push(makeEvent({ sourceAgentId: i, timestamp: 1000 + i }));
      }
      ws.emit(batch);
    });

    expect(feedRef!.entries).toHaveLength(5);
    // Newest five survive (ids 11..7), newest first.
    expect(feedRef!.entries.map(e => e.sourceAgentId)).toEqual([11, 10, 9, 8, 7]);
  });

  it('caps across multiple batches', async () => {
    await render(3);
    for (let i = 0; i < 6; i++) {
      await act(async () => ws.emit([makeEvent({ sourceAgentId: i, timestamp: 1000 + i })]));
    }

    expect(feedRef!.entries).toHaveLength(3);
    expect(feedRef!.entries.map(e => e.sourceAgentId)).toEqual([5, 4, 3]);
  });

  it('maps action type to a readable name and drops zero duration', async () => {
    await render();
    await act(async () => {
      ws.emit([makeEvent({ actionType: AgentActionType.Create, durationMs: 0 })]);
    });

    expect(feedRef!.entries[0].actionTypeName).toBe('Create');
    expect(feedRef!.entries[0].durationMs).toBeUndefined();
  });

  it('decodes intent and verification from a JSON payload', async () => {
    await render();
    const payload = new TextEncoder().encode(
      JSON.stringify({ intent: 'refactor auth module', verification: 'tests green' }),
    );
    await act(async () => ws.emit([makeEvent({ payload })]));

    expect(feedRef!.entries[0].intent).toBe('refactor auth module');
    expect(feedRef!.entries[0].verification).toBe('tests green');
  });

  it('treats a bare-string payload as intent', async () => {
    await render();
    const payload = new TextEncoder().encode('gathering context');
    await act(async () => ws.emit([makeEvent({ payload })]));

    expect(feedRef!.entries[0].intent).toBe('gathering context');
    expect(feedRef!.entries[0].verification).toBeUndefined();
  });

  it('ignores empty and non-array emissions', async () => {
    await render();
    await act(async () => {
      ws.emit([]);
      ws.emit(null);
      ws.emit(undefined);
    });
    expect(feedRef!.entries).toHaveLength(0);
  });

  it('clear() empties the buffer', async () => {
    await render();
    await act(async () => ws.emit([makeEvent(), makeEvent({ sourceAgentId: 2 })]));
    expect(feedRef!.entries).toHaveLength(2);

    await act(async () => feedRef!.clear());
    expect(feedRef!.entries).toHaveLength(0);
  });
});
