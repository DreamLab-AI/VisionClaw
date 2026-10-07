/**
 * Audio-file beat sync for the cinematic mode: decode a local file, estimate
 * its tempo and beat phase from an onset envelope, and expose a beat clock
 * that drives glow and the comet pulse. The file is decoded in the browser
 * and never uploaded.
 *
 * Adapted from the RuVector Explorer music sync (`analyzeBuffer`,
 * https://github.com/ruvnet/RuVector, docs/explorer/explorer.js, MIT
 * licence): log-energy flux, local-mean subtraction, an autocorrelation tempo
 * search with a log-normal prior near 118 bpm, then a comb fit for tempo and
 * phase. Pure functions over Float32Array so the analysis is unit-tested
 * without Web Audio; only `decodeAudioFile` needs a browser.
 */

/** analysis rate after decimation */
export const ANALYSIS_RATE = 11025;
const HOP = 128;
const WIN = 256;
/** cap on analysed audio, seconds */
const MAX_SECONDS = 120;

export interface OnsetEnvelope {
  /** half-wave rectified, local-mean-subtracted log-energy flux, one value per hop */
  env: Float32Array;
  /** envelope frames per second */
  frameRate: number;
  /**
   * seconds to add to `index / frameRate` to land on the onset: the first
   * frame whose window contains an onset starts on average `WIN - HOP / 2`
   * samples before it
   */
  lag: number;
}

export function onsetEnvelope(samples: Float32Array, sampleRate: number): OnsetEnvelope {
  const nf = Math.max(0, Math.floor((samples.length - WIN) / HOP));
  const frameRate = sampleRate / HOP;
  const energy = new Float32Array(nf);
  for (let i = 0; i < nf; i++) {
    let a = 0;
    const o = i * HOP;
    for (let j = 0; j < WIN; j++) a += samples[o + j] * samples[o + j];
    energy[i] = Math.log1p(200 * Math.sqrt(a / WIN));
  }
  const flux = new Float32Array(nf);
  let mean = 0;
  for (let i = 1; i < nf; i++) {
    flux[i] = Math.max(0, energy[i] - energy[i - 1]);
    mean += flux[i];
  }
  mean = mean / Math.max(1, nf) || 1;
  for (let i = 0; i < nf; i++) flux[i] /= mean;
  // subtract a ±0.25 s running mean, then half-wave rectify
  const W = Math.max(1, Math.round(frameRate * 0.25));
  const env = new Float32Array(nf);
  let acc = 0;
  for (let i = 0; i < Math.min(nf, W); i++) acc += flux[i];
  for (let i = 0; i < nf; i++) {
    const lo = i - W - 1;
    const hi = i + W;
    if (hi < nf) acc += flux[hi];
    if (lo >= 0) acc -= flux[lo];
    const cnt = Math.min(nf - 1, hi) - Math.max(0, lo + 1) + 1;
    env[i] = Math.max(0, flux[i] - acc / cnt);
  }
  return { env, frameRate, lag: (WIN - HOP / 2) / sampleRate };
}

export interface TempoEstimate {
  bpm: number;
  /** seconds from t = 0 to the first beat, in [0, 60 / bpm) */
  offset: number;
  /** 0..1 normalised autocorrelation at the chosen tempo (0 = no pulse) */
  confidence: number;
}

export interface TempoOptions {
  minBpm?: number;
  maxBpm?: number;
  /** seconds; defaults to the envelope's own lag when an OnsetEnvelope is passed via analyse */
  lag?: number;
}

const sampleAt = (v: Float32Array, x: number): number => {
  const k = Math.floor(x);
  const f = x - k;
  return k + 1 < v.length && k >= 0 ? v[k] * (1 - f) + v[k + 1] * f : 0;
};

/** log-normal tempo prior centred on 118 bpm, σ = 0.9 octave */
const prior = (bpm: number): number => Math.exp(-0.5 * Math.pow(Math.log2(bpm / 118) / 0.9, 2));

/**
 * Tempo and phase from an onset envelope. The score at a tempo is the
 * envelope's autocorrelation at one and two beat lags; a half-tempo candidate
 * scores the same as the true tempo on a regular pulse, so the prior breaks
 * the tie towards the tempo nearer 118 bpm.
 */
