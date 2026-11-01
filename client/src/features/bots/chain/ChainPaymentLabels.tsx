// Midpoint labels for sidechain payment edges (S5): amount and short txid,
// plus "unsettled" where it applies. Payments between the same two agents share
// one label (newest first, up to MAX_LINES_PER_PAIR) so they do not stack.
import React, { useMemo, useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';
import { agentDrawnPosition } from '../agentDrawnPosition';
import type { BotsEdge } from '../types/BotsTypes';
import { CHAIN_PAYMENT_COLOR, CHAIN_PAYMENT_EDGE_TYPE, chainEdgeLabel } from './chainPayments';

export const MAX_LINES_PER_PAIR = 3;

export interface ChainLabelGroup {
  key: string;
  srcId: string;
  tgtId: string;
  lines: string[];
  more: number;
}

/** Group payment edges by unordered agent pair, preserving the served order. */
export function groupChainLabels(edges: Iterable<BotsEdge>): ChainLabelGroup[] {
  const groups = new Map<string, ChainLabelGroup>();
  for (const edge of edges) {
    if (edge.type !== CHAIN_PAYMENT_EDGE_TYPE || !edge.chainPayment) continue;
    const [a, b] = edge.source < edge.target ? [edge.source, edge.target] : [edge.target, edge.source];
    const key = `${a}|${b}`;
    let g = groups.get(key);
    if (!g) {
      g = { key, srcId: a, tgtId: b, lines: [], more: 0 };
      groups.set(key, g);
    }
    if (g.lines.length < MAX_LINES_PER_PAIR) g.lines.push(chainEdgeLabel(edge.chainPayment));
    else g.more += 1;
  }
  return Array.from(groups.values());
}

const _src = new THREE.Vector3();
const _tgt = new THREE.Vector3();

const PairLabel: React.FC<{ group: ChainLabelGroup }> = ({ group }) => {
  const ref = useRef<THREE.Group>(null);
  // Both ends come from the drawn array, the same one the agents' gems are drawn from.
  useFrame(() => {
    const node = ref.current;
    if (!node) return;
    const ok = agentDrawnPosition(group.srcId, _src) && agentDrawnPosition(group.tgtId, _tgt);
    node.visible = ok;
    if (ok) node.position.set((_src.x + _tgt.x) / 2, (_src.y + _tgt.y) / 2, (_src.z + _tgt.z) / 2);
  });
  return (
    <group ref={ref}>
      <Html center style={{ pointerEvents: 'none', whiteSpace: 'nowrap' }}>
        <div
          data-testid="chain-edge-label"
          style={{ color: CHAIN_PAYMENT_COLOR, fontSize: '9px', fontFamily: 'monospace', textShadow: '0 0 3px black' }}
        >
          {group.lines.map((line, i) => (
            <div key={i}>{line}</div>
          ))}
          {group.more > 0 && <div>+{group.more} more</div>}
        </div>
      </Html>
    </group>
  );
};

export interface ChainPaymentLabelsProps {
  edges: Map<string, BotsEdge>;
}

export const ChainPaymentLabels: React.FC<ChainPaymentLabelsProps> = ({ edges }) => {
  const groups = useMemo(() => groupChainLabels(edges.values()), [edges]);
  return (
    <>
      {groups.map((g) => (
        <PairLabel key={g.key} group={g} />
      ))}
    </>
  );
};

export default ChainPaymentLabels;
