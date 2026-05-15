use std::ops::Deref;
use std::rc::Rc;

use im::{hashmap, hashset, vector, HashMap, HashSet, Vector};
use itertools::Itertools;

use crate::whelk::model::{
    AtomicConcept, Axiom, Complement, Concept, ConceptInclusion, Conjunction, Disjunction, Entity, ExistentialRestriction, HasSignature, QueueExpression, Role, RoleComposition,
    RoleInclusion, SelfRestriction, BOTTOM, TOP,
};

#[derive(Eq, PartialEq, Hash, Clone, Debug)]
pub struct ReasonerState {
    hier: HashMap<Rc<Role>, HashSet<Rc<Role>>>,
    hier_comps: HashMap<Rc<Role>, HashMap<Rc<Role>, Vector<Rc<Role>>>>,
    inits: HashSet<Rc<Concept>>,
    asserted_concept_inclusions_by_subclass: HashMap<Rc<Concept>, Vector<Rc<ConceptInclusion>>>,
    closure_subs_by_superclass: HashMap<Rc<Concept>, HashSet<Rc<Concept>>>,
    closure_subs_by_subclass: HashMap<Rc<Concept>, HashSet<Rc<Concept>>>,
    asserted_negative_conjunctions: HashSet<Rc<Conjunction>>,
    asserted_negative_conjunctions_by_right_operand: HashMap<Rc<Concept>, HashMap<Rc<Concept>, Rc<Conjunction>>>,
    asserted_negative_conjunctions_by_left_operand: HashMap<Rc<Concept>, HashMap<Rc<Concept>, Rc<Conjunction>>>,
    asserted_unions: HashSet<Rc<Disjunction>>,
    unions_by_operand: HashMap<Rc<Concept>, Vector<Rc<Disjunction>>>,
    links_by_subject: HashMap<Rc<Concept>, HashMap<Rc<Role>, HashSet<Rc<Concept>>>>,
    links_by_target: HashMap<Rc<Concept>, HashMap<Rc<Role>, Vector<Rc<Concept>>>>,
    negative_existential_restrictions_by_concept: HashMap<Rc<Concept>, HashSet<Rc<ExistentialRestriction>>>,
    propagations: HashMap<Rc<Concept>, HashMap<Rc<Role>, Vector<Rc<ExistentialRestriction>>>>,
    asserted_negative_self_restrictions_by_role: HashMap<Rc<Role>, Rc<SelfRestriction>>,
    top: Rc<Concept>,
    bottom: Rc<Concept>,
}

impl ReasonerState {
    fn empty() -> ReasonerState {
        ReasonerState {
            hier: Default::default(),
            hier_comps: Default::default(),
            inits: Default::default(),
            asserted_concept_inclusions_by_subclass: Default::default(),
            closure_subs_by_superclass: hashmap! {Rc::new(Concept::bottom()) => HashSet::new()},
            closure_subs_by_subclass: hashmap! {Rc::new(Concept::top()) => HashSet::new()},
            asserted_negative_conjunctions: Default::default(),
            asserted_negative_conjunctions_by_right_operand: Default::default(),
            asserted_negative_conjunctions_by_left_operand: Default::default(),
            asserted_unions: Default::default(),
            unions_by_operand: Default::default(),
            links_by_subject: Default::default(),
            links_by_target: Default::default(),
            negative_existential_restrictions_by_concept: Default::default(),
            propagations: Default::default(),
            asserted_negative_self_restrictions_by_role: Default::default(),
            top: Rc::new(Concept::AtomicConcept(Rc::new(AtomicConcept { id: TOP.to_string() }))),
            bottom: Rc::new(Concept::AtomicConcept(Rc::new(AtomicConcept { id: BOTTOM.to_string() }))),
        }
    }

    pub fn named_subsumptions(&self) -> Vector<(Rc<AtomicConcept>, Rc<AtomicConcept>)> {
        self.closure_subs_by_subclass
            .iter()
            .filter_map(|(sub, supers)| match sub.deref() {
                Concept::AtomicConcept(ac) => Some((ac, supers)),
                _ => None,
            })
            .flat_map(|(sub, supers)| {
                supers.iter().filter_map(|sup| match sup.deref() {
                    Concept::AtomicConcept(ac) => Some((Rc::clone(sub), Rc::clone(ac))),
                    _ => None,
                })
            })
            .collect()
    }
}

