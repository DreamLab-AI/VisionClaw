use crate::models::metadata::Metadata;
use crate::models::node::Node;
use crate::services::file_service::FileService;
use crate::types::vec3::Vec3Data;
use crate::{ok_json, error_json, bad_request};
use crate::AppState;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
// GraphService direct import is no longer needed as we use actors
// use crate::services::graph_service::GraphService;
use crate::actors::messages::{AddNodesFromMetadata, GetPhysicsStats, GetSettings};
use crate::models::graph_types::{classify_node_population, NodePopulation};
use crate::application::graph::queries::{
    GetAutoBalanceNotifications, GetGraphData, GetNodeMap, GetPhysicsState,
};
use crate::actors::graph_actor::PhysicsState;
use crate::models::graph::GraphData;
use crate::handlers::utils::execute_in_thread;
use hexser::{Hexserror, QueryHandler};

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettlementState {
    pub is_settled: bool,
    pub stable_frame_count: u32,
    pub kinetic_energy: f32,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeWithPosition {

    pub id: u32,
    pub metadata_id: String,
    pub label: String,

    pub position: Vec3Data,
    /// Velocity is REDUNDANT with the 28 B/node binary stream (ADR-061).
    /// The WS stream warms in <1s; the JSON init's role is render-bootstrap,
    /// not transient-state delivery. Skipping saves ~80 B/node × 25k = ~2 MB.
    /// Client default-zero handling lives in graphDataManager.ts:745.
    #[serde(skip)]
    pub velocity: Vec3Data,


    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,


    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    // ----------------------------------------------------------------------
    // ADR-050 sovereign-model fields (propagated to the REST wire so the
    // client can render opacified private nodes)
    // ----------------------------------------------------------------------
    /// Owner's Nostr pubkey — preserved on opacified nodes so future
    /// delegation/ACLs can reference the owner. `None` for public nodes with
    /// no sovereign owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_pubkey: Option<String>,

    /// Per-session HMAC-derived opaque id. Populated when the node was
    /// opacified for this caller (i.e. cross-user private). 24 hex chars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opaque_id: Option<String>,

    /// Solid Pod URL for the authoritative payload. Cleared to `None` on
    /// opacification so cross-user callers cannot dereference the pod.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphResponse {
    pub nodes: Vec<Node>,
    pub edges: Vec<crate::models::edge::Edge>,
    pub metadata: HashMap<String, Metadata>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphResponseWithPositions {
    pub nodes: Vec<NodeWithPosition>,
    pub edges: Vec<crate::models::edge::Edge>,
    pub metadata: HashMap<String, Metadata>,
    pub settlement_state: SettlementState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedGraphResponse {
    pub nodes: Vec<Node>,
    pub edges: Vec<crate::models::edge::Edge>,
    pub metadata: HashMap<String, Metadata>,
    pub total_pages: usize,
    pub current_page: usize,
    pub total_items: usize,
    pub page_size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphQuery {
    pub query: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub sort: Option<String>,
    pub filter: Option<String>,
    pub graph_type: Option<String>,
}

/// Returns true if `metadata` passes the ownership-aware visibility filter
/// for the given caller.
///
/// Rules (ADR-028 extension, sovereign-mesh):
/// - `visibility == "public"` (or the field is absent — legacy row treated
///   as public for backwards compatibility) → visible to everyone.
/// - `visibility == "private"` → visible only when `caller_pubkey` matches
///   the node's `owner_pubkey` metadata. Anonymous callers (no pubkey)
///   never see private nodes; signed callers see only their own.
/// - Any other visibility value → default-deny (hidden unless owner matches)
///   so future values fail closed.
///
/// Replicates the Cypher form `COALESCE(n.visibility, 'public')` planned for
/// the sibling ADR-050 schema rollout; applying it here keeps the auth
/// sprint deliverable self-contained while the schema lands.
pub fn visibility_allows(
    metadata: &HashMap<String, String>,
    caller_pubkey: Option<&str>,
) -> bool {
    let visibility = metadata
        .get("visibility")
        .map(|s| s.as_str())
        .unwrap_or("public");

    match visibility {
        "public" => true,
        _ => {
            // Private (or any non-public value): owner must match caller.
            match (caller_pubkey, metadata.get("owner_pubkey")) {
                (Some(caller), Some(owner)) if !caller.is_empty() => caller == owner,
                _ => false,
            }
        }
    }
}

/// ADR-050 (H1): opacify a cross-user private node instead of dropping it.
///
/// Returns a clone of `node` with identifying fields stripped:
/// - `label`, `metadata_id`, `metadata` map cleared to empty
/// - `pod_url` cleared to None (cross-user callers cannot dereference)
/// - `opaque_id` populated via HMAC (`src/utils/opaque_id.rs`) so the client
///   has a stable-within-salt-window handle for diffing
/// - `owner_pubkey` preserved (needed for future delegation/ACL checks)
/// - `x/y/z/vx/vy/vz` preserved (topology is visible per ADR-050 §three-tier)
/// - `color/size/weight/group/node_type` preserved (shape metrics are OK)
///
/// When `salt` is `None` (e.g. `OPAQUE_ID_SALT_SEED` unset), the opaque id is
/// left as `None` and the caller gets a structurally-opacified placeholder
/// with no stable handle. This fails closed — no leakage either way.
pub fn opacify_for_caller(
    node: &NodeWithPosition,
    salt: Option<&[u8]>,
) -> NodeWithPosition {
    let owner = node.metadata.get("owner_pubkey").cloned();
    let canonical_iri = node
        .metadata
        .get("canonical_iri")
        .cloned()
        // Fallback: use metadata_id as the iri input when no explicit
        // canonical_iri is stored. This keeps determinism for legacy rows.
        .unwrap_or_else(|| node.metadata_id.clone());

    let opaque_id = match (salt, owner.as_deref()) {
        (Some(s), Some(o)) => Some(crate::utils::opaque_id::opaque_id(s, o, &canonical_iri)),
        _ => None,
    };

    NodeWithPosition {
        id: node.id,
        // Identifying fields: cleared
        metadata_id: String::new(),
        label: String::new(),
        metadata: HashMap::new(),
        pod_url: None,
        // Topology / shape: preserved
        position: node.position,
        velocity: node.velocity,
        node_type: node.node_type.clone(),
        size: node.size,
        color: node.color.clone(),
        weight: node.weight,
        group: node.group.clone(),
        // Sovereign fields
        owner_pubkey: owner,
        opaque_id,
    }
}

pub async fn get_graph_data(
    state: web::Data<AppState>,
    query: web::Query<GraphQuery>,
    req: HttpRequest,
) -> impl Responder {
    // Caller identity derived from the `RequireAuth::optional()` middleware.
    // Anonymous callers arrive with `pubkey == ""`; signed callers carry
    // their NIP-98 pubkey. `filter(!p.is_empty())` normalises the former to
    // `None` so downstream code only sees meaningful identities.
    let caller_pubkey: Option<String> = crate::middleware::auth::get_authenticated_user(&req)
        .map(|u| u.pubkey)
        .filter(|p| !p.is_empty());

    info!(
        "Received request for graph data (CQRS Phase 1D), graph_type={:?}, caller_authenticated={}",
        query.graph_type,
        caller_pubkey.is_some()
    );

    // Early-return path: serve the full :OntologyClass graph directly from
    // Neo4j when graph_type=ontology. The KGNode-based pipeline only returns
    // nodes that happen to carry an owl_class_iri field; dedicated
    // :OntologyClass nodes with their SUBCLASS_OF / RELATES edges are
    // invisible to that pipeline and must be fetched via the ontology
    // repository. (Label was historically :OwlClass — see ADR-048 + 0045.)
    if query.graph_type.as_deref() == Some("ontology") {
        return match state.ontology_repository.load_ontology_graph_data().await {
            Ok(graph_data) => {
                let nodes_with_positions: Vec<NodeWithPosition> = graph_data
                    .nodes
                    .iter()
                    .map(|node| {
                        let position = node.data.position();
                        let velocity = node.data.velocity();
                        NodeWithPosition {
                            id: node.id,
                            metadata_id: node.metadata_id.clone(),
                            label: node.label.clone(),
                            position,
                            velocity,
                            metadata: node.metadata.clone(),
                            node_type: node.node_type.clone(),
                            size: node.size,
                            color: node.color.clone(),
                            weight: node.weight,
                            group: node.group.clone(),
                            owner_pubkey: node.owner_pubkey.clone(),
                            opaque_id: node.opaque_id.clone(),
                            pod_url: node.pod_url.clone(),
                        }
                    })
                    .collect();

                let response = GraphResponseWithPositions {
                    nodes: nodes_with_positions,
                    edges: graph_data.edges.clone(),
                    metadata: graph_data.metadata.clone(),
                    settlement_state: SettlementState {
                        is_settled: false,
                        stable_frame_count: 0,
                        kinetic_energy: 0.0,
                    },
                };

                info!(
                    "Sending ontology graph data: {} OwlClass nodes, {} edges",
                    response.nodes.len(),
                    response.edges.len()
                );

                ok_json!(response)
            }
            Err(e) => {
                error!("Failed to load ontology graph data: {}", e);
                Ok(HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "Failed to retrieve ontology graph data"})))
            }
        };
    }

    let graph_handler = state.graph_query_handlers.get_graph_data.clone();
    let node_map_handler = state.graph_query_handlers.get_node_map.clone();
    let physics_handler = state.graph_query_handlers.get_physics_state.clone();

    
    let graph_future = execute_in_thread(move || graph_handler.handle(GetGraphData));
    let node_map_future = execute_in_thread(move || node_map_handler.handle(GetNodeMap));
    let physics_future = execute_in_thread(move || physics_handler.handle(GetPhysicsState));

    let (graph_result, node_map_result, physics_result): (
        Result<Result<Arc<GraphData>, Hexserror>, String>,
        Result<Result<Arc<HashMap<u32, Node>>, Hexserror>, String>,
        Result<Result<PhysicsState, Hexserror>, String>,
    ) = tokio::join!(graph_future, node_map_future, physics_future);

    match (graph_result, node_map_result, physics_result) {
        (Ok(Ok(graph_data)), Ok(Ok(_node_map)), Ok(Ok(physics_state))) => {
            debug!(
                "Preparing enhanced graph response with {} nodes, {} edges, physics state: {:?}",
                graph_data.nodes.len(),
                graph_data.edges.len(),
                physics_state
            );


            let nodes_with_positions: Vec<NodeWithPosition> = graph_data
                .nodes
                .iter()
                .map(|node| {
                    // Use node's own data for position and velocity
                    // node_map contains HashMap<i32, Vec<i32>>, not physics nodes
                    let position = node.data.position();
                    let velocity = node.data.velocity();

                    NodeWithPosition {
                        id: node.id,
                        metadata_id: node.metadata_id.clone(),
                        label: node.label.clone(),
                        position,
                        velocity,
                        metadata: node.metadata.clone(),
                        node_type: node.node_type.clone(),
                        size: node.size,
                        color: node.color.clone(),
                        weight: node.weight,
                        group: node.group.clone(),
                        owner_pubkey: node.owner_pubkey.clone(),
                        opaque_id: node.opaque_id.clone(),
                        pod_url: node.pod_url.clone(),
                    }
                })
                .collect();

            // ADR-036: Filter nodes using canonical classify_node_population.
            // ADR-050 (H1): opacify cross-user private nodes rather than
            // dropping them. The three-tier privacy contract says private
            // nodes stay in the response with label/metadata stripped and an
            // HMAC-derived `opaque_id` so the client can render a placeholder
            // stub. Nodes still fail the graph_type filter as usual.
            //
            // Resolve the opaque-id salt once per request. The salt is
            // rotated daily by a background task; we read it lazily here so
            // a re-key takes effect on the next request.
            let caller_ref = caller_pubkey.as_deref();
            let opaque_salt: Option<Vec<u8>> = std::env::var("OPAQUE_ID_SALT_SEED")
                .ok()
                .filter(|s| s.len() >= 16)
                .map(|s| s.into_bytes());
            let salt_ref = opaque_salt.as_deref();

            // Codex audit 2026-04-30: drop display-empty `kg_stub` nodes
            // BEFORE the type/visibility pipeline. These are MERGE-on-id
            // orphans (an edge referenced an unknown id; the writer
            // auto-created a typed-stub row to keep topology consistent).
            // They have no `metadata_id` and no `label` — nothing to render.
            // Keeping 21k+ of them in JSON init was costing ~3.6 MB of
            // boilerplate per request and contributing to client init hangs.
            // ADR-050 §H1 "preserve topology" was about opacified MEANINGFUL
            // private nodes, not unresolved-id placeholders.
            let mut empty_stubs_dropped = 0usize;
            let filtered_nodes: Vec<NodeWithPosition> = nodes_with_positions
                .into_iter()
                .filter_map(|node| {
                    // Drop empty kg_stub orphans (no displayable identity).
                    let is_empty_stub = node.node_type.as_deref() == Some("kg_stub")
                        && node.metadata_id.is_empty()
                        && node.label.is_empty();
                    if is_empty_stub {
                        empty_stubs_dropped += 1;
                        return None;
                    }

                    // Graph-type filter — applies regardless of visibility.
                    let type_matches = match query.graph_type.as_deref() {
                        Some("knowledge") => {
                            classify_node_population(node.node_type.as_deref()) == NodePopulation::Knowledge
                        }
                        Some("ontology") => {
                            classify_node_population(node.node_type.as_deref()) == NodePopulation::Ontology
                                || node.metadata.contains_key("owl_class_iri")
                        }
                        Some("agent") => {
                            classify_node_population(node.node_type.as_deref()) == NodePopulation::Agent
                                || node.metadata.contains_key("agentType")
                        }
                        _ => true, // No type filter; admit all remaining types.
                    };
                    if !type_matches {
                        return None;
                    }

                    // Visibility gate: full node for owner, opacified for others.
                    // Topology preserved via opaque_id (not by passing the raw stub).
                    if visibility_allows(&node.metadata, caller_ref) {
                        Some(node)
                    } else {
                        Some(opacify_for_caller(&node, salt_ref))
                    }
                })
                .collect();
            if empty_stubs_dropped > 0 {
                info!(
                    "[graph_init] dropped {} empty kg_stub orphans from JSON init payload",
                    empty_stubs_dropped
                );
            }

            // Filter edges to only include those connecting filtered nodes,
            // then strip server-internal metadata keys before going on the wire.
            //
            // Edge metadata accumulated several persistence-only fields over
            // time (target_wikilink for stub-label rescue, last_seen_run_id for
            // ADR-051 retraction, neo4j_relationship for the writer). The
            // client only consumes `edge_type` / `relation_type` (see
            // hierarchyDetector.ts). Sending the rest costs ~80 bytes per edge
            // × 10k+ edges = ~1 MB of pure server-internal noise that bloated
            // the response and contributed to client init hangs.
            let filtered_node_ids: std::collections::HashSet<u32> =
                filtered_nodes.iter().map(|n| n.id).collect();
            const EDGE_META_ALLOWLIST: &[&str] = &["edge_type", "relation_type"];
            let filtered_edges: Vec<_> = graph_data
                .edges
                .iter()
                .filter(|e| {
                    filtered_node_ids.contains(&e.source)
                        && filtered_node_ids.contains(&e.target)
                })
                .map(|e| {
                    let mut e2 = e.clone();
                    if let Some(m) = e2.metadata.as_mut() {
                        m.retain(|k, _| EDGE_META_ALLOWLIST.contains(&k.as_str()));
                        if m.is_empty() {
                            e2.metadata = None;
                        }
                    }
                    e2
                })
                .collect();

            // Query real-time physics stats from ForceComputeActor (if available).
            // Previously this block hardcoded is_settled=!is_running and kinetic_energy=0.0,
            // producing misleading telemetry (is_settled=true while nodes still moving at
            // |v|~8.5 units/tick). Now we report the actual KE from the GPU compute loop
            // and compute is_settled against the configured equilibrium energy threshold.
            let (real_kinetic_energy, real_is_settled) = {
                let energy_threshold = physics_state
                    .params
                    .auto_pause_config
                    .equilibrium_energy_threshold;
                let force_compute_addr = state.physics.force_compute.clone();
                match force_compute_addr {
                    Some(addr) => match addr.send(GetPhysicsStats).await {
                        Ok(Ok(stats)) => {
                            let ke = stats.kinetic_energy;
                            // Settled = physics halted OR actual KE below threshold.
                            let settled = !physics_state.is_running
                                || (ke.is_finite() && ke < energy_threshold);
                            (ke, settled)
                        }
                        Ok(Err(e)) => {
                            warn!("[settlement] GetPhysicsStats returned error: {}", e);
                            (0.0, !physics_state.is_running)
                        }
                        Err(e) => {
                            warn!("[settlement] GetPhysicsStats send failed: {}", e);
                            (0.0, !physics_state.is_running)
                        }
                    },
                    None => (0.0, !physics_state.is_running),
                }
            };

            let response = GraphResponseWithPositions {
                nodes: filtered_nodes,
                edges: filtered_edges,
                metadata: graph_data.metadata.clone(),
                settlement_state: SettlementState {
                    is_settled: real_is_settled,
                    // stable_frame_count is tracked in PhysicsOrchestrator; exposing it
                    // end-to-end requires a separate query path — leave 0 until plumbed.
                    stable_frame_count: 0,
                    kinetic_energy: real_kinetic_energy,
                },
            };

            info!(
                "Sending graph data with {} nodes (CQRS query handlers)",
                response.nodes.len()
            );

            ok_json!(response)
        }
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            error!("Thread execution error: {}", e);
            Ok::<HttpResponse, actix_web::Error>(HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Internal server error"})))
        }
        (Ok(Err(e)), _, _) | (_, Ok(Err(e)), _) | (_, _, Ok(Err(e))) => {
            error!("Failed to fetch graph data (CQRS): {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to retrieve graph data"})))
        }
    }
}

