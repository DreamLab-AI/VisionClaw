/**
 * Per-frame derived render state, computed once per frame before any
 * component draws: smoothed positions (10 Hz frames → 60 fps), semantic-zoom
 * weights per region with members springing out of their aggregate, filter
 * visibility, focus+context styling and heat. Components read these arrays.
 */
import * as THREE from 'three';
import { kHop, communityRgb, type GraphModel } from '../graph';
import { frameVersion, tierIndex, type NodeBuffers } from '../protocol';
import type { CsState } from '../store';
import { heatAt } from '../timeline';
import { contextStyle, regionZoom, springFactor, streamedWeight, zoomFromDistance } from './lod';

export const FLAG_SELECTED = 1;
export const FLAG_HOVER = 2;
export const FLAG_DESAT = 4;
export const FLAG_HIGHLIGHT = 8;

export class FrameState {
  model: GraphModel | null = null;
  n = 0;
  /** Smoothed live positions. */
  smooth = new Float32Array(0);
  /** Displayed positions after LOD springing. */
  display = new Float32Array(0);
  /** Tier visibility × filter visibility, 0..1 (size multiplier). */
  weight = new Float32Array(0);
  /** Focus+context opacity (1 or 0.15). */
  alpha = new Float32Array(0);
  flags = new Uint8Array(0);
  heat = new Float32Array(0);
  /** Visible after filters (1/0), before tier weight. */
  visible = new Uint8Array(0);
  /** Node indices in tier order (L0, then L1, then L2) so parents resolve first. */
  order = new Uint32Array(0);
  /** Per-node zoom (0..2) of its region. */
  zoom = new Float32Array(0);
  /** Region radius in world units per L0 index (0 for non-aggregates). */
  regionRadius = new Float32Array(0);
  /** k-hop neighbourhood of the focus (index → hop). */
  hop = new Map<number, number>();
  hasFocus = false;
  selectedIndex = -1;
  hoverIndex = -1;
  /** Global zoom from camera distance (0..2). */
  globalZoom = 0;
  sceneRadius = 400;
  centre = new THREE.Vector3();
  /** Bumped whenever display/weight/alpha/flags changed this frame. */
  version = 0;
  /** Bumped when the model (index space) changes. */
  modelVersion = -1;
  /** Community → rgb cache. */
  private palette = new Map<number, [number, number, number]>();
  private lastBufVersion = -1;
  private lastStateKey = '';
  private converging = true;
  private frameNo = 0;
  private lastRegionZoom = new Float32Array(0);
  private heatOverride: Map<number, number> | null = null;
  private heatOverrideAt = -1;

  colour(c: number): [number, number, number] {
    let v = this.palette.get(c);
    if (!v) {
      v = communityRgb(c);
      this.palette.set(c, v);
    }
    return v;
  }

  private resize(model: GraphModel, buffers: NodeBuffers): void {
    const n = model.n;
    const oldIndex = this.model?.index;
    const oldSmooth = this.smooth;
    this.model = model;
    this.n = n;
    this.smooth = new Float32Array(n * 3);
    this.display = new Float32Array(n * 3);
    this.weight = new Float32Array(n);
    this.alpha = new Float32Array(n).fill(1);
    this.flags = new Uint8Array(n);
    this.heat = new Float32Array(n);
    this.visible = new Uint8Array(n).fill(1);
    this.zoom = new Float32Array(n);
    this.regionRadius = new Float32Array(n);
    this.lastRegionZoom = new Float32Array(n).fill(-1);
    for (let i = 0; i < n; i++) {
      const j = oldIndex?.get(model.ids[i]);
      const src = j !== undefined ? oldSmooth : buffers.positions;
      const k = j !== undefined ? j : i;
      this.smooth[i * 3] = src[k * 3];
      this.smooth[i * 3 + 1] = src[k * 3 + 1];
      this.smooth[i * 3 + 2] = src[k * 3 + 2];
    }
    const order = Array.from({ length: n }, (_, i) => i).sort((a, b) => model.tier[a] - model.tier[b]);
    this.order = Uint32Array.from(order);
    this.lastStateKey = '';
    this.converging = true;
    this.computeExtent();
  }

