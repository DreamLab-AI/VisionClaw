//! The deploy ritual + the `verify` audit (ADR-124 build-out §2.3/§2.4).
//!
//! This is `src/web_contract`, the ADR-124 trust ladder; it is not
//! `crates/visionclaw-contracts`, the envelope-schema crate (ADR-2111 D9).
//!
//! ## The deploy ritual
//!
//! `edit → validate → commit → git-mark → push; verify` — adopted from the
//! Carvalho deploy ritual. The `gitmark.json` step is the only verbatim
//! artefact (C7); the `validate-cli` 3-gate registry is reconstructed per the
//! webcontracts.org reference shape (C6), not lifted from a fetchable
//! create-agent artefact. The trail check in `verify` follows
//! blocktrails/verify 043e7af.
//!
//! | Step      | This impl |
//! |-----------|-----------|
//! | edit      | LDP write to the pod (WAC/402-gated) — existing substrate |
//! | validate  | [`super::reducer::ContractReducer::validate`] + the 3-gate [`Checks`] registry |
//! | commit    | git commit capturing the SHA + `agent_did` (ADR-125 `did:nostr`) author |
//! | git-mark  | emit the verbatim [`super::trail::GitMark`] (C7 five-key envelope) |
//! | anchor    | BIP-341 taproot tx (the existing Bitcoin write-side substrate) |
//! | push      | git push |
//! | verify    | [`verify`] below |
//!
//! ## The `verify` audit (§2.4)
//!
//! Given a contract package, [`verify`]:
//!   1. **recomputes the reducer** — replays `transition()` from `genesis` over
//!      the event log; asserts the stored canonical state hash == the replay;
//!   2. **replays the ledger** — recomputes balances from the reducer output;
//!      asserts the stored `ledger.json` == the replay;
//!   3. **asserts git-clean** — the working tree matches the last `gitmark.json`
//!      commit SHA;
//!   4. **walks the trail** ([`verify_trail`]) — every mark, not the tip
//!      alone. Each mark's output key `x(P_i)` is recomputed from `pubkeyBase`
//!      and the states by solid-pod-rs's walker
//!      ([`solid_pod_rs::blocktrail::walk_blocktrail`]); the
//!      [`AnchorConfirmer`] then shows each mark's output is that key, that
//!      the mark spends its predecessor and that it is confirmed. The verdict
//!      is blocktrails/verify's: `verified` when every commitment was checked
//!      and holds, `confirmed` when the marks are on-chain but there was
//!      nothing to recompute from (no `pubkeyBase`), `partial` otherwise.
//!
//! A confirmed tip shows only that someone paid the tip's key. Under
//! Blocktrails each tweak depends on the point before it, so a trail whose
//! states are reordered derives different keys for every mark from the first
//! change on, and the walk refuses it.
//!
//! ## Trust model (ADR-124 §4) — honest-or-caught → anchored → trustless
//!
//! [`TrustLevel`] is the trust commitment and the capability gate.
//!   * `L0` (honest-or-caught): public pure reducer + published verifier +
//!     per-state block-anchor + oracle ACL = operator `did:nostr`. The trail
//!     must be at least `confirmed`.
//!   * `L1` (anchored): the trail must be `verified` — every mark is the key
//!     its states derive, spends the mark before it, and is confirmed.
//!   * `L2`/`L3` (RGB/DLC trustless): **HARD-REFUSED** by [`TrustLevel::gate`]
//!     until the adaptor-sig CET engine is built AND independently audited.
//!
//! ## Invariant boundary
//!
//! The whole ritual is identity-rail-agnostic. The `agent_did` author is the
//! ADR-125 `did:nostr:<hex>` string, carried unchanged (I1). Nothing here parses
//! a verification method (I3) or re-encodes a key (I2). ADR-074 §D1 stays (I4).
//! The trail's key arithmetic is solid-pod-rs's (rust-bitcoin's libsecp256k1);
//! nothing here computes a point.

use serde::Serialize;
use solid_pod_rs::blocktrail::{
    walk_blocktrail, Blocktrail, MarkReport, MarkStatus, TrailReport, TrailVerdict,
};

use super::ledger::Ledger;
use super::reducer::{ContractReducer, ReducerError, TransitionError};
use super::state::CanonicalState;
use super::trail::Blocktrails;

