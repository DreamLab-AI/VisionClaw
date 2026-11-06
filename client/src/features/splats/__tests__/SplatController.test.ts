import * as THREE from 'three';
import { describe, expect, it, vi } from 'vitest';
import { SplatController, type SplatControllerDeps, type SplatStatus } from '../SplatController';
import type { SplatLayer } from '../SplatLayer';
import { SplatIntegrityError } from '../splatCache';
import type { SplatAsset } from '../types';
import { assetWith } from './fixtures';

function fakeLayer() {
  const layer = {
    root: new THREE.Group(),
    load: vi.fn(async () => {}),
    attach: vi.fn(function (this: unknown, parent: THREE.Object3D) { parent.add(layer.root); }),
    detach: vi.fn(),
    setOpacity: vi.fn(),
    setRenderOrder: vi.fn(),
    dispose: vi.fn(),
  };
  return layer satisfies SplatLayer;
}

function setup(over: Partial<SplatControllerDeps> = {}) {
  const parent = new THREE.Group();
  const layers: ReturnType<typeof fakeLayer>[] = [];
  const statuses: SplatStatus[] = [];
  const deps: SplatControllerDeps = {
    createLayer: vi.fn(async () => {
      const l = fakeLayer();
      layers.push(l);
      return l;
    }),
    loadBytes: vi.fn(async () => new ArrayBuffer(8)),
    graphSphere: () => ({ centre: [0, 0, 0], radius: 100 }),
    reducedMotion: () => true,
    onStatus: (s) => statuses.push(s),
    ...over,
  };
  // WebGPURenderer on its WebGL2 backend: the `desktop` tier
  const controller = new SplatController({ isWebGPURenderer: true, backend: { isWebGLBackend: true } }, parent, deps);
  return { controller, parent, layers, statuses, deps };
}

const flush = () => new Promise((r) => setTimeout(r, 0));
const framed = (asset: SplatAsset) => ({ asset, mode: 'framed' as const });

