// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * ADR-2140 (R13 F1): `qualityGates.layoutMode` is a deprecated read-only echo of
 * `physics.layoutMode`. Toggling a gate GETs the gates, merges one field and PUTs the object;
 * the echo must not ride back, or the toggle would name (and so rewrite) the layout mode.
 */

const get = vi.fn();
const put = vi.fn();
vi.mock('axios', () => ({
  default: {
    get: (...a: unknown[]) => get(...a),
    put: (...a: unknown[]) => put(...a),
    post: vi.fn(),
    interceptors: { request: { use: vi.fn() }, response: { use: vi.fn() } },
  },
}));

import { updateQualityGates } from '../endpoints';

describe('updateQualityGates', () => {
  beforeEach(() => {
    get.mockReset().mockResolvedValue({
      data: { showClusters: false, gpuAcceleration: true, layoutMode: 'force-directed' },
    });
    put.mockReset().mockResolvedValue({ data: {} });
  });

  it('does not send the echoed layoutMode back when a gate is toggled', async () => {
    await updateQualityGates({ showClusters: true });
    const [, body] = put.mock.calls[0];
    expect(body).toEqual({ showClusters: true, gpuAcceleration: true });
    expect('layoutMode' in body).toBe(false);
  });

  it('keeps a layoutMode the caller names itself', async () => {
    await updateQualityGates({ layoutMode: 'spectral' });
    expect(put.mock.calls[0][1].layoutMode).toBe('spectral');
  });
});
