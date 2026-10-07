/**
 * Cinematic session: the non-serialisable pieces of cinematic mode that do
 * not belong in zustand — the decoded audio buffer and its AudioContext, the
 * tempo estimate, and the export request / cancel handle. The panel writes
 * here; MemoryCameraRig (inside the Canvas) reads it when the director starts.
 *
 * The audio file is decoded and analysed locally; its bytes are never sent
 * anywhere.
 */

import { analyseAudioBuffer, beatClock, decodeAudioFile, type BeatClock, type TempoEstimate } from './beat';
import { useMemoryCloudStore } from './memoryCloudInstance';

export interface CinematicSession {
  audioCtx: AudioContext | null;
  audioBuffer: AudioBuffer | null;
  tempo: TempoEstimate | null;
  clock: BeatClock | null;
  /** set by the panel before startCinematic() to record the run */
  recordRequested: boolean;
  /** aborts an export in progress */
  exportAbort: AbortController | null;
}

export const cinematicSession: CinematicSession = {
  audioCtx: null,
  audioBuffer: null,
  tempo: null,
  clock: null,
  recordRequested: false,
  exportAbort: null,
};

export function ensureAudioContext(): AudioContext | null {
  if (cinematicSession.audioCtx) return cinematicSession.audioCtx;
  const AC =
    typeof window !== 'undefined'
      ? window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      : undefined;
  if (!AC) return null;
  cinematicSession.audioCtx = new AC();
  return cinematicSession.audioCtx;
}

/** Decode and analyse a local audio file for beat sync. */
export async function loadAudioFile(file: File): Promise<void> {
  const store = useMemoryCloudStore.getState();
  try {
    const ctx = ensureAudioContext();
    if (!ctx) throw new Error('Web Audio is not available in this browser');
    const buffer = await decodeAudioFile(file, ctx);
    const tempo = analyseAudioBuffer(buffer);
    cinematicSession.audioBuffer = buffer;
    cinematicSession.tempo = tempo;
    cinematicSession.clock = beatClock(tempo);
    store.setCinematic({
      audio: { name: file.name, duration: buffer.duration, bpm: tempo.bpm, offset: tempo.offset, confidence: tempo.confidence },
      audioError: null,
    });
  } catch (e) {
    clearAudio();
    store.setCinematic({ audioError: `Could not use ${file.name}: ${e instanceof Error ? e.message : String(e)}` });
  }
}

export function clearAudio(): void {
  cinematicSession.audioBuffer = null;
  cinematicSession.tempo = null;
  cinematicSession.clock = null;
  useMemoryCloudStore.getState().setCinematic({ audio: null });
}

/** Start the director; with `record`, the run is captured to video. */
export function startDirector(record: boolean): void {
  const store = useMemoryCloudStore.getState();
  if (store.cinematic.recording) return;
  cinematicSession.recordRequested = record;
  const last = store.cinematic.lastExport;
  if (record && last) {
    URL.revokeObjectURL(last.url);
    store.setCinematic({ lastExport: null });
  }
  store.setCinematic({ exportError: null, exportProgress: 0 });
  store.startCinematic();
}

export function stopDirector(): void {
  cinematicSession.exportAbort?.abort();
  useMemoryCloudStore.getState().stopCinematic();
}
