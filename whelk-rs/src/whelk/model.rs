use std::rc::Rc;

use im::{hashset, HashSet};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref TOP: &'static str = "http://www.w3.org/2002/07/owl#Thing";
    pub static ref BOTTOM: &'static str = "http://www.w3.org/2002/07/owl#Nothing";
}

pub trait HasSignature {
    fn signature(&self) -> HashSet<Rc<Entity>>;
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Role {
    pub id: String,
}

impl Role {
    pub fn composition_role_prefix() -> &'static str {
        "urn:whelk:composition_role:"
    }
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Individual {
    pub id: String,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct AtomicConcept {
    pub id: String,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct ExistentialRestriction {
    pub role: Rc<Role>,
    pub concept: Rc<Concept>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Conjunction {
    pub left: Rc<Concept>,
    pub right: Rc<Concept>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Disjunction {
    pub operands: HashSet<Rc<Concept>>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct SelfRestriction {
    pub role: Rc<Role>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Complement {
    pub concept: Rc<Concept>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct Nominal {
    pub individual: Rc<Individual>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub enum Entity {
    Role(Rc<Role>),
    AtomicConcept(Rc<AtomicConcept>),
    Individual(Rc<Individual>),
}

impl Entity {
    pub fn id(&self) -> &str {
        match self {
            Entity::Role(role) => &role.id,
            Entity::AtomicConcept(c) => &c.id,
            Entity::Individual(i) => &i.id,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Debug)]
pub enum Concept {
    AtomicConcept(Rc<AtomicConcept>),
    Complement(Rc<Complement>),
    Conjunction(Rc<Conjunction>),
    Disjunction(Rc<Disjunction>),
    ExistentialRestriction(Rc<ExistentialRestriction>),
    Nominal(Rc<Nominal>),
    SelfRestriction(Rc<SelfRestriction>),
}

impl Concept {
    pub fn concept_signature(&self) -> HashSet<Rc<Concept>> {
        match self {
            Concept::AtomicConcept(_) => HashSet::unit(Rc::new(self.clone())),
            Concept::Conjunction(conjunction) => {
                let mut sig = conjunction.left.concept_signature().union(conjunction.right.concept_signature());
                sig.insert(Rc::new(self.clone()));
                sig
            }
            Concept::Disjunction(disjunction) => disjunction.operands.iter().flat_map(|o| o.concept_signature()).collect(),
            Concept::ExistentialRestriction(er) => {
                let mut sig = er.concept.concept_signature();
                sig.insert(Rc::new(self.clone()));
                sig
            }
            Concept::SelfRestriction(_) => HashSet::unit(Rc::new(self.clone())),
            Concept::Complement(complement) => {
                let mut sig = complement.concept.concept_signature();
                sig.insert(Rc::new(self.clone()));
                sig
            }
            Concept::Nominal(_) => HashSet::unit(Rc::new(self.clone())),
        }
    }

    pub fn top() -> Concept {
        Concept::AtomicConcept(Rc::new(AtomicConcept { id: TOP.to_string() }))
    }

    pub fn bottom() -> Concept {
        Concept::AtomicConcept(Rc::new(AtomicConcept { id: BOTTOM.to_string() }))
    }
}

impl HasSignature for Concept {
    fn signature(&self) -> HashSet<Rc<Entity>> {
        match self {
            Concept::AtomicConcept(ac) => HashSet::unit(Rc::new(Entity::AtomicConcept(Rc::clone(ac)))),
            Concept::Conjunction(conjunction) => conjunction.left.signature().union(conjunction.right.signature()),
            Concept::Disjunction(d) => d.operands.iter().flat_map(|x| x.signature()).collect(),
            Concept::ExistentialRestriction(er) => {
                let mut sig = er.concept.signature();
                sig.insert(Rc::new(Entity::Role(Rc::clone(&er.role))));
                sig
            }
            Concept::SelfRestriction(sr) => HashSet::unit(Rc::new(Entity::Role(Rc::clone(&sr.role)))),
            Concept::Complement(complement) => complement.concept.signature(),
            Concept::Nominal(nominal) => HashSet::unit(Rc::new(Entity::Individual(Rc::clone(&nominal.individual)))),
        }
    }
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct ConceptInclusion {
    pub subclass: Rc<Concept>,
    pub superclass: Rc<Concept>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct RoleInclusion {
    pub subproperty: Rc<Role>,
    pub superproperty: Rc<Role>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct RoleComposition {
    pub first: Rc<Role>,
    pub second: Rc<Role>,
    pub superproperty: Rc<Role>,
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub enum Axiom {
    ConceptInclusion(Rc<ConceptInclusion>),
    RoleInclusion(Rc<RoleInclusion>),
    RoleComposition(Rc<RoleComposition>),
}

impl HasSignature for Axiom {
    fn signature(&self) -> HashSet<Rc<Entity>> {
        match self {
            Axiom::ConceptInclusion(ci) => ci.subclass.signature().union(ci.superclass.signature()),
            Axiom::RoleInclusion(ri) => hashset![Rc::new(Entity::Role(Rc::clone(&ri.subproperty))), Rc::new(Entity::Role(Rc::clone(&ri.superproperty))),],
            Axiom::RoleComposition(rc) => {
                hashset![Rc::new(Entity::Role(Rc::clone(&rc.first))), Rc::new(Entity::Role(Rc::clone(&rc.second))), Rc::new(Entity::Role(Rc::clone(&rc.superproperty))),]
            }
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Debug)]
pub enum QueueExpression {
    Concept(Rc<Concept>),
    ConceptInclusion(Rc<ConceptInclusion>),
    SubPlus(Rc<ConceptInclusion>),
    Link { subject: Rc<Concept>, role: Rc<Role>, target: Rc<Concept> },
}
