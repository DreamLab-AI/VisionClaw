// The menu's Emphasis glow controls (visualisation.glow.*) are the only glow settings. The
// legacy visualisation.bloom.* group has no control (only the chat wrote it) and defaults
// to enabled, so it used to keep the effect on and override every glow slider.
import { describe, it, expect } from 'vitest';
import { glowEffectParams } from '../glowEffect';

describe('glowEffectParams: the Emphasis glow controls are authoritative', () => {
  it('Glow off turns the effect off even when the legacy bloom group says enabled', () => {
    expect(glowEffectParams({ glow: { enabled: false }, bloom: { enabled: true, strength: 0.4 } }).enabled).toBe(false);
  });

  it('the glow sliders set the effect, not the legacy bloom values', () => {
    const p = glowEffectParams({
      glow: { enabled: true, intensity: 0.9, radius: 0.45, threshold: 0.2 },
      bloom: { enabled: true, strength: 0.4, radius: 0.3, threshold: 0.6 },
    });
    expect(p).toEqual({ enabled: true, strength: 0.9, radius: 0.45, threshold: 0.2 });
  });
});
