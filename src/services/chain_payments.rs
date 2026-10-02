//! Sidechain payments between agents, projected onto the agent graph (stream S5).
//!
//! This module is the single home of the contract VisionClaw reads from the
//! agentbox management API and of the rules that turn it into graph state:
//!
//! * `GET /v1/chain/payments` ([`CHAIN_PAYMENTS_PATH`]) returns a
//!   [`ChainPaymentsResponse`] whose `schema` is [`PAYMENTS_SCHEMA`]. The shape is
//!   pinned by `tests/fixtures/sidechain/chain-payments.v1.json`; the client panel
//!   tests read the same file.
//! * Each payment is a sidechain transaction (owner decision 2026-10-02, SC2). It
//!   becomes one `chain_payment` edge between two agent nodes, keyed by the
//!   agents' `did:nostr`. A payment whose payer or payee is not a verified agent
//!   node is dropped and counted, never drawn against a guessed node.
//! * A payment is shown as settled only when the endpoint says so AND it sits in a
//!   block at or below the reported tip. Anything else is drawn as unsettled.
//! * Balances carry one tier today, "settled (chain fold @ h)". The second tier,
//!   "in session (signed state n)", belongs to `GET /v1/chain/sessions`, which is
//!   empty until Hitch sessions land; an in-session figure is never folded into
//!   the settled one.
//! * The chain is not anchored to its parent (owner decision 2026-10-02, SC5).
//!   [`AnchorState`] reads "anchored" only when the response carries a parent
//!   checkpoint; with `checkpoint: null` it reads "not anchored (checkpoints off)".

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use visionclaw_domain::models::edge::{Edge, CHAIN_PAYMENT_EDGE_TYPE};
use visionclaw_domain::models::graph::GraphData;

/// Management API route for sidechain payments between agents.
pub const CHAIN_PAYMENTS_PATH: &str = "/v1/chain/payments";
/// Management API route for Hitch sessions (empty until sessions land).
pub const CHAIN_SESSIONS_PATH: &str = "/v1/chain/sessions";
/// The only payments contract version this build understands.
pub const PAYMENTS_SCHEMA: &str = "agentbox.chain.payments/1";
/// The only sessions contract version this build understands.
pub const SESSIONS_SCHEMA: &str = "agentbox.chain.sessions/1";
/// Most recent payments kept for the panel and drawn as edges.
pub const RECENT_PAYMENTS_LIMIT: usize = 50;
/// Label shown when the response carries no parent checkpoint (SC5).
pub const NOT_ANCHORED_LABEL: &str = "not anchored (checkpoints off)";

/// Node metadata keys written onto agent nodes by [`apply_to_bots_graph`].
pub mod node_keys {
    /// Settled balance in sats, from the chain fold.
    pub const SETTLED_SATS: &str = "chain_settled_sats";
    /// Chain height of that fold.
    pub const FOLD_HEIGHT: &str = "chain_fold_height";
    /// Human label of the balance tier, e.g. `settled (chain fold @ 941)`.
    pub const BALANCE_TIER: &str = "chain_balance_tier";
    /// `anchored` or `not_anchored`.
    pub const ANCHOR: &str = "chain_anchor";
    /// Human label of the anchor state.
    pub const ANCHOR_LABEL: &str = "chain_anchor_label";
    /// Chain id, e.g. `sidestr:dreamlab-txbt4`.
    pub const CHAIN: &str = "chain_id";

    /// Every key above, so a refresh can clear stale values.
    pub const ALL: [&str; 6] = [
        SETTLED_SATS,
        FOLD_HEIGHT,
        BALANCE_TIER,
        ANCHOR,
        ANCHOR_LABEL,
        CHAIN,
    ];
}

/// Body of `GET /v1/chain/payments`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainPaymentsResponse {
    /// Contract version; must equal [`PAYMENTS_SCHEMA`].
    pub schema: String,
    /// Chain id, e.g. `sidestr:dreamlab-txbt4`.
    pub chain: String,
    /// Public block mirror for transaction links.
    #[serde(default)]
    pub mirror_url: Option<String>,
    /// Producer tip the payments were read at; `null` when the producer is
    /// unreachable (payments then carry their stored inclusion, and no
    /// balance is shown because no fold height can be named).
    #[serde(default)]
    pub tip: Option<ChainTip>,
    /// Latest parent-chain checkpoint covering this chain; `null` while
    /// checkpoints are off.
    #[serde(default)]
    pub checkpoint: Option<ParentCheckpoint>,
    /// Payments between agents, newest first.
    pub payments: Vec<ChainPayment>,
    /// Settled balances from the chain fold at `tip`.
    #[serde(default)]
    pub balances: Vec<ChainBalance>,
}

