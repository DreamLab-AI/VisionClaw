//! Ontology Constraint Actor - GPU-accelerated ontology constraint evaluation
//!
//! This actor handles ontology-derived physics constraints on the GPU, translating
//! OWL axioms and ontology rules into physics forces that guide graph layout.
//!
//! ## Architecture
//!
//! Follows the established GPU actor pattern:
//! - SharedGPUContext for unified GPU access
//! - Graceful CPU fallback on GPU errors
//! - Memory pooling for constraint buffers
//! - Integration with ontology validation system

use actix::prelude::*;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;
use crate::models::constraints::{ConstraintData, ConstraintSetGpuExt};
use crate::physics::ontology_constraint_mapper::map_axioms_to_constraints;
use crate::physics::ontology_constraints::{OWLAxiom, OntologyConstraintTranslator};
use visionclaw_domain::models::constraints::{Constraint, ConstraintSet};
use visionclaw_domain::ports::owl_types::OwlAxiom as DomainOwlAxiom;
use visionclaw_ontology::services::iri_node_resolver::IriNodeResolver;

/// Default global force strength applied to the mapper output at dispatch.
/// Full mapper weight (1.0) collapses the 18k+ mostly-attractive constraints
/// into a single dense blob; 0.6 keeps the semantic structure visible while the
/// base repulsion still spreads the graph. The global-strength slider rescales
/// from the unscaled base, so the user can return to 1.0 or push lower live.
pub const DEFAULT_GLOBAL_STRENGTH: f32 = 0.6;

/// ADR-070 D2.3 — input-edge NaN guard.
///
/// Drop any constraint that references a node whose current position is
/// non-finite (NaN / ±Inf), so a bad upstream position can never enter the GPU
/// force kernel where it would NaN-cascade into every client (R-19 / F-09).
/// Returns the surviving constraints and the number rejected. The decision
/// logic itself lives in `visionclaw_gpu::hardening::partition_finite_constraints`
/// (pure + unit-tested); this adapter just builds the id→position lookup from
/// the live graph and maps `ConstraintData` onto the guard's input view.
fn reject_nonfinite_constraints(
    buffer: &[ConstraintData],
    graph_data: &visionclaw_domain::models::graph::GraphData,
) -> (Vec<ConstraintData>, usize) {
    use std::collections::HashMap;
    use visionclaw_gpu::hardening::{partition_finite_constraints, ConstraintForceInput};

    let mut pos_by_id: HashMap<i32, [f32; 3]> = HashMap::with_capacity(graph_data.nodes.len());
    for node in &graph_data.nodes {
        pos_by_id.insert(node.id as i32, [node.x(), node.y(), node.z()]);
    }

    let inputs: Vec<ConstraintForceInput> = buffer
        .iter()
        .map(|c| ConstraintForceInput {
            kind: c.kind,
            count: c.count,
            node_idx: c.node_idx,
            params: c.params,
            weight: c.weight,
        })
        .collect();

    let (kept, rejected) = partition_finite_constraints(&inputs, &pos_by_id);
    let survivors: Vec<ConstraintData> = kept.iter().map(|&i| buffer[i]).collect();
    (survivors, rejected.len())
}