pub fn assert(axioms: &HashSet<Rc<Axiom>>) -> ReasonerState {
    let all_roles: HashSet<Rc<Role>> = axioms
        .iter()
        .flat_map(|ax| ax.signature())
        .filter_map(|entity| match entity.deref() {
            Entity::Role(role) => Some(Rc::clone(role)),
            _ => None,
        })
        .collect();
    let all_role_inclusions = axioms
        .iter()
        .filter_map(|ax| match ax.deref() {
            Axiom::RoleInclusion(ri) => Some(Rc::clone(&ri)),
            _ => None,
        })
        .collect();
    let hier = saturate_roles(all_role_inclusions, &all_roles);
    let chains = axioms
        .iter()
        .filter_map(|ax| match ax.deref() {
            Axiom::RoleComposition(rc) => Some(Rc::clone(&rc)),
            _ => None,
        })
        .collect();
    let hier_comps = index_role_compositions(&hier, &chains);
    let concept_inclusions = axioms
        .iter()
        .filter_map(|ax| match ax.deref() {
            Axiom::ConceptInclusion(ci) => Some(Rc::clone(&ci)),
            _ => None,
        })
        .collect();
    let empty = ReasonerState::empty();
    let initial_state = ReasonerState { hier, hier_comps, ..empty };
    assert_append(&concept_inclusions, &initial_state)
}

pub fn assert_append(axioms: &HashSet<Rc<ConceptInclusion>>, state: &ReasonerState) -> ReasonerState {
    let distinct_concepts: HashSet<Rc<Concept>> = axioms.iter().flat_map(|ci| ci.subclass.concept_signature().union(ci.superclass.concept_signature())).collect();
    let atomic_concepts: HashSet<Rc<AtomicConcept>> = distinct_concepts
        .iter()
        .filter_map(|c| match c.deref() {
            Concept::AtomicConcept(ac) => Some(Rc::clone(&ac)),
            _ => None,
        })
        .collect();
    let additional_axioms: HashSet<Rc<ConceptInclusion>> = distinct_concepts
        .iter()
        .flat_map(|c| match c.deref() {
            Concept::Disjunction(disjunction) => rule_union(disjunction),
            Concept::Complement(complement) => rule_complement(complement),
            _ => HashSet::new(),
        })
        .collect();
    //TODO negative_self_restrictions
    let mut assertions_queue: Vec<Rc<ConceptInclusion>> = vec![];
    let mut todo: Vec<QueueExpression> = vec![];
    for ax in axioms {
        assertions_queue.push(Rc::clone(ax));
        todo.push(QueueExpression::ConceptInclusion(Rc::clone(ax)));
    }
    for ax in additional_axioms {
        assertions_queue.push(Rc::clone(&ax));
        todo.push(QueueExpression::ConceptInclusion(Rc::clone(&ax)));
    }
    for ac in atomic_concepts {
        todo.push(QueueExpression::Concept(Rc::new(Concept::AtomicConcept(ac))));
    }
    let mut new_state = state.clone();
    compute_closure(&mut new_state, assertions_queue, todo);
    new_state
}

fn compute_closure(state: &mut ReasonerState, assertions_queue: Vec<Rc<ConceptInclusion>>, mut todo: Vec<QueueExpression>) {
    for ci in assertions_queue {
        process_asserted_concept_inclusion(&ci, state, &mut todo);
    }
    while !todo.is_empty() {
        if let Some(item) = todo.pop() {
            process(item, state, &mut todo);
        }
    }
}

fn process_asserted_concept_inclusion(ci: &Rc<ConceptInclusion>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    match state.asserted_concept_inclusions_by_subclass.get_mut(&ci.subclass) {
        None => {
            state.asserted_concept_inclusions_by_subclass.insert(Rc::clone(&ci.subclass), vector![Rc::clone(ci)]);
        }
        Some(vec) => {
            vec.push_back(Rc::clone(ci));
        }
    }
    rule_subclass_left(ci, state, todo);
    rule_plus_and_a(ci, state, todo);
    rule_plus_some_a(ci, state, todo);
    //rule_or_a_left()
}

fn process(item: QueueExpression, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    match item {
        QueueExpression::Link { subject, role, target } => process_link(&subject, &role, &target, state, todo),
        QueueExpression::ConceptInclusion(ci) => {
            let seen = process_concept_inclusion(&ci, state, todo);
            if !seen {
                process_concept_inclusion_minus(&ci, state, todo);
            }
        }
        QueueExpression::SubPlus(ci) => {
            process_concept_inclusion(&ci, state, todo);
        }
        QueueExpression::Concept(concept) => process_concept(&concept, state, todo),
    }
}

fn process_concept(concept: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    if !state.inits.contains(concept) {
        match state.closure_subs_by_subclass.get_mut(&state.bottom) {
            None => {
                let new_super_classes_of_bottom = hashset![Rc::clone(concept)];
                state.closure_subs_by_subclass.insert(Rc::clone(&state.bottom), new_super_classes_of_bottom);
            }
            Some(super_classes_of_bottom) => {
                super_classes_of_bottom.insert(Rc::clone(concept));
            }
        }
        //TODO maybe this can be done in one step with the contains check
        state.inits.insert(Rc::clone(concept));
        rule_0(concept, state, todo);
        rule_top(concept, state, todo);
    }
}