/// Producer tip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainTip {
    /// Block height.
    pub height: u64,
    /// Block hash, hex.
    pub hash: String,
}

/// A checkpoint of this chain written into its parent chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentCheckpoint {
    /// Parent chain id, e.g. `tbtc4`.
    pub parent: String,
    /// Parent-chain transaction carrying the checkpoint.
    pub txid: String,
    /// Parent-chain block height of that transaction; `null` until the
    /// checkpoint confirms on the parent, and until then nothing is anchored.
    #[serde(default)]
    pub height: Option<u64>,
    /// Highest sidechain height the checkpoint commits to.
    pub covers_height: u64,
}

/// One payment, i.e. one sidechain transaction from payer to payee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainPayment {
    /// Sidechain transaction id, hex; `null` for a row that never reached
    /// the chain (pending approval, failed, denied). Such rows are skipped.
    #[serde(default)]
    pub txid: Option<String>,
    /// Payer `did:nostr:<hex>`.
    pub payer: String,
    /// Payee `did:nostr:<hex>`; `null` when the payee's binding did not
    /// verify. Such a payment is dropped and counted as unverified.
    #[serde(default)]
    pub payee: Option<String>,
    /// Amount in sats.
    pub amount_sats: u64,
    /// Including block height; `null` while in the mempool.
    #[serde(default)]
    pub block_height: Option<u64>,
    /// Including block hash; `null` while in the mempool.
    #[serde(default)]
    pub block_hash: Option<String>,
    /// Whether the endpoint considers the transaction settled.
    pub settled: bool,
    /// RFC 3339 time the payment was seen.
    #[serde(default)]
    pub time: Option<String>,
}

/// A settled balance from the chain fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainBalance {
    /// Owner `did:nostr:<hex>`.
    pub did: String,
    /// Settled sats.
    pub settled_sats: u64,
    /// Height of the fold.
    pub fold_height: u64,
}

/// Whether the chain is anchored to its parent, as far as the response shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AnchorState {
    /// A parent checkpoint covers the chain up to `covers_height`.
    Anchored {
        /// Parent chain id.
        parent: String,
        /// Parent-chain checkpoint transaction.
        txid: String,
        /// Parent-chain height.
        height: u64,
        /// Sidechain height covered.
        covers_height: u64,
        /// Display label.
        label: String,
    },
    /// A checkpoint was written but has not confirmed on the parent. Still
    /// not anchored: nothing on the parent yet commits to this chain.
    Pending {
        /// Parent chain id.
        parent: String,
        /// Parent-chain checkpoint transaction.
        txid: String,
        /// Sidechain height the checkpoint would cover.
        covers_height: u64,
        /// Display label.
        label: String,
    },
    /// No checkpoint: the chain rests on its single signer's word.
    NotAnchored {
        /// Display label, always [`NOT_ANCHORED_LABEL`].
        label: String,
    },
}

impl AnchorState {
    /// Derive the anchor state from an optional checkpoint.
    pub fn from_checkpoint(checkpoint: Option<&ParentCheckpoint>) -> Self {
        match checkpoint {
            Some(cp) => match cp.height {
                Some(height) => AnchorState::Anchored {
                    parent: cp.parent.clone(),
                    txid: cp.txid.clone(),
                    height,
                    covers_height: cp.covers_height,
                    label: format!(
                        "anchored @ {} h {} (covers {})",
                        cp.parent, height, cp.covers_height
                    ),
                },
                None => AnchorState::Pending {
                    parent: cp.parent.clone(),
                    txid: cp.txid.clone(),
                    covers_height: cp.covers_height,
                    label: format!("checkpoint unconfirmed on {} (not anchored)", cp.parent),
                },
            },
            None => AnchorState::NotAnchored {
                label: NOT_ANCHORED_LABEL.to_string(),
            },
        }
    }

    /// `true` only for [`AnchorState::Anchored`].
    pub fn is_anchored(&self) -> bool {
        matches!(self, AnchorState::Anchored { .. })
    }

