import { beforeEach, describe, expect, it, vi } from 'vitest';

// A mock factory runs only when its module is first imported, so a counter
// that stays at zero proves the module (and so its library) was never loaded.
const evaluated = vi.hoisted(() => ({ core: 0 }));

vi.mock('../coreAddonBackend', () => {
  evaluated.core += 1;
  return {
    CoreAddonSplatLayer: class {
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
});

describe('renderPathFor (ADR-2170 decision 5)', () => {
  it('gives the WebGPU backend the webgpu tier', async () => {
    const { renderPathFor } = await fresh();
    expect(renderPathFor({ isWebGPURenderer: true, backend: { isWebGPUBackend: true } })).toBe('webgpu');
  });

  it('gives WebGPURenderer on its WebGL2 backend the webgl tier (no GPU sort there)', async () => {
    const { renderPathFor } = await fresh();
    expect(renderPathFor({ isWebGPURenderer: true, backend: { isWebGLBackend: true } })).toBe('webgl');
    expect(renderPathFor({})).toBe('webgl');
    expect(renderPathFor(null)).toBe('webgl');
  });
});

describe('createSplatLayer', () => {
  it('imports the core addon only when a layer is built', async () => {
    const { createSplatLayer } = await fresh();
    expect(evaluated.core).toBe(0);
    const gl = { isWebGPURenderer: true };
    const layer = await createSplatLayer(gl);
    expect((layer as unknown as { gl: unknown }).gl).toBe(gl);
    expect(evaluated.core).toBe(1);
  });
});
