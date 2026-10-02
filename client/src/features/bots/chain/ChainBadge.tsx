// Agent-node chain badge (S5): the settled balance tier and the anchor state,
// rendered in the HTML nameplate. The anchor line is always shown, so a node
// never reads as anchored by omission (owner decision 2026-10-02, SC5).
import React from 'react';
import { AgentChainBadge, CHAIN_PAYMENT_COLOR, badgeLines } from './chainPayments';

export interface ChainBadgeProps {
  badge: AgentChainBadge;
}

export const ChainBadge: React.FC<ChainBadgeProps> = ({ badge }) => {
  const lines = badgeLines(badge);
  const anchorLine = lines[lines.length - 1];
  const tierLines = lines.slice(0, -1);
  return (
    <div data-testid="chain-badge" style={{ fontSize: '9px', textShadow: '0 0 3px black', fontFamily: 'monospace' }}>
      {tierLines.map((line) => (
        <div key={line} data-testid="chain-badge-tier" style={{ color: CHAIN_PAYMENT_COLOR }}>
          {line}
        </div>
      ))}
      <div
        data-testid="chain-badge-anchor"
        data-anchored={badge.anchored ? 'true' : 'false'}
        style={{ color: badge.anchored ? '#81C784' : '#FFB74D' }}
      >
        {anchorLine}
      </div>
    </div>
  );
};

ChainBadge.displayName = 'ChainBadge';

export default ChainBadge;
