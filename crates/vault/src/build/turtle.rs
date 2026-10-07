//! `ontology.ttl` — the asserted OWL 2 EL graph, ported from
//! `pipeline/jsonld_to_turtle.py`.
//!
//! The port keeps the *triples* identical, not the bytes: `rdflib`'s Turtle
//! serialiser is not reproducible outside `rdflib`, so the golden test compares
//! parsed triple sets. What is contractual, and what the vocabulary carries, is
//! the set of property IRIs — `vc:hasPart`, `vc:requires`, `vc:dependsOn` and
//! the rest under `https://narrativegoldmine.com/ns/v1#`.
//!
//! Three modelling decisions from the Python are preserved deliberately:
//!
//! * **No inverse or symmetric object properties.** Both are outside OWL 2 EL;
//!   `isPartOf`/`enabledBy` are declared as plain properties and
//!   `contrastsWith`/`bridgesTo`/`relatedTo` as plain associative links.
//! * **Domain-root disjointness stays off.** The corpus is a deliberate
//!   cross-domain lattice (1,396 multi-parent classes); disjoint roots made
//!   98.8% of classes unsatisfiable when they were last enabled. Disjointness
//!   is admitted only between **siblings** a page declares with
//!   `disjoint-with` (ADR-2125): it is emitted as `owl:disjointWith`, never
//!   onto a domain root or taxonomy category, `vault validate` refuses a
//!   non-sibling pair (`DISJOINT_NOT_SIBLINGS`), and `vault build` refuses any
//!   unsatisfiable class.
//! * **The single-ref tail becomes `skos:Concept` stubs**, not dangling
//!   `owl:Class` references, so the hierarchy stays well-founded for EL
//!   reasoning while the associative links into the long tail survive.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use vault_core::vocabulary::Vocabulary;

use crate::model::{Corpus, EntityType};

/// The `vc:` namespace.
pub const VC: &str = "https://narrativegoldmine.com/ns/v1#";
/// The `ngm:` class namespace.
pub const NGM: &str = "https://narrativegoldmine.com/class/";
/// The `ngmi:` individual namespace.
pub const NGMI: &str = "https://narrativegoldmine.com/individual/";
const OWL: &str = "http://www.w3.org/2002/07/owl#";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
const DCTERMS: &str = "http://purl.org/dc/terms/";

/// The ontology header's IRI. Each emitted graph's `owl:versionIRI` is this
/// plus `/{ontology_digest}` ([`ontology_digest`], ADR-2128).
pub const ONTOLOGY_IRI: &str = "https://narrativegoldmine.com/ontology";
/// `owl:versionInfo` — a human semver for the ontology's shape, distinct from
/// both the per-generation `owl:versionIRI` and the integer
/// `vocabulary_version`.
pub const VERSION_INFO: &str = "3.1.0";

/// The top-level domain roots and the 34 intermediate taxonomy categories.
/// Membership never implies disjointness, and neither list may ever be a
/// member of an `owl:disjointWith` axiom (ADR-2125). They live in
/// [`vault_core::consistency`] so `vault` and the elevation actor share them.
pub use vault_core::consistency::{CATEGORY_SLUGS, DOMAIN_ROOT_SLUGS};

/// The maturity levels declared as named individuals, so downstream code
/// matches an IRI rather than a string.
pub const MATURITY_LEVELS: &[(&str, &str)] = &[
    ("established", "Established"),
    ("emerging", "Emerging"),
    ("draft", "Draft"),
    ("stub", "Stub"),
    ("deprecated", "Deprecated"),
];

/// An RDF object term.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Term {
    /// An absolute IRI.
    Iri(String),
    /// A plain or language-tagged literal.
    Literal {
        /// The lexical form.
        value: String,
        /// A BCP-47 language tag, e.g. `en`.
        lang: Option<String>,
        /// A datatype IRI.
        datatype: Option<String>,
    },
    /// A blank node, identified by a deterministic label.
    Blank(String),
}

impl Term {
    /// A language-tagged literal.
    #[must_use]
    pub fn lang(value: impl Into<String>, lang: &str) -> Self {
        Self::Literal {
            value: value.into(),
            lang: Some(lang.to_owned()),
            datatype: None,
        }
    }

    /// A plain literal.
    #[must_use]
    pub fn plain(value: impl Into<String>) -> Self {
        Self::Literal {
            value: value.into(),
            lang: None,
            datatype: None,
        }
    }

    /// A typed literal.
    #[must_use]
    pub fn typed(value: impl Into<String>, datatype: &str) -> Self {
        Self::Literal {
            value: value.into(),
            lang: None,
            datatype: Some(datatype.to_owned()),
        }
    }
}

/// An RDF subject: an IRI or a blank node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subject {
    /// An absolute IRI.
    Iri(String),
    /// A blank node.
    Blank(String),
}

/// A set of triples with deterministic iteration order.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    triples: BTreeMap<Subject, BTreeMap<String, BTreeSet<Term>>>,
    count: usize,
    next_bnode: usize,
}

impl Graph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct triples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// `true` when the graph holds no triples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Add a triple, ignoring duplicates.
    pub fn add(&mut self, subject: Subject, predicate: impl Into<String>, object: Term) {
        if self
            .triples
            .entry(subject)
            .or_default()
            .entry(predicate.into())
            .or_default()
            .insert(object)
        {
            self.count += 1;
        }
    }

    /// Add a triple with an IRI subject.
    pub fn add_iri(&mut self, subject: &str, predicate: &str, object: Term) {
        self.add(Subject::Iri(subject.to_owned()), predicate, object);
    }

    /// Mint a fresh blank node label. Labels are sequential so the serialised
    /// output is stable for a given input.
    pub fn fresh_blank(&mut self) -> String {
        self.next_bnode += 1;
        format!("r{}", self.next_bnode)
    }

    /// Every triple as `(subject, predicate, object)`, in canonical order.
    pub fn iter(&self) -> impl Iterator<Item = (&Subject, &str, &Term)> {
        self.triples.iter().flat_map(|(s, preds)| {
            preds
                .iter()
                .flat_map(move |(p, objs)| objs.iter().map(move |o| (s, p.as_str(), o)))
        })
    }
}

