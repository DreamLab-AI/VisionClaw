//! Derivation certificates: RDFS forward chaining that records every step it
//! takes, in a file format an independent checker re-verifies with no engine,
//! no network and no trust in this crate.
//!
//! # What this is, and what it is not
//!
//! Whelk (see [`crate::whelk`]) remains the reasoner that gates the corpus:
//! it classifies, finds unsatisfiable classes and feeds `WHELK_INCONSISTENT`.
//! This module does not replace it. It produces a **warrant** for the closure
//! that Whelk computes and the Loom serves: an `oo-cert/1` certificate, which
//! is two tab-separated files (`asserted.tsv`, `derivations.tsv`) plus a
//! digest and a scope note. Every line of `derivations.tsv` names the rule
//! and the two premises it used, and a checker re-derives the conclusion from
//! the premises by pattern alone, accepting a premise only if it is asserted
//! or concluded on an **earlier** line.
//!
//! On the published corpus the RDFS table reproduces 100% of Whelk's
//! named-class closure (81,396 of 81,396 inferred subsumptions on generation
//! `1ec5774ad`, ADR-2147), because the emitter puts restrictions only on the
//! superclass side of `rdfs:subClassOf`, so every named entailment is
//! subsumption transitivity. [`Certificate::named_subclass_closure`] exposes
//! that set and a test pins the parity on a fixture.
//!
//! # Rules
//!
//! The six RDFS rules the checker's `Rules.lean` table lists, with premises
//! in exactly its order:
//!
//! | rule   | premises                       | conclusion     |
//! |--------|--------------------------------|----------------|
//! | rdfs2  | `s p o`, `p rdfs:domain c`     | `s rdf:type c` |
//! | rdfs3  | `s p o`, `p rdfs:range c`      | `o rdf:type c` |
//! | rdfs5  | `a sp b`, `b sp c`             | `a sp c`       |
//! | rdfs7  | `s p o`, `p sp q`              | `s q o`        |
//! | rdfs9  | `x rdf:type a`, `a sc b`       | `x rdf:type b` |
//! | rdfs11 | `a sc b`, `b sc c`             | `a sc c`       |
//!
//! A literal is never a subject, so rdfs3 does not fire on a literal object.
//! Terms are compared as their N-Triples spelling; nothing is compared by
//! value.
//!
//! # Conservativity
//!
//! [`consequence_delta`] compares the closures (asserted and derived) of two
//! certificates and reports what a proposal adds to, or removes from, what
//! the corpus entails under this rule table. Triples that mention a blank
//! node are counted but not compared: blank-node labels are minted per
//! build, so a label is not a stable identity across two graphs.
//!
//! # Provenance
//!
//! Adapted from Open Ontologies v2.0.1 (`src/reason.rs`, `src/boundary_core.rs`
//! and `src/conservativity.rs`), © 2026 Fabio Rovai, MIT licence; see
//! `crates/vault/licenses/open-ontologies.MIT.txt`. The certificate format and
//! rule table are theirs so that their Lean 4 checker (`oo-cert`, soundness
//! theorem `OOCert.certificate_sound`) re-checks what this module writes. The
//! chaining engine is rewritten over this crate's [`Graph`] with interned terms
//! and a worklist; no code is shared with the checker, which is the point.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::build::turtle::{Graph, Subject, Term};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const RDFS_SUB_PROPERTY_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";
const RDFS_DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
const RDFS_RANGE: &str = "http://www.w3.org/2000/01/rdf-schema#range";

/// The certificate format this module writes, as `oo-cert` reads it.
pub const FORMAT: &str = "oo-cert/1";

pub use vault_core::proposal::{
    ConsequenceDelta, CONSEQUENCE_RULE_TABLE as RULE_TABLE, CONSERVATIVE, NOT_CONSERVATIVE,
};

/// How many new or lost consequences a [`ConsequenceDelta`] quotes verbatim.
pub const EXAMPLE_CAP: usize = 8;

/// An RDFS rule, named as the checker names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// `s p o`, `p rdfs:domain c` ⟹ `s rdf:type c`.
    Rdfs2,
    /// `s p o`, `p rdfs:range c` ⟹ `o rdf:type c`.
    Rdfs3,
    /// `a sp b`, `b sp c` ⟹ `a sp c`.
    Rdfs5,
    /// `s p o`, `p sp q` ⟹ `s q o`.
    Rdfs7,
    /// `x rdf:type a`, `a sc b` ⟹ `x rdf:type b`.
    Rdfs9,
    /// `a sc b`, `b sc c` ⟹ `a sc c`.
    Rdfs11,
}

impl Rule {
    /// The id written into `derivations.tsv`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Rule::Rdfs2 => "rdfs2",
            Rule::Rdfs3 => "rdfs3",
            Rule::Rdfs5 => "rdfs5",
            Rule::Rdfs7 => "rdfs7",
            Rule::Rdfs9 => "rdfs9",
            Rule::Rdfs11 => "rdfs11",
        }
    }
}

