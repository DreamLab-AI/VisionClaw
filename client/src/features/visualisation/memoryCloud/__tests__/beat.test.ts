import { describe, it, expect } from 'vitest';
import { onsetEnvelope, estimateTempo, analyseAudioBuffer, mixdown } from '../beat';

const SR = 11025;

/** decaying 1.5 kHz click at every beat, faint noise between */
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

const phaseError = (got: number, want: number, period: number) => {
  const d = Math.abs(((got - want) % period + period) % period);
  return Math.min(d, period - d);
};

describe('onsetEnvelope', () => {
  it('peaks once per click', () => {
    const { env, frameRate } = onsetEnvelope(clickTrack(120, 0.25, 4), SR);
    const thr = Math.max(...env) * 0.5;
    const peaks: number[] = [];
    for (let i = 1; i < env.length - 1; i++) {
      if (env[i] > thr && env[i] >= env[i - 1] && env[i] > env[i + 1]) peaks.push(i / frameRate);
    }
    expect(peaks.length).toBeGreaterThanOrEqual(7);
    expect(peaks.length).toBeLessThanOrEqual(8);
    for (let i = 1; i < peaks.length; i++) expect(peaks[i] - peaks[i - 1]).toBeCloseTo(0.5, 1);
  });
});

describe('estimateTempo', () => {
  for (const [bpm, offset] of [[120, 0.25], [96, 0.1], [140, 0.33]] as const) {
    it(`recovers ${bpm} bpm and its phase from a synthetic click track`, () => {
      const { env, frameRate } = onsetEnvelope(clickTrack(bpm, offset, 16), SR);
      const r = estimateTempo(env, frameRate);
      expect(Math.abs(r.bpm - bpm)).toBeLessThan(1);
      expect(phaseError(r.offset, offset, 60 / bpm)).toBeLessThan(0.025);
      expect(r.confidence).toBeGreaterThan(0.6);
    });
  }

  it('reports low confidence on noise', () => {
    const noise = new Float32Array(SR * 10);
    let s = 3;
    for (let i = 0; i < noise.length; i++) noise[i] = ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) - 0.5;
    const { env, frameRate } = onsetEnvelope(noise, SR);
    expect(estimateTempo(env, frameRate).confidence).toBeLessThan(0.4);
  });
});

describe('analyseAudioBuffer', () => {
  it('mixes down, decimates 44.1 kHz stereo and finds the tempo', () => {
    const mono = clickTrack(110, 0.2, 12, 44100);
    const buf = {
      numberOfChannels: 2,
      sampleRate: 44100,
      length: mono.length,
      duration: mono.length / 44100,
      getChannelData: (c: number) => (c === 0 ? mono : mono.map((v) => v * 0.5)),
    };
    const r = analyseAudioBuffer(buf);
    expect(Math.abs(r.bpm - 110)).toBeLessThan(1);
    expect(phaseError(r.offset, 0.2, 60 / 110)).toBeLessThan(0.03);
  });

  it('mixdown averages channels', () => {
    const a = new Float32Array([1, 1]);
    const b = new Float32Array([0, -1]);
    expect(Array.from(mixdown({ numberOfChannels: 2, length: 2, getChannelData: (c) => (c ? b : a) }))).toEqual([0.5, 0]);
  });
});

