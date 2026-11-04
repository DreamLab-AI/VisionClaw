import type * as THREE from 'three';
import { createLogger } from '../../utils/loggerConfig';
import { placementFor, type Sphere, type SplatMode } from './fit';
import type { SplatBackendKind, SplatLayer } from './SplatLayer';
import { createSplatLayer, pickBackendKind, renderPathFor } from './selectBackend';
import { loadVariantBytes, SplatIntegrityError } from './splatCache';
import { beginIn, beginOut, STEADY, stepFade, type Fade } from './transition';
import type { SplatAsset, SplatVariant } from './types';
import { chooseVariant } from './variants';

const logger = createLogger('SplatController');

export type SplatStatus = 'idle' | 'loading' | 'ready' | 'refused' | 'error' | 'no-variant';

/** What the viewer has asked to see. Null removes the cloud. */
export interface SplatRequest {
  asset: SplatAsset;
  mode: SplatMode;
}

export interface SplatControllerDeps {
  createLayer?: typeof createSplatLayer;
  loadBytes?: typeof loadVariantBytes;
  /** the graph's bounding sphere right now, or null before it exists */
  graphSphere: () => Sphere | null;
  reducedMotion: () => boolean;
  onStatus: (status: SplatStatus, detail: { variant: SplatVariant | null; message?: string }) => void;
}

type Pending =
  | { kind: 'remove' }
  | { kind: 'place'; request: SplatRequest }
  | { kind: 'replace'; request: SplatRequest; variant: SplatVariant; layer: SplatLayer };

/**
 * Owns the one live splat layer of a scene: backend choice at mount, variant
 * choice, verified load, placement, transitions and disposal. No React and no
 * three scene knowledge beyond the `parent` it attaches to, which is always
 * scene level (never a graph node or the graph root).
 */
export class SplatController {
  private layer: SplatLayer | null = null;
  private current: { assetId: string; variant: SplatVariant; mode: SplatMode; asset: SplatAsset } | null = null;
  private pending: Pending | null = null;
  private fade: Fade = STEADY;
  private token = 0;
  /** the load in progress, so a repeat request for the same blob never fetches twice */
  private inflight: { address: string; token: number; req: SplatRequest } | null = null;
  private disposed = false;
  private readonly kind: SplatBackendKind;

  constructor(
    private readonly gl: unknown,
    private readonly parent: THREE.Object3D,
    private readonly deps: SplatControllerDeps,
  ) {
    this.kind = pickBackendKind(gl);
  }

  /** Ask for a cloud (or none). Safe to call every time the request changes. */
  request(req: SplatRequest | null): void {
    if (this.disposed) return;
    if (req) {
      const wanted = chooseVariant(req.asset, renderPathFor(this.kind));
      if (wanted && this.inflight?.address === wanted.address) {
        // same blob already loading: only the requested mode can differ
        this.inflight.req = req;
        return;
      }
    }
    const token = ++this.token;
    this.inflight = null;
    if (!req) {
      this.discardPendingLayer();
      if (this.layer) this.startSwap({ kind: 'remove' });
      this.deps.onStatus('idle', { variant: null });
      return;
    }
    const variant = chooseVariant(req.asset, renderPathFor(this.kind));
    if (!variant) {
      this.discardPendingLayer();
      if (this.layer) this.startSwap({ kind: 'remove' });
      this.deps.onStatus('no-variant', { variant: null });
      return;
    }
    if (this.layer && this.current?.variant.address === variant.address) {
      this.discardPendingLayer();
      if (this.current.mode !== req.mode) this.startSwap({ kind: 'place', request: req });
      this.deps.onStatus('ready', { variant });
      return;
    }
    this.discardPendingLayer();
    this.deps.onStatus('loading', { variant });
    this.inflight = { address: variant.address, token, req };
    void this.loadAndStage(token, variant);
  }