/// Build the asserted graph.
///
/// When `public_only`, private pages contribute nothing — though by the time
/// `vault build` calls this the corpus has already been through
/// [`crate::projection::project`], so the flag is a belt-and-braces second
/// filter rather than the boundary itself.
#[must_use]
#[allow(clippy::too_many_lines)] // One faithful port of one Python function.
pub fn build_graph(corpus: &Corpus, vocab: &Vocabulary, public_only: bool) -> Graph {
    let mut g = Graph::new();

    // Imported external vocabulary, declared so the ontology is self-contained
    // and EL-profile conformant (ROBOT and Whelk require every used term).
    g.add_iri(
        &format!("{SKOS}Concept"),
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}Class")),
    );
    g.add_iri(
        &format!("{SKOS}Concept"),
        &format!("{RDFS}label"),
        Term::lang("Concept", "en"),
    );
    for annotation in [format!("{SKOS}broader"), format!("{DCTERMS}creator")] {
        g.add_iri(
            &annotation,
            &format!("{RDF}type"),
            Term::Iri(format!("{OWL}AnnotationProperty")),
        );
    }

    declare_object_properties(&mut g, vocab);
    declare_annotation_properties(&mut g);
    declare_maturity(&mut g);

    let records: Vec<_> = corpus
        .records
        .iter()
        .filter(|r| (!public_only || r.public) && r.has_ontology && !r.iri.is_empty())
        .collect();

    let declared: BTreeSet<String> = records.iter().map(|r| iri_to_uri(&r.iri)).collect();
    let taxonomic: BTreeSet<&str> = DOMAIN_ROOT_SLUGS
        .iter()
        .chain(CATEGORY_SLUGS.iter())
        .copied()
        .collect();

    for record in &records {
        let uri = iri_to_uri(&record.iri);
        let entity_slug = slug_from_uri(&uri);

        if record.entity_type == EntityType::Individual {
            g.add_iri(
                &uri,
                &format!("{RDF}type"),
                Term::Iri(format!("{OWL}NamedIndividual")),
            );
            for cls in record.instance_of.iter().chain(record.sub_class_of.iter()) {
                g.add_iri(&uri, &format!("{RDF}type"), Term::Iri(iri_to_uri(&cls.iri)));
            }
        } else {
            g.add_iri(
                &uri,
                &format!("{RDF}type"),
                Term::Iri(format!("{OWL}Class")),
            );
            if taxonomic.contains(entity_slug.as_str()) {
                g.add_iri(
                    &uri,
                    &format!("{RDF}type"),
                    Term::Iri(format!("{SKOS}Concept")),
                );
            }
            for parent in &record.sub_class_of {
                let parent_uri = iri_to_uri(&parent.iri);
                if taxonomic.contains(slug_from_uri(&parent_uri).as_str()) {
                    g.add_iri(
                        &uri,
                        &format!("{SKOS}broader"),
                        Term::Iri(parent_uri.clone()),
                    );
                }
                g.add_iri(&uri, &format!("{RDFS}subClassOf"), Term::Iri(parent_uri));
            }
        }

        g.add_iri(
            &uri,
            &format!("{RDFS}label"),
            Term::lang(&record.label, "en"),
        );
        if !record.definition.is_empty() {
            g.add_iri(
                &uri,
                &format!("{RDFS}comment"),
                Term::lang(&record.definition, "en"),
            );
        }
        g.add_iri(
            &uri,
            &format!("{VC}sourceDomain"),
            Term::plain(&record.domain),
        );
        g.add_iri(
            &uri,
            &format!("{VC}qualityScore"),
            Term::typed(format_float(record.quality), &format!("{XSD}float")),
        );
        let mat = record.maturity.trim().to_lowercase();
        let mat = if MATURITY_LEVELS.iter().any(|(s, _)| *s == mat) {
            mat
        } else {
            "draft".to_owned()
        };
        g.add_iri(
            &uri,
            &format!("{VC}hasMaturity"),
            Term::Iri(format!("{NGMI}maturity-{mat}")),
        );
        g.add_iri(&uri, &format!("{VC}slug"), Term::plain(&record.slug));

        for (fm_key, json_key) in crate::model::RELATION_KEYS {
            let property = relation_property(vocab, fm_key, json_key);
            for r in record.relation(json_key) {
                g.add_iri(&uri, &property, Term::Iri(iri_to_uri(&r.iri)));
            }
        }
    }

    // Existential restrictions for the relations the vocabulary flags
    // `restriction: true` (an absent flag is false), where both endpoints are
    // declared. Whelk uses these for subsumption. Vocabulary order fixes the
    // blank-node order.
    let restricted: Vec<(String, String)> = vocab
        .restriction_relations()
        .into_iter()
        .map(|fm_key| {
            let json_key = vocab.json_key(fm_key);
            let property = relation_property(vocab, fm_key, &json_key);
            (json_key, property)
        })
        .collect();
    for record in &records {
        if record.entity_type == EntityType::Individual {
            continue;
        }
        let uri = iri_to_uri(&record.iri);
        for (json_key, property) in &restricted {
            for r in record.relation(json_key) {
                let target = iri_to_uri(&r.iri);
                if !declared.contains(&target) {
                    continue;
                }
                let b = g.fresh_blank();
                let node = Subject::Blank(b.clone());
                g.add(
                    node.clone(),
                    format!("{RDF}type"),
                    Term::Iri(format!("{OWL}Restriction")),
                );
                g.add(
                    node.clone(),
                    format!("{OWL}onProperty"),
                    Term::Iri(property.clone()),
                );
                g.add(node, format!("{OWL}someValuesFrom"), Term::Iri(target));
                g.add_iri(&uri, &format!("{RDFS}subClassOf"), Term::Blank(b));
            }
        }
    }

    // Sibling disjointness (ADR-2125): `disjoint-with` targets, emitted as
    // `owl:disjointWith` only when the vocabulary registers the key, both
    // endpoints are declared classes, and neither is a domain root or taxonomy
    // category. `vault validate` refuses the non-sibling cases with
    // `DISJOINT_NOT_SIBLINGS`; this filter guarantees the roots can never
    // receive the axiom even from an unvalidated corpus.
    if vocab.is_emitted_relation(vault_core::consistency::DISJOINT_WITH_KEY) {
        for record in &records {
            if record.entity_type == EntityType::Individual
                || vault_core::consistency::is_taxonomic(&record.iri)
            {
                continue;
            }
            let uri = iri_to_uri(&record.iri);
            for r in record.relation(crate::model::DISJOINT_WITH_JSON_KEY) {
                let target = iri_to_uri(&r.iri);
                if target == uri
                    || !declared.contains(&target)
                    || vault_core::consistency::is_taxonomic(&target)
                {
                    continue;
                }
                g.add_iri(
                    &uri,
                    vault_core::consistency::OWL_DISJOINT_WITH,
                    Term::Iri(target),
                );
            }
        }
    }

    // Curated EL definitions (ADR-2124): `defines-as` becomes
    // `C owl:equivalentClass (N₁ ⊓ … ⊓ ∃p.F …)`, only when the vocabulary
    // registers and emits the key. A definition is all or nothing: one term
    // over an undeclared class or an unusable relation drops it whole, since a
    // weakened definition classifies more than its author meant. `vault
    // validate` refuses those cases as `NON_EL_DEFINITION`. Emitted after
    // every other blank node, so a corpus without definitions keeps its
    // blank-node labels — and its version IRI — unchanged.
    if vocab.is_emitted_relation(vault_core::definition::DEFINES_AS_KEY) {
        for record in &records {
            if record.entity_type == EntityType::Individual || record.defines_as.is_empty() {
                continue;
            }
            let Some(members) = definition_members(&record.defines_as, vocab, &declared) else {
                continue;
            };
            let expression = class_expression(&mut g, members);
            g.add_iri(
                &iri_to_uri(&record.iri),
                vault_core::definition::OWL_EQUIVALENT_CLASS,
                expression,
            );
        }
    }

    // Single-ref tail policy: an ngm: IRI that is only ever an object property
    // target becomes a declared skos:Concept stub rather than a dangling class.
    let object_props: BTreeSet<String> = [
        "hasPart",
        "isPartOf",
        "requires",
        "enables",
        "enabledBy",
        "dependsOn",
        "implements",
        "contrastsWith",
        "bridgesTo",
        "uses",
        "relatedTo",
        "supports",
        "standardizedBy",
    ]
    .iter()
    .map(|p| format!("{VC}{p}"))
    .collect();
    let mut tail: BTreeSet<String> = BTreeSet::new();
    for (_, predicate, object) in g.iter() {
        if let Term::Iri(target) = object {
            if object_props.contains(predicate)
                && target.starts_with(NGM)
                && !declared.contains(target)
            {
                tail.insert(target.clone());
            }
        }
    }
    for target in tail {
        let label = label_from_slug(&slug_from_uri(&target));
        g.add_iri(
            &target,
            &format!("{RDF}type"),
            Term::Iri(format!("{SKOS}Concept")),
        );
        g.add_iri(&target, &format!("{RDFS}label"), Term::lang(label, "en"));
    }

    // The header goes on last: its `owl:versionIRI` addresses everything
    // above it, and none of it addresses itself (ADR-2128).
    let version_iri = format!("{ONTOLOGY_IRI}/{}", ontology_digest(&g));
    g.add_iri(
        ONTOLOGY_IRI,
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}Ontology")),
    );
    g.add_iri(
        ONTOLOGY_IRI,
        &format!("{RDFS}label"),
        Term::lang("NarrativeGoldmine Ontology", "en"),
    );
    g.add_iri(
        ONTOLOGY_IRI,
        &format!("{OWL}versionInfo"),
        Term::plain(VERSION_INFO),
    );
    g.add_iri(
        ONTOLOGY_IRI,
        &format!("{OWL}versionIRI"),
        Term::Iri(version_iri),
    );
    g.add_iri(
        ONTOLOGY_IRI,
        &format!("{DCTERMS}creator"),
        Term::plain("Dr John O'Hare"),
    );

    g
}

