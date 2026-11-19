//! CSR construction for the physics graph (ADR-2204 D7: drawing is separate from physics).
//!
//! One pure function turns the weighting stage's physics pairs into the symmetric CSR the force
//! kernels read. The pairs come from `visionclaw_domain::weighting::physics_pairs`. Within one
//! weighting mode the topology depends only on the edges and the node index, never on the
//! weights, so a profile change re-uploads weights alone with `row_offsets` and `col_indices`
//! identical; switching mode changes the pair list (Evidence emits one pair per connected
//! pair), hence the topology.

use std::collections::HashMap;

use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::weighting::PhysicsPair;

/// The symmetric adjacency of a graph in CSR form, plus the per-node lists it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Csr {
    /// `num_nodes + 1` offsets into `col_indices`.
    pub row_offsets: Vec<u32>,
    /// Neighbour GPU indices, row by row.
    pub col_indices: Vec<u32>,
    /// One weight per `col_indices` entry.
    pub weights: Vec<f32>,
    /// `(neighbour, weight)` per node, the rows before flattening.
    pub adjacency: Vec<Vec<(u32, f32)>>,
}

/// Build the CSR for `edges`, where `weights[i]` is the weight of `edges[i]`.
///
/// The Legacy path: one [`PhysicsPair`] per stored edge, in edge order, through
/// [`build_csr_from_pairs`].
///
/// # Panics
///
/// Panics if `weights.len() != edges.len()`: weights are aligned to edges by construction.
pub fn build_csr(
    num_nodes: usize,
    node_indices: &HashMap<u32, usize>,
    edges: &[Edge],
    weights: &[f32],
) -> Csr {
    assert_eq!(
        weights.len(),
        edges.len(),
        "one weight per edge, aligned to the edge order"
    );
    let pairs: Vec<PhysicsPair> = edges
        .iter()
        .zip(weights)
        .map(|(e, &weight)| PhysicsPair {
            source: e.source,
            target: e.target,
            weight,
        })
        .collect();
    build_csr_from_pairs(num_nodes, node_indices, &pairs)
}