fn process_concept_inclusion(ci: &Rc<ConceptInclusion>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) -> bool {
    let seen = match state.closure_subs_by_superclass.get_mut(&ci.superclass) {
        None => {
            state.closure_subs_by_superclass.insert(Rc::clone(&ci.superclass), hashset![Rc::clone(&ci.subclass)]);
            false
        }
        Some(subs) => match subs.insert(Rc::clone(&ci.subclass)) {
            None => false,
            Some(_) => true,
        },
    };
    if !seen {
        match state.closure_subs_by_subclass.get_mut(&ci.subclass) {
            None => {
                state.closure_subs_by_subclass.insert(Rc::clone(&ci.subclass), hashset![Rc::clone(&ci.superclass)]);
            }
            Some(supers) => {
                supers.insert(Rc::clone(&ci.superclass));
            }
        }
        rule_bottom_left(ci, state, todo);
        rule_subclass_right(ci, state, todo);
        //TODO
        rule_plus_and_right(ci, state, todo);
        rule_plus_and_left(ci, state, todo);
        rule_plus_some_b_right(ci, state, todo);
        //rule_minus_self()
        //rule_plus_self()
        //rule_or_right()
    }
    seen
}

fn process_concept_inclusion_minus(ci: &Rc<ConceptInclusion>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    rule_minus_some(ci, state, todo);
    rule_minus_and(ci, state, todo);
}

fn process_link(subject: &Rc<Concept>, role: &Rc<Role>, target: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let seen = match state.links_by_subject.get_mut(subject) {
        Some(roles_to_targets) => match roles_to_targets.get_mut(role) {
            Some(targets) => match targets.insert(Rc::clone(target)) {
                None => false,
                Some(_) => true,
            },
            None => {
                roles_to_targets.insert(Rc::clone(role), hashset![Rc::clone(target)]);
                false
            }
        },
        None => {
            let targets = hashset![Rc::clone(target)];
            let roles_to_targets = hashmap! {Rc::clone(role) => targets};
            state.links_by_subject.insert(Rc::clone(subject), roles_to_targets);
            false
        }
    };
    if !seen {
        match state.links_by_target.get_mut(target) {
            Some(role_to_subjects) => match role_to_subjects.get_mut(role) {
                Some(subjects) => {
                    subjects.push_back(Rc::clone(subject));
                }
                None => {
                    role_to_subjects.insert(Rc::clone(role), vector![Rc::clone(subject)]);
                }
            },
            None => {
                let subjects = vector![Rc::clone(subject)];
                let roles_to_subjects = hashmap! {Rc::clone(role) => subjects};
                state.links_by_target.insert(Rc::clone(target), roles_to_subjects);
            }
        }
        rule_bottom_right(subject, role, target, state, todo);
        rule_plus_some_right(subject, role, target, state, todo);
        rule_ring_right(subject, role, target, state, todo);
        rule_ring_left(subject, role, target, state, todo);
        rule_squiggle(subject, role, target, state, todo);
        //rule_plus_self_nominal() //TODO
    }
}

fn rule_bottom_left(ci: &Rc<ConceptInclusion>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    if ci.subclass == state.bottom {
        if let Some(roles_to_subjects) = state.links_by_target.get(&ci.subclass) {
            for subjects in roles_to_subjects.values() {
                for subject in subjects {
                    todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(subject), superclass: Rc::clone(&state.bottom) })));
                }
            }
        }
    }
}

fn rule_bottom_right(subject: &Rc<Concept>, _: &Rc<Role>, target: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Some(subs) = state.closure_subs_by_superclass.get(&state.bottom) {
        if subs.contains(target) {
            todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(subject), superclass: Rc::clone(&state.bottom) })));
        }
    }
}

fn rule_subclass_left(ci: &ConceptInclusion, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Some(others) = state.closure_subs_by_superclass.get(&ci.subclass) {
        for other in others {
            todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(other), superclass: Rc::clone(&ci.superclass) })));
        }
    }
}

fn rule_subclass_right(ci: &ConceptInclusion, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Some(others) = state.asserted_concept_inclusions_by_subclass.get(&ci.superclass) {
        for other in others {
            todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(&ci.subclass), superclass: Rc::clone(&other.superclass) })));
        }
    }
}

fn rule_0(concept: &Rc<Concept>, _: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(concept), superclass: Rc::clone(concept) })));
}

fn rule_top(concept: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(concept), superclass: Rc::clone(&state.top) })));
}

