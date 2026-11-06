/**
 * In-canvas notice for truncated overview edges (ADR-2181 D4: "N of M edges
 * drawn"; the console is not enough). A plain DOM element, to be mounted by
 * the menus lane inside the canvas container (outside the R3F <Canvas>). It
 * renders nothing while every edge is drawn.
 */
import React from 'react';
import { useOverviewEdgeReport, overviewEdgeNoticeText } from './overviewEdgeStore';

export interface OverviewEdgeNoticeProps {
  className?: string;
  style?: React.CSSProperties;
}

export const OverviewEdgeNotice: React.FC<OverviewEdgeNoticeProps> = ({ className, style }) => {
  const report = useOverviewEdgeReport();
  const text = overviewEdgeNoticeText(report);
  if (!text) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="overview-edge-notice"
      className={className}
      title={`Edge budget ${report.capacity.toLocaleString('en-GB')}: raise visualisation.graphView.overviewEdges.capacity (or set it to 'auto') to draw every edge.`}
      style={className ? style : {
        position: 'absolute', left: 12, bottom: 12, padding: '4px 8px', borderRadius: 4,
        background: 'rgba(20, 24, 32, 0.8)', color: '#e6e9ef', font: '12px system-ui, sans-serif',
        pointerEvents: 'auto', ...style,
      }}
    >
      {text}
    </div>
  );
};

export default OverviewEdgeNotice;
