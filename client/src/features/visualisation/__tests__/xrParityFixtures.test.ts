/**
 * XR parity fixtures — the bridge that keeps the Godot XR client's Rust ports
 * (xr-client/rust/src/{semantic,attention,beat}.rs) in step with the desktop
 * TypeScript they were ported from.
 *
 * This suite runs the REAL desktop functions over a fixed set of inputs and
 * compares the outputs against `xr-client/rust/tests/fixtures/desktop_parity.json`.
 * The Rust test suite asserts its ports against the same file, so a change on
 * either side fails one of the two suites until the fixture is regenerated:
 *
 *   UPDATE_XR_FIXTURES=1 ./node_modules/.bin/vitest run \
 *     src/features/visualisation/__tests__/xrParityFixtures.test.ts
 *
 * then `cargo test -p visionclaw-xr-gdext` shows whether the ports still agree.
 */

import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import {
  memoryActionProfile,
  namespaceHueShift,
  semanticBurstColor,
  agentActionShape,
  agentActionColorHex,
} from '../semanticEncoding';
import { heatBrightenFactor, HEAT_BRIGHTEN_K } from '../heatColor';
import {
  normaliseHeat,
  DEFAULT_HEAT_HALF_LIFE_MS,
  HEAT_PER_TOUCH,
  MAX_RAW_HEAT,
  HEAT_SATURATION,
  COLD_RAW_EPSILON,
  DEFAULT_MAX_HEAT_ENTRIES,
} from '../attentionHeat';
import { onsetEnvelope, estimateTempo } from '../memoryCloud/beat';
import { beatAt, createTapTempo, type BeatClockState } from '../memoryCloud/beatClock';

// vite-node in this setup cannot resolve static `fs`/`path` imports ("No such
// built-in module: node:"), so take them from the runtime directly (Node ≥ 22.3).
type Fs = typeof import('fs');
type PathMod = typeof import('path');
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;
const fs = getBuiltin('fs') as Fs;
const path = getBuiltin('path') as PathMod;

const FIXTURE = path.resolve(__dirname, '../../../../../xr-client/rust/tests/fixtures/desktop_parity.json');
const UPDATE = process.env.UPDATE_XR_FIXTURES === '1';

const VERBS = ['store', 'retrieve', 'search', 'list', 'delete', 'access', 'bogus', ''];
const NAMESPACES = ['', 'personal-context', 'patterns', 'tasks', 'ruvnet-kb', 'project-state'];
const SR = 11025;
const T0 = 1_760_000_000_000;

/** Same click track as beat.test.ts: decaying 1.5 kHz click per beat over faint LCG noise. */
function clickTrack(bpm: number, offset: number, seconds: number, sr = SR, seed = 7): Float32Array {
  const out = new Float32Array(Math.round(seconds * sr));
  let s = seed;
  const rnd = () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
  for (let i = 0; i < out.length; i++) out[i] = 0.01 * rnd();
  const period = 60 / bpm;
  for (let t = offset; t < seconds; t += period) {
    const start = Math.round(t * sr);
    for (let k = 0; k < sr * 0.03 && start + k < out.length; k++) {
      out[start + k] += Math.sin((2 * Math.PI * 1500 * k) / sr) * Math.exp(-k / (sr * 0.006));
    }
  }
  return out;
}

/** Same noise as beat.test.ts "reports low confidence on noise". */
function noise(seconds: number, seed = 3): Float32Array {
  const out = new Float32Array(SR * seconds);
  let s = seed;
  for (let i = 0; i < out.length; i++) out[i] = ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) - 0.5;
  return out;
}

function sum(a: Float32Array): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i];
  return s;
}