fn rule_minus_and(ci: &Rc<ConceptInclusion>, _: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Concept::Conjunction(conjunction) = ci.superclass.deref() {
        todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(&ci.subclass), superclass: Rc::clone(&conjunction.left) })));
        todo.push(QueueExpression::ConceptInclusion(Rc::new(ConceptInclusion { subclass: Rc::clone(&ci.subclass), superclass: Rc::clone(&conjunction.right) })));
    }
}

fn rule_plus_and_a(ci: &Rc<ConceptInclusion>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let new_negative_conjunctions = ci
        .subclass
        .concept_signature()
        .iter()
        .filter_map(|c| match c.deref() {
            Concept::Conjunction(conjunction) => {
                state.asserted_negative_conjunctions.insert(Rc::clone(conjunction));
                match state.asserted_negative_conjunctions_by_left_operand.get_mut(&conjunction.left) {
                    None => {
                        let by_right_for_left = hashmap! {Rc::clone(&conjunction.right) => Rc::clone(conjunction)};
                        state.asserted_negative_conjunctions_by_left_operand.insert(Rc::clone(&conjunction.left), by_right_for_left);
                    }
                    Some(by_right_for_left) => {
                        by_right_for_left.insert(Rc::clone(&conjunction.right), Rc::clone(conjunction));
                    }
                };
                match state.asserted_negative_conjunctions_by_right_operand.get_mut(&conjunction.right) {
                    None => {
                        let by_left_for_right = hashmap! {Rc::clone(&conjunction.left) => Rc::clone(conjunction)};
                        state.asserted_negative_conjunctions_by_right_operand.insert(Rc::clone(&conjunction.right), by_left_for_right);
                    }
                    Some(by_left_for_right) => {
                        by_left_for_right.insert(Rc::clone(&conjunction.left), Rc::clone(conjunction));
                    }
                }
                Some(Rc::clone(conjunction))
            }
            _ => None,
        })
        .collect();
    rule_plus_and_b(new_negative_conjunctions, state, todo);
}

fn rule_plus_and_b(new_negative_conjunctions: Vec<Rc<Conjunction>>, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    for conjunction in new_negative_conjunctions {
        if let Some(left_subclasses) = state.closure_subs_by_superclass.get(&conjunction.left) {
            if let Some(right_subclasses) = state.closure_subs_by_superclass.get(&conjunction.right) {
                let common = left_subclasses.clone().intersection(right_subclasses.clone());
                for c in common {
                    todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion { subclass: Rc::clone(&c), superclass: Rc::new(Concept::Conjunction(Rc::clone(&conjunction))) })));
                }
            }
        }
    }
}

fn rule_plus_and_left(ci: &Rc<ConceptInclusion>, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    let d1 = &ci.superclass;
    let c = &ci.subclass;
    if let Some(d2s) = state.closure_subs_by_subclass.get(c) {
        if let Some(conjunctions_matching_left) = state.asserted_negative_conjunctions_by_left_operand.get(d1) {
            // choose a join order: can make a massive performance difference
            if d2s.len() < conjunctions_matching_left.len() {
                for d2 in d2s {
                    if let Some(conjunction) = conjunctions_matching_left.get(d2) {
                        todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                            subclass: Rc::clone(c),
                            superclass: Rc::new(Concept::Conjunction(Rc::clone(conjunction))),
                        })));
                    }
                }
            } else {
                for (right, conjunction) in conjunctions_matching_left {
                    if d2s.contains(right) {
                        todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                            subclass: Rc::clone(c),
                            superclass: Rc::new(Concept::Conjunction(Rc::clone(conjunction))),
                        })));
                    }
                }
            }
        }
    }
}

fn rule_plus_and_right(ci: &Rc<ConceptInclusion>, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    let d2 = &ci.superclass;
    let c = &ci.subclass;
    if let Some(d1s) = state.closure_subs_by_subclass.get(c) {
        if let Some(conjunctions_matching_right) = state.asserted_negative_conjunctions_by_right_operand.get(d2) {
            // choose a join order: can make a massive performance difference
            if d1s.len() < conjunctions_matching_right.len() {
                for d1 in d1s {
                    if let Some(conjunction) = conjunctions_matching_right.get(d1) {
                        todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                            subclass: Rc::clone(c),
                            superclass: Rc::new(Concept::Conjunction(Rc::clone(conjunction))),
                        })));
                    }
                }
            } else {
                for (left, conjunction) in conjunctions_matching_right {
                    if d1s.contains(left) {
                        todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                            subclass: Rc::clone(c),
                            superclass: Rc::new(Concept::Conjunction(Rc::clone(conjunction))),
                        })));
                    }
                }
            }
        }
    }
}

