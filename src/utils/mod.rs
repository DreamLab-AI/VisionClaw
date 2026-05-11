//! Utility modules for validation, networking, and math.

// REMOVED: advanced_gpu_compute module - functionality moved to unified_gpu_compute
// REMOVED: gpu_compute module - legacy implementation replaced by unified_gpu_compute
pub mod actor_timeout;
pub mod advanced_logging;
pub mod async_improvements;
pub mod audio_processor;
pub mod auth;
pub mod binary_protocol;
pub mod cache;
// delta_encoding module removed by ADR-037 (Implemented 2026-04-20).
// V4 delta frames were never emitted in production; the module has been
// retired along with its callers.
pub mod client_message_extractor;
pub mod cuda_error_handling;
pub mod edge_data;
#[cfg(all(test, feature = "gpu"))]
mod gpu_compute_tests;
pub mod gpu_diagnostics;
#[cfg(feature = "gpu")]
pub mod gpu_memory;
pub mod gpu_safety;
pub mod handler_commons;
pub mod response_macros;
// pub mod hybrid_fault_tolerance;
// pub mod hybrid_performance_optimizer;
pub mod json;
// REMOVED: pub mod logging; - Superseded by advanced_logging, archived to archive/legacy_code_2025_11_03/
// Re-export advanced_logging as 'logging' for backwards compatibility
pub mod logging {
    pub use super::advanced_logging::is_debug_enabled;
}
pub mod math;
pub mod mcp_client_utils; // Consolidated MCP client utilities (Phase 2, Task 2.6)
pub mod mcp_connection; // Legacy wrapper - to be migrated to mcp_client_utils
pub mod mcp_tcp_client; // Legacy wrapper - to be migrated to mcp_client_utils
pub mod memory_bounds;
pub mod network;
pub mod nip98; // NIP-98 HTTP authentication for Solid Server integration
pub mod ptx;
#[cfg(test)]
mod ptx_tests;
pub mod result_helpers;
pub mod socket_flow_constants;
pub mod socket_flow_messages;
pub mod standard_websocket_messages;
pub mod time;
pub mod unified_gpu_compute;
pub mod validation;
pub mod websocket_heartbeat;
// REMOVED: result_mappers module - no longer exists
pub mod canonical_iri; // ADR-050: visionclaw:owner:{npub}/kg/{sha256(path)}
pub mod neo4j_helpers;
pub mod opaque_id; // ADR-050: HMAC-derived session opaque ids
