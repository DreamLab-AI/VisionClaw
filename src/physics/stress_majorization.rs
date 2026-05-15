//! Stress majorization solver for knowledge graph layout optimization
//!
//! This module implements a stress majorization algorithm that optimizes node positions
//! to satisfy multiple constraint types while minimizing layout stress. The solver uses
//! efficient matrix operations and integrates with the GPU physics pipeline for
//! high-performance real-time optimization.
//!
//! ## Algorithm Overview
//!
//! Stress majorization works by:
//! 1. Computing the stress function based on distance differences between ideal and actual positions
//! 2. Using majorization to create a convex approximation of the stress function
//! 3. Iteratively minimizing the majorized function to find optimal positions
//! 4. Incorporating constraints through penalty methods or Lagrange multipliers
//!
//! ## Performance Features
//!
//! - GPU-accelerated matrix operations for large graphs
//! - Sparse matrix representations for efficient computation
//! - Adaptive step sizing and convergence detection
//! - Multi-threaded CPU fallback for smaller graphs
//! - Memory-efficient algorithms for very large datasets

#[cfg(feature = "gpu")]
use cudarc::driver::CudaDevice;
use log::{debug, error, info, trace, warn};
use nalgebra::DMatrix;
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::{
    constraints::{AdvancedParams, Constraint, ConstraintKind, ConstraintSet},
    graph::GraphData,
};

#[derive(Debug, Clone)]
pub struct StressMajorizationConfig {
    pub max_iterations: u32,

    pub tolerance: f32,

    pub step_size: f32,

    pub adaptive_step: bool,

    pub constraint_weight: f32,

    pub use_gpu: bool,

    pub min_improvement: f32,

    pub convergence_check_interval: u32,
}

impl Default for StressMajorizationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            tolerance: 1e-6,
            step_size: 0.1,
            adaptive_step: true,
            constraint_weight: 1.0,
            use_gpu: true,
            min_improvement: 1e-8,
            convergence_check_interval: 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub final_stress: f32,

    pub iterations: u32,

    pub converged: bool,

    pub constraint_scores: HashMap<ConstraintKind, f32>,

    pub computation_time: u64,
}

pub struct StressMajorizationSolver {
    config: StressMajorizationConfig,
    #[cfg(feature = "gpu")]
    _gpu_context: Option<Arc<CudaDevice>>,
    cached_distance_matrix: Option<DMatrix<f32>>,
    cached_weight_matrix: Option<DMatrix<f32>>,
    iteration_history: Vec<f32>,
}

impl Clone for StressMajorizationSolver {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            #[cfg(feature = "gpu")]
            _gpu_context: self._gpu_context.clone(),
            cached_distance_matrix: self.cached_distance_matrix.clone(),
            cached_weight_matrix: self.cached_weight_matrix.clone(),
            iteration_history: self.iteration_history.clone(),
        }
    }
}

impl StressMajorizationSolver {
    pub fn new() -> Self {
        Self::with_config(StressMajorizationConfig::default())
    }

