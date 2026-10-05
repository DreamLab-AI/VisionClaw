//! Open-world answers for the ontology agent surface (ADR-2127).
//!
//! The store is open-world: an absent triple means the corpus is silent, not
//! that the relationship is false. This module holds the pure rules the
//! agent surface (`OntologyQueryService`, `ontology_agent_handler`) applies so
//! that no answer it returns lets a closed-world reader turn "no row" into
//! "no":
//!
//! - [`basis_from_named_graph`] / [`basis_from_edge_metadata`]: the
//!   [`FactBasis`] of a store row or a graph edge;
//! - [`SubsumptionIndex`]: the tri-valued membership check
//!   (`entailed | entailed_false | not_asserted`), where `entailed_false`
//!   needs an `owl:disjointWith` (ADR-2125) between a class the subject is
//!   entailed into and a class the queried class is entailed into;
//! - [`SubsumptionIndex::subject_view`]: one class's ancestors and
//!   descendants walked once, answering as `subsumption_basis` does;
//! - [`RelationIndex`]: the tri-valued relation check `(subject, property,
//!   object)`, where `entailed_false` needs a declared domain or range the
//!   subject or object is entailed-disjoint with;
//! - [`generation_from_location`] / [`store_generation`]: the generation an
//!   answer was computed against — the ADR-2128 `owl:versionIRI` of the bundle
//!   the vault root's `vault.toml` names (or [`GENERATION_ENV`] pins), else a
//!   content address of the ontology the backend loaded.

use crate::types::ontology_tools::{
    AnswerScope, DisjointnessWitness, Entailment, FactBasis, MembershipCheck, RelationCheck,
    RelationConstraint, RelationWitness,
};
use sha2::{Digest, Sha256};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// The asserted ontology named graph (mirrors
/// `visionclaw_adapters::oxigraph_ontology_repository::GRAPH_ONTOLOGY`).
pub const GRAPH_ASSERTED: &str = "urn:ngm:graph:ontology:assert";
/// The Whelk-inferred named graph (mirrors `GRAPH_ONTOLOGY_INFERRED`).
pub const GRAPH_INFERRED: &str = "urn:ngm:graph:ontology:inferred";
/// The provenance named graph (mirrors `GRAPH_PROVENANCE`).
pub const GRAPH_PROVENANCE: &str = "urn:ngm:graph:provenance";
/// The edge-metadata key `inferred_edge_materialiser` sets to `"true"` on a
/// materialised inferred edge (mirrors its `INFERRED_META_KEY`).
pub const INFERRED_EDGE_TAG: &str = "inferred";
/// The published ontology IRI; each build's `owl:versionIRI` is this plus
/// `/{ontology_digest}` (mirrors `vault::build::turtle::ONTOLOGY_IRI`, ADR-2128).
pub const ONTOLOGY_IRI: &str = "https://narrativegoldmine.com/ontology";
/// Environment variable naming a `vault build` output directory; the backend
/// reads the generation of the ontology it serves from it.
pub const GENERATION_ENV: &str = "ONTOLOGY_BUNDLE_DIR";

const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// The basis of a store row, from the named graph it was read from.
///
/// The inferred graph is `inferred`, the provenance graph is `provenance`.
/// Every other graph the agent surface reads (the asserted ontology, the
/// knowledge and agent graphs, or no graph at all) holds authored triples, so
/// it is `asserted`.
pub fn basis_from_named_graph(graph: Option<&str>) -> FactBasis {
    match graph.map(|g| g.trim_start_matches('<').trim_end_matches('>')) {
        Some(GRAPH_INFERRED) => FactBasis::Inferred,
        Some(GRAPH_PROVENANCE) => FactBasis::Provenance,
        _ => FactBasis::Asserted,
    }
}

/// The basis of a graph edge, from the tag `inferred_edge_materialiser` sets.
pub fn basis_from_edge_metadata(metadata: &HashMap<String, String>) -> FactBasis {
    match metadata.get(INFERRED_EDGE_TAG).map(String::as_str) {
        Some("true") => FactBasis::Inferred,
        _ => FactBasis::Asserted,
    }
}

/// Subsumption closure, asserted edges and disjointness over named classes:
/// everything a membership check needs.
///
/// The closure is the union of the Whelk subsumptions and the transitive
/// closure of the asserted `SubClassOf` pairs, so an engine that has not yet
/// classified still answers from what is authored.
#[derive(Debug, Default, Clone)]
pub struct SubsumptionIndex {
    parents: HashMap<String, HashSet<String>>,
    /// The reverse of `parents`, for [`Self::descendants`].
    children: HashMap<String, HashSet<String>>,
    asserted: HashSet<(String, String)>,
    disjoint: HashSet<(String, String)>,
    /// `owl:Nothing` and every class entailed under it.
    unsatisfiable: BTreeSet<String>,
}

impl SubsumptionIndex {
    /// Build from asserted `(child, parent)` pairs, Whelk `(sub, sup)` pairs
    /// and `owl:disjointWith` pairs (either order).
    pub fn new<A, C, D>(asserted_subclass: A, closure: C, disjoint: D) -> Self
    where
        A: IntoIterator<Item = (String, String)>,
        C: IntoIterator<Item = (String, String)>,
        D: IntoIterator<Item = (String, String)>,
    {
        let mut index = SubsumptionIndex::default();
        for (child, parent) in asserted_subclass {
            index
                .parents
                .entry(child.clone())
                .or_default()
                .insert(parent.clone());
            index.asserted.insert((child, parent));
        }
        for (sub, sup) in closure {
            if sub != sup {
                index.parents.entry(sub).or_default().insert(sup);
            }
        }
        for (a, b) in disjoint {
            if a != b {
                index.disjoint.insert((b.clone(), a.clone()));
                index.disjoint.insert((a, b));
            }
        }
        for (child, parents) in &index.parents {
            for parent in parents {
                index
                    .children
                    .entry(parent.clone())
                    .or_default()
                    .insert(child.clone());
            }
        }
        index.unsatisfiable = index.descendants(OWL_NOTHING);
        index
    }

