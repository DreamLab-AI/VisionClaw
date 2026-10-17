/**
 * PCM helpers for push-to-talk: 48 kHz capture → 24 kHz f32 for the ASR, and
 * little-endian framing. Pure and unit-tested.
 */

/** Windowed-sinc low-pass taps (Blackman), normalised to unity DC gain. */
export function lowpassTaps(count: number, cutoff: number): Float32Array {
  const taps = new Float32Array(count);
  const m = count - 1;
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const x = i - m / 2;
    const sinc = x === 0 ? 2 * cutoff : Math.sin(2 * Math.PI * cutoff * x) / (Math.PI * x);
    const w = 0.42 - 0.5 * Math.cos((2 * Math.PI * i) / m) + 0.08 * Math.cos((4 * Math.PI * i) / m);
    taps[i] = sinc * w;
    sum += taps[i];
  }
  for (let i = 0; i < count; i++) taps[i] /= sum;
  return taps;
}

/**
 * Streaming 2:1 decimator (48 kHz → 24 kHz) with an anti-alias FIR. Keeps
 * filter history and output phase across chunks, so chunk boundaries are seamless.
 */
export class Decimator2 {
  private readonly taps: Float32Array;
  private hist: Float32Array;
  private phase = 0;

  constructor(taps = lowpassTaps(31, 0.22)) {
    this.taps = taps;
    this.hist = new Float32Array(taps.length - 1);
  }

  process(input: Float32Array): Float32Array {
    const T = this.taps.length;
    const buf = new Float32Array(this.hist.length + input.length);
    buf.set(this.hist, 0);
    buf.set(input, this.hist.length);
    const outLen = Math.floor((input.length - this.phase + 1) / 2);
    const out = new Float32Array(Math.max(0, outLen));
    let o = 0;
    for (let n = this.phase; n < input.length; n += 2) {
      // Output aligned to input sample n; buf index of that sample = n + T - 1.
      let acc = 0;
      const end = n + T - 1;
      for (let k = 0; k < T; k++) acc += this.taps[k] * buf[end - k];
      out[o++] = acc;
    }
    const consumed = input.length - this.phase;
    this.phase = consumed % 2 === 0 ? 0 : 1;
    this.hist = buf.slice(buf.length - (T - 1));
    return o === out.length ? out : out.slice(0, o);
  }
}

/** Streaming linear resampler for capture devices that refuse 48 kHz. */
export class LinearResampler {
  private pos = 0;
  private last = 0;

  constructor(private readonly from: number, private readonly to: number) {}

  process(input: Float32Array): Float32Array {
    const step = this.from / this.to;
    const out: number[] = [];
    // pos is measured relative to the previous chunk's last sample (index -1 = this.last).
    while (this.pos < input.length - 1 + 1e-9) {
      const i = Math.floor(this.pos);
      const f = this.pos - i;
      const a = i < 0 ? this.last : input[i];
      const b = i + 1 < 0 ? this.last : input[Math.min(i + 1, input.length - 1)];
      out.push(a + (b - a) * f);
      this.pos += step;
    }
    this.pos -= input.length;
    if (input.length) this.last = input[input.length - 1];
    return Float32Array.from(out);
  }
}

/** Accumulates samples and emits fixed-size frames. */
export class Framer {
  private buf: Float32Array;
  private fill = 0;

  constructor(private readonly size: number) {
    this.buf = new Float32Array(size);
  }

  push(samples: Float32Array, emit: (frame: Float32Array) => void): void {
    let off = 0;
    while (off < samples.length) {
      const n = Math.min(this.size - this.fill, samples.length - off);
      this.buf.set(samples.subarray(off, off + n), this.fill);
      this.fill += n;
      off += n;
      if (this.fill === this.size) {
        emit(this.buf.slice());
        this.fill = 0;
      }
    }
  }

  flush(emit: (frame: Float32Array) => void): void {
    if (this.fill) emit(this.buf.slice(0, this.fill));
    this.fill = 0;
  }
}

/** f32 samples → little-endian bytes (explicit, independent of host endianness). */
export function f32ToLeBytes(samples: Float32Array): ArrayBuffer {
  const out = new ArrayBuffer(samples.length * 4);
  const v = new DataView(out);
  for (let i = 0; i < samples.length; i++) v.setFloat32(i * 4, samples[i], true);
  return out;
}

export function leBytesToF32(buf: ArrayBuffer): Float32Array<ArrayBuffer> {
  const n = Math.floor(buf.byteLength / 4);
  const v = new DataView(buf);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = v.getFloat32(i * 4, true);
  return out;
}

/** s16 little-endian PCM bytes → f32 in [-1, 1) (the server's TTS format). */
export function s16leToF32(buf: ArrayBuffer): Float32Array<ArrayBuffer> {
  const n = Math.floor(buf.byteLength / 2);
  const v = new DataView(buf);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = v.getInt16(i * 2, true) / 32768;
  return out;
}