pub async fn get_paginated_graph_data(
    state: web::Data<AppState>,
    query: web::Query<GraphQuery>,
) -> impl Responder {
    info!(
        "Received request for paginated graph data (CQRS Phase 1D): {:?}",
        query
    );

    let page = query.page.map(|p| p.saturating_sub(1)).unwrap_or(0);
    let page_size = query.page_size.unwrap_or(100);

    if page_size == 0 {
        error!("Invalid page size: {}", page_size);
        return bad_request!("Page size must be greater than 0");
    }

    
    let graph_handler = state.graph_query_handlers.get_graph_data.clone();
    let graph_result = execute_in_thread(move || graph_handler.handle(GetGraphData)).await;

    let graph_data_owned = match graph_result {
        Ok(Ok(g_owned)) => g_owned,
        Ok(Err(e)) => {
            error!("Failed to get graph data for pagination (CQRS): {}", e);
            return Ok(HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to retrieve graph data"})));
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            return Ok(HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Internal server error"})));
        }
    };

    let total_items = graph_data_owned.nodes.len();

    if total_items == 0 {
        debug!("Graph is empty");
        return ok_json!(PaginatedGraphResponse {
            nodes: Vec::new(),
            edges: Vec::new(),
            metadata: HashMap::new(),
            total_pages: 0,
            current_page: 1,
            total_items: 0,
            page_size,
        });
    }

    let total_pages = (total_items + page_size - 1) / page_size;

    if page >= total_pages {
        warn!(
            "Requested page {} exceeds total pages {}",
            page + 1,
            total_pages
        );
        return bad_request!("Page {} exceeds total available pages {}", page + 1, total_pages);
    }

    let start = page * page_size;
    let end = std::cmp::min(start + page_size, total_items);

    debug!(
        "Calculating slice from {} to {} out of {} total items",
        start, end, total_items
    );

    let page_nodes = graph_data_owned.nodes[start..end].to_vec();

    let node_ids: std::collections::HashSet<_> = page_nodes.iter().map(|node| node.id).collect();

    let relevant_edges: Vec<_> = graph_data_owned
        .edges
        .iter()
        .filter(|edge| node_ids.contains(&edge.source) || node_ids.contains(&edge.target))
        .cloned()
        .collect();

    debug!(
        "Found {} relevant edges for {} nodes (CQRS)",
        relevant_edges.len(),
        page_nodes.len()
    );

    let response = PaginatedGraphResponse {
        nodes: page_nodes,
        edges: relevant_edges,
        metadata: graph_data_owned.metadata.clone(),
        total_pages,
        current_page: page + 1,
        total_items,
        page_size,
    };

    ok_json!(response)
}

pub async fn refresh_graph(state: web::Data<AppState>) -> impl Responder {
    info!("Received request to refresh graph (CQRS Phase 1D)");

    
    let graph_handler = state.graph_query_handlers.get_graph_data.clone();
    let graph_result = execute_in_thread(move || graph_handler.handle(GetGraphData)).await;

    match graph_result {
        Ok(Ok(graph_data_owned)) => {
            debug!(
                "Returning current graph state with {} nodes and {} edges (CQRS)",
                graph_data_owned.nodes.len(),
                graph_data_owned.edges.len()
            );

            let response = GraphResponse {
                nodes: graph_data_owned.nodes.clone(),
                edges: graph_data_owned.edges.clone(),
                metadata: graph_data_owned.metadata.clone(),
            };

            ok_json!(serde_json::json!({
                "success": true,
                "message": "Graph data retrieved successfully",
                "data": response
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to get current graph data (CQRS): {}", e);
            error_json!("Failed to retrieve current graph data")
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn update_graph(state: web::Data<AppState>) -> impl Responder {
    info!("Received request to update graph");

    let mut metadata = match FileService::load_or_create_metadata() {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to load metadata: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": format!("Failed to load metadata: {}", e)
            })));
        }
    };

    let settings_result = state.settings_addr.send(GetSettings).await;
    let settings = match settings_result {
        Ok(Ok(s)) => Arc::new(tokio::sync::RwLock::new(s)),
        _ => {
            error!("Failed to retrieve settings for FileService in update_graph");
            return error_json!("Failed to retrieve application settings");
        }
    };

    let file_service = FileService::new(settings.clone());
    match file_service
        .fetch_and_process_files(state.content_api.clone(), settings.clone(), &mut metadata)
        .await
    {
        Ok(processed_files) => {
            if processed_files.is_empty() {
                debug!("No new files to process");
                return ok_json!(serde_json::json!({
                    "success": true,
                    "message": "No updates needed"
                }));
            }

            debug!("Processing {} new files", processed_files.len());

            {
                
                if let Err(e) = state
                    .metadata_addr
                    .send(crate::actors::messages::UpdateMetadata {
                        metadata: metadata.clone(),
                    })
                    .await
                {
                    error!("Failed to send UpdateMetadata to MetadataActor: {}", e);
                    
                }
            }

            
            match state
                .graph_service_addr
                .send(AddNodesFromMetadata { metadata })
                .await
            {
                Ok(Ok(())) => {
                    
                    debug!(
                        "Graph updated successfully via GraphServiceActor after file processing"
                    );
                    ok_json!(serde_json::json!({
                        "success": true,
                        "message": format!("Graph updated with {} new files", processed_files.len())
                    }))
                }
                Ok(Err(e)) => {
                    error!(
                        "GraphServiceActor failed to build graph from metadata: {}",
                        e
                    );
                    Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                        "success": false,
                        "error": format!("Failed to build graph: {}", e)
                    })))
                }
                Err(e) => {
                    error!("Failed to build new graph: {}", e);
                    Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                        "success": false,
                        "error": format!("Failed to build new graph: {}", e)
                    })))
                }
            }
        }
        Err(e) => {
            error!("Failed to fetch and process files: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": format!("Failed to fetch and process files: {}", e)
            })))
        }
    }
}

