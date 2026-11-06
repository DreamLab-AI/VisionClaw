/**
 * Highlight level → visual (ADR-2183 D3, ADR-2182 D4).
 *
 * One pure mapping from a node's highlight bits (state/highlightMask.ts) to how
 * every renderer draws it, so gems, sprites, overview edges and labels agree:
 *   - GREYED: colour mixed `greyoutMix` toward the background, drawn at
 *     `greyoutOpacity` where the renderer can vary alpha, no glow, no label;
 *   - NORMAL: the encoded colour, opacity 1;
 *   - EMPHASISED: the encoded colour, larger (EMPHASIS_SCALE), and the only
 *     level that feeds selective bloom (bloomScope 'emphasis').
 *
 * Pure TS, no React, no allocation in the per-node calls.
 */
import * as THREE from 'three';
import {
  highlightLevel,
  HIGHLIGHT_LEVEL_GREYED,
  HIGHLIGHT_LEVEL_EMPHASISED,
  type HighlightLevel,
} from '../state/highlightMask';

/** Size gain of an emphasised node (sprite and gem). */
export const EMPHASIS_SCALE = 1.35;
/** Colour gain of an emphasised node before bloom (pushes it over the bloom threshold). */
export const EMPHASIS_BRIGHTEN = 1.25;

export interface GreyoutStyle {
  /** `graphView.explore.greyoutMix`: 0..1 toward the background. */
  greyoutMix: number;
  /** `graphView.explore.greyoutOpacity`: alpha of a greyed element. */
  greyoutOpacity: number;
  /** Linear RGB of the scene background. */
  background: { r: number; g: number; b: number };
}

/** Writable visual result; callers reuse one instance (zero allocation). */
export interface HighlightVisual {
  r: number;
  g: number;
  b: number;
  alpha: number;
  scale: number;
  bloom: boolean;
  label: boolean;
  level: HighlightLevel;
}

export function createHighlightVisual(): HighlightVisual {
  return { r: 0, g: 0, b: 0, alpha: 1, scale: 1, bloom: false, label: true, level: 1 };
}

const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);

/**
 * Map one node's highlight bits and encoded colour to its drawn look.
 * `bloomScope` 'emphasis' → only emphasised nodes bloom; 'scene' → every
 * non-greyed node may bloom (today); 'off' → none.
 */
export function highlightVisualFor(
  bits: number,
  r: number,
  g: number,
  b: number,
  style: GreyoutStyle,
  bloomScope: 'emphasis' | 'scene' | 'off',
  out: HighlightVisual,
): HighlightVisual {
  const level = highlightLevel(bits);
  out.level = level;
  if (level === HIGHLIGHT_LEVEL_GREYED) {
    const t = clamp01(style.greyoutMix);
    // Desaturate first (luma), then mix toward the background.
    const l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const bg = style.background;
    out.r = l + (bg.r - l) * t;
    out.g = l + (bg.g - l) * t;
    out.b = l + (bg.b - l) * t;
    out.alpha = clamp01(style.greyoutOpacity);
    out.scale = 1;
    out.bloom = false;
    out.label = false;
    return out;
  }
  if (level === HIGHLIGHT_LEVEL_EMPHASISED) {
    out.r = r * EMPHASIS_BRIGHTEN;
    out.g = g * EMPHASIS_BRIGHTEN;
    out.b = b * EMPHASIS_BRIGHTEN;
    out.alpha = 1;
    out.scale = EMPHASIS_SCALE;
    out.bloom = bloomScope !== 'off';
    out.label = true;
    return out;
  }
  out.r = r;
  out.g = g;
  out.b = b;
  out.alpha = 1;
  out.scale = 1;
  out.bloom = bloomScope === 'scene';
  out.label = true;
  return out;
}

/**
 * Alpha of an edge from its endpoints' bits: greyed when EITHER endpoint is
 * greyed (ADR-2183 D1), else 1.
 */
export function edgeHighlightAlpha(srcBits: number, tgtBits: number, greyoutOpacity: number): number {
  return highlightLevel(srcBits) === HIGHLIGHT_LEVEL_GREYED || highlightLevel(tgtBits) === HIGHLIGHT_LEVEL_GREYED
    ? clamp01(greyoutOpacity)
    : 1;
}

/** True when an edge touches an emphasised node and neither end is greyed. */
export function edgeIsEmphasised(srcBits: number, tgtBits: number): boolean {
  const a = highlightLevel(srcBits);
  const b = highlightLevel(tgtBits);
  if (a === HIGHLIGHT_LEVEL_GREYED || b === HIGHLIGHT_LEVEL_GREYED) return false;
  return a === HIGHLIGHT_LEVEL_EMPHASISED || b === HIGHLIGHT_LEVEL_EMPHASISED;
}

/** Parse a CSS colour string to linear RGB without three (tests, workers). Falls back to black. */
export function backgroundFromSetting(
  hex: string | null | undefined,
  out: { r: number; g: number; b: number } = { r: 0, g: 0, b: 0 },
): { r: number; g: number; b: number } {
  const m = typeof hex === 'string' ? /^#?([0-9a-f]{6})$/i.exec(hex.trim()) : null;
  if (!m) { out.r = 0; out.g = 0; out.b = 0; return out; }
  const n = parseInt(m[1], 16);
  const toLinear = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  out.r = toLinear((n >> 16) & 255);
  out.g = toLinear((n >> 8) & 255);
  out.b = toLinear(n & 255);
  return out;
}

// --- Backend blend-space correction (ADR-2183 D3) ---------------------------

const srgbToLinear = (c: number): number => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const linearToSrgb = (c: number): number => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

/** Linear reference colour the correction is solved for: a mid-tone edge / node. */
export const BLEND_REFERENCE_LINEAR = 0.5;

/**
 * `greyoutOpacity` means what it looks like on WebGLRenderer, which blends the
 * overview layers in sRGB space. WebGPURenderer (WebGPU, and its WebGL2
 * fallback) blends in linear space, so the same alpha looks far brighter over
 * a dark background: measured in the browser sidecar, a 0.8 grey edge at α 0.1
 * over #101018 reads 37/255 on WebGL and 82/255 on WebGPURenderer.
 *
 * Returns the alpha that, blended in linear space, reproduces the sRGB-space
 * result for the reference colour over `backgroundSrgbLuma` (0–1). Exact for
 * the reference colour, within a few 1/255 steps for brighter or dimmer ones.
 */
export function linearBlendAlpha(alpha: number, backgroundSrgbLuma: number, reference = BLEND_REFERENCE_LINEAR): number {
  const a = clamp01(alpha);
  if (a === 0 || a === 1) return a;
  const bgS = clamp01(backgroundSrgbLuma);
  const bgL = srgbToLinear(bgS);
  const refS = linearToSrgb(reference);
  if (reference - bgL <= 1e-6) return a; // background at least as bright as the reference: nothing to solve
  const targetL = srgbToLinear(a * refS + (1 - a) * bgS);
  return clamp01((targetL - bgL) / (reference - bgL));
}

/** sRGB luma (0–1) of a CSS colour, for linearBlendAlpha's background term. */
export function srgbLuma(css: string): number {
  const c = new THREE.Color();
  try { c.set(css || '#000000'); } catch { return 0; }
  const r = linearToSrgb(c.r), g = linearToSrgb(c.g), b = linearToSrgb(c.b); // THREE.Color holds linear
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
