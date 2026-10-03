//! Layer 4 — **Trail**: the `gitmark.json` + `blocktrails.json` envelopes.
//!
//! ADR-124 build-out (`docs/adr/ADR-128-build-out-canonical-gitmark-blocktrails.md`),
//! aligned with Blocktrails by ADR-2111 (D2 amendment, 2026-10-03). This is
//! `src/web_contract`, the ADR-124 trust ladder; it is not
//! `crates/visionclaw-contracts`, the envelope-schema crate (ADR-2111 D9).
//!
//! ## `gitmark.json` is VERBATIM (C7, ground-truth-verified)
//!
//! The Carvalho-lineage ground truth is `microfed/gitmark.json`:
//!
//! ```json
//! { "@id": "gitmark:<sha>:<vout>", "genesis": "gitmark:<sha>:<vout>",
//!   "nick": "<name>", "package": "<path>", "repository": "./" }
//! ```
//!
//! It has **exactly five keys** — `@id`, `genesis`, `nick`, `package`,
//! `repository`. It does **NOT** carry `@context`, `@type`, `commit`, or
//! `parent`. Earlier drafts invented those four and omitted `repository`; the
//! corrected envelope here emits only the five ground-truth keys (ADR-124 §2.1,
//! finding C7). Parent linkage lives in the trail's `states` and `txo`, where
//! it belongs — never on the git-mark.
//!
//! ## `blocktrails.json` is the gitmark profile §5.2
//!
//! [`Blocktrails`] is solid-pod-rs's shared [`Blocktrail`], the shape
//! blocktrails/spec's gitmark profile §5.2 gives and blocktrails/git-mark
//! b852d7d `trail()` writes:
//!
//! ```json
//! { "@type": "Blocktrail", "version": "0.0.3", "profile": "gitmark",
//!   "pubkeyBase": "02…", "chain": "tbtc4",
//!   "states": ["<40-hex commit>", "…"],
//!   "txo": ["txo:tbtc4:<txid>:<vout>?amount=<sats>&commit=<40-hex commit>", "…"] }
//! ```
//!
//! Each `txo` entry is a TXO URI ([`BlocktrailTxo`]); each state is the commit
//! it marks, hashed as its text. Every mark's output key is `x(P_i)`, the base
//! key tweaked by every state so far, so a verifier recomputes each one from
//! `pubkeyBase` and `states` (see [`super::ritual::verify_trail`]).
//!
//! Files this module wrote before the alignment, with `txo` entries
//! `{txid, vout, address}`, still load: each entry becomes the TXO URI of the
//! same outpoint on the trail's chain, its commit taken from the matching
//! state. The address is dropped, since it is derived from the recomputed key.
//! Such a file is written back in the §5.2 form.
//!
//! ## Invariant boundary (I1–I4 hold trivially)
//!
//! This layer is identity-rail-agnostic. The agent attribution it carries is the
//! ADR-125 `did:nostr:<hex>` string, treated as an opaque identifier — it is
//! never parsed for a verification method, never re-encoded, never read on an
//! auth path. No key bytes are touched. ADR-074 §D1 stays.

use std::ops::{Deref, DerefMut};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub use solid_pod_rs::blocktrail::{Blocktrail, BlocktrailTxo};
use solid_pod_rs::blocktrail::{BLOCKTRAIL_TYPE, BLOCKTRAIL_VERSION, GITMARK_PROFILE};

/// The `gitmark:<sha>:<vout>` identifier: a mark's genesis or commit point.
///
/// Rendered as the `@id` of [`GitMark`] and reused as `genesis` for the first
/// mark in a chain (a genesis mark's `genesis == @id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitMarkId {
    /// The anchoring commit SHA (40-char lowercase hex git object id).
    pub commit_sha: String,
    /// The transaction output index this mark anchors at.
    pub vout: u32,
}

impl GitMarkId {
    /// Construct from a commit SHA and a `vout`.
    pub fn new(commit_sha: impl Into<String>, vout: u32) -> Self {
        Self {
            commit_sha: commit_sha.into(),
            vout,
        }
    }

