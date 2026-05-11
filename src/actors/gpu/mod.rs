//! GPU actor module for specialized GPU computation actors
//!
//! ## Architecture (Phase 7: God Actor Decomposition)
//!
//! The GPU subsystem is organized into independent subsystems managed by
//! dedicated supervisors with proper error isolation and timeout handling:
//!
//! ```text
//!                    GPUManagerActor (Coordinator)
//!                           |
//!          +----------------+----------------+----------------+
//!          |                |                |                |
//!   ResourceSupervisor  PhysicsSupervisor  AnalyticsSupervisor  GraphAnalyticsSupervisor
//!          |                |                |                |
//!   GPUResourceActor   +----+----+      +----+----+      +----+----+
//!                      |    |    |      |    |    |      |         |
//!                   Force Stress Constraint Cluster Anomaly PageRank ShortestPath ConnComp
//! ```
//!
//! ### Subsystem Supervisors
//!
//! - **ResourceSupervisor**: Manages GPU initialization with timeout handling
//! - **PhysicsSupervisor**: Force computation, stress majorization, constraints
//! - **AnalyticsSupervisor**: Clustering, anomaly detection, PageRank
//! - **GraphAnalyticsSupervisor**: Shortest path, connected components
//!
//! ### Error Isolation
//!
//! Each subsystem supervisor:
//! - Isolates failures from other subsystems
//! - Implements exponential backoff restart policies
//! - Reports health status independently
//! - Receives SharedGPUContext via GPUContextBus broadcast

// Child actors
pub mod anomaly_detection_actor;
pub mod clustering_actor;
pub mod constraint_actor;
pub mod context_bus;
pub mod cuda_stream_wrapper;
pub mod force_compute_actor;
pub mod gpu_manager_actor;
pub mod gpu_resource_actor;
pub mod ontology_constraint_actor;
pub mod pagerank_actor;
pub mod shortest_path_actor;
pub mod connected_components_actor;
pub mod shared;
pub mod stress_majorization_actor;
pub mod semantic_forces_actor;

// Physics metrics helpers (extracted from force_compute_actor.rs — P3-05 decomposition)
pub mod physics_metrics;

// Supervisor actors
pub mod supervisor_messages;
pub mod physics_supervisor;
pub mod analytics_supervisor;
pub mod graph_analytics_supervisor;
pub mod resource_supervisor;

// Child actor exports
pub use anomaly_detection_actor::AnomalyDetectionActor;
pub use clustering_actor::ClusteringActor;
pub use constraint_actor::ConstraintActor;
pub use context_bus::{GPUContextBus, GPUContextReady, GPUContextSubscriber, GPUContextSubscription};
pub use force_compute_actor::ForceComputeActor;
pub use gpu_manager_actor::GPUManagerActor;
pub use gpu_resource_actor::GPUResourceActor;
pub use ontology_constraint_actor::OntologyConstraintActor;
pub use pagerank_actor::PageRankActor;
pub use shortest_path_actor::ShortestPathActor;
pub use connected_components_actor::ConnectedComponentsActor;
pub use shared::{GPUContext, SharedGPUContext, UnifiedGPUCompute};
pub use stress_majorization_actor::StressMajorizationActor;
pub use semantic_forces_actor::SemanticForcesActor;

// Supervisor exports
pub use supervisor_messages::{
    ActorFailure, ActorHealthState, ActorRecovered, GetSubsystemHealth,
    InitializationTimeouts, InitializeSubsystem, RestartActor, RestartSubsystem,
    RouteMessage, SubsystemHealth, SubsystemInitialized, SubsystemStatus,
    SubsystemType, SupervisionPolicy,
};
pub use physics_supervisor::PhysicsSupervisor;
pub use analytics_supervisor::AnalyticsSupervisor;
pub use graph_analytics_supervisor::GraphAnalyticsSupervisor;
pub use resource_supervisor::{ResourceSupervisor, GetContextBus, SetSubsystemSupervisors};
