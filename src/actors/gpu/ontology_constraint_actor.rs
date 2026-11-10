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

/// Strength applied before the stored policy arrives: the stored default
/// (`physics.ontologyConstraintStrength`, ADR-2192), defined once in the domain crate.
pub const DEFAULT_GLOBAL_STRENGTH: f32 =
    visionclaw_domain::types::physics_config::ONTOLOGY_CONSTRAINT_STRENGTH_DEFAULT;

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
/// The clamp is the stored bound (`physics_bounds::ONTOLOGY_CONSTRAINT_STRENGTH`), which the
/// physics route already enforces; it only guards a boot row that was never validated, so a
/// non-finite or out-of-range weight can never reach the kernel.
fn scale_constraint_buffer(base: &[ConstraintData], strength: f32) -> Vec<ConstraintData> {
    let (lo, hi) = super::physics_bounds::ONTOLOGY_CONSTRAINT_STRENGTH;
    let s = if strength.is_finite() {
        strength.clamp(lo, hi)
    } else {
        lo
    };
    base.iter()
        .map(|c| {
            let mut scaled = *c;
            scaled.weight = c.weight * s;
            scaled
        })
        .collect()
}

/// The live buffer the GPU should hold under the stored policy (ADR-2192): nothing when the
/// constraints are switched off, otherwise the unscaled base at `strength`. The base is kept
/// either way, so switching back on re-applies the last sync's set without re-reasoning.
pub(crate) fn live_constraint_buffer(
    base: &[ConstraintData],
    enabled: bool,
    strength: f32,
) -> Vec<ConstraintData> {
    if enabled {
        scale_constraint_buffer(base, strength)
    } else {
        Vec::new()
    }
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
    /// to produce the live buffer: the stored `physics.ontologyConstraintStrength`.
    global_strength: f32,

    /// Whether the constraints act on the layout: the stored
    /// `physics.ontologyConstraintsEnabled` (ADR-2192). Off, the live buffer is empty.
    constraints_enabled: bool,

    /// The policy last handed to the device writer (ForceComputeActor), or `None` until one
    /// has been delivered. An identical policy is answered without re-uploading; anything
    /// else (including the first policy after a restart) is applied (ADR-2192).
    applied_policy: Option<SetOntologyConstraintPolicy>,

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
            constraints_enabled:
                visionclaw_domain::types::physics_config::ONTOLOGY_CONSTRAINTS_ENABLED_DEFAULT,
            applied_policy: None,
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
        self.constraint_buffer = live_constraint_buffer(
            &self.constraint_buffer_base,
            self.constraints_enabled,
            self.global_strength,
        );
        self.stats.total_axioms_processed += domain_axioms.len() as u32;
        let produced = self.constraint_buffer.len();
        // The materialised-axiom path uploads the live buffer to the GPU live
        // kernel, so every constraint in it is active (none while the stored
        // policy has them off). Without this the stats endpoint reported
        // `activeConstraints: 0` despite 18k+ uploaded.
        self.stats.active_ontology_constraints = produced as u32;

        info!(
            "OntologyConstraintActor: Mapped {} OWL axioms → {} live-kernel constraints ({} endpoint lookups unresolved)",
            domain_axioms.len(),
            produced,
            resolver.unresolved_count()
        );

        // The device write happens in ForceComputeActor (notified below), which
        // uploads this set together with the user and analytics constraints.
        self.last_update = Instant::now();
        self.stats.last_update_time_ms = start_time.elapsed().as_secs_f32() * 1000.0;
        self.stats.constraint_evaluation_count += 1;

        self.notify_force_compute_actor();

        Ok(produced)
    }

    /// Send the updated constraint buffer to ForceComputeActor
    /// Hand the live buffer to ForceComputeActor, the one device writer (ADR-2192): it keeps
    /// this set apart from the user and analytics constraints and uploads their union.
    /// Returns whether there was a writer to hand it to.
    fn notify_force_compute_actor(&self) -> bool {
        if let Some(ref addr) = self.force_compute_addr {
            info!(
                "OntologyConstraintActor: Sending {} constraints to ForceComputeActor",
                self.constraint_buffer.len()
            );
            addr.do_send(UpdateOntologyConstraintBuffer {
                constraint_buffer: self.constraint_buffer.clone(),
            });
            true
        } else {
            debug!(
                "OntologyConstraintActor: ForceComputeActor address not set, skipping notification"
            );
            false
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

    /// Put the live buffer on the device. This actor no longer writes the engine itself
    /// (ADR-2192): a direct write replaced the whole device set, user and analytics
    /// constraints included. It hands the buffer to ForceComputeActor, which uploads the union
    /// of the ontology set and the user set on its next step.
    fn upload_constraints_to_gpu(&self) -> Result<(), String> {
        if self.notify_force_compute_actor() {
            Ok(())
        } else {
            Err("ForceComputeActor not wired: the ontology constraints wait for it".to_string())
        }
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

impl Handler<SetOntologyConstraintPolicy> for OntologyConstraintActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(
        &mut self,
        msg: SetOntologyConstraintPolicy,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!(
            "OntologyConstraintActor: stored constraint policy enabled={} strength={:.3}",
            msg.enabled, msg.strength
        );

        // The live constraints come from the canonical mapper path
        // (`ingest_domain_axioms`), which fills `constraint_buffer_base` — NOT
        // the legacy `ontology_constraints` Vec. The live buffer is rebuilt from
        // that unscaled base, so strength changes are reversible and switching
        // the constraints back on needs no re-reasoning.
        if self.applied_policy == Some(msg) {
            return Ok(serde_json::json!({
                "success": true,
                "unchanged": true,
                "enabled": self.constraints_enabled,
                "appliedStrength": self.global_strength,
                "activeConstraints": self.constraint_buffer.len()
            }));
        }
        self.constraints_enabled = msg.enabled;
        self.global_strength = msg.strength;
        self.constraint_buffer = live_constraint_buffer(
            &self.constraint_buffer_base,
            self.constraints_enabled,
            self.global_strength,
        );
        let live = self.constraint_buffer.len() as u32;

        // ForceComputeActor is the one device writer: it replaces only its ontology
        // source (the user and analytics set stays) and uploads the union next step.
        // Without a writer the policy is not applied, so the next push retries it.
        if !self.notify_force_compute_actor() {
            self.applied_policy = None;
            return Err(
                "ForceComputeActor not wired: ontology-constraint policy not applied".into(),
            );
        }
        self.applied_policy = Some(msg);
        self.stats.active_ontology_constraints = live;

        Ok(serde_json::json!({
            "success": true,
            "enabled": self.constraints_enabled,
            "appliedStrength": self.global_strength,
            "retainedConstraints": self.constraint_buffer_base.len(),
            "activeConstraints": live
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

    fn class_node(id: u32, iri: &str, x: f32) -> visionclaw_domain::models::node::Node {
        let mut n = visionclaw_domain::models::node::Node::new_with_id(iri.to_string(), Some(id));
        n.owl_class_iri = Some(iri.to_string());
        n.set_x(x);
        n.set_y(0.0);
        n.set_z(0.0);
        n
    }

    fn sub_class_of(child: &str, parent: &str) -> DomainOwlAxiom {
        DomainOwlAxiom {
            id: None,
            axiom_type: visionclaw_domain::ports::owl_types::AxiomType::SubClassOf,
            subject: child.to_string(),
            object: parent.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn the_live_buffer_is_empty_when_off_and_the_base_at_strength_when_on() {
        let base = vec![ConstraintData {
            kind: 0,
            count: 2,
            node_idx: [1, 2, -1, -1],
            params: [10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            weight: 2.0,
            activation_frame: 0,
        }];
        assert!(live_constraint_buffer(&base, false, 0.6).is_empty());
        let on = live_constraint_buffer(&base, true, 0.25);
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].weight, 0.5);
        assert_eq!(on[0].node_idx, base[0].node_idx);
        // An unvalidated boot value can never put a non-finite or out-of-range weight on the
        // device.
        assert_eq!(live_constraint_buffer(&base, true, f32::NAN)[0].weight, 0.0);
        assert_eq!(live_constraint_buffer(&base, true, 7.0)[0].weight, 2.0);
    }

    /// ADR-2192, at the actor: with no device writer wired the policy is refused, and the
    /// same policy is applied once a writer is there (a failed push is retried, not skipped
    /// as "unchanged"); an identical policy after that is a no-op; switching off empties the
    /// writer's ontology source and leaves its user source; switching on re-applies the set
    /// at the new strength, without another sync.
    #[actix::test]
    async fn the_stored_policy_switches_the_live_set_off_and_on_without_re_reasoning() {
        use crate::actors::gpu::force_compute_actor::ForceComputeActor;
        use crate::actors::messages::{
            ApplyMaterializedAxioms, GetConstraintBuffer, GetDeviceConstraintCounts,
            SetForceComputeAddr, UpdateConstraints,
        };
        let mut graph = visionclaw_domain::models::graph::GraphData::new();
        graph.nodes = vec![
            class_node(1, "urn:vc:class:Dog", 0.0),
            class_node(2, "urn:vc:class:Animal", 40.0),
        ];
        let addr = OntologyConstraintActor::new().start();
        let off = SetOntologyConstraintPolicy {
            enabled: false,
            strength: 0.6,
        };
        assert!(
            addr.send(off).await.unwrap().is_err(),
            "no device writer: refused"
        );

        let force = ForceComputeActor::headless().start();
        // A user constraint the ontology switch must never remove.
        force
            .send(UpdateConstraints {
                constraint_data: serde_json::json!([{
                    "kind": 0, "count": 2, "node_idx": [5, 6, -1, -1],
                    "params": [10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    "weight": 1.0, "activation_frame": 0
                }]),
            })
            .await
            .unwrap()
            .unwrap();
        addr.send(SetForceComputeAddr {
            addr: force.clone(),
        })
        .await
        .unwrap();

        // Back on (the default), then a sync's axioms arrive at the default strength.
        let on = SetOntologyConstraintPolicy {
            enabled: true,
            strength: DEFAULT_GLOBAL_STRENGTH,
        };
        addr.send(on).await.unwrap().unwrap();
        let produced = addr
            .send(ApplyMaterializedAxioms {
                axioms: vec![sub_class_of("urn:vc:class:Dog", "urn:vc:class:Animal")],
                graph_data: graph,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(produced, 1);
        let at_default = addr.send(GetConstraintBuffer).await.unwrap().unwrap();
        let unscaled = at_default[0].weight / DEFAULT_GLOBAL_STRENGTH;
        let d = force.send(GetDeviceConstraintCounts).await.unwrap();
        assert_eq!((d.ontology, d.user), (1, 1));

        let same = addr.send(on).await.unwrap().unwrap();
        assert_eq!(same["unchanged"], true, "identical policy: no re-upload");

        // The earlier refused "off" is applied now that a writer exists.
        let applied = addr.send(off).await.unwrap().unwrap();
        assert_eq!(applied["activeConstraints"], 0);
        assert_eq!(applied["retainedConstraints"], 1);
        let d = force.send(GetDeviceConstraintCounts).await.unwrap();
        assert_eq!(
            (d.ontology, d.user),
            (0, 1),
            "off keeps the user constraint"
        );
        let stats = addr.send(GetOntologyStats).await.unwrap().unwrap();
        assert_eq!(stats.active_ontology_constraints, 0);

        addr.send(SetOntologyConstraintPolicy {
            enabled: true,
            strength: 0.3,
        })
        .await
        .unwrap()
        .unwrap();
        let back = addr.send(GetConstraintBuffer).await.unwrap().unwrap();
        assert_eq!(back.len(), 1);
        assert!((back[0].weight - unscaled * 0.3).abs() < 1e-6);
        let d = force.send(GetDeviceConstraintCounts).await.unwrap();
        assert_eq!((d.ontology, d.user), (1, 1));
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
