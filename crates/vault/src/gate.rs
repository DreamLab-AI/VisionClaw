//! `vault gate` — the domain-true autonomous continuation gate, ported from
//! `pipeline/gate.py`.
//!
//! Two contracts are kept verbatim from prime-agent, because both are easy to
//! lose and expensive to lose:
//!
//! * **A passed gate verifies only what that gate checks.** Here that is: the
//!   graph stays logically well-formed. It does **not** prove the enrichment is
//!   correct, and every machine-readable verdict says so.
//! * **A skipped check is reported, never silently dropped.** Recall is an
//!   environment capability, not a corpus property; its absence must not block
//!   work, and must not look like a pass either.

use serde::Serialize;

use crate::conflicts;
use crate::model::Corpus;
use crate::validate;
use crate::whelk;

/// Which predicates to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// `validate` only — fast, in-process.
    Quick,
    /// `validate` + `conflicts` + Whelk consistency.
    Full,
}

impl Tier {
    /// The verdict's tier name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }

    /// Parse a `--tier` value, defaulting to `quick`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("full") {
            Self::Full
        } else {
            Self::Quick
        }
    }
}

/// One predicate's outcome.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// The predicate's name.
    pub name: String,
    /// `pass`, `fail` or `skip`.
    pub status: &'static str,
    /// What the predicate found.
    pub detail: String,
}

impl Check {
    fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            status: "pass",
            detail: detail.into(),
        }
    }

    fn fail(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            status: "fail",
            detail: detail.into(),
        }
    }

    fn skip(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            status: "skip",
            detail: detail.into(),
        }
    }
}

/// The composite verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    /// `pass` or `fail`.
    pub gate: &'static str,
    /// The tier that ran.
    pub tier: &'static str,
    /// The prime-agent caveat, restated in every machine-readable verdict.
    pub caveat: &'static str,
    /// Every predicate's outcome.
    pub checks: Vec<Check>,
}

impl Verdict {
    /// `true` when every non-skipped check passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.status != "fail")
    }

    /// `0` on pass, `1` on fail.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.passed())
    }
}

const CAVEAT: &str = "A passed gate proves graph consistency only, NOT enrichment correctness.";