  private computeExtent(): void {
    const n = this.n;
    if (!n) return;
    let cx = 0;
    let cy = 0;
    let cz = 0;
    for (let i = 0; i < n; i++) {
      cx += this.smooth[i * 3];
      cy += this.smooth[i * 3 + 1];
      cz += this.smooth[i * 3 + 2];
    }
    cx /= n;
    cy /= n;
    cz /= n;
    let s2 = 0;
    for (let i = 0; i < n; i++) {
      const dx = this.smooth[i * 3] - cx;
      const dy = this.smooth[i * 3 + 1] - cy;
      const dz = this.smooth[i * 3 + 2] - cz;
      s2 += dx * dx + dy * dy + dz * dz;
    }
    this.centre.set(cx, cy, cz);
    this.sceneRadius = Math.max(1, Math.sqrt(s2 / n) * 1.6);
    // Region radii: RMS spread of members around their aggregate.
    const m = this.model!;
    const sum = new Float64Array(n);
    const cnt = new Uint32Array(n);
    for (let i = 0; i < n; i++) {
      const r = m.region[i];
      if (r < 0 || r === i) continue;
      const dx = this.smooth[i * 3] - this.smooth[r * 3];
      const dy = this.smooth[i * 3 + 1] - this.smooth[r * 3 + 1];
      const dz = this.smooth[i * 3 + 2] - this.smooth[r * 3 + 2];
      sum[r] += dx * dx + dy * dy + dz * dz;
      cnt[r]++;
    }
    for (let i = 0; i < n; i++) if (m.tier[i] === 0) this.regionRadius[i] = cnt[i] ? Math.sqrt(sum[i] / cnt[i]) * 1.4 : this.sceneRadius * 0.05;
  }

