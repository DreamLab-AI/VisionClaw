import { unifiedApiClient } from '../../services/api/UnifiedApiClient';
import {
  VARIANT_NAMES,
  type SplatAsset,
  type SplatListing,
  type SplatVariant,
  type Vec3,
  type VariantName,
} from './types';

const SHA256_RE = /^[0-9a-f]{64}$/;
const ADDRESS_RE = /^sha256-12-[0-9a-f]{12}$/;

function fail(path: string, why: string): never {
  throw new Error(`splat listing: ${path} ${why}`);
}

function obj(v: unknown, path: string): Record<string, unknown> {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) fail(path, 'is not an object');
  return v as Record<string, unknown>;
}

function str(v: unknown, path: string): string {
  if (typeof v !== 'string' || v.length === 0) fail(path, 'is not a non-empty string');
  return v as string;
}

function num(v: unknown, path: string): number {
  if (typeof v !== 'number' || !Number.isFinite(v)) fail(path, 'is not a finite number');
  return v as number;
}

function vec3(v: unknown, path: string): Vec3 {
  if (!Array.isArray(v) || v.length !== 3) fail(path, 'is not a 3-vector');
  const a = v as unknown[];
  return [num(a[0], `${path}[0]`), num(a[1], `${path}[1]`), num(a[2], `${path}[2]`)];
}

function parseVariant(raw: unknown, path: string): SplatVariant | null {
  const o = obj(raw, path);
  const name = str(o.name, `${path}.name`);
  // A variant name this client does not know is skipped, not fatal: the
  // server may add tiers before the client learns them.
  if (!(VARIANT_NAMES as readonly string[]).includes(name)) return null;
  const sha256 = str(o.sha256, `${path}.sha256`);
  if (!SHA256_RE.test(sha256)) fail(`${path}.sha256`, 'is not 64 lower-case hex digits');
  const address = str(o.address, `${path}.address`);
  if (!ADDRESS_RE.test(address) || address !== `sha256-12-${sha256.slice(0, 12)}`) {
    fail(`${path}.address`, 'does not match the sha256');
  }
  const url = str(o.url, `${path}.url`);
  // Same-origin path only: the blob fetch must never be steered off-site.
  if (!url.startsWith('/') || url.startsWith('//')) fail(`${path}.url`, 'is not a same-origin path');
  return {
    name: name as VariantName,
    splats: num(o.splats, `${path}.splats`),
    shDegree: num(o.shDegree, `${path}.shDegree`),
    sha256,
    address,
    bytes: num(o.bytes, `${path}.bytes`),
    url,
  };
}

function parseAsset(raw: unknown, path: string): SplatAsset {
  const o = obj(raw, path);
  const licence = obj(o.licence, `${path}.licence`);
  const bounds = obj(o.bounds, `${path}.bounds`);
  const min = vec3(bounds.min, `${path}.bounds.min`);
  const max = vec3(bounds.max, `${path}.bounds.max`);
  for (let k = 0; k < 3; k++) {
    if (min[k] > max[k]) fail(`${path}.bounds`, 'has min above max');
  }
  if (!Array.isArray(o.variants)) fail(`${path}.variants`, 'is not an array');
  const variants = (o.variants as unknown[])
    .map((v, i) => parseVariant(v, `${path}.variants[${i}]`))
    .filter((v): v is SplatVariant => v !== null);
  return {
    id: str(o.id, `${path}.id`),
    title: str(o.title, `${path}.title`),
    author: str(o.author, `${path}.author`),
    sourceUrl: str(o.sourceUrl, `${path}.sourceUrl`),
    licence: { spdx: str(licence.spdx, `${path}.licence.spdx`), url: str(licence.url, `${path}.licence.url`) },
    attribution: str(o.attribution, `${path}.attribution`),
    modified: o.modified === true,
    bounds: { min, max },
    variants,
  };
}

/** Validate and normalise a `GET /api/splats` body. Throws on a malformed listing. */
export function parseSplatListing(raw: unknown): SplatListing {
  const o = obj(raw, 'body');
  if (!Array.isArray(o.assets)) fail('assets', 'is not an array');
  return { assets: (o.assets as unknown[]).map((a, i) => parseAsset(a, `assets[${i}]`)) };
}

/** Fetch the admitted assets and their verified variants. */
export async function fetchSplatListing(): Promise<SplatListing> {
  return parseSplatListing(await unifiedApiClient.getData<unknown>('/splats'));
}