/// Scale a copy of the mapper's unscaled constraint buffer by `strength`.
/// Weight is the only field the global strength touches; node indices, kind,
/// params (rest-length / min-distance) and activation frame are preserved.
fn scale_constraint_buffer(base: &[ConstraintData], strength: f32) -> Vec<ConstraintData> {
    let s = strength.clamp(0.0, 1.0);
    base.iter()
        .map(|c| {
            let mut scaled = *c;
            scaled.weight = c.weight * s;
            scaled
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraintStats {
    pub total_axioms_processed: u32,
    pub active_ontology_constraints: u32,
    pub constraint_evaluation_count: u32,
    pub last_update_time_ms: f32,
    pub gpu_failure_count: u32,
    pub cpu_fallback_count: u32,
    pub constraint_cache_hits: u32,
    pub constraint_cache_misses: u32,
    /// ADR-070 D2.3: cumulative count of constraints dropped by the input-edge
    /// NaN guard because they referenced a node with a non-finite position.
    pub rejected_nonfinite_constraints: u32,
}

impl Default for OntologyConstraintStats {
    fn default() -> Self {
        Self {
            total_axioms_processed: 0,
            active_ontology_constraints: 0,
            constraint_evaluation_count: 0,
            last_update_time_ms: 0.0,
            gpu_failure_count: 0,
            cpu_fallback_count: 0,
            constraint_cache_hits: 0,
            constraint_cache_misses: 0,
            rejected_nonfinite_constraints: 0,
        }
    }
}

pub struct OntologyConstraintActor {
    shared_context: Option<Arc<SharedGPUContext>>,

    translator: OntologyConstraintTranslator,

    ontology_constraints: Vec<Constraint>,

    constraint_buffer: Vec<ConstraintData>,

    /// The unscaled mapper output (mapper weights at full strength). The live
    /// `constraint_buffer` is this scaled by `global_strength`; keeping the base
    /// lets the global-strength slider scale up AND down reversibly instead of
    /// permanently multiplying (which made repeated adjustments collapse to 0).
    constraint_buffer_base: Vec<ConstraintData>,

    /// Current global force strength (0..1) applied to `constraint_buffer_base`
    /// to produce the live buffer. Default keeps the semantic layout legible
    /// rather than collapsed into one dense blob at full mapper weight.
    global_strength: f32,

    gpu_state: GPUState,

    stats: OntologyConstraintStats,

    last_update: Instant,

    gpu_initialized: bool,

    /// Address of ForceComputeActor for sending constraint buffer updates
    force_compute_addr: Option<actix::Addr<super::force_compute_actor::ForceComputeActor>>,
}

impl OntologyConstraintActor {
    pub fn new() -> Self {
        info!("Creating new OntologyConstraintActor");

        Self {
            shared_context: None,
            translator: OntologyConstraintTranslator::new(),
            ontology_constraints: Vec::new(),
            constraint_buffer: Vec::new(),
            constraint_buffer_base: Vec::new(),
            global_strength: DEFAULT_GLOBAL_STRENGTH,
            gpu_state: GPUState::default(),
            stats: OntologyConstraintStats::default(),
            last_update: Instant::now(),
            gpu_initialized: false,
            force_compute_addr: None,
        }
    }

    fn initialize_gpu(&mut self) -> Result<(), String> {
        if self.shared_context.is_none() {
            return Err("GPU context not available".to_string());
        }

        info!("OntologyConstraintActor: GPU initialization - context available");
        self.gpu_initialized = true;
        Ok(())
    }

    /// Canonical OWL-axiom → live-kernel constraint ingestion (ADR-098 D1).
    ///
    /// Resolves both endpoints of every materialised axiom through the shared
    /// `IriNodeResolver` (ADR-100), maps them to the live `ConstraintData`
    /// buffer via the pure anti-corruption mapper (live kind integers
    /// 0=DISTANCE / 6=SEPARATION, never `to_gpu_data()`), uploads losslessly via
    /// `set_constraints`, and notifies ForceComputeActor so the live
    /// `force_pass_kernel` consumes it once ENABLE_CONSTRAINTS is set. Buffered +
    /// retried when the GPU context is not yet attached. Returns the number of
    /// constraints produced. The post-sync `ApplyMaterializedAxioms` entry
    /// calls it, so the mapper is the single source of truth.
    fn ingest_domain_axioms(
        &mut self,
        domain_axioms: Vec<DomainOwlAxiom>,
        graph_data: &visionclaw_domain::models::graph::GraphData,
    ) -> Result<usize, String> {
        let start_time = Instant::now();

        let mut resolver = IriNodeResolver::from_nodes(&graph_data.nodes);
        // Keep the unscaled mapper output as the base so the global-strength
        // slider can rescale reversibly; the live buffer is base × strength.
        let mapped = map_axioms_to_constraints(&domain_axioms, &mut resolver);

        // ADR-070 D2.3 — input-edge NaN guard. Reject any mapped constraint that
        // references a node with a non-finite position before it can reach the
        // GPU kernel and NaN-cascade to clients (R-19 / F-09). Applied to the
        // unscaled base so the drop persists across every subsequent
        // global-strength rescale.
        let (clean, rejected) = reject_nonfinite_constraints(&mapped, graph_data);
        if rejected > 0 {
            warn!(
                "OntologyConstraintActor: NaN guard dropped {} constraint(s) referencing non-finite node positions ({} of {} survived)",
                rejected,
                clean.len(),
                mapped.len()
            );
            self.stats.rejected_nonfinite_constraints = self
                .stats
                .rejected_nonfinite_constraints
                .saturating_add(rejected as u32);
        }
        self.constraint_buffer_base = clean;
        self.constraint_buffer =
            scale_constraint_buffer(&self.constraint_buffer_base, self.global_strength);
        self.stats.total_axioms_processed += domain_axioms.len() as u32;
        let produced = self.constraint_buffer.len();
        // The materialised-axiom path uploads the full buffer to the GPU live
        // kernel, so every produced constraint is active. Without this the stats
        // endpoint reported `activeConstraints: 0` despite 18k+ uploaded.
        self.stats.active_ontology_constraints = produced as u32;

        info!(
            "OntologyConstraintActor: Mapped {} OWL axioms → {} live-kernel constraints ({} endpoint lookups unresolved)",
            domain_axioms.len(),
            produced,
            resolver.unresolved_count()
        );

        if self.gpu_initialized && self.shared_context.is_some() {
            match self.upload_constraints_to_gpu() {
                Ok(_) => {
                    info!(
                        "OntologyConstraintActor: Uploaded {} live-kernel constraints to GPU",
                        produced
                    );
                }
                Err(e) => {
                    warn!(
                        "OntologyConstraintActor: GPU upload failed, deferring to next physics step: {}",
                        e
                    );
                    self.stats.gpu_failure_count += 1;
                    self.stats.cpu_fallback_count += 1;
                }
            }
        } else {
            debug!(
                "OntologyConstraintActor: GPU not available, constraints cached for next physics step"
            );
            self.stats.cpu_fallback_count += 1;
        }

        self.last_update = Instant::now();
        self.stats.last_update_time_ms = start_time.elapsed().as_secs_f32() * 1000.0;
        self.stats.constraint_evaluation_count += 1;

        self.notify_force_compute_actor();

        Ok(produced)
    }

    /// Send the updated constraint buffer to ForceComputeActor
    fn notify_force_compute_actor(&self) {
        if let Some(ref addr) = self.force_compute_addr {
            info!(
                "OntologyConstraintActor: Sending {} constraints to ForceComputeActor",
                self.constraint_buffer.len()
            );
            addr.do_send(UpdateOntologyConstraintBuffer {
                constraint_buffer: self.constraint_buffer.clone(),
            });
        } else {
            debug!(
                "OntologyConstraintActor: ForceComputeActor address not set, skipping notification"
            );
        }
    }

    fn update_constraints(&mut self, axioms: &[OWLAxiom]) -> Result<(), String> {
        info!(
            "OntologyConstraintActor: Updating constraints with {} new axioms",
            axioms.len()
        );

        warn!("OntologyConstraintActor: Dynamic constraint updates require graph context");
        warn!("Consider using ApplyOntologyConstraints message with full context");

        self.stats.total_axioms_processed += axioms.len() as u32;

        Ok(())
    }

    fn upload_constraints_to_gpu(&self) -> Result<(), String> {
        let shared_context = self
            .shared_context
            .as_ref()
            .ok_or("GPU context not available")?;

        let mut unified_compute = shared_context
            .unified_compute
            .lock()
            .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?;

        if self.constraint_buffer.is_empty() {
            debug!("OntologyConstraintActor: No constraints to upload, clearing GPU constraints");
            unified_compute
                .clear_constraints()
                .map_err(|e| format!("Failed to clear GPU constraints: {}", e))?;
        } else {
            // ADR-098 break #4: LOSSLESS writer — preserves all 4 node_idx + 8
            // params and stamps activation_frame for the progressive ramp.
            unified_compute
                .set_constraints(self.constraint_buffer.clone())
                .map_err(|e| format!("Failed to upload constraints to GPU: {}", e))?;
        }

        Ok(())
    }

    fn get_ontology_stats(&self) -> OntologyConstraintStats {
        self.stats.clone()
    }

    fn cleanup(&mut self) -> Result<(), String> {
        info!("OntologyConstraintActor: Cleaning up resources");

        self.ontology_constraints.clear();
        self.constraint_buffer.clear();

        if let Some(ref shared_context) = self.shared_context {
            if let Ok(mut unified_compute) = shared_context.unified_compute.lock() {
                if let Err(e) = unified_compute.clear_constraints() {
                    warn!("OntologyConstraintActor: Failed to clear GPU constraints during cleanup: {}", e);
                }
            }
        }

        self.translator.clear_cache();

        info!("OntologyConstraintActor: Cleanup completed");
        Ok(())
    }
}

impl Default for OntologyConstraintActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for OntologyConstraintActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Ontology Constraint Actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Ontology Constraint Actor stopped");
        let _ = self.cleanup();
    }
}

// === Message Handlers ===

impl Handler<ApplyOntologyConstraints> for OntologyConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ApplyOntologyConstraints, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "OntologyConstraintActor: Received ApplyOntologyConstraints message for graph_id {}",
            msg.graph_id
        );

        let constraint_count = msg.constraint_set.constraints.len();
        match msg.merge_mode {
            ConstraintMergeMode::Replace => {
                self.ontology_constraints = msg.constraint_set.constraints.clone();
                info!(
                    "OntologyConstraintActor: Replaced all constraints with {} new constraints",
                    self.ontology_constraints.len()
                );
            }
            ConstraintMergeMode::Merge => {
                let existing_count = self.ontology_constraints.len();
                self.ontology_constraints
                    .extend(msg.constraint_set.constraints.clone());
                info!("OntologyConstraintActor: Merged {} new constraints with {} existing (total: {})",
                      constraint_count, existing_count, self.ontology_constraints.len());
            }
            ConstraintMergeMode::AddIfNoConflict => {
                let initial_count = self.ontology_constraints.len();
                for constraint in msg.constraint_set.constraints.clone() {
                    let has_conflict = self.ontology_constraints.iter().any(|existing| {
                        existing.node_indices == constraint.node_indices
                            && existing.kind == constraint.kind
                    });

                    if !has_conflict {
                        self.ontology_constraints.push(constraint);
                    }
                }
                let added_count = self.ontology_constraints.len() - initial_count;
                info!(
                    "OntologyConstraintActor: Added {} non-conflicting constraints (skipped {})",
                    added_count,
                    constraint_count - added_count
                );
            }
        }

        self.constraint_buffer = msg.constraint_set.to_gpu_data();

        self.stats.active_ontology_constraints = self
            .ontology_constraints
            .iter()
            .filter(|c| c.active)
            .count() as u32;

        if self.gpu_initialized && self.shared_context.is_some() {
            match self.upload_constraints_to_gpu() {
                Ok(_) => {
                    info!("OntologyConstraintActor: Uploaded {} constraints via ApplyOntologyConstraints",
                          self.constraint_buffer.len());
                }
                Err(e) => {
                    warn!("OntologyConstraintActor: GPU upload failed: {}", e);
                    self.stats.gpu_failure_count += 1;
                }
            }
        }

        Ok(())
    }
}

