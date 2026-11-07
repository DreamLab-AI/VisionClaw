/**
 * Live-scene label harness (ADR-2186). Reproduces the overview the operator
 * reported with no labels: 9,679 nodes spread over a ±1,400 cube, the fit
 * camera ~2,000 units out (fov 75, near 0.1, far 5,000 as GraphCanvas sets
 * it), viewport 1280×900, stored budgets 40 / 40 / 100 at 13 px, no
 * highlight bits, no analytics (degree fallback). Drives LabelBudgetRunner
 * exactly as InstancedLabels does, then the glyph fill exactly as its loop
 * does (fontSize = fontPx × worldPerPx at the label's depth, wrap width via
 * labelMaxWidth). The atlas is synthetic: jsdom has no 2D canvas, so glyph
 * advances are typical system-ui ones (0.25–0.6 em).
 */
import { describe, it, expect, beforeAll } from 'vitest';
import * as THREE from 'three';
import { LabelBudgetRunner, worldPerPx, labelMaxWidth } from '../labelRunner';
import { layoutTextInline } from '../../../../rendering/text/textLayout';
import type { GlyphAtlasResult, GlyphMetrics } from '../../../../rendering/text/GlyphAtlas';
import { DEFAULT_LABELS_SETTINGS } from '../../../../api/settings/defaults';
import type { Node as GraphNode } from '../../managers/graphDataManager';

const N = 9679;
const VIEW = { width: 1280, height: 900 };
const BUDGETS = { budget: 40, dynamicBudget: 40, selectedBudget: 100 };
const FONT_PX = 13;

/** mulberry32: deterministic PRNG. */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function syntheticAtlas(): GlyphAtlasResult {
  const metrics = new Map<string, GlyphMetrics>();
  const r = rng(7);
  for (let c = 32; c < 127; c++) {
    const ch = String.fromCharCode(c);
    const advance = ch === ' ' ? 0.25 : 0.25 + r() * 0.35;
    metrics.set(ch, { u: 0, v: 0, w: 0.03, h: 0.05, advance, xOffset: 0, yOffset: 0 });
  }
  return { texture: null as unknown as THREE.CanvasTexture, metrics, lineHeight: 52 / 48 };
}

interface Scene {
  nodes: GraphNode[];
  indexMap: Map<string, number>;
  degree: Map<string, number>;
  positions: Float32Array;
  camera: THREE.PerspectiveCamera;
}

function liveScene(): Scene {
  const r = rng(1793992585);
  const gauss = () => Math.sqrt(-2 * Math.log(r() || 1e-9)) * Math.cos(2 * Math.PI * r());
  const clamp = (v: number) => Math.max(-1400, Math.min(1400, v));
  const nodes: GraphNode[] = [];
  const indexMap = new Map<string, number>();
  const degree = new Map<string, number>();
  const positions = new Float32Array(N * 3);
  // power-law degrees, max ~900, assigned to shuffled ranks
  const rank = Array.from({ length: N }, (_, i) => i);
  for (let i = N - 1; i > 0; i--) { const j = Math.floor(r() * (i + 1)); [rank[i], rank[j]] = [rank[j], rank[i]]; }
  for (let i = 0; i < N; i++) {
    const id = i < 84 ? `kg-${i}` : `onto-${i}`;
    // σ≈590 gives |y| p50≈400 as on the live scene
    const x = clamp(gauss() * 590), y = clamp(gauss() * 590), z = clamp(gauss() * 590);
    positions[i * 3] = x; positions[i * 3 + 1] = y; positions[i * 3 + 2] = z;
    nodes.push({ id, label: `Ontology Class ${i}`, position: { x, y, z }, metadata: {} } as unknown as GraphNode);
    indexMap.set(id, i);
    degree.set(id, Math.floor(900 / Math.pow(rank[i] + 1, 0.9)) + 1);
  }
  const camera = new THREE.PerspectiveCamera(75, VIEW.width / VIEW.height, 0.1, 5000);
  camera.position.set(0, 0, 2000);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld();
  return { nodes, indexMap, degree, positions, camera };
}

/** The InstancedLabels glyph fill for one label title at `distance`. */
function fillTitle(atlas: GlyphAtlasResult, camera: THREE.Camera, text: string, distance: number): number {
  const cap = 256;
  const fontSize = FONT_PX * worldPerPx(camera, distance, VIEW.height);
  return layoutTextInline(
    [{ text, color: '#ffffff', fontSize }], atlas, labelMaxWidth(DEFAULT_LABELS_SETTINGS.maxLabelWidth, fontSize),
    new Float32Array(cap * 2), new Float32Array(cap * 2), new Float32Array(cap * 4),
    new Float32Array(cap * 3), new Float32Array(cap), new Float32Array(cap * 3),
    0, 0, 0, 1, 0, cap, new THREE.Color() as unknown as { r: number; g: number; b: number; set(c: string): void },
  );
}

describe('labels on the live overview scene (ADR-2186)', () => {
  let s: Scene;
  let atlas: GlyphAtlasResult;
  beforeAll(() => { s = liveScene(); atlas = syntheticAtlas(); });

  it('the runner selects the full overview + dynamic budget, all on screen', () => {
    const runner = new LabelBudgetRunner();
    const sel = runner.run(0, s.nodes, s.indexMap, s.degree, s.positions, s.camera, VIEW, BUDGETS, FONT_PX, 90);
    expect(runner.prioritySource).toBe('degree');
    let onScreen = 0;
    for (let k = 0; k < sel.count; k++) {
      const i = sel.nodes[k];
      const x = runner.screenX(i), y = runner.screenY(i);
      if (x >= 0 && x <= VIEW.width && y >= 0 && y <= VIEW.height) onScreen++;
    }
    expect(sel.count).toBeGreaterThanOrEqual(60);
    expect(onScreen).toBe(sel.count);
  });

  it('every selected label writes its whole title into the glyph buffer', () => {
    const runner = new LabelBudgetRunner();
    const sel = runner.run(0, s.nodes, s.indexMap, s.degree, s.positions, s.camera, VIEW, BUDGETS, FONT_PX, 90);
    const v = new THREE.Vector3();
    let glyphs = 0, labelled = 0;
    for (let k = 0; k < sel.count; k++) {
      const ri = runner.renderIndexOf(sel.nodes[k]);
      v.set(s.positions[ri * 3], s.positions[ri * 3 + 1], s.positions[ri * 3 + 2]);
      const g = fillTitle(atlas, s.camera, 'Ontology', v.distanceTo(s.camera.position));
      glyphs += g;
      if (g === 'Ontology'.length) labelled++;
    }
    expect(sel.count).toBeGreaterThan(0);
    expect(labelled).toBe(sel.count);
    expect(glyphs).toBe(sel.count * 'Ontology'.length);
  });

  it('wrap width scales with the px font: same title, same glyphs, near or far', () => {
    expect(fillTitle(atlas, s.camera, 'Ontology', 200)).toBe(8);
    expect(fillTitle(atlas, s.camera, 'Ontology', 2000)).toBe(8);
  });

  it('a long title still truncates at the stored width (~12.5 em at the default 5.0)', () => {
    const long = 'Abcdefghijklmnopqrstuvwxyz Abcdefghijklmnopqrstuvwxyz';
    const near = fillTitle(atlas, s.camera, long, 200);
    expect(near).toBe(fillTitle(atlas, s.camera, long, 2000));
    expect(near).toBeGreaterThan(10);
    expect(near).toBeLessThan(long.length);
  });
});
