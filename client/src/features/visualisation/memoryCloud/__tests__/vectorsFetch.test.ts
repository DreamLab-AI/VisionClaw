import { describe, it, expect, vi } from 'vitest';
import {
  runVectorsFetch,
  createWorkerTransport,
  type VectorsFetchRequest,
  type VectorsFetchResult,
  type VectorsTransport,
} from '../vectorsFetch';

const DIM = 4;
const req = (count = 2): VectorsFetchRequest => ({ url: 'http://h/api/memory-cloud/vectors?snapshot=a', headers: { A: '1' }, count, dim: DIM });

function unitRows(count: number): ArrayBuffer {
  const buf = new ArrayBuffer(count * DIM * 4);
  const dv = new DataView(buf);
  for (let r = 0; r < count; r++) dv.setFloat32((r * DIM + (r % DIM)) * 4, 1, true);
  return buf;
}

const clock = () => {
  let t = 100;
  return () => (t += 10);
};

describe('runVectorsFetch', () => {
  it('fetches with the given headers and signal, decodes, and stamps each step', async () => {
    const ctl = new AbortController();
    const f = vi.fn(async () => new Response(unitRows(2), { headers: { 'X-Memory-Cloud-Dim': '4', 'X-Memory-Cloud-Count': '2' } }));
    const r = await runVectorsFetch(req(), f as never, ctl.signal, clock());
    expect(f).toHaveBeenCalledWith(req().url, { method: 'GET', headers: { A: '1' }, signal: ctl.signal });
    expect(r.type).toBe('ok');
    if (r.type !== 'ok') return;
    expect(Array.from(r.data)).toEqual([1, 0, 0, 0, 0, 1, 0, 0]);
    expect(r.at).toEqual({ headers: 110, body: 120, decoded: 130 });
  });

  it('refuses a shape header that disagrees before reading the body', async () => {
    const res = new Response(unitRows(2), { headers: { 'X-Memory-Cloud-Count': '3' } });
    const body = vi.spyOn(res, 'arrayBuffer');
    const r = await runVectorsFetch(req(), (async () => res) as never);
    expect(r).toMatchObject({ type: 'invalid', message: 'X-Memory-Cloud-Count is 3, but the snapshot says 2' });
    expect(body).not.toHaveBeenCalled();
  });

  it('returns non-2xx with its body text and Retry-After', async () => {
    const r = await runVectorsFetch(req(), (async () =>
      new Response('{"error":"building"}', { status: 503, statusText: 'Service Unavailable', headers: { 'Retry-After': '5' } })) as never);
    expect(r).toMatchObject({ type: 'http', status: 503, statusText: 'Service Unavailable', text: '{"error":"building"}', retryAfter: '5' });
  });

  it('reports a blob that fails validation as invalid', async () => {
    const r = await runVectorsFetch(req(), (async () => new Response(unitRows(2).slice(4))) as never);
    expect(r).toMatchObject({ type: 'invalid' });
    expect((r as { message: string }).message).toMatch(/bytes, expected 32/);
  });

  it('separates request failures, body failures and aborts', async () => {
    expect(await runVectorsFetch(req(), (async () => { throw new TypeError('Failed to fetch'); }) as never))
      .toMatchObject({ type: 'network', phase: 'request', message: 'Failed to fetch' });
    const broken = new Response(unitRows(2));
    vi.spyOn(broken, 'arrayBuffer').mockRejectedValue(new TypeError('network reset'));
    expect(await runVectorsFetch(req(), (async () => broken) as never))
      .toMatchObject({ type: 'network', phase: 'body', message: 'TypeError: network reset' });
    expect(await runVectorsFetch(req(), (async () => { throw new DOMException('aborted', 'AbortError'); }) as never))
      .toEqual({ type: 'abort' });
  });
});

/** A Worker stand-in driven by the test. */
class FakeWorker {
  onmessage: ((e: MessageEvent) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  posted: unknown[] = [];
  terminated = false;
  postMessage(m: unknown) { this.posted.push(m); }
  terminate() { this.terminated = true; }
  reply(data: unknown) { this.onmessage?.({ data } as MessageEvent); }
  fail(message: string) { this.onerror?.({ message, preventDefault: () => {} } as unknown as ErrorEvent); }
}

describe('createWorkerTransport', () => {
  const ok: VectorsFetchResult = { type: 'ok', data: new Float32Array(8), at: { body: 1 } };

  it('posts the request, resolves with the worker result and terminates the worker', async () => {
    const w = new FakeWorker();
    const fallback = vi.fn<VectorsTransport>();
    const p = createWorkerTransport(() => w as unknown as Worker, fallback)(req());
    expect(w.posted).toEqual([req()]);
    w.reply({ type: 'started' });
    expect(w.terminated).toBe(false);
    w.reply(ok);
    expect(await p).toBe(ok);
    expect(w.terminated).toBe(true);
    expect(fallback).not.toHaveBeenCalled();
  });

  it('falls back in-thread when the worker script never starts', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const w = new FakeWorker();
    const fallback = vi.fn<VectorsTransport>(async () => ok);
    const p = createWorkerTransport(() => w as unknown as Worker, fallback)(req());
    w.fail('Failed to load module script');
    expect(await p).toBe(ok);
    expect(fallback).toHaveBeenCalledWith(req(), undefined);
    expect(w.terminated).toBe(true);
    warn.mockRestore();
  });

  it('falls back when the worker cannot be constructed', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const fallback = vi.fn<VectorsTransport>(async () => ok);
    const r = await createWorkerTransport(() => { throw new Error('blocked by CSP'); }, fallback)(req());
    expect(r).toBe(ok);
    warn.mockRestore();
  });

  it('reports a crash after start as a network failure, without refetching', async () => {
    const w = new FakeWorker();
    const fallback = vi.fn<VectorsTransport>();
    const p = createWorkerTransport(() => w as unknown as Worker, fallback)(req());
    w.reply({ type: 'started' });
    w.fail('out of memory');
    expect(await p).toMatchObject({ type: 'network', message: 'vectors worker crashed: out of memory' });
    expect(fallback).not.toHaveBeenCalled();
  });

  it('aborts by terminating the worker; a late reply is ignored', async () => {
    const w = new FakeWorker();
    const ctl = new AbortController();
    const p = createWorkerTransport(() => w as unknown as Worker)(req(), ctl.signal);
    w.reply({ type: 'started' });
    ctl.abort();
    expect(await p).toEqual({ type: 'abort' });
    expect(w.terminated).toBe(true);
    w.reply(ok);
  });

  it('does not spawn for an already-aborted signal', async () => {
    const ctl = new AbortController();
    ctl.abort();
    const spawn = vi.fn();
    expect(await createWorkerTransport(spawn)(req(), ctl.signal)).toEqual({ type: 'abort' });
    expect(spawn).not.toHaveBeenCalled();
  });
});
