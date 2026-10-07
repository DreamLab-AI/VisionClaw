use log::info;
use std::collections::{HashMap, VecDeque};

use super::types::{LayoutMode, LayoutModeConfig};

/// Compute positions for a given layout mode.
/// Returns one `(x, y, z)` position per node, in the same order as `nodes`.
pub fn compute_layout(
    mode: &LayoutMode,
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    if nodes.is_empty() {
        return vec![];
    }
    match mode {
        LayoutMode::ForceDirected => {
            // Handled by GPU physics engine; return identity positions.
            vec![(0.0, 0.0, 0.0); nodes.len()]
        }
        LayoutMode::Hierarchical => hierarchical_layout(nodes, edges, config),
        LayoutMode::Radial => radial_layout(nodes, edges, config),
        LayoutMode::Spectral => spectral_layout(nodes, edges, config),
        LayoutMode::Temporal => temporal_layout(nodes, edges, config),
        LayoutMode::Clustered => clustered_layout(nodes, edges, config),
    }
}

// ---------------------------------------------------------------------------
// Helper: build adjacency lists indexed by position in `nodes`
// ---------------------------------------------------------------------------

fn build_adj(
    n: usize,
    id_to_idx: &HashMap<u32, usize>,
    edges: &[(u32, u32, f32)],
) -> Vec<Vec<(usize, f32)>> {
    let mut adj = vec![vec![]; n];
    for &(src, tgt, w) in edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
            adj[si].push((ti, w));
            adj[ti].push((si, w));
        }
    }
    adj
}

// ---------------------------------------------------------------------------
// 1. Hierarchical Layout (Sugiyama-inspired BFS)
// ---------------------------------------------------------------------------

fn hierarchical_layout(
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    info!("Computing hierarchical layout for {} nodes", nodes.len());
    let n = nodes.len();

    let id_to_idx: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    // Build directed in-degree count to find roots
    let mut in_degree = vec![0u32; n];
    for &(src, tgt, _) in edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
            if si != ti {
                in_degree[ti] += 1;
            }
        }
    }

    // Directed adjacency (source -> targets)
    let mut dir_adj: Vec<Vec<usize>> = vec![vec![]; n];
    for &(src, tgt, _) in edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
            if si != ti {
                dir_adj[si].push(ti);
            }
        }
    }

    // BFS depth assignment from roots
    let mut depth = vec![usize::MAX; n];
    let mut queue: VecDeque<usize> = VecDeque::new();

    // Start from nodes with no incoming edges
    for i in 0..n {
        if in_degree[i] == 0 {
            depth[i] = 0;
            queue.push_back(i);
        }
    }

    // If no true roots (cycle), seed with highest-degree nodes
    if queue.is_empty() {
        let undirected_adj = build_adj(n, &id_to_idx, edges);
        let root = (0..n).max_by_key(|&i| undirected_adj[i].len()).unwrap_or(0);
        depth[root] = 0;
        queue.push_back(root);
    }

    while let Some(u) = queue.pop_front() {
        for &v in &dir_adj[u] {
            if depth[v] == usize::MAX {
                depth[v] = depth[u] + 1;
                queue.push_back(v);
            }
        }
    }

    // Assign unreachable nodes to the next layer after the deepest reached
    let max_depth = depth
        .iter()
        .filter(|&&d| d != usize::MAX)
        .max()
        .copied()
        .unwrap_or(0);
    for d in depth.iter_mut() {
        if *d == usize::MAX {
            *d = max_depth + 1;
        }
    }
    let total_layers = max_depth + 2;

    // Group nodes by layer
    let mut layers: Vec<Vec<usize>> = vec![vec![]; total_layers];
    for i in 0..n {
        layers[depth[i]].push(i);
    }

    // Sort each layer by number of connections (hubs first) to reduce crossings
    let undirected_adj = build_adj(n, &id_to_idx, edges);
    for layer in layers.iter_mut() {
        layer.sort_by(|&a, &b| undirected_adj[b].len().cmp(&undirected_adj[a].len()));
    }

    // Assign positions: Y = depth * layer_spacing, X centered per layer
    let layer_spacing = config.layer_spacing;
    let node_spacing = config.node_spacing;
    let mut positions = vec![(0.0f32, 0.0f32, 0.0f32); n];

    for (layer_idx, layer) in layers.iter().enumerate() {
        let y = layer_idx as f32 * layer_spacing;
        let width = (layer.len().saturating_sub(1)) as f32 * node_spacing;
        let x_start = -width / 2.0;
        for (pos_in_layer, &node_idx) in layer.iter().enumerate() {
            let x = x_start + pos_in_layer as f32 * node_spacing;
            positions[node_idx] = (x, y, 0.0);
        }
    }

    positions
}

