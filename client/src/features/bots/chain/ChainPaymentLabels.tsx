// Midpoint labels for sidechain payment edges (S5): amount and short txid,
// plus "unsettled" where it applies. Payments between the same two agents share
// one label (newest first, up to MAX_LINES_PER_PAIR) so they do not stack.
import React, { useMemo, useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';
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

const PairLabel: React.FC<{
  group: ChainLabelGroup;
  positionsRef: React.MutableRefObject<Map<string, THREE.Vector3>>;
}> = ({ group, positionsRef }) => {
  const ref = useRef<THREE.Group>(null);
  useFrame(() => {
    const s = positionsRef.current.get(group.srcId);
    const t = positionsRef.current.get(group.tgtId);
    const node = ref.current;
    if (!node) return;
    node.visible = !!(s && t);
    if (s && t) node.position.set((s.x + t.x) / 2, (s.y + t.y) / 2, (s.z + t.z) / 2);
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
  positionsRef: React.MutableRefObject<Map<string, THREE.Vector3>>;
}

export const ChainPaymentLabels: React.FC<ChainPaymentLabelsProps> = ({ edges, positionsRef }) => {
  const groups = useMemo(() => groupChainLabels(edges.values()), [edges]);
  return (
    <>
      {groups.map((g) => (
        <PairLabel key={g.key} group={g} positionsRef={positionsRef} />
      ))}
    </>
  );
};

export default ChainPaymentLabels;
