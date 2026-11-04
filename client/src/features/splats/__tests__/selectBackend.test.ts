import { beforeEach, describe, expect, it, vi } from 'vitest';

// A mock factory runs only when its module is first imported, so a counter
// that stays at zero proves the module (and so its library) was never loaded.
const evaluated = vi.hoisted(() => ({ core: 0, spark: 0 }));

vi.mock('../coreAddonBackend', () => {
  evaluated.core += 1;
  return { CoreAddonSplatLayer: class { readonly kind = 'core-addon'; } };
});
vi.mock('../sparkBackend', () => {
  evaluated.spark += 1;
  return {
    SparkSplatLayer: class {
      readonly kind = 'spark';
      constructor(readonly gl: unknown) {}
    },
  };
});

async function fresh() {
  vi.resetModules();
  return import('../selectBackend');
}

beforeEach(() => {
  evaluated.core = 0;
  evaluated.spark = 0;
});

describe('pickBackendKind', () => {
  it('chooses the core addon for WebGPURenderer, by flag or by real backend', async () => {
    const { pickBackendKind } = await fresh();
    expect(pickBackendKind({ __isWebGPURenderer: true })).toBe('core-addon');
    expect(pickBackendKind({ backend: { isWebGPUBackend: true } })).toBe('core-addon');
  });

  it('chooses Spark for a plain WebGLRenderer', async () => {
    const { pickBackendKind } = await fresh();
    expect(pickBackendKind({})).toBe('spark');
    expect(pickBackendKind({ backend: undefined })).toBe('spark');
    expect(pickBackendKind(null)).toBe('spark');
  });

  it('treats a WebGPURenderer on its WebGL2 backend as WebGPU: Spark cannot run there', async () => {
    const { pickBackendKind } = await fresh();
    expect(pickBackendKind({ __isWebGPURenderer: true, backend: { isWebGLBackend: true } })).toBe('core-addon');
  });
});

describe('createSplatLayer', () => {
  it('on a WebGPU renderer imports the core addon backend and never the Spark one', async () => {
    const { createSplatLayer } = await fresh();
    const layer = await createSplatLayer({ __isWebGPURenderer: true });
    expect(layer.kind).toBe('core-addon');
    expect(evaluated.core).toBe(1);
    expect(evaluated.spark).toBe(0);
  });

  it('on a WebGL renderer imports the Spark backend and never the core addon one', async () => {
    const { createSplatLayer } = await fresh();
    const gl = { isWebGLRenderer: true };
    const layer = await createSplatLayer(gl);
    expect(layer.kind).toBe('spark');
    expect((layer as unknown as { gl: unknown }).gl).toBe(gl);
    expect(evaluated.spark).toBe(1);
    expect(evaluated.core).toBe(0);
  });

  it('decides from the live renderer at call time, not from module-load state', async () => {
    const { createSplatLayer } = await fresh();
    const gl: { __isWebGPURenderer?: boolean } = {};
    expect((await createSplatLayer(gl)).kind).toBe('spark');
    gl.__isWebGPURenderer = true; // the renderer finishes initialising
    expect((await createSplatLayer(gl)).kind).toBe('core-addon');
  });

  it('maps each backend to its variant path', async () => {
    const { renderPathFor } = await fresh();
    expect(renderPathFor('core-addon')).toBe('webgpu');
    expect(renderPathFor('spark')).toBe('webgl');
  });
});
