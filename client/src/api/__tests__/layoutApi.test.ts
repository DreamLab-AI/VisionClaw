// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * ADR-2140: the layout mode is the stored `physics.layoutMode`, written under the engine's
 * `LayoutMode` serde names. There is no vocabulary bridge: POST /api/layout/mode takes the
 * same name the store holds, and `dagLeftRight` / `typeClustering` are modes of their own
 * (they used to be remapped to `hierarchical` / `forceDirected`, which hid them).
 */

const post = vi.fn();
vi.mock('axios', () => ({
  default: { post: (...a: unknown[]) => post(...a), get: vi.fn() },
}));

import { layoutApi, LAYOUT_MODES } from '../layoutApi';

// The engine's accepted serde names (crates/visionclaw-domain/src/types/layout.rs).
const ENGINE_MODES = [
  'forceDirected', 'hierarchical', 'radial', 'spectral', 'temporal', 'clustered',
  'dagLeftRight', 'typeClustering',
];

describe('layoutApi.setMode', () => {
  beforeEach(() => post.mockReset().mockResolvedValue({ data: { success: true } }));

  it('posts the stored name verbatim', async () => {
    await layoutApi.setMode('forceDirected', 800);
    expect(post).toHaveBeenCalledWith('/api/layout/mode', { mode: 'forceDirected', transitionMs: 800 });
  });

  it('offers only modes the engine deserialises, each as itself', () => {
    for (const m of LAYOUT_MODES) {
      expect(ENGINE_MODES).toContain(m);
    }
    expect(new Set(LAYOUT_MODES).size).toBe(7);
  });

  it('does not remap the two modes that used to be folded into others', async () => {
    await layoutApi.setMode('dagLeftRight', 500);
    expect(post).toHaveBeenLastCalledWith('/api/layout/mode', { mode: 'dagLeftRight', transitionMs: 500 });
    await layoutApi.setMode('typeClustering', 500);
    expect(post).toHaveBeenLastCalledWith('/api/layout/mode', { mode: 'typeClustering', transitionMs: 500 });
  });
});
