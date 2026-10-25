import { describe, it, expect } from 'vitest';
import { buildPatch, shortlistSettings, isProtectedPath, parseNumber } from '../settingsPatch';

const repel = { path: 'physics.repelK', label: 'Repulsion', type: 'slider', min: 0, max: 1000, step: 10 };
const toggle = { path: 'viz.labels', label: 'Labels', type: 'toggle' };
const select = { path: 'viz.quality', label: 'Quality', type: 'select', options: ['low', 'medium', 'high'] };

describe('buildPatch (deterministic; Jev only picks the field)', () => {
  it('parses explicit numbers and clamps to range', () => {
    const r = buildPatch('set repulsion to 5000', repel, 100);
    expect(r.ok && r.patch.proposed).toBe(1000);
    expect(r.ok && r.patch.clamped).toBe(true);
    const ok = buildPatch('set repulsion to 250', repel, 100);
    expect(ok.ok && ok.patch.proposed).toBe(250);
    expect(ok.ok && ok.patch.clamped).toBe(false);
  });
  it('snaps to step', () => {
    const r = buildPatch('make repulsion 253', repel, 100);
    expect(r.ok && r.patch.proposed).toBe(250);
  });
  it('percent uses the max', () => {
    const r = buildPatch('repulsion 50%', repel, 100);
    expect(r.ok && r.patch.proposed).toBe(500);
  });
  it('direction words need a current number; no value ⇒ missing-value', () => {
    const up = buildPatch('more repulsion', repel, 100);
    expect(up.ok && up.patch.proposed).toBe(130); // 100 × 1.25 = 125, snapped to step 10
    expect(buildPatch('repulsion please', repel, 100)).toEqual({ ok: false, reason: 'missing-value' });
    expect(buildPatch('more repulsion', repel, undefined)).toEqual({ ok: false, reason: 'missing-value' });
  });
  it('toggles need exactly one polarity', () => {
    const on = buildPatch('show the labels', toggle, false);
    expect(on.ok && on.patch.proposed).toBe(true);
    expect(buildPatch('labels', toggle, false)).toEqual({ ok: false, reason: 'missing-value' });
    expect(buildPatch('turn labels on and off', toggle, false)).toEqual({ ok: false, reason: 'missing-value' });
  });
  it('select needs exactly one option named', () => {
    const r = buildPatch('quality high', select, 'low');
    expect(r.ok && r.patch.proposed).toBe('high');
    expect(buildPatch('quality', select, 'low').ok).toBe(false);
  });
  it('protected and unsupported types are refused', () => {
    expect(buildPatch('set it to 5', { path: 'auth.nostr.privateKey', label: 'k', type: 'slider' }, 1)).toEqual({ ok: false, reason: 'protected' });
    expect(buildPatch('red', { path: 'viz.color', label: 'c', type: 'color' }, '#fff')).toEqual({ ok: false, reason: 'unsupported-type' });
  });
});

describe('shortlist', () => {
  it('excludes protected paths and actions, prefers lexical overlap, bounds size', () => {
    const fields = [
      { path: 'auth.nostr.secret', label: 'Secret', type: 'text' },
      { path: 'physics.repelK', label: 'Repulsion', type: 'slider', current: 100 },
      { path: 'viz.labels', label: 'Labels', type: 'toggle' },
      { path: 'x.action', label: 'Reset', type: 'action-button' },
    ];
    const s = shortlistSettings('increase repulsion', fields, 2);
    expect(s.map((c) => c.path)).toEqual(['physics.repelK', 'viz.labels']);
    expect(s.length).toBe(2);
  });
  it('helpers', () => {
    expect(isProtectedPath('a.sessionToken')).toBe(true);
    expect(isProtectedPath('visualisation.nodes.size')).toBe(false);
    expect(parseNumber('go to 12.5%')).toEqual({ value: 12.5, percent: true });
    expect(parseNumber('nothing')).toBeNull();
  });
});