/// Run the gate.
///
/// The cheap structural check short-circuits the expensive ones: there is no
/// point reasoning over a corpus that is already known broken.
#[must_use]
pub fn run(
    report: &validate::Report,
    corpus: &Corpus,
    graph: Option<&crate::build::turtle::Graph>,
    tier: Tier,
) -> Verdict {
    let mut checks = Vec::new();

    let errors = report.errors();
    let structural_ok = errors.is_empty();
    checks.push(if structural_ok {
        Check::pass(
            "validate",
            format!(
                "{} pages, {} public, 0 errors, {} warnings",
                report.total_pages,
                report.public_pages,
                report.warnings().len()
            ),
        )
    } else {
        let mut codes: Vec<&str> = errors.iter().map(|i| i.code.as_str()).collect();
        codes.sort_unstable();
        codes.dedup();
        Check::fail(
            "validate",
            format!(
                "{} errors across [{}]; first: {}",
                errors.len(),
                codes.join(", "),
                errors[0]
            ),
        )
    });

    if tier == Tier::Full && structural_ok {
        let conflict_report = conflicts::analyse(corpus, "high");
        checks.push(if conflict_report.ok() {
            Check::pass(
                "conflicts",
                format!("{} conflicts, none blocking", conflict_report.summary.total),
            )
        } else {
            let blocking = conflict_report.blocking();
            Check::fail(
                "conflicts",
                format!(
                    "{} blocking conflict(s); first: {}",
                    blocking.len(),
                    blocking[0].detail
                ),
            )
        });

        checks.push(match graph {
            None => Check::skip(
                "whelk",
                "no asserted graph supplied — run the gate after a build to reason",
            ),
            Some(g) => {
                let reasoning = whelk::reason(g);
                if reasoning.is_consistent() {
                    Check::pass(
                        "whelk",
                        format!(
                            "{} classes classified, {} inferred subsumptions, 0 unsatisfiable",
                            reasoning.classified,
                            reasoning.inferred.len()
                        ),
                    )
                } else {
                    Check::fail(
                        "whelk",
                        format!(
                            "{} unsatisfiable class(es); first: {}",
                            reasoning.unsatisfiable.len(),
                            reasoning.unsatisfiable[0]
                        ),
                    )
                }
            }
        });
    }

    let passed = checks.iter().all(|c| c.status != "fail");
    Verdict {
        gate: if passed { "pass" } else { "fail" },
        tier: tier.as_str(),
        caveat: CAVEAT,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::page::{Page, Vault, VaultKind};
    use vault_core::vocabulary::Vocabulary;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
"#,
        )
        .unwrap()
    }

    fn setup(pages: Vec<(&str, &str)>) -> (validate::Report, Corpus) {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: pages
                .into_iter()
                .map(|(id, text)| {
                    Page::parse(
                        format!("/v/pages/{id}.md"),
                        format!("pages/{id}.md"),
                        id,
                        text,
                    )
                    .unwrap()
                })
                .collect(),
        };
        let corpus = Corpus::build(&vault, &vocab());
        let report = validate::validate(&vault, &corpus, &vocab());
        (report, corpus)
    }

    const GOOD: &str =
        "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: draft\n---\n";

    #[test]
    fn a_clean_corpus_passes_the_quick_tier() {
        let (report, corpus) = setup(vec![("A", GOOD)]);
        let v = run(&report, &corpus, None, Tier::Quick);
        assert!(v.passed());
        assert_eq!(v.exit_code(), 0);
        assert_eq!(v.checks.len(), 1);
    }

    #[test]
    fn every_verdict_carries_the_caveat() {
        let (report, corpus) = setup(vec![("A", GOOD)]);
        let json = serde_json::to_value(run(&report, &corpus, None, Tier::Quick)).unwrap();
        assert!(json["caveat"].as_str().unwrap().contains("NOT enrichment"));
    }

    #[test]
    fn a_validation_error_fails_the_gate() {
        let bad = "---\ntype: Class\npublic: true\n---\n";
        let (report, corpus) = setup(vec![("A", bad)]);
        let v = run(&report, &corpus, None, Tier::Quick);
        assert!(!v.passed());
        assert_eq!(v.exit_code(), 1);
    }

    #[test]
    fn the_full_tier_short_circuits_on_a_broken_corpus() {
        let bad = "---\ntype: Class\npublic: true\n---\n";
        let (report, corpus) = setup(vec![("A", bad)]);
        let v = run(&report, &corpus, None, Tier::Full);
        assert_eq!(v.checks.len(), 1, "no point reasoning over a broken graph");
    }

    #[test]
    fn the_full_tier_runs_conflicts_and_skips_whelk_without_a_graph() {
        let (report, corpus) = setup(vec![("A", GOOD)]);
        let v = run(&report, &corpus, None, Tier::Full);
        assert_eq!(v.checks.len(), 3);
        assert_eq!(v.checks[2].status, "skip");
        assert!(v.passed(), "a skipped check never fails the gate");
    }

    #[test]
    fn a_blocking_conflict_fails_the_full_tier() {
        let a = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: draft\nis-a: [\"[[B]]\"]\n---\n";
        let b = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:b\nstatus: draft\nis-a: [\"[[A]]\"]\n---\n";
        let (report, corpus) = setup(vec![("A", a), ("B", b)]);
        let v = run(&report, &corpus, None, Tier::Full);
        assert_eq!(v.checks[1].status, "fail");
        assert!(!v.passed());
    }

    #[test]
    fn whelk_runs_when_a_graph_is_supplied() {
        let (report, corpus) = setup(vec![("A", GOOD)]);
        let graph = crate::build::turtle::build_graph(&corpus, &vocab(), true);
        let v = run(&report, &corpus, Some(&graph), Tier::Full);
        assert_eq!(v.checks[2].status, "pass");
        assert!(v.checks[2].detail.contains("unsatisfiable"));
    }

    #[test]
    fn tier_parsing_defaults_to_quick() {
        assert_eq!(Tier::parse("full"), Tier::Full);
        assert_eq!(Tier::parse("FULL"), Tier::Full);
        assert_eq!(Tier::parse("nonsense"), Tier::Quick);
    }
}
