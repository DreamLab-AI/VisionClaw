/**
 * Hand-written mirror of the `GET /api/splats` listing (ADR-2170).
 *
 * The Rust side is `crates/visionclaw-splat/src/manifest.rs` (`SplatListing`,
 * `ListedAsset`, `ListedVariant`, all camelCase on the wire). `generate_types`
 * only emits settings types, and `client/src/types/` is generated, so this
 * mirror lives with the feature. Change both together; `parseSplatListing`
 * in `api.ts` is the runtime check that they still agree.
 */

/** Variant names the server may list, in `VariantName` order. */
export const VARIANT_NAMES = ['desktop-hi', 'desktop', 'xr-3', 'xr-2', 'xr-1'] as const;

export type VariantName = (typeof VARIANT_NAMES)[number];

export type Vec3 = [number, number, number];

export interface SplatLicence {
  /** SPDX identifier, e.g. `CC-BY-4.0` */
  spdx: string;
  url: string;
}

/** Axis-aligned box of the served splats, in the asset's own units. */
export interface SplatBounds {
  min: Vec3;
  max: Vec3;
}

export interface SplatVariant {
  name: VariantName;
  splats: number;
  shDegree: number;
  /** full lower-case hex SHA-256 of the blob */
  sha256: string;
  /** `sha256-12-<12 hex>`, the cache key */
  address: string;
  bytes: number;
  /** same-origin path, `/api/splats/blob/<address>` */
  url: string;
}

export interface SplatAsset {
  id: string;
  title: string;
  author: string;
  sourceUrl: string;
  licence: SplatLicence;
  attribution: string;
  /** true for every cached asset: decimation and SH stripping are adaptations */
  modified: boolean;
  bounds: SplatBounds;
  variants: SplatVariant[];
}

export interface SplatListing {
  assets: SplatAsset[];
}
