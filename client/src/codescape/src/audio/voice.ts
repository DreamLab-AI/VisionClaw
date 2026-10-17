/**
 * Push-to-talk client for `/ws/voice` (codescape-server routes/voice.rs):
 * text `{"type":"start"}` on press takes the floor, then mic audio at 48 kHz →
 * AudioWorklet (inline Blob module) → 24 kHz f32 LE binary frames of 20 ms;
 * `{"type":"stop"}` on release. Replies: JSON `floor`/`listening`/`query`/
 * `error`, and speech as `tts_start {sample_rate, format}` → binary PCM →
 * `tts_end`, scheduled gaplessly on an AudioContext.
 */
import { ReconnectingSocket } from '../net/ReconnectingSocket';
import { Decimator2, f32ToLeBytes, Framer, leBytesToF32, LinearResampler, s16leToF32 } from './pcm';

export const ASR_RATE = 24_000;
export const FRAME_SAMPLES = 480;

const WORKLET = `
class CsCapture extends AudioWorkletProcessor {
  constructor() { super(); this.on = true; this.port.onmessage = (e) => { this.on = !!e.data; }; }
  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (this.on && ch) this.port.postMessage(ch.slice(0));
    return true;
  }
}
registerProcessor('cs-capture', CsCapture);
`;

export function voiceAvailable(): { ok: boolean; reason?: string } {
  if (typeof window === 'undefined') return { ok: false, reason: 'no window' };
  if (!window.isSecureContext) return { ok: false, reason: 'Push-to-talk needs a secure context (https or localhost).' };
  if (!navigator.mediaDevices?.getUserMedia) return { ok: false, reason: 'This browser has no microphone API.' };
  if (typeof AudioWorkletNode === 'undefined') return { ok: false, reason: 'This browser has no AudioWorklet.' };
  return { ok: true };
}

export class VoiceClient {
  private ctx: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private node: AudioWorkletNode | null = null;
  private socket: ReconnectingSocket | null = null;
  private resampler: { process(x: Float32Array): Float32Array } | null = null;
  private framer = new Framer(FRAME_SAMPLES);
  private nextPlay = 0;
  private talking = false;
  private ttsRate = ASR_RATE;
  private ttsFormat: 's16le' | 'f32le' = 's16le';
  onLevel?: (rms: number) => void;
  onStatus?: (s: string) => void;
  /** JSON frames from the server other than TTS framing (floor, listening, query, error). */
  onMessage?: (msg: { type: string } & Record<string, unknown>) => void;

  constructor(private readonly url: string) {}

  private ensureSocket(): ReconnectingSocket {
    if (!this.socket) {
      this.socket = new ReconnectingSocket(this.url, {
        onStatus: (s) => this.onStatus?.(s),
        // Pressed before the socket was up (or it reconnected mid-press): take the floor now.
        onOpen: (ws) => {
          if (this.talking) ws.send(JSON.stringify({ type: 'start' }));
        },
        onMessage: (data) => {
          if (typeof data !== 'string') {
            this.play(data);
            return;
          }
          let msg: unknown;
          try {
            msg = JSON.parse(data);
          } catch {
            return;
          }
          if (!msg || typeof msg !== 'object' || typeof (msg as { type?: unknown }).type !== 'string') return;
          const m = msg as { type: string; sample_rate?: number; format?: string } & Record<string, unknown>;
          if (m.type === 'tts_start') {
            this.ttsRate = typeof m.sample_rate === 'number' && m.sample_rate > 0 ? m.sample_rate : ASR_RATE;
            this.ttsFormat = m.format === 'f32le' ? 'f32le' : 's16le';
          } else if (m.type !== 'tts_end') {
            this.onMessage?.(m);
          }
        },
      });
      this.socket.start();
    }
    return this.socket;
  }

  private async ensureAudio(): Promise<AudioContext> {
    if (this.ctx) {
      if (this.ctx.state === 'suspended') await this.ctx.resume();
      return this.ctx;
    }
    const ctx = new AudioContext({ sampleRate: 48_000, latencyHint: 'interactive' });
    const blobUrl = URL.createObjectURL(new Blob([WORKLET], { type: 'application/javascript' }));
    try {
      await ctx.audioWorklet.addModule(blobUrl);
    } finally {
      URL.revokeObjectURL(blobUrl);
    }
    this.ctx = ctx;
    this.resampler = ctx.sampleRate === 48_000 ? new Decimator2() : new LinearResampler(ctx.sampleRate, ASR_RATE);
    return ctx;
  }

  async start(): Promise<void> {
    if (this.talking) return;
    this.talking = true;
    const sock = this.ensureSocket();
    const ctx = await this.ensureAudio();
    if (!this.stream) {
      this.stream = await navigator.mediaDevices.getUserMedia({
        audio: { sampleRate: 48_000, channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      const src = ctx.createMediaStreamSource(this.stream);
      this.node = new AudioWorkletNode(ctx, 'cs-capture', { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 1 });
      this.node.port.onmessage = (e: MessageEvent<Float32Array>) => {
        if (!this.talking || !this.resampler) return;
        const chunk = e.data;
        let s = 0;
        for (let i = 0; i < chunk.length; i++) s += chunk[i] * chunk[i];
        this.onLevel?.(Math.sqrt(s / Math.max(1, chunk.length)));
        this.framer.push(this.resampler.process(chunk), (f) => sock.send(f32ToLeBytes(f)));
      };
      src.connect(this.node);
    }
    this.node?.port.postMessage(true);
    sock.send(JSON.stringify({ type: 'start' }));
  }

  stop(): void {
    if (!this.talking) return;
    this.talking = false;
    this.node?.port.postMessage(false);
    const sock = this.socket;
    this.framer.flush((f) => sock?.send(f32ToLeBytes(f)));
    sock?.send(JSON.stringify({ type: 'stop' }));
    this.onLevel?.(0);
  }

  private play(buf: ArrayBuffer): void {
    const ctx = this.ctx;
    if (!ctx || buf.byteLength < 2) return;
    const pcm = this.ttsFormat === 'f32le' ? leBytesToF32(buf) : s16leToF32(buf);
    if (!pcm.length) return;
    const ab = ctx.createBuffer(1, pcm.length, this.ttsRate);
    ab.copyToChannel(pcm, 0);
    const src = ctx.createBufferSource();
    src.buffer = ab;
    src.connect(ctx.destination);
    const at = Math.max(ctx.currentTime + 0.02, this.nextPlay);
    src.start(at);
    this.nextPlay = at + ab.duration;
  }

  dispose(): void {
    this.stop();
    this.stream?.getTracks().forEach((t) => t.stop());
    this.socket?.stop();
    void this.ctx?.close();
    this.ctx = null;
    this.stream = null;
    this.socket = null;
  }
}