impl Handler<crate::actors::messages::ApplyMaterializedAxioms> for OntologyConstraintActor {
    type Result = Result<usize, String>;

    fn handle(
        &mut self,
        msg: crate::actors::messages::ApplyMaterializedAxioms,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!(
            "OntologyConstraintActor: Received ApplyMaterializedAxioms — {} materialised OWL axioms over {} graph nodes",
            msg.axioms.len(),
            msg.graph_data.nodes.len()
        );
        self.ingest_domain_axioms(msg.axioms, &msg.graph_data)
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateOntologyConstraints {
    pub axioms: Vec<OWLAxiom>,
}

impl Handler<UpdateOntologyConstraints> for OntologyConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateOntologyConstraints, _ctx: &mut Self::Context) -> Self::Result {
        self.update_constraints(&msg.axioms)
    }
}

#[derive(Message)]
#[rtype(result = "Result<OntologyConstraintStats, String>")]
pub struct GetOntologyStats;

impl Handler<GetOntologyStats> for OntologyConstraintActor {
    type Result = Result<OntologyConstraintStats, String>;

    fn handle(&mut self, _msg: GetOntologyStats, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.get_ontology_stats())
    }
}

impl Handler<GetOntologyConstraintStats> for OntologyConstraintActor {
    type Result = Result<crate::actors::messages::OntologyConstraintStats, String>;