/// One conjunct of a definition, ready to emit.
enum Member {
    Named(String),
    Some { property: String, filler: String },
}

/// The conjuncts of `definition` as emittable members, or `None` when any
/// term names an undeclared class or a relation that is not an emitted object
/// property ([`Vocabulary::existential_property`]).
fn definition_members(
    definition: &[crate::model::Conjunct],
    vocab: &Vocabulary,
    declared: &BTreeSet<String>,
) -> Option<Vec<Member>> {
    use crate::model::Conjunct;
    definition
        .iter()
        .map(|conjunct| {
            let target = iri_to_uri(&conjunct.target().iri);
            if !declared.contains(&target) {
                return None;
            }
            Some(match conjunct {
                Conjunct::Named(_) => Member::Named(target),
                Conjunct::Some { relation, .. } => Member::Some {
                    property: vocab.existential_property(relation)?,
                    filler: target,
                },
            })
        })
        .collect()
}

/// Write `members` as one class expression and return its term: the lone
/// member itself, or an `owl:intersectionOf` RDF list (OWL 2 requires at least
/// two operands for an intersection).
fn class_expression(g: &mut Graph, members: Vec<Member>) -> Term {
    let mut terms: Vec<Term> = members
        .into_iter()
        .map(|member| match member {
            Member::Named(iri) => Term::Iri(iri),
            Member::Some { property, filler } => {
                let b = g.fresh_blank();
                let node = Subject::Blank(b.clone());
                g.add(
                    node.clone(),
                    format!("{RDF}type"),
                    Term::Iri(format!("{OWL}Restriction")),
                );
                g.add(
                    node.clone(),
                    format!("{OWL}onProperty"),
                    Term::Iri(property),
                );
                g.add(node, format!("{OWL}someValuesFrom"), Term::Iri(filler));
                Term::Blank(b)
            }
        })
        .collect();
    if terms.len() == 1 {
        return terms.remove(0);
    }
    let mut rest = Term::Iri(format!("{RDF}nil"));
    for term in terms.into_iter().rev() {
        let cell = g.fresh_blank();
        g.add(Subject::Blank(cell.clone()), format!("{RDF}first"), term);
        g.add(Subject::Blank(cell.clone()), format!("{RDF}rest"), rest);
        rest = Term::Blank(cell);
    }
    let b = g.fresh_blank();
    let node = Subject::Blank(b.clone());
    g.add(
        node.clone(),
        format!("{RDF}type"),
        Term::Iri(format!("{OWL}Class")),
    );
    g.add(node, format!("{OWL}intersectionOf"), rest);
    Term::Blank(b)
}

/// The ADR-2023 content address (`sha256-12-<hex>`) of `g`'s emitted triples,
/// excluding every triple whose subject is the ontology header
/// ([`ONTOLOGY_IRI`]) — so the header can carry it as `owl:versionIRI`.
///
/// The input is a canonical serialisation: one N-Triples-shaped line per
/// triple in the graph's own sorted order, so the digest depends on the
/// triple set (and the deterministic blank-node labels), never on Turtle
/// pretty-printing. A page change, a vocabulary change and an emitter change
/// all move it; an identical rebuild does not.
#[must_use]
pub fn ontology_digest(g: &Graph) -> String {
    let header = Subject::Iri(ONTOLOGY_IRI.to_owned());
    let mut canonical = String::new();
    for (subject, predicate, object) in g.iter() {
        if *subject == header {
            continue;
        }
        let s = match subject {
            Subject::Iri(iri) => format!("<{iri}>"),
            Subject::Blank(id) => format!("_:{id}"),
        };
        let o = match object {
            Term::Iri(iri) => format!("<{iri}>"),
            Term::Blank(id) => format!("_:{id}"),
            Term::Literal {
                value,
                lang,
                datatype,
            } => {
                let escaped = escape_literal(value);
                match (lang, datatype) {
                    (Some(l), _) => format!("\"{escaped}\"@{l}"),
                    (None, Some(d)) => format!("\"{escaped}\"^^<{d}>"),
                    (None, None) => format!("\"{escaped}\""),
                }
            }
        };
        let _ = writeln!(canonical, "{s} <{predicate}> {o} .");
    }
    super::generation::sha256_12(canonical.as_bytes())
}

/// The version IRI `g`'s header declares, if it has one.
#[must_use]
pub fn version_iri(g: &Graph) -> Option<&str> {
    let header = Subject::Iri(ONTOLOGY_IRI.to_owned());
    let predicate = format!("{OWL}versionIRI");
    g.iter().find_map(|(s, p, o)| match o {
        Term::Iri(iri) if *s == header && p == predicate => Some(iri.as_str()),
        _ => None,
    })
}

/// The OWL property IRI for a relation key: the vocabulary's declaration when
/// there is one, else the frozen `vc:` name.
fn relation_property(vocab: &Vocabulary, fm_key: &str, json_key: &str) -> String {
    vocab.relations.get(fm_key).map_or_else(
        || {
            let name = if json_key == "partOf" {
                "isPartOf"
            } else {
                json_key
            };
            format!("{VC}{name}")
        },
        |d| vocab.expand(&d.owl),
    )
}

