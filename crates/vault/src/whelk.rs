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
//! hierarchies, transitivity, sibling `owl:disjointWith` (ADR-2125), which
//! is the only axiom that can make a class unsatisfiable here, and the curated
//! `owl:equivalentClass` definitions (ADR-2124), which are the only axioms that
//! can classify a class *into* another. It is **not** given the SKOS
//! annotations or the datatype annotations, which carry no EL semantics.

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{
    AnnotatedComponent, ArcStr, Build, Class, ClassExpression, Component, DeclareClass,
    DeclareObjectProperty, DisjointClasses, EquivalentClasses, MutableOntology, ObjectProperty,
    ObjectPropertyExpression, SubClassOf, SubObjectPropertyExpression, SubObjectPropertyOf,
    TransitiveObjectProperty,
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
const OWL_DISJOINT_WITH: &str = vault_core::consistency::OWL_DISJOINT_WITH;
const OWL_EQUIVALENT_CLASS: &str = vault_core::definition::OWL_EQUIVALENT_CLASS;
const OWL_INTERSECTION_OF: &str = "http://www.w3.org/2002/07/owl#intersectionOf";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";

/// What the reasoner concluded.
#[derive(Debug, Clone, Default)]
pub struct Reasoning {
    /// Entailed `(sub, sup)` IRI pairs that were **not** asserted, sorted and
    /// deduplicated. `owl:Thing` and `owl:Nothing` are excluded on both sides.
    pub inferred: Vec<(String, String)>,
    /// Classes Whelk subsumes under `owl:Nothing`, sorted.
    pub unsatisfiable: Vec<String>,
    /// The subset of [`Self::unsatisfiable`] that is not explained by an
    /// unsatisfiable **asserted** superclass, sorted. A class under an
    /// unsatisfiable parent inherits `owl:Nothing` and is fixed by fixing the
    /// parent, so the build gate names these and only these (ADR-2125).
    pub root_causes: Vec<String>,
    /// Number of named classes the reasoner classified.
    pub classified: usize,
    /// Existential restrictions present in the asserted graph but **not** given
    /// to the reasoner, and therefore not saturated over. See
    /// [`Restrictions`] for why that is sound here.
    pub restrictions_skipped: usize,
    /// Existential restrictions given to the reasoner: none under
    /// [`Restrictions::Skip`], the definition-relevant ones under
    /// [`Restrictions::Relevant`], all under [`Restrictions::Include`].
    pub restrictions_kept: usize,
    /// The restriction mode the reasoner ran under.
    pub mode: Restrictions,
    /// `owl:equivalentClass` definitions given to the reasoner (ADR-2124).
    pub defined_classes: usize,
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
/// (`∃R.D ⊑ B`) or asserts an equivalence involving one. Apart from the
/// curated `defines-as` definitions (ADR-2124), the emitter in `build::turtle`
/// writes neither: restrictions are otherwise only ever emitted as the
/// superclass of a declared class. So, in a graph with no definition,
/// `C ⊑ ∃R.D` can be dropped from the *reasoning input* without changing a
/// single entailed named subsumption.
///
/// # Why `Relevant` is sound when definitions exist
///
/// A definition `A ≡ … ⊓ ∃S.E ⊓ …` puts `∃S.E` in the subclass position. In
/// EL++ (with role inclusions and transitivity, no inverses) the only way
/// `C ⊑ ∃R.D` can help derive `C ⊑ ∃S.E` is through `R ⊑* S`. [`Relevant`]
/// keeps every restriction whose property is a sub- **or** super-property of
/// some definition property (reflexively, through the extracted
/// `rdfs:subPropertyOf` closure — the super-property half is a margin, not a
/// need) and drops the rest, which by the argument above cannot reach a
/// definition. `relevant_and_include_give_the_same_closure_and_skip_loses_the_entailment`
/// pins that equivalence on a fixture where `Skip` is provably wrong.
///
/// The restrictions stay in `data/ontology.ttl` regardless — they are asserted
/// OWL that contract C3 publishes, and consumers read them. This switch governs
/// only what the reasoner is asked to saturate.
///
/// Disjointness (ADR-2125) narrows the claim to *named subsumptions between
/// satisfiable classes*. With `C ⊑ ∃R.D` skipped, an unsatisfiable filler `D`
/// does not drag `C` down to `owl:Nothing` here. That never hides a fault:
/// `D` is itself reported unsatisfiable, and is a root cause, so the
/// zero-unsatisfiable build gate and `WHELK_INCONSISTENT` still fire on the
/// same change. Only the list of *derived* casualties is shorter.
///
/// [`reason`] picks the mode from the graph: [`Relevant`] when any class is
/// defined, else [`Skip`]. `restrictions_change_no_named_subsumption` guards
/// the second half; it keeps its meaning only for graphs without definitions.
///
/// [`Relevant`]: Restrictions::Relevant
/// [`Skip`]: Restrictions::Skip
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Restrictions {
    /// Omit them all. Correct for a graph with no definition, and 42× faster.
    #[default]
    Skip,
    /// Keep exactly those over a property related, through the property
    /// hierarchy, to a property some definition mentions (ADR-2124).
    Relevant,
    /// Give every one to the reasoner. Does not finish on the full corpus; kept
    /// so the equivalences above are testable rather than merely asserted.
    Include,
}

impl Restrictions {
    /// The mode's name, as the build logs it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Relevant => "relevant",
            Self::Include => "include",
        }
    }
}

