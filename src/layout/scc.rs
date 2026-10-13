//! Strongly connected components and cycle-safe ranking for directed layouts.
//!
//! Written from Tarjan's 1972 algorithm (iterative, so a deep subclass chain
//! cannot overflow the stack) and the textbook condensation-DAG
//! longest-path ranking. Indices are positions in the caller's node slice.

/// Strongly connected components of a directed graph.
pub struct Sccs {
    /// `component[v]` = id of the component containing node `v`.
    pub component: Vec<usize>,
    /// Number of components. Ids are in reverse topological order of the
    /// condensation: an edge `a -> b` between components has `id(a) > id(b)`.
    pub count: usize,
}

impl Sccs {
    /// Size of each component, indexed by component id.
    pub fn sizes(&self) -> Vec<usize> {
        let mut sizes = vec![0; self.count];
        for &c in &self.component {
            sizes[c] += 1;
        }
        sizes
    }

    /// Nodes that sit on a cycle: members of a component with more than one
    /// node, or a node with a self-loop in `adj`.
    pub fn cyclic_nodes(&self, adj: &[Vec<usize>]) -> Vec<usize> {
        let sizes = self.sizes();
        (0..self.component.len())
            .filter(|&v| sizes[self.component[v]] > 1 || adj[v].contains(&v))
            .collect()
    }
}

/// Directed adjacency by position in `node_ids`. Edges whose endpoints are not
/// both in `node_ids` are skipped; self-loops are kept (`cyclic_nodes` needs
/// them, and `longest_path_ranks` ignores edges inside a component). The one
/// builder shared by the hierarchical layout and the cycle report, so both
/// agree on what the graph is.
pub fn directed_adjacency(
    node_ids: &[u32],
    edges: impl IntoIterator<Item = (u32, u32)>,
) -> Vec<Vec<usize>> {
    let index: std::collections::HashMap<u32, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let mut adj = vec![Vec::new(); node_ids.len()];
    for (s, t) in edges {
        if let (Some(&si), Some(&ti)) = (index.get(&s), index.get(&t)) {
            adj[si].push(ti);
        }
    }
    adj
}

/// Directed cycles in a graph, reported by node id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReport {
    /// Every node on a directed cycle (multi-node component or self-loop), ascending.
    pub cyclic_node_ids: Vec<u32>,
    /// Multi-node strongly connected components, largest first; ids ascending within each.
    pub components: Vec<Vec<u32>>,
}

/// Cycle report for `edges` over `node_ids`.
pub fn cycle_report(node_ids: &[u32], edges: impl IntoIterator<Item = (u32, u32)>) -> CycleReport {
    let adj = directed_adjacency(node_ids, edges);
    let sccs = tarjan(&adj);
    let mut cyclic_node_ids: Vec<u32> = sccs
        .cyclic_nodes(&adj)
        .into_iter()
        .map(|v| node_ids[v])
        .collect();
    cyclic_node_ids.sort_unstable();

    let sizes = sccs.sizes();
    let mut by_component: std::collections::HashMap<usize, Vec<u32>> = Default::default();
    for (v, &c) in sccs.component.iter().enumerate() {
        if sizes[c] > 1 {
            by_component.entry(c).or_default().push(node_ids[v]);
        }
    }
    let mut components: Vec<Vec<u32>> = by_component.into_values().collect();
    for c in &mut components {
        c.sort_unstable();
    }
    components.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    CycleReport {
        cyclic_node_ids,
        components,
    }
}

/// Tarjan's algorithm over `adj` (directed adjacency by node index).
pub fn tarjan(adj: &[Vec<usize>]) -> Sccs {
    const UNSEEN: usize = usize::MAX;
    let n = adj.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![UNSEEN; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0usize;
    let mut count = 0usize;

    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        // Explicit call stack of (node, next child offset).
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;

        while let Some(&mut (v, ref mut child)) = call.last_mut() {
            if *child < adj[v].len() {
                let w = adj[v][*child];
                *child += 1;
                if index[w] == UNSEEN {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(parent, _)) = call.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
                    loop {
                        let w = stack.pop().expect("tarjan stack holds the component");
                        on_stack[w] = false;
                        component[w] = count;
                        if w == v {
                            break;
                        }
                    }
                    count += 1;
                }
            }
        }
    }
    Sccs { component, count }
}

