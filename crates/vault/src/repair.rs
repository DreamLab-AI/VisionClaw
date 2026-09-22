//! `vault repair fences` — close the corpus's unmatched code-fence openers.
//!
//! 457 `knowledge/` pages and 46 `working/` pages carry a triple-backtick opener
//! that is never closed. It is a genuine authoring defect, not a parser
//! disagreement: `vault_core::code` deliberately treats the region after such
//! an opener as **prose**, because treating it as code would swallow the 1,713
//! real `key:: value` lines that fall after one. That decision keeps the
//! migration lossless; it does not make the page correct. An unclosed fence
//! still renders the rest of the page as a code block in Obsidian and in every
//! markdown renderer downstream.
//!
//! This command fixes the source. It is separate from `vault migrate` on
//! purpose: `migrate` is a one-shot that is deleted after its run, and this
//! defect will recur every time somebody pastes OWL functional syntax into a
//! page.
//!
//! # The rule
//!
//! `vault_core::code` pairs markers **sequentially**, so on a file with an odd
//! number of them the one left over is always the *last* one. That is a fact
//! about the scan, not about the defect: a file whose FIRST marker is the real
//! orphan pairs 2-3 and 4-5 quite happily and still reports marker 5. So the
//! last marker is where the imbalance surfaces, not necessarily where it is.
//!
//! Three outcomes, decided by what surrounds the marker:
//!
//! * **Code below it** → a genuine unclosed opener. A **closing fence is
//!   inserted** at the next paragraph boundary, so the code is fenced and the
//!   prose after it is not.
//! * **Nothing to enclose below it, and the marker is unambiguous** → a stray
//!   marker, **removed**. Unambiguous means either the file's only marker, or
//!   prose on both sides so there is no block it could be delimiting.
//! * **Nothing below, code immediately above, and other markers in the file**
//!   → **ambiguous: reported, nothing changed.** This marker closes the block
//!   above it; the missing one is earlier in the file, and guessing would
//!   unclose a legitimate block. 50 knowledge pages are in this state,
//!   including a mermaid diagram whose closer would otherwise be deleted.
//!
//! The rule is deterministic and the command is idempotent: a repaired page has
//! no unmatched marker, so a second run changes nothing. An ambiguous page is
//! reported on every run until a human resolves it.
//!
//! ```
//! use vault::repair::{classify, Action, Verdict};
//!
//! // A stray marker with nothing but prose after it.
//! assert_eq!(
//!     classify("```\nJust some more writing.\n"),
//!     Verdict::Repair(Action::OpenerRemoved { line: 1 })
//! );
//!
//! // A real opener: functional syntax follows, then a blank line, then prose.
//! assert_eq!(
//!     classify("```\nSubClassOf(:A :B)\n\nAnd prose.\n"),
//!     Verdict::Repair(Action::CloserInserted { line: 1, at_line: 3 })
//! );
//!
//! // A balanced page needs no repair.
//! assert_eq!(classify("```\ncode\n```\n"), Verdict::Clean);
//!
//! // An orphaned CLOSER in a file that has other fences: the real defect is
//! // earlier, so this is reported and left alone.
//! let ambiguous = "```\nx = 1;\n```\n\nSubClassOf(:A :B)\n```\n\n- prose\n";
//! assert!(matches!(classify(ambiguous), Verdict::Ambiguous { .. }));
//! ```

use std::path::{Path, PathBuf};

use serde::Serialize;
use vault_core::code::CodeMap;

/// One tree to repair: a vault's `pages/` or `journals/`.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The vault name — `knowledge` or `working`.
    pub vault: String,
    /// The directory walked.
    pub dir: PathBuf,
}

/// What to repair and how.
#[derive(Debug, Clone)]
pub struct Options {
    /// Every tree to walk.
    pub scopes: Vec<Scope>,
    /// Compute everything, write nothing.
    pub dry_run: bool,
}

/// What was done to one page's unmatched opener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// The opener was spurious and the line was deleted.
    OpenerRemoved {
        /// The 1-based line the opener was on.
        line: usize,
    },
    /// The opener was real; a closing fence was inserted.
    CloserInserted {
        /// The 1-based line the opener is on.
        line: usize,
        /// The 1-based line the closing fence was inserted **before**.
        at_line: usize,
    },
}