fn declare_object_properties(g: &mut Graph, vocab: &Vocabulary) {
    let simple = [
        "hasPart",
        "isPartOf",
        "enables",
        "enabledBy",
        "implements",
        "uses",
        "supports",
        "standardizedBy",
        "contrastsWith",
        "bridgesTo",
        "relatedTo",
        "utilises",
    ];
    for name in simple {
        declare_property(g, vocab, name, false);
    }
    // requires and dependsOn are transitive.
    declare_property(g, vocab, "requires", true);
    declare_property(g, vocab, "dependsOn", true);

    g.add_iri(
        &format!("{VC}requires"),
        &format!("{RDFS}subPropertyOf"),
        Term::Iri(format!("{VC}dependsOn")),
    );
    for name in ["uses", "supports", "implements"] {
        g.add_iri(
            &format!("{VC}{name}"),
            &format!("{RDFS}subPropertyOf"),
            Term::Iri(format!("{VC}utilises")),
        );
    }

    // ADR-2128: a signature on any other emitted relation is emitted too, not
    // only on the fourteen above. Logical relations cannot carry one (the
    // vocabulary load refuses `SIGNATURE_ON_LOGICAL_RELATION`); the filter
    // keeps the emitter safe regardless. The graph deduplicates, so the
    // fourteen are harmlessly revisited.
    for def in vocab.relations.values().filter(|d| d.emitted) {
        if vocab.is_logical(&def.owl) {
            continue;
        }
        let uri = vocab.expand(def.owl.trim());
        for (predicate, class) in [("domain", &def.domain), ("range", &def.range)] {
            if let Some(class) = scoped_class(class.as_ref()) {
                g.add_iri(&uri, &format!("{RDFS}{predicate}"), Term::Iri(class));
            }
        }
    }
}

fn declare_property(g: &mut Graph, vocab: &Vocabulary, name: &str, transitive: bool) {
    let uri = format!("{VC}{name}");
    g.add_iri(
        &uri,
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}ObjectProperty")),
    );
    if transitive {
        g.add_iri(
            &uri,
            &format!("{RDF}type"),
            Term::Iri(format!("{OWL}TransitiveProperty")),
        );
    }
    g.add_iri(&uri, &format!("{RDFS}label"), Term::lang(name, "en"));
    // ADR-2128: no blanket `owl:Thing` signature. A domain or range is emitted
    // only when the vocabulary scopes this property to a class.
    let (domain, range) = property_signature(vocab, &uri);
    if let Some(class) = domain {
        g.add_iri(&uri, &format!("{RDFS}domain"), Term::Iri(class));
    }
    if let Some(class) = range {
        g.add_iri(&uri, &format!("{RDFS}range"), Term::Iri(class));
    }
}

/// The `(domain, range)` class IRIs the vocabulary declares for the property
/// `uri` — from the relation that emits it, else from a super-property entry.
/// A slug resolves into the class namespace like any page reference; an
/// absent value or `owl:Thing` (a tautology under OWL) yields `None`.
fn property_signature(vocab: &Vocabulary, uri: &str) -> (Option<String>, Option<String>) {
    let scoped = scoped_class;
    if let Some(def) = vocab
        .relations
        .values()
        .find(|d| vocab.expand(&d.owl) == uri)
    {
        return (scoped(def.domain.as_ref()), scoped(def.range.as_ref()));
    }
    if let Some((_, def)) = vocab
        .super_properties
        .iter()
        .find(|(key, _)| vocab.expand(key) == uri)
    {
        return (scoped(def.domain.as_ref()), scoped(def.range.as_ref()));
    }
    (None, None)
}

/// A declared `domain`/`range` value as a class IRI: a slug resolves into the
/// class namespace like any page reference; absent, blank or `owl:Thing` (a
/// tautology under OWL) is no signature.
fn scoped_class(value: Option<&String>) -> Option<String> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(iri_to_uri)
        .filter(|class| *class != format!("{OWL}Thing"))
}

fn declare_annotation_properties(g: &mut Graph) {
    for name in ["sourceDomain", "qualityScore", "slug"] {
        g.add_iri(
            &format!("{VC}{name}"),
            &format!("{RDF}type"),
            Term::Iri(format!("{OWL}AnnotationProperty")),
        );
    }
}

fn declare_maturity(g: &mut Graph) {
    let has_maturity = format!("{VC}hasMaturity");
    g.add_iri(
        &has_maturity,
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}ObjectProperty")),
    );
    g.add_iri(
        &has_maturity,
        &format!("{RDFS}label"),
        Term::lang("hasMaturity", "en"),
    );
    let maturity_class = format!("{NGM}MaturityLevel");
    g.add_iri(
        &maturity_class,
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}Class")),
    );
    g.add_iri(
        &maturity_class,
        &format!("{RDFS}label"),
        Term::lang("Maturity Level", "en"),
    );
    g.add_iri(
        &has_maturity,
        &format!("{RDFS}range"),
        Term::Iri(maturity_class.clone()),
    );
    for (slug, label) in MATURITY_LEVELS {
        let uri = format!("{NGMI}maturity-{slug}");
        g.add_iri(
            &uri,
            &format!("{RDF}type"),
            Term::Iri(format!("{OWL}NamedIndividual")),
        );
        g.add_iri(
            &uri,
            &format!("{RDF}type"),
            Term::Iri(maturity_class.clone()),
        );
        g.add_iri(&uri, &format!("{RDFS}label"), Term::lang(*label, "en"));
    }
}

/// Map a corpus IRI onto its HTTP projection, exactly as
/// `jsonld_to_turtle._iri_to_uriref` did.
#[must_use]
pub fn iri_to_uri(iri: &str) -> String {
    const MAPPINGS: &[(&str, &str)] = &[
        ("urn:ngm:class:", NGM),
        ("urn:ngm:individual:", NGMI),
        ("urn:visionflow:owl:class:", NGM),
        (
            "urn:visionflow:linked:",
            "https://narrativegoldmine.com/linked/",
        ),
        (
            "urn:visionflow:page:",
            "https://narrativegoldmine.com/page/",
        ),
    ];
    if iri == "owl:Thing" {
        return format!("{OWL}Thing");
    }
    for (prefix, base) in MAPPINGS {
        if let Some(tail) = iri.strip_prefix(prefix) {
            return format!("{base}{tail}");
        }
    }
    if iri.starts_with("http") {
        return iri.to_owned();
    }
    format!("{NGM}{iri}")
}

fn slug_from_uri(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or(uri).to_owned()
}

/// Acronyms the label reconstruction knows about.
const ACRONYMS: &[(&str, &str)] = &[
    ("ai", "AI"),
    ("api", "API"),
    ("rag", "RAG"),
    ("bip", "BIP"),
    ("erc", "ERC"),
    ("w3c", "W3C"),
    ("nft", "NFT"),
    ("defi", "DeFi"),
    ("llm", "LLM"),
    ("av1", "AV1"),
    ("brdf", "BRDF"),
    ("iot", "IoT"),
    ("crdt", "CRDT"),
    ("dao", "DAO"),
    ("xr", "XR"),
    ("ar", "AR"),
    ("vr", "VR"),
    ("cbdc", "CBDC"),
    ("p2p", "P2P"),
    ("sdk", "SDK"),
    ("url", "URL"),
    ("did", "DID"),
    ("zk", "ZK"),
    ("ml", "ML"),
    ("nlp", "NLP"),
];

