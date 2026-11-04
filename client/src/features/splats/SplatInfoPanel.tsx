import type { CSSProperties } from 'react';
import { creditsLine } from './credits';
import { useSplatStore } from './splatStore';

const panelStyle: CSSProperties = {
  position: 'absolute',
  left: '16px',
  bottom: '16px',
  maxWidth: '380px',
  padding: '10px 12px',
  background: 'rgba(10, 14, 30, 0.82)',
  border: '1px solid rgba(255,255,255,0.14)',
  borderRadius: '8px',
  color: 'rgba(255,255,255,0.88)',
  font: '12px/1.45 system-ui, sans-serif',
  zIndex: 20,
};

const buttonStyle: CSSProperties = {
  padding: '3px 10px',
  background: 'rgba(255,255,255,0.08)',
  border: '1px solid rgba(255,255,255,0.2)',
  borderRadius: '4px',
  color: 'inherit',
  cursor: 'pointer',
};

/** Only http(s) links are rendered as anchors; anything else is shown as text. */
function safeHref(url: string): string | null {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.href : null;
  } catch {
    return null;
  }
}

/**
 * Scene info panel for the open splat scene (ADR-2170 decision 10). It shows
 * the attribution, the licence link and "modified (decimated)" on every view,
 * framed or entered, and carries the enter, leave and close controls.
 */
export default function SplatInfoPanel() {
  const { assets, activeId, mode, status, variant, statusMessage, enter, leave, close } = useSplatStore();
  const asset = assets.find((a) => a.id === activeId);
  if (!asset) return null;
  const licenceHref = safeHref(asset.licence.url);
  const sourceHref = safeHref(asset.sourceUrl);

  return (
    <div style={panelStyle} role="group" aria-label="Splat scene information" data-testid="splat-info-panel">
      <div style={{ fontWeight: 600 }}>{asset.title}</div>
      <div style={{ color: 'rgba(255,255,255,0.6)' }}>
        {asset.author}
        {sourceHref && (
          <>
            {' · '}
            <a href={sourceHref} target="_blank" rel="noopener noreferrer" style={{ color: '#8ab4ff' }}>
              source
            </a>
          </>
        )}
      </div>
      <div style={{ marginTop: '6px' }} data-testid="splat-credits">
        {asset.attribution}{' '}
        {licenceHref ? (
          <a href={licenceHref} target="_blank" rel="noopener noreferrer" style={{ color: '#8ab4ff' }}>
            {asset.licence.spdx}
          </a>
        ) : (
          <span>{asset.licence.spdx}</span>
        )}
        {asset.modified && <span>, modified (decimated)</span>}
      </div>
      <div style={{ marginTop: '6px', color: 'rgba(255,255,255,0.6)' }} aria-live="polite" title={creditsLine(asset)}>
        {status === 'loading' && 'Loading…'}
        {status === 'ready' && variant && `${variant.name}: ${variant.splats.toLocaleString('en-GB')} splats, ${mode}`}
        {status === 'no-variant' && 'No desktop variant available: nothing to show.'}
        {status === 'refused' && `Refused (integrity check failed). ${statusMessage ?? ''}`}
        {status === 'error' && `Could not load. ${statusMessage ?? ''}`}
      </div>
      <div style={{ marginTop: '8px', display: 'flex', gap: '8px' }}>
        {mode === 'framed' ? (
          <button type="button" style={buttonStyle} onClick={enter}>Enter</button>
        ) : (
          <button type="button" style={buttonStyle} onClick={leave}>Leave</button>
        )}
        <button type="button" style={buttonStyle} onClick={close}>Close</button>
      </div>
    </div>
  );
}
