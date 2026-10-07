import { describe, it, expect } from 'vitest';
import {
  OFF_BEAT,
  beatAt,
  createTapTempo,
  fromTempoEstimate,
  nudgeBpm,
  nudgePhase,
  isBeatClockState,
  TAP_RESET_MS,
} from '../beatClock';

const T0 = 1_760_000_000_000; // an epoch ms

describe('createTapTempo', () => {
  it('needs three taps before it reports a tempo', () => {
    const tt = createTapTempo();
    expect(tt.tap(T0)).toBeNull();
    expect(tt.tap(T0 + 500)).toBeNull();
    const r = tt.tap(T0 + 1000)!;
    expect(r.bpm).toBe(120);
    expect(r.phaseAt).toBe(T0 + 1000);
    expect(r.taps).toBe(3);
  });

  it('recovers bpm and phase from steady synthetic taps', () => {
    for (const bpm of [72, 98, 128, 174]) {
      const tt = createTapTempo();
      const period = 60_000 / bpm;
      let r = null;
      for (let i = 0; i < 8; i++) r = tt.tap(T0 + i * period);
      expect(r!.bpm).toBeCloseTo(bpm, 0);
      expect(r!.phaseAt).toBeCloseTo(T0 + 7 * period, 6);
      expect(r!.confidence).toBeGreaterThan(0.9);
    }
  });

  it('takes the median interval, so one fumbled tap does not move the tempo', () => {
    const tt = createTapTempo();
    const times = [0, 500, 1000, 1500, 1830, 2500, 3000, 3500].map((d) => T0 + d);
    let r = null;
    for (const t of times) r = tt.tap(t);
    expect(r!.bpm).toBe(120);
  });

  it('reports lower confidence for ragged tapping', () => {
    const steady = createTapTempo();
    const ragged = createTapTempo();
    let a = null;
    let b = null;
    const jitter = [0, 60, -50, 70, -40, 55, -65, 45];
    for (let i = 0; i < 8; i++) {
      a = steady.tap(T0 + i * 500);
      b = ragged.tap(T0 + i * 500 + jitter[i]);
    }
    expect(b!.confidence).toBeLessThan(a!.confidence);
    expect(b!.confidence).toBeGreaterThanOrEqual(0);
  });

  it('starts over after a pause longer than the reset gap', () => {
    const tt = createTapTempo();
    for (let i = 0; i < 4; i++) tt.tap(T0 + i * 500);
    const after = T0 + 1500 + TAP_RESET_MS + 1;
    expect(tt.tap(after)).toBeNull();
    expect(tt.count).toBe(1);
    tt.tap(after + 1000);
    expect(tt.tap(after + 2000)!.bpm).toBe(60);
  });

  it('clamps to 40–220 bpm', () => {
    const fast = createTapTempo();
    let r = null;
    for (let i = 0; i < 5; i++) r = fast.tap(T0 + i * 100);
    expect(r!.bpm).toBe(220);
    const slow = createTapTempo();
    for (let i = 0; i < 4; i++) r = slow.tap(T0 + i * 2000);
    expect(r!.bpm).toBe(40);
  });
});

describe('beatAt', () => {
  const beat = { bpm: 120, phaseAt: T0, confidence: 1, source: 'tap' as const };

  it('is off for the off source and for a clock that has never locked', () => {
    expect(beatAt(OFF_BEAT, T0).on).toBe(false);
    expect(beatAt({ ...beat, phaseAt: 0 }, T0).on).toBe(false);
  });

  it('pulses 1 on the beat and decays through it, before and after phaseAt', () => {
    expect(beatAt(beat, T0).pulse).toBeCloseTo(1, 6);
    expect(beatAt(beat, T0 + 500).pulse).toBeCloseTo(1, 6);
    expect(beatAt(beat, T0 - 500).pulse).toBeCloseTo(1, 6);
    expect(beatAt(beat, T0 + 250).phase).toBeCloseTo(0.5, 6);
    expect(beatAt(beat, T0 + 250).pulse).toBeLessThan(0.1);
  });

  it('puts the bar accent on every fourth beat counted from phaseAt', () => {
    expect(beatAt(beat, T0).bar).toBeCloseTo(1, 6);
    expect(beatAt(beat, T0 + 500).bar).toBe(0);
    expect(beatAt(beat, T0 + 2000).bar).toBeCloseTo(1, 6);
    expect(beatAt(beat, T0 - 2000).bar).toBeCloseTo(1, 6);
  });
});

describe('nudges', () => {
  const beat = { bpm: 120, phaseAt: T0, confidence: 0.8, source: 'spotify' as const };
  it('shifts phase by milliseconds', () => {
    expect(nudgePhase(beat, -20).phaseAt).toBe(T0 - 20);
  });
  it('steps bpm by tenths and clamps to 30–240', () => {
    expect(nudgeBpm(beat, 0.5).bpm).toBe(120.5);
    expect(nudgeBpm({ ...beat, bpm: 239.8 }, 1).bpm).toBe(240);
    expect(nudgeBpm({ ...beat, bpm: 30.2 }, -1).bpm).toBe(30);
  });
});

describe('fromTempoEstimate', () => {
  it('anchors the file offset to the epoch time the audio started', () => {
    const s = fromTempoEstimate({ bpm: 100, offset: 0.25, confidence: 0.7 }, T0);
    expect(s).toEqual({ bpm: 100, phaseAt: T0 + 250, confidence: 0.7, source: 'file' });
    expect(beatAt(s, T0 + 250 + 600).pulse).toBeCloseTo(1, 6);
  });
});

describe('serialisation', () => {
  it('round-trips through JSON and validates the wire shape', () => {
    const s = { bpm: 128, phaseAt: T0, confidence: 0.5, source: 'tap' as const };
    expect(JSON.parse(JSON.stringify(s))).toEqual(s);
    expect(isBeatClockState(JSON.parse(JSON.stringify(s)))).toBe(true);
    expect(isBeatClockState({ ...s, source: 'radio' })).toBe(false);
    expect(isBeatClockState({ ...s, bpm: Number.NaN })).toBe(false);
    expect(isBeatClockState(null)).toBe(false);
  });
});