/// One conjunct of an extracted `owl:equivalentClass` definition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum DefinitionTerm {
    Named(String),
    /// `∃property.filler`.
    Some(String, String),
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
    /// `(a, b)` for `a owl:disjointWith b`.
    disjoint: BTreeSet<(String, String)>,
    /// `(class, conjuncts)` for `class owl:equivalentClass (c₁ ⊓ … ⊓ cₙ)`.
    definitions: BTreeSet<(String, Vec<DefinitionTerm>)>,
}

impl ElAxioms {
    /// Properties some definition mentions in an existential.
    fn definition_properties(&self) -> BTreeSet<&str> {
        self.definitions
            .iter()
            .flat_map(|(_, terms)| terms.iter())
            .filter_map(|t| match t {
                DefinitionTerm::Some(p, _) => Some(p.as_str()),
                DefinitionTerm::Named(_) => None,
            })
            .collect()
    }

    /// Every property reachable from a definition property by following
    /// `rdfs:subPropertyOf` down (sub-properties) or up (super-properties),
    /// the definition properties included.
    fn relevant_properties(&self) -> BTreeSet<String> {
        let mut down: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut up: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (sub, sup) in &self.sub_properties {
            down.entry(sup.as_str()).or_default().push(sub.as_str());
            up.entry(sub.as_str()).or_default().push(sup.as_str());
        }
        let mut out = BTreeSet::new();
        for start in self.definition_properties() {
            for edges in [&down, &up] {
                let mut stack = vec![start];
                let mut seen = BTreeSet::new();
                while let Some(p) = stack.pop() {
                    if seen.insert(p) {
                        stack.extend(edges.get(p).into_iter().flatten().copied());
                    }
                }
                out.extend(seen.into_iter().map(str::to_owned));
            }
        }
        out
    }

    /// The existentials `mode` hands the reasoner.
    fn kept_existentials(&self, mode: Restrictions) -> Vec<&(String, String, String)> {
        match mode {
            Restrictions::Skip => Vec::new(),
            Restrictions::Include => self.existentials.iter().collect(),
            Restrictions::Relevant => {
                let relevant = self.relevant_properties();
                self.existentials
                    .iter()
                    .filter(|(_, p, _)| relevant.contains(p))
                    .collect()
            }
        }
    }
}