// ---------------------------------------------------------------------------
// 2. Radial Layout (Centrality Rings)
// ---------------------------------------------------------------------------

fn radial_layout(
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    info!("Computing radial layout for {} nodes", nodes.len());
    let n = nodes.len();
    let id_to_idx: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    // Degree centrality: count edges per node (weighted)
    let mut degree = vec![0.0f32; n];
    for &(src, tgt, w) in edges {
        if let Some(&si) = id_to_idx.get(&src) {
            degree[si] += w;
        }
        if let Some(&ti) = id_to_idx.get(&tgt) {
            degree[ti] += w;
        }
    }

    // Sort node indices by descending degree
    let mut sorted_indices: Vec<usize> = (0..n).collect();
    sorted_indices.sort_by(|&a, &b| {
        degree[b]
            .partial_cmp(&degree[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let ring_count = config.ring_count.max(1) as usize;
    let base_radius = config.node_spacing * 2.0;
    let ring_step = config.layer_spacing;

    // Golden angle in radians for even angular distribution within a ring
    let golden_angle = std::f32::consts::PI * (3.0 - 5.0f32.sqrt()); // ~2.399 rad = 137.508°

    let mut positions = vec![(0.0f32, 0.0f32, 0.0f32); n];

    // Ring 0 gets the top node (may be just 1 node at center if ring_count is large)
    // Distribute remaining nodes across rings proportionally
    let nodes_per_ring = {
        let mut counts = vec![0usize; ring_count];
        for (rank, _) in sorted_indices.iter().enumerate() {
            let ring = ((rank as f32 / n as f32) * ring_count as f32).floor() as usize;
            let ring = ring.min(ring_count - 1);
            counts[ring] += 1;
        }
        counts
    };

    let mut ring_offsets = vec![0usize; ring_count + 1];
    for i in 0..ring_count {
        ring_offsets[i + 1] = ring_offsets[i] + nodes_per_ring[i];
    }

    for (rank, &node_idx) in sorted_indices.iter().enumerate() {
        let ring = ((rank as f32 / n as f32) * ring_count as f32).floor() as usize;
        let ring = ring.min(ring_count - 1);

        let pos_in_ring = rank - ring_offsets[ring];
        let count_in_ring = nodes_per_ring[ring].max(1);

        if ring == 0 && count_in_ring == 1 {
            // Single highest-centrality node at origin
            positions[node_idx] = (0.0, 0.0, 0.0);
            continue;
        }

        let radius = base_radius + ring as f32 * ring_step;
        // Use golden angle for nice non-overlapping angular distribution within the ring
        let angle = pos_in_ring as f32 * golden_angle;
        let x = radius * angle.cos();
        let z = radius * angle.sin();
        positions[node_idx] = (x, 0.0, z);
    }

    positions
}

// ---------------------------------------------------------------------------
// 3. Spectral Layout (Laplacian Eigenvectors via Power Iteration)
// ---------------------------------------------------------------------------

fn spectral_layout(
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    info!("Computing spectral layout for {} nodes", nodes.len());
    let n = nodes.len();

    if n <= 3 {
        // Degenerate case: fall back to radial
        return radial_layout(nodes, edges, config);
    }

    let id_to_idx: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    // Build weighted adjacency and degree vectors
    let mut adj: Vec<Vec<(usize, f32)>> = vec![vec![]; n];
    let mut degree = vec![0.0f32; n];

    for &(src, tgt, w) in edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
            if si != ti {
                let w = w.max(1e-6);
                adj[si].push((ti, w));
                adj[ti].push((si, w));
                degree[si] += w;
                degree[ti] += w;
            }
        }
    }

    // Isolated nodes: assign degree = 1 to avoid division by zero
    for d in degree.iter_mut() {
        if *d < 1e-9 {
            *d = 1.0;
        }
    }

    // Random-walk transition matrix multiplication: v -> D^{-1} A v
    // We want the LARGEST eigenvectors of D^{-1}A (which correspond to
    // smallest eigenvectors of the Laplacian L = I - D^{-1}A).
    // eigenvector for eigenvalue 1 is the constant vector — we deflect it.
    let matvec = |v: &[f32]| -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let mut sum = 0.0f32;
            for &(j, w) in &adj[i] {
                sum += w * v[j];
            }
            out[i] = sum / degree[i];
        }
        out
    };

    let dot = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b.iter()).map(|(x, y)| x * y).sum() };
    let norm = |v: &[f32]| -> f32 { dot(v, v).sqrt() };
    let scale = |v: &mut Vec<f32>, s: f32| {
        for x in v.iter_mut() {
            *x *= s;
        }
    };
    let _axpy = |v: &mut Vec<f32>, alpha: f32, u: &[f32]| {
        for (x, &y) in v.iter_mut().zip(u.iter()) {
            *x += alpha * y;
        }
    };

    // Seed vectors using deterministic pseudo-random (golden-ratio hash)
    let seed_vec = |seed: u64, len: usize| -> Vec<f32> {
        (0..len)
            .map(|i| {
                let h = (i as u64).wrapping_mul(2654435761u64).wrapping_add(seed);
                let f = (h & 0xFFFF) as f32 / 65535.0;
                f * 2.0 - 1.0
            })
            .collect()
    };

    // Deflate against found eigenvectors (Gram-Schmidt)
    let deflate = |v: &mut Vec<f32>, basis: &[Vec<f32>]| {
        for b in basis {
            let proj = dot(v, b);
            for (x, &y) in v.iter_mut().zip(b.iter()) {
                *x -= proj * y;
            }
        }
    };

    // Also deflate against constant vector (eigenvalue=1 trivial eigenvector)
    let constant_vec: Vec<f32> = vec![1.0 / (n as f32).sqrt(); n];

    let iters = 80usize;
    let mut eigenvecs: Vec<Vec<f32>> = Vec::with_capacity(3);

    for ev_idx in 0..3usize {
        let mut v = seed_vec(ev_idx as u64 * 17 + 42, n);
        // Orthogonalize against constant and prior eigenvectors
        deflate(&mut v, std::slice::from_ref(&constant_vec));
        deflate(&mut v, &eigenvecs);
        let n_v = norm(&v);
        if n_v < 1e-9 {
            v = seed_vec(ev_idx as u64 * 1337, n);
            deflate(&mut v, std::slice::from_ref(&constant_vec));
            deflate(&mut v, &eigenvecs);
        }
        let n_v = norm(&v);
        if n_v > 1e-9 {
            scale(&mut v, 1.0 / n_v);
        }

        for _ in 0..iters {
            let mut mv = matvec(&v);
            // Re-deflate each iteration for numerical stability
            deflate(&mut mv, std::slice::from_ref(&constant_vec));
            deflate(&mut mv, &eigenvecs);
            let n_mv = norm(&mv);
            if n_mv < 1e-9 {
                break;
            }
            scale(&mut mv, 1.0 / n_mv);
            v = mv;
        }

        // Rayleigh quotient sign normalization: make first nonzero component positive
        if let Some(&first_nonzero) = v.iter().find(|&&x| x.abs() > 1e-9) {
            if first_nonzero < 0.0 {
                scale(&mut v, -1.0);
            }
        }

        eigenvecs.push(v);
    }

    // Scale eigenvectors to fill a comfortable volume
    let scale_factor = config.layer_spacing * 2.0;
    let rescale = |ev: &[f32]| -> Vec<f32> {
        let max_abs = ev.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        if max_abs < 1e-9 {
            return ev.to_vec();
        }
        ev.iter().map(|x| x / max_abs * scale_factor).collect()
    };

    let ex = rescale(&eigenvecs[0]);
    let ey = rescale(&eigenvecs[1]);
    let ez = rescale(&eigenvecs[2]);

    (0..n).map(|i| (ex[i], ey[i], ez[i])).collect()
}

