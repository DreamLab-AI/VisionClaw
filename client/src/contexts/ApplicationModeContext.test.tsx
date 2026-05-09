import { describe, it, expect, beforeEach, vi } from 'vitest';
import React from 'react';
import { renderHook, act } from '@testing-library/react';

vi.mock('../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

import { ApplicationModeProvider, useApplicationMode } from './ApplicationModeContext';

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <ApplicationModeProvider>{children}</ApplicationModeProvider>
);

describe('ApplicationModeContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset window size to desktop
    Object.defineProperty(window, 'innerWidth', { value: 1024, writable: true });
  });

  it('provides default desktop mode', () => {
    const { result } = renderHook(() => useApplicationMode(), { wrapper });

    expect(result.current.mode).toBe('desktop');
    expect(result.current.isXRMode).toBe(false);
    expect(result.current.previousMode).toBeNull();
  });

  it('returns layout settings for desktop mode', () => {
    const { result } = renderHook(() => useApplicationMode(), { wrapper });

    expect(result.current.layoutSettings).toEqual({
      showPanels: true,
      showViewport: true,
      showControls: true,
    });
  });

  it('switches to XR mode and updates layout', () => {
    const { result } = renderHook(() => useApplicationMode(), { wrapper });

    act(() => {
      result.current.setMode('xr');
    });

    expect(result.current.mode).toBe('xr');
    expect(result.current.isXRMode).toBe(true);
    expect(result.current.previousMode).toBe('desktop');
    expect(result.current.layoutSettings.showPanels).toBe(false);
    expect(result.current.layoutSettings.showControls).toBe(false);
  });

  it('tracks previous mode on transitions', () => {
    const { result } = renderHook(() => useApplicationMode(), { wrapper });

    act(() => result.current.setMode('xr'));
    expect(result.current.previousMode).toBe('desktop');
    expect(result.current.mode).toBe('xr');
  });

  it('responds to window resize triggering mobile mode', () => {
    const { result } = renderHook(() => useApplicationMode(), { wrapper });

    act(() => {
      Object.defineProperty(window, 'innerWidth', { value: 500, writable: true });
      window.dispatchEvent(new Event('resize'));
    });

    expect(result.current.isMobileView).toBe(true);
  });

  it('returns default values when used outside provider (no throw due to default context)', () => {
    // createContext provides a defaultContext value, so useContext returns it
    // even without a Provider. The hook's guard only throws if context is
    // falsy, but the default is a full object.
    const { result } = renderHook(() => useApplicationMode());
    expect(result.current.mode).toBe('desktop');
  });
});
