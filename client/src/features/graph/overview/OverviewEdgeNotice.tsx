/**
 * In-canvas notice for truncated overview edges and a short worker position
 * buffer (ADR-2181 D4: "N of M edges drawn", "N of M nodes drawn"; the
 * console is not enough). A plain DOM element, to be mounted by
 * the menus lane inside the canvas container (outside the R3F <Canvas>). It
 * renders nothing while every edge and every node is drawn and every overview
 * layer is healthy (a render fault is reported here too: overviewLive).
 */
import React from 'react';
import { useOverviewEdgeReport, overviewEdgeNoticeText, useNodeBufferReport, nodeBufferNoticeText } from './overviewEdgeStore';
import { useOverviewLive, overviewFaultNoticeText } from './overviewLive';

export interface OverviewEdgeNoticeProps {
  className?: string;
  style?: React.CSSProperties;
}

export const OverviewEdgeNotice: React.FC<OverviewEdgeNoticeProps> = ({ className, style }) => {
  const report = useOverviewEdgeReport();
  const nodes = useNodeBufferReport();
  const edgeText = overviewEdgeNoticeText(report);
  const nodeText = nodeBufferNoticeText(nodes);
  const faultText = overviewFaultNoticeText(useOverviewLive().faults);
  if (!edgeText && !nodeText && !faultText) return null;
  const text = [faultText, nodeText, edgeText].filter(Boolean).join(' · ');
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