// Auto-balance notifications endpoint
pub async fn get_auto_balance_notifications(
    state: web::Data<AppState>,
    query: web::Query<serde_json::Value>,
) -> impl Responder {
    let since_timestamp = query.get("since").and_then(|v| v.as_i64());

    info!("Fetching auto-balance notifications (CQRS Phase 1D)");

    
    let handler = state
        .graph_query_handlers
        .get_auto_balance_notifications
        .clone();
    let query_obj = GetAutoBalanceNotifications { since_timestamp };

    let result = execute_in_thread(move || handler.handle(query_obj)).await;

    match result {
        Ok(Ok(notifications)) => ok_json!(serde_json::json!({
            "success": true,
            "notifications": notifications
        })),
        Ok(Err(e)) => {
            error!("Failed to get auto-balance notifications (CQRS): {}", e);
            error_json!("Failed to retrieve notifications")
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

/// Return the current GPU-computed node positions (not the initial Neo4j zeros).
///
/// `GET /api/graph/positions`
pub async fn get_graph_positions(
    state: web::Data<AppState>,
) -> impl Responder {
    // Acquire ForceComputeActor address
    let gpu_addr = match state.get_gpu_compute_addr().await {
        Some(addr) => addr,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "success": false,
                "error": "GPU compute actor not available"
            }));
        }
    };

    use crate::actors::messages::GetCurrentPositions;

    match gpu_addr.send(GetCurrentPositions).await {
        Ok(Ok(snapshot)) => {
            let positions: Vec<serde_json::Value> = snapshot
                .positions
                .iter()
                .map(|(id, x, y, z)| {
                    serde_json::json!({
                        "id": id,
                        "x": x,
                        "y": y,
                        "z": z
                    })
                })
                .collect();

            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "data": {
                    "positions": positions,
                    "metadata": {
                        "numNodes": snapshot.num_nodes,
                        "settled": snapshot.settled,
                        "kineticEnergy": snapshot.kinetic_energy,
                        "boundingBox": {
                            "min": {
                                "x": snapshot.bounding_box.min_x,
                                "y": snapshot.bounding_box.min_y,
                                "z": snapshot.bounding_box.min_z
                            },
                            "max": {
                                "x": snapshot.bounding_box.max_x,
                                "y": snapshot.bounding_box.max_y,
                                "z": snapshot.bounding_box.max_z
                            }
                        }
                    }
                }
            }))
        }
        Ok(Err(e)) => {
            warn!("GetCurrentPositions returned error: {}", e);
            HttpResponse::Ok().json(serde_json::json!({
                "success": false,
                "error": e
            }))
        }
        Err(e) => {
            error!("Mailbox error sending GetCurrentPositions: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": format!("Actor mailbox error: {}", e)
            }))
        }
    }
}

