// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * Regression: the persisted `qualityGates.layoutMode` uses the quality-gate
 * vocabulary (`force-directed`, `dag-topdown`, `dag-radial`, `dag-leftright`,
 * `type-clustering` — validated by the server's settings route), but
 * POST /api/layout/mode deserialises the engine's camelCase `LayoutMode` enum
 * (`forceDirected`, `hierarchical`, `radial`, ...). GraphManager posted the
 * stored value verbatim, so every page load drew a 400 and no layout mode was
 * ever applied.
 */

const post = vi.fn();
vi.mock('axios', () => ({
  default: { post: (...a: unknown[]) => post(...a), get: vi.fn() },
}));

import { layoutApi, toEngineLayoutMode, QUALITY_GATE_LAYOUT_MODES } from '../layoutApi';

// The engine's accepted serde names (crates/visionclaw-domain/src/types/layout.rs).
const ENGINE_MODES = ['forceDirected', 'hierarchical', 'radial', 'spectral', 'temporal', 'clustered'];

describe('layoutApi.setMode', () => {
  beforeEach(() => post.mockReset().mockResolvedValue({ data: { success: true } }));

  it('posts the engine mode for the default quality-gate value', async () => {
    await layoutApi.setMode('force-directed', 800);
    expect(post).toHaveBeenCalledWith('/api/layout/mode', { mode: 'forceDirected', transitionMs: 800 });
  });

  it('maps every quality-gate mode to a mode the engine deserialises', () => {
    for (const m of QUALITY_GATE_LAYOUT_MODES) {
      expect(ENGINE_MODES).toContain(toEngineLayoutMode(m));
    }
    expect(toEngineLayoutMode('dag-topdown')).toBe('hierarchical');
    expect(toEngineLayoutMode('dag-leftright')).toBe('hierarchical');
    expect(toEngineLayoutMode('dag-radial')).toBe('radial');
  });

  it('passes engine-vocabulary modes through unchanged', () => {
    for (const m of ENGINE_MODES) {
      expect(toEngineLayoutMode(m)).toBe(m);
    }
  });
});