/// Build the CSR for a physics pair list (ADR-2204 D3: the weighting stage's output).
///
/// A pair whose endpoint is not in `node_indices` is skipped. Every other pair adds an entry to
/// its source row and, unless it is a self-loop, to its target row, both with the pair's
/// weight. Rows keep pair order.
pub fn build_csr_from_pairs(
    num_nodes: usize,
    node_indices: &HashMap<u32, usize>,
    pairs: &[PhysicsPair],
) -> Csr {
    let mut adjacency: Vec<Vec<(u32, f32)>> = vec![Vec::new(); num_nodes];
    for pair in pairs {
        if let (Some(&src), Some(&tgt)) = (
            node_indices.get(&pair.source),
            node_indices.get(&pair.target),
        ) {
            adjacency[src].push((tgt as u32, pair.weight));
            if src != tgt {
                adjacency[tgt].push((src as u32, pair.weight));
            }
        }
    }
    let mut row_offsets = vec![0u32; num_nodes + 1];
    let mut col_indices = Vec::new();
    let mut flat_weights = Vec::new();
    for (i, row) in adjacency.iter().enumerate() {
        row_offsets[i] = col_indices.len() as u32;
        for &(target, weight) in row {
            col_indices.push(target);
            flat_weights.push(weight);
        }
    }
    row_offsets[num_nodes] = col_indices.len() as u32;
    Csr {
        row_offsets,
        col_indices,
        weights: flat_weights,
        adjacency,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::EdgeProvenance;
    use visionclaw_domain::models::node::Node;
    use visionclaw_domain::weighting::{physics_edge_weights, WeightingProfile};

    fn fixture() -> (Vec<Node>, Vec<Edge>) {
        let nodes: Vec<Node> = (10..16u32)
            .map(|i| Node::new_with_id(format!("p{i}"), Some(i)))
            .collect();
        let sub = |c: u32, p: u32| {
            let mut e = Edge::new(c, p, 1.6)
                .with_edge_type("hierarchical".into())
                .with_provenance(EdgeProvenance::Asserted);
            e.owl_property_iri = Some("http://www.w3.org/2000/01/rdf-schema#subClassOf".into());
            e
        };
        let edges = vec![
            sub(11, 10),
            sub(12, 11),
            sub(13, 11),
            Edge::new(14, 15, 0.7),
            Edge::new(12, 99, 1.0), // unknown endpoint: skipped
            Edge::new(13, 13, 0.3), // self-loop: one entry
            Edge::new(10, 14, f32::from_bits(0x3dcc_cccd)),
        ];
        (nodes, edges)
    }

    fn index(nodes: &[Node]) -> HashMap<u32, usize> {
        nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect()
    }

    /// The weights of the CSR with the profile off are the stored edge weights, bit for bit,
    /// in the same slots the pre-ADR-2204 inline build produced.
    #[test]
    fn off_profile_csr_is_byte_identical_to_the_legacy_inline_build() {
        let (nodes, edges) = fixture();
        let idx = index(&nodes);

        // The loop that used to live in `try_upload_pending_graph_data`.
        let mut adj: Vec<Vec<(u32, f32)>> = vec![Vec::new(); nodes.len()];
        for e in &edges {
            if let (Some(&s), Some(&t)) = (idx.get(&e.source), idx.get(&e.target)) {
                adj[s].push((t as u32, e.weight));
                if s != t {
                    adj[t].push((s as u32, e.weight));
                }
            }
        }
        let mut ro = vec![0u32; nodes.len() + 1];
        let (mut ci, mut w) = (Vec::new(), Vec::new());
        let mut count = 0u32;
        for (i, row) in adj.iter().enumerate() {
            ro[i] = count;
            for &(t, x) in row {
                ci.push(t);
                w.push(x);
                count += 1;
            }
        }
        ro[nodes.len()] = count;

        let weights = physics_edge_weights(&nodes, &edges, &WeightingProfile::default());
        let csr = build_csr(nodes.len(), &idx, &edges, &weights);
        assert_eq!(csr.row_offsets, ro);
        assert_eq!(csr.col_indices, ci);
        let want: Vec<u32> = w.iter().map(|x| x.to_bits()).collect();
        let got: Vec<u32> = csr.weights.iter().map(|x| x.to_bits()).collect();
        assert_eq!(got, want);
    }

    /// A profile change moves weights only; the topology arrays are identical.
    #[test]
    fn a_profile_change_keeps_row_offsets_and_col_indices() {
        let (nodes, edges) = fixture();
        let idx = index(&nodes);
        let off = physics_edge_weights(&nodes, &edges, &WeightingProfile::default());
        let on = physics_edge_weights(
            &nodes,
            &edges,
            &WeightingProfile {
                hierarchy_affinity: 3.0,
                experiment: None,
                ..Default::default()
            },
        );
        let a = build_csr(nodes.len(), &idx, &edges, &off);
        let b = build_csr(nodes.len(), &idx, &edges, &on);
        assert_eq!(a.row_offsets, b.row_offsets);
        assert_eq!(a.col_indices, b.col_indices);
        assert_ne!(a.weights, b.weights, "the boost must change some weight");
        assert_eq!(a.weights.len(), b.weights.len());
    }

    /// The Legacy pair path is the old per-edge build, slot for slot, and the golden above
    /// holds through it.
    #[test]
    fn legacy_pairs_build_the_same_csr_as_the_edge_path() {
        use visionclaw_domain::weighting::physics_pairs;
        let (nodes, edges) = fixture();
        let idx = index(&nodes);
        let profile = WeightingProfile::default();
        let w = physics_edge_weights(&nodes, &edges, &profile);
        let by_edge = build_csr(nodes.len(), &idx, &edges, &w);
        let by_pair =
            build_csr_from_pairs(nodes.len(), &idx, &physics_pairs(&nodes, &edges, &profile));
        assert_eq!(by_edge, by_pair);
    }

    /// Evidence mode gives the GPU one entry per direction per pair, and the CSR itself (not
    /// only the weights) does not depend on the order edges were stored in.
    #[test]
    fn evidence_csr_is_collapsed_and_invariant_to_edge_order() {
        use visionclaw_domain::weighting::{physics_pairs, WeightingMode};
        let nodes: Vec<Node> = (1..=4u32)
            .map(|i| Node::new_with_id(format!("p{i}"), Some(i)))
            .collect();
        let l = |a, b, t: &str| {
            Edge::new(a, b, 7.0)
                .with_edge_type(t.into())
                .with_provenance(EdgeProvenance::Asserted)
        };
        let edges = vec![
            l(1, 2, "explicit_link"),
            l(2, 1, "explicit_link"), // a second encoding of the same pair
            l(2, 3, "explicit_link"),
            l(3, 4, "associative"),
        ];
        let idx = index(&nodes);
        let profile = WeightingProfile {
            mode: WeightingMode::Evidence,
            ..Default::default()
        };
        let a = build_csr_from_pairs(4, &idx, &physics_pairs(&nodes, &edges, &profile));
        let rev: Vec<Edge> = edges.iter().rev().cloned().collect();
        let b = build_csr_from_pairs(4, &idx, &physics_pairs(&nodes, &rev, &profile));
        assert_eq!(a, b);
        // 3 pairs → 6 directed entries; the legacy build would hold 8.
        assert_eq!(a.col_indices.len(), 6);
        let legacy = physics_edge_weights(&nodes, &edges, &WeightingProfile::default());
        assert_eq!(build_csr(4, &idx, &edges, &legacy).col_indices.len(), 8);
    }

    #[test]
    #[should_panic(expected = "one weight per edge")]
    fn misaligned_weights_are_refused() {
        let (nodes, edges) = fixture();
        build_csr(nodes.len(), &index(&nodes), &edges, &[1.0]);
    }
}
