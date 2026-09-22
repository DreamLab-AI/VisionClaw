//! EL++ reasoning over the corpus, through the **same** Whelk build
//! `VisionClaw`'s ontology service uses (Q11).
//!
//! Two jobs:
//!
//! * `ontology-inferred.ttl` — the EL++ subsumption closure minus the asserted
//!   axioms, which is what contract C3 asks for.
//! * The `WHELK_INCONSISTENT` blocker — a class Whelk subsumes under
//!   `owl:Nothing` is unsatisfiable, and an unsatisfiable corpus never reaches
//!   a human for signature (decision Q6).
//!
//! The reasoner is given the real thing: class declarations, `subClassOf`,
//! the existential restrictions the asserted graph carries, property
//! hierarchies and transitivity. It is **not** given the SKOS annotations or
//! the datatype annotations, which carry no EL semantics.

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{
    AnnotatedComponent, ArcStr, Build, Class, ClassExpression, Component, DeclareClass,
    DeclareObjectProperty, MutableOntology, ObjectProperty, ObjectPropertyExpression, SubClassOf,
    SubObjectPropertyExpression, SubObjectPropertyOf, TransitiveObjectProperty,
};
use horned_owl::ontology::set::SetOntology;

use crate::build::turtle::{Graph, Subject, Term};

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const RDFS_SUB_PROPERTY_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";
const OWL_ON_PROPERTY: &str = "http://www.w3.org/2002/07/owl#onProperty";
const OWL_SOME_VALUES_FROM: &str = "http://www.w3.org/2002/07/owl#someValuesFrom";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const OWL_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
const OWL_TRANSITIVE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#TransitiveProperty";

/// What the reasoner concluded.
#[derive(Debug, Clone, Default)]
pub struct Reasoning {
    /// Entailed `(sub, sup)` IRI pairs that were **not** asserted, sorted and
    /// deduplicated. `owl:Thing` and `owl:Nothing` are excluded on both sides.
    pub inferred: Vec<(String, String)>,
    /// Classes Whelk subsumes under `owl:Nothing`, sorted.
    pub unsatisfiable: Vec<String>,
    /// Number of named classes the reasoner classified.
    pub classified: usize,
    /// Existential restrictions present in the asserted graph but **not** given
    /// to the reasoner, and therefore not saturated over. See
    /// [`Restrictions::Skip`] for why that is sound here.
    pub restrictions_skipped: usize,
}

impl Reasoning {
    /// `true` when no class is unsatisfiable.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.unsatisfiable.is_empty()
    }
}

/// Whether the reasoner is given the graph's existential restrictions.
///
/// # Why skipping them is sound, and why it matters
///
/// It matters because they are almost the entire cost. On the full corpus
/// (9,135 classes) `reasoner::assert` takes **348 ms without** them and **does
/// not finish with** them; at 5,215 classes it is 122 ms against 5.18 s, a 42×
/// difference — and the closure is *identical* to the pair, 9,575 inferred
/// subsumptions either way. They are saturated over at enormous cost and
/// entail nothing.
///
/// It is sound because of the *position* they occupy. The graph extraction can
/// only produce an existential in the **superclass** position — `C ⊑ ∃R.D`, a named
/// subject with a blank restriction node as its `rdfs:subClassOf` object. In
/// EL++, an axiom of that form contributes to a **named** subsumption only when
/// some other axiom puts a restriction in the **subclass** position
/// (`∃R.D ⊑ B`) or asserts an equivalence involving one. The emitter in
/// `build::turtle` never writes either: restrictions are only ever emitted as
/// the superclass of a declared class, and no `owl:equivalentClass` is emitted
/// at all. So `C ⊑ ∃R.D` can be dropped from the *reasoning input* without
/// changing a single entailed named subsumption.
///
/// The restrictions stay in `data/ontology.ttl` regardless — they are asserted
/// OWL that contract C3 publishes, and consumers read them. This switch governs
/// only what the reasoner is asked to saturate.
///
/// If the emitter ever gains subclass-position restrictions or equivalences,
/// [`Restrictions::Include`] becomes the correct setting and
/// `restrictions_change_no_named_subsumption` will fail, which is the point of
/// that test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restrictions {
    /// Omit them. Correct for the axioms this crate emits, and 42× faster.
    Skip,
    /// Give them to the reasoner. Needed only if the emitter changes; kept so
    /// the equivalence above is testable rather than merely asserted.
    Include,
}

