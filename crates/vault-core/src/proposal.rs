//! [`PatchProposal`] — contract C4, the payload of a forum 31402
//! `ActionRequest`.
//!
//! The invariant the whole governance loop rests on: **a proposal with a
//! non-empty `blockers` list is never posted.** `vault propose` runs Whelk and
//! the conflict detector first, collects everything they find, and emits the
//! proposal with its blockers so the refusal is auditable — but it does not
//! publish it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::promotion::Blocker;

/// How far-reaching a proposal is. Schema-level changes are floored at risk
/// tier High by the panel operator (decision Q7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// A change to one page's content.
    Content,
    /// A change to the vocabulary or to the taxonomy's shape.
    Schema,
    /// A withdrawal.
    Demotion,
}

impl Level {
    /// The tag value used on the 31402 `level` tag.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Schema => "schema",
            Self::Demotion => "demotion",
        }
    }

    /// `true` when the panel must floor this proposal at risk tier High.
    #[must_use]
    pub fn floors_tier_high(self) -> bool {
        matches!(self, Self::Schema)
    }
}

impl std::str::FromStr for Level {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "content" => Ok(Self::Content),
            "schema" => Ok(Self::Schema),
            "demotion" => Ok(Self::Demotion),
            other => Err(format!(
                "unknown level {other:?}; expected content, schema or demotion"
            )),
        }
    }
}

/// The C4 JSON document.
///
/// Field order is the contract's order, and the struct is serialised with
/// `preserve_order` maps nowhere in sight, so the JSON is stable across runs —
/// which matters because [`PatchProposal::digest`] hashes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchProposal {
    /// Content, schema or demotion.
    pub level: Level,
    /// The subject's stable IRI.
    pub iri: String,
    /// The subject's page id.
    pub page: String,
    /// What the proposer believes and why. Free text, shown to the human.
    pub hypothesis: String,
    /// A unified diff of the page's frontmatter.
    pub diff: String,
    /// `sha256:<hex>` over level, iri, page, hypothesis and diff.
    pub digest: String,
    /// Automatic refusals. Non-empty means the proposal is not posted.
    pub blockers: Vec<Blocker>,
    /// The proposing actor, e.g. `process:vault/1.0`.
    pub proposer: String,
    /// The build generation the proposal was computed against.
    pub generation: String,
    /// ISO-8601 instant after which the proposal reverts to draft.
    pub stale_after: String,
    /// Every page this proposal changes, sorted, including the subject.
    ///
    /// A single-page proposal holds exactly `[page]`. A **grouped** proposal
    /// holds all of them, and `diff` is then a multi-file unified diff.
    ///
    /// Grouping exists because the two largest decisions in the corpus are not
    /// expressible one page at a time: folding `domain: ai` into
    /// `artificial-intelligence` touches 513 pages and the journal base-copy
    /// choice touches 125. Filing those as 513 and 125 separate content
    /// proposals would put a *schema*-shaped decision through the content tier
    /// 638 times, and a human would be asked to sign the same judgement over and
    /// over with no way to accept or reject it as one thing.
    #[serde(default)]
    pub pages: Vec<String>,
}

/// How long a proposal stays live before it expires (decision Q7).
pub const STALE_AFTER_DAYS: i64 = 14;

