import { describe, it, expect } from 'vitest';
import {
  CHAIN_PAYMENT_COLOR,
  NOT_ANCHORED_LABEL,
  anchorText,
  badgeFromNodeMetadata,
  PENDING_ANCHOR_LABEL,
  badgeLines,
  chainEdgeLabel,
  chainViewFromBotsData,
  edgeInfoFromWire,
  inSessionTierLabel,
  recentPayments,
  settledTierLabel,
  shortTxid,
} from '../chainPayments';
import { EDGE_TYPE_COLORS } from '../../../graph/hooks/useGraphNodeColors';
import { TX, fixtureView } from './viewFixture';

describe('chain payment edge (S5)', () => {
  it('has its own colour, distinct from hierarchy and domain spokes', () => {
    const own = EDGE_TYPE_COLORS['chain_payment'].getHexString();
    expect(`#${own}`.toUpperCase()).toBe(CHAIN_PAYMENT_COLOR);
    expect(own).not.toBe(EDGE_TYPE_COLORS['hierarchical'].getHexString());
    expect(own).not.toBe(EDGE_TYPE_COLORS['domain_member'].getHexString());
  });

  it('reads payment detail only from chain_payment edges', () => {
    const md = { txid: TX.first, amount_sats: '1000', block_height: '940', settled: 'true' };
    expect(edgeInfoFromWire('chain_payment', md)).toEqual({
      txid: TX.first, amountSats: 1000, blockHeight: 940, settled: true, mirrorLink: undefined,
    });
    expect(edgeInfoFromWire('hierarchical', md)).toBeUndefined();
    expect(edgeInfoFromWire(undefined, md)).toBeUndefined();
  });

  it('carries the chain id from the rail onto the edge', () => {
    const info = edgeInfoFromWire('chain_payment', { txid: TX.first, amount_sats: '1', settled: 'true', chain: 'sidestr:dreamlab-txbt4' });
    expect(info?.chain).toBe('sidestr:dreamlab-txbt4');
  });

  it('never upgrades a payment to settled', () => {
    for (const settled of ['false', 'TRUE', '1', '', undefined]) {
      const info = edgeInfoFromWire('chain_payment', { txid: TX.first, amount_sats: '1', settled: settled as string });
      expect(info?.settled).toBe(false);
    }
  });

  it('labels an edge with amount and short txid, flagging unsettled', () => {
    expect(shortTxid(TX.first)).toBe('579eff…3a4c');
    expect(chainEdgeLabel({ txid: TX.first, amountSats: 1000, settled: true })).toBe('1,000 sats · 579eff…3a4c');
    expect(chainEdgeLabel({ txid: TX.mempool, amountSats: 250, settled: false })).toBe('250 sats · 1cdea0…b322 · unsettled');
  });
});

describe('balance tiers and anchor state', () => {
  it('builds the settled tier and the not-anchored state from node metadata', () => {
    const badge = badgeFromNodeMetadata({
      chain_id: 'sidestr:dreamlab-txbt4', chain_settled_sats: '9000', chain_fold_height: '941',
      chain_anchor: 'not_anchored', chain_anchor_label: 'not anchored (checkpoints off)',
    });
    expect(badge).toEqual({
      chain: 'sidestr:dreamlab-txbt4', anchored: false, anchorLabel: NOT_ANCHORED_LABEL,
      settled: { sats: 9000, foldHeight: 941, label: 'settled (chain fold @ 941)' },
    });
    expect(badgeLines(badge!)).toEqual(['9,000 sats — settled (chain fold @ 941)', `dreamlab-txbt4: ${NOT_ANCHORED_LABEL}`]);
  });

  it('does not claim anchoring on a forged label without the anchored code', () => {
    const badge = badgeFromNodeMetadata({ chain_id: 'x', chain_anchor_label: 'anchored @ tbtc4 h 1' });
    expect(badge?.anchored).toBe(false);
    expect(badge?.anchorLabel).toBe(NOT_ANCHORED_LABEL);
  });

  it('keeps the session tier separate from, and after, the settled tier', () => {
    expect(settledTierLabel(941)).toBe('settled (chain fold @ 941)');
    expect(inSessionTierLabel(7)).toBe('in session (signed state 7)');
    const lines = badgeLines({
      chain: 'c', anchored: false, anchorLabel: NOT_ANCHORED_LABEL,
      settled: { sats: 100, foldHeight: 9, label: settledTierLabel(9) },
      inSession: { sats: 40, signedState: 7, label: inSessionTierLabel(7) },
    });
    expect(lines).toEqual(['100 sats — settled (chain fold @ 9)', '40 sats — in session (signed state 7)', `c: ${NOT_ANCHORED_LABEL}`]);
  });

  it('shows an unconfirmed checkpoint as pending, never anchored', () => {
    const badge = badgeFromNodeMetadata({
      chain_id: 'x', chain_anchor: 'pending', chain_anchor_label: 'checkpoint unconfirmed on tbtc4 (not anchored)',
    });
    expect(badge?.anchored).toBe(false);
    expect(badge?.anchorLabel).toBe('checkpoint unconfirmed on tbtc4 (not anchored)');
    const forged = badgeFromNodeMetadata({ chain_id: 'x', chain_anchor: 'pending', chain_anchor_label: 'anchored @ tbtc4' });
    expect(forged?.anchorLabel).toBe(PENDING_ANCHOR_LABEL);
    expect(anchorText({ state: 'pending', parent: 'tbtc4', txid: 'e', covers_height: 9, label: 'checkpoint unconfirmed on tbtc4 (not anchored)' }))
      .toBe('checkpoint unconfirmed on tbtc4 (not anchored)');
  });

  it('has no badge for a node outside the chain', () => {
    expect(badgeFromNodeMetadata({ agent_type: 'coder' })).toBeUndefined();
  });

  it('says not anchored unless the snapshot carries a checkpoint', () => {
    expect(anchorText(undefined)).toBe(NOT_ANCHORED_LABEL);
    expect(anchorText({ state: 'not_anchored', label: 'anything' })).toBe(NOT_ANCHORED_LABEL);
    expect(anchorText({ state: 'anchored', parent: 'tbtc4', txid: 'ab', height: 5, covers_height: 941, label: 'anchored @ tbtc4 h 5 (covers 941)' }))
      .toBe('anchored @ tbtc4 h 5 (covers 941)');
  });
});

describe('view plumbing', () => {
  it('finds the chain view in an enveloped or bare /bots/data body', () => {
    const view = fixtureView();
    expect(chainViewFromBotsData({ success: true, nodes: [], chain: view })).toBe(view);
    expect(chainViewFromBotsData({ data: { chain: view } })).toBe(view);
    expect(chainViewFromBotsData({ nodes: [] })).toBeNull();
    expect(chainViewFromBotsData(null)).toBeNull();
  });

  it('limits the panel list, newest first', () => {
    const ps = recentPayments(fixtureView().snapshot, 2).map((p) => p.txid);
    expect(ps).toEqual([TX.mempool, TX.pastTip]);
    expect(recentPayments(null)).toEqual([]);
  });
});