/// A triple over interned terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Fact {
    s: u32,
    p: u32,
    o: u32,
}

/// One line of `derivations.tsv`: the rule, what it concluded, and the two
/// premises it read, in the checker's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Step {
    rule: Rule,
    conclusion: Fact,
    premises: [Fact; 2],
}

/// Terms in N-Triples spelling, each stored once.
#[derive(Debug, Clone, Default)]
struct Interner {
    terms: Vec<String>,
    index: HashMap<String, u32>,
}

impl Interner {
    fn intern(&mut self, spelled: &str) -> u32 {
        if let Some(&id) = self.index.get(spelled) {
            return id;
        }
        let id = u32::try_from(self.terms.len()).expect("fewer than 2^32 terms");
        self.terms.push(spelled.to_owned());
        self.index.insert(spelled.to_owned(), id);
        id
    }

    fn resolve(&self, id: u32) -> &str {
        &self.terms[id as usize]
    }

    fn is_literal(&self, id: u32) -> bool {
        self.resolve(id).starts_with('"')
    }
}

/// A derivation certificate over one graph.
#[derive(Debug, Clone, Default)]
pub struct Certificate {
    interner: Interner,
    asserted: Vec<Fact>,
    steps: Vec<Step>,
}

/// The facts and indices the chaining engine works over.
struct Engine {
    interner: Interner,
    facts: HashSet<Fact>,
    queue: VecDeque<Fact>,
    steps: Vec<Step>,
    /// `p → [(s, o)]` for every fact.
    by_pred: HashMap<u32, Vec<(u32, u32)>>,
    /// `a → [b]` for `a rdfs:subClassOf b`, and the reverse.
    sc_out: HashMap<u32, Vec<u32>>,
    sc_in: HashMap<u32, Vec<u32>>,
    /// `a → [b]` for `a rdfs:subPropertyOf b`, and the reverse.
    sp_out: HashMap<u32, Vec<u32>>,
    sp_in: HashMap<u32, Vec<u32>>,
    /// `c → [x]` for `x rdf:type c`.
    type_in: HashMap<u32, Vec<u32>>,
    /// `p → [c]` for `p rdfs:domain c` and `p rdfs:range c`.
    domain: HashMap<u32, Vec<u32>>,
    range: HashMap<u32, Vec<u32>>,
    kw_type: u32,
    kw_subclass: u32,
    kw_subprop: u32,
    kw_domain: u32,
    kw_range: u32,
}

impl Engine {
    fn new() -> Self {
        let mut interner = Interner::default();
        let kw_type = interner.intern(&iri(RDF_TYPE));
        let kw_subclass = interner.intern(&iri(RDFS_SUBCLASS_OF));
        let kw_subprop = interner.intern(&iri(RDFS_SUB_PROPERTY_OF));
        let kw_domain = interner.intern(&iri(RDFS_DOMAIN));
        let kw_range = interner.intern(&iri(RDFS_RANGE));
        Self {
            interner,
            facts: HashSet::new(),
            queue: VecDeque::new(),
            steps: Vec::new(),
            by_pred: HashMap::new(),
            sc_out: HashMap::new(),
            sc_in: HashMap::new(),
            sp_out: HashMap::new(),
            sp_in: HashMap::new(),
            type_in: HashMap::new(),
            domain: HashMap::new(),
            range: HashMap::new(),
            kw_type,
            kw_subclass,
            kw_subprop,
            kw_domain,
            kw_range,
        }
    }

    /// Insert a fact. Returns `true` when it was new; a new fact is indexed
    /// and queued, and a new *derived* fact is also recorded as a step.
    fn insert(&mut self, fact: Fact, step: Option<(Rule, [Fact; 2])>) -> bool {
        if !self.facts.insert(fact) {
            return false;
        }
        let Fact { s, p, o } = fact;
        self.by_pred.entry(p).or_default().push((s, o));
        if p == self.kw_subclass {
            self.sc_out.entry(s).or_default().push(o);
            self.sc_in.entry(o).or_default().push(s);
        } else if p == self.kw_subprop {
            self.sp_out.entry(s).or_default().push(o);
            self.sp_in.entry(o).or_default().push(s);
        } else if p == self.kw_type {
            self.type_in.entry(o).or_default().push(s);
        } else if p == self.kw_domain {
            self.domain.entry(s).or_default().push(o);
        } else if p == self.kw_range {
            self.range.entry(s).or_default().push(o);
        }
        if let Some((rule, premises)) = step {
            self.steps.push(Step {
                rule,
                conclusion: fact,
                premises,
            });
        }
        self.queue.push_back(fact);
        true
    }

