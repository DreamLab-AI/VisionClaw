//! Timing guard and phase breakdown for the EL++ closure.
//!
//! `vault build` on the full corpus did not finish: 4,000 classes reason in
//! 9 s, 5,000 in 50 s, 6,000 in 285 s, and 8,457 had not returned after 27
//! minutes. `vault gate --tier quick`, which skips the reasoner, is 1 s at
//! 6,000, so the cost is entirely in `whelk::reason`. `vault propose` shares
//! the same call and the same symptom.
//!
//! The guard test runs in CI on a synthetic corpus shaped like the real one.
//! The phase breakdown is `#[ignore]`d and points at a real repository, because
//! it is a diagnostic rather than an assertion:
//!
//! ```text
//! VAULT_SCALING_REPO=/path/to/visionGraph \
//!   cargo test -p vault --test whelk_scaling -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use vault::build::turtle;
use vault::model::Corpus;
use vault::whelk;
use vault_core::page::{Page, Vault, VaultKind};
use vault_core::vocabulary::Vocabulary;

/// The real vocabulary, so the emitted axioms match production exactly.
fn vocabulary() -> Option<Vocabulary> {
    for candidate in [
        "../../../visionGraph/ontology/vocabulary.yaml",
        "../../visionGraph/ontology/vocabulary.yaml",
        "/home/devuser/workspace/visionGraph/ontology/vocabulary.yaml",
    ] {
        if let Ok(v) = Vocabulary::load(candidate) {
            return Some(v);
        }
    }
    None
}

/// A corpus of `n` classes shaped like `knowledge/`: a shallow domain lattice,
/// several parents per class, and a scattering of typed relations that become
/// existential restrictions.
fn synthetic(n: usize, vocab: &Vocabulary) -> Corpus {
    const DOMAINS: usize = 6;
    let mut pages: Vec<Page> = Vec::with_capacity(n + DOMAINS);
    for d in 0..DOMAINS {
        let text = format!(
            "---\ntype: Class\ntitle: Domain {d}\nresource: urn:ngm:class:domain-{d}\n\
             status: stable\npublic: true\n---\n\nA domain.\n"
        );
        pages.push(
            Page::parse(
                format!("/v/pages/Domain {d}.md"),
                format!("pages/Domain {d}.md"),
                format!("Domain {d}"),
                &text,
            )
            .expect("synthetic domain parses"),
        );
    }
    // Chains, not a star. A depth-1 hierarchy entails nothing beyond what is
    // asserted, so a star-shaped fixture reasons instantly and the guard cannot
    // fail for the right reason. Chains of `DEPTH` give every class `DEPTH`-ish
    // ancestors and a closure that actually has to be computed.
    const DEPTH: usize = 20;
    for i in 0..n {
        let d = i % DOMAINS;
        // The parent is the previous link in the chain, except at the head where
        // it is the domain root.
        let parent = if i % DEPTH == 0 {
            format!("  - '[[Domain {d}]]'\n")
        } else {
            format!("  - '[[Class {}]]'\n", i - 1)
        };
        // Two parents for a quarter of the corpus: the real lattice has 1,397
        // multi-parent classes, and multi-parent is what makes a closure grow.
        let second = if i % 4 == 0 {
            format!("  - '[[Domain {}]]'\n", (d + 1) % DOMAINS)
        } else {
            String::new()
        };
        // A relation every third class, which the emitter turns into
        // `C ⊑ ∃R.D` when both endpoints are declared pages.
        let relation = if i % 3 == 0 {
            format!("requires:\n  - '[[Class {}]]'\n", (i + 1) % n)
        } else {
            String::new()
        };
        let text = format!(
            "---\ntype: Class\ntitle: Class {i}\nresource: urn:ngm:class:class-{i}\n\
             status: stable\npublic: true\nis-a:\n{parent}{second}{relation}---\n\n\
             A class.\n"
        );
        pages.push(
            Page::parse(
                format!("/v/pages/Class {i}.md"),
                format!("pages/Class {i}.md"),
                format!("Class {i}"),
                &text,
            )
            .expect("synthetic class parses"),
        );
    }
    let vault = Vault {
        root: std::path::PathBuf::from("/v"),
        kind: VaultKind::Knowledge,
        pages,
        journals: Vec::new(),
        skipped: Vec::new(),
    };
    Corpus::build(&vault, vocab)
}

