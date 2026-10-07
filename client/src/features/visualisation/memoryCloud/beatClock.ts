/**
 * Renderer-agnostic beat clock for the memory explorer.
 *
 * One serialisable state drives the glow and comet pulse whatever the sound
 * source is, so it can be broadcast unchanged to other clients (the Godot XR
 * client):
 *
 *   { bpm, phaseAt: epoch ms of a beat, confidence: 0..1, source }
 *
 * Sources: `file` (a local audio file analysed by beat.ts; locked to the
 * moment the director starts it), `tap` (tap tempo, key B), `spotify` (the
 * official embed plays the music; its audio is cross-origin and cannot be
 * analysed, so the beat comes from tap tempo too) and `off`. A `phaseAt` of 0
 * means the clock has not locked yet.
 *
 * Tap-tempo maths after the RuVector Explorer's music module (MIT,
 * `docs/explorer/explorer.js`, `tap`/`nudgeBpm`/`nudgePhase`): median of the
 * last eight intervals, reset after a pause, phase locked to the last tap.
 */

export type BeatSource = 'off' | 'file' | 'tap' | 'spotify';

export interface BeatClockState {
  bpm: number;
  /** epoch ms at which a beat falls; 0 = not locked */
  phaseAt: number;
  /** 0..1 */
  confidence: number;
  source: BeatSource;
}

export const BEAT_SOURCES: readonly BeatSource[] = ['off', 'file', 'tap', 'spotify'];
export const DEFAULT_BPM = 120;
export const OFF_BEAT: BeatClockState = { bpm: DEFAULT_BPM, phaseAt: 0, confidence: 0, source: 'off' };

/** a pause longer than this starts a new tap sequence */
export const TAP_RESET_MS = 2500;
const TAP_KEEP = 9;
const TAP_MIN_BPM = 40;
const TAP_MAX_BPM = 220;
const NUDGE_MIN_BPM = 30;
const NUDGE_MAX_BPM = 240;
/** pulse decay per beat: exp(-phase · k) */
const PULSE_DECAY = 6;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
const tenth = (v: number) => Math.round(v * 10) / 10;

function median(sorted: number[]): number {
  const n = sorted.length;
  return n % 2 ? sorted[n >> 1] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2;
}

export interface TapReading {
  bpm: number;
  /** epoch ms of the last tap: the phase lock */
  phaseAt: number;
  confidence: number;
  taps: number;
}

export interface TapTempo {
  /** register a tap; returns a reading once three taps are in, else null */
  tap(atMs?: number): TapReading | null;
  reset(): void;
  readonly count: number;
}

export function createTapTempo(): TapTempo {
  const times: number[] = [];
  return {
    tap(atMs = Date.now()) {
      if (times.length && atMs - times[times.length - 1] > TAP_RESET_MS) times.length = 0;
      times.push(atMs);
      if (times.length > TAP_KEEP) times.shift();
      if (times.length < 3) return null;
      const iv: number[] = [];
      for (let i = Math.max(1, times.length - 8); i < times.length; i++) iv.push(times[i] - times[i - 1]);
      const sorted = [...iv].sort((a, b) => a - b);
      const m = median(sorted);
      const mad = median(iv.map((d) => Math.abs(d - m)).sort((a, b) => a - b));
      const bpm = clamp(tenth(60_000 / m), TAP_MIN_BPM, TAP_MAX_BPM);
      // steadiness (median absolute deviation relative to the beat) × how many intervals back it
      const confidence = clamp(1 - (4 * mad) / m, 0, 1) * Math.min(1, iv.length / 4);
      return { bpm, phaseAt: atMs, confidence, taps: times.length };
    },
    reset() {
      times.length = 0;
    },
    get count() {
      return times.length;
    },
  };
}

export interface BeatSample {
  on: boolean;
  /** 0..1 position within the current beat */
  phase: number;
  /** 1 on the beat, exponential decay through it */
  pulse: number;
  /** pulse on the first beat of each 4-beat bar counted from phaseAt, else 0 */
  bar: number;
  beatIndex: number;
}

const OFF_SAMPLE: BeatSample = { on: false, phase: 0, pulse: 0, bar: 0, beatIndex: 0 };

/** Evaluate the clock at an epoch time. */
export function beatAt(state: BeatClockState, nowMs: number): BeatSample {
  if (state.source === 'off' || !(state.bpm > 0) || !(state.phaseAt > 0)) return OFF_SAMPLE;
  const pos = (nowMs - state.phaseAt) / (60_000 / state.bpm);
  // tolerate float error just below an integer beat
  const beatIndex = Math.floor(pos + 1e-9);
  const phase = Math.max(0, pos - beatIndex);
  const pulse = Math.exp(-phase * PULSE_DECAY);
  const bar = ((beatIndex % 4) + 4) % 4 === 0 ? pulse : 0;
  return { on: true, phase, pulse, bar, beatIndex };
}

export function nudgePhase(state: BeatClockState, ms: number): BeatClockState {
  return state.phaseAt > 0 ? { ...state, phaseAt: state.phaseAt + ms } : state;
}

export function nudgeBpm(state: BeatClockState, delta: number): BeatClockState {
  return { ...state, bpm: clamp(tenth(state.bpm + delta), NUDGE_MIN_BPM, NUDGE_MAX_BPM) };
}

/** Lock a file's tempo estimate (offset in seconds into the audio) to when the audio started. */
export function fromTempoEstimate(
  est: { bpm: number; offset: number; confidence: number },
  audioStartEpochMs: number,
): BeatClockState {
  return { bpm: est.bpm, phaseAt: audioStartEpochMs + est.offset * 1000, confidence: est.confidence, source: 'file' };
}

/** Type guard for a received wire value. */
export function isBeatClockState(v: unknown): v is BeatClockState {
  if (!v || typeof v !== 'object') return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.bpm === 'number' && Number.isFinite(s.bpm) && s.bpm > 0 &&
    typeof s.phaseAt === 'number' && Number.isFinite(s.phaseAt) && s.phaseAt >= 0 &&
    typeof s.confidence === 'number' && Number.isFinite(s.confidence) && s.confidence >= 0 && s.confidence <= 1 &&
    typeof s.source === 'string' && (BEAT_SOURCES as readonly string[]).includes(s.source)
  );
}