    /// Render the `gitmark:<commit_sha>:<vout>` string used as `@id`/`genesis`.
    pub fn to_at_id(&self) -> String {
        format!("gitmark:{}:{}", self.commit_sha, self.vout)
    }
}

/// The **verbatim** `gitmark.json` envelope — exactly five keys (C7).
///
/// `@id`/`genesis` serialise to `gitmark:<sha>:<vout>` strings; `genesis` for a
/// genesis mark equals its own `@id`. The serializer source is the existing
/// VisionClaw provenance substrate (`agent_events::provenance`) projected over a
/// captured commit SHA + anchoring `vout`; `nick`/`package`/`repository` are the
/// additive projection fields.
///
/// **Do NOT add `@context`/`@type`/`commit`/`parent`** — they are not in the
/// ground-truth file and adding them breaks byte-parity with create-agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitMark {
    /// `@id` — `gitmark:<commit_sha>:<vout>`.
    #[serde(rename = "@id")]
    pub at_id: String,
    /// `genesis` — `gitmark:<first-commit-sha>:<vout>`. Equals `@id` for a
    /// genesis mark.
    pub genesis: String,
    /// `nick` — short human name for the contract package.
    pub nick: String,
    /// `package` — pod-relative package path (e.g. `./package.json`).
    pub package: String,
    /// `repository` — repo-relative root (e.g. `"./"`).
    pub repository: String,
}

impl GitMark {
    /// Build a genesis git-mark (where `genesis == @id`).
    pub fn genesis(
        id: &GitMarkId,
        nick: impl Into<String>,
        package: impl Into<String>,
        repository: impl Into<String>,
    ) -> Self {
        let at_id = id.to_at_id();
        Self {
            genesis: at_id.clone(),
            at_id,
            nick: nick.into(),
            package: package.into(),
            repository: repository.into(),
        }
    }

    /// Build a non-genesis git-mark that points back to an existing `genesis`
    /// `@id` (parent linkage lives in the trail's `states[]`, not here).
    pub fn marked(
        id: &GitMarkId,
        genesis: impl Into<String>,
        nick: impl Into<String>,
        package: impl Into<String>,
        repository: impl Into<String>,
    ) -> Self {
        Self {
            at_id: id.to_at_id(),
            genesis: genesis.into(),
            nick: nick.into(),
            package: package.into(),
            repository: repository.into(),
        }
    }
}

/// A `blocktrails.json` trail: solid-pod-rs's [`Blocktrail`] in the gitmark
/// profile §5.2 shape, reading this module's earlier files as well.
///
/// It dereferences to [`Blocktrail`], so `version`, `pubkey_base`, `chain`,
/// `states` and `txo` are read and written as that type's fields. Serialising
/// writes exactly what [`Blocktrail`] writes.
///
/// # Examples
///
/// ```
/// use visionclaw_server::web_contract::Blocktrails;
///
/// let base = "0273c7f6cf0f135a63bc95a2e676bcf0a592c8b508fae8697e43f778c74e232b24";
/// let mut trail = Blocktrails::new("tbtc4", base);
/// trail.push_link(
///     "9adc596cfd1100333393a12f2f41b2d820f16d0b",
///     "51d87101b7cbb01cc5a68785bf3141ec6fd00894d71ab1168d4daa20420eeacf",
///     0,
///     999_700,
/// );
/// let json: serde_json::Value = serde_json::to_value(&trail).unwrap();
/// assert_eq!(json["version"], "0.0.3");
/// assert_eq!(
///     json["txo"][0],
///     "txo:tbtc4:51d87101b7cbb01cc5a68785bf3141ec6fd00894d71ab1168d4daa20420eeacf:0\
///      ?amount=999700&commit=9adc596cfd1100333393a12f2f41b2d820f16d0b"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Blocktrails(Blocktrail);

impl Blocktrails {
    /// The `@type` every trail carries.
    pub const AT_TYPE: &'static str = BLOCKTRAIL_TYPE;
    /// The profile this layer writes: each state is a git commit.
    pub const PROFILE: &'static str = GITMARK_PROFILE;
    /// The schema `version` this layer writes (git-mark b852d7d).
    pub const VERSION: &'static str = BLOCKTRAIL_VERSION;