describe('SplatController', () => {
  it('on the WebGL2 backend loads the desktop variant, attaches at scene level and places it', async () => {
    const { controller, parent, layers, statuses, deps } = setup();
    const asset = assetWith(['desktop-hi', 'desktop']);
    controller.request(framed(asset));
    await flush();
    expect(statuses).toEqual(['loading', 'ready']);
    expect((deps.loadBytes as ReturnType<typeof vi.fn>).mock.calls[0][0].name).toBe('desktop');
    expect(layers).toHaveLength(1);
    expect(layers[0].attach).toHaveBeenCalledWith(parent);
    expect(layers[0].root.scale.x).toBeGreaterThan(0);
    expect(layers[0].setRenderOrder).toHaveBeenCalledWith(0);
    expect(layers[0].root.quaternion.equals(new THREE.Quaternion())).toBe(true);
  });

  it('entering changes placement and render order without reloading', async () => {
    const { controller, layers, deps } = setup();
    const asset = assetWith(['desktop']);
    controller.request(framed(asset));
    await flush();
    controller.request({ asset, mode: 'entered' });
    controller.update(1); // reduced motion: the swap lands inside the dim hold
    expect(layers).toHaveLength(1);
    expect(deps.loadBytes).toHaveBeenCalledTimes(1);
    expect(layers[0].setRenderOrder).toHaveBeenLastCalledWith(-1000);
  });

  it('closing disposes the layer and frees it exactly once', async () => {
    const { controller, parent, layers } = setup();
    controller.request(framed(assetWith(['desktop'])));
    await flush();
    controller.request(null);
    controller.update(1);
    expect(layers[0].dispose).toHaveBeenCalledTimes(1);
    controller.dispose();
    expect(layers[0].dispose).toHaveBeenCalledTimes(1);
    expect(parent.children).toHaveLength(1); // the fake leaves root attached; the real layer's dispose detaches
  });

  it('a request superseded mid-load never attaches and its layer is freed', async () => {
    let release: (b: ArrayBuffer) => void = () => {};
    const slow = new Promise<ArrayBuffer>((r) => (release = r));
    const loadBytes = vi.fn().mockReturnValueOnce(slow).mockResolvedValue(new ArrayBuffer(8));
    const { controller, layers } = setup({ loadBytes });
    controller.request(framed(assetWith(['desktop'])));
    controller.request(null);
    release(new ArrayBuffer(8));
    await flush();
    expect(layers).toHaveLength(0);
  });

  it('disposing while a load is in flight leaves nothing behind', async () => {
    const { controller, layers, parent } = setup();
    controller.request(framed(assetWith(['desktop'])));
    controller.dispose();
    await flush();
    expect(layers.every((l) => l.attach.mock.calls.length === 0)).toBe(true);
    expect(parent.children).toHaveLength(0);
  });

  it('reports an integrity refusal and creates no layer', async () => {
    const loadBytes = vi.fn().mockRejectedValue(new SplatIntegrityError('sha256-12-aaaaaaaaaaaa', 'a', 'b'));
    const { controller, layers, statuses } = setup({ loadBytes });
    controller.request(framed(assetWith(['desktop'])));
    await flush();
    expect(statuses).toEqual(['loading', 'refused']);
    expect(layers).toHaveLength(0);
    expect(loadBytes).toHaveBeenCalledTimes(1);
  });

  it('shows nothing when the asset has no desktop variant', async () => {
    const { controller, layers, statuses, deps } = setup();
    controller.request(framed(assetWith(['xr-3', 'xr-1'])));
    await flush();
    expect(statuses).toEqual(['no-variant']);
    expect(deps.loadBytes).not.toHaveBeenCalled();
    expect(layers).toHaveLength(0);
  });

  it('never fetches the same blob twice when the mode changes mid-load', async () => {
    let release: (b: ArrayBuffer) => void = () => {};
    const loadBytes = vi.fn().mockReturnValue(new Promise<ArrayBuffer>((r) => (release = r)));
    const { controller, layers, statuses } = setup({ loadBytes });
    const asset = assetWith(['desktop']);
    controller.request(framed(asset));
    controller.request({ asset, mode: 'entered' }); // Enter pressed while loading
    controller.request({ asset: assetWith(['desktop']), mode: 'entered' }); // listing refreshed: new object, same blob
    release(new ArrayBuffer(8));
    await flush();
    expect(loadBytes).toHaveBeenCalledTimes(1);
    expect(layers).toHaveLength(1);
    expect(layers[0].setRenderOrder).toHaveBeenLastCalledWith(-1000); // the latest mode won
    expect(statuses).toEqual(['loading', 'ready']);
  });

  it('surfaces a refusal that arrives after a mid-load mode change', async () => {
    const loadBytes = vi.fn().mockRejectedValue(new SplatIntegrityError('sha256-12-aaaaaaaaaaaa', 'a', 'b'));
    const { controller, statuses } = setup({ loadBytes });
    const asset = assetWith(['desktop']);
    controller.request(framed(asset));
    controller.request({ asset, mode: 'entered' });
    await flush();
    expect(loadBytes).toHaveBeenCalledTimes(1);
    expect(statuses).toEqual(['loading', 'refused']);
  });

  it('withdraw removes the cloud in the same call, with no fade, even under eased motion', async () => {
    const { controller, layers, statuses } = setup({ reducedMotion: () => false });
    controller.request(framed(assetWith(['desktop'])));
    await flush();
    controller.withdraw();
    expect(layers[0].dispose).toHaveBeenCalledTimes(1);
    expect(statuses.at(-1)).toBe('idle');
    controller.update(0.016);
    expect(layers[0].dispose).toHaveBeenCalledTimes(1);
  });

  it('close, by contrast, keeps the transition: the layer lives until the fade ends', async () => {
    const { controller, layers } = setup({ reducedMotion: () => false });
    controller.request(framed(assetWith(['desktop'])));
    await flush();
    controller.update(1); // let the fade-in finish
    controller.request(null);
    expect(layers[0].dispose).not.toHaveBeenCalled();
    controller.update(0.1);
    expect(layers[0].dispose).not.toHaveBeenCalled();
    controller.update(0.2);
    expect(layers[0].dispose).toHaveBeenCalledTimes(1);
  });

  it('withdraw cancels a load in flight', async () => {
    let release: (b: ArrayBuffer) => void = () => {};
    const loadBytes = vi.fn().mockReturnValue(new Promise<ArrayBuffer>((r) => (release = r)));
    const { controller, layers } = setup({ loadBytes });
    controller.request(framed(assetWith(['desktop'])));
    controller.withdraw();
    release(new ArrayBuffer(8));
    await flush();
    expect(layers).toHaveLength(0);
  });
});