function build() {
  const memoryProfiles = VERBS.map((v) => ({ action: v, ...memoryActionProfile(v) }));
  const burstColors = [] as Array<{ action: string; namespace: string; shift: number; hex: string }>;
  const c = new THREE.Color();
  for (const v of VERBS) {
    for (const ns of NAMESPACES) {
      semanticBurstColor(c, v, ns);
      burstColors.push({ action: v, namespace: ns, shift: namespaceHueShift(ns), hex: c.getHexString() });
    }
  }
  const beam = [0, 1, 2, 3, 4, 5, 999].map((t) => ({ actionType: t, color: agentActionColorHex(t), ...agentActionShape(t) }));

  const heatBrighten = [] as number[][];
  for (const [r, g, b] of [[0.2, 0.4, 0.8], [1, 0.5, 0.25], [0, 0, 0], [0.9, 0.9, 0.9], [0.3, 0.3, 0.3]]) {
    for (const h of [0, 0.25, 0.49, 0.86, 1]) heatBrighten.push([r, g, b, h, heatBrightenFactor(r, g, b, h)]);
  }

  const states: BeatClockState[] = [
    { bpm: 120, phaseAt: T0, confidence: 1, source: 'tap' },
    { bpm: 97.3, phaseAt: T0 + 123.4, confidence: 0.5, source: 'file' },
    { bpm: 174, phaseAt: T0 - 9876.5, confidence: 0.8, source: 'spotify' },
    { bpm: 120, phaseAt: 0, confidence: 1, source: 'tap' },
    { bpm: 120, phaseAt: T0, confidence: 1, source: 'off' },
  ];
  const beatSamples = [] as Array<{ state: BeatClockState; now: number; sample: ReturnType<typeof beatAt> }>;
  for (const st of states) {
    for (const dt of [-2000, -500, -1, 0, 1, 137, 250, 499.99, 500, 1999, 2000, 12_345.6]) {
      beatSamples.push({ state: st, now: T0 + dt, sample: beatAt(st, T0 + dt) });
    }
  }

  const tapSequences = [
    [0, 500, 1000],
    [0, 500, 1000, 1500, 1830, 2500, 3000, 3500],
    [0, 560, 950, 1570, 1960, 2555, 2935, 3545],
    [0, 500, 1000, 1500, 4001, 5001, 6001],
    [0, 100, 200, 300, 400],
    [0, 2000, 4000, 6000],
    [0, 600, 1200, 1800, 2400, 3000, 3600, 4200, 4800, 5400, 6000],
  ].map((offsets) => {
    const tt = createTapTempo();
    return { taps: offsets.map((o) => T0 + o), readings: offsets.map((o) => tt.tap(T0 + o)) };
  });

  const tempo = ([[120, 0.25, 16], [96, 0.1, 16], [140, 0.33, 16], [110, 0.2, 12]] as const).map(([bpm, offset, seconds]) => {
    const { env, frameRate, lag } = onsetEnvelope(clickTrack(bpm, offset, seconds), SR);
    return { bpm, offset, seconds, seed: 7, envLen: env.length, envSum: sum(env), frameRate, lag, result: estimateTempo(env, frameRate) };
  });
  const nz = onsetEnvelope(noise(10), SR);
  const noiseTempo = { seconds: 10, seed: 3, envLen: nz.env.length, envSum: sum(nz.env), result: estimateTempo(nz.env, nz.frameRate) };

  return {
    _comment: 'Generated by client/src/features/visualisation/__tests__/xrParityFixtures.test.ts — do not hand-edit.',
    heatConstants: {
      halfLifeMs: DEFAULT_HEAT_HALF_LIFE_MS,
      perTouch: HEAT_PER_TOUCH,
      maxRaw: MAX_RAW_HEAT,
      saturation: HEAT_SATURATION,
      coldEpsilon: COLD_RAW_EPSILON,
      maxEntries: DEFAULT_MAX_HEAT_ENTRIES,
      brightenK: HEAT_BRIGHTEN_K,
    },
    normaliseHeat: [0, -1, 0.25, 0.5, 1, 2, 3, 6].map((r) => [r, normaliseHeat(r)]),
    heatBrighten,
    memoryProfiles,
    burstColors,
    beam,
    beatSamples,
    tapSequences,
    tempo,
    noiseTempo,
  };
}

/** JSON round-trip so the comparison sees exactly what is on disk (no -0, no undefined). */
const canon = (v: unknown) => JSON.parse(JSON.stringify(v));

describe('XR parity fixtures', () => {
  it('desktop functions reproduce desktop_parity.json', () => {
    const live = canon(build());
    if (UPDATE) {
      fs.mkdirSync(path.dirname(FIXTURE), { recursive: true });
      fs.writeFileSync(FIXTURE, JSON.stringify(live, null, 1) + '\n');
    }
    expect(fs.existsSync(FIXTURE)).toBe(true);
    const disk = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'));
    expect(live).toEqual(disk);
  });
});