/// The EL-relevant slice of the asserted graph, pulled out of the Turtle model
/// so the reasoner and the serialised ontology can never disagree.
struct ElAxioms {
    classes: BTreeSet<String>,
    properties: BTreeSet<String>,
    transitive: BTreeSet<String>,
    sub_properties: BTreeSet<(String, String)>,
    sub_classes: BTreeSet<(String, String)>,
    /// `(sub class, property, filler)` for `sub ⊑ ∃property.filler`.
    existentials: BTreeSet<(String, String, String)>,
}

fn extract(graph: &Graph) -> ElAxioms {
    let mut classes = BTreeSet::new();
    let mut properties = BTreeSet::new();
    let mut transitive = BTreeSet::new();
    let mut sub_properties = BTreeSet::new();
    let mut sub_classes = BTreeSet::new();
    let mut restriction_property: BTreeMap<String, String> = BTreeMap::new();
    let mut restriction_filler: BTreeMap<String, String> = BTreeMap::new();
    let mut subclass_of_blank: Vec<(String, String)> = Vec::new();

    for (subject, predicate, object) in graph.iter() {
        match (subject, predicate, object) {
            (Subject::Iri(s), RDF_TYPE, Term::Iri(o)) if o == OWL_CLASS => {
                classes.insert(s.clone());
            }
            (Subject::Iri(s), RDF_TYPE, Term::Iri(o)) if o == OWL_OBJECT_PROPERTY => {
                properties.insert(s.clone());
            }
            (Subject::Iri(s), RDF_TYPE, Term::Iri(o)) if o == OWL_TRANSITIVE_PROPERTY => {
                transitive.insert(s.clone());
            }
            (Subject::Iri(s), RDFS_SUB_PROPERTY_OF, Term::Iri(o)) => {
                sub_properties.insert((s.clone(), o.clone()));
            }
            (Subject::Iri(s), RDFS_SUBCLASS_OF, Term::Iri(o)) => {
                classes.insert(s.clone());
                classes.insert(o.clone());
                sub_classes.insert((s.clone(), o.clone()));
            }
            (Subject::Iri(s), RDFS_SUBCLASS_OF, Term::Blank(b)) => {
                classes.insert(s.clone());
                subclass_of_blank.push((s.clone(), b.clone()));
            }
            (Subject::Blank(b), OWL_ON_PROPERTY, Term::Iri(o)) => {
                restriction_property.insert(b.clone(), o.clone());
            }
            (Subject::Blank(b), OWL_SOME_VALUES_FROM, Term::Iri(o)) => {
                restriction_filler.insert(b.clone(), o.clone());
                classes.insert(o.clone());
            }
            _ => {}
        }
    }

    let existentials = subclass_of_blank
        .into_iter()
        .filter_map(|(s, b)| {
            Some((
                s,
                restriction_property.get(&b)?.clone(),
                restriction_filler.get(&b)?.clone(),
            ))
        })
        .collect();

    ElAxioms {
        classes,
        properties,
        transitive,
        sub_properties,
        sub_classes,
        existentials,
    }
}

/// Classify the asserted graph with Whelk.
///
/// # Panics
/// Never: every component built here is well-formed by construction.
#[must_use]
pub fn reason(graph: &Graph) -> Reasoning {
    reason_with(graph, Restrictions::Skip)
}

