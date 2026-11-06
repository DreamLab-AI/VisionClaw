// ADR-090: model types are canonical in visionclaw-domain. We re-export
// the modules themselves so existing webxr callers writing
// `crate::models::node::Node` (etc.) continue to resolve to the domain
// type. The duplicate shim FILES were deleted; this is module-alias-only.
pub use visionclaw_domain::models::edge;
pub use visionclaw_domain::models::graph;
pub use visionclaw_domain::models::metadata;
pub use visionclaw_domain::models::node;
// constraints is a HYBRID — visionclaw-domain owns the trait/struct types,
// but webxr has its own ConstraintData (bytemuck/GPU layout) + extension
// traits. `pub mod constraints` below re-exports from domain inside the
// webxr-local file.
pub mod constraints;
pub mod force_channels;
pub mod graph_export;
pub mod graph_types;
pub mod pagination;
pub mod protected_settings;
pub mod ragflow_chat;
pub mod rbac;
pub mod simulation_params;
pub mod user_settings;
pub mod workspace;

pub use pagination::PaginationParams;
pub use simulation_params::SimulationParams;
pub use user_settings::UserSettings;
pub use visionclaw_domain::models::edge::SemanticEdgeType;
pub use visionclaw_domain::models::metadata::MetadataStore;
pub use workspace::{
    CreateWorkspaceRequest, UpdateWorkspaceRequest, Workspace, WorkspaceListResponse,
    WorkspaceResponse,
};