    /// `iri` and every class entailed under it, ordered.
    pub fn descendants(&self, iri: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([iri.to_string()]);
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(children) = self.children.get(&current) {
                queue.extend(children.iter().filter(|c| !seen.contains(*c)).cloned());
            }
        }
        seen
    }

    /// Both directions of subsumption around one class, computed once, so a
    /// caller relating it to every other class (`read_note`'s related notes)
    /// pays two walks rather than one per class.
    pub fn subject_view(&self, iri: &str) -> SubjectView<'_> {
        SubjectView {
            index: self,
            subject: iri.to_string(),
            ancestors: self.ancestors(iri),
            descendants: self.descendants(iri),
        }
    }

    /// `iri` and every class it is entailed into, ordered.
    pub fn ancestors(&self, iri: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([iri.to_string()]);
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(parents) = self.parents.get(&current) {
                queue.extend(parents.iter().filter(|p| !seen.contains(*p)).cloned());
            }
        }
        seen
    }

    /// `Some(asserted)` for an authored `sub ⊑ sup`, `Some(inferred)` for one
    /// only the closure holds, `None` when the corpus is silent.
    pub fn subsumption_basis(&self, sub: &str, sup: &str) -> Option<FactBasis> {
        if self.asserted.contains(&(sub.to_string(), sup.to_string())) {
            return Some(FactBasis::Asserted);
        }
        let ancestors = self.ancestors(sub);
        if ancestors.contains(sup) || ancestors.contains(OWL_NOTHING) {
            Some(FactBasis::Inferred)
        } else {
            None
        }
    }

    /// The tri-valued answer to "is `subject` ⊑ `class`?".
    ///
    /// `entailed` when asserted or in the closure; `entailed_false` when some
    /// class the subject is entailed into is disjoint with some class the
    /// queried class is entailed into (holding the membership would make the
    /// subject unsatisfiable); otherwise `not_asserted`.
    pub fn check(
        &self,
        subject: &str,
        class: &str,
    ) -> (Entailment, Option<FactBasis>, Option<DisjointnessWitness>) {
        if let Some(basis) = self.subsumption_basis(subject, class) {
            return (Entailment::Entailed, Some(basis), None);
        }
        let class_side = self.ancestors(class);
        for s in self.ancestors(subject) {
            for c in &class_side {
                if self.disjoint.contains(&(s.clone(), c.clone())) {
                    return (
                        Entailment::EntailedFalse,
                        None,
                        Some(DisjointnessWitness {
                            subject_side: s,
                            class_side: c.clone(),
                        }),
                    );
                }
            }
        }
        (Entailment::NotAsserted, None, None)
    }

    /// [`Self::check`] packaged as the wire answer.
    pub fn membership(&self, subject: &str, class: &str, scope: AnswerScope) -> MembershipCheck {
        let (verdict, basis, witness) = self.check(subject, class);
        MembershipCheck {
            subject: subject.to_string(),
            class: class.to_string(),
            verdict,
            basis,
            witness,
            scope,
        }
    }
}

/// One class's place in a [`SubsumptionIndex`]: its ancestors and
/// descendants, walked once. [`Self::basis_to`] and [`Self::basis_from`]
/// answer exactly as [`SubsumptionIndex::subsumption_basis`] does.
#[derive(Debug, Clone)]
pub struct SubjectView<'a> {
    index: &'a SubsumptionIndex,
    subject: String,
    ancestors: BTreeSet<String>,
    descendants: BTreeSet<String>,
}

impl SubjectView<'_> {
    /// The class this view is centred on.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The subject and every class it is entailed into.
    pub fn ancestors(&self) -> &BTreeSet<String> {
        &self.ancestors
    }

    /// `subsumption_basis(subject, sup)`.
    pub fn basis_to(&self, sup: &str) -> Option<FactBasis> {
        if self
            .index
            .asserted
            .contains(&(self.subject.clone(), sup.to_string()))
        {
            Some(FactBasis::Asserted)
        } else if self.ancestors.contains(sup) || self.ancestors.contains(OWL_NOTHING) {
            Some(FactBasis::Inferred)
        } else {
            None
        }
    }

    /// `subsumption_basis(sub, subject)`.
    pub fn basis_from(&self, sub: &str) -> Option<FactBasis> {
        if self
            .index
            .asserted
            .contains(&(sub.to_string(), self.subject.clone()))
        {
            Some(FactBasis::Asserted)
        } else if self.descendants.contains(sub) || self.index.unsatisfiable.contains(sub) {
            Some(FactBasis::Inferred)
        } else {
            None
        }
    }
}

/// The declared `rdfs:domain` and `rdfs:range` of one property. Several
/// entries are an intersection, as in OWL.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PropertySignature {
    pub domain: Vec<String>,
    pub range: Vec<String>,
}

/// Class-level relation edges (`C p D`, read as `C ⊑ ∃p.D` — the corpus's
/// object-property relations and existential restrictions) and the
/// `SubPropertyOf` hierarchy: everything a relation check needs beyond the
/// [`SubsumptionIndex`].
#[derive(Debug, Default, Clone)]
pub struct RelationIndex {
    edges_by_subject: HashMap<String, Vec<(String, String)>>,
    super_properties: HashMap<String, HashSet<String>>,
}

impl RelationIndex {
    /// Build from `(subject, property, object)` edges and `(sub, sup)`
    /// `SubPropertyOf` pairs.
    pub fn new<E, S>(edges: E, sub_property: S) -> Self
    where
        E: IntoIterator<Item = (String, String, String)>,
        S: IntoIterator<Item = (String, String)>,
    {
        let mut index = RelationIndex::default();
        for (s, p, o) in edges {
            index.edges_by_subject.entry(s).or_default().push((p, o));
        }
        for (sub, sup) in sub_property {
            if sub != sup {
                index.super_properties.entry(sub).or_default().insert(sup);
            }
        }
        index
    }

