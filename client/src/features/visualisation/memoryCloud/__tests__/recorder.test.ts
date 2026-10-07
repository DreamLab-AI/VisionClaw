import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { pickRecorderMime, recordCanvas, exportFilename, RecorderError } from '../recorder';

class FakeTrack {
  stopped = false;
  constructor(public kind: string) {}
  stop() { this.stopped = true; }
}

class FakeStream {
  tracks: FakeTrack[] = [new FakeTrack('video')];
  getTracks() { return this.tracks; }
  addTrack(t: FakeTrack) { this.tracks.push(t); }
}

class FakeRecorder {
  static instances: FakeRecorder[] = [];
  static supported = new Set<string>();
  static isTypeSupported(m: string) { return FakeRecorder.supported.has(m); }
  state: 'inactive' | 'recording' = 'inactive';
  ondataavailable: ((e: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  timeslice?: number;
  constructor(public stream: FakeStream, public options: { mimeType: string; videoBitsPerSecond?: number }) {
    FakeRecorder.instances.push(this);
  }
  start(timeslice?: number) { this.state = 'recording'; this.timeslice = timeslice; }
  stop() {
    if (this.state === 'inactive') return;
    this.state = 'inactive';
    this.ondataavailable?.({ data: new Blob([new Uint8Array([1, 2, 3])]) });
    queueMicrotask(() => this.onstop?.());
  }
}

describe('pickRecorderMime', () => {
  it('prefers mp4 (H.264) when the browser can record it', () => {
    const ok = new Set(['video/mp4;codecs=avc1.42E01F', 'video/webm;codecs=vp9']);
    expect(pickRecorderMime((m) => ok.has(m))).toEqual({ mime: 'video/mp4;codecs=avc1.42E01F', ext: 'mp4' });
  });
  it('falls back to webm, best codec first', () => {
    const ok = new Set(['video/webm;codecs=vp8', 'video/webm;codecs=vp9', 'video/webm']);
    expect(pickRecorderMime((m) => ok.has(m))).toEqual({ mime: 'video/webm;codecs=vp9', ext: 'webm' });
  });
  it('returns null when nothing is recordable', () => {
    expect(pickRecorderMime(() => false)).toBeNull();
  });
  it('uses MediaRecorder.isTypeSupported by default and tolerates its absence', () => {
    vi.stubGlobal('MediaRecorder', undefined);
    expect(pickRecorderMime()).toBeNull();
    vi.unstubAllGlobals();
  });
});

describe('exportFilename', () => {
  it('stamps the local date and time', () => {
    expect(exportFilename('webm', new Date(2026, 9, 7, 9, 5, 3))).toBe('memory-route-20261007-090503.webm');
  });
});

describe('recordCanvas', () => {
  let canvas: HTMLCanvasElement & { captureStream: (fps: number) => FakeStream };

  beforeEach(() => {
    vi.useFakeTimers();
    FakeRecorder.instances = [];
    FakeRecorder.supported = new Set(['video/webm;codecs=vp9']);
    vi.stubGlobal('MediaRecorder', FakeRecorder);
    canvas = document.createElement('canvas') as typeof canvas;
    canvas.captureStream = vi.fn(() => new FakeStream());
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('records for the duration, reports progress and returns a typed blob', async () => {
    const progress: number[] = [];
    const p = recordCanvas(canvas, { durationSec: 2, fps: 30, onProgress: (f) => progress.push(f) });
    expect(canvas.captureStream).toHaveBeenCalledWith(30);
    const rec = FakeRecorder.instances[0];
    expect(rec.options.mimeType).toBe('video/webm;codecs=vp9');
    expect(rec.state).toBe('recording');
    await vi.advanceTimersByTimeAsync(2100);
    const out = await p;
    expect(out.ext).toBe('webm');
    expect(out.mime).toBe('video/webm;codecs=vp9');
    expect(out.blob.size).toBe(3);
    expect(out.blob.type).toBe('video/webm');
    expect(progress.length).toBeGreaterThan(5);
    expect(progress[progress.length - 1]).toBe(1);
    for (let i = 1; i < progress.length; i++) expect(progress[i]).toBeGreaterThanOrEqual(progress[i - 1]);
    expect(rec.stream.tracks.every((t) => t.stopped)).toBe(true);
  });

  it('adds audio tracks from a supplied stream', async () => {
    const audio = { getAudioTracks: () => [new FakeTrack('audio')] } as unknown as MediaStream;
    const p = recordCanvas(canvas, { durationSec: 0.5, audio });
    const rec = FakeRecorder.instances[0];
    expect(rec.stream.tracks.map((t) => t.kind)).toEqual(['video', 'audio']);
    await vi.advanceTimersByTimeAsync(600);
    await p;
  });

  it('cancels on abort, stops the recorder and rejects with an abort error', async () => {
    const ctl = new AbortController();
    const p = recordCanvas(canvas, { durationSec: 10, signal: ctl.signal });
    const settled = p.catch((e) => e);
    await vi.advanceTimersByTimeAsync(500);
    ctl.abort();
    await vi.advanceTimersByTimeAsync(10);
    const err = await settled;
    expect(err).toBeInstanceOf(RecorderError);
    expect(err.kind).toBe('aborted');
    expect(FakeRecorder.instances[0].state).toBe('inactive');
  });

  it('refuses when the canvas cannot be captured or nothing is recordable', async () => {
    const bare = document.createElement('canvas');
    await expect(recordCanvas(bare, { durationSec: 1 })).rejects.toMatchObject({ kind: 'unsupported' });
    FakeRecorder.supported = new Set();
    await expect(recordCanvas(canvas, { durationSec: 1 })).rejects.toMatchObject({ kind: 'unsupported' });
  });
});