    pub fn with_config(config: StressMajorizationConfig) -> Self {
        #[cfg(feature = "gpu")]
        let gpu_context = if config.use_gpu {
            match Self::initialize_gpu() {
                Ok(device) => Some(device),
                Err(e) => {
                    warn!("Failed to initialize GPU, falling back to CPU: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            config,
            #[cfg(feature = "gpu")]
            _gpu_context: gpu_context,
            cached_distance_matrix: None,
            cached_weight_matrix: None,
            iteration_history: Vec::new(),
        }
    }

    pub fn from_advanced_params(params: &AdvancedParams) -> Self {
        let config = StressMajorizationConfig {
            max_iterations: params.stress_step_interval_frames * 10,
            constraint_weight: params.constraint_force_weight,
            step_size: 0.05,
            tolerance: 1e-5,
            adaptive_step: params.adaptive_force_scaling,
            ..Default::default()
        };

        Self::with_config(config)
    }

    #[cfg(feature = "gpu")]
    fn initialize_gpu() -> Result<Arc<CudaDevice>, Box<dyn std::error::Error>> {
        info!("Initializing GPU device for stress majorization");
        let device = CudaDevice::new(0)?;
        info!("Successfully initialized CUDA device for stress majorization");
        Ok(device)
    }

    /// Maximum number of nodes for which a dense (N x N) matrix is allocated.
    /// At 10,000 nodes the distance + weight matrices consume ~800 MB of f32
    /// storage (2 x 10000^2 x 4 bytes). Beyond this threshold the solver
    /// refuses to allocate and returns an error directing the caller to use a
    /// sparse representation (see P1-23).
    const DENSE_MATRIX_NODE_LIMIT: usize = 10_000;

    pub fn optimize(
        &mut self,
        graph_data: &mut GraphData,
        constraints: &ConstraintSet,
    ) -> Result<OptimizationResult, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        info!(
            "Starting stress majorization optimization for {} nodes",
            graph_data.nodes.len()
        );

        if graph_data.nodes.is_empty() {
            return Err("Cannot optimize empty graph".into());
        }

        // P1-23: Guard against OOM from dense N x N matrix allocation.
        let n = graph_data.nodes.len();
        if n > Self::DENSE_MATRIX_NODE_LIMIT {
            let estimated_mb = (2 * n * n * std::mem::size_of::<f32>()) / (1024 * 1024);
            error!(
                "Stress majorization: refusing dense matrix allocation for {} nodes \
                 (~{} MB). Use sparse mode for graphs exceeding {} nodes.",
                n,
                estimated_mb,
                Self::DENSE_MATRIX_NODE_LIMIT,
            );
            return Err(format!(
                "Graph has {} nodes, exceeding the dense matrix limit of {}. \
                 Dense distance+weight matrices would require ~{} MB. \
                 A sparse solver is required for graphs of this size.",
                n,
                Self::DENSE_MATRIX_NODE_LIMIT,
                estimated_mb,
            )
            .into());
        }

        self.compute_distance_matrix(graph_data)?;
        self.compute_weight_matrix(graph_data)?;

        self.initialize_positions(graph_data)?;

        let mut best_stress = f32::INFINITY;
        let mut current_positions = self.extract_positions(graph_data);
        let mut iterations = 0;
        let mut converged = false;

        info!(
            "Beginning iterative optimization with {} constraints",
            constraints.constraints.len()
        );

        while iterations < self.config.max_iterations && !converged {
            let current_stress = self.compute_stress(&current_positions, graph_data)?;

            if current_stress < best_stress {
                best_stress = current_stress;
                self.apply_positions(graph_data, &current_positions)?;
            }

            let gradient = self.compute_gradient(&current_positions, graph_data, constraints)?;
            let new_positions = self.update_positions(&current_positions, &gradient)?;

            if iterations % self.config.convergence_check_interval == 0 {
                let improvement = (best_stress - current_stress) / best_stress.max(1e-10);
                converged = improvement < self.config.tolerance;

                if iterations % 100 == 0 {
                    debug!(
                        "Iteration {}: stress = {:.6}, improvement = {:.8}",
                        iterations, current_stress, improvement
                    );
                }
            }

            current_positions = new_positions;
            iterations += 1;
            self.iteration_history.push(current_stress);
        }

        self.apply_positions(graph_data, &current_positions)?;

        let constraint_scores = self.compute_constraint_scores(graph_data, constraints)?;

        let result = OptimizationResult {
            final_stress: best_stress,
            iterations,
            converged,
            constraint_scores,
            computation_time: start_time.elapsed().as_millis() as u64,
        };

        info!(
            "Stress majorization completed: {} iterations, stress = {:.6}, converged = {}",
            iterations, best_stress, converged
        );

        Ok(result)
    }

    fn compute_distance_matrix(
        &mut self,
        graph_data: &GraphData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let n = graph_data.nodes.len();
        let mut distance_matrix = DMatrix::zeros(n, n);

        for (i, node_a) in graph_data.nodes.iter().enumerate() {
            for (j, node_b) in graph_data.nodes.iter().enumerate() {
                if i == j {
                    distance_matrix[(i, j)] = 0.0;
                } else {
                    let direct_distance = graph_data
                        .edges
                        .iter()
                        .find(|edge| {
                            (edge.source == node_a.id && edge.target == node_b.id)
                                || (edge.source == node_b.id && edge.target == node_a.id)
                        })
                        .map(|_| 1.0)
                        .unwrap_or(f32::INFINITY);

                    distance_matrix[(i, j)] = direct_distance;
                }
            }
        }

        let num_landmarks = (n as f32).sqrt().ceil() as usize;
        let num_landmarks = num_landmarks.min(n).max(10);

        let mut landmarks = Vec::new();
        let stride = n / num_landmarks;
        for i in 0..num_landmarks {
            landmarks.push(i * stride);
        }

        let mut landmark_distances = vec![vec![f32::INFINITY; n]; num_landmarks];
        for (k_idx, &landmark) in landmarks.iter().enumerate() {
            let mut dist = vec![f32::INFINITY; n];
            dist[landmark] = 0.0;

            let mut queue = std::collections::VecDeque::new();
            queue.push_back(landmark);

            while let Some(u) = queue.pop_front() {
                for v in 0..n {
                    if distance_matrix[(u, v)] < f32::INFINITY && distance_matrix[(u, v)] > 0.0 {
                        let new_dist = dist[u] + distance_matrix[(u, v)];
                        if new_dist < dist[v] {
                            dist[v] = new_dist;
                            queue.push_back(v);
                        }
                    }
                }
            }

            landmark_distances[k_idx] = dist;
        }

        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let mut min_dist = f32::INFINITY;
                    for k_idx in 0..num_landmarks {
                        let dist_ki = landmark_distances[k_idx][i];
                        let dist_kj = landmark_distances[k_idx][j];
                        if dist_ki < f32::INFINITY && dist_kj < f32::INFINITY {
                            min_dist = min_dist.min(dist_ki + dist_kj);
                        }
                    }

                    if min_dist < distance_matrix[(i, j)] {
                        distance_matrix[(i, j)] = min_dist;
                    }
                }
            }
        }

