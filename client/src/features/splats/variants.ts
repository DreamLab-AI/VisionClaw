import type { SplatAsset, SplatVariant, VariantName } from './types';

/** Which renderer path will draw the splats. */
export type RenderPath = 'webgpu' | 'webgl';

/**
 * Preference order per path (ADR-2170 decision 5). The `xr-*` variants are
 * SH0 headset tiers and are never listed here, so they cannot be chosen.
 */
const PREFERENCE: Record<RenderPath, readonly VariantName[]> = {
  webgpu: ['desktop-hi', 'desktop'],
  webgl: ['desktop'],
};

/** The variant to load for a path, or null when none fits and nothing is shown. */
export function chooseVariant(asset: SplatAsset, path: RenderPath): SplatVariant | null {
  for (const name of PREFERENCE[path]) {
    const found = asset.variants.find((v) => v.name === name);
    if (found) return found;
  }
  return null;
}
