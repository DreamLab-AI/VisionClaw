// Sidechain payments between agents (stream S5): wire types and pure display
// rules. The server projects `GET /v1/chain/payments` (agentbox) onto the bots
// graph in `src/services/chain_payments.rs`; this file is its client mirror.
//
// Rules carried over from the owner decisions of 2026-10-02:
//   SC2  each payment is one sidechain transaction → one `chain_payment` edge.
//   SC5  the chain is not anchored to its parent; the screen says so plainly and
//        shows "anchored" only when the server reports a parent checkpoint.
// Balance tiers: "settled (chain fold @ h)" today; "in session (signed state n)"
// is a separate tier that arrives with Hitch sessions and is never shown as, or
// summed into, the settled figure.

/** Edge label the server writes on payment edges. Not `hierarchical`, not `domain_member`. */
export const CHAIN_PAYMENT_EDGE_TYPE = 'chain_payment';

/** Payment edge colour: magenta, distinct from every ontology edge hue. */
export const CHAIN_PAYMENT_COLOR = '#E040FB';

/** Label when no parent checkpoint covers the chain (SC5). */
export const NOT_ANCHORED_LABEL = 'not anchored (checkpoints off)';

/** Fallback label for a checkpoint not yet confirmed on the parent. */
export const PENDING_ANCHOR_LABEL = 'checkpoint unconfirmed (not anchored)';

/** Payments listed by the control-centre panel. */
export const PANEL_PAYMENT_LIMIT = 10;

export type AnchorState =
  | {
      state: 'anchored';
      parent: string;
      txid: string;
      height: number;
      covers_height: number;
      label: string;
    }
  | {
      /** Checkpoint written, not yet confirmed on the parent: still not anchored. */
      state: 'pending';
      parent: string;
      txid: string;
      covers_height: number;
      label: string;
    }
  | { state: 'not_anchored'; label: string };

export interface ProjectedPayment {
  txid: string;
  payer: string;
  payee: string;
  amount_sats: number;
  block_height: number | null;
  settled: boolean;
  time: string | null;
  /** Null when the server names no mirror for this chain. */
  mirror_link: string | null;
}

export interface SettledBalance {
  did: string;
  settled_sats: number;
  fold_height: number;
  tier: string;
}

export interface ChainPaymentsSnapshot {
  schema: string;
  chain: string;
  mirror_url: string | null;
  /** Null when the producer was unreachable; balances are then empty. */
  tip: { height: number; hash: string } | null;
  anchor: AnchorState;
  payments: ProjectedPayment[];
  balances: SettledBalance[];
  dropped_unverified: number;
  dropped_malformed: number;
  skipped_off_chain: number;
}

/** The `chain` object on `/api/bots/data` (Rust `ChainPaymentsView`). */
export interface ChainPaymentsView {
  snapshot: ChainPaymentsSnapshot | null;
  dropped_unverified_total: number;
  dropped_malformed_total: number;
  route_available: boolean;
}

/** Payment detail carried on a `chain_payment` BotsEdge. */
export interface ChainPaymentEdgeInfo {
  /** Chain id from the rail's read route, e.g. `sidestr:dreamlab-txbt4`. */
  chain?: string;
  txid: string;
  amountSats: number;
  blockHeight?: number;
  settled: boolean;
  mirrorLink?: string;
}

/** Chain badge carried on an agent node. */
export interface AgentChainBadge {
  chain: string;
  anchored: boolean;
  anchorLabel: string;
  /** Present only when the chain fold reported a settled balance for this DID. */
  settled?: { sats: number; foldHeight: number; label: string };
  /** Hitch session state — absent until `/v1/chain/sessions` carries sessions. */
  inSession?: { sats: number; signedState: number; label: string };
}

/** `ab12cd…ef90` form of a txid. */
export const shortTxid = (txid: string): string =>
  txid.length > 12 ? `${txid.slice(0, 6)}…${txid.slice(-4)}` : txid;

/** `1,000 sats`. */
export const formatSats = (sats: number): string =>
  `${Math.trunc(sats).toLocaleString('en-GB')} sats`;

/** Label drawn at an edge's midpoint: amount, short txid, and "unsettled" when so. */
export const chainEdgeLabel = (info: ChainPaymentEdgeInfo): string =>
  `${formatSats(info.amountSats)} · ${shortTxid(info.txid)}${info.settled ? '' : ' · unsettled'}`;

