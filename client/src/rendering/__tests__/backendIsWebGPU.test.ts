import { describe, expect, it, vi } from 'vitest';

vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() }),
}));

import { backendIsWebGPU } from '../rendererFactory';

// Stand-ins shaped like three 0.187's backends, under minified class names:
// terser renames WebGLBackend to something like `_A`, which is what broke the
// old constructor.name gate.
class _A {
  isWebGLBackend = true;
}
class _B {
  isWebGPUBackend = true;
}

describe('backendIsWebGPU', () => {
  it('rejects the WebGL2 fallback backend even when its class is minified', () => {
    expect(_A.name).not.toBe('WebGLBackend');
    expect(backendIsWebGPU(new _A())).toBe(false);
  });

  it('accepts the real WebGPU backend under a minified name', () => {
    expect(backendIsWebGPU(new _B())).toBe(true);
  });

  it('rejects a missing or unrecognised backend', () => {
    expect(backendIsWebGPU(undefined)).toBe(false);
    expect(backendIsWebGPU(null)).toBe(false);
    expect(backendIsWebGPU({})).toBe(false);
  });

  it('rejects a backend claiming both flags', () => {
    expect(backendIsWebGPU({ isWebGPUBackend: true, isWebGLBackend: true })).toBe(false);
  });
});
