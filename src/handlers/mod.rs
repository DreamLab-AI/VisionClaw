pub mod admin_sync_handler;
pub mod api_handler;
pub mod bots_handler;
pub mod bots_visualization_handler;
pub mod client_log_handler;
pub mod client_messages_handler;
pub mod clustering_handler;
pub mod consolidated_health_handler;
pub mod metrics_handler;
pub mod constraints_handler;
pub mod graph_export_handler;
pub mod graph_state_handler;
pub mod mcp_relay_handler;
pub mod multi_mcp_websocket_handler;
pub mod natural_language_query_handler;
pub mod nostr_handler;
pub mod ontology_handler;
pub mod ontology_agent_handler;
pub use ontology_agent_handler::configure_ontology_agent_routes;
pub mod pages_handler;
pub mod ragflow_handler;
pub mod settings_handler;
pub mod settings_validation_fix;
pub mod socket_flow_handler;
pub mod speech_socket_handler;
pub mod utils;
pub mod validation_handler;
pub mod websocket_utils;
pub mod workspace_handler;

// Phase 5: Hexagonal architecture handlers
pub mod physics_handler;
pub mod schema_handler;
pub mod semantic_handler;

pub use natural_language_query_handler::configure_nl_query_routes;
pub use physics_handler::configure_routes as configure_physics_routes;
pub use schema_handler::configure_schema_routes;
pub use semantic_handler::configure_routes as configure_semantic_routes;

// Phase 7: Inference handler
pub mod inference_handler;

pub use inference_handler::configure_routes as configure_inference_routes;

#[cfg(test)]
pub mod tests;
pub mod semantic_pathfinding_handler;
pub use semantic_pathfinding_handler::configure_pathfinding_routes;

// Briefing workflow handler
pub mod briefing_handler;
pub use briefing_handler::configure_routes as configure_briefing_routes;

// Memory flash handler (RuVector access → WS broadcast)
pub mod memory_flash_handler;
pub use memory_flash_handler::configure_routes as configure_memory_flash_routes;

// URI resolver (PRD-006 §5.2) — URN/CURIE parsing + 307/404/400 surface.
pub mod uri_resolver_handler;
pub use uri_resolver_handler::configure_routes as configure_uri_resolver_routes;

// Layout mode system (ADR-031)
pub mod layout_handler;
pub use layout_handler::configure_layout_routes;

// High-Performance Networking (QUIC/WebTransport)
pub mod quic_transport_handler;

// Solid Pod server — native `solid-pod-rs` backend (ADR-053/056).
// JSS proxy + shadow comparator retired 2026-04-20; the external
// `solid-pod-rs` crate is now the sole Pod implementation.
pub mod solid_pod_handler;
pub use solid_pod_handler::configure_routes as configure_solid_native_routes;
pub use solid_pod_handler::NativeSolidService;

// Image generation (ComfyUI Flux2)
pub mod image_gen_handler;
pub use image_gen_handler::configure_routes as configure_image_gen_routes;

// Server Nostr identity (kind 30023/30100/30200/30300/30301 signer pubkey)
pub mod server_identity_handler;
pub use server_identity_handler::configure_routes as configure_server_identity_routes;

// PRD-013 G7: Enrichment proposal REST surface (agentbox git-bridge → broker)
pub mod enrichment_proposal_handler;
pub use enrichment_proposal_handler::configure_routes as configure_enrichment_proposal_routes;

// ADR-059 / ADR-014 — bidirectional agent events WebSocket
pub mod agent_events_ws_handler;
// ADR-059 §3 Phase 3 — user-interaction REST → broadcaster
pub mod user_interaction_handler;

// PRD-008: XR presence WebSocket (`/ws/presence`) — Quest 3 native APK multi-user sync
pub mod presence_handler;

// Combined content + topology embedding discovery (384-dim MiniLM + 128-dim TransE)
pub mod discovery_handler;
pub use discovery_handler::configure_routes as configure_discovery_routes;

// DID document endpoint: /.well-known/did/nostr/{pubkey}.json
pub mod did_nostr_handler;
pub use did_nostr_handler::configure_routes as configure_did_nostr_routes;

// HTTP 402 payment gating via did:nostr pubkeys (Web Ledgers / solid-pod-rs)
pub mod pay_handler;
pub use pay_handler::configure_pay_routes;

// Ecosystem health aggregator — concurrent substrate liveness checks
pub mod ecosystem_health_handler;
pub use ecosystem_health_handler::configure_routes as configure_ecosystem_health_routes;

pub use quic_transport_handler::{
    QuicTransportServer, QuicServerConfig,
    PostcardNodeUpdate, PostcardBatchUpdate, PostcardDeltaUpdate,
    ControlMessage, TopologyNode, TopologyEdge,
    encode_postcard_batch, decode_postcard_batch, calculate_deltas,
};