impl PatchProposal {
    /// Assemble a proposal and compute its digest.
    ///
    /// `stale_after` is supplied by the caller rather than read from the clock
    /// so the whole type stays deterministic and testable.
    ///
    /// The nine parameters are the nine fields of contract C4 minus the two
    /// this constructor derives; collapsing them into a builder would hide the
    /// contract rather than express it.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        level: Level,
        iri: impl Into<String>,
        page: impl Into<String>,
        hypothesis: impl Into<String>,
        diff: impl Into<String>,
        blockers: Vec<Blocker>,
        proposer: impl Into<String>,
        generation: impl Into<String>,
        stale_after: impl Into<String>,
    ) -> Self {
        let (iri, page) = (iri.into(), page.into());
        let (hypothesis, diff) = (hypothesis.into(), diff.into());
        let digest = Self::compute_digest(level, &iri, &page, &hypothesis, &diff);
        Self {
            level,
            iri,
            pages: vec![page.clone()],
            page,
            hypothesis,
            diff,
            digest,
            blockers,
            proposer: proposer.into(),
            generation: generation.into(),
            stale_after: stale_after.into(),
        }
    }

    /// Declare the full page set of a **grouped** proposal, recomputing the
    /// digest.
    ///
    /// The subject is kept at the head of [`PatchProposal::page`] — it is what
    /// names the case — and `pages` becomes the whole set, sorted and
    /// deduplicated.
    #[must_use]
    pub fn with_pages(mut self, pages: impl IntoIterator<Item = String>) -> Self {
        let mut pages: Vec<String> = pages.into_iter().collect();
        pages.sort();
        pages.dedup();
        if !pages.contains(&self.page) {
            pages.push(self.page.clone());
            pages.sort();
        }
        self.pages = pages;
        self.digest = Self::compute_digest_grouped(
            self.level,
            &self.iri,
            &self.page,
            &self.hypothesis,
            &self.diff,
            &self.pages,
        );
        self
    }

    /// `true` when the proposal changes more than its subject.
    #[must_use]
    pub fn is_grouped(&self) -> bool {
        self.pages.len() > 1
    }

    /// `sha256:<hex>` over the semantic fields, newline-separated.
    ///
    /// `blockers`, `proposer`, `generation` and `stale_after` are deliberately
    /// *outside* the digest: the same proposed change must hash identically
    /// whoever proposes it and whenever, so a re-proposal after a blocker is
    /// cleared lands on the same case id.
    #[must_use]
    pub fn compute_digest(
        level: Level,
        iri: &str,
        page: &str,
        hypothesis: &str,
        diff: &str,
    ) -> String {
        Self::compute_digest_grouped(level, iri, page, hypothesis, diff, &[])
    }

    /// The digest, including a **grouped** proposal's page set.
    ///
    /// The set is folded in only when it names more than the subject, so a
    /// single-page proposal's digest — and therefore its 31402 `d` tag and its
    /// case id — is exactly what it was before grouping existed. A grouped
    /// proposal must hash differently from a single-page one over the same
    /// subject, or two genuinely different decisions would share a case.
    #[must_use]
    pub fn compute_digest_grouped(
        level: Level,
        iri: &str,
        page: &str,
        hypothesis: &str,
        diff: &str,
        pages: &[String],
    ) -> String {
        let mut h = Sha256::new();
        for part in [level.as_str(), iri, page, hypothesis, diff] {
            h.update(part.as_bytes());
            h.update(b"\n");
        }
        if pages.len() > 1 {
            for p in pages {
                h.update(p.as_bytes());
                h.update(b"\n");
            }
        }
        format!("sha256:{}", hex::encode(h.finalize()))
    }

    /// `true` when nothing blocks the proposal and it may be posted.
    #[must_use]
    pub fn is_postable(&self) -> bool {
        self.blockers.is_empty()
    }

    /// The bare hex digest, for use as the 31402 `d` tag.
    #[must_use]
    pub fn digest_hex(&self) -> &str {
        self.digest.strip_prefix("sha256:").unwrap_or(&self.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(blockers: Vec<Blocker>) -> PatchProposal {
        PatchProposal::new(
            Level::Content,
            "urn:ngm:class:knowledge-graph",
            "Knowledge Graph",
            "quality is understated",
            "-quality: 0.35\n+quality: 0.55\n",
            blockers,
            "process:vault/1.0",
            "visionGraph@abc1234",
            "2026-10-06T00:00:00Z",
        )
    }

    #[test]
    fn a_clean_proposal_is_postable() {
        assert!(proposal(vec![]).is_postable());
    }

    #[test]
    fn any_blocker_stops_the_post() {
        let p = proposal(vec![Blocker::new("WHELK_INCONSISTENT", "owl:Nothing")]);
        assert!(!p.is_postable());
    }

    #[test]
    fn the_digest_ignores_proposer_and_expiry() {
        let a = proposal(vec![]);
        let mut b = proposal(vec![Blocker::new("X", "y")]);
        b.proposer = "agent:other/9".into();
        b.stale_after = "2099-01-01T00:00:00Z".into();
        assert_eq!(a.digest, b.digest);
    }

    #[test]
    fn the_digest_tracks_the_diff() {
        let a = proposal(vec![]);
        let b = PatchProposal::new(
            Level::Content,
            "urn:ngm:class:knowledge-graph",
            "Knowledge Graph",
            "quality is understated",
            "-quality: 0.35\n+quality: 0.65\n",
            vec![],
            "process:vault/1.0",
            "visionGraph@abc1234",
            "2026-10-06T00:00:00Z",
        );
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn digest_hex_strips_the_algorithm_prefix() {
        let p = proposal(vec![]);
        assert_eq!(p.digest_hex().len(), 64);
        assert!(!p.digest_hex().contains(':'));
    }

    #[test]
    fn schema_floors_the_tier() {
        assert!(Level::Schema.floors_tier_high());
        assert!(!Level::Content.floors_tier_high());
    }

    #[test]
    fn the_c4_json_shape_is_stable() {
        let json = serde_json::to_value(proposal(vec![])).unwrap();
        for key in [
            "level",
            "iri",
            "page",
            "hypothesis",
            "diff",
            "digest",
            "blockers",
            "proposer",
            "generation",
            "stale_after",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert_eq!(json["level"], "content");
    }
}