/// The trust spectrum + capability gate (ADR-124 §4 / build-out §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// honest-or-caught: public reducer + published verifier + block-anchor.
    /// The trail must be at least `confirmed`.
    L0HonestOrCaught,
    /// anchored: the trail is `verified`, every mark walked (each the key its
    /// states derive, spending the mark before it, confirmed).
    L1Anchored,
    /// RGB/DLC trustless endgame (adaptor-sig CET) — gated off.
    L2AdaptorSigCet,
    /// RGB consignment trustless endgame — gated off.
    L3Rgb,
}

impl TrustLevel {
    /// The capability gate: a hard pre-condition on `transition()` commit.
    ///
    /// `L0`/`L1` are available. `L2`/`L3` are **HARD-REFUSED** until the
    /// adaptor-sig CET engine is built and independently audited (ADR-124 §2.4 /
    /// R3). `L0 → L1` is in place over the same trail (it asks the walk for
    /// more); `L2 → L3` is a layer rewrite.
    pub fn gate(self) -> Result<(), &'static str> {
        match self {
            TrustLevel::L0HonestOrCaught | TrustLevel::L1Anchored => Ok(()),
            TrustLevel::L2AdaptorSigCet | TrustLevel::L3Rgb => Err(
                "trustless (RGB/DLC) trust levels are hard-refused until the \
                 adaptor-signature CET engine is built and independently audited \
                 (ADR-124 §2.4 R3)",
            ),
        }
    }

    /// True iff this level is currently deployable.
    pub fn is_available(self) -> bool {
        self.gate().is_ok()
    }

    /// Whether a trail with this verdict meets this level: `L0` takes
    /// `verified` or `confirmed`, `L1` only `verified`, `L2`/`L3` nothing.
    pub fn accepts(self, verdict: TrailVerdict) -> bool {
        match self {
            TrustLevel::L0HonestOrCaught => {
                matches!(verdict, TrailVerdict::Verified | TrailVerdict::Confirmed)
            }
            TrustLevel::L1Anchored => verdict == TrailVerdict::Verified,
            TrustLevel::L2AdaptorSigCet | TrustLevel::L3Rgb => false,
        }
    }
}

/// The 3-gate CHECKS registry (port of `validate-cli.js`, C6 reconstruction).
///
/// One schema, three gates — browser/wasm, `ship`, and CI — all running the same
/// pure validation so the answer cannot diverge across environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// In-browser / wasm validation (the author's edit-time check).
    BrowserWasm,
    /// The `ship` ritual gate (pre-commit).
    Ship,
    /// The CI gate (post-push).
    Ci,
}

/// Runs the single reducer-validate across all three gates and reports any
/// divergence — the determinism guarantee the three-gate registry exists to
/// protect.
pub struct Checks;

impl Checks {
    /// All three gates.
    pub const GATES: [Gate; 3] = [Gate::BrowserWasm, Gate::Ship, Gate::Ci];

    /// Validate a state at all three gates. Because the reducer is pure, every
    /// gate must return identical findings; this asserts that and returns the
    /// (single) finding set. A divergence is a determinism bug and is surfaced as
    /// an `Err`.
    pub fn run_all<R: ContractReducer>(
        reducer: &R,
        state: &R::State,
    ) -> Result<Vec<ReducerError>, &'static str> {
        let mut prior: Option<Vec<ReducerError>> = None;
        for _gate in Self::GATES {
            let findings = reducer.validate(state);
            match &prior {
                None => prior = Some(findings),
                Some(p) if *p != findings => {
                    return Err("reducer validation diverged across gates (non-deterministic)")
                }
                Some(_) => {}
            }
        }
        Ok(prior.unwrap_or_default())
    }
}

/// What the chain says about a trail's marks: the seam [`verify`] reads the
/// chain through, so the verifier is testable without a node.
///
/// A production implementation answers from a node or an explorer
/// (ADR-2111 D2 names `sidestr-node`); the tests answer from captured
/// mempool.space transactions.
pub trait AnchorConfirmer {
    /// True iff the transaction holding `txid:vout` is in a block.
    fn is_confirmed(&self, txid: &str, vout: u32) -> bool;

    /// The key output `txid:vout` pays: for a P2TR output its x-only key as
    /// 64 hex (the witness program of its `5120…` script). For any other
    /// output, its scriptPubKey hex, which never equals a key. `None` when the
    /// transaction or the output does not exist.
    fn output_key(&self, txid: &str, vout: u32) -> Option<String>;

    /// True iff transaction `txid` spends the output `prev_txid:prev_vout`.
    fn spends(&self, txid: &str, prev_txid: &str, prev_vout: u32) -> bool;
}