  /**
   * Advance one frame. Returns true when anything visible changed.
   * `camera` and `viewportHeight` drive per-region zoom.
   */
  update(s: CsState, buffers: NodeBuffers, camera: THREE.PerspectiveCamera, viewportHeight: number, dt: number, controlsTarget: THREE.Vector3): boolean {
    const model = s.model;
    if (!model) return false;
    if (model !== this.model) {
      this.resize(model, buffers);
      this.modelVersion++;
    }
    this.frameNo++;
    const n = this.n;
    let changed = false;

    // 1. Smooth toward the latest frame.
    const bv = frameVersion(buffers);
    if (bv !== this.lastBufVersion) {
      this.lastBufVersion = bv;
      this.converging = true;
    }
    if (this.converging) {
      const f = springFactor(dt, 0.07);
      const tgt = buffers.positions;
      let maxd = 0;
      const lim = Math.min(n * 3, tgt.length);
      for (let k = 0; k < lim; k++) {
        const d = tgt[k] - this.smooth[k];
        this.smooth[k] += d * f;
        const ad = d < 0 ? -d : d;
        if (ad > maxd) maxd = ad;
      }
      if (maxd < this.sceneRadius * 1e-5) this.converging = false;
      changed = true;
    }
    if (this.frameNo % 60 === 1) this.computeExtent();

    // 2. Focus / filters / timeline: recompute only when their inputs change.
    const sel = s.selection !== null ? (model.index.get(s.selection) ?? -1) : -1;
    const hov = s.hover !== null ? (model.index.get(s.hover) ?? -1) : -1;
    const timelineT = s.timeline.mode === 'live' ? null : s.timeline.t;
    const stateKey = `${s.tierMode}|${s.currentTier}|${s.modelVersion}|${sel}|${hov}|${s.focusHops}|${s.highlight ? s.highlight.ids.length + ':' + s.highlight.command : ''}|${s.filter.langs.join(',')}|${s.filter.community}|${s.filter.hideTests}|${timelineT === null ? 'live' : Math.round(timelineT / 250)}`;
    if (stateKey !== this.lastStateKey) {
      this.lastStateKey = stateKey;
      this.selectedIndex = sel;
      this.hoverIndex = hov;
      const seeds: number[] = sel >= 0 ? [sel] : [];
      this.hop = seeds.length ? kHop(model, seeds, s.focusHops, 60_000) : new Map();
      const hl = new Set<number>();
      if (s.highlight) {
        for (const id of [...s.highlight.ids, ...s.highlight.path]) {
          const i = model.index.get(id);
          if (i !== undefined) {
            hl.add(i);
            if (!this.hop.has(i)) this.hop.set(i, 0);
          }
        }
      }
      this.hasFocus = this.hop.size > 0;
      const langs = s.filter.langs;
      const nodes = model.snapshot.nodes;
      for (let i = 0; i < n; i++) {
        let vis = 1;
        const nd = nodes[i];
        if (langs.length && model.tier[i] !== 0 && !langs.includes(nd.lang)) vis = 0;
        if (s.filter.community !== null && model.tier[i] !== 0 && buffers.community[i] !== s.filter.community) vis = 0;
        if (s.filter.hideTests && model.isTest[i]) vis = 0;
        if (timelineT !== null) {
          const added = s.addedAt[nd.id];
          if (added !== undefined && added > timelineT) vis = 0;
        }
        this.visible[i] = vis;
        const cs = contextStyle(this.hasFocus, this.hop.get(i), s.focusHops);
        this.alpha[i] = cs.alpha;
        let fl = cs.desaturate ? FLAG_DESAT : 0;
        if (i === sel) fl |= FLAG_SELECTED;
        if (i === hov) fl |= FLAG_HOVER;
        if (hl.has(i)) fl |= FLAG_HIGHLIGHT;
        this.flags[i] = fl;
      }
      if (timelineT !== null) {
        this.heatOverride = heatAt(s.presence, timelineT);
        this.heatOverrideAt = timelineT;
      } else {
        this.heatOverride = null;
        this.heatOverrideAt = -1;
      }
      changed = true;
    }

    // 3. Heat (live from frames, or replayed).
    if (this.heatOverride && this.heatOverrideAt >= 0) {
      this.heat.fill(0);
      this.heatOverride.forEach((h, id) => {
        const i = model.index.get(id);
        if (i !== undefined) this.heat[i] = h;
      });
    } else if (changed) {
      this.heat.set(buffers.heat.subarray(0, n));
    }

    // 4. Semantic zoom per region.
    const dist = camera.position.distanceTo(controlsTarget);
    this.globalZoom = s.tierMode === 'auto' ? zoomFromDistance(dist, this.sceneRadius) : s.tierMode === 'l0' ? 0 : s.tierMode === 'l1' ? 1 : 2;
    const fovScale = viewportHeight / 2 / Math.tan((camera.fov * Math.PI) / 360);
    const tmp = new THREE.Vector3();
    let zoomChanged = false;
    for (let i = 0; i < n; i++) {
      if (model.tier[i] !== 0) continue;
      let z: number;
      if (s.tierMode !== 'auto') z = this.globalZoom;
      else {
        tmp.set(this.smooth[i * 3], this.smooth[i * 3 + 1], this.smooth[i * 3 + 2]);
        const d = Math.max(1e-3, tmp.distanceTo(camera.position));
        z = regionZoom((this.regionRadius[i] * fovScale) / d);
      }
      if (Math.abs(z - this.lastRegionZoom[i]) > 1e-3) {
        this.lastRegionZoom[i] = z;
        zoomChanged = true;
      }
      this.zoom[i] = z;
    }
    if (!changed && !zoomChanged) return false;

    // 5. Display positions & weights, parents first. A region may only open as
    // far as the server streams (View tier, plus the focused file's symbols);
    // deeper nodes stay collapsed into their parent instead of sitting at
    // stale seed positions. The demo streams everything.
    const streamTier = s.demo ? 2 : tierIndex(s.tierMode === 'auto' ? s.currentTier : s.tierMode);
    let focusFile = -1;
    if (sel >= 0) {
      let j = sel;
      while (j >= 0 && model.tier[j] > 1) j = model.parent[j];
      if (j >= 0 && model.tier[j] === 1) focusFile = j;
    }
    const order = this.order;
    for (let k = 0; k < n; k++) {
      const i = order[k];
      const t = model.tier[i];
      const r = model.region[i];
      const z = r >= 0 ? this.zoom[r] : this.globalZoom;
      this.zoom[i] = z;
      const w = streamedWeight(t, z, streamTier, model.parent[i] === focusFile);
      const vis = this.visible[i];
      // Selected / highlighted nodes stay open regardless of zoom.
      const pinned = (this.flags[i] & (FLAG_SELECTED | FLAG_HIGHLIGHT)) !== 0;
      const ww = pinned ? 1 : w;
      this.weight[i] = vis ? ww : 0;
      const p = model.parent[i];
      const a = i * 3;
      if (t === 0 || p < 0 || ww >= 0.999) {
        this.display[a] = this.smooth[a];
        this.display[a + 1] = this.smooth[a + 1];
        this.display[a + 2] = this.smooth[a + 2];
      } else {
        const b = p * 3;
        this.display[a] = this.display[b] + (this.smooth[a] - this.display[b]) * ww;
        this.display[a + 1] = this.display[b + 1] + (this.smooth[a + 1] - this.display[b + 1]) * ww;
        this.display[a + 2] = this.display[b + 2] + (this.smooth[a + 2] - this.display[b + 2]) * ww;
      }
    }
    this.version++;
    return true;
  }
}