/// Reconstruct a display label from a slug (`_label_from_slug`).
#[must_use]
pub fn label_from_slug(slug: &str) -> String {
    slug.split('-')
        .map(|part| {
            ACRONYMS
                .iter()
                .find(|(k, _)| *k == part)
                .map_or_else(|| capitalise(part), |(_, v)| (*v).to_owned())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Python's `str.capitalize()`: first character upper, the rest lower.
fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
    }
}

/// `xsd:float` lexical form: `rdflib` writes a Python float's `repr`, which
/// always carries a decimal point.
fn format_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        let s = v.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

/// Serialise the graph as Turtle.
///
/// Output is deterministic: prefixes in a fixed order, then subjects, their
/// predicates and objects in the graph's canonical order.
#[must_use]
pub fn serialise(g: &Graph) -> String {
    const PREFIXES: &[(&str, &str)] = &[
        ("dc", DCTERMS),
        ("ngm", NGM),
        ("ngmi", NGMI),
        ("owl", OWL),
        ("prov", "http://www.w3.org/ns/prov#"),
        ("rdf", RDF),
        ("rdfs", RDFS),
        ("skos", SKOS),
        ("vc", VC),
        ("xsd", XSD),
    ];
    let mut out = String::new();
    for (prefix, base) in PREFIXES {
        let _ = writeln!(out, "@prefix {prefix}: <{base}> .");
    }
    out.push('\n');

    for (subject, preds) in &g.triples {
        let s = match subject {
            Subject::Iri(iri) => shorten(iri, PREFIXES),
            Subject::Blank(id) => format!("_:{id}"),
        };
        let _ = write!(out, "{s}");
        let mut first = true;
        for (predicate, objects) in preds {
            for object in objects {
                let sep = if first { " " } else { " ;\n    " };
                first = false;
                let _ = write!(
                    out,
                    "{sep}{} {}",
                    shorten(predicate, PREFIXES),
                    render_term(object, PREFIXES)
                );
            }
        }
        out.push_str(" .\n");
    }
    out
}

fn render_term(t: &Term, prefixes: &[(&str, &str)]) -> String {
    match t {
        Term::Iri(iri) => shorten(iri, prefixes),
        Term::Blank(id) => format!("_:{id}"),
        Term::Literal {
            value,
            lang,
            datatype,
        } => {
            let escaped = escape_literal(value);
            match (lang, datatype) {
                (Some(l), _) => format!("\"{escaped}\"@{l}"),
                (None, Some(d)) => format!("\"{escaped}\"^^{}", shorten(d, prefixes)),
                (None, None) => format!("\"{escaped}\""),
            }
        }
    }
}

fn escape_literal(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Shorten an IRI to `prefix:local` when the local part is a legal `PN_LOCAL`,
/// else write it in angle brackets.
fn shorten(iri: &str, prefixes: &[(&str, &str)]) -> String {
    if iri == format!("{RDF}type") {
        return "a".to_owned();
    }
    for (prefix, base) in prefixes {
        if let Some(local) = iri.strip_prefix(*base) {
            if !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
                && !local.starts_with('.')
                && !local.ends_with('.')
            {
                return format!("{prefix}:{local}");
            }
        }
    }
    format!("<{iri}>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ref;
    use indexmap::IndexMap;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires", restriction: true }
  has-part: { owl: "vc:hasPart", restriction: true }
  part-of:  { owl: "vc:isPartOf" }
"#,
        )
        .unwrap()
    }

    /// The same relations with no `restriction` flag anywhere.
    fn vocab_without_restrictions() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires" }
  has-part: { owl: "vc:hasPart" }
  part-of:  { owl: "vc:isPartOf", restriction: true }
"#,
        )
        .unwrap()
    }

    /// Every `owl:onProperty` a restriction in `g` points at.
    fn restricted_properties(g: &Graph) -> BTreeSet<String> {
        g.iter()
            .filter(|(_, p, _)| *p == format!("{OWL}onProperty"))
            .filter_map(|(_, _, o)| match o {
                Term::Iri(i) => Some(i.clone()),
                _ => None,
            })
            .collect()
    }

    /// A pair of declared classes linked by `requires`, `has-part` and
    /// `part-of`, so each relation has a candidate restriction.
    fn linked_pair() -> crate::model::Corpus {
        let target = || {
            vec![Ref {
                iri: "urn:ngm:class:b".into(),
                label: "B".into(),
            }]
        };
        let mut a = record("A");
        a.relations.insert("requires", target());
        a.relations.insert("hasPart", target());
        a.relations.insert("partOf", target());
        corpus_of(vec![a, record("B")])
    }

    #[test]
    fn restrictions_follow_the_vocabulary_flag() {
        let flagged = build_graph(&linked_pair(), &vocab(), true);
        assert_eq!(
            restricted_properties(&flagged),
            BTreeSet::from([format!("{VC}hasPart"), format!("{VC}requires")]),
            "exactly the relations flagged `restriction: true`"
        );
    }

    #[test]
    fn an_absent_restriction_flag_means_no_restriction() {
        let g = build_graph(&linked_pair(), &vocab_without_restrictions(), true);
        assert_eq!(
            restricted_properties(&g),
            BTreeSet::from([format!("{VC}isPartOf")]),
            "requires/has-part lose their restriction once unflagged; part-of gains one"
        );
    }

    fn record(id: &str) -> crate::model::ClassRecord {
        crate::model::ClassRecord {
            page_id: id.to_owned(),
            slug: vault_core::slug::slugify(id),
            title: id.to_owned(),
            public: true,
            page_iri: String::new(),
            iri: format!("urn:ngm:class:{}", vault_core::slug::slugify(id)),
            label: id.to_owned(),
            entity_type: EntityType::Class,
            domain: "spatial-computing".into(),
            definition: String::new(),
            maturity: "established".into(),
            quality: 0.35,
            legacy_term_id: String::new(),
            sub_class_of: Vec::new(),
            instance_of: Vec::new(),
            relations: IndexMap::new(),
            links: Vec::new(),
            body: String::new(),
            has_ontology: true,
            defines_as: Vec::new(),
        }
    }

    fn corpus_of(records: Vec<crate::model::ClassRecord>) -> Corpus {
        let mut by_class_slug = IndexMap::new();
        for (i, r) in records.iter().enumerate() {
            by_class_slug.entry(r.class_slug()).or_insert(i);
        }
        Corpus {
            records,
            by_class_slug,
        }
    }

    #[test]
    fn maps_urns_onto_the_http_projection() {
        assert_eq!(iri_to_uri("urn:ngm:class:x"), format!("{NGM}x"));
        assert_eq!(iri_to_uri("urn:ngm:individual:y"), format!("{NGMI}y"));
        assert_eq!(iri_to_uri("owl:Thing"), format!("{OWL}Thing"));
        assert_eq!(iri_to_uri("https://example.org/z"), "https://example.org/z");
        assert_eq!(iri_to_uri("bare"), format!("{NGM}bare"));
    }

    #[test]
    fn rebuilds_labels_with_acronyms() {
        assert_eq!(label_from_slug("ai-reasoning"), "AI Reasoning");
        assert_eq!(label_from_slug("knowledge-graph"), "Knowledge Graph");
        assert_eq!(label_from_slug("defi-protocol"), "DeFi Protocol");
    }

    #[test]
    fn emits_a_class_with_its_annotations() {
        let g = build_graph(&corpus_of(vec![record("Knowledge Graph")]), &vocab(), true);
        let ttl = serialise(&g);
        assert!(ttl.contains("ngm:knowledge-graph a owl:Class"));
        assert!(ttl.contains("vc:sourceDomain \"spatial-computing\""));
        assert!(ttl.contains("vc:hasMaturity ngmi:maturity-established"));
        assert!(ttl.contains("vc:qualityScore \"0.35\"^^xsd:float"));
    }

    #[test]
    fn a_taxonomic_parent_also_gets_skos_broader() {
        let mut child = record("Child");
        child.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:spatial-computing".into(),
            label: "Spatial Computing".into(),
        }];
        let g = build_graph(&corpus_of(vec![child]), &vocab(), true);
        let ttl = serialise(&g);
        assert!(ttl.contains("skos:broader ngm:spatial-computing"));
        assert!(ttl.contains("rdfs:subClassOf ngm:spatial-computing"));
    }

    #[test]
    fn a_non_taxonomic_parent_gets_only_subclassof() {
        let mut child = record("Child");
        child.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:ordinary".into(),
            label: "Ordinary".into(),
        }];
        let ttl = serialise(&build_graph(&corpus_of(vec![child]), &vocab(), true));
        assert!(!ttl.contains("skos:broader ngm:ordinary"));
    }

    #[test]
    fn an_undeclared_relation_target_becomes_a_skos_concept_stub() {
        let mut r = record("A");
        r.relations.insert(
            "uses",
            vec![Ref {
                iri: "urn:ngm:class:tail-concept".into(),
                label: "Tail Concept".into(),
            }],
        );
        let ttl = serialise(&build_graph(&corpus_of(vec![r]), &vocab(), true));
        assert!(ttl.contains("ngm:tail-concept a skos:Concept"));
        assert!(ttl.contains("\"Tail Concept\"@en"));
    }

    #[test]
    fn an_existential_restriction_needs_both_endpoints_declared() {
        let mut a = record("A");
        a.relations.insert(
            "requires",
            vec![
                Ref {
                    iri: "urn:ngm:class:b".into(),
                    label: "B".into(),
                },
                Ref {
                    iri: "urn:ngm:class:nowhere".into(),
                    label: "Nowhere".into(),
                },
            ],
        );
        let g = build_graph(&corpus_of(vec![a, record("B")]), &vocab(), true);
        let restrictions = g
            .iter()
            .filter(|(_, p, o)| {
                *p == format!("{OWL}someValuesFrom") && matches!(o, Term::Iri(_)) && {
                    let Term::Iri(i) = o else { unreachable!() };
                    i.ends_with("/b")
                }
            })
            .count();
        assert_eq!(restrictions, 1);
        assert_eq!(
            g.iter()
                .filter(|(_, p, _)| *p == format!("{OWL}Restriction"))
                .count(),
            0
        );
    }

    #[test]
    fn requires_and_dependson_are_transitive() {
        let ttl = serialise(&build_graph(&corpus_of(vec![]), &vocab(), true));
        assert!(ttl.contains("owl:TransitiveProperty"));
        assert!(ttl.contains("rdfs:subPropertyOf vc:dependsOn"));
        let g = build_graph(&corpus_of(vec![]), &vocab(), true);
        assert!(g.iter().any(|(s, p, o)| {
            matches!(s, Subject::Iri(i) if *i == format!("{VC}requires"))
                && p == format!("{RDFS}subPropertyOf")
                && *o == Term::Iri(format!("{VC}dependsOn"))
        }));
    }

    /// [`vocab`] plus the ADR-2125 `disjoint-with` registration.
    fn vocab_with_disjointness() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:          { owl: "rdfs:subClassOf" }
  requires:      { owl: "vc:requires", restriction: true }
  disjoint-with: { owl: "owl:disjointWith", status: provisional }