fn rule_minus_some(ci: &ConceptInclusion, _: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Concept::ExistentialRestriction(er) = ci.superclass.deref() {
        todo.push(QueueExpression::Link { subject: Rc::clone(&ci.subclass), role: Rc::clone(&er.role), target: Rc::clone(&er.concept) })
    }
}

fn rule_plus_some_a(ci: &ConceptInclusion, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let new_negative_existentials = ci
        .subclass
        .concept_signature()
        .iter()
        .filter_map(|c| match c.deref() {
            Concept::ExistentialRestriction(er) => {
                match state.negative_existential_restrictions_by_concept.get_mut(&er.concept) {
                    Some(ers) => {
                        ers.insert(Rc::clone(er));
                    }
                    None => {
                        state.negative_existential_restrictions_by_concept.insert(Rc::clone(&er.concept), hashset![Rc::clone(er)]);
                    }
                }
                Some(Rc::clone(er))
            }
            _ => None,
        })
        .collect();
    rule_plus_some_b_left(new_negative_existentials, state, todo);
}

fn rule_plus_some_b_left(new_negative_existentials: Vec<Rc<ExistentialRestriction>>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let mut new_propagations = vec![];
    for er in new_negative_existentials {
        if let Some(subclasses) = state.closure_subs_by_superclass.get(&er.concept) {
            for subclass in subclasses {
                new_propagations.push((Rc::clone(subclass), Rc::clone(&er)));
                match state.propagations.get_mut(subclass) {
                    Some(roles_to_ers) => match roles_to_ers.get_mut(&er.role) {
                        Some(ers) => {
                            ers.push_back(Rc::clone(&er));
                        }
                        None => {
                            roles_to_ers.insert(Rc::clone(&er.role), vector![Rc::clone(&er)]);
                        }
                    },
                    None => {
                        state.propagations.insert(Rc::clone(subclass), hashmap! {Rc::clone(&er.role) => vector![Rc::clone(&er)]});
                    }
                }
            }
        }
    }
    rule_plus_some_left(new_propagations, state, todo);
}

fn rule_plus_some_b_right(ci: &ConceptInclusion, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let mut new_propagations = vec![];
    if let Some(ers) = state.negative_existential_restrictions_by_concept.get(&ci.superclass) {
        for er in ers {
            new_propagations.push((Rc::clone(&ci.subclass), Rc::clone(&er)));
            match state.propagations.get_mut(&ci.subclass) {
                Some(roles_to_ers) => match roles_to_ers.get_mut(&er.role) {
                    Some(ers) => {
                        ers.push_back(Rc::clone(&er));
                    }
                    None => {
                        roles_to_ers.insert(Rc::clone(&er.role), vector![Rc::clone(&er)]);
                    }
                },
                None => {
                    state.propagations.insert(Rc::clone(&ci.subclass), hashmap! {Rc::clone(&er.role) => vector![Rc::clone(&er)]});
                }
            }
        }
    };
    rule_plus_some_left(new_propagations, state, todo);
}

fn rule_plus_some_left(new_propagations: Vec<(Rc<Concept>, Rc<ExistentialRestriction>)>, state: &ReasonerState, todo: &mut Vec<QueueExpression>) {
    for (concept, er) in new_propagations {
        if let Some(links_with_target) = state.links_by_target.get(&concept) {
            for (role, subjects) in links_with_target {
                if let Some(super_roles) = state.hier.get(role) {
                    if super_roles.contains(&er.role) {
                        for subject in subjects {
                            todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                                subclass: Rc::clone(subject),
                                superclass: Rc::new(Concept::ExistentialRestriction(Rc::clone(&er))),
                            })));
                        }
                    }
                }
            }
        }
    }
}

fn rule_plus_some_right(subject: &Rc<Concept>, role: &Rc<Role>, target: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Some(role_to_er) = state.propagations.get(target) {
        if let Some(ss) = state.hier.get(role) {
            for s in ss {
                if let Some(fs) = role_to_er.get(s) {
                    for f in fs {
                        todo.push(QueueExpression::SubPlus(Rc::new(ConceptInclusion {
                            subclass: Rc::clone(subject),
                            superclass: Rc::new(Concept::ExistentialRestriction(Rc::clone(f))),
                        })));
                    }
                }
            }
        }
    }
}

fn rule_ring_left(subject: &Rc<Concept>, role: &Rc<Role>, target: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    if let Some(links_by_target_for_subject) = state.links_by_target.get(subject) {
        for (r1, es) in links_by_target_for_subject {
            if let Some(r1s) = state.hier_comps.get(r1) {
                if let Some(ss) = r1s.get(role) {
                    for s in ss {
                        for e in es {
                            todo.push(QueueExpression::Link { subject: Rc::clone(e), role: Rc::clone(s), target: Rc::clone(target) });
                        }
                    }
                }
            }
        }
    }
}