        for i in 0..n {
            for j in 0..n {
                if distance_matrix[(i, j)].is_infinite() {
                    distance_matrix[(i, j)] = (n as f32) * 2.0;
                }
            }
        }

        self.cached_distance_matrix = Some(distance_matrix);
        trace!("Computed distance matrix for {} nodes", n);
        Ok(())
    }

    fn compute_weight_matrix(
        &mut self,
        graph_data: &GraphData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let distance_matrix = self
            .cached_distance_matrix
            .as_ref()
            .ok_or("Distance matrix must be computed first")?;

        let n = graph_data.nodes.len();
        let mut weight_matrix = DMatrix::zeros(n, n);

        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let distance = distance_matrix[(i, j)];
                    if distance > 0.0 {
                        weight_matrix[(i, j)] = 1.0 / (distance * distance);
                    }
                }
            }
        }

        self.cached_weight_matrix = Some(weight_matrix);
        trace!("Computed weight matrix for {} nodes", n);
        Ok(())
    }

    fn initialize_positions(
        &self,
        graph_data: &mut GraphData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = rand::thread_rng();

        for node in &mut graph_data.nodes {
            if node.data.x.abs() < f32::EPSILON
                && node.data.y.abs() < f32::EPSILON
                && node.data.z.abs() < f32::EPSILON
            {
                use rand::Rng;
                let theta = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let phi = rng.gen_range(0.0..std::f32::consts::PI);
                let radius = rng.gen_range(50.0..200.0);

                node.data.x = radius * phi.sin() * theta.cos();
                node.data.y = radius * phi.sin() * theta.sin();
                node.data.z = radius * phi.cos();
            }
        }

        trace!("Initialized positions for {} nodes", graph_data.nodes.len());
        Ok(())
    }

    fn extract_positions(&self, graph_data: &GraphData) -> DMatrix<f32> {
        let n = graph_data.nodes.len();
        let mut positions = DMatrix::zeros(n, 3);

        for (i, node) in graph_data.nodes.iter().enumerate() {
            positions[(i, 0)] = node.data.x;
            positions[(i, 1)] = node.data.y;
            positions[(i, 2)] = node.data.z;
        }

        positions
    }

    fn apply_positions(
        &self,
        graph_data: &mut GraphData,
        positions: &DMatrix<f32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if positions.nrows() != graph_data.nodes.len() || positions.ncols() != 3 {
            return Err("Position matrix dimensions don't match graph data".into());
        }

        for (i, node) in graph_data.nodes.iter_mut().enumerate() {
            node.data.x = positions[(i, 0)];
            node.data.y = positions[(i, 1)];
            node.data.z = positions[(i, 2)];
        }

        Ok(())
    }

    fn compute_stress(
        &self,
        positions: &DMatrix<f32>,
        graph_data: &GraphData,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        let distance_matrix = self
            .cached_distance_matrix
            .as_ref()
            .ok_or("Distance matrix not computed")?;
        let weight_matrix = self
            .cached_weight_matrix
            .as_ref()
            .ok_or("Weight matrix not computed")?;

        let n = graph_data.nodes.len();
        let mut stress = 0.0f32;

        // Use SIMD-accelerated batch computation for the inner loop.
        // For each row i, collect all pairs (i, j) where j > i into SoA buffers,
        // compute distances via SIMD, then compute weighted stress via SIMD.
        let max_batch = n; // maximum inner loop length
        let mut pos_x_i = vec![0.0f32; max_batch];
        let mut pos_y_i = vec![0.0f32; max_batch];
        let mut pos_z_i = vec![0.0f32; max_batch];
        let mut pos_x_j = vec![0.0f32; max_batch];
        let mut pos_y_j = vec![0.0f32; max_batch];
        let mut pos_z_j = vec![0.0f32; max_batch];
        let mut actual_dists = vec![0.0f32; max_batch];
        let mut ideal_dists = vec![0.0f32; max_batch];
        let mut weights = vec![0.0f32; max_batch];

        for i in 0..n {
            let batch_len = n - i - 1;
            if batch_len == 0 {
                continue;
            }

            // Fill SoA buffers for this row
            let xi = positions[(i, 0)];
            let yi = positions[(i, 1)];
            let zi = positions[(i, 2)];
            for (k, j) in (i + 1..n).enumerate() {
                pos_x_i[k] = xi;
                pos_y_i[k] = yi;
                pos_z_i[k] = zi;
                pos_x_j[k] = positions[(j, 0)];
                pos_y_j[k] = positions[(j, 1)];
                pos_z_j[k] = positions[(j, 2)];
                ideal_dists[k] = distance_matrix[(i, j)];
                weights[k] = weight_matrix[(i, j)];
            }

            // SIMD batch distance computation
            crate::physics::simd_forces::compute_distances_simd(
                &pos_x_i[..batch_len],
                &pos_y_i[..batch_len],
                &pos_z_i[..batch_len],
                &pos_x_j[..batch_len],
                &pos_y_j[..batch_len],
                &pos_z_j[..batch_len],
                &mut actual_dists[..batch_len],
            );

            // SIMD batch stress accumulation
            stress += crate::physics::simd_forces::compute_stress_batch_simd(
                &ideal_dists[..batch_len],
                &actual_dists[..batch_len],
                &weights[..batch_len],
            );
        }

        Ok(stress)
    }

    fn compute_gradient(
        &self,
        positions: &DMatrix<f32>,
        graph_data: &GraphData,
        constraints: &ConstraintSet,
    ) -> Result<DMatrix<f32>, Box<dyn std::error::Error>> {
        let distance_matrix = self
            .cached_distance_matrix
            .as_ref()
            .ok_or("Distance matrix not computed")?;
        let weight_matrix = self
            .cached_weight_matrix
            .as_ref()
            .ok_or("Weight matrix not computed")?;

        let n = graph_data.nodes.len();
        let mut gradient = DMatrix::zeros(n, 3);

        // SIMD-accelerated gradient computation.
        // For each row i, batch-compute distances to all other nodes j,
        // then compute factors and accumulate gradient contributions.
        let mut pos_x_i = vec![0.0f32; n];
        let mut pos_y_i = vec![0.0f32; n];
        let mut pos_z_i = vec![0.0f32; n];
        let mut pos_x_j = vec![0.0f32; n];
        let mut pos_y_j = vec![0.0f32; n];
        let mut pos_z_j = vec![0.0f32; n];
        let mut batch_dists = vec![0.0f32; n];
        let mut delta_x_buf = vec![0.0f32; n];
        let mut delta_y_buf = vec![0.0f32; n];
        let mut delta_z_buf = vec![0.0f32; n];
        let mut factors_buf = vec![0.0f32; n];

        for i in 0..n {
            // Build SoA for all j != i (pack contiguously, skip i)
            let xi = positions[(i, 0)];
            let yi = positions[(i, 1)];
            let zi = positions[(i, 2)];
            let mut k = 0;
            for j in 0..n {
                if j == i {
                    continue;
                }
                pos_x_i[k] = xi;
                pos_y_i[k] = yi;
                pos_z_i[k] = zi;
                pos_x_j[k] = positions[(j, 0)];
                pos_y_j[k] = positions[(j, 1)];
                pos_z_j[k] = positions[(j, 2)];
                k += 1;
            }
            let batch_len = k; // == n - 1

            // SIMD batch distance
            crate::physics::simd_forces::compute_distances_simd(
                &pos_x_i[..batch_len],
                &pos_y_i[..batch_len],
                &pos_z_i[..batch_len],
                &pos_x_j[..batch_len],
                &pos_y_j[..batch_len],
                &pos_z_j[..batch_len],
                &mut batch_dists[..batch_len],
            );

            // Compute per-pair factors and delta vectors, then accumulate gradient
            // factor = 2 * w * (1 - ideal/actual) when actual > epsilon, else 0
            let mut grad_x = 0.0f32;
            let mut grad_y = 0.0f32;
            let mut grad_z = 0.0f32;

            k = 0;
            for j in 0..n {
                if j == i {
                    continue;
                }
                let current_distance = batch_dists[k];
                if current_distance > f32::EPSILON {
                    let ideal = distance_matrix[(i, j)];
                    let weight = weight_matrix[(i, j)];
                    let factor = 2.0 * weight * (1.0 - ideal / current_distance);

                    delta_x_buf[k] = xi - positions[(j, 0)];
                    delta_y_buf[k] = yi - positions[(j, 1)];
                    delta_z_buf[k] = zi - positions[(j, 2)];
                    factors_buf[k] = factor;
                } else {
                    delta_x_buf[k] = 0.0;
                    delta_y_buf[k] = 0.0;
                    delta_z_buf[k] = 0.0;
                    factors_buf[k] = 0.0;
                }
                k += 1;
            }

            // SIMD dot-product-style accumulation: grad += sum(delta * factor)
            // Use dot_product_simd for each dimension (factor acts as the "b" vector
            // when we treat delta*factor as an element-wise product to sum)
            grad_x += crate::physics::simd_forces::dot_product_simd(
                &delta_x_buf[..batch_len],
                &factors_buf[..batch_len],
            );
            grad_y += crate::physics::simd_forces::dot_product_simd(
                &delta_y_buf[..batch_len],
                &factors_buf[..batch_len],
            );
            grad_z += crate::physics::simd_forces::dot_product_simd(
                &delta_z_buf[..batch_len],
                &factors_buf[..batch_len],
            );

            gradient[(i, 0)] += grad_x;
            gradient[(i, 1)] += grad_y;
            gradient[(i, 2)] += grad_z;
        }

        for constraint in constraints.active_constraints() {
            self.add_constraint_gradient(&mut gradient, positions, constraint)?;
        }

        Ok(gradient)
    }

    fn add_constraint_gradient(
        &self,
        gradient: &mut DMatrix<f32>,
        positions: &DMatrix<f32>,
        constraint: &Constraint,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match constraint.kind {
            ConstraintKind::FixedPosition => {
                if let Some(&node_idx) = constraint.node_indices.first() {
                    if constraint.params.len() >= 3 && node_idx < positions.nrows() as u32 {
                        let node_idx = node_idx as usize;
                        let weight = constraint.weight * self.config.constraint_weight;

                        gradient[(node_idx, 0)] +=
                            weight * 2.0 * (positions[(node_idx, 0)] - constraint.params[0]);
                        gradient[(node_idx, 1)] +=
                            weight * 2.0 * (positions[(node_idx, 1)] - constraint.params[1]);
                        gradient[(node_idx, 2)] +=
                            weight * 2.0 * (positions[(node_idx, 2)] - constraint.params[2]);
                    }
                }
            }

            ConstraintKind::Separation => {
                if constraint.node_indices.len() >= 2 && !constraint.params.is_empty() {
                    let i = constraint.node_indices[0] as usize;
                    let j = constraint.node_indices[1] as usize;
                    let min_distance = constraint.params[0];

                    if i < positions.nrows() && j < positions.nrows() {
                        let current_distance = self.euclidean_distance(positions, i, j);

                        if current_distance < min_distance && current_distance > f32::EPSILON {
                            let weight = constraint.weight * self.config.constraint_weight;
                            let factor =
                                weight * (min_distance - current_distance) / current_distance;

                            for dim in 0..3 {
                                let diff = positions[(i, dim)] - positions[(j, dim)];
                                gradient[(i, dim)] -= factor * diff;
                                gradient[(j, dim)] += factor * diff;
                            }
                        }
                    }
                }
            }

            ConstraintKind::AlignmentHorizontal => {
                if !constraint.params.is_empty() {
                    let target_y = constraint.params[0];
                    let weight = constraint.weight * self.config.constraint_weight;

                    for &node_idx in &constraint.node_indices {
                        if node_idx < positions.nrows() as u32 {
                            let node_idx = node_idx as usize;
                            gradient[(node_idx, 1)] +=
                                weight * 2.0 * (positions[(node_idx, 1)] - target_y);
                        }
                    }
                }
            }

            ConstraintKind::Clustering => {
                if constraint.params.len() >= 2 {
                    let strength = constraint.params[1];
                    let weight = constraint.weight * self.config.constraint_weight * strength;

                    let mut centroid = [0.0f32; 3];
                    let mut valid_nodes = 0;

                    for &node_idx in &constraint.node_indices {
                        if node_idx < positions.nrows() as u32 {
                            let node_idx = node_idx as usize;
                            for dim in 0..3 {
                                centroid[dim] += positions[(node_idx, dim)];
                            }
                            valid_nodes += 1;
                        }
                    }

                    if valid_nodes > 0 {
                        for dim in 0..3 {
                            centroid[dim] /= valid_nodes as f32;
                        }

                        for &node_idx in &constraint.node_indices {
                            if node_idx < positions.nrows() as u32 {
                                let node_idx = node_idx as usize;
                                for dim in 0..3 {
                                    gradient[(node_idx, dim)] +=
                                        weight * (centroid[dim] - positions[(node_idx, dim)]);
                                }
                            }
                        }
                    }
                }
            }

            _ => {
                debug!(
                    "Constraint type {:?} not yet implemented in gradient computation",
                    constraint.kind
                );
            }
        }

        Ok(())
    }

    fn update_positions(
        &self,
        positions: &DMatrix<f32>,
        gradient: &DMatrix<f32>,
    ) -> Result<DMatrix<f32>, Box<dyn std::error::Error>> {
        let mut new_positions = positions.clone();
        let step_size = self.config.step_size;

        for i in 0..positions.nrows() {
            for j in 0..positions.ncols() {
                new_positions[(i, j)] -= step_size * gradient[(i, j)];
            }
        }

        Ok(new_positions)
    }

    fn euclidean_distance(&self, positions: &DMatrix<f32>, i: usize, j: usize) -> f32 {
        let mut sum = 0.0;
        for dim in 0..3 {
            let diff = positions[(i, dim)] - positions[(j, dim)];
            sum += diff * diff;
        }
        sum.sqrt()
    }

    fn compute_constraint_scores(
        &self,
        graph_data: &GraphData,
        constraints: &ConstraintSet,
    ) -> Result<HashMap<ConstraintKind, f32>, Box<dyn std::error::Error>> {
        let mut scores = HashMap::new();
        let positions = self.extract_positions(graph_data);

        for constraint in constraints.active_constraints() {
            let score = match constraint.kind {
                ConstraintKind::FixedPosition => {
                    self.score_fixed_position(&positions, constraint)?
                }
                ConstraintKind::Separation => self.score_separation(&positions, constraint)?,
                ConstraintKind::AlignmentHorizontal => {
                    self.score_alignment_horizontal(&positions, constraint)?
                }
                ConstraintKind::Clustering => self.score_clustering(&positions, constraint)?,
                _ => 0.5,
            };

            scores
                .entry(constraint.kind)
                .and_modify(|e| *e = (*e + score) / 2.0)
                .or_insert(score);
        }

        Ok(scores)
    }

    fn score_fixed_position(
        &self,
        positions: &DMatrix<f32>,
        constraint: &Constraint,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        if let Some(&node_idx) = constraint.node_indices.first() {
            if constraint.params.len() >= 3 && node_idx < positions.nrows() as u32 {
                let node_idx = node_idx as usize;
                let distance = ((positions[(node_idx, 0)] - constraint.params[0]).powi(2)
                    + (positions[(node_idx, 1)] - constraint.params[1]).powi(2)
                    + (positions[(node_idx, 2)] - constraint.params[2]).powi(2))
                .sqrt();

                return Ok((1.0 / (1.0 + distance / 10.0)).max(0.0).min(1.0));
            }
        }
        Ok(0.0)
    }

    fn score_separation(
        &self,
        positions: &DMatrix<f32>,
        constraint: &Constraint,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        if constraint.node_indices.len() >= 2 && !constraint.params.is_empty() {
            let i = constraint.node_indices[0] as usize;
            let j = constraint.node_indices[1] as usize;
            let min_distance = constraint.params[0];

            if i < positions.nrows() && j < positions.nrows() {
                let current_distance = self.euclidean_distance(positions, i, j);
                return Ok(if current_distance >= min_distance {
                    1.0
                } else {
                    current_distance / min_distance
                });
            }
        }
        Ok(0.0)
    }

    fn score_alignment_horizontal(
        &self,
        positions: &DMatrix<f32>,
        constraint: &Constraint,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        if !constraint.params.is_empty() {
            let target_y = constraint.params[0];
            let mut total_deviation = 0.0;
            let mut count = 0;

            for &node_idx in &constraint.node_indices {
                if node_idx < positions.nrows() as u32 {
                    let node_idx = node_idx as usize;
                    total_deviation += (positions[(node_idx, 1)] - target_y).abs();
                    count += 1;
                }
            }

            if count > 0 {
                let avg_deviation = total_deviation / count as f32;
                return Ok((1.0 / (1.0 + avg_deviation / 10.0)).max(0.0).min(1.0));
            }
        }
        Ok(0.0)
    }

    fn score_clustering(
        &self,
        positions: &DMatrix<f32>,
        constraint: &Constraint,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        if constraint.node_indices.len() > 1 {
            let mut total_distance = 0.0;
            let mut count = 0;

            for i in 0..constraint.node_indices.len() {
                for j in i + 1..constraint.node_indices.len() {
                    let node_i = constraint.node_indices[i] as usize;
                    let node_j = constraint.node_indices[j] as usize;

                    if node_i < positions.nrows() && node_j < positions.nrows() {
                        total_distance += self.euclidean_distance(positions, node_i, node_j);
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let avg_distance = total_distance / count as f32;

                return Ok((1.0 / (1.0 + avg_distance / 50.0)).max(0.0).min(1.0));
            }
        }
        Ok(0.0)
    }

    pub fn get_iteration_history(&self) -> &[f32] {
        &self.iteration_history
    }

    pub fn clear_cache(&mut self) {
        self.cached_distance_matrix = None;
        self.cached_weight_matrix = None;
        self.iteration_history.clear();
        trace!("Cleared stress majorization cache");
    }

    pub fn update_config(&mut self, config: StressMajorizationConfig) {
        self.config = config;
        info!("Updated stress majorization configuration");
    }
}

impl Default for StressMajorizationSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{edge::Edge, graph::GraphData, node::Node};
    use crate::utils::socket_flow_messages::BinaryNodeData;

    fn create_test_graph() -> GraphData {
        let mut graph = GraphData {
            nodes: vec![
                Node::new_with_id("test1".to_string(), Some(1)),
                Node::new_with_id("test2".to_string(), Some(2)),
                Node::new_with_id("test3".to_string(), Some(3)),
            ],
            edges: vec![Edge::new(1, 2, 1.0), Edge::new(2, 3, 1.0)],
            metadata: crate::models::metadata::MetadataStore::new(),
            id_to_metadata: std::collections::HashMap::new(),
        };

        graph.nodes[0].data.x = 0.0;
        graph.nodes[0].data.y = 0.0;
        graph.nodes[0].data.z = 0.0;

        graph.nodes[1].data.x = 100.0;
        graph.nodes[1].data.y = 0.0;
        graph.nodes[1].data.z = 0.0;

        graph.nodes[2].data.x = 50.0;
        graph.nodes[2].data.y = 100.0;
        graph.nodes[2].data.z = 0.0;

        graph
    }

    #[test]
    fn test_solver_creation() {
        let solver = StressMajorizationSolver::new();
        assert_eq!(solver.config.max_iterations, 1000);
        assert!(solver.config.tolerance > 0.0);
    }

    #[test]
    fn test_distance_matrix_computation() {
        let mut solver = StressMajorizationSolver::new();
        let graph = create_test_graph();

        solver.compute_distance_matrix(&graph).unwrap();
        assert!(solver.cached_distance_matrix.is_some());

        let distance_matrix = solver
            .cached_distance_matrix
            .as_ref()
            .expect("Expected value to be present");
        assert_eq!(distance_matrix.nrows(), 3);
        assert_eq!(distance_matrix.ncols(), 3);

        for i in 0..3 {
            assert_eq!(distance_matrix[(i, i)], 0.0);
        }
    }

    #[test]
    fn test_position_extraction_and_application() {
        let solver = StressMajorizationSolver::new();
        let mut graph = create_test_graph();

        let positions = solver.extract_positions(&graph);
        assert_eq!(positions.nrows(), 3);
        assert_eq!(positions.ncols(), 3);
        assert_eq!(positions[(0, 0)], 0.0);
        assert_eq!(positions[(1, 0)], 100.0);

        let mut new_positions = positions.clone();
        new_positions[(0, 0)] = 50.0;

        solver.apply_positions(&mut graph, &new_positions).unwrap();
        assert_eq!(graph.nodes[0].data.x, 50.0);
    }

    #[test]
    fn test_constraint_score_computation() {
        let solver = StressMajorizationSolver::new();
        let graph = create_test_graph();
        let mut constraint_set = ConstraintSet::default();

        constraint_set.add(Constraint::separation(1, 2, 50.0));

        let scores = solver
            .compute_constraint_scores(&graph, &constraint_set)
            .unwrap();
        assert!(scores.contains_key(&ConstraintKind::Separation));

        let sep_score = scores[&ConstraintKind::Separation];
        assert!(sep_score >= 0.0 && sep_score <= 1.0);
    }
}
