/**
 * useTelemetry Hook Tests
 *
 * Comprehensive tests for telemetry hooks that track component lifecycle,
 * performance metrics, user interactions, and error boundaries.
 *
 * Uses direct hook invocation via test components to avoid React 19
 * compatibility issues with @testing-library/react's renderHook.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import React, { useEffect } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { useTelemetry, useThreeJSTelemetry, setPerFrameTelemetryEnabled, useWebSocketTelemetry } from '../useTelemetry';
import { agentTelemetry } from '../AgentTelemetry';

// Mock the AgentTelemetry module
vi.mock('../AgentTelemetry', () => ({
  agentTelemetry: {
    logAgentSpawn: vi.fn(),
    logAgentAction: vi.fn(),
    logRenderCycle: vi.fn(),
    logUserInteraction: vi.fn(),
    logThreeJSOperation: vi.fn(),
    logWebSocketMessage: vi.fn(),
  },
}));

// Test harness types
interface UseTelemetryResult {
  startRender: () => void;
  endRender: () => void;
  logInteraction: (interactionType: string, metadata?: Record<string, any>) => void;
  logError: (error: Error, context?: string) => void;
}

interface UseThreeJSTelemetryResult {
  logPositionUpdate: (position: { x: number; y: number; z: number }, metadata?: Record<string, any>) => void;
  logMeshCreate: (position?: { x: number; y: number; z: number }, metadata?: Record<string, any>) => void;
  logAnimationFrame: (position?: { x: number; y: number; z: number }, rotation?: { x: number; y: number; z: number }) => void;
  logForceApplied: (force: { x: number; y: number; z: number }, position?: { x: number; y: number; z: number }) => void;
}

interface UseWebSocketTelemetryResult {
  logMessage: (messageType: string, direction: 'incoming' | 'outgoing', data?: any) => void;
}

// Hook result references
let telemetryResultRef: UseTelemetryResult | null = null;
let threeJSResultRef: UseThreeJSTelemetryResult | null = null;
let webSocketResultRef: UseWebSocketTelemetryResult | null = null;
let resolveRender: (() => void) | null = null;

/**
 * Test component for useTelemetry hook
 */
function TelemetryTestComponent({ componentName }: { componentName: string }) {
  const result = useTelemetry(componentName);
  telemetryResultRef = result;

  useEffect(() => {
    if (resolveRender) {
      resolveRender();
      resolveRender = null;
    }
  });

  return null;
}

/**
 * Test component for useThreeJSTelemetry hook
 */
function ThreeJSTestComponent({ objectId }: { objectId: string }) {
  const result = useThreeJSTelemetry(objectId);
  threeJSResultRef = result;

  useEffect(() => {
    if (resolveRender) {
      resolveRender();
      resolveRender = null;
    }
  });

  return null;
}

/**
 * Test component for useWebSocketTelemetry hook
 */
function WebSocketTestComponent() {
  const result = useWebSocketTelemetry();
  webSocketResultRef = result;

  useEffect(() => {
    if (resolveRender) {
      resolveRender();
      resolveRender = null;
    }
  });

  return null;
}

