import type { SplatAsset } from './types';

/**
 * The credits line every view of an asset shows (ADR-2170 decision 10).
 * Same wording as `ListedAsset::credits()` in the manifest crate.
 */
export function creditsLine(asset: SplatAsset): string {
  const base = `${asset.attribution} (${asset.licence.spdx} ${asset.licence.url})`;
  return asset.modified ? `${base}, modified (decimated)` : base;
}
