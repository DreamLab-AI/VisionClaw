/**
 * Video export of the scene canvas for the cinematic mode:
 * `canvas.captureStream` into a `MediaRecorder`, mp4 (H.264) when the
 * browser can record it, else webm. Real-time capture: the director plays
 * the route while the recorder runs, so a 20-second clip takes 20 seconds.
 *
 * The mime ladder follows the RuVector Explorer `recorderMime` /
 * `exportRecorder` (https://github.com/ruvnet/RuVector, docs/explorer, MIT
 * licence), with a webm fallback because Chromium on Linux rarely records mp4.
 */

export type RecorderErrorKind = 'unsupported' | 'aborted' | 'failed';

export class RecorderError extends Error {
  constructor(readonly kind: RecorderErrorKind, message: string) {
    super(message);
    this.name = kind === 'aborted' ? 'AbortError' : 'RecorderError';
  }
}

export const RECORDER_MIME_CANDIDATES: ReadonlyArray<{ mime: string; ext: 'mp4' | 'webm' }> = [
  { mime: 'video/mp4;codecs=avc1.42E01F', ext: 'mp4' },
  { mime: 'video/mp4;codecs=avc1', ext: 'mp4' },
  { mime: 'video/mp4', ext: 'mp4' },
  { mime: 'video/webm;codecs=vp9', ext: 'webm' },
  { mime: 'video/webm;codecs=vp8', ext: 'webm' },
  { mime: 'video/webm', ext: 'webm' },
];

export interface RecorderMime {
  mime: string;
  ext: 'mp4' | 'webm';
}

function defaultIsSupported(): ((m: string) => boolean) | null {
  const MR = (globalThis as { MediaRecorder?: { isTypeSupported?: (m: string) => boolean } }).MediaRecorder;
  if (!MR || typeof MR.isTypeSupported !== 'function') return null;
  return (m) => MR.isTypeSupported!(m);
}

/** First recordable container: mp4 when supported, else the best webm codec, else null. */
export function pickRecorderMime(isSupported?: (mime: string) => boolean): RecorderMime | null {
  const check = isSupported ?? defaultIsSupported();
  if (!check) return null;
  for (const c of RECORDER_MIME_CANDIDATES) if (check(c.mime)) return { ...c };
  return null;
}

const pad = (n: number) => String(n).padStart(2, '0');

export function exportFilename(ext: string, at = new Date()): string {
  const d = `${at.getFullYear()}${pad(at.getMonth() + 1)}${pad(at.getDate())}`;
  const t = `${pad(at.getHours())}${pad(at.getMinutes())}${pad(at.getSeconds())}`;
  return `memory-route-${d}-${t}.${ext}`;
}

export interface RecordOptions {
  durationSec: number;
  fps?: number;
  /** bits per second; default scales with the canvas area */
  bitrate?: number;
  /** stream whose audio tracks are mixed into the recording */
  audio?: MediaStream | null;
  /** 0..1, called roughly ten times a second */
  onProgress?: (fraction: number) => void;
  signal?: AbortSignal;
}

export interface RecordResult {
  blob: Blob;
  mime: string;
  ext: 'mp4' | 'webm';
  durationSec: number;
}

type CapturableCanvas = HTMLCanvasElement & { captureStream?: (fps?: number) => MediaStream };

/** Record `durationSec` of the canvas in real time. */
export function recordCanvas(canvas: HTMLCanvasElement, opts: RecordOptions): Promise<RecordResult> {
  const c = canvas as CapturableCanvas;
  if (typeof c.captureStream !== 'function') {
    return Promise.reject(new RecorderError('unsupported', 'This browser cannot capture the canvas (captureStream unavailable)'));
  }
  const picked = pickRecorderMime();
  if (!picked) {
    return Promise.reject(new RecorderError('unsupported', 'This browser cannot record video (no supported MediaRecorder type)'));
  }
  if (opts.signal?.aborted) return Promise.reject(new RecorderError('aborted', 'Export cancelled'));

  const fps = Math.max(1, Math.round(opts.fps ?? 30));
  const stream = c.captureStream(fps);
  for (const t of opts.audio?.getAudioTracks() ?? []) stream.addTrack(t);
  const bitrate = opts.bitrate ?? Math.round(Math.max(2e6, Math.min(16e6, (canvas.width || 1280) * (canvas.height || 720) * fps * 0.12)));
  const rec = new MediaRecorder(stream, { mimeType: picked.mime, videoBitsPerSecond: bitrate });
  const parts: Blob[] = [];
  const container = picked.ext === 'mp4' ? 'video/mp4' : 'video/webm';

  return new Promise<RecordResult>((resolve, reject) => {
    let aborted = false;
    let finished = false;
    const t0 = performance.now();
    const cleanup = () => {
      clearInterval(timer);
      opts.signal?.removeEventListener('abort', onAbort);
      for (const t of stream.getTracks()) t.stop();
    };
    const stop = () => {
      if (rec.state !== 'inactive') rec.stop();
    };
    const onAbort = () => {
      aborted = true;
      stop();
    };
    rec.ondataavailable = (e: BlobEvent) => {
      if (e.data && e.data.size) parts.push(e.data);
    };
    rec.onerror = (e: Event) => {
      if (finished) return;
      finished = true;
      cleanup();
      const inner = (e as unknown as { error?: Error }).error;
      reject(new RecorderError('failed', `Recording failed: ${inner?.message ?? 'unknown error'}`));
    };
    rec.onstop = () => {
      if (finished) return;
      finished = true;
      cleanup();
      if (aborted) {
        reject(new RecorderError('aborted', 'Export cancelled'));
        return;
      }
      opts.onProgress?.(1);
      resolve({ blob: new Blob(parts, { type: container }), mime: picked.mime, ext: picked.ext, durationSec: opts.durationSec });
    };
    const timer = setInterval(() => {
      const f = Math.min(1, (performance.now() - t0) / 1000 / opts.durationSec);
      if (f < 1) opts.onProgress?.(f);
      else stop();
    }, 100);
    opts.signal?.addEventListener('abort', onAbort);
    try {
      rec.start(1000);
    } catch (e) {
      finished = true;
      cleanup();
      reject(new RecorderError('failed', `Recorder would not start: ${e instanceof Error ? e.message : String(e)}`));
    }
  });
}

/** Save a blob through a temporary object URL. */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.rel = 'noopener';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