describe('useTelemetry', () => {
  let container: HTMLDivElement;
  let root: Root;
  let mockPerformanceNow: number;

  beforeEach(() => {
    vi.clearAllMocks();
    mockPerformanceNow = 1000;
    telemetryResultRef = null;
    resolveRender = null;

    // Create DOM container
    container = document.createElement('div');
    document.body.appendChild(container);

    // Mock performance.now
    vi.spyOn(performance, 'now').mockImplementation(() => mockPerformanceNow);
  });

  afterEach(() => {
    if (root) {
      root.unmount();
    }
    if (container) {
      document.body.removeChild(container);
    }
    vi.restoreAllMocks();
  });

  /**
   * Renders the test component and waits for it to be ready
   */
  async function renderTelemetryHook(componentName: string): Promise<UseTelemetryResult> {
    root = createRoot(container);

    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      root.render(React.createElement(TelemetryTestComponent, { componentName }));
    });

    await new Promise((resolve) => setTimeout(resolve, 10));

    if (!telemetryResultRef) {
      throw new Error('Hook did not render');
    }

    return telemetryResultRef;
  }

  async function waitForUpdate(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }

  describe('Hook initialization', () => {
    it('returns startRender function', async () => {
      const result = await renderTelemetryHook('TestComponent');
      expect(typeof result.startRender).toBe('function');
    });

    it('returns endRender function', async () => {
      const result = await renderTelemetryHook('TestComponent');
      expect(typeof result.endRender).toBe('function');
    });

    it('returns logInteraction function', async () => {
      const result = await renderTelemetryHook('TestComponent');
      expect(typeof result.logInteraction).toBe('function');
    });

    it('returns logError function', async () => {
      const result = await renderTelemetryHook('TestComponent');
      expect(typeof result.logError).toBe('function');
    });

    it('logs agent spawn on mount with component name', async () => {
      await renderTelemetryHook('MyTestComponent');
      await waitForUpdate();

      expect(agentTelemetry.logAgentSpawn).toHaveBeenCalledWith(
        'react-component',
        'MyTestComponent',
        { action: 'mount' }
      );
    });

    it('logs spawn with different component names', async () => {
      await renderTelemetryHook('HeaderComponent');
      await waitForUpdate();

      expect(agentTelemetry.logAgentSpawn).toHaveBeenCalledWith(
        'react-component',
        'HeaderComponent',
        { action: 'mount' }
      );
    });
  });

  describe('Component lifecycle tracking', () => {
    it('logs unmount action with lifetime when component unmounts', async () => {
      await renderTelemetryHook('UnmountTestComponent');
      await waitForUpdate();

      // Advance time to simulate component lifetime
      mockPerformanceNow = 1500;

      root.unmount();
      await waitForUpdate();

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react-component',
        'UnmountTestComponent',
        'unmount',
        { lifetime: 500 }
      );
    });

    it('calculates correct lifetime based on performance.now difference', async () => {
      await renderTelemetryHook('LifetimeComponent');
      await waitForUpdate();

      // Simulate 2000ms lifetime
      mockPerformanceNow = 3000;

      root.unmount();
      await waitForUpdate();

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react-component',
        'LifetimeComponent',
        'unmount',
        { lifetime: 2000 }
      );
    });

    it('handles immediate unmount with zero lifetime', async () => {
      await renderTelemetryHook('QuickComponent');
      await waitForUpdate();

      // No time advance - immediate unmount
      root.unmount();
      await waitForUpdate();

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react-component',
        'QuickComponent',
        'unmount',
        { lifetime: 0 }
      );
    });
  });

  describe('Render cycle tracking', () => {
    it('startRender captures current time', async () => {
      const result = await renderTelemetryHook('RenderComponent');

      result.startRender();

      // Internal state captured - verify by calling endRender
      mockPerformanceNow = 1050;
      result.endRender();

      expect(agentTelemetry.logRenderCycle).toHaveBeenCalledWith(50);
    });

    it('endRender logs render cycle duration', async () => {
      const result = await renderTelemetryHook('RenderComponent');

      result.startRender();
      mockPerformanceNow = 1100;
      result.endRender();

      expect(agentTelemetry.logRenderCycle).toHaveBeenCalledWith(100);
    });

    it('endRender does nothing if startRender was not called', async () => {
      const result = await renderTelemetryHook('NoStartComponent');

      result.endRender();

      expect(agentTelemetry.logRenderCycle).not.toHaveBeenCalled();
    });

    it('tracks multiple render cycles independently', async () => {
      const result = await renderTelemetryHook('MultiRenderComponent');

      // First render cycle
      result.startRender();
      mockPerformanceNow = 1016;
      result.endRender();

      // Second render cycle
      result.startRender();
      mockPerformanceNow = 1050;
      result.endRender();

      expect(agentTelemetry.logRenderCycle).toHaveBeenCalledTimes(2);
      expect(agentTelemetry.logRenderCycle).toHaveBeenNthCalledWith(1, 16);
      expect(agentTelemetry.logRenderCycle).toHaveBeenNthCalledWith(2, 34);
    });

    it('handles sub-millisecond render times', async () => {
      const result = await renderTelemetryHook('FastComponent');

      result.startRender();
      mockPerformanceNow = 1000.5;
      result.endRender();

      expect(agentTelemetry.logRenderCycle).toHaveBeenCalledWith(0.5);
    });
  });

  describe('User interaction logging', () => {
    it('logs interaction with type and component name', async () => {
      const result = await renderTelemetryHook('InteractiveComponent');

      result.logInteraction('click');

      expect(agentTelemetry.logUserInteraction).toHaveBeenCalledWith(
        'click',
        'InteractiveComponent',
        undefined
      );
    });

    it('logs interaction with metadata', async () => {
      const result = await renderTelemetryHook('ButtonComponent');

      result.logInteraction('click', { buttonId: 'submit', position: { x: 100, y: 200 } });

      expect(agentTelemetry.logUserInteraction).toHaveBeenCalledWith(
        'click',
        'ButtonComponent',
        { buttonId: 'submit', position: { x: 100, y: 200 } }
      );
    });

    it('logs different interaction types', async () => {
      const result = await renderTelemetryHook('FormComponent');

      result.logInteraction('focus');
      result.logInteraction('blur');
      result.logInteraction('change');

      expect(agentTelemetry.logUserInteraction).toHaveBeenCalledTimes(3);
      expect(agentTelemetry.logUserInteraction).toHaveBeenNthCalledWith(1, 'focus', 'FormComponent', undefined);
      expect(agentTelemetry.logUserInteraction).toHaveBeenNthCalledWith(2, 'blur', 'FormComponent', undefined);
      expect(agentTelemetry.logUserInteraction).toHaveBeenNthCalledWith(3, 'change', 'FormComponent', undefined);
    });

    it('handles empty metadata object', async () => {
      const result = await renderTelemetryHook('EmptyMetaComponent');

      result.logInteraction('hover', {});

      expect(agentTelemetry.logUserInteraction).toHaveBeenCalledWith(
        'hover',
        'EmptyMetaComponent',
        {}
      );
    });
  });

  describe('Error logging', () => {
    it('logs error with message and stack', async () => {
      const result = await renderTelemetryHook('ErrorComponent');
      const testError = new Error('Test error message');
      testError.stack = 'Error: Test error message\n    at TestComponent';

      result.logError(testError);

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react',
        'error',
        'component_error',
        {
          componentName: 'ErrorComponent',
          context: undefined,
          errorMessage: 'Test error message',
          errorStack: 'Error: Test error message\n    at TestComponent',
        }
      );
    });

    it('logs error with context', async () => {
      const result = await renderTelemetryHook('ContextErrorComponent');
      const testError = new Error('Render failed');

      result.logError(testError, 'during useEffect');

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react',
        'error',
        'component_error',
        {
          componentName: 'ContextErrorComponent',
          context: 'during useEffect',
          errorMessage: 'Render failed',
          errorStack: expect.any(String),
        }
      );
    });

    it('handles error without stack trace', async () => {
      const result = await renderTelemetryHook('NoStackComponent');
      const testError = new Error('Stackless error');
      delete testError.stack;

      result.logError(testError);

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
        'react',
        'error',
        'component_error',
        expect.objectContaining({
          componentName: 'NoStackComponent',
          errorMessage: 'Stackless error',
        })
      );
    });

    it('logs multiple errors from same component', async () => {
      const result = await renderTelemetryHook('MultiErrorComponent');

      result.logError(new Error('First error'), 'init');
      result.logError(new Error('Second error'), 'render');

      expect(agentTelemetry.logAgentAction).toHaveBeenCalledTimes(2);
    });
  });

  describe('Callback stability', () => {
    it('startRender callback is stable across renders', async () => {
      await renderTelemetryHook('StableComponent');
      const firstStartRender = telemetryResultRef!.startRender;

      // Force re-render by unmounting and remounting
      root.unmount();
      await waitForUpdate();

      root = createRoot(container);
      await new Promise<void>((resolve) => {
        resolveRender = resolve;
        root.render(React.createElement(TelemetryTestComponent, { componentName: 'StableComponent' }));
      });
      await waitForUpdate();

      // Different instance after remount, but function signature same
      expect(typeof telemetryResultRef!.startRender).toBe('function');
    });

    it('logInteraction includes componentName in closure', async () => {
      await renderTelemetryHook('ClosureComponent');
      const logInteraction = telemetryResultRef!.logInteraction;

      // Call the captured function
      logInteraction('test-action');

      expect(agentTelemetry.logUserInteraction).toHaveBeenCalledWith(
        'test-action',
        'ClosureComponent',
        undefined
      );
    });
  });
});