/// [`reason`], choosing whether the existential restrictions are saturated over.
///
/// See [`Restrictions`]. `reason` picks [`Restrictions::Skip`], which is both
/// correct for this crate's axioms and the difference between a build that
/// finishes and one that does not.
#[must_use]
#[allow(clippy::too_many_lines)] // One `insert` per EL axiom kind; a list reads better whole.
pub fn reason_with(graph: &Graph, restrictions: Restrictions) -> Reasoning {
    let axioms = extract(graph);
    let build = Build::new();
    let mut ontology: SetOntology<ArcStr> = SetOntology::new();

    let annotated = |component: Component<ArcStr>| AnnotatedComponent {
        component,
        ann: std::collections::BTreeSet::new(),
    };

    for iri in &axioms.classes {
        ontology.insert(annotated(Component::DeclareClass(DeclareClass(Class(
            build.iri(iri.clone()),
        )))));
    }
    for iri in &axioms.properties {
        ontology.insert(annotated(Component::DeclareObjectProperty(
            DeclareObjectProperty(ObjectProperty(build.iri(iri.clone()))),
        )));
    }
    for iri in &axioms.transitive {
        ontology.insert(annotated(Component::TransitiveObjectProperty(
            TransitiveObjectProperty(ObjectPropertyExpression::ObjectProperty(ObjectProperty(
                build.iri(iri.clone()),
            ))),
        )));
    }
    for (sub, sup) in &axioms.sub_properties {
        ontology.insert(annotated(Component::SubObjectPropertyOf(
            SubObjectPropertyOf {
                sub: SubObjectPropertyExpression::ObjectPropertyExpression(
                    ObjectPropertyExpression::ObjectProperty(ObjectProperty(
                        build.iri(sub.clone()),
                    )),
                ),
                sup: ObjectPropertyExpression::ObjectProperty(ObjectProperty(
                    build.iri(sup.clone()),
                )),
            },
        )));
    }
    for (sub, sup) in &axioms.sub_classes {
        ontology.insert(annotated(Component::SubClassOf(SubClassOf {
            sub: ClassExpression::Class(Class(build.iri(sub.clone()))),
            sup: ClassExpression::Class(Class(build.iri(sup.clone()))),
        })));
    }
    for (sub, property, filler) in axioms
        .existentials
        .iter()
        .filter(|_| restrictions == Restrictions::Include)
    {
        ontology.insert(annotated(Component::SubClassOf(SubClassOf {
            sub: ClassExpression::Class(Class(build.iri(sub.clone()))),
            sup: ClassExpression::ObjectSomeValuesFrom {
                ope: ObjectPropertyExpression::ObjectProperty(ObjectProperty(
                    build.iri(property.clone()),
                )),
                bce: Box::new(ClassExpression::Class(Class(build.iri(filler.clone())))),
            },
        })));
    }

    // Phase timing, opt-in. The reasoner is the dominant cost on the real
    // corpus and the three phases have very different fixes: a slow
    // `translate_ontology` is a translation problem, a slow `assert` is
    // saturation, and a slow `named_subsumptions` is output size. Guessing
    // between them wasted a day, so the measurement is wired in.
    let trace = std::env::var_os("VAULT_WHELK_TRACE").is_some();
    let mark = std::time::Instant::now();
    if trace {
        eprintln!(
            "whelk: {} classes, {} subClassOf, {} existentials, {} properties, \
             {} transitive, {} sub-properties",
            axioms.classes.len(),
            axioms.sub_classes.len(),
            axioms.existentials.len(),
            axioms.properties.len(),
            axioms.transitive.len(),
            axioms.sub_properties.len(),
        );
        eprintln!("whelk: ontology built in {:?}", mark.elapsed());
    }

    let mark = std::time::Instant::now();
    let whelk_axioms = whelk::whelk::owl::translate_ontology(&ontology);
    if trace {
        eprintln!(
            "whelk: translate_ontology {:?} -> {} axioms",
            mark.elapsed(),
            whelk_axioms.len()
        );
    }

    let mark = std::time::Instant::now();
    let state = whelk::whelk::reasoner::assert(&whelk_axioms);
    if trace {
        eprintln!("whelk: reasoner::assert {:?}", mark.elapsed());
    }
    let mark = std::time::Instant::now();

    let mut inferred = BTreeSet::new();
    let mut unsatisfiable = BTreeSet::new();
    let subsumptions = state.named_subsumptions();
    if trace {
        eprintln!(
            "whelk: named_subsumptions {:?} -> {} pairs",
            mark.elapsed(),
            subsumptions.len()
        );
    }
    for (sub, sup) in subsumptions {
        let (sub, sup) = (sub.id.clone(), sup.id.clone());
        if sup == OWL_NOTHING {
            if sub != OWL_NOTHING {
                unsatisfiable.insert(sub);
            }
            continue;
        }
        if sub == sup || sub == OWL_THING || sup == OWL_THING || sub == OWL_NOTHING {
            continue;
        }
        if axioms.sub_classes.contains(&(sub.clone(), sup.clone())) {
            continue; // asserted, not inferred
        }
        inferred.insert((sub, sup));
    }

    Reasoning {
        inferred: inferred.into_iter().collect(),
        unsatisfiable: unsatisfiable.into_iter().collect(),
        classified: axioms.classes.len(),
        restrictions_skipped: if restrictions == Restrictions::Skip {
            axioms.existentials.len()
        } else {
            0
        },
    }
}