    /// Display label.
    pub fn label(&self) -> &str {
        match self {
            AnchorState::Anchored { label, .. }
            | AnchorState::Pending { label, .. }
            | AnchorState::NotAnchored { label } => label,
        }
    }

    /// Short machine value written onto nodes.
    pub fn code(&self) -> &'static str {
        match self {
            AnchorState::Anchored { .. } => "anchored",
            AnchorState::Pending { .. } => "pending",
            AnchorState::NotAnchored { .. } => "not_anchored",
        }
    }
}

/// A payment between two verified agents, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectedPayment {
    /// Sidechain transaction id.
    pub txid: String,
    /// Payer DID (canonical).
    pub payer: String,
    /// Payee DID (canonical).
    pub payee: String,
    /// Amount in sats.
    pub amount_sats: u64,
    /// Including block height, if any.
    pub block_height: Option<u64>,
    /// Settled under this module's rule (see [`is_settled`]).
    pub settled: bool,
    /// Seen time, if reported.
    pub time: Option<String>,
    /// Link to the chain's public mirror for this payment; `None` when the
    /// response names no mirror (never another chain's mirror).
    pub mirror_link: Option<String>,
}

/// One balance tier on an agent node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettledBalance {
    /// Owner DID (canonical).
    pub did: String,
    /// Settled sats.
    pub settled_sats: u64,
    /// Fold height.
    pub fold_height: u64,
    /// Tier label, `settled (chain fold @ h)`.
    pub tier: String,
}

/// Why a payment was not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Payer or payee is not a well-formed `did:nostr`.
    MalformedDid,
    /// Payer or payee is well formed but is not a verified agent node.
    UnverifiedAgent,
}

/// The projection of one poll, held by the monitor and served to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainPaymentsSnapshot {
    /// Contract version read.
    pub schema: String,
    /// Chain id.
    pub chain: String,
    /// Mirror base URL used for links, as the response named it.
    pub mirror_url: Option<String>,
    /// Tip the payments were read at; `None` when the producer was unreachable.
    pub tip: Option<ChainTip>,
    /// Anchor state.
    pub anchor: AnchorState,
    /// Recent payments between verified agents, newest first.
    pub payments: Vec<ProjectedPayment>,
    /// Settled balances of verified agents.
    pub balances: Vec<SettledBalance>,
    /// Payments dropped this poll because an endpoint is not a verified agent.
    pub dropped_unverified: u32,
    /// Payments dropped this poll because a DID is malformed.
    pub dropped_malformed: u32,
    /// Rows skipped this poll because they never reached the chain (no txid:
    /// pending approval, failed, denied). Not an error.
    pub skipped_off_chain: u32,
}

/// Contract errors that make a response unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// The response names a schema this build does not understand.
    UnknownSchema(String),
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractError::UnknownSchema(s) => {
                write!(
                    f,
                    "unknown chain payments schema {s:?}; expected {PAYMENTS_SCHEMA:?}"
                )
            }
        }
    }
}

impl std::error::Error for ContractError {}

/// Settled means: the endpoint says settled, the payment has a block, and that
/// block is at or below the tip the response was read at. A payment that claims
/// settlement without a block, or in a block past the tip, is unsettled.
///
/// With no tip (producer unreachable) the endpoint's stored inclusion stands:
/// settled only if it says so and names a block.
pub fn is_settled(payment: &ChainPayment, tip_height: Option<u64>) -> bool {
    match (payment.block_height, tip_height) {
        (Some(h), Some(tip)) => payment.settled && h <= tip,
        (Some(_), None) => payment.settled,
        (None, _) => false,
    }
}

/// Balance tier label for a settled fold.
pub fn settled_tier_label(fold_height: u64) -> String {
    format!("settled (chain fold @ {fold_height})")
}

/// Mirror link for a transaction. The Pages mirror publishes the chain index,
/// not per-transaction pages, so the link opens the index with the txid as the
/// fragment.
pub fn mirror_link(mirror_url: &str, txid: &str) -> String {
    format!("{}/blocks.json#{}", mirror_url.trim_end_matches('/'), txid)
}

fn canonical_did(claimed: &str) -> Option<String> {
    crate::services::bots_client::validate_did_nostr(claimed)
}

