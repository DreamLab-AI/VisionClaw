/**
 * Pure data shaping for the memory cloud: point colours by namespace, source
 * type or age; key / namespace lookup for `memory_flash` events; and the
 * focus-pull dimming applied while a query route is on screen.
 */

import type { MemoryCloudMeta } from './types';

/** categorical palette (16, then cycles) — unchanged from the original cloud */
export const CLOUD_PALETTE = [
  0x4fc3f7, 0xaa96da, 0x81c784, 0xffb74d, 0xef5350,
  0x26c6da, 0xfff176, 0xce93d8, 0xa1887f, 0x90a4ae,
  0x4db6ac, 0xf06292, 0xaed581, 0x7986cb, 0xffcc80, 0xe0e0e0,
] as const;

export type CloudColourMode = 'namespace' | 'sourceType' | 'age';

export interface IndexMaps {
  /** key → rows, also `namespace:key` → rows */
  byKey: Map<string, number[]>;
  byNamespace: Map<string, number[]>;
}

const push = (m: Map<string, number[]>, k: string, i: number) => {
  const a = m.get(k);
  if (a) a.push(i);
  else m.set(k, [i]);
};

export function buildIndexMaps(metadata: MemoryCloudMeta[]): IndexMaps {
  const byKey = new Map<string, number[]>();
  const byNamespace = new Map<string, number[]>();
  metadata.forEach((m, i) => {
    if (m.key) {
      push(byKey, m.key, i);
      if (m.namespace) push(byKey, `${m.namespace}:${m.key}`, i);
    }
    if (m.namespace) push(byNamespace, m.namespace, i);
  });
  return { byKey, byNamespace };
}

export interface FlashEventLike {
  key?: string;
  namespace?: string;
}

export interface FlashTargets {
  indices: number[];
  /** 'key' = the flashed entry is in the sample; 'namespace' = only its namespace is; 'none' = unmatched */
  match: 'key' | 'namespace' | 'none';
}

/** most namespace stand-ins lit for one unmatched key */
const NAMESPACE_PICKS = 3;

/**
 * Rows a `memory_flash` lands on. An exact key wins (narrowed to the event's
 * namespace when the key repeats across namespaces); otherwise up to three
 * rows of the namespace stand in. There is no random-point fallback: a flash
 * whose namespace is not sampled reports `none`, and the HUD counts it.
 */
export function resolveFlashTargets(
  ev: FlashEventLike,
  maps: IndexMaps,
  rand: () => number = Math.random,
): FlashTargets {
  if (ev.key) {
    const qualified = ev.namespace ? maps.byKey.get(`${ev.namespace}:${ev.key}`) : undefined;
    const hit = qualified ?? maps.byKey.get(ev.key);
    if (hit && hit.length) return { indices: [...hit], match: 'key' };
  }
  const ns = ev.namespace ? maps.byNamespace.get(ev.namespace) : undefined;
  if (ns && ns.length) {
    const picks = Math.min(NAMESPACE_PICKS, ns.length);
    const chosen = new Set<number>();
    let guard = 0;
    while (chosen.size < picks && guard++ < picks * 8) {
      chosen.add(ns[Math.min(ns.length - 1, Math.floor(rand() * ns.length))]);
    }
    return { indices: [...chosen], match: 'namespace' };
  }
  return { indices: [], match: 'none' };
}

/** Rank of each row's `updatedAt`, 0 = oldest, 1 = newest (ties share the lower rank). */
export function ageRanks(metadata: MemoryCloudMeta[]): number[] {
  const n = metadata.length;
  if (n <= 1) return metadata.map(() => 1);
  const order = metadata.map((m, i) => [m.updatedAt ?? 0, i] as const).sort((a, b) => a[0] - b[0]);
  const out = new Array<number>(n);
  let r = 0;
  for (let k = 0; k < n; k++) {
    if (k > 0 && order[k][0] !== order[k - 1][0]) r = k;
    out[order[k][1]] = r / (n - 1);
  }
  return out;
}