"#,
        )
        .unwrap()
    }

    /// Every `(subject, object)` of an `owl:disjointWith` triple in `g`.
    fn disjoint_pairs(g: &Graph) -> BTreeSet<(String, String)> {
        g.iter()
            .filter(|(_, p, _)| *p == format!("{OWL}disjointWith"))
            .filter_map(|(s, _, o)| match (s, o) {
                (Subject::Iri(s), Term::Iri(o)) => Some((s.clone(), o.clone())),
                _ => None,
            })
            .collect()
    }

    fn disjoint_with(r: &mut crate::model::ClassRecord, targets: &[&str]) {
        r.relations.insert(
            crate::model::DISJOINT_WITH_JSON_KEY,
            targets
                .iter()
                .map(|t| Ref {
                    iri: format!("urn:ngm:class:{t}"),
                    label: (*t).to_owned(),
                })
                .collect(),
        );
    }

    #[test]
    fn disjoint_with_between_declared_classes_is_emitted() {
        let mut a = record("A");
        disjoint_with(&mut a, &["b"]);
        let g = build_graph(
            &corpus_of(vec![a, record("B")]),
            &vocab_with_disjointness(),
            true,
        );
        assert_eq!(
            disjoint_pairs(&g),
            BTreeSet::from([(format!("{NGM}a"), format!("{NGM}b"))])
        );
        assert!(serialise(&g).contains("owl:disjointWith ngm:b"));
    }

    #[test]
    fn an_unregistered_disjoint_with_key_emits_nothing() {
        let mut a = record("A");
        disjoint_with(&mut a, &["b"]);
        let g = build_graph(&corpus_of(vec![a, record("B")]), &vocab(), true);
        assert!(disjoint_pairs(&g).is_empty());
    }

    #[test]
    fn an_undeclared_disjoint_target_emits_nothing() {
        let mut a = record("A");
        disjoint_with(&mut a, &["nowhere"]);
        let g = build_graph(&corpus_of(vec![a]), &vocab_with_disjointness(), true);
        assert!(disjoint_pairs(&g).is_empty());
    }

    // ── ADR-2124: `defines-as` → owl:equivalentClass ───────────────────────

    fn vocab_with_definitions() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:       { owl: "rdfs:subClassOf" }
  has-part:   { owl: "vc:hasPart", restriction: true }
  produces:   { owl: "vc:produces", emitted: false, status: provisional }
  defines-as: { owl: "owl:equivalentClass", status: provisional }