    fn fact(s: u32, p: u32, o: u32) -> Fact {
        Fact { s, p, o }
    }

    /// Every conclusion `f` licenses together with a fact already known,
    /// in either premise position. Collected first, inserted after, so no
    /// index is read while it is being written.
    fn consequences_of(&self, f: Fact) -> Vec<(Rule, Fact, [Fact; 2])> {
        let Fact { s, p, o } = f;
        let (t, sc, sp, dom, rng) = (
            self.kw_type,
            self.kw_subclass,
            self.kw_subprop,
            self.kw_domain,
            self.kw_range,
        );
        let mut out = Vec::new();

        // rdfs2 — `s p o`, `p domain c`.
        if let Some(classes) = self.domain.get(&p) {
            for &c in classes {
                out.push((Rule::Rdfs2, Self::fact(s, t, c), [f, Self::fact(p, dom, c)]));
            }
        }
        if p == dom {
            if let Some(uses) = self.by_pred.get(&s) {
                for &(s2, o2) in uses {
                    out.push((Rule::Rdfs2, Self::fact(s2, t, o), [Self::fact(s2, s, o2), f]));
                }
            }
        }

        // rdfs3 — `s p o`, `p range c`; never a literal subject.
        if !self.interner.is_literal(o) {
            if let Some(classes) = self.range.get(&p) {
                for &c in classes {
                    out.push((Rule::Rdfs3, Self::fact(o, t, c), [f, Self::fact(p, rng, c)]));
                }
            }
        }
        if p == rng {
            if let Some(uses) = self.by_pred.get(&s) {
                for &(s2, o2) in uses {
                    if !self.interner.is_literal(o2) {
                        out.push((Rule::Rdfs3, Self::fact(o2, t, o), [Self::fact(s2, s, o2), f]));
                    }
                }
            }
        }

        // rdfs5 — `a sp b`, `b sp c`.
        if p == sp {
            if let Some(ups) = self.sp_out.get(&o) {
                for &c in ups {
                    out.push((Rule::Rdfs5, Self::fact(s, sp, c), [f, Self::fact(o, sp, c)]));
                }
            }
            if let Some(downs) = self.sp_in.get(&s) {
                for &a in downs {
                    out.push((Rule::Rdfs5, Self::fact(a, sp, o), [Self::fact(a, sp, s), f]));
                }
            }
        }

        // rdfs7 — `s p o`, `p sp q`.
        if let Some(supers) = self.sp_out.get(&p) {
            for &q in supers {
                out.push((Rule::Rdfs7, Self::fact(s, q, o), [f, Self::fact(p, sp, q)]));
            }
        }
        if p == sp {
            if let Some(uses) = self.by_pred.get(&s) {
                for &(s2, o2) in uses {
                    out.push((Rule::Rdfs7, Self::fact(s2, o, o2), [Self::fact(s2, s, o2), f]));
                }
            }
        }

        // rdfs9 — `x type a`, `a sc b`.
        if p == t {
            if let Some(ups) = self.sc_out.get(&o) {
                for &b in ups {
                    out.push((Rule::Rdfs9, Self::fact(s, t, b), [f, Self::fact(o, sc, b)]));
                }
            }
        }
        if p == sc {
            if let Some(members) = self.type_in.get(&s) {
                for &x in members {
                    out.push((Rule::Rdfs9, Self::fact(x, t, o), [Self::fact(x, t, s), f]));
                }
            }
        }

        // rdfs11 — `a sc b`, `b sc c`.
        if p == sc {
            if let Some(ups) = self.sc_out.get(&o) {
                for &c in ups {
                    out.push((Rule::Rdfs11, Self::fact(s, sc, c), [f, Self::fact(o, sc, c)]));
                }
            }
            if let Some(downs) = self.sc_in.get(&s) {
                for &a in downs {
                    out.push((Rule::Rdfs11, Self::fact(a, sc, o), [Self::fact(a, sc, s), f]));
                }
            }
        }

        out
    }

    fn run_to_fixpoint(&mut self) {
        while let Some(f) = self.queue.pop_front() {
            for (rule, conclusion, premises) in self.consequences_of(f) {
                self.insert(conclusion, Some((rule, premises)));
            }
        }
    }
}

/// Reason `graph` to its RDFS fixpoint, recording every step.
///
/// The asserted triples are taken in the graph's canonical order and the
/// derivations in the order the fixpoint reached them, so the certificate's
/// bytes are a function of the graph alone.
#[must_use]
pub fn certify(graph: &Graph) -> Certificate {
    let mut engine = Engine::new();
    let mut asserted = Vec::with_capacity(graph.len());
    for (subject, predicate, object) in graph.iter() {
        let s = engine.interner.intern(&spell_subject(subject));
        let p = engine.interner.intern(&iri(predicate));
        let o = engine.interner.intern(&spell_term(object));
        let fact = Fact { s, p, o };
        if engine.insert(fact, None) {
            asserted.push(fact);
        }
    }
    engine.run_to_fixpoint();
    Certificate {
        interner: engine.interner,
        asserted,
        steps: engine.steps,
    }
}