/// Walk a trail mark by mark, as blocktrails/verify 043e7af does.
///
/// Every mark's output key `x(P_i)` is recomputed from `pubkeyBase` and the
/// states by [`walk_blocktrail`] (a string state hashed as its text, an object
/// as its JCS; a git-mark trail with no states reads its commits from its TXO
/// URIs). Then, through `confirmer`, each mark must exist
/// ([`MarkStatus::NotFound`]), pay the recomputed key
/// ([`MarkStatus::WrongKey`]), spend the previous mark
/// ([`MarkStatus::BrokenLink`]) and be confirmed ([`MarkStatus::Unconfirmed`]).
/// A trail that cannot be walked (no `pubkeyBase`, say) is still checked for
/// existence, links and confirmation, and is at best
/// [`TrailVerdict::Confirmed`], the reason in [`TrailReport::walk_error`].
///
/// Two differences from blocktrails/verify, both shared with solid-pod-rs's
/// `verify_blocktrail`, or stricter: a mark that does not spend its
/// predecessor is `broken link` (the page shows "chain link ✗" yet labels it
/// verified), and the link is checked as the exact outpoint, not the txid
/// alone. One difference from both: the [`AnchorConfirmer`] does not report
/// output values, so the amount a TXO URI records is not checked here.
pub fn verify_trail(trail: &Blocktrail, confirmer: &dyn AnchorConfirmer) -> TrailReport {
    let (expected, walk_error) = match walk_blocktrail(trail) {
        Ok(walk) => (Some(walk.expected), None),
        Err(e) => (None, Some(e)),
    };
    let walk_error = if trail.txo.is_empty() {
        Some(walk_error.unwrap_or_else(|| "the trail has no marks".into()))
    } else {
        walk_error
    };

    let marks: Vec<MarkReport> = trail
        .txo
        .iter()
        .enumerate()
        .map(|(i, mark)| {
            let want = expected.as_ref().and_then(|e| e.get(i)).map(hex::encode);
            let key = confirmer.output_key(&mark.txid, mark.vout);
            let found = key.is_some();
            let confirmed = found && confirmer.is_confirmed(&mark.txid, mark.vout);
            let commits = match (&want, &key) {
                (Some(want), Some(key)) => Some(key.eq_ignore_ascii_case(want)),
                _ => None,
            };
            let links_to_prev = (found && i > 0).then(|| {
                let prev = &trail.txo[i - 1];
                confirmer.spends(&mark.txid, &prev.txid, prev.vout)
            });
            MarkReport {
                index: i,
                txid: mark.txid.clone(),
                vout: mark.vout,
                status: mark_status(found, confirmed, commits, links_to_prev),
                confirmed,
                block_height: None,
                value: None,
                amount: mark.amount,
                commits,
                links_to_prev,
                expected_output: want,
            }
        })
        .collect();

    let commitments_checked = walk_error.is_none();
    let verdict = trail_verdict(&marks, commitments_checked);
    TrailReport {
        marks,
        commitments_checked,
        walk_error,
        verdict,
    }
}

/// A mark's status in blocktrails/verify's precedence (with solid-pod-rs's
/// `broken link`); the amount is not checked here (see [`verify_trail`]).
fn mark_status(
    found: bool,
    confirmed: bool,
    commits: Option<bool>,
    links_to_prev: Option<bool>,
) -> MarkStatus {
    if !found {
        MarkStatus::NotFound
    } else if commits == Some(false) {
        MarkStatus::WrongKey
    } else if links_to_prev == Some(false) {
        MarkStatus::BrokenLink
    } else if !confirmed {
        MarkStatus::Unconfirmed
    } else if commits == Some(true) {
        MarkStatus::Verified
    } else {
        MarkStatus::Confirmed
    }
}

/// blocktrails/verify's summary: `allOK = okCount === n && (!walked ||
/// commitOK === n)`, verified when walked, confirmed when not; an empty trail
/// is never more than partial.
fn trail_verdict(marks: &[MarkReport], commitments_checked: bool) -> TrailVerdict {
    let n = marks.len();
    let ok = marks.iter().filter(|m| m.status.is_ok()).count();
    let commit_ok = marks.iter().filter(|m| m.commits == Some(true)).count();
    if n > 0 && ok == n && (!commitments_checked || commit_ok == n) {
        if commitments_checked {
            TrailVerdict::Verified
        } else {
            TrailVerdict::Confirmed
        }
    } else {
        TrailVerdict::Partial
    }
}

