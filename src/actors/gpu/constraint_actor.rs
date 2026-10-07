//! Constraint Actor - Handles constraint management and updates

use actix::prelude::*;
use log::{debug, error, info};
use std::sync::Arc;

use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;
use crate::models::constraints::ConstraintData;
use visionclaw_domain::models::constraints::{Constraint, ConstraintKind, ConstraintSet};

pub struct ConstraintActor {
    gpu_state: GPUState,

    shared_context: Option<Arc<SharedGPUContext>>,

    constraints: Vec<Constraint>,
}

impl ConstraintActor {
    pub fn new() -> Self {
        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            constraints: Vec::new(),
        }
    }

    fn update_constraints(&mut self, new_constraints: Vec<Constraint>) -> Result<(), String> {
        info!(
            "ConstraintActor: Updating constraints - {} current, {} new",
            self.constraints.len(),
            new_constraints.len()
        );

        self.constraints = new_constraints;

        if self.shared_context.is_some() {
            self.upload_constraints_to_gpu()?;
        } else {
            info!("ConstraintActor: GPU not initialized, constraints stored locally");
        }

        info!(
            "ConstraintActor: Constraint update completed - {} total constraints",
            self.constraints.len()
        );
        Ok(())
    }

    fn upload_constraints_to_gpu(&self) -> Result<(), String> {
        info!(
            "ConstraintActor: Uploading {} constraints to GPU",
            self.constraints.len()
        );

        let mut unified_compute = match &self.shared_context {
            Some(ctx) => ctx
                .unified_compute
                .lock()
                .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?,
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        let constraint_data = self.convert_constraints_to_gpu_format()?;

        if constraint_data.is_empty() {
            info!("ConstraintActor: No constraints to upload, clearing GPU constraints");
            unified_compute
                .clear_constraints()
                .map_err(|e| format!("Failed to clear GPU constraints: {}", e))?;
        } else {
            unified_compute
                .upload_constraints(&constraint_data)
                .map_err(|e| format!("Failed to upload constraints to GPU: {}", e))?;

            info!(
                "ConstraintActor: Successfully uploaded {} constraint entries to GPU",
                constraint_data.len()
            );
        }

        Ok(())
    }

    fn convert_constraints_to_gpu_format(&self) -> Result<Vec<ConstraintData>, String> {
        let mut constraint_data = Vec::new();

        for constraint in self.constraints.iter() {
            if constraint.active {
                for &node_idx in &constraint.node_indices {
                    if node_idx >= self.gpu_state.num_nodes {
                        error!(
                            "ConstraintActor: Node index {} out of range (max: {})",
                            node_idx,
                            self.gpu_state.num_nodes - 1
                        );
                        continue;
                    }
                }

                let gpu_constraint = ConstraintData::from_constraint(constraint);
                constraint_data.push(gpu_constraint);
            }
        }

        info!(
            "ConstraintActor: Converted {} active constraints to {} GPU constraint entries",
            self.constraints.iter().filter(|c| c.active).count(),
            constraint_data.len()
        );

        Ok(constraint_data)
    }

    fn get_current_constraints(&self) -> ConstraintSet {
        ConstraintSet {
            constraints: self.constraints.clone(),
            groups: std::collections::HashMap::new(),
        }
    }

    fn clear_constraints(&mut self) -> Result<(), String> {
        info!("ConstraintActor: Clearing all constraints");

        self.constraints.clear();

        if let Some(ctx) = &self.shared_context {
            let mut unified_compute = ctx
                .unified_compute
                .lock()
                .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?;

            unified_compute
                .clear_constraints()
                .map_err(|e| format!("Failed to clear GPU constraints: {}", e))?;

            info!("ConstraintActor: GPU constraints cleared");
        }

        info!("ConstraintActor: All constraints cleared successfully");
        Ok(())
    }

    fn get_constraint_statistics(&self) -> ConstraintStatistics {
        let mut stats = ConstraintStatistics {
            total_constraints: self.constraints.len(),
            distance_constraints: 0,
            angle_constraints: 0,
            position_constraints: 0,
            cluster_constraints: 0,
            active_constraints: self.constraints.len(),
        };

        for constraint in &self.constraints {
            if constraint.active {
                match constraint.kind {
                    ConstraintKind::Separation => stats.distance_constraints += 1,
                    ConstraintKind::FixedPosition => stats.position_constraints += 1,
                    ConstraintKind::Clustering => {
                        stats.cluster_constraints += 1;

                        stats.total_constraints += constraint.node_indices.len().saturating_sub(1);
                    }
                    ConstraintKind::AlignmentHorizontal
                    | ConstraintKind::AlignmentVertical
                    | ConstraintKind::AlignmentDepth => stats.angle_constraints += 1,
                    _ => {}
                }
            }
        }

        stats
    }
}

