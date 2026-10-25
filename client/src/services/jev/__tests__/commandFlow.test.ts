import { describe, it, expect, beforeEach, vi } from 'vitest';
import { planJevCommand } from '../commandFlow';
import { resetJevStatusCache, type JevApi } from '../jevClient';

const fields = [
  { path: 'physics.repelK', label: 'Repulsion', type: 'slider', min: 0, max: 1000, step: 10, current: 100 },
  { path: 'viz.labels', label: 'Labels', type: 'toggle', current: false },
];

const ON = { enabled: { 'settings-patch': true, 'action-class': true, 'command-intent': true }, available: true };
function api(post: unknown, opts: { reject?: boolean; status?: unknown } = {}): JevApi {
  return {
    get: vi.fn(async () => ({ data: opts.status ?? ON })),
    post: vi.fn(async () => {
      if (opts.reject) throw new Error('x');
      return { data: post };
    }),
  };
}

describe('planJevCommand', () => {
  beforeEach(() => resetJevStatusCache());

  it.each([
    ['flag off', api({}, { status: { enabled: {}, available: false } })],
    ['transport error', api({}, { reject: true })],
    ['used:false', api({ used: false, reason: 'low-confidence' })],
    ['malformed', api('nope')],
  ])('%s ⇒ null (caller keeps the existing settings-assistant call)', async (_n, a) => {
    expect(await planJevCommand('make repulsion bigger', fields, a)).toBeNull();
  });

  it('accept: Jev picks the field, code builds the patch', async () => {
    const r = await planJevCommand('set repulsion to 300', fields, api({
      used: true,
      actionClass: { id: 'persistent_setting', confidence: 0.9 },
      setting: { id: 'physics.repelK', confidence: 0.9 },
    }));
    expect(r).toMatchObject({ kind: 'patch', patch: { path: 'physics.repelK', current: 100, proposed: 300 } });
  });

  it('a field outside the registry is never patched', async () => {
    const r = await planJevCommand('set x to 3', fields, api({
      used: true, setting: { id: 'not.in.registry', confidence: 0.99 },
    }));
    expect(r).toBeNull();
  });

  it('a protected path chosen by a misbehaving server is refused', async () => {
    const f = [...fields, { path: 'auth.nostr.privateKey', label: 'Key', type: 'slider', current: 1 }];
    const r = await planJevCommand('set key to 3', f, api({ used: true, setting: { id: 'auth.nostr.privateKey', confidence: 0.99 } }));
    expect(r).toBeNull();
  });

  it('VC-06-S1-02: a one-off action is routed to review, not persisted', async () => {
    const r = await planJevCommand('pause the simulation', fields, api({
      used: true,
      actionClass: { id: 'temporary_physics_action', confidence: 0.9 },
      setting: { id: 'physics.repelK', confidence: 0.6 },
    }));
    expect(r).toMatchObject({ kind: 'route', classId: 'temporary_physics_action' });
  });

  it('missing value ⇒ clarification note, no patch', async () => {
    const r = await planJevCommand('repulsion', fields, api({
      used: true, setting: { id: 'physics.repelK', confidence: 0.9 }, valueMissing: 0.9,
    }));
    expect(r).toMatchObject({ kind: 'route', classId: 'clarify' });
  });

  it('VC-34-S1-01: typed intent maps to a registry command id, never an endpoint', async () => {
    const r = await planJevCommand('how do I do this', fields, api({ used: true, intent: { id: 'help', confidence: 0.9 } }));
    expect(r).toMatchObject({ kind: 'intent', intentId: 'help', commandId: 'nav.help' });
    const r2 = await planJevCommand('find node foo', fields, api({ used: true, intent: { id: 'graph_search', confidence: 0.9 } }));
    expect(r2).toMatchObject({ kind: 'intent', intentId: 'graph_search' });
    expect((r2 as { commandId?: string }).commandId).toBeUndefined();
  });
});
