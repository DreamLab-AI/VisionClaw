// Server bots graph → client agents/edges for chain payments (S5).
import { describe, it, expect } from 'vitest';
import { transformAgentData, agentChanged, edgeChanged } from '../../hooks/useAgentPolling';
import { chainPaymentStyle } from '../../components/BotsEdges';
import { groupChainLabels, MAX_LINES_PER_PAIR } from '../ChainPaymentLabels';
import { CHAIN_PAYMENT_COLOR } from '../chainPayments';
import type { AgentSwarmData } from '../../services/AgentPollingService';
import { DID_A, DID_B, TX } from './viewFixture';

function wire(): AgentSwarmData {
  const chainMeta = { chain_id: 'sidestr:dreamlab-txbt4', chain_anchor: 'not_anchored', chain_anchor_label: 'not anchored (checkpoints off)', chain_fold_height: '941' };
  return {
    nodes: [
      { id: 10000, metadataId: 'task-a', label: 'a', metadata: { agent_type: 'coder', did_nostr: DID_A, chain_settled_sats: '9000', ...chainMeta } },
      { id: 10001, metadataId: 'task-b', label: 'b', metadata: { agent_type: 'coder', did_nostr: DID_B, chain_settled_sats: '1330', ...chainMeta } },
    ],
    edges: [
      { id: '10000-10001', source: 10000, target: 10001, weight: 0.5 },
      { id: `chain_payment:${TX.first}`, source: 10000, target: 10001, weight: 1, edgeType: 'chain_payment',
        metadata: { txid: TX.first, amount_sats: '1000', block_height: '940', settled: 'true' } },
      { id: `chain_payment:${TX.mempool}`, source: 10000, target: 10001, weight: 1, edgeType: 'chain_payment',
        metadata: { txid: TX.mempool, amount_sats: '250', settled: 'false' } },
    ],
  };
}

describe('transformAgentData with chain payments', () => {
  it('carries the DID and chain badge onto agents and payment detail onto edges', () => {
    const { agents, edges } = transformAgentData(wire());
    expect(agents[0].did_nostr).toBe(DID_A);
    expect(agents[0].chain?.settled?.sats).toBe(9000);
    expect(agents[0].chain?.anchored).toBe(false);
    const pay = edges.find((e) => e.id === `chain_payment:${TX.first}`)!;
    expect(pay.type).toBe('chain_payment');
    expect(pay.chainPayment).toMatchObject({ amountSats: 1000, blockHeight: 940, settled: true });
    expect(edges.find((e) => e.id === '10000-10001')!.chainPayment).toBeUndefined();
  });

  it('notices a payment settling and a balance moving', () => {
    const before = transformAgentData(wire());
    const next = wire();
    next.edges[2].metadata = { ...next.edges[2].metadata!, settled: 'true', block_height: '942' };
    next.nodes[0].metadata!.chain_settled_sats = '8750';
    const after = transformAgentData(next);
    expect(edgeChanged(before.edges[2], after.edges[2])).toBe(true);
    expect(agentChanged(before.agents[0], after.agents[0])).toBe(true);
    expect(edgeChanged(before.edges[1], after.edges[1])).toBe(false);
  });
});

describe('payment edge drawing', () => {
  it('uses the chain colour, dimmed while unsettled; other edges untouched', () => {
    const { edges } = transformAgentData(wire());
    expect(chainPaymentStyle(edges[1])).toEqual({ color: CHAIN_PAYMENT_COLOR, opacity: 1 });
    expect(chainPaymentStyle(edges[2])).toEqual({ color: CHAIN_PAYMENT_COLOR, opacity: 0.45 });
    expect(chainPaymentStyle(edges[0])).toBeUndefined();
  });

  it('groups payment labels per agent pair, newest first, capped', () => {
    const { edges } = transformAgentData(wire());
    const groups = groupChainLabels(edges);
    expect(groups).toHaveLength(1);
    expect(groups[0].lines).toEqual(['1,000 sats · 579eff…3a4c', '250 sats · 1cdea0…b322 · unsettled']);

    const many = Array.from({ length: MAX_LINES_PER_PAIR + 2 }, (_, i) => ({
      ...edges[1], id: `p${i}`, source: i % 2 ? 'task-a' : 'task-b', target: i % 2 ? 'task-b' : 'task-a',
    }));
    const [g] = groupChainLabels(many);
    expect(g.lines).toHaveLength(MAX_LINES_PER_PAIR);
    expect(g.more).toBe(2);
  });
});