impl Default for ConstraintActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ConstraintActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Constraint Actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Constraint Actor stopped");
    }
}

// === Message Handlers ===

impl Handler<UpdateConstraints> for ConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        info!("ConstraintActor: UpdateConstraints received");

        let constraints =
            match serde_json::from_value::<Vec<Constraint>>(msg.constraint_data.clone()) {
                Ok(constraints) => constraints,
                Err(e) => match serde_json::from_value::<ConstraintSet>(msg.constraint_data) {
                    Ok(constraint_set) => constraint_set.constraints,
                    Err(_) => {
                        error!("ConstraintActor: Failed to parse constraint_data: {}", e);
                        return Err(format!("Failed to parse constraints: {}", e));
                    }
                },
            };

        self.update_constraints(constraints)
    }
}

impl Handler<GetConstraints> for ConstraintActor {
    type Result = Result<ConstraintSet, String>;

    fn handle(&mut self, _msg: GetConstraints, _ctx: &mut Self::Context) -> Self::Result {
        debug!("ConstraintActor: GetConstraints request");
        Ok(self.get_current_constraints())
    }
}

impl Handler<UploadConstraintsToGPU> for ConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UploadConstraintsToGPU, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ConstraintActor: UploadConstraintsToGPU received - {} constraint entries",
            msg.constraint_data.len()
        );

        let mut unified_compute = match &self.shared_context {
            Some(ctx) => ctx
                .unified_compute
                .lock()
                .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?,
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        unified_compute
            .upload_constraints(&msg.constraint_data)
            .map_err(|e| format!("Failed to upload constraints to GPU: {}", e))?;

        info!(
            "ConstraintActor: Successfully uploaded {} constraint entries to GPU",
            msg.constraint_data.len()
        );
        Ok(())
    }
}

// Custom message handlers for constraint management
impl Handler<ClearConstraints> for ConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: ClearConstraints, _ctx: &mut Self::Context) -> Self::Result {
        self.clear_constraints()
    }
}

impl Handler<GetConstraintStatistics> for ConstraintActor {
    type Result = Result<ConstraintStatistics, String>;

    fn handle(&mut self, _msg: GetConstraintStatistics, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.get_constraint_statistics())
    }
}

// Custom messages for constraint management
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ClearConstraints;

#[derive(Message)]
#[rtype(result = "Result<ConstraintStatistics, String>")]
pub struct GetConstraintStatistics;

impl Handler<SetSharedGPUContext> for ConstraintActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("ConstraintActor: Received SharedGPUContext from ResourceActor");
        self.shared_context = Some(msg.context);

        info!("ConstraintActor: SharedGPUContext stored successfully");
        Ok(())
    }
}

// Constraint statistics structure
#[derive(Debug, Clone)]
pub struct ConstraintStatistics {
    pub total_constraints: usize,
    pub distance_constraints: usize,
    pub angle_constraints: usize,
    pub position_constraints: usize,
    pub cluster_constraints: usize,
    pub active_constraints: usize,
}