    fn handle(
        &mut self,
        _msg: GetOntologyConstraintStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("OntologyConstraintActor: Received GetOntologyConstraintStats message");

        let stats = crate::actors::messages::OntologyConstraintStats {
            total_axioms_processed: self.stats.total_axioms_processed,
            active_ontology_constraints: self.stats.active_ontology_constraints,
            constraint_evaluation_count: self.stats.constraint_evaluation_count,
            last_update_time_ms: self.stats.last_update_time_ms,
            gpu_failure_count: self.stats.gpu_failure_count,
            cpu_fallback_count: self.stats.cpu_fallback_count,
        };

        Ok(stats)
    }
}

/// Handler for setting the ForceComputeActor address for bidirectional communication
impl Handler<SetForceComputeAddr> for OntologyConstraintActor {
    type Result = ();

    fn handle(&mut self, msg: SetForceComputeAddr, _ctx: &mut Self::Context) -> Self::Result {
        info!("OntologyConstraintActor: Received ForceComputeActor address for constraint synchronization");
        self.force_compute_addr = Some(msg.addr);

        // If we already have constraints buffered, send them immediately
        if !self.constraint_buffer.is_empty() {
            self.notify_force_compute_actor();
        }
    }
}

impl Handler<SetSharedGPUContext> for OntologyConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("OntologyConstraintActor: Received SharedGPUContext from ResourceActor");

        self.shared_context = Some(msg.context);

        match self.initialize_gpu() {
            Ok(_) => {
                info!("OntologyConstraintActor: GPU initialization successful");
                Ok(())
            }
            Err(e) => {
                warn!("OntologyConstraintActor: GPU initialization failed: {}", e);

                Ok(())
            }
        }
    }
}