/// The outcome of the [`verify`] audit (§2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    /// Step 1 — replayed reducer state hash matched the stored hash.
    pub reducer_replay_ok: bool,
    /// Step 2 — replayed ledger matched the stored ledger.
    pub ledger_replay_ok: bool,
    /// Step 3 — working tree matched the last git-mark commit SHA.
    pub git_clean_ok: bool,
    /// Step 4 — the trail is well formed and its verdict meets the declared
    /// trust level ([`TrustLevel::accepts`]).
    pub trail_ok: bool,
    /// Step 4's evidence: every mark's status and the trail's verdict.
    pub trail: TrailReport,
}

impl VerifyReport {
    /// True iff every gate passed.
    pub fn is_valid(&self) -> bool {
        self.reducer_replay_ok && self.ledger_replay_ok && self.git_clean_ok && self.trail_ok
    }
}

/// Inputs to the [`verify`] audit: the stored artefacts to check the replay
/// against.
pub struct VerifyInput<'a, R: ContractReducer> {
    /// The reducer (pure contract).
    pub reducer: &'a R,
    /// The recorded, ordered event log.
    pub events: &'a [R::Event],
    /// The stored canonical state hash (from the State layer / trail).
    pub stored_state_hash: &'a str,
    /// The stored `ledger.json` (recomputed and compared).
    pub stored_ledger: &'a Ledger,
    /// A pure projection from the replayed reducer state to the expected ledger.
    pub project_ledger: &'a dyn Fn(&R::State) -> Ledger,
    /// The trail being verified.
    pub trail: &'a Blocktrails,
    /// True iff the working tree matches the last git-mark commit SHA.
    pub git_clean: bool,
    /// The declared trust level (gates capability, sets the verdict the
    /// trail must reach).
    pub trust_level: TrustLevel,
    /// What the chain says about the trail's marks.
    pub confirmer: &'a dyn AnchorConfirmer,
}