// Configure routes using snake_case
/// SECURITY: Graph mutation operations require authentication.
///
/// `/graph/data` is wrapped with `RequireAuth::optional()` so anonymous
/// callers see only `visibility=public` nodes while signed callers also see
/// their own `visibility=private` nodes. Other reads remain fully public
/// (rate-limited) because they expose positions/notifications, not
/// ownership-scoped content. Writes continue to require `authenticated()`.
pub fn config(cfg: &mut web::ServiceConfig) {
    use crate::middleware::{RateLimit, RateLimitConfig, RequireAuth};

    cfg.service(
        web::scope("/graph")
            .wrap(RateLimit::new(RateLimitConfig { requests_per_minute: 600, burst_size: 100, ..Default::default() }))  // 600 req/min for public reads
            // Ownership-aware read — optional auth: anonymous gets public-only,
            // signed gets public + own-private. Handler-side filter enforced.
            .service(
                web::scope("")
                    .wrap(RequireAuth::optional())
                    .route("/data", web::get().to(get_graph_data))
            )
            // Other reads are not ownership-scoped; leave public.
            .route("/data/paginated", web::get().to(get_paginated_graph_data))
            .route("/positions", web::get().to(get_graph_positions))
            .route(
                "/auto-balance-notifications",
                web::get().to(get_auto_balance_notifications),
            )
    )
    .service(
        web::scope("/graph")
            .wrap(RequireAuth::authenticated())  // Write operations require auth
            .wrap(RateLimit::new(RateLimitConfig { requests_per_minute: 60, burst_size: 10, ..Default::default() }))     // 60 req/min for writes
            .route("/update", web::post().to(update_graph))
            .route("/refresh", web::post().to(refresh_graph))
    );
}