const hexRgb = (hex: number): [number, number, number] => [
  ((hex >> 16) & 255) / 255,
  ((hex >> 8) & 255) / 255,
  (hex & 255) / 255,
];

/** age ramp: oldest deep blue → mid blue → newest mint */
const AGE_STOPS: Array<[number, [number, number, number]]> = [
  [0, hexRgb(0x1d2a52)],
  [0.6, hexRgb(0x6f9bff)],
  [1, hexRgb(0x7ef0cf)],
];

function ageColour(u: number): [number, number, number] {
  for (let s = 1; s < AGE_STOPS.length; s++) {
    const [u1, c1] = AGE_STOPS[s];
    const [u0, c0] = AGE_STOPS[s - 1];
    if (u <= u1) {
      const f = (u - u0) / (u1 - u0 || 1);
      return [c0[0] + (c1[0] - c0[0]) * f, c0[1] + (c1[1] - c0[1]) * f, c0[2] + (c1[2] - c0[2]) * f];
    }
  }
  return AGE_STOPS[AGE_STOPS.length - 1][1];
}

export interface ColourableSnapshot {
  count: number;
  metadata: MemoryCloudMeta[];
  namespaces: string[];
  sourceTypes: string[];
}

export function buildCloudColours(snap: ColourableSnapshot, mode: CloudColourMode): Float32Array {
  const colours = new Float32Array(snap.count * 3);
  if (mode === 'age') {
    const ranks = ageRanks(snap.metadata);
    for (let i = 0; i < snap.count; i++) {
      const c = ageColour(ranks[i] ?? 1);
      colours[i * 3] = c[0];
      colours[i * 3 + 1] = c[1];
      colours[i * 3 + 2] = c[2];
    }
    return colours;
  }
  const cats = mode === 'namespace' ? snap.namespaces : snap.sourceTypes;
  const catIndex = new Map<string, number>();
  cats.forEach((c, i) => catIndex.set(c, i));
  for (let i = 0; i < snap.count; i++) {
    const m = snap.metadata[i];
    const cat = mode === 'namespace' ? m.namespace : m.sourceType;
    const c = hexRgb(CLOUD_PALETTE[(catIndex.get(cat) ?? 0) % CLOUD_PALETTE.length]);
    colours[i * 3] = c[0];
    colours[i * 3 + 1] = c[1];
    colours[i * 3 + 2] = c[2];
  }
  return colours;
}

/**
 * Write `base` into `out`, dimming every row not in `keep` by `amount`
 * (0 = untouched, 1 = black). `keep = null` means no focus: a plain copy.
 */
export function applyFocusDim(base: Float32Array, out: Float32Array, keep: Set<number> | null, amount: number): void {
  if (!keep || amount <= 0) {
    out.set(base);
    return;
  }
  const f = 1 - Math.min(1, amount);
  const n = base.length / 3;
  for (let i = 0; i < n; i++) {
    const k = keep.has(i) ? 1 : f;
    out[i * 3] = base[i * 3] * k;
    out[i * 3 + 1] = base[i * 3 + 1] * k;
    out[i * 3 + 2] = base[i * 3 + 2] * k;
  }
}

/** Burst ring size and opacity at life fraction `t` ∈ [0, 1). */
export function burstFrame(
  t: number,
  profile: { maxScale: number; motion: 'expand' | 'implode' },
  reducedMotion: boolean,
): { scale: number; alpha: number } {
  const alpha = (1 - t * t) * 0.85;
  // reduced motion: no expanding or imploding ring, a fixed marker that fades
  if (reducedMotion) return { scale: profile.maxScale * 0.6, alpha };
  const scale = profile.motion === 'implode' ? profile.maxScale * Math.pow(1 - t, 3) : profile.maxScale * (1 - Math.pow(1 - t, 3));
  return { scale: Math.max(scale, 0.01), alpha };
}