fn time_reason(corpus: &Corpus, vocab: &Vocabulary) -> (Duration, whelk::Reasoning) {
    let graph = turtle::build_graph(corpus, vocab, true);
    let start = Instant::now();
    let reasoning = whelk::reason(&graph);
    (start.elapsed(), reasoning)
}

/// The guard the lead asked for: 2,000 classes must reason in under 5 s.
///
/// Deliberately a wall-clock assertion rather than a complexity claim. The
/// failure being guarded against is a superlinear regression, and the only
/// honest statement about one is "this used to take a second and now it does
/// not".
#[test]
fn two_thousand_classes_reason_within_the_budget() {
    let Some(vocab) = vocabulary() else {
        eprintln!("skipped: the real vocabulary is not reachable from here");
        return;
    };
    let corpus = synthetic(2_000, &vocab);
    let (elapsed, reasoning) = time_reason(&corpus, &vocab);
    eprintln!(
        "2,000 classes: {:?}, {} classified, {} inferred",
        elapsed,
        reasoning.classified,
        reasoning.inferred.len()
    );
    // A guard that reasons over nothing cannot fail for the right reason, so
    // assert the fixture still entails something before trusting the timing.
    assert!(
        reasoning.inferred.len() > 1_000,
        "the fixture must entail a non-trivial closure, got {} pairs",
        reasoning.inferred.len()
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "reasoning over 2,000 classes took {elapsed:?}, budget is 5s — \
         this is the superlinear regression, not a slow machine"
    );
}

/// The shape of the curve, printed rather than asserted.
#[test]
#[ignore = "diagnostic: prints a timing curve, asserts nothing"]
fn the_scaling_curve() {
    let Some(vocab) = vocabulary() else {
        eprintln!("skipped: the real vocabulary is not reachable from here");
        return;
    };
    for n in [500, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000] {
        let corpus = synthetic(n, &vocab);
        let (elapsed, reasoning) = time_reason(&corpus, &vocab);
        eprintln!(
            "{n:>6} classes  reason {:>10.3?}  inferred {:>8}  unsat {}",
            elapsed,
            reasoning.inferred.len(),
            reasoning.unsatisfiable.len()
        );
    }
}