fn extract(graph: &Graph) -> ElAxioms {
    let mut classes = BTreeSet::new();
    let mut properties = BTreeSet::new();
    let mut transitive = BTreeSet::new();
    let mut sub_properties = BTreeSet::new();
    let mut sub_classes = BTreeSet::new();
    let mut disjoint = BTreeSet::new();
    let mut restriction_property: BTreeMap<String, String> = BTreeMap::new();
    let mut restriction_filler: BTreeMap<String, String> = BTreeMap::new();
    let mut subclass_of_blank: Vec<(String, String)> = Vec::new();
    let mut equivalent: Vec<(String, Term)> = Vec::new();
    let mut intersection_of: BTreeMap<String, Term> = BTreeMap::new();
    let mut list_first: BTreeMap<String, Term> = BTreeMap::new();
    let mut list_rest: BTreeMap<String, Term> = BTreeMap::new();

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
            (Subject::Iri(s), OWL_DISJOINT_WITH, Term::Iri(o)) => {
                classes.insert(s.clone());
                classes.insert(o.clone());
                disjoint.insert((s.clone(), o.clone()));
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
            (Subject::Iri(s), OWL_EQUIVALENT_CLASS, o) => {
                classes.insert(s.clone());
                equivalent.push((s.clone(), o.clone()));
            }
            (Subject::Blank(b), OWL_INTERSECTION_OF, o) => {
                intersection_of.insert(b.clone(), o.clone());
            }
            (Subject::Blank(b), RDF_FIRST, o) => {
                list_first.insert(b.clone(), o.clone());
            }
            (Subject::Blank(b), RDF_REST, o) => {
                list_rest.insert(b.clone(), o.clone());
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

    let definitions = resolve_definitions(
        equivalent,
        &DefinitionNodes {
            restriction_property: &restriction_property,
            restriction_filler: &restriction_filler,
            intersection_of: &intersection_of,
            list_first: &list_first,
            list_rest: &list_rest,
        },
        &mut classes,
    );

    ElAxioms {
        classes,
        properties,
        transitive,
        sub_properties,
        sub_classes,
        existentials,
        disjoint,
        definitions,
    }
}

/// The blank-node maps a definition's class expression is read back from.
struct DefinitionNodes<'a> {
    restriction_property: &'a BTreeMap<String, String>,
    restriction_filler: &'a BTreeMap<String, String>,
    intersection_of: &'a BTreeMap<String, Term>,
    list_first: &'a BTreeMap<String, Term>,
    list_rest: &'a BTreeMap<String, Term>,
}

/// Read each `class owl:equivalentClass <expression>` back into conjuncts,
/// declaring every named conjunct as a class.
///
/// An operand is a named class or an existential restriction. Anything else
/// (a nested intersection, a non-EL node) is not a shape the emitter writes;
/// that definition is then left out whole rather than weakened, and
/// `VAULT_WHELK_TRACE` says so.
fn resolve_definitions(
    equivalent: Vec<(String, Term)>,
    nodes: &DefinitionNodes<'_>,
    classes: &mut BTreeSet<String>,
) -> BTreeSet<(String, Vec<DefinitionTerm>)> {
    let operand = |t: &Term| -> Option<DefinitionTerm> {
        match t {
            Term::Iri(iri) => Some(DefinitionTerm::Named(iri.clone())),
            Term::Blank(b) => Some(DefinitionTerm::Some(
                nodes.restriction_property.get(b)?.clone(),
                nodes.restriction_filler.get(b)?.clone(),
            )),
            Term::Literal { .. } => None,
        }
    };
    let list = |head: &Term| -> Option<Vec<DefinitionTerm>> {
        let mut out = Vec::new();
        let mut cell = head;
        // Bounded by the number of list cells, so a cyclic list cannot hang.
        for _ in 0..=nodes.list_first.len() {
            match cell {
                Term::Iri(iri) if iri == RDF_NIL => return Some(out),
                Term::Blank(b) => {
                    out.push(operand(nodes.list_first.get(b)?)?);
                    cell = nodes.list_rest.get(b)?;
                }
                _ => return None,
            }
        }
        None
    };
    let mut definitions = BTreeSet::new();
    for (class, target) in equivalent {
        let terms = match &target {
            Term::Blank(b) if nodes.intersection_of.contains_key(b) => {
                list(&nodes.intersection_of[b])
            }
            other => operand(other).map(|t| vec![t]),
        };
        match terms {
            Some(terms) if !terms.is_empty() => {
                for t in &terms {
                    if let DefinitionTerm::Named(iri) = t {
                        classes.insert(iri.clone());
                    }
                }
                definitions.insert((class, terms));
            }
            _ => {
                if std::env::var_os("VAULT_WHELK_TRACE").is_some() {
                    eprintln!("whelk: unreadable owl:equivalentClass on {class}; left out");
                }
            }
        }
    }
    definitions
}

/// The default [`reason`] cap on existentials the `Relevant` mode would keep
/// over a **transitive** property (ADR-2124).
///
/// Measured on the 9,135-class corpus: a definition over the non-transitive
/// `hasPart` keeps 9,563 existentials and `reasoner::assert` takes 1.16 s; one
/// over the transitive `requires` keeps 12,515 (`requires` and its transitive
/// super-property `dependsOn`) and does not finish in 600 s. Total size is
/// not the cost, transitivity is, so the cap counts only the existentials over
/// a transitive property. `vault validate` refuses such a definition first
/// (`DEFINITION_OVER_TRANSITIVE`); this cap is the backstop for a graph that
/// reaches the reasoner without that check, and fails fast instead of hanging.
///
/// `vault.toml` carries no reasoner section, so the cap is this constant;
/// [`reason_capped`] takes another.
pub const RELEVANT_TRANSITIVE_CAP: usize = 2000;

/// The code [`ReasoningError::RelevantOverCap`] is reported under, by the
/// build, the publish gate and as a `vault propose` blocker.
pub const WHELK_RELEVANT_CAP: &str = "WHELK_RELEVANT_CAP";

/// Why [`reason`] refused to run the reasoner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningError {
    /// `Restrictions::Relevant` would keep more existentials over transitive
    /// properties than the cap allows; saturating them does not finish.
    RelevantOverCap {
        /// Existentials `Relevant` would keep, all properties.
        kept: usize,
        /// Of those, the ones over a transitive property — the counted figure.
        transitive_kept: usize,
        /// The cap that was exceeded.
        cap: usize,
        /// The transitive properties those existentials are over, sorted.
        properties: Vec<String>,
    },
}