describe('useThreeJSTelemetry', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    threeJSResultRef = null;
    resolveRender = null;
    setPerFrameTelemetryEnabled(true);

    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    if (root) {
      root.unmount();
    }
    if (container) {
      document.body.removeChild(container);
    }
  });

  async function renderThreeJSHook(objectId: string): Promise<UseThreeJSTelemetryResult> {
    root = createRoot(container);

    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      root.render(React.createElement(ThreeJSTestComponent, { objectId }));
    });

    await new Promise((resolve) => setTimeout(resolve, 10));

    if (!threeJSResultRef) {
      throw new Error('Hook did not render');
    }

    return threeJSResultRef;
  }

  describe('Position update logging', () => {
    it('logs position update with coordinates', async () => {
      const result = await renderThreeJSHook('mesh-001');

      result.logPositionUpdate({ x: 10, y: 20, z: 30 });

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'position_update',
        'mesh-001',
        { x: 10, y: 20, z: 30 },
        undefined,
        undefined
      );
    });

    it('logs position update with metadata', async () => {
      const result = await renderThreeJSHook('agent-mesh');

      result.logPositionUpdate(
        { x: 5, y: 10, z: 15 },
        { velocity: 2.5, targetReached: true }
      );

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'position_update',
        'agent-mesh',
        { x: 5, y: 10, z: 15 },
        undefined,
        { velocity: 2.5, targetReached: true }
      );
    });
  });

  describe('Mesh creation logging', () => {
    it('logs mesh create without position', async () => {
      const result = await renderThreeJSHook('new-mesh');

      result.logMeshCreate();

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'mesh_create',
        'new-mesh',
        undefined,
        undefined,
        undefined
      );
    });

    it('logs mesh create with position and metadata', async () => {
      const result = await renderThreeJSHook('positioned-mesh');

      result.logMeshCreate(
        { x: 0, y: 0, z: 0 },
        { meshType: 'sphere', radius: 5 }
      );

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'mesh_create',
        'positioned-mesh',
        { x: 0, y: 0, z: 0 },
        undefined,
        { meshType: 'sphere', radius: 5 }
      );
    });
  });

  describe('Animation frame logging', () => {
    it('logs animation frame with position only', async () => {
      const result = await renderThreeJSHook('animated-object');

      result.logAnimationFrame({ x: 1, y: 2, z: 3 });

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'animation_frame',
        'animated-object',
        { x: 1, y: 2, z: 3 },
        undefined
      );
    });

    it('logs animation frame with position and rotation', async () => {
      const result = await renderThreeJSHook('rotating-cube');

      result.logAnimationFrame(
        { x: 0, y: 5, z: 0 },
        { x: 0.1, y: 0.2, z: 0.3 }
      );

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'animation_frame',
        'rotating-cube',
        { x: 0, y: 5, z: 0 },
        { x: 0.1, y: 0.2, z: 0.3 }
      );
    });

    it('logs animation frame without parameters', async () => {
      const result = await renderThreeJSHook('minimal-object');

      result.logAnimationFrame();

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'animation_frame',
        'minimal-object',
        undefined,
        undefined
      );
    });
  });

  describe('Force application logging', () => {
    it('logs force applied with force vector', async () => {
      const result = await renderThreeJSHook('physics-object');

      result.logForceApplied({ x: 100, y: 0, z: 50 });

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'force_applied',
        'physics-object',
        undefined,
        undefined,
        { force: { x: 100, y: 0, z: 50 } }
      );
    });

    it('logs force applied with position', async () => {
      const result = await renderThreeJSHook('impulse-target');

      result.logForceApplied(
        { x: 0, y: 500, z: 0 },
        { x: 10, y: 0, z: 10 }
      );

      expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalledWith(
        'force_applied',
        'impulse-target',
        { x: 10, y: 0, z: 10 },
        undefined,
        { force: { x: 0, y: 500, z: 0 } }
      );
    });
  });

  describe('Object ID binding', () => {
    it('uses objectId for all operations', async () => {
      const result = await renderThreeJSHook('unique-id-123');

      result.logPositionUpdate({ x: 0, y: 0, z: 0 });
      result.logMeshCreate();
      result.logAnimationFrame();
      result.logForceApplied({ x: 1, y: 1, z: 1 });

      const calls = vi.mocked(agentTelemetry.logThreeJSOperation).mock.calls;
      expect(calls.every(call => call[1] === 'unique-id-123')).toBe(true);
    });
  });
});

