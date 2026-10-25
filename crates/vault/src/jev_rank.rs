//! `vault rank` — VC-20-S1-01 and VG-01-S1-01 (one implementation serves both),
//! the opt-in Jev option for ontology patch
//! proposals. Compiled only with the `jev` cargo feature.
//!
//! Reads `PatchProposal` JSON files, asks Jev (when `VISIONCLAW_JEV_PATCH_RANK=1`
//! and an endpoint resolves) how well each proposal's own evidence supports
//! its hypothesis, and prints a reading order. The proposals themselves are
//! never modified, posted, signed or approved; formal OWL reasoning, the
//! derivation certificates in `consequences` and human acceptance remain the
//! authority. With Jev unavailable the input order is printed unchanged.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use vault_core::proposal::PatchProposal;
use visionclaw_jev::patch_rank::{self, Assessment, PatchFacts};
use visionclaw_jev::{Item, Jev};

/// The facts about a proposal Jev may see: its own text, diff, consequence
/// delta and the blockers code already found.
pub fn facts_of(p: &PatchProposal) -> PatchFacts {
    PatchFacts {
        kind: format!("{:?}", p.kind),
        hypothesis: p.hypothesis.clone(),
        diff: p.diff.clone(),
        consequences: p.consequences.as_ref().and_then(|c| serde_json::to_string(c).ok()),
        blockers: p.blockers.iter().map(ToString::to_string).collect(),
    }
}

/// Order `(path, proposal)` pairs best-supported first, or unchanged, and
/// return each one's assessment alongside.
pub async fn rank(
    jev: Option<&Jev>,
    items: Vec<(PathBuf, PatchProposal)>,
    params: &patch_rank::Params,
) -> (Vec<(PathBuf, PatchProposal)>, Vec<Option<Assessment>>) {
    patch_rank::rank_proposals(jev, items, |(_, p)| facts_of(p), params).await
}

fn read(path: &Path) -> anyhow::Result<PatchProposal> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not a PatchProposal", path.display()))
}

/// The `vault rank` entry point.
///
/// # Errors
///
/// A file that cannot be read or is not a `PatchProposal`, or a runtime that
/// cannot start. Jev being unavailable is not an error.
pub fn run(files: &[PathBuf], json: bool) -> anyhow::Result<ExitCode> {
    let items = files
        .iter()
        .map(|f| read(f).map(|p| (f.clone(), p)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let jev = Jev::from_env_logged(Item::PatchRank, patch_rank::QSET_VERSION);
    let params = patch_rank::Params::from_lookup(&|k| std::env::var(k).ok());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (ordered, ann) = rt.block_on(rank(jev.as_ref(), items, &params));
    if json {
        let rows: Vec<_> = ordered
            .iter()
            .zip(&ann)
            .map(|((path, p), a)| serde_json::json!({"file": path, "iri": p.iri, "jev": a}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        if jev.is_none() {
            eprintln!("jev: unavailable or off; input order kept");
        }
        for ((path, p), a) in ordered.iter().zip(&ann) {
            match a {
                Some(a) => println!(
                    "{:.2}  {}  {}{}{}",
                    a.priority,
                    path.display(),
                    p.iri,
                    if a.contradiction_flag { "  [possible contradiction]" } else { "" },
                    if a.unsupported_flag { "  [unsupported claims]" } else { "" }
                ),
                None => println!("  --  {}  {}", path.display(), p.iri),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::proposal::{Level, ProposalKind};
    use visionclaw_jev::client::testing::{jev as test_jev, FnTransport, MockTransport, Script};
    use visionclaw_jev::client::{RawResponse, TransportError};

    fn proposal(hyp: &str) -> PatchProposal {
        PatchProposal {
            level: Level::Content,
            kind: ProposalKind::default(),
            iri: format!("https://example.test/{hyp}"),
            page: hyp.into(),
            hypothesis: hyp.into(),
            diff: format!("+ {hyp}"),
            digest: "sha256:00".into(),
            blockers: vec![],
            preexisting: vec![],
            proposer: "process:vault/1.0".into(),
            generation: "g".into(),
            stale_after: "2099-01-01T00:00:00Z".into(),
            pages: vec![hyp.into()],
            consequences: None,
        }
    }

    fn items() -> Vec<(PathBuf, PatchProposal)> {
        ["weak", "mid", "strong"].iter().map(|h| (PathBuf::from(format!("{h}.json")), proposal(h))).collect()
    }

    fn names(v: &[(PathBuf, PatchProposal)]) -> Vec<String> {
        v.iter().map(|(p, _)| p.display().to_string()).collect()
    }

    fn body(support: f64, conf: f64) -> String {
        serde_json::json!({"model":"m","usage":{"input_tokens":1,"output_tokens":1},"answers":{
            "support":{"type":"score","score":support,"confidence":conf},
            "clarity":{"type":"score","score":1.0,"confidence":conf},
            "contradiction":{"type":"noul","noul":0.1},
            "unsupported":{"type":"noul","noul":0.1},
            "novelty":{"type":"score","score":1.0,"confidence":conf},
            "urgency":{"type":"score","score":support,"confidence":conf}}})
        .to_string()
    }

    #[test]
    fn facts_carry_the_proposals_own_evidence() {
        let f = facts_of(&proposal("A"));
        assert_eq!((f.hypothesis.as_str(), f.diff.as_str()), ("A", "+ A"));
    }

    #[tokio::test]
    async fn accept_path_orders_best_supported_first() {
        let t = FnTransport::new(|b| {
            let s = match b["state"]["hypothesis"].as_str().unwrap() {
                "weak" => 0.0,
                "mid" => 1.0,
                _ => 3.0,
            };
            Ok(RawResponse { status: 200, body: body(s, 0.9) })
        });
        let j = test_jev(Item::PatchRank, t);
        let (out, ann) = rank(Some(&j), items(), &patch_rank::Params::default()).await;
        assert_eq!(names(&out), ["strong.json", "mid.json", "weak.json"]);
        assert!(ann.iter().all(Option::is_some));
    }

    #[tokio::test]
    async fn every_fallback_keeps_the_input_order() {
        let want = names(&items());
        let (out, _) = rank(None, items(), &patch_rank::Params::default()).await;
        assert_eq!(names(&out), want);
        for s in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, body(3.0, 0.0)),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let j = test_jev(Item::PatchRank, MockTransport::new(s));
            let (out, ann) = rank(Some(&j), items(), &patch_rank::Params::default()).await;
            assert_eq!(names(&out), want);
            assert!(ann.iter().all(Option::is_none));
        }
    }
}