/// What [`classify`] concluded about a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every fence marker is paired; nothing to do.
    Clean,
    /// An unambiguous defect, with the repair for it.
    Repair(Action),
    /// An unmatched marker whose **location** cannot be determined safely.
    ///
    /// Reported and left alone. The alternative is to act on the marker
    /// sequential pairing happens to name, which on these pages would delete a
    /// legitimate closing fence and unclose the block above it.
    Ambiguous {
        /// The 1-based line of the unmatched marker.
        line: usize,
        /// Why it could not be resolved, for the report.
        reason: &'static str,
    },
}

/// A page with an unmatched marker that was reported rather than changed.
#[derive(Debug, Clone, Serialize)]
pub struct Ambiguity {
    /// The page id.
    pub id: String,
    /// The vault the page belongs to.
    pub vault: String,
    /// The 1-based line of the unmatched marker.
    pub line: usize,
    /// Why it could not be resolved.
    pub reason: String,
}

/// One page's repair.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// The page id.
    pub id: String,
    /// The vault the page belongs to.
    pub vault: String,
    /// What was done.
    #[serde(flatten)]
    pub action: Action,
    /// Absolute path.
    #[serde(skip)]
    pub path: PathBuf,
    /// The repaired file.
    #[serde(skip)]
    pub after: String,
}

/// Aggregate counts and the evidence.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Markdown files examined.
    pub pages_examined: usize,
    /// Pages that carried an unmatched opener and were repaired.
    pub pages_repaired: usize,
    /// Openers removed as spurious.
    pub openers_removed: usize,
    /// Closing fences inserted.
    pub closers_inserted: usize,
    /// Pages whose unmatched marker was **reported, not repaired**, because its
    /// location is ambiguous. These still fail `vault validate`'s
    /// `UNTERMINATED_FENCE` check, deliberately: the defect is real and a human
    /// has to decide where it is.
    pub ambiguous: Vec<Ambiguity>,
    /// Every change, page by page. The report is the audit trail: a repair that
    /// is not listed did not happen.
    pub changes: Vec<Change>,
}

/// Leading keywords of the languages that actually appear in the corpus's
/// fenced blocks. Not a general-purpose language detector: a wider list starts
/// classifying English prose as code.
const CODE_KEYWORDS: &[&str] = &[
    "SELECT ",
    "CONSTRUCT ",
    "WHERE ",
    "import ",
    "from ",
    "def ",
    "class ",
    "fn ",
    "pub ",
    "const ",
    "let ",
    "var ",
    "function ",
    "#include",
    "$ ",
    "curl ",
    "docker ",
    "cargo ",
    "npm ",
];

/// `true` when `line` looks like code rather than prose.
///
/// The signals are **content** signals, never indentation. The corpus was
/// migrated from a Logseq outliner export and nearly every prose line is still
/// an indented bullet, so "indented therefore code" would classify the whole vault as code.
///
/// What actually follows a real stray opener in this corpus is OWL functional
/// syntax, Turtle and the occasional SPARQL query, so those are what the list
/// recognises.
#[must_use]
pub fn looks_like_code(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    // An outliner bullet, a heading or a quote is prose by construction.
    if t.starts_with("- ") || t.starts_with('#') || t.starts_with("> ") {
        return false;
    }
    // Turtle and SPARQL preambles.
    if t.starts_with("@prefix") || t.starts_with("@base") || t.starts_with("PREFIX ") {
        return true;
    }
    // An angle-bracketed IRI — Turtle, N-Triples, SPARQL.
    if t.contains("<http") && t.contains('>') {
        return true;
    }
    // A closing or opening brace on its own, or a line that opens a block.
    if t == "}" || t == ")" || t == "]" || t.ends_with('{') {
        return true;
    }
    // A statement terminator, which prose does not use.
    if t.ends_with(';') || t.ends_with(" .") {
        return true;
    }
    // An arrow or a scope operator.
    if t.contains("=>") || t.contains("->") || t.contains("::=") {
        return true;
    }
    // OWL functional syntax and any function-call form: an identifier
    // immediately followed by `(`, with a matching `)` on the line.
    if let Some(open) = t.find('(') {
        let head = &t[..open];
        if !head.is_empty()
            && head
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
            && head.chars().next().is_some_and(char::is_alphabetic)
            && t[open..].contains(')')
        {
            return true;
        }
    }
    // A leading keyword from the languages that actually appear in the corpus.
    CODE_KEYWORDS.iter().any(|k| t.starts_with(k))
}

