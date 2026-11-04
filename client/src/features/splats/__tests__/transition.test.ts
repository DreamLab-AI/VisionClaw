import { describe, expect, it } from 'vitest';
import { beginIn, beginOut, DIM_HOLD_SECONDS, DIM_OPACITY, FADE_SECONDS, STEADY, stepFade } from '../transition';

describe('transitions', () => {
  it('eases out then in over half a second, swapping at the dark point', () => {
    let f = beginOut(STEADY, false);
    let swapped = 0;
    let t = 0;
    const seen: number[] = [];
    while (f.phase !== 'steady' && t < 2) {
      const s = stepFade(f, 0.05, false);
      f = s.fade;
      if (s.swap) swapped++;
      seen.push(f.opacity);
      t += 0.05;
    }
    expect(swapped).toBe(1);
    expect(t).toBeCloseTo(FADE_SECONDS, 1);
    expect(Math.min(...seen)).toBeLessThanOrEqual(0.05);
    expect(f.opacity).toBe(1);
  });

  it('with reduced motion dims, swaps once and snaps back, never easing', () => {
    let f = beginOut(STEADY, true);
    expect(f.opacity).toBe(DIM_OPACITY);
    const mid = stepFade(f, DIM_HOLD_SECONDS / 2, true);
    expect(mid.swap).toBe(false);
    expect(mid.fade.opacity).toBe(DIM_OPACITY);
    const end = stepFade(mid.fade, DIM_HOLD_SECONDS, true);
    expect(end.swap).toBe(true);
    expect(end.fade).toEqual(STEADY);
    f = end.fade;
    expect(stepFade(f, 1, true).swap).toBe(false);
  });

  it('shows a first cloud at once under reduced motion and fades it in otherwise', () => {
    expect(beginIn(true).opacity).toBe(1);
    expect(beginIn(false)).toMatchObject({ phase: 'in', opacity: 0 });
  });
});
