/**
 * Labels: a glyph atlas (ASCII, CanvasTexture, built once) drawn as instanced
 * screen-aligned quads. Every few frames ≤ 300 labels are chosen by
 * importance / distance² with 2D grid collision rejection; focus, hover,
 * highlights and actor foci are pinned and always win.
 */
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { useCs } from '../store';
import { useScene } from './context';
import { FLAG_HIGHLIGHT, FLAG_HOVER, FLAG_SELECTED } from './frameState';
import { labelScore, selectLabels, topK, type LabelCandidate } from './labelSelect';

export const MAX_LABELS = 300;
const MAX_CHARS = 32;
const FIRST = 32;
const LAST = 126;
const CELL_W = 48;
const CELL_H = 56;
const COLS = 16;
const FONT_PX = 36;

export interface GlyphAtlas {
  texture: THREE.CanvasTexture;
  /** Advance per glyph in atlas px (index = code - 32). */
  advance: Float32Array;
  cols: number;
  rows: number;
  cellW: number;
  cellH: number;
  fontPx: number;
}

export function buildGlyphAtlas(): GlyphAtlas {
  const count = LAST - FIRST + 1;
  const rows = Math.ceil(count / COLS);
  const canvas = document.createElement('canvas');
  canvas.width = COLS * CELL_W;
  canvas.height = rows * CELL_H;
  const ctx = canvas.getContext('2d');
  const advance = new Float32Array(count);
  if (ctx) {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.font = `600 ${FONT_PX}px "Inter", "Segoe UI", system-ui, sans-serif`;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    ctx.lineJoin = 'round';
    for (let k = 0; k < count; k++) {
      const ch = String.fromCharCode(FIRST + k);
      const x = (k % COLS) * CELL_W + 4;
      const y = Math.floor(k / COLS) * CELL_H + CELL_H / 2;
      advance[k] = Math.min(CELL_W - 12, Math.max(8, ctx.measureText(ch).width));
      ctx.lineWidth = 7;
      ctx.strokeStyle = 'rgba(0,0,0,0.9)';
      ctx.strokeText(ch, x, y);
      ctx.fillStyle = '#ffffff';
      ctx.fillText(ch, x, y);
    }
  } else {
    advance.fill(FONT_PX * 0.6);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.generateMipmaps = true;
  texture.colorSpace = THREE.NoColorSpace;
  return { texture, advance, cols: COLS, rows, cellW: CELL_W, cellH: CELL_H, fontPx: FONT_PX };
}

/** Pixel width of `text` at `px` font size using the atlas advances. */
export function textWidth(atlas: Pick<GlyphAtlas, 'advance' | 'fontPx'>, text: string, px: number): number {
  let w = 0;
  for (let k = 0; k < text.length; k++) {
    const c = text.charCodeAt(k);
    w += atlas.advance[(c >= FIRST && c <= LAST ? c : 63) - FIRST];
  }
  return (w * px) / atlas.fontPx;
}

const VERT = /* glsl */ `
attribute vec3 aAnchor;
attribute vec4 aGlyph;   // x offset px, y offset px, glyph width px (screen), glyph index
attribute vec4 aColor;
attribute float aPx;     // font px on screen
uniform vec2 uViewport;
uniform vec2 uAtlas;     // cols, rows
uniform vec2 uCell;      // cell w, h (atlas px)
uniform float uFontPx;
varying vec2 vUv;
varying vec4 vColor;
void main() {
  vec4 clip = projectionMatrix * modelViewMatrix * vec4(aAnchor, 1.0);
  float scale = aPx / uFontPx;
  vec2 cellPx = uCell * scale;
  // position.xy in [0,1]^2 over the glyph cell; glyph quad is the full cell height, advance-trimmed width.
  vec2 corner = vec2(position.x * (aGlyph.z + 8.0 * scale), (position.y - 0.5) * cellPx.y);
  vec2 px = vec2(aGlyph.x - 4.0 * scale, aGlyph.y) + corner;
  clip.xy += px / (uViewport * 0.5) * clip.w;
  gl_Position = clip;
  float idx = aGlyph.w;
  float col = mod(idx, uAtlas.x);
  float row = floor(idx / uAtlas.x);
  float u0 = col / uAtlas.x;
  float uw = (aGlyph.z / scale + 8.0) / (uCell.x * uAtlas.x);
  float v1 = 1.0 - row / uAtlas.y;
  vUv = vec2(u0 + position.x * uw, v1 - (1.0 - position.y) / uAtlas.y);
  vColor = aColor;
}
`;
const FRAG = /* glsl */ `
uniform sampler2D uMap;
varying vec2 vUv;
varying vec4 vColor;
void main() {
  vec4 t = texture2D(uMap, vUv);
  if (t.a < 0.04) discard;
  // White fill tinted by colour; the black outline stays black.
  gl_FragColor = vec4(t.rgb * vColor.rgb, t.a * vColor.a);
}
`;

export function Labels() {
  const { session, frame } = useScene();
  const model = useCs((s) => s.model);
  const actors = useCs((s) => s.actors);
  const { size, camera } = useThree();
  const atlas = useMemo(() => buildGlyphAtlas(), []);
  const capacity = MAX_LABELS * MAX_CHARS + 64 * MAX_CHARS;
  const geometry = useMemo(() => {
    const g = new THREE.InstancedBufferGeometry();
    const quad = new THREE.PlaneGeometry(1, 1);
    quad.translate(0.5, 0.5, 0);
    g.index = quad.index;
    g.setAttribute('position', quad.getAttribute('position'));
    g.setAttribute('aAnchor', new THREE.InstancedBufferAttribute(new Float32Array(capacity * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aGlyph', new THREE.InstancedBufferAttribute(new Float32Array(capacity * 4), 4).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aColor', new THREE.InstancedBufferAttribute(new Float32Array(capacity * 4), 4).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aPx', new THREE.InstancedBufferAttribute(new Float32Array(capacity), 1).setUsage(THREE.DynamicDrawUsage));
    g.instanceCount = 0;
    g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
    return g;
  }, [capacity]);
  const material = useMemo(
    () =>
      new THREE.ShaderMaterial({
        vertexShader: VERT,
        fragmentShader: FRAG,
        uniforms: {
          uMap: { value: atlas.texture },
          uViewport: { value: new THREE.Vector2(1920, 1080) },
          uAtlas: { value: new THREE.Vector2(atlas.cols, atlas.rows) },
          uCell: { value: new THREE.Vector2(atlas.cellW, atlas.cellH) },
          uFontPx: { value: atlas.fontPx },
        },
        transparent: true,
        depthTest: false,
        depthWrite: false,
      }),
    [atlas],
  );
  useEffect(() => () => {
    geometry.dispose();
    material.dispose();
    atlas.texture.dispose();
  }, [geometry, material, atlas]);

  const chosen = useRef<number[]>([]);
  const frameNo = useRef(0);
  const lastCam = useRef(new THREE.Matrix4());

  useFrame(() => {
    const m = frame.model;
    if (!m || m !== model) return;
    (material.uniforms.uViewport.value as THREE.Vector2).set(size.width, size.height);
    frameNo.current++;
    const camMoved = !lastCam.current.equals(camera.matrixWorld);
    if (camMoved) lastCam.current.copy(camera.matrixWorld);
    if (frameNo.current % 4 !== 0 && !(camMoved && frameNo.current % 2 === 0) && chosen.current.length) {
      // Positions still move between selections: just refresh anchors below.
    } else {
      chosen.current = chooseLabels();
    }
    writeInstances(chosen.current);
  });

  function chooseLabels(): number[] {
    const m = frame.model!;
    const n = m.n;
    const vp = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    const e = vp.elements;
    const cam = camera.position;
    const pinned = new Set<number>();
    if (frame.selectedIndex >= 0) pinned.add(frame.selectedIndex);
    if (frame.hoverIndex >= 0) pinned.add(frame.hoverIndex);
    for (const a of Object.values(actors)) {
      const i = a.focus !== null ? m.index.get(a.focus) : undefined;
      if (i !== undefined) pinned.add(i);
    }
    const hw = size.width / 2;
    const hh = size.height / 2;
    const cands: LabelCandidate[] = [];
    const project = (i: number): { x: number; y: number; d: number } | null => {
      const x = frame.display[i * 3];
      const y = frame.display[i * 3 + 1];
      const z = frame.display[i * 3 + 2];
      const w = e[3] * x + e[7] * y + e[11] * z + e[15];
      if (w <= 0.01) return null;
      const nx = (e[0] * x + e[4] * y + e[8] * z + e[12]) / w;
      const ny = (e[1] * x + e[5] * y + e[9] * z + e[13]) / w;
      if (nx < -1.1 || nx > 1.1 || ny < -1.1 || ny > 1.1) return null;
      const dx = x - cam.x;
      const dy = y - cam.y;
      const dz = z - cam.z;
      return { x: (nx + 1) * hw, y: (1 - ny) * hh, d: Math.sqrt(dx * dx + dy * dy + dz * dz) };
    };
    function* scored(): Generator<LabelCandidate> {
      for (let i = 0; i < n; i++) {
        if (frame.weight[i] < 0.6 || frame.alpha[i] < 0.5 || pinned.has(i)) continue;
        const p = project(i);
        if (!p) continue;
        const imp = (frame.flags[i] & FLAG_HIGHLIGHT ? 2 : 0) + (m.tier[i] === 0 ? 1.5 : m.tier[i] === 1 ? 0.35 : 0) + (session.pipeline.buffers.centrality[i] || m.importance[i]);
        yield { index: i, x: p.x, y: p.y, w: 0, h: 0, score: labelScore(imp, p.d) };
      }
    }
    const best = topK(scored(), 1500);
    for (const i of pinned) {
      const p = project(i);
      if (p) best.unshift({ index: i, x: p.x, y: p.y, w: 0, h: 0, score: Infinity, pinned: true });
    }
    for (const c of best) {
      const px = fontPx(c.index);
      const text = labelText(c.index);
      c.w = textWidth(atlas, text, px) + 6;
      c.h = px * 1.3;
      c.y -= 8;
      cands.push(c);
    }
    return selectLabels(cands, { max: MAX_LABELS, width: size.width, height: size.height, cell: 14 });
  }

  function fontPx(i: number): number {
    const m = frame.model!;
    const base = m.tier[i] === 0 ? 17 : m.tier[i] === 1 ? 13 : 12;
    return frame.flags[i] & (FLAG_SELECTED | FLAG_HOVER) ? base + 3 : base;
  }

  function labelText(i: number): string {
    const m = frame.model!;
    const l = m.snapshot.nodes[i].label;
    const t = m.tier[i] === 0 ? `${l}/` : l;
    return t.length > MAX_CHARS ? `${t.slice(0, MAX_CHARS - 1)}~` : t;
  }

  function writeInstances(ids: number[]): void {
    const anchor = geometry.getAttribute('aAnchor').array as Float32Array;
    const glyph = geometry.getAttribute('aGlyph').array as Float32Array;
    const color = geometry.getAttribute('aColor').array as Float32Array;
    const pxA = geometry.getAttribute('aPx').array as Float32Array;
    let g = 0;
    for (const i of ids) {
      const text = labelText(i);
      const px = fontPx(i);
      const scale = px / atlas.fontPx;
      const width = textWidth(atlas, text, px);
      const pinnedLabel = (frame.flags[i] & (FLAG_SELECTED | FLAG_HOVER | FLAG_HIGHLIGHT)) !== 0;
      const tier = frame.model!.tier[i];
      const rgb: [number, number, number] = pinnedLabel ? [1, 0.95, 0.75] : tier === 0 ? [0.92, 0.95, 1] : [0.78, 0.82, 0.88];
      const a = Math.min(1, frame.weight[i] * 1.2) * (frame.alpha[i] < 1 && !pinnedLabel ? 0.5 : 1);
      let x = -width / 2;
      const y = 10 + px * 0.6;
      for (let k = 0; k < text.length && g < capacity; k++) {
        const c = text.charCodeAt(k);
        const gi = (c >= FIRST && c <= LAST ? c : 63) - FIRST;
        const adv = atlas.advance[gi] * scale;
        if (c !== 32) {
          anchor[g * 3] = frame.display[i * 3];
          anchor[g * 3 + 1] = frame.display[i * 3 + 1];
          anchor[g * 3 + 2] = frame.display[i * 3 + 2];
          glyph[g * 4] = x;
          glyph[g * 4 + 1] = y;
          glyph[g * 4 + 2] = adv;
          glyph[g * 4 + 3] = gi;
          color[g * 4] = rgb[0];
          color[g * 4 + 1] = rgb[1];
          color[g * 4 + 2] = rgb[2];
          color[g * 4 + 3] = a;
          pxA[g] = px;
          g++;
        }
        x += adv;
      }
    }
    for (const name of ['aAnchor', 'aGlyph', 'aColor', 'aPx']) {
      const attr = geometry.getAttribute(name) as THREE.InstancedBufferAttribute;
      attr.clearUpdateRanges();
      attr.addUpdateRange(0, g * attr.itemSize);
      attr.needsUpdate = true;
    }
    geometry.instanceCount = g;
  }

  if (!model) return null;
  return <mesh geometry={geometry} material={material} frustumCulled={false} renderOrder={10} raycast={() => null} />;
}
