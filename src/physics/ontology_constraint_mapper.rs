//! OWL axiom → live-kernel `ConstraintData` mapper (PRD-018 WS-3, ADR-098 D1).
//!
//! This is a pure anti-corruption layer. It translates materialised OWL axioms
//! (asserted + inferred named graphs) into the `ConstraintData` buffer consumed
//! DIRECTLY by the live `force_pass_kernel` constraint loop in
//! `visionclaw_unified.cu`.
//!
//! ## Why it does NOT use the domain `ConstraintKind` enum
//!
//! The domain `ConstraintKind` (`{FixedPosition=0, Separation=1, …, Semantic=10}`)
//! and the live kernel's `ConstraintKind` (`{DISTANCE=0, POSITION=1, ANGLE=2,
//! SEMANTIC=3, TEMPORAL=4, GROUP=5, SEPARATION=6}`) are divergently numbered
//! (ADR-098 verified-topology break #3). `ConstraintData.kind` is read by the
//! kernel as a raw `i32`, so routing through the domain enum's `as i32` cast
//! would mismap (domain `Separation=1` → kernel `POSITION`). This mapper writes
//! the **live kernel** integers literally via [`LiveKernelKind`].
//!
//! ## Mapping table (ADR-098 D1)
//!
//! | Axiom / relation                     | kind             | Effect                       | params[0]      |
//! |--------------------------------------|------------------|------------------------------|----------------|
//! | `rdfs:subClassOf`, `vc:partOf`       | `DISTANCE` (0)   | attraction child→parent      | short rest-len |
//! | `owl:sameAs`, `owl:equivalentClass`  | `DISTANCE` (0)   | colocate                     | rest-len ≈ 0   |
//! | `owl:disjointWith`, inter-domain     | `SEPARATION` (6) | one-sided min-distance push  | min-distance   |
//!
//! Endpoints that do not resolve to a node index are counted via
//! [`IriNodeResolver::resolve_counted`] and logged — never silently dropped.

use log::{debug, info};

use visionclaw_domain::ports::owl_types::{AxiomType, OwlAxiom};
use visionclaw_ontology::services::iri_node_resolver::IriNodeResolver;

use crate::models::constraints::ConstraintData;

/// Live-kernel `ConstraintKind` discriminants — must match the `enum
/// ConstraintKind` in `crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu`.
/// These are intentionally distinct from `visionclaw_domain::models::constraints::ConstraintKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LiveKernelKind {
    /// Bidirectional rest-length spring (attraction / colocate).
    Distance = 0,
    /// Attract a node to a fixed target position.
    Position = 1,
    /// One-sided min-distance push (no long-range attraction).
    Separation = 6,
    /// One-axis alignment pull toward the pair midpoint (params[0] = axis 0/1/2).
    Alignment = 7,
}

