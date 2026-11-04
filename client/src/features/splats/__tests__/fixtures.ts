import type { SplatAsset, SplatVariant, VariantName } from '../types';

/** SHA-256 of [10,20,30,40,50] and of [99,99,99,99,99] (checked by the cache tests against SubtleCrypto). */
export const GOOD_BYTES = new Uint8Array([10, 20, 30, 40, 50]);
export const GOOD_SHA = '6ef53a6408ec4429791fa808e4fbe7843f936316950a20ba5fbbbdd01cd29899';
export const BAD_BYTES = new Uint8Array([99, 99, 99, 99, 99]);
export const BAD_SHA = '6304fbfe2b22557c34c42a70056616786a733b3d09fb326308c813d6ab712ec0';

/** SHA-256 of [i, 9, 9, 9] for i = 0..4: one distinct address per variant of an asset. */
const TABLE = [
  '321955d9f5a8b7a6eaf7a9fe0aa3f76437300a20f7dc0acfa67b8dcf7c511da0',
  '2921079c3fd9c52e6607206f66c14a8ac8900d9c6491c4f7fc316d6bfa2658a0',
  '3077146a832a9eca788b3de15cf180e7573e55fd3db066c9d85d43c7da1311c4',
  'ed376cfb2f087c1aeac34bf259cc8482461ee4e542de7ab5080a0ac36f9545db',
  '1937b367fa498a39d2638236da371f6bd1c7ec02cfcc75287de531c95a8d2317',
];

export function variantFor(name: VariantName, sha256: string = GOOD_SHA, byteLength = 5): SplatVariant {
  return {
    name,
    splats: name === 'desktop-hi' ? 3_000_000 : 1_500_000,
    shDegree: name.startsWith('xr') ? 0 : 3,
    sha256,
    address: `sha256-12-${sha256.slice(0, 12)}`,
    bytes: byteLength,
    url: `/api/splats/blob/sha256-12-${sha256.slice(0, 12)}`,
  };
}

/** Distinct bytes per variant name so addresses differ. */
export function assetWith(names: VariantName[]): SplatAsset {
  return {
    id: 'iss',
    title: 'International Space Station',
    author: 'Tamás Máté',
    sourceUrl: 'https://superspl.at/scene/cc026f6a',
    licence: { spdx: 'CC-BY-4.0', url: 'https://creativecommons.org/licenses/by/4.0/' },
    attribution: 'International Space Station by Tamás Máté',
    modified: true,
    bounds: { min: [-64, -36, -26], max: [64, 35, 43] },
    variants: names.map((n, i) => variantFor(n, TABLE[i], 4)),
  };
}

/** The wire shape of `GET /api/splats`, as the Rust side serialises it. */
export function wireListing(): unknown {
  const asset = assetWith(['desktop-hi', 'desktop', 'xr-3']);
  return JSON.parse(JSON.stringify({ assets: [asset] }));
}
