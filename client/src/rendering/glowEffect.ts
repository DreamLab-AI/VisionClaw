/**
 * Which settings drive the post-processing glow (GemPostProcessing): the menu's
 * Emphasis glow group, `visualisation.glow.*`, alone. The legacy `visualisation.bloom.*`
 * group had no control, defaulted to enabled and, when enabled, both kept the effect on
 * and replaced every glow slider's value (settings audit 2026-11-06); it is retired.
 */
interface BloomLikeSettings {
  enabled?: boolean;
  strength?: number;
  intensity?: number;
  radius?: number;
  threshold?: number;
}

export interface GlowEffectParams {
  enabled: boolean;
  strength: number;
  radius: number;
  threshold: number;
}

export function glowEffectParams(vis: { glow?: BloomLikeSettings } | undefined): GlowEffectParams {
  const glow = vis?.glow;
  return {
    // Same rule as GraphCanvas (`glow.enabled !== false`): an absent key means on, the
    // shipped default; only an explicit false turns the effect off.
    enabled: glow?.enabled !== false,
    strength: glow?.strength ?? glow?.intensity ?? 0.3,
    radius: glow?.radius ?? 0.2,
    threshold: glow?.threshold ?? 0.3,
  };
}