/// Verified agent DIDs on the bots graph, mapped to their node ids.
///
/// A node counts only when it is an agent node (`is_agent = "true"`) and its
/// `did_nostr` survives the canonical `uri::did_nostr()` round-trip. That DID was
/// already gated on carry (`bots_client::Agent`); re-checking here keeps this
/// function safe on any graph.
pub fn verified_agent_dids(graph: &GraphData) -> HashMap<String, u32> {
    graph
        .nodes
        .iter()
        .filter(|n| n.metadata.get("is_agent").map(String::as_str) == Some("true"))
        .filter_map(|n| {
            let did = n.metadata.get("did_nostr")?;
            canonical_did(did).map(|d| (d, n.id))
        })
        .collect()
}

/// Project a response onto the set of verified agent DIDs.
pub fn project(
    response: &ChainPaymentsResponse,
    verified: &HashSet<String>,
) -> Result<ChainPaymentsSnapshot, ContractError> {
    if response.schema != PAYMENTS_SCHEMA {
        return Err(ContractError::UnknownSchema(response.schema.clone()));
    }
    // No default: two chains run side by side (`dreamlab`, `dreamlab-txbt4`),
    // and a fallback would link one chain's payment to the other's mirror.
    let mirror_url = response.mirror_url.clone().filter(|m| !m.is_empty());
    let tip_height = response.tip.as_ref().map(|t| t.height);

    let mut payments = Vec::new();
    let mut dropped_unverified = 0u32;
    let mut dropped_malformed = 0u32;
    let mut skipped_off_chain = 0u32;
    for p in &response.payments {
        let Some(txid) = p.txid.as_deref() else {
            skipped_off_chain += 1;
            continue;
        };
        let Some(payee_raw) = p.payee.as_deref() else {
            dropped_unverified += 1;
            continue;
        };
        let (Some(payer), Some(payee)) = (canonical_did(&p.payer), canonical_did(payee_raw)) else {
            dropped_malformed += 1;
            continue;
        };
        if !verified.contains(&payer) || !verified.contains(&payee) {
            dropped_unverified += 1;
            continue;
        }
        if payments.len() < RECENT_PAYMENTS_LIMIT {
            payments.push(ProjectedPayment {
                txid: txid.to_string(),
                payer,
                payee,
                amount_sats: p.amount_sats,
                block_height: p.block_height,
                settled: is_settled(p, tip_height),
                time: p.time.clone(),
                mirror_link: mirror_url.as_deref().map(|m| mirror_link(m, txid)),
            });
        }
    }

    let balances = response
        .balances
        .iter()
        // No tip, no fold height to name: no balance is shown rather than one
        // whose tier label would be a guess.
        .filter(|b| tip_height.is_some_and(|t| b.fold_height <= t))
        .filter_map(|b| {
            let did = canonical_did(&b.did)?;
            verified.contains(&did).then(|| SettledBalance {
                did,
                settled_sats: b.settled_sats,
                fold_height: b.fold_height,
                tier: settled_tier_label(b.fold_height),
            })
        })
        .collect();

    Ok(ChainPaymentsSnapshot {
        schema: response.schema.clone(),
        chain: response.chain.clone(),
        mirror_url,
        tip: response.tip.clone(),
        anchor: AnchorState::from_checkpoint(response.checkpoint.as_ref()),
        payments,
        balances,
        dropped_unverified,
        dropped_malformed,
        skipped_off_chain,
    })
}

/// Graph edge id for a payment.
pub fn edge_id(txid: &str) -> String {
    format!("{CHAIN_PAYMENT_EDGE_TYPE}:{txid}")
}

/// Build the `chain_payment` edges for a snapshot, resolving DIDs to node ids.
/// A payment whose agent is no longer on the graph is skipped until the next
/// poll re-projects it.
pub fn edges_for(
    snapshot: &ChainPaymentsSnapshot,
    did_to_node: &HashMap<String, u32>,
) -> Vec<Edge> {
    snapshot
        .payments
        .iter()
        .filter_map(|p| {
            let source = *did_to_node.get(&p.payer)?;
            let target = *did_to_node.get(&p.payee)?;
            let mut metadata = HashMap::new();
            metadata.insert(
                "communication_type".to_string(),
                CHAIN_PAYMENT_EDGE_TYPE.to_string(),
            );
            metadata.insert("chain".to_string(), snapshot.chain.clone());
            metadata.insert("txid".to_string(), p.txid.clone());
            metadata.insert("amount_sats".to_string(), p.amount_sats.to_string());
            metadata.insert("settled".to_string(), p.settled.to_string());
            if let Some(h) = p.block_height {
                metadata.insert("block_height".to_string(), h.to_string());
            }
            if let Some(link) = &p.mirror_link {
                metadata.insert("mirror_link".to_string(), link.clone());
            }
            Some(Edge {
                id: edge_id(&p.txid),
                source,
                target,
                weight: 1.0,
                edge_type: Some(CHAIN_PAYMENT_EDGE_TYPE.to_string()),
                owl_property_iri: None,
                metadata: Some(metadata),
            })
        })
        .collect()
}