/// What a written certificate directory holds, for a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Always [`FORMAT`].
    pub format: String,
    /// Always [`RULE_TABLE`].
    pub rule_table: String,
    /// Lines in `asserted.tsv`.
    pub asserted: usize,
    /// Lines in `derivations.tsv`.
    pub derivations: usize,
    /// Derivations per rule.
    pub by_rule: BTreeMap<String, usize>,
    /// The sha256 of `asserted.tsv`, as `asserted.sha256` records it.
    pub asserted_sha256: String,
}

impl Certificate {
    /// Number of asserted triples.
    #[must_use]
    pub fn asserted_count(&self) -> usize {
        self.asserted.len()
    }

    /// Number of derivation steps, one per inferred triple.
    #[must_use]
    pub fn derivation_count(&self) -> usize {
        self.steps.len()
    }

    /// Derivations per rule name.
    #[must_use]
    pub fn by_rule(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for step in &self.steps {
            *out.entry(step.rule.name().to_owned()).or_insert(0) += 1;
        }
        out
    }

    fn spell(&self, f: Fact) -> (&str, &str, &str) {
        (
            self.interner.resolve(f.s),
            self.interner.resolve(f.p),
            self.interner.resolve(f.o),
        )
    }

    /// Every inferred triple in N-Triples spelling, in derivation order.
    pub fn conclusions(&self) -> impl Iterator<Item = (&str, &str, &str)> + '_ {
        self.steps.iter().map(|s| self.spell(s.conclusion))
    }

    /// Inferred `rdfs:subClassOf` pairs between two IRIs, as bare IRIs,
    /// excluding reflexive pairs — the shape [`crate::whelk::Reasoning::inferred`]
    /// has, so the two can be compared directly.
    #[must_use]
    pub fn named_subclass_closure(&self) -> BTreeSet<(String, String)> {
        let sc = iri(RDFS_SUBCLASS_OF);
        self.conclusions()
            .filter(|(s, p, o)| *p == sc && s.starts_with('<') && o.starts_with('<') && s != o)
            .map(|(s, _, o)| (unbracket(s).to_owned(), unbracket(o).to_owned()))
            .collect()
    }

    /// The bytes of `asserted.tsv`: one `s TAB p TAB o NEWLINE` per triple.
    ///
    /// This is the whole grammar of that file, and the one function that
    /// turns a selection of triples into bytes, so the digest a certificate
    /// records and the file a checker reads can never disagree.
    #[must_use]
    pub fn asserted_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.asserted.len() * 96);
        for &f in &self.asserted {
            let (s, p, o) = self.spell(f);
            push_line(&mut out, &[s, p, o]);
        }
        out
    }

    /// The sha256 of [`Self::asserted_bytes`], lower-case hex.
    #[must_use]
    pub fn asserted_sha256(&self) -> String {
        format!("{:x}", Sha256::digest(self.asserted_bytes()))
    }

    /// The bytes of `derivations.tsv`: `rule TAB conclusion TAB premise1 TAB
    /// premise2`, each triple as three fields.
    #[must_use]
    pub fn derivations_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.steps.len() * 320);
        for step in &self.steps {
            let (cs, cp, co) = self.spell(step.conclusion);
            let (as_, ap, ao) = self.spell(step.premises[0]);
            let (bs, bp, bo) = self.spell(step.premises[1]);
            push_line(
                &mut out,
                &[step.rule.name(), cs, cp, co, as_, ap, ao, bs, bp, bo],
            );
        }
        out
    }

    /// The `scope.tsv` beside the certificate: which graph was read. No
    /// checker reads it; it is there so the selection travels with the files.
    #[must_use]
    pub fn scope_tsv(&self, generation: &str) -> String {
        let mut s = String::from("oo-scope/1\n");
        s.push_str("selector\tvault-graph\n");
        s.push_str("semantics_version\tvault/1\n");
        s.push_str("default_graph\ttrue\n");
        s.push_str("complete\ttrue\n");
        s.push_str("graph\t*\n");
        let _ = writeln!(s, "generation\t{}", generation.replace(['\t', '\n', '\r'], " "));
        s
    }

    /// Write `asserted.tsv`, `derivations.tsv`, `asserted.sha256` and
    /// `scope.tsv` into `dir`, creating it.
    ///
    /// # Errors
    ///
    /// Any I/O failure.
    pub fn write(&self, dir: &Path, generation: &str) -> std::io::Result<Summary> {
        std::fs::create_dir_all(dir)?;
        let asserted = self.asserted_bytes();
        let digest = format!("{:x}", Sha256::digest(&asserted));
        std::fs::write(dir.join("asserted.tsv"), &asserted)?;
        std::fs::write(dir.join("derivations.tsv"), self.derivations_bytes())?;
        std::fs::write(dir.join("asserted.sha256"), format!("{digest}\n"))?;
        std::fs::write(dir.join("scope.tsv"), self.scope_tsv(generation))?;
        Ok(Summary {
            format: FORMAT.to_owned(),
            rule_table: RULE_TABLE.to_owned(),
            asserted: self.asserted.len(),
            derivations: self.steps.len(),
            by_rule: self.by_rule(),
            asserted_sha256: digest,
        })
    }

    /// The closure — asserted triples **and** conclusions — with no blank
    /// node, spelled, and how many triples were excluded for mentioning one.
    ///
    /// The closure, not the conclusions alone: a triple the base derived and
    /// the proposal asserts is no longer a derivation on the proposal side
    /// and would read as *lost*; an assertion the proposal drops entails
    /// nothing on either side and would read as nothing at all. Both are
    /// changes to what the graph entails, and the closure sees both.
    fn comparable_closure(&self) -> (BTreeSet<String>, usize) {
        let mut set = BTreeSet::new();
        let mut excluded = 0;
        let asserted = self.asserted.iter().map(|&f| self.spell(f));
        for (s, p, o) in asserted.chain(self.conclusions()) {
            if s.starts_with("_:") || o.starts_with("_:") {
                excluded += 1;
            } else {
                set.insert(format!("{s} {p} {o}"));
            }
        }
        (set, excluded)
    }
}