/// What `text`'s fence markers amount to: nothing, a repair, or a defect whose
/// location cannot be pinned down.
///
/// The unmatched marker [`CodeMap`] reports is always the **last** one in the
/// file, because it pairs markers sequentially. That is where the imbalance
/// surfaces; it is not necessarily where the missing marker belongs. So the
/// decision uses what sits on BOTH sides of it.
#[must_use]
pub fn classify(text: &str) -> Verdict {
    let Some(line) = CodeMap::scan(text).unterminated_fence else {
        return Verdict::Clean;
    };
    let lines: Vec<&str> = text.lines().collect();
    // `line` is 1-based, so the remainder starts at index `line`.
    let remainder = lines.get(line..).unwrap_or(&[]);

    // The region a closing fence would actually ENCLOSE: from just after the
    // marker to the next paragraph boundary, or to the end of the file.
    //
    // Computing this before asking "is there code?" is the whole correctness
    // argument. Asking the question of the entire remainder instead found code
    // anywhere later in the file and then inserted the closer at the *first*
    // blank line, which on an orphaned CLOSER is the line immediately after the
    // marker. That produced an empty code block on 34 knowledge pages: it
    // stopped the runaway and validated clean, while leaving a fresh defect in
    // the source the repair exists to remove.
    let span = remainder
        .iter()
        .position(|l| l.trim().is_empty())
        .unwrap_or(remainder.len());
    if remainder[..span].iter().any(|l| looks_like_code(l)) {
        // A genuine unclosed opener: fence what follows it.
        return Verdict::Repair(Action::CloserInserted {
            line,
            at_line: line + span + 1,
        });
    }

    // Nothing below to enclose, so the marker is not an opener. Either it is a
    // stray, or it is the closer of the block ABOVE it — and which one it is
    // decides whether removing it is lossless.
    let code_above = lines[..line.saturating_sub(1)]
        .iter()
        .rev()
        .take_while(|l| !l.trim().is_empty())
        .any(|l| looks_like_code(l));
    let markers = lines
        .iter()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        })
        .count();

    if code_above && markers > 1 {
        // This marker closes the code above it, and the file has other markers,
        // so the missing one is EARLIER. Deleting this one would unclose a
        // legitimate block — on one page, a mermaid diagram.
        return Verdict::Ambiguous {
            line,
            reason: "closes the code above it; the missing marker is earlier in the file",
        };
    }
    // The file's only marker, or prose on both sides: a stray marker with
    // nothing it could be delimiting.
    Verdict::Repair(Action::OpenerRemoved { line })
}