    /// `property` and every property it is a sub-property of.
    pub fn property_ancestors(&self, property: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([property.to_string()]);
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(sups) = self.super_properties.get(&current) {
                queue.extend(sups.iter().filter(|p| !seen.contains(*p)).cloned());
            }
        }
        seen
    }

    /// The tri-valued answer to "does `subject` stand in `property` to
    /// `object`?".
    ///
    /// `entailed`, `asserted` for the authored edge itself; `entailed`,
    /// `inferred` when an edge `s' p' o'` exists with `subject ⊑ s'`,
    /// `p' ⊑ property` and `o' ⊑ object` (an existential inherits down the
    /// subject, widens up the object and lifts up the property hierarchy);
    /// `entailed_false` only when the property (or a super-property) declares
    /// a domain the subject is entailed-disjoint with, or a range the object
    /// is entailed-disjoint with — holding the relation would then make the
    /// subject unsatisfiable. Without such a declaration a relation is never
    /// false, only `not_asserted`.
    pub fn check(
        &self,
        classes: &SubsumptionIndex,
        signatures: &HashMap<String, PropertySignature>,
        subject: &str,
        property: &str,
        object: &str,
    ) -> (Entailment, Option<FactBasis>, Option<RelationWitness>) {
        let asserted = self
            .edges_by_subject
            .get(subject)
            .is_some_and(|es| es.iter().any(|(p, o)| p == property && o == object));
        if asserted {
            return (Entailment::Entailed, Some(FactBasis::Asserted), None);
        }
        let subject_ancestors = classes.ancestors(subject);
        if subject_ancestors.contains(OWL_NOTHING) {
            return (Entailment::Entailed, Some(FactBasis::Inferred), None);
        }
        for s in &subject_ancestors {
            for (p, o) in self.edges_by_subject.get(s).into_iter().flatten() {
                if self.property_ancestors(p).contains(property)
                    && classes.ancestors(o).contains(object)
                {
                    return (Entailment::Entailed, Some(FactBasis::Inferred), None);
                }
            }
        }
        for declaring in self.property_ancestors(property) {
            let Some(sig) = signatures.get(&declaring) else {
                continue;
            };
            let sides = sig
                .domain
                .iter()
                .map(|d| (RelationConstraint::Domain, subject, d))
                .chain(
                    sig.range
                        .iter()
                        .map(|r| (RelationConstraint::Range, object, r)),
                );
            for (constraint, member, declared) in sides {
                if let (Entailment::EntailedFalse, _, Some(disjointness)) =
                    classes.check(member, declared)
                {
                    return (
                        Entailment::EntailedFalse,
                        None,
                        Some(RelationWitness {
                            constraint,
                            property: declaring.clone(),
                            declared_class: declared.clone(),
                            disjointness,
                        }),
                    );
                }
            }
        }
        (Entailment::NotAsserted, None, None)
    }

    /// [`Self::check`] packaged as the wire answer.
    pub fn relation(
        &self,
        classes: &SubsumptionIndex,
        signatures: &HashMap<String, PropertySignature>,
        subject: &str,
        property: &str,
        object: &str,
        scope: AnswerScope,
    ) -> RelationCheck {
        let (verdict, basis, witness) = self.check(classes, signatures, subject, property, object);
        RelationCheck {
            subject: subject.to_string(),
            property: property.to_string(),
            object: object.to_string(),
            verdict,
            basis,
            witness,
            scope,
        }
    }
}

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const OWL_ONTOLOGY: &str = "http://www.w3.org/2002/07/owl#Ontology";
const OWL_VERSION_IRI: &str = "http://www.w3.org/2002/07/owl#versionIRI";

/// The `owl:versionIRI` an ADR-2128 `ontology.ttl` declares on its
/// `owl:Ontology` header, if any.
///
/// Only a `owl:versionIRI` whose subject is declared `a owl:Ontology` counts;
/// the text inside literals, comments and other subjects' triples is never
/// read as the generation. Prefixed names are expanded through the document's
/// `@prefix` / `PREFIX` declarations, so the header may use any prefix (or
/// full IRIs) and may sit anywhere in the document.
pub fn version_iri_from_turtle(turtle: &str) -> Option<String> {
    let statements = turtle_statements(turtle);
    let mut prefixes: HashMap<String, String> = HashMap::new();
    // (subject, predicate, object) at nesting depth zero, terms expanded.
    let mut triples: Vec<(String, String, String)> = Vec::new();
    for st in &statements {
        match st.first() {
            Some(Tok::Name(kw)) if kw == "@prefix" || kw.eq_ignore_ascii_case("prefix") => {
                if let (Some(Tok::Name(p)), Some(Tok::Iri(iri))) = (st.get(1), st.get(2)) {
                    prefixes.insert(p.trim_end_matches(':').to_string(), iri.clone());
                }
                continue;
            }
            Some(Tok::Name(kw)) if kw == "@base" || kw.eq_ignore_ascii_case("base") => continue,
            _ => {}
        }
        let expand = |t: &Tok| -> Option<String> {
            match t {
                Tok::Iri(iri) => Some(iri.clone()),
                Tok::Name(n) if n == "a" => Some(RDF_TYPE.to_string()),
                Tok::Name(n) => {
                    let (p, local) = n.split_once(':')?;
                    prefixes.get(p).map(|base| format!("{base}{local}"))
                }
                _ => None,
            }
        };
        let Some(subject) = st.first().and_then(expand) else {
            continue;
        };
        // Predicate-object list at depth zero: `p o (, o)* (; p o (, o)*)*`.
        let mut depth = 0i32;
        let mut predicate: Option<String> = None;
        let mut expect_predicate = true;
        for tok in &st[1..] {
            match tok {
                Tok::Open => {
                    if depth == 0 {
                        expect_predicate = false;
                    }
                    depth += 1;
                }
                Tok::Close => depth -= 1,
                _ if depth > 0 => {}
                Tok::Semi => {
                    predicate = None;
                    expect_predicate = true;
                }
                Tok::Comma => {}
                t if expect_predicate => {
                    predicate = expand(t);
                    expect_predicate = false;
                }
                t => {
                    if let (Some(p), Some(o)) = (predicate.as_ref(), expand(t)) {
                        triples.push((subject.clone(), p.clone(), o));
                    }
                }
            }
        }
    }
    let ontologies: HashSet<&str> = triples
        .iter()
        .filter(|(_, p, o)| p == RDF_TYPE && o == OWL_ONTOLOGY)
        .map(|(s, _, _)| s.as_str())
        .collect();
    triples
        .iter()
        .find(|(s, p, o)| p == OWL_VERSION_IRI && ontologies.contains(s.as_str()) && !o.is_empty())
        .map(|(_, _, o)| o.clone())
}

