// REC-4 / ADR-043 resurrection (PRD-023 WP-8): the control-centre four-KPI panel.
//
// An always-mounted glass pill shows the two live KPI headline figures; clicking
// it expands the four-tile grid. The two computed KPIs (Augmentation Ratio,
// Trust Variance) render their value + confidence; the two not-yet-computable
// KPIs (Mesh Velocity, HITL Precision) render "awaiting data source" with the
// source named — never a fabricated number (WP-8 falsification).

import React, { useState } from 'react';
import { Gauge } from 'lucide-react';
import { GlassPanel } from '../primitives/GlassPanel';
import { useKpiSummary } from './useKpiSummary';
import type { KpiTileView } from './kpiSummary';

const Tile: React.FC<{ tile: KpiTileView }> = ({ tile }) => (
  <div
    className="rounded bg-white/5 p-2"
    data-testid="kpi-tile"
    data-kpi={tile.kpi}
    data-computed={tile.computed}
  >
    <div className="text-[11px] font-medium truncate" title={tile.label}>
      {tile.label}
    </div>

    {tile.computed ? (
      <>
        <div className="text-lg font-semibold text-foreground" data-testid="kpi-value">
          {tile.valueText}
        </div>
        <div className="text-[10px] text-muted-foreground">
          {typeof tile.confidencePct === 'number' ? `confidence ${tile.confidencePct}%` : ''}
          {tile.detail ? ` · ${tile.detail}` : ''}
        </div>
      </>
    ) : (
      <div className="text-[10px] text-amber-400/80 italic" data-testid="kpi-awaiting">
        {tile.awaitingText}
      </div>
    )}

    <div className="text-[9px] text-muted-foreground line-clamp-2 mt-1" title={tile.source}>
      {tile.source}
    </div>
  </div>
);

export const KpiPanel: React.FC = () => {
  const { tiles, loading } = useKpiSummary();
  const [expanded, setExpanded] = useState(false);

  const headline = tiles.find((t) => t.computed && t.valueText);

  return (
    <div className="relative flex flex-col items-end gap-2" data-corner-item="kpi">
      {expanded && (
        <GlassPanel
          elevation="overlay"
          data-testid="kpi-panel"
          role="region"
          aria-label="Organisational KPI dashboard"
          className="w-80 max-h-[60vh] overflow-y-auto p-3 text-foreground"
        >
          <div className="flex items-center justify-between mb-2">
            <span className="text-sm font-semibold">Organisational KPIs</span>
            <button
              type="button"
              aria-label="Close KPI dashboard"
              onClick={() => setExpanded(false)}
              className="text-xs text-muted-foreground hover:text-foreground"
            >
              ✕
            </button>
          </div>

          {loading && tiles.length === 0 ? (
            <p className="text-xs text-muted-foreground">Computing KPIs…</p>
          ) : (
            <div className="grid grid-cols-2 gap-2">
              {tiles.map((tile) => (
                <Tile key={tile.kpi} tile={tile} />
              ))}
            </div>
          )}
        </GlassPanel>
      )}

      <button
        type="button"
        data-testid="kpi-indicator"
        aria-label="Organisational KPI dashboard"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        className="cc-glass flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Gauge size={13} aria-hidden="true" />
        KPIs
        {headline && (
          <span
            className="inline-flex items-center justify-center h-[18px] px-1.5 rounded-full text-[10px]"
            style={{ background: 'rgba(59,130,246,0.2)', color: '#3b82f6' }}
          >
            {headline.valueText}
          </span>
        )}
      </button>
    </div>
  );
};

KpiPanel.displayName = 'KpiPanel';

export default KpiPanel;
