/**
 * Label budget glue (ADR-2186): projects the label candidates, reads their
 * highlight bits, ranks them by priority and runs `selectLabels` — at the
 * label cadence (LABEL_REFRESH_MS) and on camera stop, never per frame.
 * Shared by both InstancedLabels paths so WebGL and WebGPU pick the same
 * labels. Grow-only buffers; no per-node allocation.
 */
import * as THREE from 'three';
import type { Node as GraphNode } from '../managers/graphDataManager';
import { highlightMask } from '../state/highlightMask';
import { nodeAnalyticsStore } from '../../analytics/store/nodeAnalyticsStore';
import {
  selectLabels, rankByPriority, labelPriorities, createLabelSelection, CollisionGrid,
  LABEL_REFRESH_MS, type LabelBudgets, type LabelSelection,
} from './labelBudget';
import {
  assignGroups, computeClusterCandidates, createClusterCandidates, createClusterGroups, createClusterScratch,
  type ClusterCandidates,
} from './clusterLabels';

const _v = new THREE.Vector3();

export class LabelBudgetRunner {
  private renderIndex = new Int32Array(0);
  private priority = new Float32Array(0);
  private order: Uint32Array = new Uint32Array(0);
  private bits = new Uint8Array(0);
  private sx = new Float32Array(0);
  private sy = new Float32Array(0);
  private onScreen = new Uint8Array(0);
  private mark = new Uint8Array(0);
  private world = new Float32Array(0);
  private groups = createClusterGroups();
  private clusterScratch = createClusterScratch();
  private _clusters: ClusterCandidates = createClusterCandidates();
  private projCam: THREE.Camera | null = null;
  private projW = 1;
  private projH = 1;
  /** Projector for cluster centroids, created once (no per-run closure). */
  private readonly project = (x: number, y: number, z: number, out: { sx: number; sy: number }): boolean => {
    _v.set(x, y, z).project(this.projCam!);
    out.sx = (_v.x * 0.5 + 0.5) * this.projW;
    out.sy = (0.5 - _v.y * 0.5) * this.projH;
    return _v.z < 1 && _v.z > -1 && _v.x >= -1 && _v.x <= 1 && _v.y >= -1 && _v.y <= 1;
  };
  private grid = new CollisionGrid();
  private _selection: LabelSelection = createLabelSelection(0);
  private rankKey: { nodes: unknown; map: unknown; cc: unknown; analytics: unknown } = { nodes: null, map: null, cc: null, analytics: null };
  private lastRunMs = -Infinity;
  private lastMaskVersion = -1;
  private wasMoving = false;
  /** 'centrality' or 'degree': what the ranking used last. */
  prioritySource: 'centrality' | 'degree' = 'degree';

  get selection(): LabelSelection { return this._selection; }
  /** Cluster label candidates of the last run; selection.clusters indexes them. */
  get clusters(): ClusterCandidates { return this._clusters; }
  /** Render index of a selected local node. */
  renderIndexOf(local: number): number { return this.renderIndex[local]; }
  screenX(local: number): number { return this.sx[local]; }
  screenY(local: number): number { return this.sy[local]; }

  /**
   * True when a refresh is due: the cadence elapsed, the camera just stopped,
   * or the mask changed. Call every frame with whether the camera is moving.
   */
  due(nowMs: number, cameraMoving: boolean): boolean {
    const stopped = this.wasMoving && !cameraMoving;
    this.wasMoving = cameraMoving;
    if (cameraMoving) return false;
    return stopped || nowMs - this.lastRunMs >= LABEL_REFRESH_MS || highlightMask.version !== this.lastMaskVersion;
  }

  private ensure(n: number, cap: number): void {
    if (this.renderIndex.length < n) {
      this.renderIndex = new Int32Array(n);
      this.priority = new Float32Array(n);
      this.bits = new Uint8Array(n);
      this.sx = new Float32Array(n);
      this.sy = new Float32Array(n);
      this.onScreen = new Uint8Array(n);
      this.mark = new Uint8Array(n);
      this.world = new Float32Array(n * 3);
      this.rankKey.nodes = null;
    }
    if (this._selection.nodes.length < cap) this._selection = createLabelSelection(cap);
  }