impl Handler<GetConstraintStats> for OntologyConstraintActor {
    type Result = Result<ConstraintStats, String>;

    fn handle(&mut self, _msg: GetConstraintStats, _ctx: &mut Self::Context) -> Self::Result {
        let mut stats = ConstraintStats {
            total_constraints: self.ontology_constraints.len(),
            active_constraints: self.stats.active_ontology_constraints as usize,
            constraint_groups: std::collections::HashMap::new(),
            ontology_constraints: self.ontology_constraints.len(),
            user_constraints: 0,
        };

        stats.constraint_groups.insert(
            "ontology_derived".to_string(),
            self.ontology_constraints.len(),
        );

        Ok(stats)
    }
}

/// Handler for GetConstraintBuffer - provides GPU-ready constraint data
/// This is the key integration point for P0-2: it returns the constraint_buffer
/// that ForceComputeActor needs to upload to GPU via UnifiedGPUCompute::upload_constraints()
impl Handler<crate::actors::messages::GetConstraintBuffer> for OntologyConstraintActor {
    type Result = Result<Vec<ConstraintData>, String>;

    fn handle(
        &mut self,
        _msg: crate::actors::messages::GetConstraintBuffer,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(
            "OntologyConstraintActor: Providing constraint buffer for GPU upload ({} constraints)",
            self.constraint_buffer.len()
        );

        // Return a clone of the constraint buffer for GPU upload
        // This buffer contains ConstraintData structs ready for GPU processing
        Ok(self.constraint_buffer.clone())
    }
}

impl Handler<UpdateConstraints> for OntologyConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        info!("OntologyConstraintActor: Received UpdateConstraints message");

        let constraints =
            match serde_json::from_value::<Vec<Constraint>>(msg.constraint_data.clone()) {
                Ok(constraints) => constraints,
                Err(e) => match serde_json::from_value::<ConstraintSet>(msg.constraint_data) {
                    Ok(constraint_set) => constraint_set.constraints,
                    Err(_) => {
                        error!(
                            "OntologyConstraintActor: Failed to parse constraint_data: {}",
                            e
                        );
                        return Err(format!("Failed to parse constraints: {}", e));
                    }
                },
            };

        self.ontology_constraints = constraints;
        self.constraint_buffer = self
            .ontology_constraints
            .iter()
            .filter(|c| c.active)
            .map(ConstraintData::from_constraint)
            .collect();

        if self.gpu_initialized && self.shared_context.is_some() {
            self.upload_constraints_to_gpu()?;
        }

        Ok(())
    }
}

impl Handler<InitializeGPU> for OntologyConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeGPU, _ctx: &mut Self::Context) -> Self::Result {
        info!("OntologyConstraintActor: InitializeGPU received");

        self.gpu_state.num_nodes = msg.graph.nodes.len() as u32;
        self.gpu_state.num_edges = msg.graph.edges.len() as u32;

        info!(
            "OntologyConstraintActor: Graph dimensions stored - {} nodes, {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        Ok(())
    }
}