/// Run the `verify` audit (§2.4): recompute the reducer, replay the ledger,
/// assert git-clean, walk the trail.
///
/// Returns `Err` if the trust level is hard-refused (capability gate). Otherwise
/// a [`VerifyReport`] with one bit per audit step and the trail's report.
pub fn verify<R: ContractReducer>(input: VerifyInput<'_, R>) -> Result<VerifyReport, &'static str> {
    // Capability gate first — a hard-refused trust level never verifies.
    input.trust_level.gate()?;

    // Step 1: recompute the reducer and compare the canonical state hash.
    let reducer_replay_ok = match input.reducer.replay(input.events) {
        Ok(state) => CanonicalState::from_state(&state)
            .map(|cs| cs.matches(input.stored_state_hash))
            .unwrap_or(false),
        Err(_) => false,
    };

    // Step 2: replay the ledger from the (re)computed reducer state.
    let ledger_replay_ok = match input.reducer.replay(input.events) {
        Ok(state) => (input.project_ledger)(&state) == *input.stored_ledger,
        Err(_) => false,
    };

    // Step 3: git-clean assertion (caller-supplied; the substrate's
    // ShellGitMarker computes it from the working tree vs the last mark SHA).
    let git_clean_ok = input.git_clean;

    // Step 4: walk every mark of the trail.
    let trail = verify_trail(input.trail, input.confirmer);
    let trail_ok = input.trail.is_well_formed() && input.trust_level.accepts(trail.verdict);

    Ok(VerifyReport {
        reducer_replay_ok,
        ledger_replay_ok,
        git_clean_ok,
        trail_ok,
        trail,
    })
}

/// The transition-commit capability gate (build-out §3): a hard pre-condition on
/// committing a `transition()` result. Refuses if the trust level is hard-refused.
/// `substrate_disabled` hard-disables money-moving transitions (`.swap`/`.pool`/
/// `.withdraw`/cash-out) until trust-level AND owner+legal sign-off authorise
/// (ADR-124 §7 R5).
pub fn commit_gate(
    trust_level: TrustLevel,
    moves_money: bool,
    substrate_disabled: bool,
) -> Result<(), TransitionError> {
    trust_level.gate().map_err(|m| TransitionError::Rejected {
        code: "trust_level_refused".into(),
        message: m.into(),
    })?;
    if moves_money && substrate_disabled {
        return Err(TransitionError::Rejected {
            code: "substrate_disabled".into(),
            message: "money-moving transitions are disabled until trust-level AND \
                      owner+legal Judgment-Broker sign-off authorise (ADR-124 §7 R5)"
                .into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::web_contract::ledger::LedgerEntry;
    use crate::web_contract::reducer::ContractReducer;
    use serde::Serialize;
    use serde_json::Value;
    use solid_pod_rs::blocktrail::verify_blocktrail;
    use solid_pod_rs::mrc20::{MempoolLookup, TxIn, TxInfo, TxOut, Utxo};
    use solid_pod_rs::payments::PaymentError;

    /// mempool.space testnet4 `GET /api/tx/{txid}` for git-mark b852d7d's
    /// three live marks (vendored from solid-pod-rs 0.5.0-alpha.12).
    const LIVE_TXS: &str = include_str!("../../tests/fixtures/blocktrails/live-trail-txs.json");
    /// blocktrails/verify 043e7af run over the live trail and variants.
    const VECTORS: &str =
        include_str!("../../tests/fixtures/blocktrails/verify-trail-vectors.json");

    #[derive(Clone, Debug, Serialize, PartialEq, Eq)]
    struct PoolState {
        pot_sats: u64,
        owner_did: String,
    }

    #[derive(Clone, Debug, Serialize)]
    struct Stake {
        sats: u64,
    }

    struct Pool {
        owner_did: String,
    }

    impl ContractReducer for Pool {
        type State = PoolState;
        type Event = Stake;
        fn genesis(&self) -> PoolState {
            PoolState {
                pot_sats: 0,
                owner_did: self.owner_did.clone(),
            }
        }
        fn validate(&self, _s: &PoolState) -> Vec<ReducerError> {
            Vec::new()
        }
        fn transition(&self, s: &PoolState, e: &Stake) -> Result<PoolState, TransitionError> {
            Ok(PoolState {
                pot_sats: s.pot_sats + e.sats,
                owner_did: s.owner_did.clone(),
            })
        }
    }

    /// A chain served from captured transactions: the real output keys,
    /// inputs and confirmation status.
    struct Captured(HashMap<String, Value>);

    impl Captured {
        fn from_txs(txs: &Value) -> Self {
            Self(
                txs.as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (t["txid"].as_str().unwrap().to_string(), t.clone()))
                    .collect(),
            )
        }

        fn live() -> Self {
            let file: Value = serde_json::from_str(LIVE_TXS).unwrap();
            Self::from_txs(&file["txs"])
        }

        fn tx_info(t: &Value) -> TxInfo {
            TxInfo {
                txid: t["txid"].as_str().unwrap().to_string(),
                vin: t["vin"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| TxIn {
                        txid: i["txid"].as_str().unwrap_or_default().to_string(),
                        vout: i["vout"].as_u64().unwrap_or_default() as u32,
                    })
                    .collect(),
                vout: t["vout"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| TxOut {
                        value: o["value"].as_u64().unwrap_or_default(),
                        scriptpubkey: o["scriptpubkey"].as_str().map(str::to_string),
                        scriptpubkey_address: o["scriptpubkey_address"]
                            .as_str()
                            .map(str::to_string),
                    })
                    .collect(),
                confirmed: t["status"]["confirmed"].as_bool().unwrap_or(false),
                block_height: t["status"]["block_height"].as_u64(),
            }
        }
    }

    impl AnchorConfirmer for Captured {
        fn is_confirmed(&self, txid: &str, _vout: u32) -> bool {
            self.0
                .get(txid)
                .and_then(|t| t["status"]["confirmed"].as_bool())
                .unwrap_or(false)
        }
        fn output_key(&self, txid: &str, vout: u32) -> Option<String> {
            let spk = self.0.get(txid)?["vout"].get(vout as usize)?["scriptpubkey"].as_str()?;
            Some(match spk.strip_prefix("5120").filter(|k| k.len() == 64) {
                Some(key) => key.to_string(),
                None => spk.to_string(),
            })
        }
        fn spends(&self, txid: &str, prev_txid: &str, prev_vout: u32) -> bool {
            self.0.get(txid).is_some_and(|t| {
                t["vin"].as_array().into_iter().flatten().any(|i| {
                    i["txid"] == prev_txid && i["vout"].as_u64() == Some(u64::from(prev_vout))
                })
            })
        }
    }

    /// The same captured chain as solid-pod-rs's walker reads it.
    #[async_trait::async_trait(?Send)]
    impl MempoolLookup for Captured {
        async fn address_utxos(&self, _address: &str) -> Result<Vec<Utxo>, PaymentError> {
            Ok(vec![])
        }
        async fn tx(&self, txid: &str) -> Result<TxInfo, PaymentError> {
            self.0
                .get(txid)
                .map(Self::tx_info)
                .ok_or_else(|| PaymentError::InvalidState(format!("tx {txid} not found")))
        }
    }

    fn vectors() -> Value {
        serde_json::from_str(VECTORS).unwrap()
    }

    fn live_trail() -> Blocktrails {
        let v = vectors();
        let live = v["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "live-trail")
            .unwrap()
            .clone();
        serde_json::from_value(live["trail"].clone()).unwrap()
    }

    fn project_ledger(s: &PoolState) -> Ledger {
        Ledger::from_balances(vec![LedgerEntry {
            account: s.owner_did.clone(),
            balance_sats: s.pot_sats as i64,
        }])
    }

    fn audit(
        trail: &Blocktrails,
        level: TrustLevel,
        confirmer: &dyn AnchorConfirmer,
    ) -> VerifyReport {
        let pool = Pool {
            owner_did: "did:nostr:aa".into(),
        };
        let events = vec![Stake { sats: 1000 }, Stake { sats: 2000 }];
        let final_state = pool.replay(&events).unwrap();
        let stored_hash = CanonicalState::from_state(&final_state).unwrap().state_hash;
        let stored_ledger = project_ledger(&final_state);
        verify(VerifyInput {
            reducer: &pool,
            events: &events,
            stored_state_hash: &stored_hash,
            stored_ledger: &stored_ledger,
            project_ledger: &project_ledger,
            trail,
            git_clean: true,
            trust_level: level,
            confirmer,
        })
        .unwrap()
    }

    #[test]
    fn the_live_trail_is_verified_at_l0_and_l1() {
        let trail = live_trail();
        let chain = Captured::live();
        for level in [TrustLevel::L0HonestOrCaught, TrustLevel::L1Anchored] {
            let report = audit(&trail, level, &chain);
            assert!(report.is_valid(), "{level:?}: {report:?}");
            assert_eq!(report.trail.verdict, TrailVerdict::Verified);
            assert!(report.trail.is_intact());
            assert!(report
                .trail
                .marks
                .iter()
                .all(|m| m.status == MarkStatus::Verified));
        }
    }

    #[test]
    fn reordered_states_fail_verify_although_the_tip_is_confirmed() {
        let mut trail = live_trail();
        trail.states.swap(1, 2);
        let chain = Captured::live();

        // What the old tip-only check looked at holds: the tip is a confirmed
        // output carrying a real key.
        let tip = trail.tip().unwrap().clone();
        assert!(chain.is_confirmed(&tip.txid, tip.vout));
        assert!(chain.output_key(&tip.txid, tip.vout).is_some());

        for level in [TrustLevel::L0HonestOrCaught, TrustLevel::L1Anchored] {
            let report = audit(&trail, level, &chain);
            assert!(!report.trail_ok, "{level:?}");
            assert!(!report.is_valid());
            assert_eq!(report.trail.verdict, TrailVerdict::Partial);
            assert_eq!(
                report
                    .trail
                    .marks
                    .iter()
                    .map(|m| m.status)
                    .collect::<Vec<_>>(),
                [
                    MarkStatus::Verified,
                    MarkStatus::WrongKey,
                    MarkStatus::WrongKey
                ],
                "refused from the swap on"
            );
        }
    }

    #[test]
    fn no_pubkey_base_is_confirmed_which_meets_l0_but_not_l1() {
        let mut trail = live_trail();
        trail.pubkey_base = None;
        let chain = Captured::live();

        let l0 = audit(&trail, TrustLevel::L0HonestOrCaught, &chain);
        assert_eq!(l0.trail.verdict, TrailVerdict::Confirmed);
        assert!(!l0.trail.commitments_checked);
        assert_eq!(
            l0.trail.walk_error.as_deref(),
            Some("the trail names no base key (pubkeyBase): nothing to recompute from")
        );
        assert!(l0.is_valid());

        let l1 = audit(&trail, TrustLevel::L1Anchored, &chain);
        assert_eq!(l1.trail.verdict, TrailVerdict::Confirmed);
        assert!(!l1.trail_ok);
        assert!(!l1.is_valid());
    }

    #[test]
    fn a_mark_that_does_not_spend_its_predecessor_breaks_the_trail() {
        struct Unlinked(Captured);
        impl AnchorConfirmer for Unlinked {
            fn is_confirmed(&self, txid: &str, vout: u32) -> bool {
                self.0.is_confirmed(txid, vout)
            }
            fn output_key(&self, txid: &str, vout: u32) -> Option<String> {
                self.0.output_key(txid, vout)
            }
            fn spends(&self, _txid: &str, _prev_txid: &str, _prev_vout: u32) -> bool {
                false
            }
        }
        let report = audit(
            &live_trail(),
            TrustLevel::L0HonestOrCaught,
            &Unlinked(Captured::live()),
        );
        assert_eq!(report.trail.verdict, TrailVerdict::Partial);
        assert_eq!(report.trail.marks[1].status, MarkStatus::BrokenLink);
        assert!(!report.is_valid());
    }

    #[test]
    fn an_empty_trail_never_verifies() {
        let trail = Blocktrails::new(
            "tbtc4",
            "0273c7f6cf0f135a63bc95a2e676bcf0a592c8b508fae8697e43f778c74e232b24",
        );
        let report = audit(&trail, TrustLevel::L0HonestOrCaught, &Captured::live());
        assert_eq!(report.trail.verdict, TrailVerdict::Partial);
        assert!(!report.trail_ok);
    }

    /// The host walker against blocktrails/verify 043e7af, case by case, and
    /// against solid-pod-rs's own `verify_blocktrail` on the same chain.
    #[tokio::test]
    async fn the_host_walker_gives_blocktrails_verify_s_verdicts() {
        // Where the host differs, and why (see `verify_trail`):
        //  - broken link: the page labels it verified; solid-pod-rs and the
        //    host say broken link.
        //  - amount mismatch: the confirmer reports no output values, so the
        //    host does not check the recorded amount.
        const BROKEN_LINK: &str = "live-trail-broken-link";
        const AMOUNT: &str = "live-trail-amount-mismatch";

        let v = vectors();
        let cases = v["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 13);
        for c in cases {
            let name = c["name"].as_str().unwrap();
            let trail: Blocktrails = serde_json::from_value(c["trail"].clone()).unwrap();
            assert_eq!(serde_json::to_value(&trail).unwrap(), c["trail"], "{name}");
            let chain = Captured::from_txs(&c["txs"]);

            let host = verify_trail(&trail, &chain);
            let crate_report = verify_blocktrail(&trail, &chain).await;
            let page: Vec<&str> = c["marks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["label"].as_str().unwrap())
                .collect();
            let labels: Vec<&str> = host.marks.iter().map(|m| m.status.as_str()).collect();

            // the walk: the expected outputs, or why there are none
            match c["walk"]["expected"].as_array() {
                Some(want) => {
                    let got: Vec<Value> = host
                        .marks
                        .iter()
                        .map(|m| Value::from(m.expected_output.clone().unwrap()))
                        .collect();
                    assert_eq!(&got, want, "{name}: expected outputs");
                }
                None => assert_eq!(
                    host.walk_error.as_deref(),
                    c["walk"]["error"].as_str(),
                    "{name}: walk error"
                ),
            }
            for (i, m) in c["marks"].as_array().unwrap().iter().enumerate() {
                if !m["linksToPrev"].is_null() {
                    assert_eq!(
                        host.marks[i].links_to_prev.map(Value::Bool),
                        Some(m["linksToPrev"].clone()),
                        "{name}: mark {i} link"
                    );
                }
            }

            match name {
                BROKEN_LINK => {
                    assert_eq!(page, ["verified", "verified", "verified"]);
                    assert_eq!(labels, ["verified", "broken link", "verified"]);
                    assert_eq!(host.verdict, crate_report.verdict);
                    assert_eq!(host.verdict, TrailVerdict::Partial);
                }
                AMOUNT => {
                    assert_eq!(page, ["verified", "amount mismatch", "verified"]);
                    assert_eq!(labels, ["verified", "verified", "verified"]);
                    assert_eq!(host.verdict, TrailVerdict::Verified);
                    assert_eq!(crate_report.verdict, TrailVerdict::Partial);
                }
                _ => {
                    assert_eq!(labels, page, "{name}: labels");
                    assert_eq!(
                        host.verdict.as_str(),
                        c["summary"]["pill"].as_str().unwrap(),
                        "{name}: verdict"
                    );
                    assert_eq!(host.verdict, crate_report.verdict, "{name}");
                    let crate_labels: Vec<&str> = crate_report
                        .marks
                        .iter()
                        .map(|m| m.status.as_str())
                        .collect();
                    assert_eq!(labels, crate_labels, "{name}");
                }
            }
            assert_eq!(
                host.commitments_checked,
                c["walk"]["error"].is_null(),
                "{name}"
            );
        }
    }

    /// git-mark b852d7d's live trail, through the host walker, gets the
    /// verdict blocktrails/verify gives it, with git-mark's own expected
    /// outputs.
    #[test]
    fn git_mark_s_live_trail_through_the_host_walker() {
        let v = vectors();
        let gm = &v["gitMark"]["liveVerify"];
        assert_eq!(gm["valid"], true);
        let live = v["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "live-trail")
            .unwrap();

        let report = verify_trail(&live_trail(), &Captured::live());
        assert_eq!(report.verdict.as_str(), live["summary"]["pill"]);
        assert_eq!(report.verdict, TrailVerdict::Verified);
        let got: Vec<Value> = report
            .marks
            .iter()
            .map(|m| Value::from(m.expected_output.clone().unwrap()))
            .collect();
        assert_eq!(&got, gm["expected"].as_array().unwrap());
    }

    #[test]
    fn tampered_state_hash_fails_reducer_replay() {
        let pool = Pool {
            owner_did: "did:nostr:aa".into(),
        };
        let events = vec![Stake { sats: 1000 }];
        let final_state = pool.replay(&events).unwrap();
        let stored_ledger = project_ledger(&final_state);
        let trail = live_trail();
        let confirmer = Captured::live();

        let report = verify(VerifyInput {
            reducer: &pool,
            events: &events,
            stored_state_hash: "deadbeef", // wrong
            stored_ledger: &stored_ledger,
            project_ledger: &project_ledger,
            trail: &trail,
            git_clean: true,
            trust_level: TrustLevel::L0HonestOrCaught,
            confirmer: &confirmer,
        })
        .unwrap();

        assert!(!report.reducer_replay_ok);
        assert!(report.trail_ok);
        assert!(!report.is_valid());
    }

    #[test]
    fn trust_levels_serialise_as_named() {
        assert_eq!(
            serde_json::to_value(TrustLevel::L1Anchored).unwrap(),
            "l1_anchored"
        );
        assert_eq!(
            serde_json::to_value(TrustLevel::L0HonestOrCaught).unwrap(),
            "l0_honest_or_caught"
        );
    }

    #[test]
    fn trustless_levels_are_hard_refused() {
        assert!(TrustLevel::L0HonestOrCaught.is_available());
        assert!(TrustLevel::L1Anchored.is_available());
        assert!(!TrustLevel::L2AdaptorSigCet.is_available());
        assert!(!TrustLevel::L3Rgb.is_available());

        // commit_gate refuses an L2 transition outright.
        let err = commit_gate(TrustLevel::L2AdaptorSigCet, false, false).unwrap_err();
        assert!(matches!(err, TransitionError::Rejected { .. }));

        // and verify refuses to run at one
        let pool = Pool {
            owner_did: "did:nostr:aa".into(),
        };
        let ledger = Ledger::from_balances(vec![]);
        let trail = live_trail();
        let refused = verify(VerifyInput {
            reducer: &pool,
            events: &[],
            stored_state_hash: "",
            stored_ledger: &ledger,
            project_ledger: &project_ledger,
            trail: &trail,
            git_clean: true,
            trust_level: TrustLevel::L3Rgb,
            confirmer: &Captured::live(),
        });
        assert!(refused.is_err());
    }

    #[test]
    fn substrate_disablement_blocks_money_moves() {
        // L0, but money-moving with the substrate disabled → refused.
        let err = commit_gate(TrustLevel::L0HonestOrCaught, true, true).unwrap_err();
        match err {
            TransitionError::Rejected { code, .. } => assert_eq!(code, "substrate_disabled"),
            _ => panic!("expected Rejected"),
        }
        // Non-money transition is allowed even when disabled.
        assert!(commit_gate(TrustLevel::L0HonestOrCaught, false, true).is_ok());
        // Money move allowed when substrate enabled.
        assert!(commit_gate(TrustLevel::L0HonestOrCaught, true, false).is_ok());
    }

    #[test]
    fn three_gate_checks_agree_when_deterministic() {
        let pool = Pool {
            owner_did: "did:nostr:aa".into(),
        };
        let findings = Checks::run_all(&pool, &pool.genesis()).unwrap();
        assert!(findings.is_empty());
    }
}