// ---------------------------------------------------------------------------
// 4. Temporal Layout (Z = creation order, X/Y = 2D radial spread)
// ---------------------------------------------------------------------------

fn temporal_layout(
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    info!("Computing temporal layout for {} nodes", nodes.len());
    let n = nodes.len();

    if n == 0 {
        return vec![];
    }

    let id_to_idx: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    // Degree centrality for XY spread
    let mut degree = vec![0.0f32; n];
    for &(src, tgt, w) in edges {
        if let Some(&si) = id_to_idx.get(&src) {
            degree[si] += w;
        }
        if let Some(&ti) = id_to_idx.get(&tgt) {
            degree[ti] += w;
        }
    }

    let max_degree = degree.iter().copied().fold(0.0f32, f32::max).max(1.0);

    // Z range: total temporal extent
    let z_range = config.layer_spacing * (n as f32 / 10.0).max(5.0);
    let z_scale = z_range / (n as f32 - 1.0).max(1.0);

    // XY: golden-angle spiral based on degree (high degree near center)
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    let xy_radius_max = config.node_spacing * (n as f32).sqrt();

    let mut positions = vec![(0.0f32, 0.0f32, 0.0f32); n];

    for (idx, _) in nodes.iter().enumerate() {
        // Z follows creation order (node index as temporal proxy)
        let z = idx as f32 * z_scale - z_range / 2.0;

        // XY: nodes with higher degree are closer to the center
        let centrality = degree[idx] / max_degree; // [0,1], 1 = most central
        let r = xy_radius_max * (1.0 - centrality * 0.8).max(0.05);
        let angle = idx as f32 * golden;
        let x = r * angle.cos();
        let y = r * angle.sin();

        positions[idx] = (x, y, z);
    }

    positions
}

