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
//!   98.8% of classes unsatisfiable when they were last enabled. Re-enable only
//!   behind a normalisation pass and a zero-unsatisfiable CI gate.
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

/// The six top-level domain roots.
pub const DOMAIN_ROOT_SLUGS: &[&str] = &[
    "artificial-intelligence",
    "spatial-computing",
    "blockchain",
    "infrastructure",
    "distributed-collaboration",
    "robotics",
];

/// The 34 intermediate taxonomy categories.
pub const CATEGORY_SLUGS: &[&str] = &[
    "ai-technique",
    "ai-model-architecture",
    "ai-application",
    "ai-governance-and-ethics",
    "cat-ai-infrastructure",
    "ai-research-area",
    "sc-display-and-rendering",
    "sc-interaction",
    "sc-content-and-assets",
    "sc-platform-and-environment",
    "sc-standards-and-interop",
    "sc-governance-and-safety",
    "bc-protocol-and-consensus",
    "bc-cryptographic-primitive",
    "bc-token-and-asset",
    "bc-defi-and-economics",
    "bc-network-component",
    "bc-governance-and-regulation",
    "infra-computing-and-cloud",
    "infra-network-and-comms",
    "infra-security-and-identity",
    "infra-data-management",
    "infra-legal-and-regulatory",
    "infra-software-engineering",
    "robo-perception",
    "robo-actuation-and-control",
    "robo-robot-type",
    "robo-navigation-and-planning",
    "robo-safety-and-standards",
    "robo-human-robot-interaction",
    "dc-communication",
    "dc-workspace-tools",
    "dc-telepresence",
    "dc-protocol-and-infra",
];

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
    let ontology = "https://narrativegoldmine.com/ontology";
    g.add_iri(
        ontology,
        &format!("{RDF}type"),
        Term::Iri(format!("{OWL}Ontology")),
    );
    g.add_iri(
        ontology,
        &format!("{RDFS}label"),
        Term::lang("NarrativeGoldmine Ontology", "en"),
    );
    g.add_iri(ontology, &format!("{OWL}versionInfo"), Term::plain("3.1.0"));
    g.add_iri(
        ontology,
        &format!("{DCTERMS}creator"),
        Term::plain("Dr John O'Hare"),
    );

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

    declare_object_properties(&mut g);
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

    g
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

fn declare_object_properties(g: &mut Graph) {
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
        declare_property(g, name, false);
    }
    // requires and dependsOn are transitive.
    declare_property(g, "requires", true);
    declare_property(g, "dependsOn", true);

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
}

fn declare_property(g: &mut Graph, name: &str, transitive: bool) {
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
    g.add_iri(
        &uri,
        &format!("{RDFS}domain"),
        Term::Iri(format!("{OWL}Thing")),
    );
    g.add_iri(
        &uri,
        &format!("{RDFS}range"),
        Term::Iri(format!("{OWL}Thing")),
    );
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

    #[test]
    fn no_domain_disjointness_is_emitted() {
        let ttl = serialise(&build_graph(&corpus_of(vec![]), &vocab(), true));
        assert!(!ttl.contains("AllDisjointClasses"));
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

    #[test]
    fn serialisation_is_deterministic() {
        let corpus = corpus_of(vec![record("B"), record("A")]);
        let one = serialise(&build_graph(&corpus, &vocab(), true));
        let two = serialise(&build_graph(&corpus, &vocab(), true));
        assert_eq!(one, two);
    }
}
