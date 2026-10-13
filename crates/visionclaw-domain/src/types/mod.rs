//! Domain types module

pub mod actor_types;
pub mod claude_flow;
pub mod layout;
pub mod mcp_responses;
pub mod ontology_tools;
pub mod physics_config;
pub mod semantic_config;
pub mod speech;
pub mod user_context;
pub mod vec3;

pub use actor_types::{AutoBalanceNotification, PhysicsState};
pub use layout::{ConstraintZone, LayoutMode, LayoutModeConfig, LayoutStatus};
pub use physics_config::{
    AutoBalanceConfig, AutoPauseConfig, ClusteringConfiguration, ConstraintSystem,
    LegacyConstraintData, PhysicsSettings, PhysicsUpdate, CANONICAL_MAX_FORCE,
    CANONICAL_MAX_VELOCITY,
};
pub use semantic_config::{
    AttributeSpringForce, CollisionForce, CrossDomainForce, DagForce, MaturityLayoutForce,
    PhysicalityClusterForce, RoleClusterForce, SemanticForces, TypeClusterForce,
};
pub use vec3::{BinaryNodeData, Vec3Data};