impl Handler<AdjustConstraintWeights> for OntologyConstraintActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(&mut self, msg: AdjustConstraintWeights, _ctx: &mut Self::Context) -> Self::Result {
        let global_strength = msg.global_strength.clamp(0.0, 1.0);
        info!(
            "OntologyConstraintActor: Adjusting constraint weights with global_strength={:.3}",
            global_strength
        );

        // The live constraints come from the canonical mapper path
        // (`ingest_domain_axioms`), which fills `constraint_buffer_base` — NOT
        // the legacy `ontology_constraints` Vec (empty on the mapper path).
        // Scale the live buffer from that unscaled base so adjustments are
        // reversible; the previous code multiplied `ontology_constraints`
        // (0 entries) and re-uploaded an empty buffer, wiping all 18k+
        // constraints off the GPU on the first slider move.
        self.global_strength = global_strength;
        self.constraint_buffer =
            scale_constraint_buffer(&self.constraint_buffer_base, global_strength);
        let adjusted_count = self.constraint_buffer.len() as u32;

        // Re-upload to GPU if initialized
        if self.gpu_initialized && self.shared_context.is_some() {
            if let Err(e) = self.upload_constraints_to_gpu() {
                warn!(
                    "OntologyConstraintActor: GPU re-upload after weight adjustment failed: {}",
                    e
                );
                self.stats.gpu_failure_count += 1;
            }
        }

        // Notify ForceComputeActor about the updated buffer
        self.notify_force_compute_actor();

        // Strength does not change how many constraints are active — every base
        // constraint stays in the buffer, only its weight scales.
        self.stats.active_ontology_constraints = self.constraint_buffer.len() as u32;

        Ok(serde_json::json!({
            "success": true,
            "appliedStrength": global_strength,
            "adjustedConstraints": adjusted_count,
            "activeConstraints": self.stats.active_ontology_constraints
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actor_creation() {
        let actor = OntologyConstraintActor::new();
        assert_eq!(actor.ontology_constraints.len(), 0);
        assert_eq!(actor.constraint_buffer.len(), 0);
        assert!(!actor.gpu_initialized);
    }

    #[test]
    fn test_stats_default() {
        let stats = OntologyConstraintStats::default();
        assert_eq!(stats.total_axioms_processed, 0);
        assert_eq!(stats.active_ontology_constraints, 0);
        assert_eq!(stats.gpu_failure_count, 0);
    }

    #[test]
    fn test_nan_guard_drops_constraints_on_nonfinite_nodes() {
        use visionclaw_domain::models::graph::GraphData;
        use visionclaw_domain::models::node::Node;

        // Three nodes; node id 2 has a NaN X position.
        let mut good_a = Node::new("a".to_string());
        good_a.id = 1;
        good_a.set_x(0.0);
        good_a.set_y(0.0);
        good_a.set_z(0.0);

        let mut bad = Node::new("b".to_string());
        bad.id = 2;
        bad.set_x(f32::NAN);
        bad.set_y(0.0);
        bad.set_z(0.0);

        let mut good_c = Node::new("c".to_string());
        good_c.id = 3;
        good_c.set_x(5.0);
        good_c.set_y(0.0);
        good_c.set_z(0.0);

        let mut graph = GraphData::new();
        graph.nodes = vec![good_a, bad, good_c];

        let buffer = vec![
            ConstraintData {
                kind: 0,
                count: 2,
                node_idx: [1, 3, 0, 0],
                params: [10.0; 8],
                weight: 1.0,
                activation_frame: 0,
            },
            ConstraintData {
                kind: 0,
                count: 2,
                node_idx: [1, 2, 0, 0], // references the NaN node → dropped
                params: [10.0; 8],
                weight: 1.0,
                activation_frame: 0,
            },
        ];

        let (survivors, rejected) = reject_nonfinite_constraints(&buffer, &graph);
        assert_eq!(rejected, 1);
        assert_eq!(survivors.len(), 1);
        assert_eq!(survivors[0].node_idx, [1, 3, 0, 0]);
    }

    #[test]
    fn test_constraint_buffer_conversion() {
        let mut actor = OntologyConstraintActor::new();

        let constraints = vec![
            Constraint::fixed_position(0, 10.0, 20.0, 30.0),
            Constraint::separation(1, 2, 50.0),
        ];

        actor.ontology_constraints = constraints;
        actor.constraint_buffer = actor
            .ontology_constraints
            .iter()
            .map(ConstraintData::from_constraint)
            .collect();

        assert_eq!(actor.constraint_buffer.len(), 2);
    }
}
