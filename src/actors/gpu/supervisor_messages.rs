//! Supervisor messages for GPU subsystem management
//!
//! Defines messages for health monitoring, restart policies, and error isolation
//! across GPU subsystem supervisors (Physics, Analytics, GraphAnalytics, Resource).

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ============================================================================
// Subsystem Health and Status Messages
// ============================================================================

/// Request health status from a subsystem supervisor
#[derive(Message, Debug, Clone)]
#[rtype(result = "SubsystemHealth")]
pub struct GetSubsystemHealth;

/// Health status of a subsystem and its managed actors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemHealth {
    /// Name of the subsystem (e.g., "physics", "analytics")
    pub subsystem_name: String,
    /// Overall health status
    pub status: SubsystemStatus,
    /// Number of healthy actors
    pub healthy_actors: u32,
    /// Total number of managed actors
    pub total_actors: u32,
    /// List of actor health states
    pub actor_states: Vec<ActorHealthState>,
    /// Time since last successful operation
    pub last_success_ms: Option<u64>,
    /// Number of restarts in the current window
    pub restart_count: u32,
}

/// Status of a subsystem
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubsystemStatus {
    /// All actors healthy and operational
    Healthy,
    /// Some actors degraded but subsystem functional
    Degraded,
    /// Subsystem initializing
    Initializing,
    /// Subsystem failed and needs intervention
    Failed,
    /// Subsystem stopped
    Stopped,
}

/// Health state of an individual actor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorHealthState {
    pub actor_name: String,
    pub is_running: bool,
    pub has_context: bool,
    pub failure_count: u32,
    pub last_error: Option<String>,
}

// ============================================================================
// Actor Lifecycle Messages
// ============================================================================

/// Notify supervisor that an actor has failed
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ActorFailure {
    pub actor_name: String,
    pub error: String,
    pub is_fatal: bool,
}

/// Notify supervisor that an actor has recovered
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ActorRecovered {
    pub actor_name: String,
}

/// Request to restart a specific actor
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct RestartActor {
    pub actor_name: String,
    pub reason: String,
}

/// Request to restart the entire subsystem
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct RestartSubsystem {
    pub reason: String,
}

// ============================================================================
// Initialization Messages
// ============================================================================

/// Initialize a subsystem with GPU context
#[derive(Message, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct InitializeSubsystem {
    pub context: std::sync::Arc<super::shared::SharedGPUContext>,
    pub graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,
    pub timeout: Duration,
}

/// Subsystem initialization complete notification
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SubsystemInitialized {
    pub subsystem_name: String,
    pub success: bool,
    pub error: Option<String>,
}

// ============================================================================
// Supervision Policy
// ============================================================================

/// Supervision policy for actor restarts
#[derive(Debug, Clone)]
pub struct SupervisionPolicy {
    /// Maximum restart attempts within the window
    pub max_restarts: u32,
    /// Time window for restart counting
    pub restart_window: Duration,
    /// Delay between restart attempts
    pub restart_delay: Duration,
    /// Backoff multiplier for successive restarts
    pub backoff_multiplier: f64,
    /// Maximum delay after backoff
    pub max_delay: Duration,
    /// Whether to escalate to parent on max restarts exceeded
    pub escalate_on_failure: bool,
}

impl Default for SupervisionPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            restart_window: Duration::from_secs(60),
            restart_delay: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(30),
            escalate_on_failure: true,
        }
    }
}

impl SupervisionPolicy {
    /// Create a policy for critical actors (more restarts, longer window)
    pub fn critical() -> Self {
        Self {
            max_restarts: 5,
            restart_window: Duration::from_secs(120),
            restart_delay: Duration::from_millis(250),
            backoff_multiplier: 1.5,
            max_delay: Duration::from_secs(15),
            escalate_on_failure: true,
        }
    }

    /// Create a policy for non-critical actors (fewer restarts, quick fail)
    pub fn non_critical() -> Self {
        Self {
            max_restarts: 2,
            restart_window: Duration::from_secs(30),
            restart_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(5),
            escalate_on_failure: false,
        }
    }

    /// Phase 5 / ADR-01 D4 policy for `ForceComputeActor`: OneForOne
    /// supervision with a strict 3-restarts-per-60-seconds budget. On exceed,
    /// the actor is stopped and a `SubsystemHealth::Failed` alarm is surfaced.
    /// Restart delay is short (250ms) because a cold CUDA context init is the
    /// dominant cost; no exponential backoff is applied within the window.
    pub fn physics_v2() -> Self {
        Self {
            max_restarts: 3,
            restart_window: Duration::from_secs(60),
            restart_delay: Duration::from_millis(250),
            backoff_multiplier: 1.0, // ADR-01 D4: no backoff within the window
            max_delay: Duration::from_millis(250),
            escalate_on_failure: true,
        }
    }
}

// ============================================================================
// Timeout Configuration
// ============================================================================

/// Configuration for initialization timeouts
#[derive(Debug, Clone)]
pub struct InitializationTimeouts {
    /// Timeout for GPU device initialization
    pub device_init: Duration,
    /// Timeout for PTX module loading
    pub ptx_load: Duration,
    /// Timeout for graph data upload
    pub graph_upload: Duration,
    /// Timeout for context distribution to child actors
    pub context_distribution: Duration,
    /// Total timeout for full initialization sequence
    pub total: Duration,
}

impl Default for InitializationTimeouts {
    fn default() -> Self {
        Self {
            device_init: Duration::from_secs(10),
            ptx_load: Duration::from_secs(5),
            graph_upload: Duration::from_secs(30),
            context_distribution: Duration::from_secs(5),
            total: Duration::from_secs(60),
        }
    }
}

// ============================================================================
// Message Routing
// ============================================================================

/// Route a message to the appropriate subsystem
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct RouteMessage {
    pub target_subsystem: SubsystemType,
    pub message_type: String,
    pub payload: serde_json::Value,
}

/// Type of GPU subsystem
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubsystemType {
    Physics,
    Analytics,
    GraphAnalytics,
    Resource,
}

impl std::fmt::Display for SubsystemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubsystemType::Physics => write!(f, "physics"),
            SubsystemType::Analytics => write!(f, "analytics"),
            SubsystemType::GraphAnalytics => write!(f, "graph_analytics"),
            SubsystemType::Resource => write!(f, "resource"),
        }
    }
}