/// Layer index per node: the longest path from any source of the condensation
/// DAG. Every node of one cycle shares a layer, and every edge that leaves a
/// cycle points to a strictly deeper layer, so cyclic graphs rank without
/// dumping unreachable nodes on a trailing layer.
pub fn longest_path_ranks(adj: &[Vec<usize>]) -> Vec<usize> {
    let sccs = tarjan(adj);
    let mut rank = vec![0usize; sccs.count];
    // Component ids descend along edges, so walk them from highest to lowest:
    // every predecessor is final before its successors are visited.
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); sccs.count];
    for (v, &c) in sccs.component.iter().enumerate() {
        members[c].push(v);
    }
    for c in (0..sccs.count).rev() {
        for &v in &members[c] {
            for &w in &adj[v] {
                let cw = sccs.component[w];
                if cw != c {
                    rank[cw] = rank[cw].max(rank[c] + 1);
                }
            }
        }
    }
    sccs.component.iter().map(|&c| rank[c]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_has_singleton_components_and_longest_path_ranks() {
        // 0 -> 1 -> 3, 0 -> 2 -> 3, 0 -> 3
        let adj = vec![vec![1, 2, 3], vec![3], vec![3], vec![]];
        let s = tarjan(&adj);
        assert_eq!(s.count, 4);
        assert!(s.cyclic_nodes(&adj).is_empty());
        assert_eq!(longest_path_ranks(&adj), vec![0, 1, 1, 2]);
    }

    #[test]
    fn a_cycle_shares_one_layer_and_its_exits_go_deeper() {
        // 0 -> 1 -> 2 -> 1 (cycle {1,2}), 2 -> 3
        let adj = vec![vec![1], vec![2], vec![1, 3], vec![]];
        let s = tarjan(&adj);
        assert_eq!(s.component[1], s.component[2]);
        assert_ne!(s.component[0], s.component[1]);
        assert_eq!(s.cyclic_nodes(&adj), vec![1, 2]);
        assert_eq!(longest_path_ranks(&adj), vec![0, 1, 1, 2]);
    }

    #[test]
    fn a_pure_cycle_with_no_roots_still_ranks() {
        let adj = vec![vec![1], vec![2], vec![0]];
        assert_eq!(longest_path_ranks(&adj), vec![0, 0, 0]);
    }

    #[test]
    fn self_loops_count_as_cyclic() {
        let adj = vec![vec![0], vec![]];
        let s = tarjan(&adj);
        assert_eq!(s.cyclic_nodes(&adj), vec![0]);
    }

    #[test]
    fn report_maps_positions_back_to_node_ids() {
        // ids 10..14; 11 <-> 12 cycle, 13 self-loop, edge to unknown id 99 skipped
        let ids = [10, 11, 12, 13, 14];
        let r = cycle_report(
            &ids,
            [(10, 11), (11, 12), (12, 11), (13, 13), (12, 14), (12, 99)],
        );
        assert_eq!(r.cyclic_node_ids, vec![11, 12, 13]);
        assert_eq!(r.components, vec![vec![11, 12]]);
    }

    #[test]
    fn an_acyclic_graph_reports_nothing() {
        let r = cycle_report(&[1, 2, 3], [(1, 2), (2, 3), (1, 3)]);
        assert_eq!(r, CycleReport::default());
    }

    #[test]
    fn a_long_chain_does_not_overflow_the_stack() {
        let n = 200_000;
        let adj: Vec<Vec<usize>> = (0..n)
            .map(|i| if i + 1 < n { vec![i + 1] } else { vec![] })
            .collect();
        let ranks = longest_path_ranks(&adj);
        assert_eq!(ranks[n - 1], n - 1);
    }
}