/// Serialise the inferred subsumptions as `ontology-inferred.ttl`.
#[must_use]
pub fn inferred_turtle(reasoning: &Reasoning, generated_at: &str) -> String {
    let mut g = Graph::new();
    let ontology = "https://narrativegoldmine.com/ontology/inferred";
    g.add_iri(
        ontology,
        RDF_TYPE,
        Term::Iri("http://www.w3.org/2002/07/owl#Ontology".to_owned()),
    );
    g.add_iri(
        ontology,
        "http://www.w3.org/2000/01/rdf-schema#label",
        Term::lang("NarrativeGoldmine Inferred Axioms", "en"),
    );
    g.add_iri(
        ontology,
        &format!("{}inferenceMethod", crate::build::turtle::VC),
        Term::plain("whelk-el++-closure"),
    );
    g.add_iri(
        ontology,
        &format!("{}generatedAt", crate::build::turtle::VC),
        Term::typed(generated_at, "http://www.w3.org/2001/XMLSchema#dateTime"),
    );
    for (sub, sup) in &reasoning.inferred {
        g.add_iri(sub, RDFS_SUBCLASS_OF, Term::Iri(sup.clone()));
    }
    crate::build::turtle::serialise(&g)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::build::turtle::{Subject as S, Term as T};

    fn class(g: &mut Graph, iri: &str) {
        g.add_iri(iri, RDF_TYPE, T::Iri(OWL_CLASS.to_owned()));
    }

    fn sub(g: &mut Graph, a: &str, b: &str) {
        g.add_iri(a, RDFS_SUBCLASS_OF, T::Iri(b.to_owned()));
    }

    /// The load-bearing claim: skipping the existential restrictions changes no
    /// entailed named subsumption.
    ///
    /// If this ever fails, the emitter has gained a subclass-position
    /// restriction or an equivalence, and `Restrictions::Include` becomes the
    /// correct setting for `reason`. That is exactly the signal wanted: the
    /// speed-up is legitimate only while the entailments are identical.
    #[test]
    fn restrictions_change_no_named_subsumption() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b", "urn:c", "urn:d"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:a", "urn:b");
        sub(&mut g, "urn:b", "urn:c");
        // Two restrictions in the superclass position, over a declared property.
        g.add_iri("urn:p", RDF_TYPE, T::Iri(OWL_OBJECT_PROPERTY.to_owned()));
        for (subject, filler) in [("urn:a", "urn:d"), ("urn:b", "urn:c")] {
            let blank = g.fresh_blank();
            g.add(
                S::Blank(blank.clone()),
                RDF_TYPE.to_owned(),
                T::Iri("http://www.w3.org/2002/07/owl#Restriction".to_owned()),
            );
            g.add(
                S::Blank(blank.clone()),
                OWL_ON_PROPERTY.to_owned(),
                T::Iri("urn:p".to_owned()),
            );
            g.add(
                S::Blank(blank.clone()),
                OWL_SOME_VALUES_FROM.to_owned(),
                T::Iri(filler.to_owned()),
            );
            g.add_iri(subject, RDFS_SUBCLASS_OF, T::Blank(blank));
        }

        let with = reason_with(&g, Restrictions::Include);
        let without = reason_with(&g, Restrictions::Skip);

        // Vacuous otherwise.
        assert_eq!(
            without.restrictions_skipped, 2,
            "the fixture must carry restrictions for this to mean anything"
        );
        assert_eq!(with.restrictions_skipped, 0);

        assert_eq!(
            with.inferred, without.inferred,
            "skipping restrictions must not change the inferred closure"
        );
        assert_eq!(with.unsatisfiable, without.unsatisfiable);
        assert_eq!(with.classified, without.classified);
        // And the closure is the real one, not empty.
        assert!(
            without
                .inferred
                .contains(&("urn:a".to_owned(), "urn:c".to_owned())),
            "{:?}",
            without.inferred
        );
    }

    #[test]
    fn derives_the_transitive_subclass_closure() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b", "urn:c"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:a", "urn:b");
        sub(&mut g, "urn:b", "urn:c");
        let r = reason(&g);
        assert!(r.is_consistent());
        assert!(r
            .inferred
            .contains(&("urn:a".to_owned(), "urn:c".to_owned())));
        // The asserted edges are excluded from the inferred set.
        assert!(!r
            .inferred
            .contains(&("urn:a".to_owned(), "urn:b".to_owned())));
    }

    #[test]
    fn classifies_through_an_existential_restriction() {
        let mut g = Graph::new();
        for c in ["urn:parent", "urn:child", "urn:filler"] {
            class(&mut g, c);
        }
        g.add_iri("urn:p", RDF_TYPE, T::Iri(OWL_OBJECT_PROPERTY.to_owned()));
        let b = g.fresh_blank();
        g.add(
            S::Blank(b.clone()),
            OWL_ON_PROPERTY,
            T::Iri("urn:p".to_owned()),
        );
        g.add(
            S::Blank(b.clone()),
            OWL_SOME_VALUES_FROM,
            T::Iri("urn:filler".to_owned()),
        );
        g.add_iri("urn:child", RDFS_SUBCLASS_OF, T::Blank(b));
        let r = reason(&g);
        assert!(r.is_consistent());
        assert!(r.classified >= 3);
    }

    #[test]
    fn an_empty_graph_classifies_cleanly() {
        let r = reason(&Graph::new());
        assert!(r.is_consistent());
        assert!(r.inferred.is_empty());
        assert_eq!(r.classified, 0);
    }

    #[test]
    fn owl_thing_never_appears_in_the_inferred_set() {
        let mut g = Graph::new();
        class(&mut g, "urn:a");
        class(&mut g, OWL_THING);
        sub(&mut g, "urn:a", OWL_THING);
        let r = reason(&g);
        assert!(r
            .inferred
            .iter()
            .all(|(s, o)| s != OWL_THING && o != OWL_THING));
    }

    #[test]
    fn the_inferred_turtle_carries_the_method_and_the_pairs() {
        let reasoning = Reasoning {
            restrictions_skipped: 0,
            inferred: vec![("urn:a".into(), "urn:c".into())],
            unsatisfiable: Vec::new(),
            classified: 3,
        };
        let ttl = inferred_turtle(&reasoning, "2026-09-22T00:00:00+00:00");
        assert!(ttl.contains("whelk-el++-closure"));
        assert!(ttl.contains("<urn:a> rdfs:subClassOf <urn:c>"));
        assert!(ttl.contains("2026-09-22T00:00:00+00:00"));
    }

    #[test]
    fn reasoning_is_deterministic() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b", "urn:c"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:a", "urn:b");
        sub(&mut g, "urn:b", "urn:c");
        assert_eq!(reason(&g).inferred, reason(&g).inferred);
    }
}