impl ReasoningError {
    /// The error's code, [`WHELK_RELEVANT_CAP`].
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::RelevantOverCap { .. } => WHELK_RELEVANT_CAP,
        }
    }

    /// The message without the code, for a report that carries the code
    /// separately (a `vault propose` blocker).
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::RelevantOverCap {
                kept,
                transitive_kept,
                cap,
                properties,
            } => format!(
                "a defines-as definition makes the reasoner keep {transitive_kept} existential(s) \
                 over the transitive propert{} {} ({kept} kept in all), over the cap of {cap}; \
                 saturating them does not finish, so the reasoner was not run. Define the class \
                 over a non-transitive relation (ADR-2124, DEFINITION_OVER_TRANSITIVE)",
                if properties.len() == 1 { "y" } else { "ies" },
                properties.join(", ")
            ),
        }
    }
}

impl std::fmt::Display for ReasoningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for ReasoningError {}

/// Classify the asserted graph with Whelk, under [`RELEVANT_TRANSITIVE_CAP`].
///
/// The restriction mode is decided by the graph (ADR-2124):
/// [`Restrictions::Relevant`] when any class carries an `owl:equivalentClass`
/// definition, else [`Restrictions::Skip`]. Never [`Restrictions::Include`],
/// which does not finish on the full corpus.
///
/// # Errors
/// [`ReasoningError::RelevantOverCap`], before the reasoner runs, when
/// `Relevant` would keep more than [`RELEVANT_TRANSITIVE_CAP`] existentials
/// over transitive properties.
pub fn reason(graph: &Graph) -> Result<Reasoning, ReasoningError> {
    reason_capped(graph, RELEVANT_TRANSITIVE_CAP)
}

/// [`reason`] with the transitive-existential cap set to `cap`.
///
/// # Errors
/// [`ReasoningError::RelevantOverCap`] when `Relevant` would keep more than
/// `cap` existentials over transitive properties. `Skip` never fails.
pub fn reason_capped(graph: &Graph, cap: usize) -> Result<Reasoning, ReasoningError> {
    let axioms = extract(graph);
    if axioms.definitions.is_empty() {
        return Ok(reason_axioms(&axioms, Restrictions::Skip));
    }
    let kept = axioms.kept_existentials(Restrictions::Relevant);
    let over_transitive: Vec<_> = kept
        .iter()
        .filter(|(_, p, _)| axioms.transitive.contains(p))
        .collect();
    if over_transitive.len() > cap {
        let properties: BTreeSet<&String> = over_transitive.iter().map(|(_, p, _)| p).collect();
        return Err(ReasoningError::RelevantOverCap {
            kept: kept.len(),
            transitive_kept: over_transitive.len(),
            cap,
            properties: properties.into_iter().cloned().collect(),
        });
    }
    Ok(reason_axioms(&axioms, Restrictions::Relevant))
}

/// [`reason`] with the restriction mode forced. See [`Restrictions`].
#[must_use]
pub fn reason_with(graph: &Graph, restrictions: Restrictions) -> Reasoning {
    reason_axioms(&extract(graph), restrictions)
}

/// The class expression one definition operand denotes.
fn operand_expression(build: &Build<ArcStr>, term: &DefinitionTerm) -> ClassExpression<ArcStr> {
    match term {
        DefinitionTerm::Named(iri) => ClassExpression::Class(Class(build.iri(iri.clone()))),
        DefinitionTerm::Some(property, filler) => ClassExpression::ObjectSomeValuesFrom {
            ope: ObjectPropertyExpression::ObjectProperty(ObjectProperty(
                build.iri(property.clone()),
            )),
            bce: Box::new(ClassExpression::Class(Class(build.iri(filler.clone())))),
        },
    }
}