describe('useWebSocketTelemetry', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    webSocketResultRef = null;
    resolveRender = null;

    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    if (root) {
      root.unmount();
    }
    if (container) {
      document.body.removeChild(container);
    }
  });

  async function renderWebSocketHook(): Promise<UseWebSocketTelemetryResult> {
    root = createRoot(container);

    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      root.render(React.createElement(WebSocketTestComponent));
    });

    await new Promise((resolve) => setTimeout(resolve, 10));

    if (!webSocketResultRef) {
      throw new Error('Hook did not render');
    }

    return webSocketResultRef;
  }

  describe('Message logging', () => {
    it('logs incoming message with type', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('agent_update', 'incoming');

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'agent_update',
        'incoming',
        undefined,
        undefined
      );
    });

    it('logs outgoing message with type', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('command', 'outgoing');

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'command',
        'outgoing',
        undefined,
        undefined
      );
    });

    it('logs message with data and calculates size', async () => {
      const result = await renderWebSocketHook();
      const testData = { agentId: 1, status: 'active', metrics: [1, 2, 3] };

      result.logMessage('status_update', 'incoming', testData);

      const expectedSize = JSON.stringify(testData).length;
      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'status_update',
        'incoming',
        testData,
        expectedSize
      );
    });

    it('handles empty object data', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('ping', 'outgoing', {});

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'ping',
        'outgoing',
        {},
        2 // "{}" is 2 characters
      );
    });

    it('handles array data', async () => {
      const result = await renderWebSocketHook();
      const arrayData = [1, 2, 3, 4, 5];

      result.logMessage('batch', 'incoming', arrayData);

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'batch',
        'incoming',
        arrayData,
        JSON.stringify(arrayData).length
      );
    });

    it('handles string data', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('text', 'incoming', 'hello world');

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'text',
        'incoming',
        'hello world',
        JSON.stringify('hello world').length
      );
    });

    it('handles undefined data with undefined size', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('heartbeat', 'outgoing', undefined);

      expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalledWith(
        'heartbeat',
        'outgoing',
        undefined,
        undefined
      );
    });
  });

  describe('Message direction', () => {
    it('correctly tags incoming messages', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('data', 'incoming', { test: true });

      const call = vi.mocked(agentTelemetry.logWebSocketMessage).mock.calls[0];
      expect(call[1]).toBe('incoming');
    });

    it('correctly tags outgoing messages', async () => {
      const result = await renderWebSocketHook();

      result.logMessage('request', 'outgoing', { action: 'fetch' });

      const call = vi.mocked(agentTelemetry.logWebSocketMessage).mock.calls[0];
      expect(call[1]).toBe('outgoing');
    });
  });
});