/// Apply an [`Action`] to `text`, preserving the trailing newline convention.
///
/// The inserted closer copies the opener's own indentation and marker, so a
/// tilde fence is closed with tildes and a nested bullet's fence stays aligned.
#[must_use]
pub fn apply(text: &str, action: Action) -> String {
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    match action {
        Action::OpenerRemoved { line } => {
            if line >= 1 && line <= lines.len() {
                lines.remove(line - 1);
            }
        }
        Action::CloserInserted { line, at_line } => {
            let opener = lines.get(line - 1).map_or("```", String::as_str);
            let indent: String = opener
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let marker = if opener.trim_start().starts_with("~~~") {
                "~~~"
            } else {
                "```"
            };
            let at = (at_line - 1).min(lines.len());
            lines.insert(at, format!("{indent}{marker}"));
        }
    }
    let mut out = lines.join("\n");
    if ends_with_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Repair every unmatched fence opener under `options.scopes`.
///
/// # Errors
/// Any read or write failure.
pub fn run(options: &Options) -> anyhow::Result<Report> {
    let mut report = Report::default();
    for scope in &options.scopes {
        if !scope.dir.is_dir() {
            continue;
        }
        for path in vault_core::page::walk_pages(&scope.dir)?.files {
            report.pages_examined += 1;
            let before = std::fs::read_to_string(&path)?;
            let rel = path.strip_prefix(&scope.dir).unwrap_or(&path);
            let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
            let action = match classify(&before) {
                Verdict::Clean => continue,
                Verdict::Ambiguous { line, reason } => {
                    report.ambiguous.push(Ambiguity {
                        id,
                        vault: scope.vault.clone(),
                        line,
                        reason: reason.to_owned(),
                    });
                    continue;
                }
                Verdict::Repair(action) => action,
            };
            let after = apply(&before, action);
            // A repair that does not remove the defect is not a repair.
            debug_assert!(
                matches!(classify(&after), Verdict::Clean),
                "repair left an unmatched marker in {}",
                path.display()
            );
            report.pages_repaired += 1;
            match action {
                Action::OpenerRemoved { .. } => report.openers_removed += 1,
                Action::CloserInserted { .. } => report.closers_inserted += 1,
            }
            report.changes.push(Change {
                id,
                vault: scope.vault.clone(),
                action,
                path: path.clone(),
                after,
            });
        }
    }
    if !options.dry_run {
        for change in &report.changes {
            std::fs::write(&change.path, &change.after)?;
        }
    }
    Ok(report)
}

/// The scopes a `--vault` value names: both trees of every selected vault.
#[must_use]
pub fn scopes_for(root: &Path, vaults: &[&str]) -> Vec<Scope> {
    let mut out = Vec::new();
    for vault in vaults {
        for tree in ["pages", "journals"] {
            out.push(Scope {
                vault: (*vault).to_owned(),
                dir: root.join(vault).join(tree),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repair `classify` prescribes, or a panic naming what it said instead.
    fn repair_of(text: &str) -> Action {
        match classify(text) {
            Verdict::Repair(action) => action,
            other => panic!("expected a repair, got {other:?}"),
        }
    }

    #[test]
    fn a_balanced_page_is_not_touched() {
        assert_eq!(classify("```\ncode here\n```\n"), Verdict::Clean);
        assert_eq!(classify("no fences at all\n"), Verdict::Clean);
        assert_eq!(classify("~~~\nx\n~~~\n"), Verdict::Clean);
    }

    #[test]
    fn a_spurious_opener_is_removed() {
        let text = "- A bullet.\n```\n- More prose.\n- And more.\n";
        let action = repair_of(text);
        assert_eq!(action, Action::OpenerRemoved { line: 2 });
        let after = apply(text, action);
        assert_eq!(after, "- A bullet.\n- More prose.\n- And more.\n");
        assert_eq!(classify(&after), Verdict::Clean);
    }

    #[test]
    fn a_real_opener_gains_a_closer_at_the_paragraph_boundary() {
        let text = "Intro.\n```\nSubClassOf(:A :B)\nDeclaration(Class(:C))\n\nProse after.\n";
        let action = repair_of(text);
        assert_eq!(
            action,
            Action::CloserInserted {
                line: 2,
                at_line: 5
            }
        );
        let after = apply(text, action);
        assert_eq!(
            after,
            "Intro.\n```\nSubClassOf(:A :B)\nDeclaration(Class(:C))\n```\n\nProse after.\n"
        );
        assert_eq!(classify(&after), Verdict::Clean);
    }

    #[test]
    fn a_real_opener_with_no_boundary_is_closed_at_the_end() {
        let text = "```\n@prefix ex: <http://example.org/> .\n";
        let action = repair_of(text);
        assert_eq!(
            action,
            Action::CloserInserted {
                line: 1,
                at_line: 3
            }
        );
        let after = apply(text, action);
        assert_eq!(after, "```\n@prefix ex: <http://example.org/> .\n```\n");
        assert_eq!(classify(&after), Verdict::Clean);
    }

    #[test]
    fn the_closer_copies_the_openers_marker_and_indentation() {
        let text = "  ~~~\n  SubClassOf(:A :B)\n\n  prose\n";
        let after = apply(text, repair_of(text));
        assert!(after.contains("\n  ~~~\n\n  prose"), "{after}");
        assert_eq!(classify(&after), Verdict::Clean);
    }

    #[test]
    fn a_lone_orphaned_closer_is_removed_not_paired_with_another_marker() {
        // The 34-page bug. `migrate` consumed the opening `json-ld` fence and
        // left its closer behind, so the code sits ABOVE the marker and there
        // is nothing below it to enclose. The old rule found code further down
        // the file, inserted a closer at the next blank line — which is the very
        // next line — and produced an empty code block.
        let text = "\t  FunctionalDataProperty(ai:dataEfficiencyGain)\n\t  ```\n\n\
                    \t  - ## About Active Learning\n\t  - More prose.\n";
        let action = repair_of(text);
        assert_eq!(
            action,
            Action::OpenerRemoved { line: 2 },
            "a marker with nothing below it to enclose is stray"
        );
        let after = apply(text, action);
        assert!(!after.contains("```"), "{after}");
        assert!(
            after.contains("FunctionalDataProperty(ai:dataEfficiencyGain)"),
            "the code above the marker must survive: {after}"
        );
        assert_eq!(classify(&after), Verdict::Clean);
    }

    #[test]
    fn code_far_below_the_marker_does_not_justify_a_closer_next_to_it() {
        // The exact discriminator that made this non-obvious: the deciding line
        // was not near the fence at all.
        let text = "```\n\nprose paragraph\n\n@prefix ex: <http://example.org/> .\n";
        assert_eq!(repair_of(text), Action::OpenerRemoved { line: 1 });
    }

    #[test]
    fn a_closer_is_never_inserted_around_nothing() {
        // The structural invariant, asserted over every shape that reaches the
        // insert branch: the enclosed region always holds a code-looking line,
        // so an empty ``` ``` pair is unreachable.
        for text in [
            "```\nSubClassOf(:A :B)\n\nprose\n",
            "intro\n```\n@prefix a: <http://x/> .\nex:a ex:b ex:c .\n\nprose\n",
            "```\nlet x = 1;\n",
            "```\n\nprose\n\nSubClassOf(:A :B)\n",
            "\t  Declaration(Class(:C))\n\t  ```\n\n\t  - ## About\n",
        ] {
            let Verdict::Repair(action) = classify(text) else {
                continue;
            };
            let after = apply(text, action);
            assert_eq!(classify(&after), Verdict::Clean, "not repaired: {after}");
            // No two fence markers may end up adjacent with nothing between.
            let markers: Vec<usize> = after
                .lines()
                .enumerate()
                .filter(|(_, l)| {
                    let t = l.trim_start();
                    t.starts_with("```") || t.starts_with("~~~")
                })
                .map(|(i, _)| i)
                .collect();
            for pair in markers.windows(2) {
                assert!(
                    pair[1] - pair[0] > 1,
                    "empty code block at lines {pair:?} in:\n{after}"
                );
            }
        }
    }

    #[test]
    fn an_orphaned_closer_in_a_multi_fence_file_is_reported_not_guessed() {
        // `Ontology conversation with AIs`: a mermaid diagram whose closer is
        // the last marker in a 15-marker file. Sequential pairing reports THIS
        // marker, but the missing one is earlier — deleting it would unclose
        // the diagram and make the page worse than the defect being repaired.
        let text = "```mermaid\nA --> B\n```\n\nprose\n\n\
                    graph TD\n  I2VE2 -->|connects to| VTR\n```\n\n\
                    - ### Key Components\n";
        match classify(text) {
            Verdict::Ambiguous { line, reason } => {
                assert_eq!(line, 9);
                assert!(reason.contains("earlier"), "{reason}");
            }
            other => panic!("must not guess: {other:?}"),
        }
    }

    #[test]
    fn a_stray_marker_with_prose_on_both_sides_is_removed_even_in_a_multi_fence_file() {
        // No block it could be delimiting, so removal is lossless regardless of
        // how many other markers the file has.
        let text = "```\nlet x = 1;\n```\n\n- prose\n```\n\n- more prose\n";
        assert_eq!(repair_of(text), Action::OpenerRemoved { line: 6 });
    }

    #[test]
    fn an_ambiguous_page_is_reported_and_left_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("knowledge/pages");
        std::fs::create_dir_all(&pages).unwrap();
        let text =
            "```mermaid\nA --> B\n```\n\nprose\n\ngraph TD\n  X -->|to| Y\n```\n\n- ### After\n";
        std::fs::write(pages.join("Mermaid.md"), text).unwrap();
        let report = run(&Options {
            scopes: scopes_for(dir.path(), &["knowledge"]),
            dry_run: false,
        })
        .unwrap();
        assert_eq!(report.pages_repaired, 0);
        assert_eq!(report.ambiguous.len(), 1);
        assert_eq!(report.ambiguous[0].id, "Mermaid");
        // Reported, and the file is untouched.
        assert_eq!(
            std::fs::read_to_string(pages.join("Mermaid.md")).unwrap(),
            text
        );
    }

    #[test]
    fn the_repair_is_idempotent() {
        for text in [
            "- A bullet.\n```\n- More prose.\n",
            "Intro.\n```\nSubClassOf(:A :B)\n\nProse.\n",
            "```\n@prefix a: <http://x/> .\n",
        ] {
            let once = apply(text, repair_of(text));
            assert_eq!(classify(&once), Verdict::Clean, "{once}");
        }
    }

    #[test]
    fn a_logseq_bullet_is_prose_not_code() {
        // The trap: in an outliner every prose line is an indented bullet. A
        // repair that read indentation as code would fence the whole vault.
        assert!(!looks_like_code("\t\t- A deeply nested note."));
        assert!(!looks_like_code("    - Another one."));
        assert!(!looks_like_code("# A heading"));
        assert!(!looks_like_code("> A quotation"));
        assert!(!looks_like_code(""));
    }

    #[test]
    fn owl_and_turtle_are_code() {
        assert!(looks_like_code("SubClassOf(:A :B)"));
        assert!(looks_like_code("Declaration(Class(:Thing))"));
        assert!(looks_like_code("@prefix ex: <http://example.org/> ."));
        assert!(looks_like_code("ex:a ex:b ex:c ."));
        assert!(looks_like_code("SELECT ?s WHERE {"));
        assert!(looks_like_code("}"));
        assert!(looks_like_code("let x = 1;"));
    }

    #[test]
    fn a_run_reports_every_change_and_writes_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("knowledge/pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("Stray.md"), "- A bullet.\n```\n- More.\n").unwrap();
        std::fs::write(
            pages.join("Real.md"),
            "Intro.\n```\nSubClassOf(:A :B)\n\nProse.\n",
        )
        .unwrap();
        std::fs::write(pages.join("Fine.md"), "```\ncode\n```\n").unwrap();

        let scopes = scopes_for(dir.path(), &["knowledge"]);

        // A dry run reports and writes nothing.
        let dry = run(&Options {
            scopes: scopes.clone(),
            dry_run: true,
        })
        .unwrap();
        assert_eq!(dry.pages_examined, 3);
        assert_eq!(dry.pages_repaired, 2);
        assert_eq!((dry.openers_removed, dry.closers_inserted), (1, 1));
        assert_eq!(dry.changes.len(), 2);
        assert_eq!(
            std::fs::read_to_string(pages.join("Stray.md")).unwrap(),
            "- A bullet.\n```\n- More.\n"
        );

        // The real run writes, and a second run finds nothing left to do.
        let wet = run(&Options {
            scopes: scopes.clone(),
            dry_run: false,
        })
        .unwrap();
        assert_eq!(wet.pages_repaired, 2);
        let again = run(&Options {
            scopes,
            dry_run: false,
        })
        .unwrap();
        assert_eq!(again.pages_repaired, 0, "{:?}", again.changes);
        assert!(again.changes.is_empty());
    }
}
