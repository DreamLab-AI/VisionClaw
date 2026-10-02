// Control-centre payments panel (S5): the chain, its tip and anchor state, the
// settled balances of verified agents, and the last N payments with links to
// the public Pages mirror. Payments whose payer or payee is not a verified
// agent node are not listed; the panel shows how many were dropped.
import React from 'react';
import { shortDid } from '../agentIdentity';
import {
  ChainPaymentsView,
  CHAIN_PAYMENT_COLOR,
  PANEL_PAYMENT_LIMIT,
  anchorText,
  chainName,
  formatSats,
  recentPayments,
  shortTxid,
} from './chainPayments';
import { useChainPayments } from './useChainPayments';

export interface ChainPaymentsPanelViewProps {
  view: ChainPaymentsView | null;
  error?: string | null;
  limit?: number;
}

/** Pure presentation, unit-tested without a network. */
export const ChainPaymentsPanelView: React.FC<ChainPaymentsPanelViewProps> = ({
  view,
  error,
  limit = PANEL_PAYMENT_LIMIT,
}) => {
  const snapshot = view?.snapshot ?? null;
  return (
    <section data-testid="chain-payments-panel" aria-label="Sidechain payments" className="text-xs">
      <div className="flex items-center justify-between mb-1">
        <span className="font-semibold" style={{ color: CHAIN_PAYMENT_COLOR }}>
          Sidechain payments
        </span>
        {snapshot && (
          <span className="text-muted-foreground" data-testid="chain-tip">
            {snapshot.chain} @ {snapshot.tip ? snapshot.tip.height : 'tip unknown (producer unreachable)'}
          </span>
        )}
      </div>

      {!view?.route_available && !snapshot && (
        <p data-testid="chain-unavailable" className="text-muted-foreground">
          {error ? `unavailable: ${error}` : 'No chain payments route on the management API yet.'}
        </p>
      )}

      {snapshot && (
        <>
          <p
            data-testid="chain-anchor"
            data-anchored={snapshot.anchor.state === 'anchored' ? 'true' : 'false'}
            style={{ color: snapshot.anchor.state === 'anchored' ? '#81C784' : '#FFB74D' }}
          >
            {chainName(snapshot.chain)}: {anchorText(snapshot.anchor)}
          </p>

          <div className="mt-1" data-testid="chain-balances">
            {snapshot.balances.length === 0 ? (
              <p className="text-muted-foreground">No settled balances for verified agents.</p>
            ) : (
              snapshot.balances.map((b) => (
                <div key={b.did} className="flex justify-between gap-2" data-testid="chain-balance">
                  <span className="font-mono">{shortDid(b.did)}</span>
                  <span>
                    {formatSats(b.settled_sats)} <span className="text-muted-foreground">{b.tier}</span>
                  </span>
                </div>
              ))
            )}
            <p className="text-muted-foreground" data-testid="chain-session-tier">
              in session: none (sessions not live)
            </p>
          </div>

          <ol className="mt-2 space-y-1" data-testid="chain-payment-list">
            {recentPayments(snapshot, limit).map((p) => (
              <li key={p.txid} data-testid="chain-payment" data-settled={p.settled ? 'true' : 'false'}>
                <span className="font-mono">{shortDid(p.payer)}</span> →{' '}
                <span className="font-mono">{shortDid(p.payee)}</span>{' '}
                <strong>{formatSats(p.amount_sats)}</strong>{' '}
                {p.mirror_link ? (
                  <a
                    href={p.mirror_link}
                    target="_blank"
                    rel="noreferrer noopener"
                    className="font-mono underline"
                    title={p.txid}
                  >
                    {shortTxid(p.txid)}
                  </a>
                ) : (
                  <span className="font-mono" title={p.txid}>{shortTxid(p.txid)}</span>
                )}{' '}
                <span className="text-muted-foreground">
                  {p.settled ? `settled @ ${p.block_height}` : 'unsettled'}
                </span>
              </li>
            ))}
          </ol>
        </>
      )}

      {view && view.dropped_unverified_total + view.dropped_malformed_total > 0 && (
        <p className="mt-1 text-muted-foreground" data-testid="chain-dropped">
          not drawn: {view.dropped_unverified_total} unverified agent,{' '}
          {view.dropped_malformed_total} malformed did (since start)
        </p>
      )}
    </section>
  );
};

ChainPaymentsPanelView.displayName = 'ChainPaymentsPanelView';

export const ChainPaymentsPanel: React.FC<{ className?: string }> = ({ className }) => {
  const { view, error } = useChainPayments();
  return (
    <div className={className}>
      <ChainPaymentsPanelView view={view} error={error} />
    </div>
  );
};

ChainPaymentsPanel.displayName = 'ChainPaymentsPanel';

export default ChainPaymentsPanel;