/// Compare the closures of `base` and `proposal`: what the proposal entails
/// that the base does not, and the reverse. A triple the proposal asserts
/// counts as a new consequence of the proposal, as it is.
#[must_use]
pub fn consequence_delta(base: &Certificate, proposal: &Certificate) -> ConsequenceDelta {
    let (before, _) = base.comparable_closure();
    let (after, excluded) = proposal.comparable_closure();
    let new: Vec<&String> = after.difference(&before).collect();
    let lost: Vec<&String> = before.difference(&after).collect();
    let mut new_by_predicate = BTreeMap::new();
    for line in &new {
        let predicate = line.split(' ').nth(1).unwrap_or("?");
        *new_by_predicate
            .entry(local_name(predicate).to_owned())
            .or_insert(0) += 1;
    }
    let verdict = if new.is_empty() && lost.is_empty() {
        CONSERVATIVE
    } else {
        NOT_CONSERVATIVE
    };
    ConsequenceDelta {
        rule_table: RULE_TABLE.to_owned(),
        verdict: verdict.to_owned(),
        new_consequences: new.len(),
        lost_consequences: lost.len(),
        new_by_predicate,
        new_examples: new.iter().take(EXAMPLE_CAP).map(|s| (*s).clone()).collect(),
        lost_examples: lost.iter().take(EXAMPLE_CAP).map(|s| (*s).clone()).collect(),
        not_compared_blank_node_bearing: excluded,
        base_derivations: base.derivation_count(),
        proposal_derivations: proposal.derivation_count(),
        proposal_asserted_sha256: proposal.asserted_sha256(),
    }
}

// ── spelling ──────────────────────────────────────────────────────────────

fn iri(i: &str) -> String {
    format!("<{i}>")
}

fn unbracket(t: &str) -> &str {
    t.strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(t)
}

fn local_name(spelled: &str) -> &str {
    let bare = unbracket(spelled);
    bare.rsplit(['#', '/']).next().unwrap_or(bare)
}

fn spell_subject(s: &Subject) -> String {
    match s {
        Subject::Iri(i) => iri(i),
        Subject::Blank(b) => format!("_:{b}"),
    }
}

fn spell_term(t: &Term) -> String {
    match t {
        Term::Iri(i) => iri(i),
        Term::Blank(b) => format!("_:{b}"),
        Term::Literal {
            value,
            lang,
            datatype,
        } => {
            let mut out = String::with_capacity(value.len() + 8);
            out.push('"');
            escape_into(&mut out, value);
            out.push('"');
            if let Some(lang) = lang {
                out.push('@');
                out.push_str(lang);
            } else if let Some(dt) = datatype {
                out.push_str("^^");
                out.push_str(&iri(dt));
            }
            out
        }
    }
}

/// N-Triples string escaping. Every control character is escaped, so the
/// spelled literal can never carry the tab or newline the file format splits
/// on.
fn escape_into(out: &mut String, v: &str) {
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
}