export function estimateTempo(env: Float32Array, frameRate: number, opts: TempoOptions = {}): TempoEstimate {
  const minBpm = opts.minBpm ?? 70;
  const maxBpm = opts.maxBpm ?? 180;
  const lagSec = opts.lag ?? (WIN - HOP / 2) / ANALYSIS_RATE;
  const nf = env.length;
  if (nf < 8) return { bpm: 120, offset: 0, confidence: 0 };

  const acf = (bpm: number): number => {
    const lag = (60 * frameRate) / bpm;
    const n2 = nf - 2 * lag - 2;
    let sum = 0;
    for (let t = 0; t < n2; t++) {
      const v = env[t];
      if (v) sum += v * (sampleAt(env, t + lag) + 0.5 * sampleAt(env, t + 2 * lag));
    }
    return sum / Math.max(1, n2);
  };
  const score = (bpm: number) => acf(bpm) * (0.6 + 0.4 * prior(bpm));

  let best = 0;
  let bestB = 120;
  for (let bp = minBpm; bp <= maxBpm; bp += 0.5) {
    const sc = score(bp);
    if (sc > best) {
      best = sc;
      bestB = bp;
    }
  }
  for (let bp = bestB - 0.5; bp <= bestB + 0.5; bp += 0.05) {
    const sc = score(bp);
    if (sc > best) {
      best = sc;
      bestB = bp;
    }
  }
  // Normalised autocorrelation at the chosen tempo: 0 for an uncorrelated
  // envelope (whose expected lagged product is mean²), 1 for a pulse train
  // that repeats exactly at one and two beats (lagged product 1.5 · E[v²]).
  let m1 = 0;
  let m2 = 0;
  for (let i = 0; i < nf; i++) {
    m1 += env[i];
    m2 += env[i] * env[i];
  }
  m1 /= nf;
  m2 /= nf;
  const base = 1.5 * m1 * m1;
  const top = 1.5 * m2;
  const confidence = top > base ? Math.min(1, Math.max(0, (acf(bestB) - base) / (top - base))) : 0;

  // joint tempo + phase refinement by a comb over the whole window
  const comb = (bpm: number, ph: number): number => {
    const P = (60 * frameRate) / bpm;
    let s = 0;
    for (let x = ph; x < nf - 1; x += P) {
      const k = Math.round(x);
      s += Math.max(env[k] || 0, 0.5 * (env[k - 1] || 0), 0.5 * (env[k + 1] || 0));
    }
    return s;
  };
  let bestPh = 0;
  let bestS = -1;
  let cb = bestB;
  for (let bb = bestB - 1.5; bb <= bestB + 1.5; bb += 0.02) {
    const Pb = (60 * frameRate) / bb;
    for (let ph = 0; ph < Pb; ph += 0.5) {
      const s = comb(bb, ph);
      if (s > bestS) {
        bestS = s;
        bestPh = ph;
        cb = bb;
      }
    }
  }
  bestB = cb;
  const P = (60 * frameRate) / bestB;
  bestS = -1;
  for (let ph = Math.max(0, bestPh - 1); ph <= bestPh + 1; ph += 0.05) {
    let s = 0;
    for (let x = ph; x < nf - 1; x += P) s += sampleAt(env, x);
    if (s > bestS) {
      bestS = s;
      bestPh = ph;
    }
  }
  const period = 60 / bestB;
  let offset = bestPh / frameRate + lagSec;
  offset = ((offset % period) + period) % period;
  return {
    bpm: Math.round(bestB * 100) / 100,
    offset: Math.round(offset * 10000) / 10000,
    confidence: Math.round(confidence * 100) / 100,
  };
}

/** The slice of AudioBuffer the analysis needs (lets tests pass a plain object). */
export interface AudioBufferLike {
  numberOfChannels: number;
  length: number;
  sampleRate?: number;
  getChannelData(channel: number): Float32Array;
}

export function mixdown(buf: AudioBufferLike): Float32Array {
  const out = new Float32Array(buf.length);
  const ch = Math.max(1, buf.numberOfChannels);
  for (let c = 0; c < ch; c++) {
    const d = buf.getChannelData(c);
    for (let i = 0; i < out.length; i++) out[i] += d[i] / ch;
  }
  return out;
}

/** Box-filter decimation to roughly ANALYSIS_RATE. */
function decimate(samples: Float32Array, sampleRate: number): { data: Float32Array; rate: number } {
  const factor = Math.max(1, Math.round(sampleRate / ANALYSIS_RATE));
  if (factor === 1) return { data: samples, rate: sampleRate };
  const n = Math.floor(samples.length / factor);
  const data = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    let s = 0;
    const o = i * factor;
    for (let k = 0; k < factor; k++) s += samples[o + k];
    data[i] = s / factor;
  }
  return { data, rate: sampleRate / factor };
}

/** Tempo and phase of a decoded buffer (first two minutes). */
export function analyseAudioBuffer(buf: AudioBufferLike & { sampleRate: number }): TempoEstimate {
  const mono = mixdown(buf);
  const capped = mono.subarray(0, Math.min(mono.length, Math.round(MAX_SECONDS * buf.sampleRate)));
  const { data, rate } = decimate(capped, buf.sampleRate);
  const { env, frameRate, lag } = onsetEnvelope(data, rate);
  return estimateTempo(env, frameRate, { lag });
}

export interface BeatClock {
  bpm: number;
  offset: number;
  period: number;
  /** 0..1 position within the current beat */
  phase(t: number): number;
  /** sharp attack, exponential decay; 1 on the beat */
  pulse(t: number): number;
  beatIndex(t: number): number;
  /** pulse on the first beat of each 4-beat bar, 0 otherwise */
  barPulse(t: number): number;
}

export function beatClock({ bpm, offset }: { bpm: number; offset: number }): BeatClock {
  const period = 60 / bpm;
  const pos = (t: number) => (t - offset) / period;
  const phase = (t: number) => {
    const p = pos(t);
    return p - Math.floor(p);
  };
  const pulse = (t: number) => Math.exp(-phase(t) * 6);
  const beatIndex = (t: number) => Math.floor(pos(t) + 1e-9);
  return {
    bpm,
    offset,
    period,
    phase,
    pulse,
    beatIndex,
    barPulse: (t: number) => (((beatIndex(t) % 4) + 4) % 4 === 0 ? pulse(t) : 0),
  };
}

/** Decode a local audio file with Web Audio. Browser only; the bytes stay on this machine. */
export async function decodeAudioFile(file: File, ctx?: BaseAudioContext): Promise<AudioBuffer> {
  const bytes = await file.arrayBuffer();
  const ac: BaseAudioContext =
    ctx ??
    new (
      window.AudioContext ??
      (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext
    )();
  return ac.decodeAudioData(bytes);
}