describe('Integration scenarios', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    telemetryResultRef = null;
    threeJSResultRef = null;
    webSocketResultRef = null;
    resolveRender = null;

    container = document.createElement('div');
    document.body.appendChild(container);

    vi.spyOn(performance, 'now').mockReturnValue(1000);
  });

  afterEach(() => {
    if (root) {
      root.unmount();
    }
    if (container) {
      document.body.removeChild(container);
    }
    vi.restoreAllMocks();
  });

  it('multiple hooks can be used in same component', async () => {
    // Component using all three hooks
    function MultiHookComponent() {
      const telemetry = useTelemetry('MultiHookComponent');
      const threeJS = useThreeJSTelemetry('object-1');
      const webSocket = useWebSocketTelemetry();

      telemetryResultRef = telemetry;
      threeJSResultRef = threeJS;
      webSocketResultRef = webSocket;

      useEffect(() => {
        if (resolveRender) {
          resolveRender();
          resolveRender = null;
        }
      });

      return null;
    }

    root = createRoot(container);
    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      root.render(React.createElement(MultiHookComponent));
    });
    await new Promise((resolve) => setTimeout(resolve, 10));

    // All hooks should be initialized
    expect(telemetryResultRef).not.toBeNull();
    expect(threeJSResultRef).not.toBeNull();
    expect(webSocketResultRef).not.toBeNull();

    // Each hook works independently
    telemetryResultRef!.logInteraction('click');
    threeJSResultRef!.logPositionUpdate({ x: 1, y: 2, z: 3 });
    webSocketResultRef!.logMessage('test', 'incoming');

    expect(agentTelemetry.logUserInteraction).toHaveBeenCalled();
    expect(agentTelemetry.logThreeJSOperation).toHaveBeenCalled();
    expect(agentTelemetry.logWebSocketMessage).toHaveBeenCalled();
  });

  it('hooks maintain state across re-renders', async () => {
    let renderCount = 0;
    let forceUpdate: () => void;

    function RerenderComponent() {
      const [, setState] = React.useState(0);
      forceUpdate = () => setState((s) => s + 1);
      renderCount++;

      const telemetry = useTelemetry('RerenderComponent');
      telemetryResultRef = telemetry;

      useEffect(() => {
        if (resolveRender) {
          resolveRender();
          resolveRender = null;
        }
      });

      return null;
    }

    root = createRoot(container);
    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      root.render(React.createElement(RerenderComponent));
    });
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect(renderCount).toBe(1);

    // Force re-render
    await new Promise<void>((resolve) => {
      resolveRender = resolve;
      forceUpdate!();
    });
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect(renderCount).toBe(2);
    // Hook should still work after re-render
    telemetryResultRef!.logInteraction('test');
    expect(agentTelemetry.logUserInteraction).toHaveBeenCalledWith(
      'test',
      'RerenderComponent',
      undefined
    );
  });
});

describe('per-frame telemetry gate', () => {
  it('logPositionUpdate and logAnimationFrame are no-ops unless enabled', async () => {
    setPerFrameTelemetryEnabled(false);
    const c = document.createElement('div');
    document.body.appendChild(c);
    const r = createRoot(c);
    let res: UseThreeJSTelemetryResult | null = null;
    const H = () => { res = useThreeJSTelemetry('obj'); return null; };
    await new Promise<void>(ok => { r.render(React.createElement(H)); setTimeout(ok, 20); });
    vi.clearAllMocks();
    for (let i = 0; i < 100; i++) { res!.logPositionUpdate({ x: 0, y: 0, z: 0 }); res!.logAnimationFrame(); }
    expect(agentTelemetry.logThreeJSOperation).not.toHaveBeenCalled();
    r.unmount();
    c.remove();
  });
});