"#,
        )
        .unwrap()
    }

    fn named(slug: &str) -> crate::model::Conjunct {
        crate::model::Conjunct::Named(Ref {
            iri: format!("urn:ngm:class:{slug}"),
            label: slug.to_owned(),
        })
    }

    fn some(relation: &str, slug: &str) -> crate::model::Conjunct {
        crate::model::Conjunct::Some {
            relation: relation.to_owned(),
            filler: Ref {
                iri: format!("urn:ngm:class:{slug}"),
                label: slug.to_owned(),
            },
        }
    }

    /// The objects of `subject predicate ?o`.
    fn objects(g: &Graph, subject: &Subject, predicate: &str) -> Vec<Term> {
        g.iter()
            .filter(|(s, p, _)| *s == subject && *p == predicate)
            .map(|(_, _, o)| o.clone())
            .collect()
    }

    fn blank(t: &Term) -> Subject {
        match t {
            Term::Blank(b) => Subject::Blank(b.clone()),
            other => panic!("expected a blank node, got {other:?}"),
        }
    }

    /// Walk an RDF list from its head cell.
    fn list_members(g: &Graph, head: &Term) -> Vec<Term> {
        let mut out = Vec::new();
        let mut cell = head.clone();
        while cell != Term::Iri(format!("{RDF}nil")) {
            let node = blank(&cell);
            out.push(objects(g, &node, &format!("{RDF}first")).remove(0));
            cell = objects(g, &node, &format!("{RDF}rest")).remove(0);
        }
        out
    }

    fn defined_corpus(conjuncts: Vec<crate::model::Conjunct>) -> Corpus {
        let mut d = record("Gripping Robot");
        d.defines_as = conjuncts;
        corpus_of(vec![record("Robot"), record("Gripper"), d])
    }

    #[test]
    fn a_definition_is_emitted_as_an_equivalent_intersection() {
        let g = build_graph(
            &defined_corpus(vec![named("robot"), some("has-part", "gripper")]),
            &vocab_with_definitions(),
            true,
        );
        let defined = Subject::Iri(format!("{NGM}gripping-robot"));
        let eq = objects(&g, &defined, &format!("{OWL}equivalentClass"));
        assert_eq!(eq.len(), 1, "{eq:?}");
        let node = blank(&eq[0]);
        assert_eq!(
            objects(&g, &node, &format!("{RDF}type")),
            vec![Term::Iri(format!("{OWL}Class"))]
        );
        let head = objects(&g, &node, &format!("{OWL}intersectionOf")).remove(0);
        let members = list_members(&g, &head);
        assert_eq!(members.len(), 2, "{members:?}");
        assert_eq!(members[0], Term::Iri(format!("{NGM}robot")));
        let restriction = blank(&members[1]);
        assert_eq!(
            objects(&g, &restriction, &format!("{OWL}onProperty")),
            vec![Term::Iri(format!("{VC}hasPart"))]
        );
        assert_eq!(
            objects(&g, &restriction, &format!("{OWL}someValuesFrom")),
            vec![Term::Iri(format!("{NGM}gripper"))]
        );
        assert_eq!(
            objects(&g, &restriction, &format!("{RDF}type")),
            vec![Term::Iri(format!("{OWL}Restriction"))]
        );
        // The definition is not also asserted as a superclass.
        assert_eq!(objects(&g, &defined, &format!("{RDFS}subClassOf")), []);
        let ttl = serialise(&g);
        assert!(ttl.contains("owl:equivalentClass"), "{ttl}");
        assert!(ttl.contains("owl:intersectionOf"), "{ttl}");
    }

    #[test]
    fn a_lone_conjunct_is_the_equivalent_class_itself() {
        let g = build_graph(
            &defined_corpus(vec![some("has-part", "gripper")]),
            &vocab_with_definitions(),
            true,
        );
        let defined = Subject::Iri(format!("{NGM}gripping-robot"));
        let eq = objects(&g, &defined, &format!("{OWL}equivalentClass"));
        assert_eq!(eq.len(), 1);
        let node = blank(&eq[0]);
        assert_eq!(objects(&g, &node, &format!("{OWL}intersectionOf")), []);
        assert_eq!(
            objects(&g, &node, &format!("{OWL}someValuesFrom")),
            vec![Term::Iri(format!("{NGM}gripper"))]
        );
    }

    #[test]
    fn an_unregistered_defines_as_key_emits_nothing() {
        let g = build_graph(
            &defined_corpus(vec![named("robot"), some("has-part", "gripper")]),
            &vocab(),
            true,
        );
        assert!(!serialise(&g).contains("equivalentClass"));
    }

    /// A definition is all or nothing: dropping one conjunct would *weaken*
    /// it and classify more classes than the author meant.
    #[test]
    fn a_definition_with_an_undeclared_or_unusable_term_is_dropped_whole() {
        for conjuncts in [
            vec![named("robot"), some("has-part", "nowhere")],
            vec![named("nowhere"), some("has-part", "gripper")],
            vec![named("robot"), some("produces", "gripper")],
            vec![named("robot"), some("is-a", "gripper")],
            vec![named("robot"), some("undeclared", "gripper")],
        ] {
            let g = build_graph(
                &defined_corpus(conjuncts.clone()),
                &vocab_with_definitions(),
                true,
            );
            let ttl = serialise(&g);
            assert!(!ttl.contains("equivalentClass"), "{conjuncts:?}");
            assert!(!ttl.contains("intersectionOf"), "{conjuncts:?}");
        }
    }

    #[test]
    fn an_individual_is_never_defined() {
        let mut d = record("R2");
        d.entity_type = EntityType::Individual;
        d.defines_as = vec![named("robot")];
        let g = build_graph(
            &corpus_of(vec![record("Robot"), d]),
            &vocab_with_definitions(),
            true,
        );
        assert!(!serialise(&g).contains("equivalentClass"));
    }

    /// Replaces `no_domain_disjointness_is_emitted` (ADR-2125), which only
    /// checked `AllDisjointClasses` on an empty corpus. Every domain root and
    /// taxonomy category is declared, each is made disjoint with an ordinary
    /// class and vice versa, and not one `owl:disjointWith` may reach them.
    #[test]
    fn domain_roots_never_receive_owl_disjoint_with() {
        let taxonomic: Vec<&str> = DOMAIN_ROOT_SLUGS
            .iter()
            .chain(CATEGORY_SLUGS)
            .copied()
            .collect();
        let mut x = record("X");
        disjoint_with(&mut x, &taxonomic);
        let mut records = vec![x];
        for slug in &taxonomic {
            let mut r = record(slug);
            disjoint_with(&mut r, &["x"]);
            records.push(r);
        }
        let g = build_graph(&corpus_of(records), &vocab_with_disjointness(), true);
        let pairs = disjoint_pairs(&g);
        assert!(pairs.is_empty(), "{pairs:?}");
        assert!(!serialise(&g).contains("AllDisjointClasses"));
    }

    #[test]
    fn private_records_contribute_nothing_when_public_only() {
        let mut private = record("Secret");
        private.public = false;
        let ttl = serialise(&build_graph(&corpus_of(vec![private]), &vocab(), true));
        assert!(!ttl.contains("ngm:secret "));
    }

    #[test]
    fn float_formatting_always_carries_a_decimal_point() {
        assert_eq!(format_float(0.0), "0.0");
        assert_eq!(format_float(1.0), "1.0");
        assert_eq!(format_float(0.35), "0.35");
    }

    // ── ADR-2128: version IRI and scoped property signatures ─────────────

    /// The object of the ontology header's `owl:versionIRI`, if any.
    fn version_iri_of(g: &Graph) -> Option<String> {
        g.iter().find_map(|(s, p, o)| match (s, o) {
            (Subject::Iri(s), Term::Iri(o))
                if s == ONTOLOGY_IRI && p == format!("{OWL}versionIRI") =>
            {
                Some(o.clone())
            }
            _ => None,
        })
    }

    #[test]
    fn the_header_carries_a_content_addressed_version_iri() {
        let g = build_graph(&linked_pair(), &vocab(), true);
        let iri = version_iri_of(&g).expect("owl:versionIRI is emitted");
        let digest = ontology_digest(&g);
        assert_eq!(iri, format!("{ONTOLOGY_IRI}/{digest}"));
        let hex = digest.strip_prefix("sha256-12-").expect("ADR-2023 grammar");
        assert_eq!(hex.len(), 12);
        assert!(hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn an_identical_rebuild_keeps_the_version_iri() {
        let one = build_graph(&linked_pair(), &vocab(), true);
        let two = build_graph(&linked_pair(), &vocab(), true);
        assert_eq!(version_iri_of(&one), version_iri_of(&two));
        assert_eq!(serialise(&one), serialise(&two));
    }

    #[test]
    fn a_page_change_changes_the_version_iri() {
        let before = build_graph(&linked_pair(), &vocab(), true);
        let mut corpus = linked_pair();
        corpus.records[0].definition = "A changed definition.".into();
        let after = build_graph(&corpus, &vocab(), true);
        assert_ne!(version_iri_of(&before), version_iri_of(&after));
    }

    #[test]
    fn a_vocabulary_only_change_changes_the_version_iri() {
        let corpus = linked_pair();
        let flagged = build_graph(&corpus, &vocab(), true);
        let unflagged = build_graph(&corpus, &vocab_without_restrictions(), true);
        assert_ne!(
            version_iri_of(&flagged),
            version_iri_of(&unflagged),
            "same pages, different vocabulary: a different emitted graph"
        );
    }

    #[test]
    fn the_digest_excludes_the_header_itself() {
        let g = build_graph(&linked_pair(), &vocab(), true);
        let mut without_header = Graph::new();
        for (s, p, o) in g.iter() {
            if *s != Subject::Iri(ONTOLOGY_IRI.to_owned()) {
                without_header.add(s.clone(), p, o.clone());
            }
        }
        assert_eq!(ontology_digest(&g), ontology_digest(&without_header));
    }

    #[test]
    fn version_info_stays_a_semver_string() {
        let g = build_graph(&corpus_of(vec![]), &vocab(), true);
        let info = g
            .iter()
            .find_map(|(s, p, o)| match (s, o) {
                (Subject::Iri(s), Term::Literal { value, .. })
                    if s == ONTOLOGY_IRI && p == format!("{OWL}versionInfo") =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .expect("owl:versionInfo");
        assert_eq!(info.split('.').count(), 3, "{info} is semver");
        assert!(info.split('.').all(|n| n.parse::<u32>().is_ok()));
    }

    #[test]
    fn no_owl_thing_domain_or_range_is_emitted_by_default() {
        let g = build_graph(&linked_pair(), &vocab(), true);
        let thing = Term::Iri(format!("{OWL}Thing"));
        let blanket: Vec<_> = g
            .iter()
            .filter(|(_, p, o)| {
                (*p == format!("{RDFS}domain") || *p == format!("{RDFS}range")) && **o == thing
            })
            .collect();
        assert!(blanket.is_empty(), "unexpected: {blanket:?}");
        assert!(!serialise(&g).contains("owl:Thing"));
        // Every vc: object property is now unsigned; `hasMaturity`'s range
        // (`ngm:MaturityLevel`) is a real scope and stays.
        let signed: BTreeSet<String> = g
            .iter()
            .filter(|(_, p, _)| *p == format!("{RDFS}domain") || *p == format!("{RDFS}range"))
            .filter_map(|(s, _, _)| match s {
                Subject::Iri(i) => Some(i.clone()),
                Subject::Blank(_) => None,
            })
            .collect();
        assert_eq!(signed, BTreeSet::from([format!("{VC}hasMaturity")]));
    }

    #[test]
    fn a_declared_domain_and_range_emit_scoped_signatures() {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires", domain: a, range: b }
  has-part: { owl: "vc:hasPart", domain: owl:Thing }
"#,
        )
        .unwrap();
        let g = build_graph(&linked_pair(), &vocab, true);
        let has = |s: &str, p: &str, o: &str| {
            g.iter().any(|(subj, pred, obj)| {
                *subj == Subject::Iri(s.to_owned()) && pred == p && *obj == Term::Iri(o.to_owned())
            })
        };
        let requires = format!("{VC}requires");
        assert!(has(&requires, &format!("{RDFS}domain"), &format!("{NGM}a")));
        assert!(has(&requires, &format!("{RDFS}range"), &format!("{NGM}b")));
        let has_part = format!("{VC}hasPart");
        assert!(
            !g.iter()
                .any(|(s, p, _)| *s == Subject::Iri(has_part.clone())
                    && (p == format!("{RDFS}domain") || p == format!("{RDFS}range"))),
            "owl:Thing is the absence of a signature, not one"
        );
        assert_ne!(
            version_iri_of(&g),
            version_iri_of(&build_graph(&linked_pair(), &self::vocab(), true)),
            "declaring a signature is a vocabulary change the version IRI sees"
        );
    }

    /// Defect: a signature on an emitted relation outside the fourteen
    /// hard-coded `vc:` properties was accepted by the vocabulary and then
    /// silently dropped. Every emitted relation that declares one emits it;
    /// an unemitted relation emits nothing.
    #[test]
    fn a_signature_on_any_emitted_relation_is_emitted() {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:    { owl: "rdfs:subClassOf" }
  governs: { owl: "vc:governs", domain: a, range: b }
  ext:     { owl: "https://example.org/onto#ext", range: b }
  drafts:  { owl: "vc:drafts", emitted: false, status: provisional, domain: a }
"#,
        )
        .unwrap();
        let g = build_graph(&linked_pair(), &vocab, true);
        let signature = |s: &str| -> BTreeSet<(String, Term)> {
            g.iter()
                .filter(|(subj, p, _)| {
                    **subj == Subject::Iri(s.to_owned())
                        && (*p == format!("{RDFS}domain") || *p == format!("{RDFS}range"))
                })
                .map(|(_, p, o)| (p.to_owned(), o.clone()))
                .collect()
        };
        assert_eq!(
            signature(&format!("{VC}governs")),
            BTreeSet::from([
                (format!("{RDFS}domain"), Term::Iri(format!("{NGM}a"))),
                (format!("{RDFS}range"), Term::Iri(format!("{NGM}b"))),
            ])
        );
        assert_eq!(
            signature("https://example.org/onto#ext"),
            BTreeSet::from([(format!("{RDFS}range"), Term::Iri(format!("{NGM}b")))])
        );
        assert!(signature(&format!("{VC}drafts")).is_empty(), "not emitted");
    }

    #[test]
    fn serialisation_is_deterministic() {
        let corpus = corpus_of(vec![record("B"), record("A")]);
        let one = serialise(&build_graph(&corpus, &vocab(), true));
        let two = serialise(&build_graph(&corpus, &vocab(), true));
        assert_eq!(one, two);
    }
}