/** Settled tier label, matching the server's wording exactly. */
export const settledTierLabel = (foldHeight: number): string =>
  `settled (chain fold @ ${foldHeight})`;

/** Session tier label. Reserved for Hitch sessions; never used for settled coins. */
export const inSessionTierLabel = (signedState: number): string =>
  `in session (signed state ${signedState})`;

/** Read the payment detail off a raw server edge (`edgeType` + string metadata). */
export function edgeInfoFromWire(
  edgeType: string | undefined,
  metadata: Record<string, string> | undefined,
): ChainPaymentEdgeInfo | undefined {
  if (edgeType !== CHAIN_PAYMENT_EDGE_TYPE || !metadata?.txid) return undefined;
  const amount = Number(metadata.amount_sats);
  const block = metadata.block_height !== undefined ? Number(metadata.block_height) : undefined;
  return {
    chain: metadata.chain,
    txid: metadata.txid,
    amountSats: Number.isFinite(amount) ? amount : 0,
    blockHeight: block !== undefined && Number.isFinite(block) ? block : undefined,
    // Anything but the literal "true" is unsettled: never upgrade a payment.
    settled: metadata.settled === 'true',
    mirrorLink: metadata.mirror_link,
  };
}

/** Read the chain badge off agent node metadata (keys from `chain_payments::node_keys`). */
export function badgeFromNodeMetadata(
  metadata: Record<string, string | undefined> | undefined,
): AgentChainBadge | undefined {
  if (!metadata?.chain_id) return undefined;
  const anchored = metadata.chain_anchor === 'anchored';
  const badge: AgentChainBadge = {
    chain: metadata.chain_id,
    anchored,
    anchorLabel: anchored
      ? metadata.chain_anchor_label || 'anchored'
      : metadata.chain_anchor === 'pending'
        ? pendingLabel(metadata.chain_anchor_label)
        : NOT_ANCHORED_LABEL,
  };
  const sats = Number(metadata.chain_settled_sats);
  const fold = Number(metadata.chain_fold_height);
  if (metadata.chain_settled_sats !== undefined && Number.isFinite(sats) && Number.isFinite(fold)) {
    badge.settled = { sats, foldHeight: fold, label: settledTierLabel(fold) };
  }
  return badge;
}

/** `sidestr:dreamlab-txbt4` → `dreamlab-txbt4`. */
export const chainName = (chainId: string): string =>
  chainId.startsWith('sidestr:') ? chainId.slice('sidestr:'.length) : chainId;

/**
 * Badge lines in display order: settled tier, then session tier, then the
 * anchor state named for its chain (two chains run side by side, so "not
 * anchored" always says which one).
 */
export function badgeLines(badge: AgentChainBadge): string[] {
  const lines: string[] = [];
  if (badge.settled) lines.push(`${formatSats(badge.settled.sats)} — ${badge.settled.label}`);
  if (badge.inSession) lines.push(`${formatSats(badge.inSession.sats)} — ${badge.inSession.label}`);
  lines.push(`${chainName(badge.chain)}: ${badge.anchorLabel}`);
  return lines;
}

/** Anchor text for a snapshot: "anchored" only with a reported parent checkpoint. */
export const anchorText = (anchor: AnchorState | undefined): string => {
  if (anchor?.state === 'anchored') return anchor.label;
  if (anchor?.state === 'pending') return pendingLabel(anchor.label);
  return NOT_ANCHORED_LABEL;
};

/** A pending label must say it is not anchored; anything else falls back. */
function pendingLabel(label: string | undefined): string {
  return label && label.endsWith('(not anchored)') ? label : PENDING_ANCHOR_LABEL;
}

/** Most recent payments for the panel, newest first as served. */
export const recentPayments = (
  snapshot: ChainPaymentsSnapshot | null | undefined,
  limit = PANEL_PAYMENT_LIMIT,
): ProjectedPayment[] => (snapshot?.payments ?? []).slice(0, limit);

/** Pull the `chain` view out of a `/bots/data` body, enveloped or bare. */
export function chainViewFromBotsData(raw: unknown): ChainPaymentsView | null {
  if (!raw || typeof raw !== 'object') return null;
  const body = raw as { chain?: ChainPaymentsView; data?: { chain?: ChainPaymentsView } };
  return body.chain ?? body.data?.chain ?? null;
}
