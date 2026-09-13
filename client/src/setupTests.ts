// Vitest DOM extensions for better assertions
import '@testing-library/jest-dom/vitest';
import { vi, beforeAll, afterAll } from 'vitest';
import * as React from 'react';

// Node 26 exposes an incomplete experimental `localStorage` unless
// --localstorage-file is supplied, and that object can leak into jsdom too.
// Give persistence middleware a deterministic standards-shaped test store.
const localStorageData = new Map<string, string>();
const testLocalStorage: Storage = {
  get length() { return localStorageData.size; },
  clear: () => localStorageData.clear(),
  getItem: (key) => localStorageData.get(key) ?? null,
  key: (index) => Array.from(localStorageData.keys())[index] ?? null,
  removeItem: (key) => { localStorageData.delete(key); },
  setItem: (key, value) => { localStorageData.set(key, String(value)); },
};
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: testLocalStorage,
});
Object.defineProperty(window, 'localStorage', {
  configurable: true,
  value: testLocalStorage,
});

// Polyfill React.act for React 19 compatibility with @testing-library/react
// React 19 moved act from react-dom/test-utils to the main react package
if (typeof (React as unknown as Record<string, unknown>).act === 'function') {
  // React.act is available, patch react-dom/test-utils
  (globalThis as unknown as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
}

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState = MockWebSocket.OPEN;
  url = '';

  constructor(url: string) {
    this.url = url;
  }

  send = vi.fn();
  close = vi.fn();
  addEventListener = vi.fn();
  removeEventListener = vi.fn();
}

global.WebSocket = MockWebSocket as unknown as typeof WebSocket;

// Mock ResizeObserver
global.ResizeObserver = vi.fn().mockImplementation(() => ({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
}));

// Mock requestAnimationFrame
global.requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => setTimeout(cb, 16) as unknown as number);
global.cancelAnimationFrame = vi.fn((id: number) => clearTimeout(id));

// Mock matchMedia
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

// Mock WebGL context for Three.js
HTMLCanvasElement.prototype.getContext = vi.fn((contextType: string) => {
  if (contextType === 'webgl' || contextType === 'webgl2') {
    return {
      createShader: vi.fn(),
      shaderSource: vi.fn(),
      compileShader: vi.fn(),
      getShaderParameter: vi.fn(() => true),
      createProgram: vi.fn(),
      attachShader: vi.fn(),
      linkProgram: vi.fn(),
      getProgramParameter: vi.fn(() => true),
      useProgram: vi.fn(),
      getAttribLocation: vi.fn(),
      getUniformLocation: vi.fn(),
      uniform1f: vi.fn(),
      uniform2f: vi.fn(),
      uniform3f: vi.fn(),
      uniform4f: vi.fn(),
      uniformMatrix4fv: vi.fn(),
      createBuffer: vi.fn(),
      bindBuffer: vi.fn(),
      bufferData: vi.fn(),
      enableVertexAttribArray: vi.fn(),
      vertexAttribPointer: vi.fn(),
      clear: vi.fn(),
      clearColor: vi.fn(),
      enable: vi.fn(),
      disable: vi.fn(),
      blendFunc: vi.fn(),
      drawArrays: vi.fn(),
      viewport: vi.fn(),
      getParameter: vi.fn((param: number) => {
        if (param === 7936) return 'WebGL Vendor'; // VENDOR
        if (param === 7937) return 'WebGL Renderer'; // RENDERER
        return null;
      }),
      getExtension: vi.fn(),
      createTexture: vi.fn(),
      bindTexture: vi.fn(),
      texImage2D: vi.fn(),
      texParameteri: vi.fn(),
      pixelStorei: vi.fn(),
      generateMipmap: vi.fn(),
    };
  }
  return null;
}) as unknown as typeof HTMLCanvasElement.prototype.getContext;

// Suppress console errors in tests (optional)
const originalError = console.error;
beforeAll(() => {
  console.error = (...args: unknown[]) => {
    if (
      typeof args[0] === 'string' &&
      args[0].includes('Warning: ReactDOM.render')
    ) {
      return;
    }
    originalError.call(console, ...args);
  };
});

afterAll(() => {
  console.error = originalError;
});