/// Append one tab-separated line. Every field is checked before any byte is
/// appended, so a refusal leaves `out` as it was; a field that carries a
/// separator cannot exist here because every term is spelled by
/// [`spell_term`], and this is the assertion of that.
fn push_line(out: &mut Vec<u8>, fields: &[&str]) {
    for f in fields {
        assert!(
            !f.is_empty() && !f.contains(['\t', '\n', '\r']),
            "a certificate field carried a separator: {f:?}"
        );
    }
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b'\t');
        }
        out.extend_from_slice(f.as_bytes());
    }
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    const EX: &str = "urn:ex:";

    fn ex(local: &str) -> String {
        format!("{EX}{local}")
    }

    fn t(local: &str) -> Term {
        Term::Iri(ex(local))
    }

    fn sc(g: &mut Graph, a: &str, b: &str) {
        g.add_iri(&ex(a), RDFS_SUBCLASS_OF, t(b));
    }

    fn lines(bytes: &[u8]) -> Vec<Vec<String>> {
        std::str::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|l| l.split('\t').map(str::to_owned).collect())
            .collect()
    }

    #[test]
    fn a_chain_derives_its_transitive_closure_under_rdfs11() {
        let mut g = Graph::new();
        sc(&mut g, "espresso", "coffee");
        sc(&mut g, "coffee", "drink");
        g.add_iri(&ex("cup"), RDF_TYPE, t("espresso"));
        let c = certify(&g);
        assert_eq!(c.asserted_count(), 3);
        let by_rule = c.by_rule();
        assert_eq!(by_rule.get("rdfs11"), Some(&1));
        assert_eq!(by_rule.get("rdfs9"), Some(&2));
        assert_eq!(c.derivation_count(), 3);
        assert_eq!(
            c.named_subclass_closure(),
            BTreeSet::from([(ex("espresso"), ex("drink"))])
        );
    }

    #[test]
    fn every_premise_is_asserted_or_concluded_on_an_earlier_line() {
        let mut g = Graph::new();
        for (a, b) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e")] {
            sc(&mut g, a, b);
        }
        g.add_iri(&ex("x"), RDF_TYPE, t("a"));
        g.add_iri(&ex("p"), RDFS_DOMAIN, t("a"));
        g.add_iri(&ex("p"), RDFS_RANGE, t("c"));
        g.add_iri(&ex("q"), RDFS_SUB_PROPERTY_OF, t("p"));
        g.add_iri(&ex("y"), &ex("q"), t("z"));
        let c = certify(&g);
        let asserted: HashSet<Vec<String>> = lines(&c.asserted_bytes()).into_iter().collect();
        let mut known = asserted.clone();
        for row in lines(&c.derivations_bytes()) {
            assert_eq!(row.len(), 10, "{row:?}");
            let conclusion = row[1..4].to_vec();
            for premise in [row[4..7].to_vec(), row[7..10].to_vec()] {
                assert!(
                    known.contains(&premise),
                    "premise {premise:?} of {conclusion:?} is neither asserted nor earlier"
                );
            }
            known.insert(conclusion);
        }
        // `y q z` + `q sp p` ⟹ `y p z` (rdfs7); then domain and range type y and z,
        // and rdfs9 walks both up the chain.
        let by_rule = c.by_rule();
        assert_eq!(by_rule.get("rdfs7"), Some(&1));
        assert_eq!(by_rule.get("rdfs2"), Some(&1));
        assert_eq!(by_rule.get("rdfs3"), Some(&1));
    }

    #[test]
    fn the_checker_premise_order_is_kept_for_every_rule() {
        let mut g = Graph::new();
        sc(&mut g, "a", "b");
        sc(&mut g, "b", "c");
        g.add_iri(&ex("x"), RDF_TYPE, t("a"));
        g.add_iri(&ex("p"), RDFS_DOMAIN, t("a"));
        g.add_iri(&ex("p"), RDFS_RANGE, t("a"));
        g.add_iri(&ex("q"), RDFS_SUB_PROPERTY_OF, t("p"));
        g.add_iri(&ex("r"), RDFS_SUB_PROPERTY_OF, t("q"));
        g.add_iri(&ex("s"), &ex("q"), t("o"));
        let c = certify(&g);
        let type_ = iri(RDF_TYPE);
        let sc_ = iri(RDFS_SUBCLASS_OF);
        let sp_ = iri(RDFS_SUB_PROPERTY_OF);
        let dom = iri(RDFS_DOMAIN);
        let rng = iri(RDFS_RANGE);
        for row in lines(&c.derivations_bytes()) {
            let (rule, c0, p1, p2) = (&row[0], &row[1..4], &row[4..7], &row[7..10]);
            match rule.as_str() {
                "rdfs2" => {
                    assert_eq!(p2[1], dom);
                    assert_eq!(p1[1], p2[0]);
                    assert_eq!(c0, &[p1[0].clone(), type_.clone(), p2[2].clone()]);
                }
                "rdfs3" => {
                    assert_eq!(p2[1], rng);
                    assert_eq!(p1[1], p2[0]);
                    assert_eq!(c0, &[p1[2].clone(), type_.clone(), p2[2].clone()]);
                }
                "rdfs5" => {
                    assert_eq!(p1[1], sp_);
                    assert_eq!(p2[1], sp_);
                    assert_eq!(p1[2], p2[0]);
                    assert_eq!(c0, &[p1[0].clone(), sp_.clone(), p2[2].clone()]);
                }
                "rdfs7" => {
                    assert_eq!(p2[1], sp_);
                    assert_eq!(p1[1], p2[0]);
                    assert_eq!(c0, &[p1[0].clone(), p2[2].clone(), p1[2].clone()]);
                }
                "rdfs9" => {
                    assert_eq!(p1[1], type_);
                    assert_eq!(p2[1], sc_);
                    assert_eq!(p1[2], p2[0]);
                    assert_eq!(c0, &[p1[0].clone(), type_.clone(), p2[2].clone()]);
                }
                "rdfs11" => {
                    assert_eq!(p1[1], sc_);
                    assert_eq!(p2[1], sc_);
                    assert_eq!(p1[2], p2[0]);
                    assert_eq!(c0, &[p1[0].clone(), sc_.clone(), p2[2].clone()]);
                }
                other => panic!("unknown rule {other}"),
            }
        }
        assert_eq!(c.by_rule().get("rdfs5"), Some(&1));
    }

    #[test]
    fn a_literal_object_never_becomes_a_subject_under_rdfs3() {
        let mut g = Graph::new();
        g.add_iri(&ex("p"), RDFS_RANGE, t("c"));
        g.add_iri(
            &ex("s"),
            &ex("p"),
            Term::Literal {
                value: "a tab\there".into(),
                lang: Some("en".into()),
                datatype: None,
            },
        );
        let c = certify(&g);
        assert_eq!(c.derivation_count(), 0);
        let asserted = String::from_utf8(c.asserted_bytes()).unwrap();
        assert!(asserted.contains("\"a tab\\there\"@en"), "{asserted}");
        assert_eq!(asserted.lines().count(), 2);
        assert!(asserted.lines().all(|l| l.split('\t').count() == 3));
    }

    #[test]
    fn restrictions_on_the_superclass_side_are_inherited_but_excluded_from_the_named_closure() {
        let mut g = Graph::new();
        let r = g.fresh_blank();
        sc(&mut g, "a", "b");
        g.add_iri(&ex("b"), RDFS_SUBCLASS_OF, Term::Blank(r.clone()));
        g.add(
            Subject::Blank(r),
            "http://www.w3.org/2002/07/owl#someValuesFrom",
            t("c"),
        );
        let c = certify(&g);
        // a ⊑ _:r is a real RDFS consequence and is certified...
        assert_eq!(c.derivation_count(), 1);
        // ...but it is not a named subsumption.
        assert!(c.named_subclass_closure().is_empty());
        // The comparable closure keeps `a ⊑ b` and excludes the three
        // blank-node-bearing triples (two asserted, one derived).
        let (comparable, excluded) = c.comparable_closure();
        assert_eq!(comparable.len(), 1);
        assert_eq!(excluded, 3);
    }

    #[test]
    fn whelk_and_the_certificate_agree_on_the_named_closure() {
        let mut g = Graph::new();
        for c in ["a", "b", "c", "d", "e", "x"] {
            g.add_iri(
                &ex(c),
                RDF_TYPE,
                Term::Iri("http://www.w3.org/2002/07/owl#Class".into()),
            );
        }
        g.add_iri(
            &ex("p"),
            RDF_TYPE,
            Term::Iri("http://www.w3.org/2002/07/owl#ObjectProperty".into()),
        );
        sc(&mut g, "a", "b");
        sc(&mut g, "b", "c");
        sc(&mut g, "c", "d");
        sc(&mut g, "x", "c");
        sc(&mut g, "e", "a");
        let r = g.fresh_blank();
        g.add_iri(&ex("b"), RDFS_SUBCLASS_OF, Term::Blank(r.clone()));
        g.add(
            Subject::Blank(r.clone()),
            "http://www.w3.org/2002/07/owl#onProperty",
            t("p"),
        );
        g.add(
            Subject::Blank(r),
            "http://www.w3.org/2002/07/owl#someValuesFrom",
            t("d"),
        );
        let whelk: BTreeSet<(String, String)> =
            crate::whelk::reason(&g).unwrap().inferred.into_iter().collect();
        assert!(!whelk.is_empty());
        assert_eq!(certify(&g).named_subclass_closure(), whelk);
    }

    #[test]
    fn the_delta_names_what_a_change_adds_and_loses_and_skips_blank_nodes() {
        let mut base = Graph::new();
        sc(&mut base, "a", "b");
        sc(&mut base, "b", "c");
        base.add_iri(&ex("x"), RDF_TYPE, t("a"));
        let mut proposed = base.clone();
        sc(&mut proposed, "c", "d");
        let r = proposed.fresh_blank();
        proposed.add_iri(&ex("d"), RDFS_SUBCLASS_OF, Term::Blank(r));
        let delta = consequence_delta(&certify(&base), &certify(&proposed));
        assert_eq!(delta.verdict, NOT_CONSERVATIVE);
        assert!(!delta.is_conservative());
        // The asserted c⊑d, then a⊑d, b⊑d (rdfs11) and x type d (rdfs9); the
        // blank-node triples (d ⊑ _:r1 asserted; a, b, c ⊑ _:r1 and
        // x type _:r1 derived) are counted, not compared.
        assert_eq!(delta.new_consequences, 4, "{delta:?}");
        assert_eq!(delta.lost_consequences, 0);
        assert_eq!(delta.new_by_predicate.get("subClassOf"), Some(&3));
        assert_eq!(delta.new_by_predicate.get("type"), Some(&1));
        assert_eq!(delta.not_compared_blank_node_bearing, 5);
        assert_eq!(delta.new_examples.len(), 4);
        assert_eq!(delta.proposal_asserted_sha256.len(), 64);

        let reverse = consequence_delta(&certify(&proposed), &certify(&base));
        assert_eq!(reverse.lost_consequences, 4);
        assert_eq!(reverse.new_consequences, 0);

        let same = consequence_delta(&certify(&base), &certify(&base));
        assert_eq!(same.verdict, CONSERVATIVE);
        assert!(same.is_conservative());
    }

    #[test]
    fn asserting_what_was_already_entailed_is_conservative_and_dropping_an_assertion_is_not() {
        // Found by the cross-vendor audit (ADR-2147): a delta over derivations
        // alone reports the first as LOST and the second as nothing.
        let mut base = Graph::new();
        base.add_iri(&ex("a"), &ex("p"), t("b"));
        base.add_iri(&ex("p"), RDFS_DOMAIN, t("c"));
        let mut proposed = base.clone();
        proposed.add_iri(&ex("a"), RDF_TYPE, t("c"));
        let delta = consequence_delta(&certify(&base), &certify(&proposed));
        assert_eq!(delta.verdict, CONSERVATIVE, "{delta:?}");

        let dropped = Graph::new();
        let delta = consequence_delta(&certify(&base), &certify(&dropped));
        assert_eq!(delta.verdict, NOT_CONSERVATIVE);
        assert_eq!(delta.lost_consequences, 3);
        assert_eq!(delta.new_consequences, 0);
    }

    #[test]
    fn the_written_directory_is_the_format_and_its_digest_is_the_file() {
        let mut g = Graph::new();
        sc(&mut g, "a", "b");
        sc(&mut g, "b", "c");
        let c = certify(&g);
        let dir = tempfile::tempdir().unwrap();
        let summary = c.write(dir.path(), "g@abc").unwrap();
        assert_eq!(summary.format, FORMAT);
        assert_eq!((summary.asserted, summary.derivations), (2, 1));
        let asserted = std::fs::read(dir.path().join("asserted.tsv")).unwrap();
        let recorded = std::fs::read_to_string(dir.path().join("asserted.sha256")).unwrap();
        assert_eq!(recorded.trim(), format!("{:x}", Sha256::digest(&asserted)));
        assert_eq!(recorded.trim(), summary.asserted_sha256);
        let scope = std::fs::read_to_string(dir.path().join("scope.tsv")).unwrap();
        assert!(scope.starts_with("oo-scope/1\n"));
        assert!(scope.contains("generation\tg@abc\n"));
        let derivations = std::fs::read_to_string(dir.path().join("derivations.tsv")).unwrap();
        assert_eq!(
            derivations,
            format!(
                "rdfs11\t<{a}>\t{sc}\t<{c}>\t<{a}>\t{sc}\t<{b}>\t<{b}>\t{sc}\t<{c}>\n",
                a = ex("a"),
                b = ex("b"),
                c = ex("c"),
                sc = iri(RDFS_SUBCLASS_OF)
            )
        );
    }

    #[test]
    fn certification_is_deterministic_for_the_same_graph() {
        let mut g = Graph::new();
        for (a, b) in [("a", "b"), ("b", "c"), ("c", "d"), ("q", "c")] {
            sc(&mut g, a, b);
        }
        g.add_iri(&ex("x"), RDF_TYPE, t("a"));
        let one = certify(&g);
        let two = certify(&g.clone());
        assert_eq!(one.derivations_bytes(), two.derivations_bytes());
        assert_eq!(one.asserted_sha256(), two.asserted_sha256());
    }
}
