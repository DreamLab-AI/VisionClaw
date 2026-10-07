/** Shared inline styles for the memory explorer's HTML overlay. */
import type React from 'react';
import { ROUTE_PALETTE } from './routeMath';

export const css = {
  panel: {
    position: 'absolute',
    top: 14,
    right: 14,
    width: 368,
    maxWidth: 'calc(100vw - 28px)',
    maxHeight: 'calc(100% - 84px)',
    overflowY: 'auto',
    zIndex: 20,
    padding: 12,
    color: '#e9edf5',
    fontSize: 12,
    fontFamily: 'Inter, system-ui, sans-serif',
    background: 'rgba(5, 6, 10, 0.78)',
  } as React.CSSProperties,
  row: { display: 'flex', gap: 6, alignItems: 'center' } as React.CSSProperties,
  label: { color: '#8b93a7', fontSize: 11 } as React.CSSProperties,
  mono: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: 11 } as React.CSSProperties,
  input: {
    flex: 1,
    minWidth: 0,
    background: 'rgba(255,255,255,0.06)',
    border: '1px solid rgba(255,255,255,0.12)',
    borderRadius: 6,
    color: '#e9edf5',
    padding: '6px 8px',
    fontSize: 12,
  } as React.CSSProperties,
  button: (on = false): React.CSSProperties => ({
    background: on ? 'rgba(126, 240, 207, 0.16)' : 'rgba(255,255,255,0.06)',
    border: `1px solid ${on ? ROUTE_PALETTE.mint : 'rgba(255,255,255,0.14)'}`,
    color: on ? ROUTE_PALETTE.mint : '#e9edf5',
    borderRadius: 6,
    padding: '5px 9px',
    fontSize: 12,
    cursor: 'pointer',
  }),
  section: { borderTop: '1px solid rgba(255,255,255,0.08)', paddingTop: 10, marginTop: 10 } as React.CSSProperties,
  stat: { display: 'flex', flexDirection: 'column', minWidth: 0 } as React.CSSProperties,
};

export const pct = (v: number) => `${(v * 100).toFixed(0)}%`;
