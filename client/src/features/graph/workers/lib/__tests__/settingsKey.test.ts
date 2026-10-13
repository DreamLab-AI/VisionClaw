import { describe, it, expect } from 'vitest';
import { settingsIdempotencyKey } from '../settingsKey';

describe('settingsIdempotencyKey', () => {
  const physics = { springStrength: 0.001, damping: 0.98 };
  const tween = { enabled: true, lerpBase: 0.003, snapThreshold: 0.05, maxDivergence: 50 };

  it('is stable for identical input, so unchanged pushes are skipped', () => {
    expect(settingsIdempotencyKey(physics, null, tween)).toBe(settingsIdempotencyKey({ ...physics }, null, { ...tween }));
  });

  it('changes when only a tweening value changes, so the Smooth Movement sliders reach the worker', () => {
    const before = settingsIdempotencyKey(physics, null, tween);
    expect(settingsIdempotencyKey(physics, null, { ...tween, lerpBase: 0.02 })).not.toBe(before);
    expect(settingsIdempotencyKey(physics, null, { ...tween, enabled: false })).not.toBe(before);
  });

  it('still changes when physics changes', () => {
    expect(settingsIdempotencyKey({ ...physics, damping: 0.9 }, null, tween)).not.toBe(settingsIdempotencyKey(physics, null, tween));
  });

  it('treats absent tweening and absent physics as distinct, stable states', () => {
    expect(settingsIdempotencyKey(undefined, null, undefined)).toBe(settingsIdempotencyKey(undefined, null, undefined));
    expect(settingsIdempotencyKey(undefined, null, undefined)).not.toBe(settingsIdempotencyKey(undefined, null, tween));
  });
});
