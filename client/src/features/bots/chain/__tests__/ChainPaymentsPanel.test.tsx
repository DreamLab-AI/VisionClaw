import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, cleanup, within, waitFor } from '@testing-library/react';

const { getDataSpy } = vi.hoisted(() => ({ getDataSpy: vi.fn() }));
vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: { getData: (...args: unknown[]) => getDataSpy(...args) },
}));

import { ChainPaymentsPanel, ChainPaymentsPanelView } from '../ChainPaymentsPanel';
import { ChainBadge } from '../ChainBadge';
import { TX, fixtureView } from './viewFixture';

beforeEach(() => {
  cleanup();
  getDataSpy.mockReset();
});

describe('ChainPaymentsPanel (S5)', () => {
  it('lists the last N payments with mirror links and settled state', () => {
    render(<ChainPaymentsPanelView view={fixtureView()} limit={3} />);
    const rows = screen.getAllByTestId('chain-payment');
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveAttribute('data-settled', 'false');
    expect(rows[0]).toHaveTextContent('250 sats');
    expect(rows[0]).toHaveTextContent('unsettled');
    expect(rows[2]).toHaveAttribute('data-settled', 'true');
    expect(rows[2]).toHaveTextContent('settled @ 941');
    const link = within(rows[2]).getByRole('link');
    expect(link).toHaveAttribute('href', `https://dreamlab-ai.github.io/sidestr-dreamlab-txbt4/blocks.json#${TX.atTip}`);
    expect(link).toHaveTextContent('d5587b…7176');
  });

  it('shows a payment claimed past the tip as unsettled', () => {
    render(<ChainPaymentsPanelView view={fixtureView()} />);
    const row = screen.getAllByTestId('chain-payment')[1];
    expect(row).toHaveTextContent('50 sats');
    expect(row).toHaveAttribute('data-settled', 'false');
  });

  it('states plainly that the chain is not anchored', () => {
    render(<ChainPaymentsPanelView view={fixtureView()} />);
    const anchor = screen.getByTestId('chain-anchor');
    expect(anchor).toHaveTextContent('dreamlab-txbt4: not anchored (checkpoints off)');
    expect(anchor).toHaveAttribute('data-anchored', 'false');
    expect(screen.getByTestId('chain-tip')).toHaveTextContent('sidestr:dreamlab-txbt4 @ 941');
  });

  it('shows settled balances with their tier and no in-session figure', () => {
    render(<ChainPaymentsPanelView view={fixtureView()} />);
    const balances = screen.getAllByTestId('chain-balance');
    expect(balances).toHaveLength(2);
    expect(balances[0]).toHaveTextContent('9,000 sats');
    expect(balances[0]).toHaveTextContent('settled (chain fold @ 941)');
    expect(screen.getByTestId('chain-session-tier')).toHaveTextContent('in session: none');
  });

  it('shows the txid without a link when the chain names no mirror', () => {
    const view = fixtureView();
    view.snapshot!.mirror_url = null;
    view.snapshot!.payments.forEach((p) => { p.mirror_link = null; });
    render(<ChainPaymentsPanelView view={view} />);
    expect(screen.queryAllByRole('link')).toHaveLength(0);
    expect(screen.getAllByTestId('chain-payment')[3]).toHaveTextContent('579eff…3a4c');
  });

  it('says the tip is unknown when the producer is unreachable', () => {
    const view = fixtureView();
    view.snapshot!.tip = null;
    view.snapshot!.balances = [];
    render(<ChainPaymentsPanelView view={view} />);
    expect(screen.getByTestId('chain-tip')).toHaveTextContent('tip unknown (producer unreachable)');
    expect(screen.getAllByTestId('chain-payment')).toHaveLength(4);
  });

  it('counts payments it did not draw', () => {
    render(<ChainPaymentsPanelView view={fixtureView()} />);
    expect(screen.getByTestId('chain-dropped')).toHaveTextContent('1 unverified agent, 1 malformed did');
  });

  it('says so when the management API has no payments route', () => {
    render(<ChainPaymentsPanelView view={{ snapshot: null, dropped_unverified_total: 0, dropped_malformed_total: 0, route_available: false }} />);
    expect(screen.getByTestId('chain-unavailable')).toBeInTheDocument();
    expect(screen.queryByTestId('chain-anchor')).toBeNull();
  });

  it('reads the chain view from /bots/data', async () => {
    getDataSpy.mockResolvedValue({ success: true, nodes: [], edges: [], chain: fixtureView() });
    render(<ChainPaymentsPanel />);
    await waitFor(() => expect(screen.getAllByTestId('chain-payment')).toHaveLength(4));
    expect(getDataSpy).toHaveBeenCalledWith('/bots/data');
  });
});

describe('ChainBadge (S5)', () => {
  it('renders the settled tier then the not-anchored state', () => {
    render(
      <ChainBadge
        badge={{
          chain: 'sidestr:dreamlab-txbt4', anchored: false, anchorLabel: 'not anchored (checkpoints off)',
          settled: { sats: 1330, foldHeight: 941, label: 'settled (chain fold @ 941)' },
        }}
      />,
    );
    expect(screen.getByTestId('chain-badge-tier')).toHaveTextContent('1,330 sats — settled (chain fold @ 941)');
    const anchor = screen.getByTestId('chain-badge-anchor');
    expect(anchor).toHaveTextContent('dreamlab-txbt4: not anchored (checkpoints off)');
    expect(anchor).toHaveAttribute('data-anchored', 'false');
  });

  it('shows the anchor line even with no settled balance', () => {
    render(<ChainBadge badge={{ chain: 'c', anchored: false, anchorLabel: 'not anchored (checkpoints off)' }} />);
    expect(screen.queryByTestId('chain-badge-tier')).toBeNull();
    expect(screen.getByTestId('chain-badge-anchor')).toBeInTheDocument();
  });
});
