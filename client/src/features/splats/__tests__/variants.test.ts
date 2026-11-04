import { describe, expect, it } from 'vitest';
import { chooseVariant } from '../variants';
import { VARIANT_NAMES } from '../types';
import { assetWith } from './fixtures';

describe('chooseVariant', () => {
  it('prefers desktop-hi, then desktop, on the WebGPU path', () => {
    expect(chooseVariant(assetWith(['xr-1', 'desktop', 'desktop-hi']), 'webgpu')?.name).toBe('desktop-hi');
    expect(chooseVariant(assetWith(['xr-3', 'desktop']), 'webgpu')?.name).toBe('desktop');
  });

  it('uses desktop on the WebGL path even when desktop-hi exists', () => {
    expect(chooseVariant(assetWith(['desktop-hi', 'desktop']), 'webgl')?.name).toBe('desktop');
  });

  it('shows nothing when no desktop variant exists', () => {
    expect(chooseVariant(assetWith(['xr-3', 'xr-2', 'xr-1']), 'webgpu')).toBeNull();
    expect(chooseVariant(assetWith(['xr-3', 'xr-2', 'xr-1']), 'webgl')).toBeNull();
    expect(chooseVariant(assetWith(['desktop-hi']), 'webgl')).toBeNull();
    expect(chooseVariant(assetWith([]), 'webgpu')).toBeNull();
  });

  it('never returns an xr-* variant, whatever the asset lists', () => {
    for (const path of ['webgpu', 'webgl'] as const) {
      const chosen = chooseVariant(assetWith([...VARIANT_NAMES]), path);
      expect(chosen?.name.startsWith('xr-')).toBe(false);
    }
  });
});
