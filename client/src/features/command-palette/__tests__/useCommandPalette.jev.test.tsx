import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

const rank = vi.fn();
vi.mock('../../../services/jev/jevClient', async (orig) => {
  const real = await orig<typeof import('../../../services/jev/jevClient')>();
  return { ...real, jevRankPalette: (...a: unknown[]) => rank(...a) };
});
vi.mock('../../../hooks/useKeyboardShortcuts', () => ({ useKeyboardShortcuts: () => {}, formatShortcut: () => '' }));

import { useCommandPalette } from '../hooks/useCommandPalette';
import { commandRegistry } from '../CommandRegistry';

const h = () => {};

describe('command palette Jev option (VC-34-S1-02)', () => {
  beforeEach(() => {
    rank.mockReset();
    for (const c of commandRegistry.getAllCommands()) commandRegistry.unregisterCommand(c.id);
    commandRegistry.registerCommands([
      { id: 'settings.undo', title: 'Undo Settings Change', description: 'Revert', category: 'settings', handler: h },
      { id: 'view.theme', title: 'Toggle Theme', category: 'view', handler: h },
      { id: 'sys.off', title: 'Disabled thing', category: 'system', handler: h, enabled: false },
    ]);
  });

  async function search(q: string) {
    const hook = renderHook(() => useCommandPalette());
    act(() => hook.result.current.open());
    act(() => hook.result.current.setSearchQuery(q));
    return hook;
  }

  it.each([['null (flag off / error / timeout / malformed)', null], ['no match', { ranked: [], noMatch: true }]])(
    'ranking %s ⇒ lexical results only, identical to pre-change',
    async (_n, ranking) => {
      rank.mockResolvedValue(ranking);
      const { result } = await search('theme');
      await waitFor(() => expect(rank).toHaveBeenCalled(), { timeout: 2000 });
      expect(result.current.suggestedCommands).toEqual([]);
      expect(result.current.filteredCommands.map((c) => c.id)).toEqual(commandRegistry.searchCommands('theme').map((c) => c.id));
    },
  );

  it('accept: adds enabled, unseen suggestions after lexical results; never the disabled one', async () => {
    rank.mockResolvedValue({
      ranked: [{ id: 'settings.undo', score: 1.9 }, { id: 'sys.off', score: 2 }, { id: 'view.theme', score: 1.7 }],
      noMatch: false,
    });
    const { result } = await search('theme');
    await waitFor(() => expect(result.current.suggestedCommands.map((c) => c.id)).toEqual(['settings.undo']), { timeout: 2000 });
    // Lexical hit unchanged and first.
    expect(result.current.filteredCommands.map((c) => c.id)).toEqual(['view.theme']);
    // Wire: only enabled commands are offered to Jev.
    const sent = rank.mock.calls[0][1] as Array<{ id: string }>;
    expect(sent.map((c) => c.id).sort()).toEqual(['settings.undo', 'view.theme']);
  });

  it('a rejected ranking promise cannot break the palette', async () => {
    rank.mockRejectedValue(new Error('x'));
    const { result } = await search('theme');
    await new Promise((r) => setTimeout(r, 500));
    expect(result.current.suggestedCommands).toEqual([]);
    expect(result.current.filteredCommands.length).toBe(1);
  });
});