/// A Turtle token, as far as the header parse needs: IRIs, names (prefixed
/// names, `a`, keywords, numbers), literals (content dropped), punctuation.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Iri(String),
    Name(String),
    Literal,
    Semi,
    Comma,
    /// `[` or `(`.
    Open,
    /// `]` or `)`.
    Close,
}

/// Split a Turtle document into top-level statements of [`Tok`]s. Literals
/// (short and long, single and double quoted, with escapes), comments and
/// IRIs are consumed whole, so a `.` or `;` inside them never ends anything.
fn turtle_statements(src: &str) -> Vec<Vec<Tok>> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut out: Vec<Vec<Tok>> = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut depth = 0i32;
    let flush = |cur: &mut Vec<Tok>, out: &mut Vec<Vec<Tok>>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    let is_delim = |c: u8| {
        c.is_ascii_whitespace()
            || matches!(
                c,
                b'<' | b'"' | b'\'' | b';' | b',' | b'[' | b']' | b'(' | b')' | b'#'
            )
    };
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'<' {
            let start = i + 1;
            i = start;
            while i < b.len() && b[i] != b'>' {
                i += 1;
            }
            cur.push(Tok::Iri(src[start..i.min(b.len())].trim().to_string()));
            i += 1;
        } else if c == b'"' || c == b'\'' {
            let long = i + 2 < b.len() && b[i + 1] == c && b[i + 2] == c;
            i += if long { 3 } else { 1 };
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                } else if long {
                    if i + 2 < b.len() && b[i] == c && b[i + 1] == c && b[i + 2] == c {
                        i += 3;
                        // A long string may end with extra quotes (`""""`).
                        while i < b.len() && b[i] == c {
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                } else if b[i] == c {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            // Language tag or datatype belong to the literal.
            if i < b.len() && b[i] == b'@' {
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
                    i += 1;
                }
            } else if i + 1 < b.len() && b[i] == b'^' && b[i + 1] == b'^' {
                i += 2;
                if i < b.len() && b[i] == b'<' {
                    while i < b.len() && b[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                } else {
                    while i < b.len() && !is_delim(b[i]) {
                        i += 1;
                    }
                    // A trailing `.` ends the statement, not the datatype.
                    while i > 0 && b[i - 1] == b'.' {
                        i -= 1;
                    }
                }
            }
            cur.push(Tok::Literal);
        } else if c == b';' {
            cur.push(Tok::Semi);
            i += 1;
        } else if c == b',' {
            cur.push(Tok::Comma);
            i += 1;
        } else if c == b'[' || c == b'(' {
            depth += 1;
            cur.push(Tok::Open);
            i += 1;
        } else if c == b']' || c == b')' {
            depth -= 1;
            cur.push(Tok::Close);
            i += 1;
        } else if c == b'.' && (i + 1 >= b.len() || is_delim(b[i + 1]) || b[i + 1] == b'.') {
            if depth <= 0 {
                flush(&mut cur, &mut out);
                depth = 0;
            }
            i += 1;
        } else {
            let start = i;
            while i < b.len() && !is_delim(b[i]) {
                i += 1;
            }
            // A name never ends in `.`: a trailing one terminates the statement.
            let mut end = i;
            while end > start && b[end - 1] == b'.' {
                end -= 1;
            }
            i = end;
            let name = &src[start..end];
            if name.is_empty() {
                i += 1;
                continue;
            }
            cur.push(Tok::Name(name.to_string()));
            // SPARQL-style `PREFIX p: <iri>` / `BASE <iri>` carry no `.`.
            let sparql_directive = cur.len() == 1
                && (name.eq_ignore_ascii_case("prefix") || name.eq_ignore_ascii_case("base"));
            if sparql_directive {
                let want = if name.eq_ignore_ascii_case("prefix") {
                    2
                } else {
                    1
                };
                let mut got = 0;
                while got < want && i < b.len() {
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < b.len() && b[i] == b'<' {
                        let s = i + 1;
                        while i < b.len() && b[i] != b'>' {
                            i += 1;
                        }
                        cur.push(Tok::Iri(src[s..i.min(b.len())].to_string()));
                        i += 1;
                    } else {
                        let s = i;
                        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'<' {
                            i += 1;
                        }
                        cur.push(Tok::Name(src[s..i].to_string()));
                    }
                    got += 1;
                }
                flush(&mut cur, &mut out);
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// The generation a `vault build` `.generation.json` marker names: the
/// version IRI its `ontology_digest` addresses (ADR-2128), else its `id`.
pub fn generation_from_marker(marker_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(marker_json).ok()?;
    if let Some(digest) = v.get("ontology_digest").and_then(|d| d.as_str()) {
        if !digest.is_empty() {
            return Some(format!("{ONTOLOGY_IRI}/{digest}"));
        }
    }
    v.get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// The generation to report: the Turtle's `owl:versionIRI` first, then the
/// marker's, else `None`.
pub fn resolve_generation(turtle: Option<&str>, marker_json: Option<&str>) -> Option<String> {
    turtle
        .and_then(version_iri_from_turtle)
        .or_else(|| marker_json.and_then(generation_from_marker))
}

/// The generation of a `vault build` output directory: `data/ontology.ttl`'s
/// version IRI, else `data/.generation.json` or `.generation.json`.
pub fn generation_from_bundle_dir(dir: &Path) -> Option<String> {
    generation_from_bundle(
        dir,
        Path::new("data/ontology.ttl"),
        Path::new(".generation.json"),
    )
}

/// The generation of a bundle at `dir` whose asserted Turtle and generation
/// marker sit at the given bundle-relative paths. The marker is also looked
/// for under `data/`, where `vault build` writes its scoped copy.
fn generation_from_bundle(dir: &Path, ontology: &Path, marker: &Path) -> Option<String> {
    let turtle = std::fs::read_to_string(dir.join(ontology)).ok();
    let marker = std::fs::read_to_string(dir.join(marker))
        .or_else(|_| std::fs::read_to_string(dir.join("data").join(".generation.json")))
        .ok();
    resolve_generation(turtle.as_deref(), marker.as_deref())
}

/// Environment variable naming the vault root (the path authority the
/// backend's corpus source reads pages from; mirrors
/// `corpus_source::VAULT_ROOT_ENV`).
pub const VAULT_ROOT_ENV: &str = "VAULT_ROOT";
/// The vault manifest at a vault root.
pub const VAULT_MANIFEST: &str = "vault.toml";

/// Where the `vault build` bundle for the loaded ontology is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleLocation {
    /// An explicit bundle directory ([`GENERATION_ENV`]); the default
    /// artefact layout is assumed.
    Dir(PathBuf),
    /// A vault root: `vault.toml` names the bundle directory (`[build] out`)
    /// and its artefacts (`[build.artifacts] ontology`, `generation`).
    VaultRoot(PathBuf),
}

/// The bundle location the environment configures: [`GENERATION_ENV`] when
/// set (an operator pin), else the vault root the corpus is read from
/// ([`VAULT_ROOT_ENV`]), else `None`.
pub fn bundle_location_from_env() -> Option<BundleLocation> {
    let non_empty = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    non_empty(GENERATION_ENV)
        .map(|d| BundleLocation::Dir(PathBuf::from(d)))
        .or_else(|| non_empty(VAULT_ROOT_ENV).map(|r| BundleLocation::VaultRoot(PathBuf::from(r))))
}

/// The generation a bundle location names, or `None` when it names no
/// readable bundle. A vault root without a `vault.toml`, or whose manifest has
/// no `[build] out`, names none.
pub fn generation_from_location(location: &BundleLocation) -> Option<String> {
    match location {
        BundleLocation::Dir(dir) => generation_from_bundle_dir(dir),
        BundleLocation::VaultRoot(root) => {
            let manifest: toml::Value =
                toml::from_str(&std::fs::read_to_string(root.join(VAULT_MANIFEST)).ok()?).ok()?;
            let build = manifest.get("build")?;
            let out = build.get("out")?.as_str()?;
            let artifact = |key: &str, default: &str| -> PathBuf {
                PathBuf::from(
                    build
                        .get("artifacts")
                        .and_then(|a| a.get(key))
                        .and_then(|v| v.as_str())
                        .unwrap_or(default),
                )
            };
            generation_from_bundle(
                &root.join(out),
                &artifact("ontology", "data/ontology.ttl"),
                &artifact("generation", ".generation.json"),
            )
        }
    }
}

/// [`generation_from_location`] over [`bundle_location_from_env`].
pub fn generation_from_env() -> Option<String> {
    generation_from_location(&bundle_location_from_env()?)
}

/// The prefix of a [`store_generation`] identifier. Deliberately not the
/// ADR-2128 `owl:versionIRI` namespace: it addresses the store's content, not
/// a `vault build` graph.
pub const STORE_GENERATION_PREFIX: &str = "urn:visionclaw:generation:store:";

/// A content address of the ontology a backend has loaded: SHA-256 over the
/// sorted, de-duplicated class IRIs and `(type, subject, property, object)`
/// axioms, rendered `urn:visionclaw:generation:store:sha256-12-<hex>`.
/// Order-free, so two loads of the same content agree whatever order the
/// store returns rows in.
pub fn store_generation<'a, C, A>(classes: C, axioms: A) -> String
where
    C: IntoIterator<Item = &'a str>,
    A: IntoIterator<Item = (&'a str, &'a str, &'a str, &'a str)>,
{
    let mut lines: BTreeSet<String> = classes.into_iter().map(|c| format!("C\t{c}")).collect();
    lines.extend(
        axioms
            .into_iter()
            .map(|(t, s, p, o)| format!("A\t{t}\t{s}\t{p}\t{o}")),
    );
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{STORE_GENERATION_PREFIX}sha256-12-{hex}")
}

/// An order-free 64-bit fingerprint of a multiset of items: a cheap change
/// detector for an in-process cache, not an identifier (it is not stable
/// across builds).
pub fn fingerprint<I, T>(items: I) -> u64
where
    I: IntoIterator<Item = T>,
    T: Hash,
{
    let mut sum = 0u64;
    let mut count = 0u64;
    for item in items {
        let mut h = DefaultHasher::new();
        item.hash(&mut h);
        sum = sum.wrapping_add(h.finish());
        count += 1;
    }
    let mut h = DefaultHasher::new();
    (sum, count).hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    /// P ⊑ A (asserted), Q ⊑ B (asserted), A disjoint-with B.
    fn disjoint_fixture() -> SubsumptionIndex {
        SubsumptionIndex::new(
            vec![p("ex:P", "ex:A"), p("ex:Q", "ex:B"), p("ex:A", "ex:Root")],
            Vec::new(),
            vec![p("ex:A", "ex:B")],
        )
    }

    #[test]
    fn named_graphs_map_to_their_basis() {
        assert_eq!(
            basis_from_named_graph(Some(GRAPH_INFERRED)),
            FactBasis::Inferred
        );
        assert_eq!(
            basis_from_named_graph(Some("<urn:ngm:graph:provenance>")),
            FactBasis::Provenance
        );
        assert_eq!(
            basis_from_named_graph(Some(GRAPH_ASSERTED)),
            FactBasis::Asserted
        );
        assert_eq!(basis_from_named_graph(None), FactBasis::Asserted);
    }

    #[test]
    fn the_materialiser_tag_makes_an_edge_inferred() {
        let mut meta = HashMap::new();
        assert_eq!(basis_from_edge_metadata(&meta), FactBasis::Asserted);
        meta.insert(INFERRED_EDGE_TAG.to_string(), "true".to_string());
        assert_eq!(basis_from_edge_metadata(&meta), FactBasis::Inferred);
    }

    #[test]
    fn an_asserted_parent_is_entailed_on_an_asserted_basis() {
        let idx = disjoint_fixture();
        assert_eq!(
            idx.check("ex:P", "ex:A"),
            (Entailment::Entailed, Some(FactBasis::Asserted), None)
        );
    }

    #[test]
    fn a_transitive_ancestor_is_entailed_on_an_inferred_basis() {
        let idx = disjoint_fixture();
        assert_eq!(
            idx.check("ex:P", "ex:Root"),
            (Entailment::Entailed, Some(FactBasis::Inferred), None)
        );
    }

    #[test]
    fn a_whelk_only_subsumption_is_inferred() {
        let idx = SubsumptionIndex::new(Vec::new(), vec![p("ex:X", "ex:Y")], Vec::new());
        assert_eq!(
            idx.subsumption_basis("ex:X", "ex:Y"),
            Some(FactBasis::Inferred)
        );
    }

    #[test]
    fn membership_in_a_class_disjoint_with_an_ancestor_is_entailed_false() {
        let idx = disjoint_fixture();
        let (verdict, basis, witness) = idx.check("ex:P", "ex:B");
        assert_eq!(verdict, Entailment::EntailedFalse);
        assert_eq!(basis, None);
        assert_eq!(
            witness,
            Some(DisjointnessWitness {
                subject_side: "ex:A".into(),
                class_side: "ex:B".into(),
            })
        );
    }

    #[test]
    fn disjointness_reaches_the_queried_class_through_its_ancestors() {
        // Q ⊑ B, A disjoint B: P ⊑ Q would put P under both A and B.
        let idx = disjoint_fixture();
        assert_eq!(idx.check("ex:P", "ex:Q").0, Entailment::EntailedFalse);
    }

    #[test]
    fn silence_is_not_asserted_never_false() {
        let idx = disjoint_fixture();
        assert_eq!(
            idx.check("ex:P", "ex:Unrelated"),
            (Entailment::NotAsserted, None, None)
        );
        // Without the disjointness the same question is merely silent.
        let open = SubsumptionIndex::new(
            vec![p("ex:P", "ex:A"), p("ex:Q", "ex:B")],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(open.check("ex:P", "ex:B").0, Entailment::NotAsserted);
    }

    #[test]
    fn the_membership_answer_serialises_snake_case_with_its_scope() {
        let idx = disjoint_fixture();
        let answer = idx.membership("ex:P", "ex:B", AnswerScope::open(Some("g1".into())));
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(json["verdict"], "entailed_false");
        assert_eq!(json["scope"]["closure"], "open");
        assert_eq!(json["scope"]["generation"], "g1");
        assert_eq!(json["witness"]["subject_side"], "ex:A");
        let silent = idx.membership("ex:P", "ex:Z", AnswerScope::open(None));
        assert_eq!(
            serde_json::to_value(&silent).unwrap()["verdict"],
            "not_asserted"
        );
    }

    #[test]
    fn the_turtle_version_iri_is_the_generation() {
        let ttl = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
                   <https://narrativegoldmine.com/ontology> a owl:Ontology ;\n    \
                   owl:versionIRI <https://narrativegoldmine.com/ontology/sha256-12-f1366a5adcc8> ;\n    \
                   owl:versionInfo \"1.0.0\" .\n";
        assert_eq!(
            version_iri_from_turtle(ttl).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-f1366a5adcc8")
        );
        assert_eq!(version_iri_from_turtle("<a> <b> <c> ."), None);
    }

    #[test]
    fn the_version_iri_comes_from_the_ontology_header_only() {
        // A literal mentioning the string, and another subject's versionIRI,
        // both precede the header; neither is the generation.
        let ttl = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
                   @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
                   <urn:ngm:class:doc> rdfs:comment \"write owl:versionIRI <urn:wrong:literal> here\" .\n\
                   <urn:other:thing> owl:versionIRI <urn:wrong:subject> .\n\
                   <https://narrativegoldmine.com/ontology> a owl:Ontology ;\n    \
                   rdfs:label \"a; tricky. label\" ;\n    \
                   owl:versionIRI <https://narrativegoldmine.com/ontology/sha256-12-aaaaaaaaaaaa> .\n";
        assert_eq!(
            version_iri_from_turtle(ttl).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-aaaaaaaaaaaa")
        );
        // A long-string literal spanning statements is skipped too.
        let long = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
                    <urn:x> <urn:p> \"\"\"line . with\n<urn:y> owl:versionIRI <urn:wrong:long> .\"\"\" .\n\
                    <urn:onto:alt> a owl:Ontology .\n\
                    <urn:onto:alt> owl:versionIRI <urn:right:separate> .\n";
        assert_eq!(
            version_iri_from_turtle(long).as_deref(),
            Some("urn:right:separate")
        );
        // A versionIRI on a subject that is not the ontology is not one.
        assert_eq!(
            version_iri_from_turtle("<urn:x> <http://www.w3.org/2002/07/owl#versionIRI> <urn:v> ."),
            None
        );
        // The full-IRI spelling of the header is recognised.
        let full = "<urn:onto> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> \
                    <http://www.w3.org/2002/07/owl#Ontology> ; \
                    <http://www.w3.org/2002/07/owl#versionIRI> <urn:v:full> .";
        assert_eq!(version_iri_from_turtle(full).as_deref(), Some("urn:v:full"));
    }

    // ---- subject views (read_note's per-call basis, computed once) ----

    #[test]
    fn a_subject_view_answers_exactly_as_subsumption_basis() {
        // Asserted, closure-only, multi-parent and unsatisfiable shapes.
        let idx = SubsumptionIndex::new(
            vec![
                p("ex:P", "ex:A"),
                p("ex:A", "ex:Root"),
                p("ex:Q", "ex:B"),
                p("ex:Q", "ex:A"),
                p("ex:U", OWL_NOTHING),
            ],
            vec![p("ex:W", "ex:Root"), p("ex:A", "ex:Top")],
            vec![p("ex:A", "ex:B")],
        );
        let classes = [
            "ex:P",
            "ex:A",
            "ex:Root",
            "ex:Q",
            "ex:B",
            "ex:U",
            "ex:W",
            "ex:Top",
            "ex:Z",
            OWL_NOTHING,
        ];
        for s in classes {
            let view = idx.subject_view(s);
            assert_eq!(view.ancestors(), &idx.ancestors(s), "ancestors of {s}");
            for c in classes {
                assert_eq!(view.basis_to(c), idx.subsumption_basis(s, c), "{s} ⊑ {c}");
                assert_eq!(view.basis_from(c), idx.subsumption_basis(c, s), "{c} ⊑ {s}");
            }
        }
    }

    // ---- relation checks (ADR-2127 decision 2) ----

    fn t(s: &str, p: &str, o: &str) -> (String, String, String) {
        (s.to_string(), p.to_string(), o.to_string())
    }

    /// Wheel ⊑ Part, Car ⊑ Vehicle, Vehicle hasPart Wheel (asserted),
    /// hasFrontWheel ⊑ hasPart, Engine disjoint-with Part, Animal
    /// disjoint-with Vehicle. hasPart: domain Vehicle, range Part.
    fn relation_fixture() -> (
        SubsumptionIndex,
        RelationIndex,
        HashMap<String, PropertySignature>,
    ) {
        let classes = SubsumptionIndex::new(
            vec![
                p("ex:Wheel", "ex:Part"),
                p("ex:Car", "ex:Vehicle"),
                p("ex:Alloy", "ex:Wheel"),
                p("ex:Cat", "ex:Animal"),
            ],
            Vec::new(),
            vec![p("ex:Engine", "ex:Part"), p("ex:Animal", "ex:Vehicle")],
        );
        let relations = RelationIndex::new(
            vec![
                t("ex:Vehicle", "ex:hasPart", "ex:Wheel"),
                t("ex:Car", "ex:hasFrontWheel", "ex:Alloy"),
            ],
            vec![p("ex:hasFrontWheel", "ex:hasPart")],
        );
        let mut sigs = HashMap::new();
        sigs.insert(
            "ex:hasPart".to_string(),
            PropertySignature {
                domain: vec!["ex:Vehicle".to_string()],
                range: vec!["ex:Part".to_string()],
            },
        );
        (classes, relations, sigs)
    }

    #[test]
    fn an_asserted_edge_is_entailed_on_an_asserted_basis() {
        let (c, r, s) = relation_fixture();
        assert_eq!(
            r.check(&c, &s, "ex:Vehicle", "ex:hasPart", "ex:Wheel"),
            (Entailment::Entailed, Some(FactBasis::Asserted), None)
        );
    }

    #[test]
    fn an_edge_reached_through_the_closure_is_inferred() {
        let (c, r, s) = relation_fixture();
        // Inherited by a subclass of the subject.
        assert_eq!(
            r.check(&c, &s, "ex:Car", "ex:hasPart", "ex:Wheel").0,
            Entailment::Entailed
        );
        assert_eq!(
            r.check(&c, &s, "ex:Car", "ex:hasPart", "ex:Wheel").1,
            Some(FactBasis::Inferred)
        );
        // Widened to a superclass of the object.
        assert_eq!(
            r.check(&c, &s, "ex:Vehicle", "ex:hasPart", "ex:Part").1,
            Some(FactBasis::Inferred)
        );
        // Lifted through a sub-property.
        assert_eq!(
            r.check(&c, &s, "ex:Car", "ex:hasPart", "ex:Alloy"),
            (Entailment::Entailed, Some(FactBasis::Inferred), None)
        );
        // Never lowered to a sub-property or narrowed to a subclass object.
        assert_eq!(
            r.check(&c, &s, "ex:Vehicle", "ex:hasFrontWheel", "ex:Wheel")
                .0,
            Entailment::NotAsserted
        );
        assert_eq!(
            r.check(&c, &s, "ex:Vehicle", "ex:hasPart", "ex:Alloy").0,
            Entailment::NotAsserted
        );
    }

    #[test]
    fn an_object_disjoint_with_the_declared_range_is_entailed_false() {
        let (c, r, s) = relation_fixture();
        let (verdict, basis, witness) = r.check(&c, &s, "ex:Car", "ex:hasPart", "ex:Engine");
        assert_eq!(verdict, Entailment::EntailedFalse);
        assert_eq!(basis, None);
        assert_eq!(
            witness,
            Some(RelationWitness {
                constraint: RelationConstraint::Range,
                property: "ex:hasPart".into(),
                declared_class: "ex:Part".into(),
                disjointness: DisjointnessWitness {
                    subject_side: "ex:Engine".into(),
                    class_side: "ex:Part".into(),
                },
            })
        );
        // The range is inherited by a sub-property.
        assert_eq!(
            r.check(&c, &s, "ex:Car", "ex:hasFrontWheel", "ex:Engine").0,
            Entailment::EntailedFalse
        );
    }

    #[test]
    fn a_subject_disjoint_with_the_declared_domain_is_entailed_false() {
        let (c, r, s) = relation_fixture();
        let (verdict, _, witness) = r.check(&c, &s, "ex:Cat", "ex:hasPart", "ex:Wheel");
        assert_eq!(verdict, Entailment::EntailedFalse);
        let w = witness.unwrap();
        assert_eq!(w.constraint, RelationConstraint::Domain);
        assert_eq!(w.disjointness.subject_side, "ex:Animal");
        assert_eq!(w.disjointness.class_side, "ex:Vehicle");
    }

    #[test]
    fn a_relation_without_a_declared_signature_is_never_false() {
        let (c, r, _) = relation_fixture();
        let none = HashMap::new();
        // Without the range declaration Engine is merely unmentioned.
        assert_eq!(
            r.check(&c, &none, "ex:Car", "ex:hasPart", "ex:Engine"),
            (Entailment::NotAsserted, None, None)
        );
        let (c, r, s) = relation_fixture();
        assert_eq!(
            r.check(&c, &s, "ex:Car", "ex:requires", "ex:Engine"),
            (Entailment::NotAsserted, None, None)
        );
        let answer = r.relation(
            &c,
            &s,
            "ex:Car",
            "ex:hasPart",
            "ex:Engine",
            AnswerScope::open(None),
        );
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(json["verdict"], "entailed_false");
        assert_eq!(json["witness"]["constraint"], "range");
        assert_eq!(json["property"], "ex:hasPart");
        assert_eq!(json["scope"]["closure"], "open");
    }

    // ---- generation of the loaded ontology (ADR-2127 decision 3) ----

    #[test]
    fn the_store_generation_is_order_free_and_content_addressed() {
        let a = store_generation(
            ["ex:A", "ex:B"],
            [
                ("SubClassOf", "ex:A", "", "ex:B"),
                ("DisjointWith", "ex:A", "", "ex:C"),
            ],
        );
        let b = store_generation(
            ["ex:B", "ex:A", "ex:A"],
            [
                ("DisjointWith", "ex:A", "", "ex:C"),
                ("SubClassOf", "ex:A", "", "ex:B"),
            ],
        );
        assert_eq!(a, b);
        assert!(a.starts_with(STORE_GENERATION_PREFIX), "{a}");
        assert_eq!(
            a.len(),
            STORE_GENERATION_PREFIX.len() + "sha256-12-".len() + 12
        );
        let c = store_generation(["ex:A", "ex:B"], [("SubClassOf", "ex:A", "", "ex:B")]);
        assert_ne!(a, c);
        assert_eq!(
            fingerprint(["x", "y", "z"]),
            fingerprint(["z", "x", "y"]),
            "the change detector ignores order"
        );
        assert_ne!(fingerprint(["x", "y"]), fingerprint(["x", "y", "y"]));
    }

    #[test]
    fn a_vault_root_names_its_bundle_through_vault_toml() {
        let root = std::env::temp_dir().join(format!("adr2127-vault-{}", std::process::id()));
        let data = root.join("out-dir").join("data");
        std::fs::create_dir_all(&data).unwrap();
        // No vault.toml: the root names no bundle.
        assert_eq!(
            generation_from_location(&BundleLocation::VaultRoot(root.clone())),
            None
        );
        std::fs::write(
            root.join("vault.toml"),
            "[vault]\nname = \"t\"\n[build]\nout = \"out-dir\"\n\
             [build.artifacts]\nontology = \"data/onto.ttl\"\ngeneration = \".generation.json\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("out-dir").join(".generation.json"),
            r#"{"id":"visionGraph@abc","ontology_digest":"sha256-12-00000000000a"}"#,
        )
        .unwrap();
        assert_eq!(
            generation_from_location(&BundleLocation::VaultRoot(root.clone())).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-00000000000a")
        );
        std::fs::write(
            data.join("onto.ttl"),
            "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
             <https://narrativegoldmine.com/ontology> a owl:Ontology ;\n\
             owl:versionIRI <https://narrativegoldmine.com/ontology/sha256-12-00000000000b> .\n",
        )
        .unwrap();
        assert_eq!(
            generation_from_location(&BundleLocation::VaultRoot(root.clone())).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-00000000000b")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_marker_prefers_the_ontology_digest_over_the_id() {
        let with_digest = r#"{"id":"visionGraph@abc","ontology_digest":"sha256-12-f1366a5adcc8"}"#;
        assert_eq!(
            generation_from_marker(with_digest).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-f1366a5adcc8")
        );
        let legacy = r#"{"id":"visionGraph@abc+dirty","content_digest":"x"}"#;
        assert_eq!(
            generation_from_marker(legacy).as_deref(),
            Some("visionGraph@abc+dirty")
        );
        assert_eq!(generation_from_marker("not json"), None);
    }

    #[test]
    fn the_turtle_wins_over_the_marker_and_absence_is_none() {
        let ttl = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
                   <x> a owl:Ontology ; owl:versionIRI <urn:v:1> .";
        assert_eq!(
            resolve_generation(Some(ttl), Some(r#"{"id":"m"}"#)).as_deref(),
            Some("urn:v:1")
        );
        assert_eq!(
            resolve_generation(None, Some(r#"{"id":"m"}"#)).as_deref(),
            Some("m")
        );
        assert_eq!(resolve_generation(None, None), None);
    }

    #[test]
    fn a_bundle_directory_yields_its_generation() {
        let dir = std::env::temp_dir().join(format!("adr2127-bundle-{}", std::process::id()));
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join(".generation.json"),
            r#"{"id":"visionGraph@abc","ontology_digest":"sha256-12-000000000001"}"#,
        )
        .unwrap();
        assert_eq!(
            generation_from_bundle_dir(&dir).as_deref(),
            Some("https://narrativegoldmine.com/ontology/sha256-12-000000000001")
        );
        std::fs::write(
            data.join("ontology.ttl"),
            "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
             <https://narrativegoldmine.com/ontology> a owl:Ontology ; owl:versionIRI <urn:v:ttl> .",
        )
        .unwrap();
        assert_eq!(
            generation_from_bundle_dir(&dir).as_deref(),
            Some("urn:v:ttl")
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(generation_from_bundle_dir(&dir), None);
    }
}