  run(
    nowMs: number,
    nodes: readonly GraphNode[],
    nodeIdToIndexMap: ReadonlyMap<string, number>,
    connectionCountMap: ReadonlyMap<string, number>,
    positions: Float32Array | null | undefined,
    camera: THREE.Camera,
    view: { width: number; height: number },
    budgets: LabelBudgets,
    fontPx: number,
    /** ADR-2186 D2: clusters closer than this to the camera get no cluster label (0 = always). */
    clusterMinDistance = 0,
  ): LabelSelection {
    this.lastRunMs = nowMs;
    this.lastMaskVersion = highlightMask.version;
    const n = nodes.length;
    this.ensure(n, Math.max(0, budgets.budget) + Math.max(0, budgets.dynamicBudget) + Math.max(0, budgets.selectedBudget) + 2);

    // Ranking: only when the population, the index, degrees or analytics change.
    const analytics = nodeAnalyticsStore.getIndexedBuffer(nodeIdToIndexMap as Map<string, number>);
    const k = this.rankKey;
    if (k.nodes !== nodes || k.map !== nodeIdToIndexMap || k.cc !== connectionCountMap || k.analytics !== analytics) {
      k.nodes = nodes; k.map = nodeIdToIndexMap; k.cc = connectionCountMap; k.analytics = analytics;
      for (let i = 0; i < n; i++) this.renderIndex[i] = nodeIdToIndexMap.get(String(nodes[i].id)) ?? -1;
      this.prioritySource = labelPriorities(
        this.renderIndex, n, (i) => connectionCountMap.get(String(nodes[i].id)) ?? 0, analytics, this.priority,
      );
      this.order = rankByPriority(this.priority, n);
      assignGroups(this.renderIndex, n, analytics, this.groups);
    }

    // Projection + bits.
    const mask = highlightMask.getMask();
    const W = view.width || 1;
    const H = view.height || 1;
    camera.updateMatrixWorld();
    for (let i = 0; i < n; i++) {
      const ri = this.renderIndex[i];
      this.bits[i] = ri >= 0 && ri < mask.length ? mask[ri] : 0;
      let x: number, y: number, z: number;
      if (positions && ri >= 0 && ri * 3 + 2 < positions.length) {
        x = positions[ri * 3]; y = positions[ri * 3 + 1]; z = positions[ri * 3 + 2];
      } else {
        const p = nodes[i].position;
        x = p?.x ?? 0; y = p?.y ?? 0; z = p?.z ?? 0;
      }
      this.world[i * 3] = x; this.world[i * 3 + 1] = y; this.world[i * 3 + 2] = z;
      _v.set(x, y, z).project(camera);
      const on = _v.z < 1 && _v.z > -1 && _v.x >= -1 && _v.x <= 1 && _v.y >= -1 && _v.y <= 1;
      this.onScreen[i] = on ? 1 : 0;
      this.sx[i] = (_v.x * 0.5 + 0.5) * W;
      this.sy[i] = (0.5 - _v.y * 0.5) * H;
    }

    this.projCam = camera; this.projW = W; this.projH = H;
    const cp = camera.position;
    computeClusterCandidates({
      count: n, groups: this.groups, order: this.order, bits: this.bits, world: this.world,
      camX: cp.x, camY: cp.y, camZ: cp.z, minDistance: clusterMinDistance, project: this.project,
    }, this.clusterScratch, this._clusters);

    return selectLabels({
      count: n, order: this.order, priority: this.priority, bits: this.bits,
      sx: this.sx, sy: this.sy, onScreen: this.onScreen, viewWidth: W, viewHeight: H,
      clusters: this._clusters,
      // one cell ≈ one short label: ~8 em wide, ~1.6 lines tall
      cellW: Math.max(24, fontPx * 8), cellH: Math.max(8, fontPx * 1.6),
    }, budgets, this.grid, this.mark, this._selection);
  }
}

/** World units per CSS px at `distance` for a perspective camera (labels sized in px). */
export function worldPerPx(camera: THREE.Camera, distance: number, viewHeightPx: number): number {
  const p = camera as THREE.PerspectiveCamera;
  if (!p.isPerspectiveCamera || !(viewHeightPx > 0)) return 0.01;
  return (2 * Math.tan((p.fov * Math.PI) / 360) * Math.max(distance, 1e-3)) / viewHeightPx;
}

/**
 * The world-unit font size `labels.maxLabelWidth` was calibrated against
 * before ADR-2186 sized labels in px (`desktopFontSize` default 0.4): the
 * stored width is in units of 1/0.4 em, so the default 5.0 ≈ 12.5 em.
 */
const LEGACY_LABEL_FONT_SIZE = 0.4;

/**
 * Glyph-layout wrap width (world units) for a line of `fontSize` world units.
 * Scales with the font, so a label truncates at the same number of ems near
 * or far; a fixed world width (the pre-ADR-2186 behaviour) is narrower than
 * one glyph once labels are px-sized at overview distance, and no label draws.
 */
export function labelMaxWidth(maxLabelWidth: number, fontSize: number): number {
  return (maxLabelWidth / LEGACY_LABEL_FONT_SIZE) * fontSize;
}
