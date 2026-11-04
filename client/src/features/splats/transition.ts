/**
 * Enter, leave and variant-change transitions (ADR-2170 decision 9). With
 * reduced motion the cloud dims, swaps and snaps back; otherwise it eases out
 * and in over `FADE_SECONDS`. Pure, so the timing is testable without a GPU.
 */

export const FADE_SECONDS = 0.5;
/** opacity held while a reduced-motion swap happens */
export const DIM_OPACITY = 0.2;
export const DIM_HOLD_SECONDS = 0.15;

export type FadePhase = 'steady' | 'out' | 'in';

export interface Fade {
  phase: FadePhase;
  /** seconds into the current phase */
  elapsed: number;
  opacity: number;
}

export const STEADY: Fade = { phase: 'steady', elapsed: 0, opacity: 1 };

const HALF = FADE_SECONDS / 2;

/** Start swapping out, continuing from the current opacity so a restart does not jump. */
export function beginOut(current: Fade, reduced: boolean): Fade {
  if (reduced) return { phase: 'out', elapsed: 0, opacity: DIM_OPACITY };
  return { phase: 'out', elapsed: (1 - current.opacity) * HALF, opacity: current.opacity };
}

/** Start fading a freshly shown cloud in; reduced motion shows it at once. */
export function beginIn(reduced: boolean): Fade {
  return reduced ? STEADY : { phase: 'in', elapsed: 0, opacity: 0 };
}

/**
 * Advance by `dt` seconds. `swap` is true on the step where the out phase
 * ends: the caller applies the pending change then.
 */
export function stepFade(fade: Fade, dt: number, reduced: boolean): { fade: Fade; swap: boolean } {
  if (fade.phase === 'steady') return { fade, swap: false };
  const elapsed = fade.elapsed + Math.max(dt, 0);
  if (fade.phase === 'out') {
    if (reduced) {
      return elapsed >= DIM_HOLD_SECONDS
        ? { fade: STEADY, swap: true }
        : { fade: { phase: 'out', elapsed, opacity: DIM_OPACITY }, swap: false };
    }
    if (elapsed >= HALF) return { fade: { phase: 'in', elapsed: 0, opacity: 0 }, swap: true };
    return { fade: { phase: 'out', elapsed, opacity: 1 - elapsed / HALF }, swap: false };
  }
  if (reduced || elapsed >= HALF) return { fade: STEADY, swap: false };
  return { fade: { phase: 'in', elapsed, opacity: elapsed / HALF }, swap: false };
}