// ---------------------------------------------------------------------------
// 5. Clustered Layout (Label Propagation + Fibonacci Sphere)
// ---------------------------------------------------------------------------

fn clustered_layout(
    nodes: &[(u32, String)],
    edges: &[(u32, u32, f32)],
    config: &LayoutModeConfig,
) -> Vec<(f32, f32, f32)> {
    info!("Computing clustered layout for {} nodes", nodes.len());
    let n = nodes.len();

    if n == 0 {
        return vec![];
    }

    let id_to_idx: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let adj = build_adj(n, &id_to_idx, edges);

    // --- Label Propagation ---
    let mut labels: Vec<usize> = (0..n).collect();
    let iters = 50usize;

    for _ in 0..iters {
        let mut changed = false;
        // Randomize update order deterministically (Fisher-Yates with LCG)
        let mut order: Vec<usize> = (0..n).collect();
        let mut rng_state: u64 = 0xDEADBEEF;
        for i in (1..n).rev() {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng_state >> 33) as usize % (i + 1);
            order.swap(i, j);
        }

        for &node in &order {
            if adj[node].is_empty() {
                continue;
            }

            // Count neighbor labels (weighted)
            let mut label_counts: HashMap<usize, f32> = HashMap::new();
            for &(nb, w) in &adj[node] {
                *label_counts.entry(labels[nb]).or_insert(0.0) += w;
            }

            // Adopt the most frequent neighbor label
            if let Some((&best_label, _)) = label_counts
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            {
                if labels[node] != best_label {
                    labels[node] = best_label;
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    // Collect clusters
    let mut cluster_members: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        cluster_members.entry(labels[i]).or_default().push(i);
    }

    // Sort clusters by size descending for stable ordering
    let mut clusters: Vec<(usize, Vec<usize>)> = cluster_members.into_iter().collect();
    clusters.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let num_clusters = clusters.len();

    // Place cluster centroids on a Fibonacci sphere
    let cluster_spread = config.layer_spacing * (num_clusters as f32).sqrt().max(1.0);
    let golden_ratio = (1.0 + 5.0f32.sqrt()) / 2.0;

    let fibonacci_sphere_point = |i: usize, total: usize| -> (f32, f32, f32) {
        if total == 1 {
            return (0.0, 0.0, 0.0);
        }
        let theta = 2.0 * std::f32::consts::PI * i as f32 / golden_ratio;
        let phi = (1.0 - 2.0 * (i as f32 + 0.5) / total as f32).acos();
        (
            cluster_spread * phi.sin() * theta.cos(),
            cluster_spread * phi.cos(),
            cluster_spread * phi.sin() * theta.sin(),
        )
    };

    let mut positions = vec![(0.0f32, 0.0f32, 0.0f32); n];

    for (cluster_idx, (_label, members)) in clusters.iter().enumerate() {
        let (cx, cy, cz) = fibonacci_sphere_point(cluster_idx, num_clusters);

        let cluster_size = members.len();
        // Intra-cluster radius proportional to sqrt(cluster_size)
        let intra_radius = config.node_spacing * (cluster_size as f32).sqrt().max(1.0);

        // Place intra-cluster nodes on a smaller Fibonacci sphere around the centroid
        for (local_idx, &node_idx) in members.iter().enumerate() {
            let (lx, ly, lz) = fibonacci_sphere_point(local_idx, cluster_size);
            positions[node_idx] = (
                cx + lx * intra_radius / cluster_spread.max(1.0),
                cy + ly * intra_radius / cluster_spread.max(1.0),
                cz + lz * intra_radius / cluster_spread.max(1.0),
            );
        }
    }

    positions
}