fn rule_ring_right(subject: &Rc<Concept>, role: &Rc<Role>, target: &Rc<Concept>, state: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    let links_by_link_subject = state.links_by_subject.get(subject);
    if let Some(r2s) = state.hier_comps.get(role) {
        if let Some(r2_to_targets) = state.links_by_subject.get(target) {
            for (r2, targets) in r2_to_targets {
                if let Some(ss) = r2s.get(r2) {
                    for s in ss {
                        let links_with_s = links_by_link_subject.map(|x| x.get(s)).flatten();
                        for d in targets {
                            // This is just an optimization to reduce the number of redundant links put on the queue, which can be very large for this rule
                            let create_link = match links_with_s {
                                None => true,
                                Some(l) => !l.contains(d),
                            };
                            if create_link {
                                todo.push(QueueExpression::Link { subject: Rc::clone(subject), role: Rc::clone(s), target: Rc::clone(d) });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn rule_squiggle(_: &Rc<Concept>, _: &Rc<Role>, target: &Rc<Concept>, _: &mut ReasonerState, todo: &mut Vec<QueueExpression>) {
    todo.push(QueueExpression::Concept(Rc::clone(target)));
}

fn rule_union(disjunction: &Rc<Disjunction>) -> HashSet<Rc<ConceptInclusion>> {
    disjunction.operands.iter().map(|o| ConceptInclusion { subclass: Rc::clone(o), superclass: Rc::new(Concept::Disjunction(Rc::clone(disjunction))) }).collect()
}

fn rule_complement(complement: &Complement) -> HashSet<Rc<ConceptInclusion>> {
    HashSet::unit(Rc::new(ConceptInclusion { subclass: Rc::clone(&complement.concept), superclass: Rc::new(Concept::bottom()) }))
}

fn saturate_roles(role_inclusions: HashSet<Rc<RoleInclusion>>, all_roles: &HashSet<Rc<Role>>) -> HashMap<Rc<Role>, HashSet<Rc<Role>>> {
    // this can replace the following 6 lines:
    // let sub_to_super: HashMap<Rc<Role>, HashSet<Rc<Role>>> =
    //     role_inclusions.iter().map(|a| (a.subproperty.clone(), a.superproperty.clone())).into_grouping_map().collect::<HashSet<_>>().into();
    let grouped = role_inclusions.iter().into_group_map_by(|ri| &ri.subproperty);
    let mut sub_to_super: HashMap<Rc<Role>, HashSet<Rc<Role>>> = HashMap::new();
    for (sub, ris) in &grouped {
        let supers = ris.iter().map(|ri| Rc::clone(&ri.superproperty)).collect();
        sub_to_super.insert(Rc::clone(sub), supers);
    }
    let mut result = HashMap::new();
    for role in sub_to_super.keys() {
        let all_supers = all_super_roles(role, &HashSet::new(), &sub_to_super);
        result.insert(Rc::clone(role), all_supers);
    }
    // add reflexive role inclusions
    for role in all_roles {
        match result.get_mut(role) {
            None => {
                result.insert(Rc::clone(role), hashset![Rc::clone(role)]);
            }
            Some(supers) => {
                supers.insert(Rc::clone(role));
            }
        }
    }
    result
}

fn all_super_roles(role: &Rc<Role>, exclude: &HashSet<Rc<Role>>, sub_to_super: &HashMap<Rc<Role>, HashSet<Rc<Role>>>) -> HashSet<Rc<Role>> {
    let current_exclude = exclude.update(Rc::clone(role));
    let mut result = HashSet::new();
    for super_prop in sub_to_super.get(role).unwrap_or(&HashSet::default()).iter().filter(|super_prop| !current_exclude.contains(&Rc::clone(super_prop))) {
        let all_supers_reflexive = all_super_roles(super_prop, &current_exclude, sub_to_super).update(Rc::clone(super_prop));
        for super_super_prop in all_supers_reflexive {
            result.insert(super_super_prop);
        }
    }
    result
}

fn index_role_compositions(hier: &HashMap<Rc<Role>, HashSet<Rc<Role>>>, chains: &HashSet<Rc<RoleComposition>>) -> HashMap<Rc<Role>, HashMap<Rc<Role>, Vector<Rc<Role>>>> {
    let role_comps_groups = chains.iter().group_by(|rc| (&rc.first, &rc.second));
    let mut role_comps: HashMap<(Rc<Role>, Rc<Role>), HashSet<Rc<Role>>> = HashMap::new();
    for (chain, group) in &role_comps_groups {
        let supers = group.map(|rc| Rc::clone(&rc.superproperty)).collect();
        role_comps.insert((Rc::clone(chain.0), Rc::clone(chain.1)), supers);
    }
    let hier_comps_tuples: HashSet<(Rc<Role>, Rc<Role>, Rc<Role>)> = hier
        .iter()
        .flat_map(|(r1, s1s)| {
            s1s.iter().flat_map(|s1| {
                hier.iter().flat_map(|(r2, s2s)| {
                    s2s.iter().flat_map(|s2| match role_comps.get(&(Rc::clone(s1), Rc::clone(s2))) {
                        Some(ss) => ss.iter().flat_map(|s| hashset![(Rc::clone(r1), Rc::clone(r2), Rc::clone(s))]).collect(),
                        None => HashSet::<(Rc<Role>, Rc<Role>, Rc<Role>)>::new(),
                    })
                })
            })
        })
        .collect();
    let hier_comps_remove: HashSet<(Rc<Role>, Rc<Role>, Rc<Role>)> = hier_comps_tuples
        .iter()
        .flat_map(|(r1, r2, s)| {
            hier.get(&Rc::clone(s))
                .unwrap()
                .iter()
                .filter(|super_s| *super_s != s)
                .filter(|super_s| hier_comps_tuples.contains(&(Rc::clone(r1), Rc::clone(r2), Rc::clone(super_s))))
                .flat_map(|super_s| hashset![(Rc::clone(r1), Rc::clone(r2), Rc::clone(super_s))])
                .collect::<HashSet<(Rc<Role>, Rc<Role>, Rc<Role>)>>()
        })
        .collect();
    let hier_comps_tuples_filtered = hier_comps_tuples.relative_complement(hier_comps_remove);
    let mut hier_comps: HashMap<Rc<Role>, HashMap<Rc<Role>, Vector<Rc<Role>>>> = HashMap::new();
    for (r1, r2, s) in hier_comps_tuples_filtered {
        match hier_comps.get_mut(&r1) {
            Some(r2_map) => match r2_map.get_mut(&r2) {
                Some(ss) => {
                    ss.push_back(s);
                }
                None => {
                    let ss = vector![s];
                    r2_map.insert(Rc::clone(&r2), ss);
                }
            },
            None => {
                let r2_map = hashmap! {Rc::clone(&r2) => vector![s]};
                hier_comps.insert(Rc::clone(&r1), r2_map);
            }
        }
    }
    hier_comps
}

#[cfg(test)]
mod test {
    use crate::read_input;
    use crate::whelk::model as wm;
    use crate::whelk::model::TOP;
    use crate::whelk::owl::translate_ontology;
    use crate::whelk::reasoner::assert;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use im::{HashMap, HashSet};
    use std::ops::Deref;
    use std::rc::Rc;
    use std::{error, fs, path};

    fn load_test_ontologies(parent_path: &path::PathBuf) -> Result<(Option<SetOntology<RcStr>>, Option<SetOntology<RcStr>>, Option<SetOntology<RcStr>>), Box<dyn error::Error>> {
        let parent_name = parent_path.file_name().unwrap();
        let asserted_path = parent_path.clone().join(format!("{}-asserted.owx", parent_name.to_string_lossy()));
        let entailed_path = parent_path.clone().join(format!("{}-entailed.owx", parent_name.to_string_lossy()));
        let invalid_path = parent_path.clone().join(format!("{}-invalid.owx", parent_name.to_string_lossy()));

        let asserted_ontology = read_input(&asserted_path).expect("failed to read asserted ontology file");

        let ret = match (entailed_path.exists(), invalid_path.exists()) {
            (true, true) => {
                let entailed_ontology = read_input(&entailed_path).expect("failed to read entailed ontology file");
                let invalid_ontology = read_input(&invalid_path).expect("failed to read invalid ontology file");
                (Some(asserted_ontology), Some(entailed_ontology), Some(invalid_ontology))
            }
            (true, false) => {
                let entailed_ontology = read_input(&entailed_path).expect("failed to read entailed ontology file");
                (Some(asserted_ontology), Some(entailed_ontology), None)
            }
            (false, true) => {
                let invalid_ontology = read_input(&invalid_path).expect("failed to read invalid ontology file");
                (Some(asserted_ontology), None, Some(invalid_ontology))
            }
            _ => (None, None, None),
        };

        Ok(ret)
    }

    #[test]
    fn test_for_subclassof() {
        let data_inference_tests_dir = path::PathBuf::from("./src/data/inference-tests");
        let read_dir_results = fs::read_dir(data_inference_tests_dir).expect("no such directory?");

        let assert_entailed_whelk_axioms_exist_in_map =
            |whelk_subs_by_subclass: &HashMap<Rc<wm::Concept>, HashSet<Rc<wm::Concept>>>, whelk_axioms: &HashSet<Rc<wm::Axiom>>| -> () {
                let mut subs_checked = 0;
                whelk_axioms.iter().map(|a| Rc::deref(a)).for_each(|a| match a {
                    wm::Axiom::ConceptInclusion(ci) => match (Rc::deref(&ci.subclass), Rc::deref(&ci.superclass)) {
                        (wm::Concept::AtomicConcept(sub), wm::Concept::AtomicConcept(sup)) => {
                            let subclass_deref = ci.subclass.deref();
                            let supclass_deref = ci.superclass.deref();
                            let values_by_subclass = whelk_subs_by_subclass.get(subclass_deref);
                            assert!(values_by_subclass.is_some(), "{}", format!("values by subclass key is not found: {:?}", subclass_deref));
                            subs_checked += 1;
                            assert!(
                                values_by_subclass.unwrap().contains(supclass_deref),
                                "{}",
                                format!("{:?} should be contained in subclass set with key {:?}", supclass_deref, subclass_deref)
                            );
                        }
                        _ => {}
                    },
                    _ => {}
                });
                println!("Checked {} entailed subsumptions", subs_checked);
            };

        let assert_invalid_whelk_axioms_exist_in_map = |whelk_subs_by_subclass: &HashMap<Rc<wm::Concept>, HashSet<Rc<wm::Concept>>>, whelk_axioms: &HashSet<Rc<wm::Axiom>>| -> () {
            let mut subs_checked = 0;
            whelk_axioms.iter().map(|a| Rc::deref(a)).for_each(|a| match a {
                wm::Axiom::ConceptInclusion(ci) => match (Rc::deref(&ci.subclass), Rc::deref(&ci.superclass)) {
                    (wm::Concept::AtomicConcept(sub), wm::Concept::AtomicConcept(sup)) if sup.id != TOP.to_string() => {
                        let subclass_deref = ci.subclass.deref();
                        let supclass_deref = ci.superclass.deref();
                        if let Some(values_by_subclass) = whelk_subs_by_subclass.get(subclass_deref) {
                            assert!(
                                !values_by_subclass.contains(supclass_deref),
                                "{}",
                                format!("{:?} should not be contained in subclass set with key {:?}", supclass_deref, subclass_deref)
                            );
                            subs_checked += 1;
                        }
                    }
                    _ => {}
                },
                _ => {}
            });
            println!("Checked {} invalid subsumptions", subs_checked);
        };

        let test_directories: Vec<path::PathBuf> = read_dir_results
            .flat_map(|a| a.map(|b| b.path()))
            .filter_map(|a| {
                let path = a.as_path();
                if path.is_dir() {
                    Some(path.to_path_buf())
                } else {
                    None
                }
            })
            .collect();

        test_directories.iter().for_each(|test_dir| {
            println!("testing directory: {:?}", test_dir);
            let (asserted_ontology, entailed_ontology, invalid_ontology) = load_test_ontologies(&test_dir).expect("could not get test ontologies");

            match (asserted_ontology, entailed_ontology, invalid_ontology) {
                (Some(ao), Some(eo), Some(io)) => {
                    let asserted_whelk_axioms = translate_ontology(&ao);

                    let whelk = assert(&asserted_whelk_axioms);
                    let whelk_subs_by_subclass = whelk.closure_subs_by_subclass;
                    // whelk_subs_by_subclass.iter().for_each(|a| println!("subclass: {:?}", a));

                    let entailed_whelk_axioms = translate_ontology(&eo);
                    assert_entailed_whelk_axioms_exist_in_map(&whelk_subs_by_subclass, &entailed_whelk_axioms);

                    let invalid_whelk_axioms = translate_ontology(&io);
                    assert_invalid_whelk_axioms_exist_in_map(&whelk_subs_by_subclass, &invalid_whelk_axioms);
                }
                (Some(ao), Some(eo), None) => {
                    let asserted_whelk_axioms = translate_ontology(&ao);

                    let whelk = assert(&asserted_whelk_axioms);
                    let whelk_subs_by_subclass = whelk.closure_subs_by_subclass;

                    let entailed_whelk_axioms = translate_ontology(&eo);
                    assert_entailed_whelk_axioms_exist_in_map(&whelk_subs_by_subclass, &entailed_whelk_axioms);
                }
                (Some(ao), None, Some(io)) => {
                    let asserted_whelk_axioms = translate_ontology(&ao);

                    let whelk = assert(&asserted_whelk_axioms);
                    let whelk_subs_by_subclass = whelk.closure_subs_by_subclass;

                    let invalid_whelk_axioms = translate_ontology(&io);
                    assert_invalid_whelk_axioms_exist_in_map(&whelk_subs_by_subclass, &invalid_whelk_axioms);
                }
                _ => {}
            }
        });
    }
}