impl LiveKernelKind {
    #[inline]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Default rest-length (world units) for hierarchical attraction
/// (`subClassOf` / `partOf`): pull the child toward the parent at a legible,
/// fixed distance. Tuned up from 60 → 90: at scale (18k+ attractive
/// constraints) a short rest-length plus the aggregate inward pull collapses
/// the whole graph into a single dense blob. 90 lets hierarchies breathe while
/// still reading as "child near parent".
pub const SUBCLASS_REST_LENGTH: f32 = 90.0;
/// Default attraction weight for hierarchical edges.
pub const SUBCLASS_WEIGHT: f32 = 0.6;

/// Rest-length so `sameAs` / `equivalentClass` endpoints sit together without
/// collapsing to a singularity. Tuned up from 2 → 10: rest≈0 stacked thousands
/// of equivalent nodes on the same point, producing the dense over-tight core.
pub const COLOCATE_REST_LENGTH: f32 = 10.0;
/// Strong weight so colocation still dominates ordinary spring forces, eased
/// from 0.95 → 0.85 so equivalents cluster tightly but not as a hard knot.
pub const COLOCATE_WEIGHT: f32 = 0.85;

/// Minimum separation distance (world units) enforced for `disjointWith`.
/// Tuned up from 200 → 350 so disjoint top-level domains visibly push apart and
/// the semantic layout reads as distinct regions rather than one cloud.
pub const DISJOINT_MIN_DISTANCE: f32 = 350.0;
/// Default separation weight.
pub const DISJOINT_WEIGHT: f32 = 0.8;

/// Axis (0=x, 1=y, 2=z) that sibling classes align on: Y, complementing the
/// P4a Sugiyama Y-by-rank spring so a parent's children share a layer.
pub const SIBLING_ALIGN_AXIS: f32 = 1.0;
/// Modest weight for the sibling-alignment pull — it nudges siblings onto a
/// shared layer without overpowering the hierarchical attraction.
pub const SIBLING_ALIGN_WEIGHT: f32 = 0.5;
/// Upper bound on emitted sibling-alignment constraints so a very wide hierarchy
/// (thousands of children under one parent) can never flood the constraint buffer.
pub const MAX_SIBLING_ALIGN_CONSTRAINTS: usize = 20_000;

/// How a resolved axiom maps onto a live-kernel constraint.
enum MappedKind {
    /// `subClassOf` / `partOf` — short-rest-length attraction.
    Attract,
    /// `sameAs` / `equivalentClass` — near-zero-rest colocation.
    Colocate,
    /// `disjointWith` — one-sided separation push.
    Separate,
}

impl MappedKind {
    /// Classify a domain `OwlAxiom` into a live-kernel mapping bucket.
    ///
    /// `partOf` / `sameAs` are not first-class `AxiomType` variants in the
    /// repository's `get_axioms()` output; when they surface as
    /// `ObjectPropertyAssertion`, the predicate is carried in the axiom
    /// `annotations` under the key `"predicate"` (canonical IRI or local name).
    fn classify(axiom: &OwlAxiom) -> Option<MappedKind> {
        match axiom.axiom_type {
            AxiomType::SubClassOf => Some(MappedKind::Attract),
            AxiomType::EquivalentClass => Some(MappedKind::Colocate),
            AxiomType::DisjointWith => Some(MappedKind::Separate),
            AxiomType::ObjectPropertyAssertion => {
                let pred = axiom
                    .annotations
                    .get("predicate")
                    .map(|p| local_name(p))
                    .unwrap_or_default();
                match pred.as_str() {
                    "hasPart" | "has_part" | "partOf" | "isPartOf" | "is_part_of" | "part_of" => {
                        Some(MappedKind::Attract)
                    }
                    "sameAs" | "same_as" => Some(MappedKind::Colocate),
                    "disjointWith" | "disjoint_with" => Some(MappedKind::Separate),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// IRI local name (after the last `#`, `/`, or `:`), lower-cased-insensitive
/// comparison handled by the caller.
fn local_name(iri: &str) -> String {
    iri.rsplit_once(['#', '/', ':'])
        .map(|(_, r)| r.to_string())
        .unwrap_or_else(|| iri.to_string())
}

/// Build a pairwise `ConstraintData` with live-kernel `kind`, two endpoints,
/// `params[0]` carrying the rest-length / min-distance, and `activation_frame=0`
/// (`set_constraints` stamps the real frame so the progressive ramp engages).
fn pairwise(
    kind: LiveKernelKind,
    subject: u32,
    object: u32,
    param0: f32,
    weight: f32,
) -> ConstraintData {
    let mut node_idx = [-1i32; 4];
    node_idx[0] = subject as i32;
    node_idx[1] = object as i32;

    let mut params = [0.0f32; 8];
    params[0] = param0;

    ConstraintData {
        kind: kind.as_i32(),
        count: 2,
        node_idx,
        params,
        weight,
        activation_frame: 0,
    }
}

/// Translate materialised OWL axioms into the live-kernel constraint buffer.
///
/// Both endpoints are resolved through the shared [`IriNodeResolver`] (ADR-100).
/// Self-loops (subject == object) and axioms with any unresolved endpoint are
/// skipped; unresolved endpoints are counted on the resolver and logged.
///
/// The resolver is taken by `&mut` so [`IriNodeResolver::resolve_counted`] can
/// accrue the coverage metric; call [`IriNodeResolver::unresolved_count`] after
/// to read it.
pub fn map_axioms_to_constraints(
    axioms: &[OwlAxiom],
    resolver: &mut IriNodeResolver,
) -> Vec<ConstraintData> {
    let mut out: Vec<ConstraintData> = Vec::with_capacity(axioms.len());
    let mut mapped = 0usize;
    let mut skipped_unclassified = 0usize;
    let mut skipped_unresolved = 0usize;

    // Parent node index → resolved child node indices (subClassOf only), used
    // after the main pass to emit sibling-alignment constraints (ADR-141 P4b).
    // BTreeMap keeps the emission order deterministic.
    let mut children_by_parent: std::collections::BTreeMap<u32, Vec<u32>> =
        std::collections::BTreeMap::new();

    for axiom in axioms {
        let mapping = match MappedKind::classify(axiom) {
            Some(m) => m,
            None => {
                skipped_unclassified += 1;
                continue;
            }
        };

        let subject = resolver.resolve_counted(&axiom.subject);
        let object = resolver.resolve_counted(&axiom.object);

        let (subject, object) = match (subject, object) {
            (Some(s), Some(o)) => (s, o),
            _ => {
                skipped_unresolved += 1;
                continue;
            }
        };

        if subject == object {
            // A self-referential constraint contributes no force; drop it.
            continue;
        }

        let constraint = match mapping {
            MappedKind::Attract => pairwise(
                LiveKernelKind::Distance,
                subject,
                object,
                SUBCLASS_REST_LENGTH,
                SUBCLASS_WEIGHT,
            ),
            MappedKind::Colocate => pairwise(
                LiveKernelKind::Distance,
                subject,
                object,
                COLOCATE_REST_LENGTH,
                COLOCATE_WEIGHT,
            ),
            MappedKind::Separate => pairwise(
                LiveKernelKind::Separation,
                subject,
                object,
                DISJOINT_MIN_DISTANCE,
                DISJOINT_WEIGHT,
            ),
        };

        out.push(constraint);
        mapped += 1;

        // Record subClassOf children under their shared parent (object) so
        // siblings can be aligned onto a common layer below. partOf/sameAs are
        // deliberately excluded — only taxonomic siblings share a rank layer.
        if axiom.axiom_type == AxiomType::SubClassOf {
            children_by_parent.entry(object).or_default().push(subject);
        }
    }

    // Sibling alignment (ADR-141 P4b): for each parent with ≥2 subClassOf
    // children, chain consecutive siblings with a one-axis (Y) ALIGNMENT
    // constraint. This complements the P4a Sugiyama Y-by-rank spring by pulling
    // classes that share a parent onto the same layer. Bounded by
    // MAX_SIBLING_ALIGN_CONSTRAINTS so a very wide hierarchy cannot flood the buffer.
    let mut sibling_aligned = 0usize;
    'parents: for children in children_by_parent.values() {
        if children.len() < 2 {
            continue;
        }
        // Dedup siblings first: duplicate subClassOf axioms (or a child listed
        // twice) would otherwise emit both A–B and B–A alignment constraints,
        // doubling the force on that pair. Sort+dedup gives a stable, unique chain.
        let mut siblings: Vec<u32> = children.clone();
        siblings.sort_unstable();
        siblings.dedup();
        if siblings.len() < 2 {
            continue;
        }
        for pair in siblings.windows(2) {
            if sibling_aligned >= MAX_SIBLING_ALIGN_CONSTRAINTS {
                break 'parents;
            }
            // Skip degenerate self-pairs (a child resolving to itself twice).
            if pair[0] == pair[1] {
                continue;
            }
            out.push(pairwise(
                LiveKernelKind::Alignment,
                pair[0],
                pair[1],
                SIBLING_ALIGN_AXIS,
                SIBLING_ALIGN_WEIGHT,
            ));
            sibling_aligned += 1;
        }
    }

    let dropped = resolver.unresolved_count();
    info!(
        "OWL→constraint mapper: {} axioms in, {} constraints out ({} sibling-alignment, {} unclassified, {} with unresolved endpoint(s); resolver dropped {} endpoint lookups)",
        axioms.len(),
        mapped + sibling_aligned,
        sibling_aligned,
        skipped_unclassified,
        skipped_unresolved,
        dropped
    );
    if dropped > 0 {
        debug!(
            "OWL→constraint mapper: {} endpoint IRIs failed to resolve to a node index (see resolver coverage gate)",
            dropped
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use visionclaw_domain::models::node::Node;

    fn node(id: u32, iri: &str) -> Node {
        Node {
            id,
            metadata_id: iri.to_string(),
            owl_class_iri: Some(iri.to_string()),
            metadata: HashMap::new(),
            ..Default::default()
        }
    }

    fn axiom(axiom_type: AxiomType, subject: &str, object: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.to_string(),
            object: object.to_string(),
            annotations: HashMap::new(),
        }
    }

    /// One subClassOf → DISTANCE attraction, one disjointWith → SEPARATION, one
    /// equivalentClass (sameAs-class) → DISTANCE colocate. Asserts the live-kernel
    /// kind integers, node_idx ordering, count, and params[0].
    #[test]
    fn maps_three_axiom_kinds_to_live_kernel_kinds() {
        let nodes = vec![
            node(1, "urn:vc:class:Dog"),
            node(2, "urn:vc:class:Animal"),
            node(3, "urn:vc:class:Plant"),
            node(4, "urn:vc:class:Canine"),
        ];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let axioms = vec![
            // Dog subClassOf Animal  → DISTANCE attraction
            axiom(
                AxiomType::SubClassOf,
                "urn:vc:class:Dog",
                "urn:vc:class:Animal",
            ),
            // Animal disjointWith Plant → SEPARATION
            axiom(
                AxiomType::DisjointWith,
                "urn:vc:class:Animal",
                "urn:vc:class:Plant",
            ),
            // Dog equivalentClass Canine → DISTANCE colocate
            axiom(
                AxiomType::EquivalentClass,
                "urn:vc:class:Dog",
                "urn:vc:class:Canine",
            ),
        ];

        let constraints = map_axioms_to_constraints(&axioms, &mut resolver);
        assert_eq!(constraints.len(), 3, "all three axioms resolve");

        // subClassOf → DISTANCE(0), child=subject first, short rest-length.
        let sub = &constraints[0];
        assert_eq!(sub.kind, 0, "subClassOf maps to live DISTANCE = 0");
        assert_eq!(sub.count, 2);
        assert_eq!(sub.node_idx[0], 1, "subject (Dog) is endpoint 0");
        assert_eq!(sub.node_idx[1], 2, "object (Animal) is endpoint 1");
        assert_eq!(sub.params[0], SUBCLASS_REST_LENGTH);
        assert_eq!(sub.weight, SUBCLASS_WEIGHT);
        assert_eq!(sub.activation_frame, 0);

        // disjointWith → SEPARATION(6), min-distance in params[0].
        let dis = &constraints[1];
        assert_eq!(dis.kind, 6, "disjointWith maps to live SEPARATION = 6");
        assert_eq!(dis.count, 2);
        assert_eq!(dis.node_idx[0], 2);
        assert_eq!(dis.node_idx[1], 3);
        assert_eq!(dis.params[0], DISJOINT_MIN_DISTANCE);
        assert_eq!(dis.weight, DISJOINT_WEIGHT);

        // equivalentClass → DISTANCE(0) colocate, near-zero rest-length.
        let eq = &constraints[2];
        assert_eq!(
            eq.kind, 0,
            "equivalentClass maps to live DISTANCE = 0 (colocate)"
        );
        assert_eq!(eq.params[0], COLOCATE_REST_LENGTH);
        assert_eq!(eq.weight, COLOCATE_WEIGHT);
    }

    /// sameAs delivered as an ObjectPropertyAssertion with predicate annotation
    /// → DISTANCE colocate.
    #[test]
    fn sameas_property_assertion_maps_to_colocate() {
        let nodes = vec![node(10, "urn:vc:ind:a"), node(11, "urn:vc:ind:b")];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let mut ax = axiom(
            AxiomType::ObjectPropertyAssertion,
            "urn:vc:ind:a",
            "urn:vc:ind:b",
        );
        ax.annotations
            .insert("predicate".to_string(), "owl:sameAs".to_string());

        let constraints = map_axioms_to_constraints(&[ax], &mut resolver);
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].kind, 0);
        assert_eq!(constraints[0].params[0], COLOCATE_REST_LENGTH);
    }

    /// hasPart delivered as an ObjectPropertyAssertion (full IRI predicate) →
    /// DISTANCE attraction. Guards the mereological path: the live corpus carries
    /// 5589 `hasPart` triples and 0 `isPartOf`, so without this arm those
    /// relations produce no force.
    #[test]
    fn haspart_property_assertion_maps_to_attract() {
        let nodes = vec![node(20, "urn:vc:ind:engine"), node(21, "urn:vc:ind:piston")];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let mut ax = axiom(
            AxiomType::ObjectPropertyAssertion,
            "urn:vc:ind:engine",
            "urn:vc:ind:piston",
        );
        // Full vocabulary IRI; classify() runs it through local_name() → "hasPart".
        ax.annotations.insert(
            "predicate".to_string(),
            "https://narrativegoldmine.com/ns/v1#hasPart".to_string(),
        );

        let constraints = map_axioms_to_constraints(&[ax], &mut resolver);
        assert_eq!(constraints.len(), 1, "hasPart resolves to one constraint");
        assert_eq!(
            constraints[0].kind, 0,
            "hasPart maps to live DISTANCE = 0 (attract)"
        );
        assert_eq!(
            constraints[0].node_idx[0], 20,
            "subject (whole) is endpoint 0"
        );
        assert_eq!(
            constraints[0].node_idx[1], 21,
            "object (part) is endpoint 1"
        );
        assert_eq!(constraints[0].params[0], SUBCLASS_REST_LENGTH);
        assert_eq!(constraints[0].weight, SUBCLASS_WEIGHT);
    }

    /// Two classes sharing a parent (subClassOf the same node) yield a kind-7
    /// ALIGNMENT constraint on axis Y (params[0] == 1.0), complementing the P4a
    /// Sugiyama Y-by-rank spring. The two subClassOf attractions are still emitted.
    #[test]
    fn siblings_yield_kind7_alignment_constraint() {
        let nodes = vec![
            node(1, "urn:vc:class:Animal"),
            node(2, "urn:vc:class:Dog"),
            node(3, "urn:vc:class:Cat"),
        ];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let axioms = vec![
            // Dog subClassOf Animal
            axiom(
                AxiomType::SubClassOf,
                "urn:vc:class:Dog",
                "urn:vc:class:Animal",
            ),
            // Cat subClassOf Animal  → Dog & Cat are siblings under Animal
            axiom(
                AxiomType::SubClassOf,
                "urn:vc:class:Cat",
                "urn:vc:class:Animal",
            ),
        ];

        let constraints = map_axioms_to_constraints(&axioms, &mut resolver);

        // Two DISTANCE attractions + one sibling ALIGNMENT.
        let alignments: Vec<&ConstraintData> = constraints.iter().filter(|c| c.kind == 7).collect();
        assert_eq!(alignments.len(), 1, "one sibling-alignment constraint");
        let a = alignments[0];
        assert_eq!(a.kind, 7, "sibling alignment maps to live ALIGNMENT = 7");
        assert_eq!(a.count, 2);
        assert_eq!(a.params[0], 1.0, "aligns on axis Y (params[0] == 1.0)");
        assert_eq!(a.weight, SIBLING_ALIGN_WEIGHT);
        // Endpoints are the two children (Dog=2, Cat=3) in insertion order.
        assert_eq!(a.node_idx[0], 2);
        assert_eq!(a.node_idx[1], 3);
    }

    /// A parent with a single child yields NO alignment constraint.
    #[test]
    fn single_child_yields_no_alignment() {
        let nodes = vec![node(1, "urn:vc:class:Animal"), node(2, "urn:vc:class:Dog")];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let axioms = vec![axiom(
            AxiomType::SubClassOf,
            "urn:vc:class:Dog",
            "urn:vc:class:Animal",
        )];

        let constraints = map_axioms_to_constraints(&axioms, &mut resolver);
        assert!(
            constraints.iter().all(|c| c.kind != 7),
            "a lone child produces no sibling alignment"
        );
    }

    /// Unresolved endpoints are counted, not silently dropped.
    #[test]
    fn unresolved_endpoints_are_counted_and_skipped() {
        let nodes = vec![node(1, "urn:vc:class:Known")];
        let mut resolver = IriNodeResolver::from_nodes(&nodes);

        let axioms = vec![axiom(
            AxiomType::SubClassOf,
            "urn:vc:class:Known",
            "urn:vc:class:Unknown",
        )];

        let constraints = map_axioms_to_constraints(&axioms, &mut resolver);
        assert!(
            constraints.is_empty(),
            "axiom with unresolved object is skipped"
        );
        assert!(resolver.unresolved_count() >= 1, "miss is counted");
    }
}