/// Where the time goes inside `reason`, on a synthetic corpus of a given size.
#[test]
#[ignore = "diagnostic: prints a phase breakdown, asserts nothing"]
fn the_phase_breakdown() {
    let Some(vocab) = vocabulary() else {
        eprintln!("skipped: the real vocabulary is not reachable from here");
        return;
    };
    let n: usize = std::env::var("VAULT_SCALING_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let corpus = synthetic(n, &vocab);

    let t = Instant::now();
    let graph = turtle::build_graph(&corpus, &vocab, true);
    eprintln!("{n} classes: build_graph {:?}", t.elapsed());

    let t = Instant::now();
    let reasoning = whelk::reason(&graph);
    eprintln!(
        "{n} classes: reason {:?} -> {} inferred, {} classified",
        t.elapsed(),
        reasoning.inferred.len(),
        reasoning.classified
    );
}

/// Phase timings and closure size on a **real** repository.
///
/// The question this answers: is the cost the reasoner's saturation, or the
/// sheer SIZE of the closure it correctly produces? A closure with millions of
/// pairs is a modelling result — the corpus is a dense multi-parent lattice with
/// existential restrictions — and no amount of tuning makes iterating it cheap.
/// A small closure with a large runtime is an implementation bug.
///
/// ```text
/// VAULT_SCALING_REPO=/path/to/repo VAULT_SCALING_LIMIT=6000 \
///   cargo test -p vault --release --test whelk_scaling -- --ignored --nocapture real_repo
/// ```
#[test]
#[ignore = "diagnostic: needs VAULT_SCALING_REPO"]
fn real_repo_phase_breakdown() {
    let Ok(repo) = std::env::var("VAULT_SCALING_REPO") else {
        eprintln!("skipped: set VAULT_SCALING_REPO");
        return;
    };
    let repo = std::path::PathBuf::from(repo);
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let limit: usize = std::env::var("VAULT_SCALING_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    let t = Instant::now();
    let mut vault = Vault::load(repo.join("knowledge"), VaultKind::Knowledge).expect("vault");
    vault.pages.truncate(limit);
    eprintln!("load {:?} -> {} pages", t.elapsed(), vault.pages.len());

    let t = Instant::now();
    let corpus = Corpus::build(&vault, &vocab);
    eprintln!(
        "corpus {:?} -> {} records",
        t.elapsed(),
        corpus.records.len()
    );

    let t = Instant::now();
    let graph = turtle::build_graph(&corpus, &vocab, true);
    eprintln!("build_graph {:?}", t.elapsed());

    let t = Instant::now();
    let reasoning = whelk::reason(&graph);
    eprintln!(
        "reason {:?} -> {} classified, {} INFERRED PAIRS, {} unsatisfiable",
        t.elapsed(),
        reasoning.classified,
        reasoning.inferred.len(),
        reasoning.unsatisfiable.len()
    );
}

/// A pure `subClassOf` DAG: `n` classes, each with `parents` parents chosen from
/// earlier classes. No object properties, no restrictions, no cycles.
///
/// This is the reproduction. The real corpus's cost survives removing object
/// properties, transitivity, restrictions and cycles, so what is left is the
/// shape of the taxonomy itself — and the real taxonomy is a *tangled*
/// multi-parent DAG (2,233 `MULTI_PARENT` classes), not the tree a chain
/// fixture builds.
fn synthetic_dag(n: usize, parents: usize, vocab: &Vocabulary) -> Corpus {
    let mut pages: Vec<Page> = Vec::with_capacity(n + 1);
    pages.push(
        Page::parse(
            "/v/pages/Root.md",
            "pages/Root.md",
            "Root",
            "---\ntype: Class\ntitle: Root\nresource: urn:ngm:class:root\n\
             status: stable\npublic: true\n---\n\nRoot.\n",
        )
        .expect("root parses"),
    );
    for i in 0..n {
        let mut is_a = String::new();
        if i == 0 {
            is_a.push_str("  - '[[Root]]'\n");
        } else {
            // Spread the parents over the earlier classes rather than taking the
            // immediately preceding ones, so the DAG is wide and tangled instead
            // of a near-tree. A deterministic stride keeps the fixture stable.
            for k in 0..parents.min(i) {
                let parent = (i * 7 + k * 13) % i;
                let _ = std::fmt::Write::write_fmt(
                    &mut is_a,
                    format_args!("  - '[[Class {parent}]]'\n"),
                );
            }
        }
        let text = format!(
            "---\ntype: Class\ntitle: Class {i}\nresource: urn:ngm:class:class-{i}\n\
             status: stable\npublic: true\nis-a:\n{is_a}---\n\nA class.\n"
        );
        pages.push(
            Page::parse(
                format!("/v/pages/Class {i}.md"),
                format!("pages/Class {i}.md"),
                format!("Class {i}"),
                &text,
            )
            .expect("class parses"),
        );
    }
    let vault = Vault {
        root: std::path::PathBuf::from("/v"),
        kind: VaultKind::Knowledge,
        pages,
        journals: Vec::new(),
        skipped: Vec::new(),
    };
    Corpus::build(&vault, vocab)
}

/// Does multi-parent density reproduce the blow-up? Prints a grid.
#[test]
#[ignore = "diagnostic: prints a density grid, asserts nothing"]
fn multi_parent_density() {
    let Some(vocab) = vocabulary() else {
        eprintln!("skipped: the real vocabulary is not reachable from here");
        return;
    };
    for parents in [1usize, 2, 3, 4] {
        for n in [1_000usize, 2_000, 3_000] {
            let corpus = synthetic_dag(n, parents, &vocab);
            let (elapsed, reasoning) = time_reason(&corpus, &vocab);
            eprintln!(
                "parents={parents} n={n:>5}  reason {:>12.3?}  inferred {:>9}",
                elapsed,
                reasoning.inferred.len()
            );
        }
    }
}