#[allow(clippy::too_many_lines)] // One `insert` per EL axiom kind; a list reads better whole.
fn reason_axioms(axioms: &ElAxioms, restrictions: Restrictions) -> Reasoning {
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
    for (a, b) in &axioms.disjoint {
        ontology.insert(annotated(Component::DisjointClasses(DisjointClasses(
            vec![
                ClassExpression::Class(Class(build.iri(a.clone()))),
                ClassExpression::Class(Class(build.iri(b.clone()))),
            ],
        ))));
    }
    for (class, terms) in &axioms.definitions {
        let definiens = if let [only] = terms.as_slice() {
            operand_expression(&build, only)
        } else {
            ClassExpression::ObjectIntersectionOf(
                terms
                    .iter()
                    .map(|t| operand_expression(&build, t))
                    .collect(),
            )
        };
        ontology.insert(annotated(Component::EquivalentClasses(EquivalentClasses(
            vec![
                ClassExpression::Class(Class(build.iri(class.clone()))),
                definiens,
            ],
        ))));
    }
    let kept = axioms.kept_existentials(restrictions);
    for (sub, property, filler) in &kept {
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
            "whelk: {} classes, {} subClassOf, {} disjointWith, {} definitions, \
             {} existentials ({} kept, mode {}), {} properties, {} transitive, \
             {} sub-properties",
            axioms.classes.len(),
            axioms.sub_classes.len(),
            axioms.disjoint.len(),
            axioms.definitions.len(),
            axioms.existentials.len(),
            kept.len(),
            restrictions.as_str(),
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

    let root_causes = axioms
        .sub_classes
        .iter()
        .fold(unsatisfiable.clone(), |mut roots, (sub, sup)| {
            if unsatisfiable.contains(sup) {
                roots.remove(sub);
            }
            roots
        })
        .into_iter()
        .collect();

    Reasoning {
        inferred: inferred.into_iter().collect(),
        unsatisfiable: unsatisfiable.into_iter().collect(),
        root_causes,
        classified: axioms.classes.len(),
        restrictions_skipped: axioms.existentials.len() - kept.len(),
        restrictions_kept: kept.len(),
        mode: restrictions,
        defined_classes: axioms
            .definitions
            .iter()
            .map(|(class, _)| class)
            .collect::<BTreeSet<_>>()
            .len(),
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
    /// It holds for graphs **without** a definition, which is when `reason`
    /// picks `Skip`. If it ever fails, the emitter has gained a
    /// subclass-position restriction outside `defines-as`, and `reason`'s mode
    /// choice is no longer sound. That is exactly the signal wanted: the
    /// speed-up is legitimate only while the entailments are identical. Graphs
    /// with definitions are covered by
    /// `relevant_and_include_give_the_same_closure_and_skip_loses_the_entailment`.
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

    fn disjoint(g: &mut Graph, a: &str, b: &str) {
        g.add_iri(a, OWL_DISJOINT_WITH, T::Iri(b.to_owned()));
    }

    /// `P ⊑ A, P ⊑ B, A disjointWith B` (A, B siblings under `parent`), and
    /// `Q ⊑ P`. With the disjointness P and Q are unsatisfiable.
    fn probe(with_disjointness: bool) -> Graph {
        let mut g = Graph::new();
        for c in ["urn:parent", "urn:a", "urn:b", "urn:p", "urn:q"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:a", "urn:parent");
        sub(&mut g, "urn:b", "urn:parent");
        sub(&mut g, "urn:p", "urn:a");
        sub(&mut g, "urn:p", "urn:b");
        sub(&mut g, "urn:q", "urn:p");
        if with_disjointness {
            disjoint(&mut g, "urn:a", "urn:b");
        }
        g
    }

    #[test]
    fn disjoint_parents_make_their_common_subclass_unsatisfiable() {
        let r = reason(&probe(true)).unwrap();
        assert!(!r.is_consistent());
        assert_eq!(
            r.unsatisfiable,
            vec!["urn:p".to_owned(), "urn:q".to_owned()]
        );
    }

    #[test]
    fn without_the_disjointness_the_probe_is_consistent() {
        let r = reason(&probe(false)).unwrap();
        assert!(r.is_consistent(), "{:?}", r.unsatisfiable);
        assert!(r.root_causes.is_empty());
    }

    /// Q is unsatisfiable only because it sits under P; P is the root cause.
    #[test]
    fn root_causes_exclude_classes_derived_from_an_unsatisfiable_parent() {
        let r = reason(&probe(true)).unwrap();
        assert_eq!(r.root_causes, vec!["urn:p".to_owned()]);
    }

    /// Two unrelated clashes are two root causes, each named once.
    #[test]
    fn two_independent_clashes_are_two_root_causes() {
        let mut g = probe(true);
        for c in ["urn:c", "urn:d", "urn:r"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:c", "urn:parent");
        sub(&mut g, "urn:d", "urn:parent");
        sub(&mut g, "urn:r", "urn:c");
        sub(&mut g, "urn:r", "urn:d");
        disjoint(&mut g, "urn:c", "urn:d");
        let r = reason(&g).unwrap();
        assert_eq!(r.root_causes, vec!["urn:p".to_owned(), "urn:r".to_owned()]);
    }

    // ── ADR-2124: defined classes and Restrictions::Relevant ───────────────

    const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
    const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
    const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";
    const OWL_RESTRICTION: &str = "http://www.w3.org/2002/07/owl#Restriction";
    const OWL_EQUIVALENT: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
    const OWL_INTERSECTION: &str = "http://www.w3.org/2002/07/owl#intersectionOf";

    fn property(g: &mut Graph, iri: &str) {
        g.add_iri(iri, RDF_TYPE, T::Iri(OWL_OBJECT_PROPERTY.to_owned()));
    }

    fn some(g: &mut Graph, prop: &str, filler: &str) -> String {
        let b = g.fresh_blank();
        g.add(
            S::Blank(b.clone()),
            RDF_TYPE,
            T::Iri(OWL_RESTRICTION.to_owned()),
        );
        g.add(
            S::Blank(b.clone()),
            OWL_ON_PROPERTY,
            T::Iri(prop.to_owned()),
        );
        g.add(
            S::Blank(b.clone()),
            OWL_SOME_VALUES_FROM,
            T::Iri(filler.to_owned()),
        );
        b
    }

    /// `sub ⊑ ∃prop.filler`, in the superclass position the emitter writes.
    fn existential(g: &mut Graph, sub: &str, prop: &str, filler: &str) {
        let b = some(g, prop, filler);
        g.add_iri(sub, RDFS_SUBCLASS_OF, T::Blank(b));
    }

    /// `class ≡ named₁ ⊓ … ⊓ ∃p₁.f₁ ⊓ …`, written exactly as
    /// `build::turtle` writes a `defines-as` page: an `owl:intersectionOf`
    /// RDF list, or the lone conjunct when there is only one.
    fn define(g: &mut Graph, class_iri: &str, named: &[&str], somes: &[(&str, &str)]) {
        let mut members: Vec<T> = named.iter().map(|n| T::Iri((*n).to_owned())).collect();
        for (p, f) in somes {
            members.push(T::Blank(some(g, p, f)));
        }
        if members.len() == 1 {
            g.add_iri(class_iri, OWL_EQUIVALENT, members.remove(0));
            return;
        }
        let mut rest = T::Iri(RDF_NIL.to_owned());
        for member in members.into_iter().rev() {
            let cell = g.fresh_blank();
            g.add(S::Blank(cell.clone()), RDF_FIRST, member);
            g.add(S::Blank(cell.clone()), RDF_REST, rest);
            rest = T::Blank(cell);
        }
        let node = g.fresh_blank();
        g.add(
            S::Blank(node.clone()),
            RDF_TYPE,
            T::Iri(OWL_CLASS.to_owned()),
        );
        g.add(S::Blank(node.clone()), OWL_INTERSECTION, rest);
        g.add_iri(class_iri, OWL_EQUIVALENT, T::Blank(node));
    }

    /// `GrippingRobot ≡ Robot ⊓ ∃hasPart.Gripper`; `Arm ⊑ Robot`,
    /// `Arm ⊑ ∃hasPart.SmallGripper`, `SmallGripper ⊑ Gripper`. Nobody wrote
    /// `Arm ⊑ GrippingRobot`; the definition entails it. `Arm ⊑ ∃relatedTo.Gripper`
    /// shares no property with the definition and is the restriction
    /// `Relevant` drops.
    fn defined_fixture() -> Graph {
        let mut g = Graph::new();
        for c in [
            "urn:robot",
            "urn:gripper",
            "urn:small-gripper",
            "urn:arm",
            "urn:gripping-robot",
        ] {
            class(&mut g, c);
        }
        property(&mut g, "urn:hasPart");
        property(&mut g, "urn:relatedTo");
        sub(&mut g, "urn:small-gripper", "urn:gripper");
        sub(&mut g, "urn:arm", "urn:robot");
        existential(&mut g, "urn:arm", "urn:hasPart", "urn:small-gripper");
        existential(&mut g, "urn:arm", "urn:relatedTo", "urn:gripper");
        define(
            &mut g,
            "urn:gripping-robot",
            &["urn:robot"],
            &[("urn:hasPart", "urn:gripper")],
        );
        g
    }

    fn entails(r: &Reasoning, sub: &str, sup: &str) -> bool {
        r.inferred.contains(&(sub.to_owned(), sup.to_owned()))
    }

    #[test]
    fn a_definition_yields_a_named_subsumption_nobody_asserted() {
        let g = defined_fixture();
        let r = reason(&g).unwrap();
        assert_eq!(
            r.mode,
            Restrictions::Relevant,
            "a definition selects Relevant"
        );
        assert_eq!(r.defined_classes, 1);
        assert!(
            entails(&r, "urn:arm", "urn:gripping-robot"),
            "Arm ⊑ Robot ⊓ ∃hasPart.Gripper must classify Arm under GrippingRobot: {:?}",
            r.inferred
        );
        // It is inferred, not asserted: no rdfs:subClassOf triple says so.
        assert!(!g.iter().any(|(s, p, o)| {
            *s == S::Iri("urn:arm".into())
                && p == RDFS_SUBCLASS_OF
                && *o == T::Iri("urn:gripping-robot".into())
        }));
        // And the definition's necessary half holds: GrippingRobot ⊑ Robot.
        assert!(
            entails(&r, "urn:gripping-robot", "urn:robot"),
            "{:?}",
            r.inferred
        );
    }

    #[test]
    fn relevant_and_include_give_the_same_closure_and_skip_loses_the_entailment() {
        let g = defined_fixture();
        let relevant = reason_with(&g, Restrictions::Relevant);
        let include = reason_with(&g, Restrictions::Include);
        let skip = reason_with(&g, Restrictions::Skip);

        assert_eq!(relevant.inferred, include.inferred);
        assert_eq!(relevant.unsatisfiable, include.unsatisfiable);
        assert!(entails(&include, "urn:arm", "urn:gripping-robot"));
        assert!(
            !entails(&skip, "urn:arm", "urn:gripping-robot"),
            "Skip must lose the definitional entailment, or the fixture proves nothing"
        );

        assert_eq!(
            (include.restrictions_kept, include.restrictions_skipped),
            (2, 0)
        );
        assert_eq!(
            (relevant.restrictions_kept, relevant.restrictions_skipped),
            (1, 1)
        );
        assert_eq!((skip.restrictions_kept, skip.restrictions_skipped), (0, 2));
    }

    /// `n` classes `urn:c<i> ⊑ ∃prop.urn:filler`, plus (in [`fan_out`]) a definition
    /// `urn:defined ≡ ∃prop.urn:filler`, with `prop` transitive or not.
    fn fan_out(n: usize, transitive: bool) -> Graph {
        let mut g = fan_out_undefined(n, transitive);
        define(&mut g, "urn:defined", &[], &[("urn:prop", "urn:filler")]);
        g
    }

    fn fan_out_undefined(n: usize, transitive: bool) -> Graph {
        let mut g = Graph::new();
        class(&mut g, "urn:filler");
        class(&mut g, "urn:defined");
        property(&mut g, "urn:prop");
        if transitive {
            g.add_iri(
                "urn:prop",
                RDF_TYPE,
                T::Iri(OWL_TRANSITIVE_PROPERTY.to_owned()),
            );
        }
        for i in 0..n {
            let c = format!("urn:c{i}");
            class(&mut g, &c);
            existential(&mut g, &c, "urn:prop", "urn:filler");
        }
        g
    }

    /// ADR-2124 defect: a definition over a transitive property kept 12,515
    /// existentials and the build hung. Over the cap the reasoner is not run
    /// at all; the named error says why and over which property.
    #[test]
    fn relevant_over_the_transitive_cap_is_refused_before_reasoning() {
        let err = reason_capped(&fan_out(5, true), 3).expect_err("over the cap");
        assert_eq!(
            err,
            ReasoningError::RelevantOverCap {
                kept: 5,
                transitive_kept: 5,
                cap: 3,
                properties: vec!["urn:prop".to_owned()],
            }
        );
        assert_eq!(err.code(), WHELK_RELEVANT_CAP);
        assert!(err.to_string().starts_with("WHELK_RELEVANT_CAP: "), "{err}");
        assert!(err.to_string().contains("urn:prop"), "{err}");
        // At the cap it runs, and classifies through the definition.
        let r = reason_capped(&fan_out(3, true), 3).expect("at the cap");
        assert_eq!(r.mode, Restrictions::Relevant);
        assert!(entails(&r, "urn:c0", "urn:defined"), "{:?}", r.inferred);
    }

    /// Size alone is not refused: the measured `hasPart` definition keeps
    /// 9,563 existentials and reasons in about a second. Only existentials over
    /// a transitive property count toward the cap.
    #[test]
    fn non_transitive_existentials_do_not_count_toward_the_cap() {
        let r = reason_capped(&fan_out(5, false), 3).expect("non-transitive");
        assert_eq!(r.restrictions_kept, 5);
        assert!(entails(&r, "urn:c4", "urn:defined"));
    }

    /// Without a definition the mode is `Skip` and nothing is kept, so no cap
    /// can apply however many transitive existentials the graph holds.
    #[test]
    fn skip_is_never_capped() {
        let r = reason_capped(&fan_out_undefined(5, true), 0).expect("skip");
        assert_eq!(r.mode, Restrictions::Skip);
    }

    #[test]
    fn a_graph_without_definitions_still_uses_skip() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b"] {
            class(&mut g, c);
        }
        property(&mut g, "urn:p");
        existential(&mut g, "urn:a", "urn:p", "urn:b");
        let r = reason(&g).unwrap();
        assert_eq!(r.mode, Restrictions::Skip);
        assert_eq!(r.defined_classes, 0);
        assert_eq!((r.restrictions_kept, r.restrictions_skipped), (0, 1));
    }

    /// Relevant follows the property hierarchy both ways. A definition over
    /// `dependsOn` keeps `requires` (a sub-property: it feeds the definition)
    /// and `top` (a super-property), and drops the unrelated `relatedTo`.
    #[test]
    fn relevant_keeps_restrictions_over_sub_and_super_properties_only() {
        let mut g = Graph::new();
        for c in ["urn:x", "urn:y", "urn:z", "urn:d", "urn:needs-d"] {
            class(&mut g, c);
        }
        for p in ["urn:requires", "urn:dependsOn", "urn:top", "urn:relatedTo"] {
            property(&mut g, p);
        }
        g.add_iri(
            "urn:requires",
            RDFS_SUB_PROPERTY_OF,
            T::Iri("urn:dependsOn".into()),
        );
        g.add_iri(
            "urn:dependsOn",
            RDFS_SUB_PROPERTY_OF,
            T::Iri("urn:top".into()),
        );
        existential(&mut g, "urn:x", "urn:requires", "urn:d");
        existential(&mut g, "urn:y", "urn:top", "urn:d");
        existential(&mut g, "urn:z", "urn:relatedTo", "urn:d");
        define(&mut g, "urn:needs-d", &[], &[("urn:dependsOn", "urn:d")]);

        let relevant = reason_with(&g, Restrictions::Relevant);
        assert_eq!(
            (relevant.restrictions_kept, relevant.restrictions_skipped),
            (2, 1)
        );
        // X ⊑ ∃requires.D, requires ⊑ dependsOn ⇒ X ⊑ ∃dependsOn.D ≡ NeedsD.
        assert!(
            entails(&relevant, "urn:x", "urn:needs-d"),
            "{:?}",
            relevant.inferred
        );
        // A super-property does not feed the definition, and Z is unrelated.
        assert!(!entails(&relevant, "urn:y", "urn:needs-d"));
        assert!(!entails(&relevant, "urn:z", "urn:needs-d"));
        assert_eq!(
            relevant.inferred,
            reason_with(&g, Restrictions::Include).inferred
        );
    }

    /// A definition of named classes only (`C ≡ A ⊓ B`) mentions no property,
    /// so `Relevant` keeps no restriction — and still classifies.
    #[test]
    fn a_named_only_definition_keeps_no_restriction_and_still_classifies() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b", "urn:ab", "urn:x", "urn:f"] {
            class(&mut g, c);
        }
        property(&mut g, "urn:p");
        sub(&mut g, "urn:x", "urn:a");
        sub(&mut g, "urn:x", "urn:b");
        existential(&mut g, "urn:x", "urn:p", "urn:f");
        define(&mut g, "urn:ab", &["urn:a", "urn:b"], &[]);
        let r = reason(&g).unwrap();
        assert_eq!(r.mode, Restrictions::Relevant);
        assert_eq!((r.restrictions_kept, r.restrictions_skipped), (0, 1));
        assert!(entails(&r, "urn:x", "urn:ab"), "{:?}", r.inferred);
    }

    /// The lone-conjunct form (`C owl:equivalentClass [restriction]`, no
    /// intersection list) is a definition too.
    #[test]
    fn a_single_existential_definition_is_extracted() {
        let mut g = Graph::new();
        for c in ["urn:x", "urn:f", "urn:has-f"] {
            class(&mut g, c);
        }
        property(&mut g, "urn:p");
        existential(&mut g, "urn:x", "urn:p", "urn:f");
        define(&mut g, "urn:has-f", &[], &[("urn:p", "urn:f")]);
        let r = reason(&g).unwrap();
        assert_eq!(r.defined_classes, 1);
        assert!(entails(&r, "urn:x", "urn:has-f"), "{:?}", r.inferred);
    }

    #[test]
    fn derives_the_transitive_subclass_closure() {
        let mut g = Graph::new();
        for c in ["urn:a", "urn:b", "urn:c"] {
            class(&mut g, c);
        }
        sub(&mut g, "urn:a", "urn:b");
        sub(&mut g, "urn:b", "urn:c");
        let r = reason(&g).unwrap();
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
        let r = reason(&g).unwrap();
        assert!(r.is_consistent());
        assert!(r.classified >= 3);
    }

    #[test]
    fn an_empty_graph_classifies_cleanly() {
        let r = reason(&Graph::new()).unwrap();
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
        let r = reason(&g).unwrap();
        assert!(r
            .inferred
            .iter()
            .all(|(s, o)| s != OWL_THING && o != OWL_THING));
    }

    #[test]
    fn the_inferred_turtle_carries_the_method_and_the_pairs() {
        let reasoning = Reasoning {
            restrictions_skipped: 0,
            restrictions_kept: 0,
            mode: Restrictions::Skip,
            defined_classes: 0,
            inferred: vec![("urn:a".into(), "urn:c".into())],
            unsatisfiable: Vec::new(),
            root_causes: Vec::new(),
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
        assert_eq!(reason(&g).unwrap().inferred, reason(&g).unwrap().inferred);
    }
}
