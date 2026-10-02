// The `/api/bots/data` `chain` view, pinned in the repository-root fixture that
// the Rust test `served_view_matches_the_pinned_screen_fixture` checks against
// the server's own serialisation of tests/fixtures/sidechain/chain-payments.v1.json.
import type { ChainPaymentsView } from '../chainPayments';
import pinned from '../../../../../../tests/fixtures/sidechain/bots-data-chain.v1.json';

export const DID_A = 'did:nostr:3bf34d40533c4da9e7d23f05c47573ac302e60d26910f3f2d9fb2cca5a7caa8e';
export const DID_B = 'did:nostr:30d89dcc39e2a9aa35f1b0d941aa2eedd939f0949b2bb31eedfe0aee7fd20098';

export const TX = {
  mempool: '1cdea06af117fe8e3cef652ab7fb1a3f1ef51ac1fed9e7a5c6259728017eb322',
  pastTip: 'c4ee635af3e648c43453f4744c478b5845d80a1e8f1a59e53d360f62df7ac9e7',
  atTip: 'd5587bdd00e73f51cc218a50311bcb5f30c66274088d620e25e31f1203d87176',
  first: '579effcfa31c97df0fa934c730cac4137e1bba716694e3bafe11cca8ac013a4c',
};

/** A fresh deep copy of the pinned view, so a test may mutate it. */
export function fixtureView(): ChainPaymentsView {
  return JSON.parse(JSON.stringify(pinned.chain)) as ChainPaymentsView;
}