  /**
   * Remove the cloud in this frame, with no transition. For the low
   * visual-quality setting, which withdraws splats at once (ADR-2170 decision 9).
   */
  withdraw(): void {
    if (this.disposed) return;
    this.token++;
    this.inflight = null;
    this.discardPendingLayer();
    this.layer?.dispose();
    this.layer = null;
    this.current = null;
    this.fade = STEADY;
    this.deps.onStatus('idle', { variant: null });
  }

  /** Per-frame step: advance the transition and push opacity to the layer. */
  update(dt: number): void {
    if (this.disposed || !this.layer) return;
    const step = stepFade(this.fade, dt, this.deps.reducedMotion());
    this.fade = step.fade;
    if (step.swap) this.commit();
    this.pushOpacity();
  }

  private pushOpacity(): void {
    this.layer?.setOpacity(this.fade.opacity);
  }

  dispose(): void {
    this.disposed = true;
    this.token++;
    this.inflight = null;
    this.discardPendingLayer();
    this.layer?.dispose();
    this.layer = null;
    this.current = null;
  }

  private async loadAndStage(token: number, variant: SplatVariant): Promise<void> {
    let layer: SplatLayer | null = null;
    try {
      const bytes = await (this.deps.loadBytes ?? loadVariantBytes)(variant);
      if (token !== this.token) return;
      layer = await (this.deps.createLayer ?? createSplatLayer)(this.gl, this.kind);
      await layer.load(bytes);
      if (token !== this.token) {
        layer.dispose();
        return;
      }
      const req = this.inflight?.req;
      this.inflight = null;
      if (!req) {
        layer.dispose();
        return;
      }
      this.startSwap({ kind: 'replace', request: req, variant, layer });
      this.deps.onStatus('ready', { variant });
    } catch (e) {
      layer?.dispose();
      if (token !== this.token) return;
      this.inflight = null;
      const message = e instanceof Error ? e.message : String(e);
      logger.error(`splat ${variant.address} not shown: ${message}`);
      this.deps.onStatus(e instanceof SplatIntegrityError ? 'refused' : 'error', { variant, message });
    }
  }

  private startSwap(next: Pending): void {
    this.pending = next;
    const reduced = this.deps.reducedMotion();
    if (!this.layer && next.kind === 'replace') {
      // nothing on screen yet: show it straight away, fading in unless reduced
      this.commit();
      this.fade = beginIn(reduced);
      this.pushOpacity();
      return;
    }
    if (!this.layer) {
      this.pending = null;
      return;
    }
    this.fade = beginOut(this.fade, reduced);
    this.layer.setOpacity(this.fade.opacity);
  }

  private commit(): void {
    const next = this.pending;
    this.pending = null;
    if (!next) return;
    if (next.kind === 'remove') {
      this.layer?.dispose();
      this.layer = null;
      this.current = null;
      return;
    }
    if (next.kind === 'replace') {
      this.layer?.dispose();
      this.layer = next.layer;
      this.current = { assetId: next.request.asset.id, variant: next.variant, mode: next.request.mode, asset: next.request.asset };
      this.layer.attach(this.parent);
    } else if (this.current) {
      this.current.mode = next.request.mode;
    }
    if (this.layer && this.current) this.applyPlacement(this.layer, this.current.mode, this.current.asset);
  }

  private applyPlacement(layer: SplatLayer, mode: SplatMode, asset: SplatAsset): void {
    const p = placementFor(mode, asset.bounds, this.deps.graphSphere());
    layer.root.position.set(p.position[0], p.position[1], p.position[2]);
    layer.root.scale.setScalar(p.scale);
    // world-locked and static: the orientation is never animated
    layer.root.quaternion.identity();
    layer.setRenderOrder(p.renderOrder);
  }

  private discardPendingLayer(): void {
    if (this.pending?.kind === 'replace') this.pending.layer.dispose();
    this.pending = null;
  }
}