/// Replace the `chain_payment` edges and chain node metadata on the bots graph
/// with those of `snapshot` (or clear them when `None`).
pub fn apply_to_bots_graph(graph: &mut GraphData, snapshot: Option<&ChainPaymentsSnapshot>) {
    graph
        .edges
        .retain(|e| e.edge_type.as_deref() != Some(CHAIN_PAYMENT_EDGE_TYPE));
    for node in graph.nodes.iter_mut() {
        for key in node_keys::ALL {
            node.metadata.remove(key);
        }
    }
    let Some(snapshot) = snapshot else { return };

    let did_to_node = verified_agent_dids(graph);
    let balances: HashMap<&str, &SettledBalance> = snapshot
        .balances
        .iter()
        .map(|b| (b.did.as_str(), b))
        .collect();
    for node in graph.nodes.iter_mut() {
        let Some(did) = node
            .metadata
            .get("did_nostr")
            .and_then(|d| canonical_did(d))
        else {
            continue;
        };
        if !did_to_node.contains_key(&did) {
            continue;
        }
        node.metadata
            .insert(node_keys::CHAIN.to_string(), snapshot.chain.clone());
        node.metadata.insert(
            node_keys::ANCHOR.to_string(),
            snapshot.anchor.code().to_string(),
        );
        node.metadata.insert(
            node_keys::ANCHOR_LABEL.to_string(),
            snapshot.anchor.label().to_string(),
        );
        if let Some(b) = balances.get(did.as_str()) {
            node.metadata.insert(
                node_keys::SETTLED_SATS.to_string(),
                b.settled_sats.to_string(),
            );
            node.metadata.insert(
                node_keys::FOLD_HEIGHT.to_string(),
                b.fold_height.to_string(),
            );
            node.metadata
                .insert(node_keys::BALANCE_TIER.to_string(), b.tier.clone());
        }
    }
    graph.edges.extend(edges_for(snapshot, &did_to_node));
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::node::Node;

    const FIXTURE: &str = include_str!("../../tests/fixtures/sidechain/chain-payments.v1.json");
    const SESSIONS_FIXTURE: &str =
        include_str!("../../tests/fixtures/sidechain/chain-sessions.v1.json");
    const A: &str = "did:nostr:3bf34d40533c4da9e7d23f05c47573ac302e60d26910f3f2d9fb2cca5a7caa8e";
    const B: &str = "did:nostr:30d89dcc39e2a9aa35f1b0d941aa2eedd939f0949b2bb31eedfe0aee7fd20098";
    const C: &str = "did:nostr:b5f180b509ecd638adb9167eff90e65e414b6dda28ee950db8cbe4bd12b1fa25";

    fn fixture() -> ChainPaymentsResponse {
        serde_json::from_str(FIXTURE).expect("fixture parses as the v1 contract")
    }

    fn verified_ab() -> HashSet<String> {
        [A, B].iter().map(|s| s.to_string()).collect()
    }

    fn agent_node(id: u32, did: Option<&str>) -> Node {
        let mut n = Node::new_with_id(format!("agent-{id}"), Some(id));
        n.metadata.insert("is_agent".into(), "true".into());
        if let Some(d) = did {
            n.metadata.insert("did_nostr".into(), d.into());
        }
        n
    }

    #[test]
    fn fixture_parses_and_sessions_fixture_is_empty() {
        let r = fixture();
        assert_eq!(r.schema, PAYMENTS_SCHEMA);
        assert_eq!(r.payments.len(), 6);
        let s: serde_json::Value = serde_json::from_str(SESSIONS_FIXTURE).unwrap();
        assert_eq!(s["schema"], SESSIONS_SCHEMA);
        assert_eq!(s["sessions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn payments_between_verified_agents_become_edges_and_others_are_counted() {
        let snap = project(&fixture(), &verified_ab()).unwrap();
        let txids: Vec<&str> = snap.payments.iter().map(|p| &p.txid[..6]).collect();
        assert_eq!(
            txids,
            vec!["1cdea0", "c4ee63", "d5587b", "579eff"],
            "newest first, A/B only"
        );
        assert_eq!(snap.dropped_unverified, 1, "A→C: C is not an agent node");
        assert_eq!(snap.dropped_malformed, 1, "B→did:nostr:NOT-A-PUBKEY");
    }

    #[test]
    fn settled_requires_a_block_at_or_below_the_tip() {
        let snap = project(&fixture(), &verified_ab()).unwrap();
        let by = |pre: &str| {
            snap.payments
                .iter()
                .find(|p| p.txid.starts_with(pre))
                .unwrap()
        };
        assert!(!by("1cdea0").settled, "mempool payment is unsettled");
        assert!(
            !by("c4ee63").settled,
            "claims settled at 945 > tip 941: unsettled"
        );
        assert!(by("d5587b").settled, "block 941 = tip");
        assert!(by("579eff").settled, "block 940 < tip");
    }

    #[test]
    fn balances_are_settled_tier_only_and_only_for_verified_agents() {
        let snap = project(&fixture(), &verified_ab()).unwrap();
        let dids: Vec<&str> = snap.balances.iter().map(|b| b.did.as_str()).collect();
        assert_eq!(dids, vec![A, B]);
        assert_eq!(snap.balances[0].settled_sats, 9000);
        assert_eq!(snap.balances[0].tier, "settled (chain fold @ 941)");
        assert!(!snap.balances.iter().any(|b| b.did == C));
    }

    #[test]
    fn null_checkpoint_is_not_anchored_and_a_checkpoint_is_anchored() {
        let snap = project(&fixture(), &verified_ab()).unwrap();
        assert!(!snap.anchor.is_anchored());
        assert_eq!(snap.anchor.label(), "not anchored (checkpoints off)");

        let mut r = fixture();
        r.checkpoint = Some(ParentCheckpoint {
            parent: "tbtc4".into(),
            txid: "ab".repeat(32),
            height: Some(120_000),
            covers_height: 941,
        });
        let snap = project(&r, &verified_ab()).unwrap();
        assert!(snap.anchor.is_anchored());
        assert_eq!(snap.anchor.code(), "anchored");
    }

    #[test]
    fn unknown_schema_is_refused() {
        let mut r = fixture();
        r.schema = "agentbox.chain.payments/2".into();
        assert!(matches!(
            project(&r, &verified_ab()),
            Err(ContractError::UnknownSchema(_))
        ));
    }

    #[test]
    fn nobody_verified_means_no_edges_and_everything_counted() {
        let snap = project(&fixture(), &HashSet::new()).unwrap();
        assert!(snap.payments.is_empty());
        assert!(snap.balances.is_empty());
        assert_eq!(snap.dropped_unverified + snap.dropped_malformed, 6);
    }

    #[test]
    fn verified_agent_dids_reads_only_agent_nodes_with_canonical_dids() {
        let mut g = GraphData::new();
        g.nodes = vec![
            agent_node(10_000, Some(A)),
            agent_node(10_001, Some("did:nostr:XYZ")),
            agent_node(10_002, None),
        ];
        let mut not_agent = Node::new_with_id("page".into(), Some(5));
        not_agent.metadata.insert("did_nostr".into(), B.into());
        g.nodes.push(not_agent);
        let m = verified_agent_dids(&g);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(A), Some(&10_000));
    }

    #[test]
    fn apply_draws_chain_payment_edges_with_amount_txid_block_settled() {
        let mut g = GraphData::new();
        g.nodes = vec![agent_node(10_000, Some(A)), agent_node(10_001, Some(B))];
        g.edges.push(Edge::new(10_000, 10_001, 0.5));
        let snap = project(&fixture(), &verified_ab()).unwrap();

        apply_to_bots_graph(&mut g, Some(&snap));

        let chain: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| e.edge_type.as_deref() == Some("chain_payment"))
            .collect();
        assert_eq!(chain.len(), 4);
        assert_eq!(g.edges.len(), 5, "collaboration edge kept");
        let e = chain
            .iter()
            .find(|e| {
                e.id == edge_id("579effcfa31c97df0fa934c730cac4137e1bba716694e3bafe11cca8ac013a4c")
            })
            .unwrap();
        assert_eq!((e.source, e.target), (10_000, 10_001), "A pays B");
        let md = e.metadata.as_ref().unwrap();
        assert_eq!(md["amount_sats"], "1000");
        assert_eq!(md["block_height"], "940");
        assert_eq!(md["settled"], "true");
        let mempool = chain.iter().find(|e| e.id.contains("1cdea06a")).unwrap();
        let md = mempool.metadata.as_ref().unwrap();
        assert_eq!(md["settled"], "false");
        assert!(!md.contains_key("block_height"));

        let a = &g.nodes[0].metadata;
        assert_eq!(a[node_keys::SETTLED_SATS], "9000");
        assert_eq!(a[node_keys::BALANCE_TIER], "settled (chain fold @ 941)");
        assert_eq!(a[node_keys::ANCHOR], "not_anchored");
        assert_eq!(a[node_keys::ANCHOR_LABEL], "not anchored (checkpoints off)");
    }

    #[test]
    fn apply_is_idempotent_and_none_clears() {
        let mut g = GraphData::new();
        g.nodes = vec![agent_node(10_000, Some(A)), agent_node(10_001, Some(B))];
        let snap = project(&fixture(), &verified_ab()).unwrap();
        apply_to_bots_graph(&mut g, Some(&snap));
        apply_to_bots_graph(&mut g, Some(&snap));
        assert_eq!(g.edges.len(), 4, "a re-apply replaces, never duplicates");
        apply_to_bots_graph(&mut g, None);
        assert!(g.edges.is_empty());
        assert!(!g.nodes[0].metadata.contains_key(node_keys::SETTLED_SATS));
    }

    #[test]
    fn an_agent_that_left_the_graph_loses_its_edges_until_the_next_poll() {
        let mut g = GraphData::new();
        g.nodes = vec![agent_node(10_000, Some(A))];
        let snap = project(&fixture(), &verified_ab()).unwrap();
        apply_to_bots_graph(&mut g, Some(&snap));
        assert!(g.edges.is_empty());
    }

    /// The view `/api/bots/data` serves for the payments fixture is pinned by
    /// `bots-data-chain.v1.json`, which the client tests and the rehearsal
    /// script's self-test read. A change to the served shape fails here first.
    #[test]
    fn served_view_matches_the_pinned_screen_fixture() {
        let mut view = crate::actors::messages::ChainPaymentsView::default();
        let snap = project(&fixture(), &verified_ab()).unwrap();
        view.dropped_unverified_total = u64::from(snap.dropped_unverified);
        view.dropped_malformed_total = u64::from(snap.dropped_malformed);
        view.route_available = true;
        view.snapshot = Some(std::sync::Arc::new(snap));
        let served = serde_json::to_value(&view).unwrap();
        let pinned: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/sidechain/bots-data-chain.v1.json"
        ))
        .unwrap();
        assert_eq!(served, pinned["chain"]);
    }

    #[test]
    fn a_response_without_a_mirror_gets_no_link_never_another_chains() {
        let mut r = fixture();
        r.mirror_url = None;
        let snap = project(&r, &verified_ab()).unwrap();
        assert_eq!(snap.mirror_url, None);
        assert!(snap.payments.iter().all(|p| p.mirror_link.is_none()));
        let mut g = GraphData::new();
        g.nodes = vec![agent_node(10_000, Some(A)), agent_node(10_001, Some(B))];
        apply_to_bots_graph(&mut g, Some(&snap));
        assert!(g
            .edges
            .iter()
            .all(|e| !e.metadata.as_ref().unwrap().contains_key("mirror_link")));
        assert!(g
            .edges
            .iter()
            .all(|e| e.metadata.as_ref().unwrap()["chain"] == "sidestr:dreamlab-txbt4"));
    }

    #[test]
    fn mirror_link_points_at_the_pages_index() {
        assert_eq!(
            mirror_link(
                "https://dreamlab-ai.github.io/sidestr-dreamlab-txbt4/",
                "abcd"
            ),
            "https://dreamlab-ai.github.io/sidestr-dreamlab-txbt4/blocks.json#abcd"
        );
    }

    /// A row exactly as S3's route emits it, extra fields included.
    fn s3_row(txid: Option<&str>, payee: Option<&str>, status: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "pay-1", "status": status, "txid": txid,
            "payer": format!("did:nostr:{}", "a".repeat(64)),
            "payee": payee, "amount_sats": 1200,
            "block_height": if txid.is_some() { serde_json::json!(941) } else { serde_json::Value::Null },
            "block_hash": null, "settled": status == "settled", "time": "2026-10-02T20:00:00Z",
            "fee_sats": 3, "receipt_urn": null, "payment_urn": "urn:x", "memo": "m",
            "payee_pubkey": null, "payee_address": null, "url": null, "error": null
        })
    }

    fn s3_response(
        rows: Vec<serde_json::Value>,
        tip: serde_json::Value,
        checkpoint: serde_json::Value,
    ) -> ChainPaymentsResponse {
        serde_json::from_value(serde_json::json!({
            "schema": PAYMENTS_SCHEMA, "chain": "sidestr:dreamlab-txbt4",
            "mirror_url": null, "tip": tip, "checkpoint": checkpoint,
            "payments": rows,
            "balances": [{"did": format!("did:nostr:{}", "a".repeat(64)), "settled_sats": 5000, "fold_height": 941}]
        }))
        .expect("S3 shape with nulls and extra fields must parse")
    }

    fn both_verified() -> HashSet<String> {
        [
            format!("did:nostr:{}", "a".repeat(64)),
            format!("did:nostr:{}", "b".repeat(64)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn off_chain_and_unattributed_rows_do_not_break_the_response() {
        let b = format!("did:nostr:{}", "b".repeat(64));
        let tx = "c".repeat(64);
        let resp = s3_response(
            vec![
                s3_row(None, Some(&b), "pending-approval"),
                s3_row(None, Some(&b), "failed"),
                s3_row(Some(&tx), None, "settled"),
                s3_row(Some(&tx), Some(&b), "settled"),
            ],
            serde_json::json!({"height": 941, "hash": "d".repeat(64)}),
            serde_json::Value::Null,
        );
        let snap = project(&resp, &both_verified()).unwrap();
        assert_eq!(
            snap.skipped_off_chain, 2,
            "no txid: never on chain, skipped"
        );
        assert_eq!(
            snap.dropped_unverified, 1,
            "null payee: binding unverified, dropped"
        );
        assert_eq!(snap.payments.len(), 1);
        assert!(snap.payments[0].settled);
        assert_eq!(
            snap.payments[0].mirror_link, None,
            "no mirror named, no link"
        );
    }

    #[test]
    fn no_tip_keeps_stored_inclusion_and_shows_no_balance() {
        let b = format!("did:nostr:{}", "b".repeat(64));
        let tx = "c".repeat(64);
        let resp = s3_response(
            vec![s3_row(Some(&tx), Some(&b), "settled")],
            serde_json::Value::Null,
            serde_json::Value::Null,
        );
        let snap = project(&resp, &both_verified()).unwrap();
        assert!(snap.tip.is_none());
        assert!(
            snap.payments[0].settled,
            "stored inclusion stands without a tip"
        );
        assert!(
            snap.balances.is_empty(),
            "no fold height to name: no settled figure"
        );
    }

    #[test]
    fn unconfirmed_checkpoint_is_pending_and_not_anchored() {
        let resp = s3_response(
            vec![],
            serde_json::json!({"height": 941, "hash": "d".repeat(64)}),
            serde_json::json!({"parent": "tbtc4", "txid": "e".repeat(64), "height": null, "covers_height": 940}),
        );
        let snap = project(&resp, &both_verified()).unwrap();
        assert!(!snap.anchor.is_anchored());
        assert_eq!(snap.anchor.code(), "pending");
        assert_eq!(
            snap.anchor.label(),
            "checkpoint unconfirmed on tbtc4 (not anchored)"
        );
    }

    #[test]
    fn confirmed_checkpoint_is_anchored() {
        let resp = s3_response(
            vec![],
            serde_json::json!({"height": 941, "hash": "d".repeat(64)}),
            serde_json::json!({"parent": "tbtc4", "txid": "e".repeat(64), "height": 120_000, "covers_height": 940}),
        );
        let snap = project(&resp, &both_verified()).unwrap();
        assert!(snap.anchor.is_anchored());
        assert_eq!(
            snap.anchor.label(),
            "anchored @ tbtc4 h 120000 (covers 940)"
        );
    }
}
