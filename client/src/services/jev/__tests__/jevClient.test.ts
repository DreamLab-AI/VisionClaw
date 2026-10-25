import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import {
  getJevStatus,
  isJevEnabled,
  jevUnderstandCommand,
  jevRankPalette,
  semanticSuggestions,
  resetJevStatusCache,
  JEV_DEADLINE_MS,
  type JevApi,
} from '../jevClient';

const ON = {
  enabled: { 'settings-patch': true, 'action-class': true, 'command-intent': true, 'palette-search': true },
  available: true,
};

function api(over: Partial<{ status: unknown; post: unknown; postThrows: boolean; statusThrows: boolean }> = {}) {
  const calls: Array<{ url: string; body?: unknown }> = [];
  const fake: JevApi = {
    get: vi.fn(async (url: string) => {
      calls.push({ url });
      if (over.statusThrows) throw new Error('boom');
      return { data: over.status ?? ON };
    }),
    post: vi.fn(async (url: string, body: unknown) => {
      calls.push({ url, body });
      if (over.postThrows) throw new Error('boom');
      return { data: over.post };
    }),
  };
  return { fake, calls };
}

describe('Jev browser client — fail-open', () => {
  beforeEach(() => resetJevStatusCache());
  afterEach(() => vi.useRealTimers());

  it('flag off: reports disabled and sends no /jev/command or /jev/palette', async () => {
    const { fake, calls } = api({ status: { enabled: {}, available: false } });
    expect(await isJevEnabled('settings-patch', fake)).toBe(false);
    expect(await jevUnderstandCommand('make nodes bigger', [], fake)).toBeNull();
    expect(await jevRankPalette('undo', [{ id: 'a', title: 'A' }], fake)).toBeNull();
    expect(calls.every((c) => c.url === '/jev/status')).toBe(true);
  });

  it('enabled flag but no endpoint (available=false) is treated as off', async () => {
    const { fake } = api({ status: { enabled: { 'palette-search': true }, available: false } });
    expect(await isJevEnabled('palette-search', fake)).toBe(false);
  });

  it('status transport error ⇒ null everywhere', async () => {
    const { fake } = api({ statusThrows: true });
    expect(await getJevStatus(fake)).toBeNull();
    expect(await jevUnderstandCommand('x', [], fake)).toBeNull();
  });

  it.each([
    ['transport error', { postThrows: true }],
    ['used:false', { post: { used: false, reason: 'http-500' } }],
    ['malformed body', { post: 'garbage' }],
    ['empty understanding', { post: { used: true } }],
  ])('command: %s ⇒ null', async (_n, over) => {
    const { fake } = api(over as never);
    expect(await jevUnderstandCommand('make nodes bigger', [], fake)).toBeNull();
  });

  it('timeout ⇒ null at the deadline', async () => {
    vi.useFakeTimers();
    const fake: JevApi = {
      get: vi.fn(async () => ({ data: ON })),
      post: vi.fn(() => new Promise(() => {})), // never resolves
    };
    const p = jevUnderstandCommand('x', [], fake);
    await vi.advanceTimersByTimeAsync(JEV_DEADLINE_MS + 50);
    expect(await p).toBeNull();
  });

  it('command wire shape and accept path', async () => {
    const { fake, calls } = api({
      post: { used: true, intent: { id: 'help', confidence: 0.9 }, setting: { id: 'a.b', confidence: 0.8 }, valueMissing: 0.1 },
    });
    const cands = [{ path: 'a.b', label: 'AB', type: 'slider', min: 0, max: 10 }];
    const u = await jevUnderstandCommand('hello', cands, fake);
    expect(u?.setting?.id).toBe('a.b');
    expect(u?.intent?.id).toBe('help');
    const post = calls.find((c) => c.url === '/jev/command')!;
    expect(post.body).toEqual({ command: 'hello', candidates: cands });
  });

  it('palette: malformed ranking entries ⇒ null; accept path maps ids', async () => {
    const bad = api({ post: { used: true, ranked: [{ id: 'a' }], noMatch: false } });
    expect(await jevRankPalette('q', [{ id: 'a', title: 'A' }], bad.fake)).toBeNull();
    resetJevStatusCache();
    const good = api({ post: { used: true, ranked: [{ id: 'a', score: 1.9 }], noMatch: false } });
    const r = await jevRankPalette('q', [{ id: 'a', title: 'A' }], good.fake);
    expect(r).toEqual({ ranked: [{ id: 'a', score: 1.9 }], noMatch: false });
    const post = good.calls.find((c) => c.url === '/jev/palette')!;
    expect(post.body).toEqual({ query: 'q', commands: [{ id: 'a', title: 'A' }] });
  });
});

describe('semanticSuggestions', () => {
  const cmds = [
    { id: 'settings.undo', enabled: true },
    { id: 'view.theme', enabled: true },
    { id: 'off.cmd', enabled: false },
  ];
  it('leaves lexical results untouched and only adds enabled, unseen commands', () => {
    const lexical = [cmds[1]];
    const before = [...lexical];
    const out = semanticSuggestions(lexical, {
      ranked: [{ id: 'view.theme', score: 2 }, { id: 'off.cmd', score: 2 }, { id: 'ghost', score: 2 }, { id: 'settings.undo', score: 1.8 }],
      noMatch: false,
    }, cmds);
    expect(lexical).toEqual(before);
    expect(out.map((c) => c.id)).toEqual(['settings.undo']);
  });
  it('null ranking or noMatch ⇒ nothing', () => {
    expect(semanticSuggestions([], null, cmds)).toEqual([]);
    expect(semanticSuggestions([], { ranked: [{ id: 'settings.undo', score: 2 }], noMatch: true }, cmds)).toEqual([]);
  });
});