    /// An empty git-mark trail on `chain` (a TXO URI chain token such as
    /// `tbtc4`) over `pubkey_base`, the base key as a full compressed point.
    pub fn new(chain: impl Into<String>, pubkey_base: impl Into<String>) -> Self {
        Self(Blocktrail::gitmark(
            pubkey_base,
            chain,
            Vec::new(),
            Vec::new(),
        ))
    }

    /// Append one mark: the commit it commits to, and the output at
    /// `txid:vout` on the trail's chain carrying `amount` sats. The commit is
    /// both the new state and the TXO URI's `commit`.
    pub fn push_link(
        &mut self,
        commit: impl Into<String>,
        txid: impl Into<String>,
        vout: u32,
        amount: u64,
    ) {
        let commit = commit.into();
        let chain = self.0.chain.clone().unwrap_or_default();
        self.0.states.push(Value::String(commit.clone()));
        self.0.txo.push(
            BlocktrailTxo::new(chain, txid, vout)
                .with_amount(amount)
                .with_commit(commit),
        );
    }

    /// The newest mark, if any.
    pub fn tip(&self) -> Option<&BlocktrailTxo> {
        self.0.txo.last()
    }

    /// True iff every mark has a state (one state per mark, none null). For a
    /// git-mark trail that lists no states the commits in its TXO URIs count,
    /// as blocktrails/verify reads them.
    pub fn is_well_formed(&self) -> bool {
        self.0.state_strings().is_ok()
    }

    /// The shared trail type.
    pub fn as_blocktrail(&self) -> &Blocktrail {
        &self.0
    }

    /// Unwrap into the shared trail type.
    pub fn into_blocktrail(self) -> Blocktrail {
        self.0
    }
}

impl From<Blocktrail> for Blocktrails {
    fn from(trail: Blocktrail) -> Self {
        Self(trail)
    }
}

impl Deref for Blocktrails {
    type Target = Blocktrail;

    fn deref(&self) -> &Blocktrail {
        &self.0
    }
}

impl DerefMut for Blocktrails {
    fn deref_mut(&mut self) -> &mut Blocktrail {
        &mut self.0
    }
}

impl<'de> Deserialize<'de> for Blocktrails {
    /// Reads the §5.2 shape and solid-pod-rs's earlier `{outpoint,
    /// blockheight}` entries as [`Blocktrail`] does, and this module's
    /// earlier `{txid, vout, address}` entries as the TXO URI of that
    /// outpoint on the trail's chain, committing to the matching state.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut doc = Value::deserialize(d)?;
        let Some(obj) = doc.as_object_mut() else {
            return Err(D::Error::custom("blocktrails.json is a JSON object"));
        };
        let chain = obj.get("chain").and_then(Value::as_str).map(str::to_owned);
        let states: Vec<Option<String>> = obj
            .get("states")
            .and_then(Value::as_array)
            .map(|s| s.iter().map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let entries = match obj.remove("txo") {
            Some(Value::Array(entries)) => entries,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "txo is a list of marks, not {other}"
                )))
            }
            None => Vec::new(),
        };
        let mut trail: Blocktrail = serde_json::from_value(doc).map_err(D::Error::custom)?;
        trail.txo = entries
            .into_iter()
            .enumerate()
            .map(|(i, entry)| match host_legacy_txo(&entry) {
                Some((txid, vout)) => {
                    let chain = chain.clone().ok_or_else(|| {
                        D::Error::custom("a {txid, vout, address} mark needs the trail's chain")
                    })?;
                    let txo = BlocktrailTxo::new(chain, txid, vout);
                    Ok(match states.get(i).cloned().flatten() {
                        Some(commit) => txo.with_commit(commit),
                        None => txo,
                    })
                }
                None => serde_json::from_value(entry).map_err(D::Error::custom),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self(trail))
    }
}

/// `(txid, vout)` of a mark in this module's earlier `{txid, vout, address}`
/// form, or `None` for any other entry.
fn host_legacy_txo(entry: &Value) -> Option<(String, u32)> {
    let obj = entry.as_object()?;
    let txid = obj.get("txid")?.as_str()?;
    let vout = u32::try_from(obj.get("vout")?.as_u64()?).ok()?;
    Some((txid.to_owned(), vout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const SHA0: &str = "09689e988a2630e6904e6f53ddd6e1ab2f823b77ab0b160b4f98442cedb3e68c";

    /// git-mark b852d7d's live trail: base key and the first three marks.
    const BASE: &str = "0273c7f6cf0f135a63bc95a2e676bcf0a592c8b508fae8697e43f778c74e232b24";
    const COMMITS: [&str; 3] = [
        "9adc596cfd1100333393a12f2f41b2d820f16d0b",
        "4490c4c39e145915c59c0964b6dcd8dc720c9d2e",
        "699ee3a3ea9332cc9ec435acf8fd8cd07eecf940",
    ];
    const TXIDS: [&str; 3] = [
        "51d87101b7cbb01cc5a68785bf3141ec6fd00894d71ab1168d4daa20420eeacf",
        "0ffaa7d29beebdf5207cd8ee7ea11eb74e38864c861ad7a33302b7dabbf37a08",
        "3d90109823521b006f45f15640410abe52a4dce79820567b9b7045aa035bf4f0",
    ];
    const AMOUNTS: [u64; 3] = [999_700, 999_400, 999_100];

    const VECTORS: &str =
        include_str!("../../tests/fixtures/blocktrails/verify-trail-vectors.json");

    fn live_trail() -> Blocktrails {
        let mut trail = Blocktrails::new("tbtc4", BASE);
        for i in 0..3 {
            trail.push_link(COMMITS[i], TXIDS[i], 0, AMOUNTS[i]);
        }
        trail
    }

    #[test]
    fn gitmark_genesis_emits_exactly_five_ground_truth_keys() {
        // C7: the verbatim envelope is { @id, genesis, nick, package, repository }
        // — NO @context / @type / commit / parent.
        let id = GitMarkId::new(SHA0, 0);
        let mark = GitMark::genesis(&id, "gitmark", "./package.json", "./");
        let v: Value = serde_json::to_value(&mark).unwrap();
        let obj = v.as_object().unwrap();

        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["@id", "genesis", "nick", "package", "repository"]);

        // Genesis: @id == genesis.
        assert_eq!(obj["@id"], format!("gitmark:{SHA0}:0"));
        assert_eq!(obj["genesis"], format!("gitmark:{SHA0}:0"));
        assert_eq!(obj["repository"], "./");

        // The four invented fields MUST be absent.
        for forbidden in ["@context", "@type", "commit", "parent"] {
            assert!(
                !obj.contains_key(forbidden),
                "forbidden key {forbidden} present"
            );
        }
    }

    #[test]
    fn gitmark_byte_matches_carvalho_ground_truth() {
        // Reproduce microfed/gitmark.json exactly (key set + values), proving the
        // verbatim claim is honoured.
        let id = GitMarkId::new(SHA0, 0);
        let mark = GitMark::genesis(&id, "gitmark", "./package.json", "./");
        let v: Value = serde_json::to_value(&mark).unwrap();
        let ground_truth: Value = serde_json::json!({
            "@id": format!("gitmark:{SHA0}:0"),
            "genesis": format!("gitmark:{SHA0}:0"),
            "nick": "gitmark",
            "package": "./package.json",
            "repository": "./"
        });
        assert_eq!(v, ground_truth);
    }

    #[test]
    fn a_built_trail_is_the_gitmark_profile_5_2_shape() {
        // blocktrails/verify 043e7af's "live-trail" case is git-mark b852d7d's
        // trail() output for the same marks: the same document, key for key.
        let vectors: Value = serde_json::from_str(VECTORS).unwrap();
        let live = vectors["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "live-trail")
            .unwrap();
        let trail = live_trail();
        assert_eq!(serde_json::to_value(&trail).unwrap(), live["trail"]);

        let text = serde_json::to_string(&trail).unwrap();
        let keys: Vec<&str> = [
            "@type",
            "version",
            "profile",
            "pubkeyBase",
            "chain",
            "states",
            "txo",
        ]
        .into_iter()
        .collect();
        let positions: Vec<usize> = keys
            .iter()
            .map(|k| text.find(&format!("\"{k}\"")).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "§5.2 key order");

        assert!(trail.is_well_formed());
        assert_eq!(trail.type_, Blocktrails::AT_TYPE);
        assert_eq!(trail.version.as_deref(), Some(Blocktrails::VERSION));
        assert_eq!(trail.tip().unwrap().txid, TXIDS[2]);
        for (i, txo) in trail.txo.iter().enumerate() {
            assert_eq!(txo.commit.as_deref(), Some(COMMITS[i]));
            assert_eq!(txo.commit.as_deref().map(str::len), Some(40));
        }
    }

    #[test]
    fn a_new_trail_round_trips_through_its_json() {
        let trail = live_trail();
        let back: Blocktrails =
            serde_json::from_str(&serde_json::to_string(&trail).unwrap()).unwrap();
        assert_eq!(back, trail);
    }

    #[test]
    fn an_earlier_txid_vout_address_file_still_loads() {
        // The shape this module wrote before the alignment.
        let old = serde_json::json!({
            "@type": "Blocktrail",
            "profile": "gitmark",
            "chain": "tbtc4",
            "pubkeyBase": BASE,
            "states": COMMITS,
            "txo": [
                {"txid": TXIDS[0], "vout": 0, "address": "tb1puspauu7f0jmu5tham2uzxjf60w2fpp0yqzr6e9l7dzzdkykvwl0sdxlkml"},
                {"txid": TXIDS[1], "vout": 0, "address": "tb1p…"},
                {"txid": TXIDS[2], "vout": 0, "address": "tb1p…"}
            ]
        });
        let trail: Blocktrails = serde_json::from_value(old).unwrap();
        assert!(trail.is_well_formed());
        assert_eq!(trail.version, None, "an old file names no version");
        assert_eq!(trail.tip().unwrap().txid, TXIDS[2]);
        // Written back as TXO URIs committing to the matching state.
        let json = serde_json::to_value(&trail).unwrap();
        assert_eq!(
            json["txo"][1],
            format!("txo:tbtc4:{}:0?commit={}", TXIDS[1], COMMITS[1])
        );
        // ... and that is the §5.2 form, which reads back unchanged.
        let again: Blocktrails = serde_json::from_value(json).unwrap();
        assert_eq!(again, trail);
    }

    #[test]
    fn solid_pod_rs_earlier_outpoint_entries_still_load_unchanged() {
        let old = serde_json::json!({
            "@id": format!("gitmark:{}:0", COMMITS[0]),
            "@type": "Blocktrail",
            "profile": "gitmark",
            "genesis": format!("gitmark:{}:0", COMMITS[0]),
            "states": [COMMITS[0]],
            "txo": [{"outpoint": format!("{}:0", TXIDS[0]), "blockheight": 139313}]
        });
        let trail: Blocktrails = serde_json::from_value(old.clone()).unwrap();
        assert!(trail.is_legacy());
        assert_eq!(serde_json::to_value(&trail).unwrap(), old);
    }

    #[test]
    fn a_mark_without_a_state_is_not_well_formed() {
        let mut trail = Blocktrails::new("tbtc4", BASE);
        trail.states.push(Value::String(COMMITS[0].into())); // a state with no mark
        assert!(!trail.is_well_formed());

        let mut trail = live_trail();
        trail.states.pop(); // a mark with no state
        assert!(!trail.is_well_formed());
    }

    #[test]
    fn an_old_mark_on_a_trail_with_no_chain_is_refused() {
        let old = serde_json::json!({
            "@type": "Blocktrail",
            "states": [COMMITS[0]],
            "txo": [{"txid": TXIDS[0], "vout": 0, "address": "tb1p…"}]
        });
        assert!(serde_json::from_value::<Blocktrails>(old).is_err());
    }
}
