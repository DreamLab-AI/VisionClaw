use actix::{prelude::*, Actor, Handler, Message};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::app_state::AppState;
use crate::types::vec3::Vec3Data;
use crate::utils::binary_protocol;
use crate::utils::socket_flow_messages::{
    BinaryNodeData, BinaryNodeDataClient, PingMessage, PongMessage,
};
use crate::utils::validation::rate_limit::{
    create_rate_limit_response, extract_client_id, EndpointRateLimits, RateLimiter,
};

// Constants for throttling debug logs
const DEBUG_LOG_SAMPLE_RATE: usize = 10; 

// Default values for deadbands if not provided in settings
const DEFAULT_POSITION_DEADBAND: f32 = 0.01; 
const DEFAULT_VELOCITY_DEADBAND: f32 = 0.005; 
                                              
const BATCH_UPDATE_WINDOW_MS: u64 = 200; 

// Create a global rate limiter for WebSocket position updates
lazy_static::lazy_static! {
    static ref WEBSOCKET_RATE_LIMITER: Arc<RateLimiter> = {
        Arc::new(RateLimiter::new(EndpointRateLimits::socket_flow_updates()))
    };
}

// Note: Now using u32 node IDs throughout the system

#[derive(Clone, Debug)]
pub struct PreReadSocketSettings {
    pub min_update_rate: u32,
    pub max_update_rate: u32,
    pub motion_threshold: f32,
    pub motion_damping: f32,
    pub heartbeat_interval_ms: u64, 
    pub heartbeat_timeout_ms: u64,  
}

// Old ClientManager struct removed - now using ClientManagerActor

// Message to set client ID after registration
#[derive(Message)]
#[rtype(result = "()")]
struct SetClientId(usize);

// Implement handler for SetClientId message
impl Handler<SetClientId> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SetClientId, _ctx: &mut Self::Context) -> Self::Result {
        self.client_id = Some(msg.0);
        info!("[WebSocket] Client assigned ID: {}", msg.0);
    }
}

// Implement handler for BroadcastPositionUpdate message
impl Handler<BroadcastPositionUpdate> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: BroadcastPositionUpdate, ctx: &mut Self::Context) -> Self::Result {
        if !msg.0.is_empty() {
            
            let binary_data = binary_protocol::encode_node_data(&msg.0);

            
            ctx.binary(binary_data);

            
            if self.should_log_update() {
                trace!("[WebSocket] Position update sent: {} nodes", msg.0.len());
            }
        }
    }
}
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct BroadcastPositionUpdate(pub Vec<(u32, BinaryNodeData)>);

// Import the new messages
use crate::actors::messages::{SendToClientBinary, SendToClientText};

impl Handler<SendToClientBinary> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendToClientBinary, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

impl Handler<SendToClientText> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendToClientText, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

// Handler for initial graph load - sends all nodes and edges as JSON
use crate::actors::messages::{SendInitialGraphLoad, SendPositionUpdate};

impl Handler<SendInitialGraphLoad> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendInitialGraphLoad, ctx: &mut Self::Context) -> Self::Result {
        use crate::utils::socket_flow_messages::Message;

        let initial_load = Message::InitialGraphLoad {
            nodes: msg.nodes,
            edges: msg.edges,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };

        if let Ok(json) = serde_json::to_string(&initial_load) {
            ctx.text(json);
            if let Message::InitialGraphLoad { nodes, edges, .. } = &initial_load {
                info!("[WebSocket] Sent initial graph load: {} nodes, {} edges",
                       nodes.len(), edges.len());
            }
        } else {
            error!("[WebSocket] Failed to serialize initial graph load message");
        }
    }
}

// Handler for streamed position updates - sends individual node updates efficiently
impl Handler<SendPositionUpdate> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendPositionUpdate, ctx: &mut Self::Context) -> Self::Result {
        use crate::utils::socket_flow_messages::Message;

        let position_update = Message::PositionUpdate {
            node_id: msg.node_id,
            x: msg.x,
            y: msg.y,
            z: msg.z,
            vx: msg.vx,
            vy: msg.vy,
            vz: msg.vz,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };

        if let Ok(json) = serde_json::to_string(&position_update) {
            ctx.text(json);
            if self.should_log_update() {
                trace!("[WebSocket] Sent position update for node {}", msg.node_id);
            }
        } else {
            error!("[WebSocket] Failed to serialize position update for node {}", msg.node_id);
        }
    }
}

pub struct SocketFlowServer {
    app_state: Arc<AppState>,
    client_id: Option<usize>,
    client_manager_addr:
        actix::Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>,
    last_ping: Option<u64>,
    update_counter: usize,
    last_activity: std::time::Instant,
    heartbeat_timer_set: bool,

    _node_position_cache: HashMap<String, BinaryNodeData>,
    last_sent_positions: HashMap<String, Vec3Data>,
    last_sent_velocities: HashMap<String, Vec3Data>,
    position_deadband: f32,
    velocity_deadband: f32,

    last_transfer_size: usize,
    last_transfer_time: Instant,
    total_bytes_sent: usize,
    update_count: usize,
    nodes_sent_count: usize,


    last_batch_time: Instant,
    current_update_rate: u32,

    min_update_rate: u32,
    max_update_rate: u32,
    motion_threshold: f32,
    motion_damping: f32,


    nodes_in_motion: usize,
    total_node_count: usize,
    last_motion_check: Instant,


    client_ip: String,
    is_reconnection: bool,
    state_synced: bool,

    // Authentication state
    pubkey: Option<String>,
    is_power_user: bool,
}

impl SocketFlowServer {
    pub fn new(
        app_state: Arc<AppState>,
        pre_read_settings: PreReadSocketSettings,
        client_manager_addr: actix::Addr<
            crate::actors::client_coordinator_actor::ClientCoordinatorActor,
        >,
        client_ip: String,
    ) -> Self {
        let min_update_rate = pre_read_settings.min_update_rate;
        let max_update_rate = pre_read_settings.max_update_rate;
        let motion_threshold = pre_read_settings.motion_threshold;
        let motion_damping = pre_read_settings.motion_damping;
        
        

        
        let position_deadband = DEFAULT_POSITION_DEADBAND;
        let velocity_deadband = DEFAULT_VELOCITY_DEADBAND;

        
        let current_update_rate = max_update_rate;

        Self {
            app_state,
            client_id: None,
            client_manager_addr,
            last_ping: None,
            update_counter: 0,
            last_activity: std::time::Instant::now(),
            heartbeat_timer_set: false,
            _node_position_cache: HashMap::new(),
            last_sent_positions: HashMap::new(),
            last_sent_velocities: HashMap::new(),
            position_deadband,
            velocity_deadband,
            last_transfer_size: 0,
            last_transfer_time: Instant::now(),
            total_bytes_sent: 0,
            last_batch_time: Instant::now(),
            update_count: 0,
            nodes_sent_count: 0,
            current_update_rate,
            min_update_rate,
            max_update_rate,
            motion_threshold,
            motion_damping,


            nodes_in_motion: 0,
            total_node_count: 0,
            last_motion_check: Instant::now(),
            client_ip,
            is_reconnection: false,
            state_synced: false,
            pubkey: None,
            is_power_user: false,
        }
    }

    
    fn send_full_state_sync(&self, ctx: &mut <Self as Actor>::Context) {
        let app_state = self.app_state.clone();
        let addr = ctx.address();

        
        actix::spawn(async move {
            
            if let Ok(Ok(graph_data)) = app_state
                .graph_service_addr
                .send(crate::actors::messages::GetGraphData)
                .await
            {
                
                if let Ok(Ok(settings)) = app_state
                    .settings_addr
                    .send(crate::actors::messages::GetSettings)
                    .await
                {
                    
                    let state_sync = serde_json::json!({
                        "type": "state_sync",
                        "data": {
                            "graph": {
                                "nodes_count": graph_data.nodes.len(),
                                "edges_count": graph_data.edges.len(),
                                "metadata_count": graph_data.metadata.len(),
                            },
                            "settings": {
                                "version": settings.version,
                            },
                            "timestamp": std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        }
                    });

                    
                    if let Ok(msg_str) = serde_json::to_string(&state_sync) {
                        addr.do_send(SendToClientText(msg_str));
                        info!(
                            "Sent state sync: {} nodes, {} edges, version: {}",
                            graph_data.nodes.len(),
                            graph_data.edges.len(),
                            settings.version
                        );
                    }


                    // DEFAULT INITIAL LOAD SIZE - fresh clients receive sparse, metadata-rich dataset
                    // Client can request more nodes later using filter settings
                    const DEFAULT_INITIAL_NODE_LIMIT: usize = 200;

                    // Send new InitialGraphLoad message with LIMITED node set for fast initial render
                    if !graph_data.nodes.is_empty() || !graph_data.edges.is_empty() {
                        use crate::utils::socket_flow_messages::{InitialNodeData, InitialEdgeData};
                        use std::collections::HashSet;

                        // Take first N nodes (sorted by quality if available, otherwise by ID)
                        // This provides a sparse initial load that client can expand via filter controls
                        let mut sorted_nodes: Vec<&crate::models::node::Node> = graph_data
                            .nodes
                            .iter()
                            .collect();

                        // Sort by quality_score descending (nodes with quality first, then others)
                        sorted_nodes.sort_by(|a, b| {
                            let quality_a = graph_data.metadata.get(&a.metadata_id)
                                .and_then(|m| m.quality_score)
                                .unwrap_or(0.0);
                            let quality_b = graph_data.metadata.get(&b.metadata_id)
                                .and_then(|m| m.quality_score)
                                .unwrap_or(0.0);
                            quality_b.partial_cmp(&quality_a).unwrap_or(std::cmp::Ordering::Equal)
                        });

                        // Take limited set for initial sparse load
                        let filtered_nodes: Vec<&crate::models::node::Node> = sorted_nodes
                            .into_iter()
                            .take(DEFAULT_INITIAL_NODE_LIMIT)
                            .collect();

                        // Collect filtered node IDs for edge filtering
                        let filtered_node_ids: HashSet<u32> = filtered_nodes.iter().map(|n| n.id).collect();

                        let nodes: Vec<InitialNodeData> = filtered_nodes
                            .iter()
                            .map(|node| InitialNodeData {
                                id: node.id,
                                metadata_id: node.metadata_id.clone(),
                                label: node.label.clone(),
                                x: node.data.x,
                                y: node.data.y,
                                z: node.data.z,
                                vx: node.data.vx,
                                vy: node.data.vy,
                                vz: node.data.vz,
                                owl_class_iri: node.owl_class_iri.clone(),
                                node_type: node.node_type.clone(),
                            })
                            .collect();

                        // Only include edges where BOTH source and target are in filtered nodes
                        let edges: Vec<InitialEdgeData> = graph_data
                            .edges
                            .iter()
                            .filter(|edge| {
                                filtered_node_ids.contains(&edge.source) && filtered_node_ids.contains(&edge.target)
                            })
                            .map(|edge| InitialEdgeData {
                                id: edge.id.clone(),
                                source_id: edge.source,
                                target_id: edge.target,
                                weight: Some(edge.weight),
                                edge_type: edge.edge_type.clone(),
                            })
                            .collect();

                        addr.do_send(SendInitialGraphLoad { nodes: nodes.clone(), edges: edges.clone() });
                        info!("✅ Sent InitialGraphLoad: {} nodes (sparse from {} total), {} edges [limit: {}]",
                              nodes.len(), graph_data.nodes.len(),
                              edges.len(), DEFAULT_INITIAL_NODE_LIMIT);

                        // Also send binary position data for SAME limited nodes only
                        // Use filtered_node_ids to maintain consistency with InitialGraphLoad
                        let node_data: Vec<(u32, BinaryNodeData)> = graph_data
                            .nodes
                            .iter()
                            .filter(|node| filtered_node_ids.contains(&node.id))
                            .map(|node| {
                                (
                                    node.id,
                                    BinaryNodeData {
                                        node_id: node.id,
                                        x: node.data.x,
                                        y: node.data.y,
                                        z: node.data.z,
                                        vx: node.data.vx,
                                        vy: node.data.vy,
                                        vz: node.data.vz,
                                    },
                                )
                            })
                            .collect();

                        addr.do_send(BroadcastPositionUpdate(node_data.clone()));
                        debug!("Sent initial node positions for {} limited nodes (binary)", node_data.len());
                    }
                }
            }
        });
    }

    fn handle_ping(&mut self, msg: PingMessage) -> PongMessage {
        self.last_ping = Some(msg.timestamp);
        PongMessage {
            type_: "pong".to_string(),
            timestamp: msg.timestamp,
        }
    }

    
    fn should_log_update(&mut self) -> bool {
        self.update_counter = (self.update_counter + 1) % DEBUG_LOG_SAMPLE_RATE;
        self.update_counter == 0
    }

    
    fn has_node_changed_significantly(
        &mut self,
        node_id: &str,
        new_position: Vec3Data,
        new_velocity: Vec3Data,
    ) -> bool {
        let position_changed = if let Some(last_position) = self.last_sent_positions.get(node_id) {
            
            let dx = new_position.x - last_position.x;
            let dy = new_position.y - last_position.y;
            let dz = new_position.z - last_position.z;
            let distance_squared = dx * dx + dy * dy + dz * dz;

            
            distance_squared > self.position_deadband * self.position_deadband
        } else {
            
            true
        };

        let velocity_changed = if let Some(last_velocity) = self.last_sent_velocities.get(node_id) {
            
            let dvx = new_velocity.x - last_velocity.x;
            let dvy = new_velocity.y - last_velocity.y;
            let dvz = new_velocity.z - last_velocity.z;
            let velocity_change_squared = dvx * dvx + dvy * dvy + dvz * dvz;

            
            velocity_change_squared > self.velocity_deadband * self.velocity_deadband
        } else {
            
            true
        };

        
        if position_changed || velocity_changed {
            self.last_sent_positions
                .insert(node_id.to_string(), new_position);
            self.last_sent_velocities
                .insert(node_id.to_string(), new_velocity);
            return true;
        }

        false
    }

    
    fn get_current_update_interval(&self) -> std::time::Duration {
        let millis = (1000.0 / self.current_update_rate as f64) as u64;
        std::time::Duration::from_millis(millis)
    }

    
    fn calculate_motion_percentage(&self) -> f32 {
        if self.total_node_count == 0 {
            return 0.0;
        }

        (self.nodes_in_motion as f32) / (self.total_node_count as f32)
    }

    
    fn update_dynamic_rate(&mut self) {
        
        let now = Instant::now();
        let batch_window = std::time::Duration::from_millis(BATCH_UPDATE_WINDOW_MS);
        let elapsed = now.duration_since(self.last_batch_time);

        
        if elapsed >= batch_window {
            
            let motion_pct = self.calculate_motion_percentage();

            
            if motion_pct > self.motion_threshold {
                
                self.current_update_rate = ((self.current_update_rate as f32) * self.motion_damping
                    + (self.max_update_rate as f32) * (1.0 - self.motion_damping))
                    as u32;
            } else {
                
                self.current_update_rate = ((self.current_update_rate as f32) * self.motion_damping
                    + (self.min_update_rate as f32) * (1.0 - self.motion_damping))
                    as u32;
            }

            
            self.current_update_rate = self
                .current_update_rate
                .clamp(self.min_update_rate, self.max_update_rate);

            
            self.last_motion_check = now;
        }
    }

    
    

    
    
    

    
    
    
    
    

    
    
}

impl Actor for SocketFlowServer {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let client_ip = self.client_ip.clone();
        let cm_addr = self.client_manager_addr.clone();
        let addr = ctx.address();
        let is_reconnection = self.is_reconnection;
        let addr_clone = addr.clone();

        actix::spawn(async move {
            use crate::actors::messages::RegisterClient;
            match cm_addr.send(RegisterClient { addr: addr_clone }).await {
                Ok(Ok(id)) => {
                    addr.do_send(SetClientId(id));
                }
                Ok(Err(e)) => {
                    error!("ClientManagerActor failed to register client: {}", e);
                }
                Err(e) => {
                    error!(
                        "Failed to send RegisterClient message to ClientManagerActor: {}",
                        e
                    );
                }
            }
        });

        info!(
            "[WebSocket] {} client connected from {}",
            if is_reconnection {
                "Reconnecting"
            } else {
                "New"
            },
            client_ip
        );
        self.last_activity = std::time::Instant::now();

        // Note: client_id is assigned asynchronously via SetClientId message after RegisterClient completes
        // We don't reset it here to avoid race conditions - it will be set by the async handler

        if !self.heartbeat_timer_set {
            ctx.run_interval(std::time::Duration::from_secs(5), |act, ctx| {
                
                trace!("[WebSocket] Sending server heartbeat ping");
                ctx.ping(b"");

                
                act.last_activity = std::time::Instant::now();
            });
            self.heartbeat_timer_set = true;
        }

        
        self.send_full_state_sync(ctx);
        self.state_synced = true;


        let response = serde_json::json!({
            "type": "connection_established",
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "is_reconnection": is_reconnection,
            "state_sync_sent": true,
            "protocol": {
                "supported": [2, 3],
                "preferred": 3
            }
        });

        if let Ok(msg_str) = serde_json::to_string(&response) {
            ctx.text(msg_str);
            self.last_activity = std::time::Instant::now();
        }

        
        let loading_msg = serde_json::json!({
            "type": "loading",
            "message": if is_reconnection { "Restoring state..." } else { "Calculating initial layout..." }
        });
        ctx.text(serde_json::to_string(&loading_msg).unwrap_or_default());
        self.last_activity = std::time::Instant::now();
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        
        if let Some(client_id) = self.client_id {
            let cm_addr = self.client_manager_addr.clone();
            actix::spawn(async move {
                use crate::actors::messages::UnregisterClient;
                if let Err(e) = cm_addr.send(UnregisterClient { client_id }).await {
                    error!("Failed to unregister client from ClientManagerActor: {}", e);
                }
            });
            info!("[WebSocket] Client {} disconnected", client_id);
        }
    }
}

// Helper function to fetch nodes without borrowing from the actor
// Update signature to work with actor system
async fn fetch_nodes(
    app_state: Arc<AppState>,
    _settings_addr: actix::Addr<crate::actors::optimized_settings_actor::OptimizedSettingsActor>,
) -> Option<(Vec<(u32, BinaryNodeData)>, bool)> {
    
    use crate::actors::messages::GetGraphData;
    let graph_data = match app_state.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => {
            error!("[WebSocket] Failed to get graph data: {}", e);
            return None;
        }
        Err(e) => {
            error!(
                "[WebSocket] Failed to send message to GraphServiceActor: {}",
                e
            );
            return None;
        }
    };

    if graph_data.nodes.is_empty() {
        debug!("[WebSocket] No nodes to send! Empty graph data.");
        return None;
    }

    
    let debug_enabled = crate::utils::logging::is_debug_enabled();
    let debug_websocket = debug_enabled; 
    let detailed_debug = debug_enabled && debug_websocket;

    if detailed_debug {
        debug!(
            "Raw nodes count: {}, showing first 5 nodes IDs:",
            graph_data.nodes.len()
        );
        for (i, node) in graph_data.nodes.iter().take(5).enumerate() {
            debug!(
                "  Node {}: id={} (numeric), metadata_id={} (filename)",
                i, node.id, node.metadata_id
            );
        }
    }

    let mut nodes = Vec::with_capacity(graph_data.nodes.len());
    for node in &graph_data.nodes {
        
        
        let node_id = node.id;
        let node_data =
            BinaryNodeDataClient::new(node_id, node.data.position(), node.data.velocity());
        nodes.push((node_id, node_data));
    }

    if nodes.is_empty() {
        return None;
    }

    
    Some((nodes, detailed_debug))
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for SocketFlowServer {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                debug!("[WebSocket] Received standard ping");
                self.last_activity = std::time::Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                
                
                self.last_activity = std::time::Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                info!("Received text message: {}", text);
                self.last_activity = std::time::Instant::now();
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(msg) => {
                        match msg.get("type").and_then(|t| t.as_str()) {
                            Some("ping") => {
                                if let Ok(ping_msg) =
                                    serde_json::from_value::<PingMessage>(msg.clone())
                                {
                                    let pong = self.handle_ping(ping_msg);
                                    self.last_activity = std::time::Instant::now();
                                    if let Ok(response) = serde_json::to_string(&pong) {
                                        ctx.text(response);
                                    }
                                } else if let Some(text_ping) = msg.as_str() {
                                    if text_ping == "ping" {
                                        self.last_activity = std::time::Instant::now();
                                        ctx.text("pong");
                                    }
                                }
                            }
                            Some("update_physics_params") => {
                                warn!("Client attempted deprecated WebSocket physics update - ignoring");
                                ctx.text(r#"{"type":"error","message":"Physics updates must use REST API: POST /api/analytics/params"}"#);
                            }
                            Some("request_full_snapshot") => {
                                info!("Client requested full position snapshot");

                                
                                let include_knowledge = msg
                                    .get("graphs")
                                    .and_then(|g| g.as_array())
                                    .map_or(true, |arr| {
                                        arr.iter().any(|v| v.as_str() == Some("knowledge"))
                                    });
                                let include_agent = msg
                                    .get("graphs")
                                    .and_then(|g| g.as_array())
                                    .map_or(true, |arr| {
                                        arr.iter().any(|v| v.as_str() == Some("agent"))
                                    });


                                let fut = async move {
                                    // Log position snapshot request (GraphServiceSupervisor doesn't implement Handler<RequestPositionSnapshot>)
                                    debug!("RequestPositionSnapshot: include_knowledge={}, include_agent={}",
                                           include_knowledge, include_agent);
                                    // Return empty snapshot as GraphServiceSupervisor handler not implemented
                                    crate::actors::messages::PositionSnapshot {
                                        knowledge_nodes: Vec::new(),
                                        agent_nodes: Vec::new(),
                                        timestamp: std::time::Instant::now(),
                                    }
                                };

                                let fut = actix::fut::wrap_future::<_, Self>(fut);
                                ctx.spawn(fut.map(move |snapshot, _act, ctx| {
                                    let mut all_nodes = Vec::new();

                                    // Add knowledge nodes
                                    for (id, data) in snapshot.knowledge_nodes {
                                        all_nodes.push((
                                            binary_protocol::set_knowledge_flag(id),
                                            data,
                                        ));
                                    }

                                    // Add agent nodes
                                    for (id, data) in snapshot.agent_nodes {
                                        all_nodes.push((
                                            binary_protocol::set_agent_flag(id),
                                            data,
                                        ));
                                    }

                                    if !all_nodes.is_empty() {
                                        let binary_data =
                                            binary_protocol::encode_node_data(&all_nodes);
                                        ctx.binary(binary_data);
                                        info!(
                                            "Sent position snapshot with {} nodes",
                                            all_nodes.len()
                                        );
                                    }
                                }));
                            }
                            Some("requestInitialData") => {
                                info!("Client requested initial data - unified init flow expects REST call first");

                                
                                
                                
                                let response = serde_json::json!({
                                    "type": "initialDataInfo",
                                    "message": "Please call REST endpoint /api/graph/data first, which will trigger WebSocket sync",
                                    "flow": "unified_init",
                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                });

                                if let Ok(msg_str) = serde_json::to_string(&response) {
                                    self.last_activity = std::time::Instant::now();
                                    ctx.text(msg_str);
                                }
                            }
                            Some("enableRandomization") => {
                                if let Ok(enable_msg) =
                                    serde_json::from_value::<serde_json::Value>(msg.clone())
                                {
                                    let enabled = enable_msg
                                        .get("enabled")
                                        .and_then(|e| e.as_bool())
                                        .unwrap_or(false);
                                    info!("Client requested to {} node position randomization (server-side randomization removed)",
                                         if enabled { "enable" } else { "disable" });

                                    
                                    
                                    actix::spawn(async move {
                                        
                                        info!("Node position randomization request acknowledged, but server-side randomization is no longer supported");
                                        info!("Client-side randomization is now used instead");
                                    });
                                }
                            }
                            Some("requestBotsGraph") => {
                                info!("Client requested bots graph - returning optimized position data only");

                                
                                let graph_addr = self.app_state.graph_service_addr.clone();

                                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                                    
                                    use crate::actors::messages::GetBotsGraphData;
                                    match graph_addr.send(GetBotsGraphData).await {
                                        Ok(Ok(graph_data)) => Some(graph_data),
                                        _ => None
                                    }
                                }).map(|graph_data_opt, _act, ctx| {
                                    if let Some(graph_data) = graph_data_opt {
                                        
                                        let minimal_nodes: Vec<serde_json::Value> = graph_data.nodes.iter().map(|node| {
                                            serde_json::json!({
                                                "id": node.id,
                                                "metadata_id": node.metadata_id,
                                                "x": node.data.x,
                                                "y": node.data.y,
                                                "z": node.data.z,
                                                "vx": node.data.vx,
                                                "vy": node.data.vy,
                                                "vz": node.data.vz
                                            })
                                        }).collect();

                                        let minimal_edges: Vec<serde_json::Value> = graph_data.edges.iter().map(|edge| {
                                            serde_json::json!({
                                                "id": edge.id,
                                                "source": edge.source,
                                                "target": edge.target,
                                                "weight": edge.weight
                                            })
                                        }).collect();

                                        let response = serde_json::json!({
                                            "type": "botsGraphUpdate",
                                            "data": {
                                                "nodes": minimal_nodes,
                                                "edges": minimal_edges,
                                            },
                                            "meta": {
                                                "optimized": true,
                                                "message": "This response contains only position data. For full agent details:",
                                                "api_endpoints": {
                                                    "full_agent_data": "/api/bots/data",
                                                    "agent_status": "/api/bots/status",
                                                    "individual_agent": "/api/agents/{id}"
                                                }
                                            },
                                            "timestamp": chrono::Utc::now().timestamp_millis()
                                        });

                                        if let Ok(msg_str) = serde_json::to_string(&response) {
                                            let original_size = graph_data.nodes.len() * 500; 
                                            let optimized_size = msg_str.len();
                                            info!("Sending optimized bots graph: {} nodes, {} edges ({} bytes, est. {}% reduction)",
                                                minimal_nodes.len(), minimal_edges.len(), optimized_size,
                                                if original_size > 0 { 100 - (optimized_size * 100 / original_size) } else { 0 });
                                            ctx.text(msg_str);
                                        }
                                    } else {
                                        warn!("No bots graph data available");
                                        let response = serde_json::json!({
                                            "type": "botsGraphUpdate",
                                            "error": "No data available",
                                            "meta": {
                                                "api_endpoints": {
                                                    "full_agent_data": "/api/bots/data",
                                                    "agent_status": "/api/bots/status"
                                                }
                                            },
                                            "timestamp": chrono::Utc::now().timestamp_millis()
                                        });
                                        if let Ok(msg_str) = serde_json::to_string(&response) {
                                            ctx.text(msg_str);
                                        }
                                    }
                                }));
                            }
                            Some("requestBotsPositions") => {
                                info!("Client requested bots position updates");

                                
                                let app_state = self.app_state.clone();

                                ctx.spawn(
                                    actix::fut::wrap_future::<_, Self>(async move {
                                        
                                        let bots_nodes =
                                            crate::handlers::bots_handler::get_bots_positions(
                                                &app_state.bots_client,
                                            )
                                            .await;

                                        if bots_nodes.is_empty() {
                                            return vec![];
                                        }

                                        
                                        let mut nodes_data = Vec::new();
                                        for node in bots_nodes {
                                            let node_data = BinaryNodeData {
                                                node_id: node.id,
                                                x: node.data.x,
                                                y: node.data.y,
                                                z: node.data.z,
                                                vx: node.data.vx,
                                                vy: node.data.vy,
                                                vz: node.data.vz,
                                            };
                                            nodes_data.push((node.id, node_data));
                                        }

                                        nodes_data
                                    })
                                    .map(
                                        |nodes_data, _act, ctx| {
                                            if !nodes_data.is_empty() {
                                                
                                                let binary_data =
                                                    binary_protocol::encode_node_data(&nodes_data);

                                                info!(
                                                    "Sending bots positions: {} nodes, {} bytes",
                                                    nodes_data.len(),
                                                    binary_data.len()
                                                );

                                                ctx.binary(binary_data);
                                            }
                                        },
                                    ),
                                );

                                
                                let response = serde_json::json!({
                                    "type": "botsUpdatesStarted",
                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                });
                                if let Ok(msg_str) = serde_json::to_string(&response) {
                                    ctx.text(msg_str);
                                }
                            }
                            Some("subscribe_position_updates") => {
                                info!("Client requested position update subscription");

                                
                                let interval = msg
                                    .get("data")
                                    .and_then(|data| data.get("interval"))
                                    .and_then(|interval| interval.as_u64())
                                    .unwrap_or(60); 

                                let binary = msg
                                    .get("data")
                                    .and_then(|data| data.get("binary"))
                                    .and_then(|binary| binary.as_bool())
                                    .unwrap_or(true); 

                                
                                let min_allowed_interval = 1000
                                    / (EndpointRateLimits::socket_flow_updates()
                                        .requests_per_minute
                                        / 60);
                                let actual_interval = interval.max(min_allowed_interval as u64);

                                if actual_interval != interval {
                                    info!("Adjusted position update interval from {}ms to {}ms to comply with rate limits",
                                        interval, actual_interval);
                                }

                                info!(
                                    "Starting position updates with interval: {}ms, binary: {}",
                                    actual_interval, binary
                                );

                                
                                let update_interval =
                                    std::time::Duration::from_millis(actual_interval);
                                let app_state = self.app_state.clone();
                                let settings_addr = self.app_state.settings_addr.clone();

                                
                                let response = serde_json::json!({
                                    "type": "subscription_confirmed",
                                    "subscription": "position_updates",
                                    "interval": actual_interval,
                                    "binary": binary,
                                    "timestamp": chrono::Utc::now().timestamp_millis(),
                                    "rate_limit": {
                                        "requests_per_minute": EndpointRateLimits::socket_flow_updates().requests_per_minute,
                                        "min_interval_ms": min_allowed_interval
                                    }
                                });
                                if let Ok(msg_str) = serde_json::to_string(&response) {
                                    ctx.text(msg_str);
                                }

                                
                                ctx.run_later(update_interval, move |_act, ctx| {
                                    
                                    let fut = fetch_nodes(app_state.clone(), settings_addr.clone());
                                    let fut = actix::fut::wrap_future::<_, Self>(fut);

                                    ctx.spawn(fut.map(move |result, act, ctx| {
                                        if let Some((nodes, detailed_debug)) = result {
                                            
                                            let mut filtered_nodes = Vec::new();
                                            for (node_id, node_data) in &nodes {
                                                let node_id_str = node_id.to_string();
                                                let position = node_data.position();
                                                let velocity = node_data.velocity();

                                                
                                                if act.has_node_changed_significantly(
                                                    &node_id_str,
                                                    position.clone(),
                                                    velocity.clone()
                                                ) {
                                                    filtered_nodes.push((*node_id, node_data.clone()));
                                                }
                                            }

                                            
                                            if !filtered_nodes.is_empty() {
                                                
                                                let binary_data = binary_protocol::encode_node_data(&filtered_nodes);

                                                
                                                act.total_node_count = filtered_nodes.len();
                                                let moving_nodes = filtered_nodes.iter()
                                                    .filter(|(_, node_data)| {
                                                        let vel = node_data.velocity();
                                                        vel.x.abs() > 0.001 || vel.y.abs() > 0.001 || vel.z.abs() > 0.001
                                                    })
                                                    .count();
                                                act.nodes_in_motion = moving_nodes;

                                                
                                                act.last_transfer_size = binary_data.len();
                                                act.total_bytes_sent += binary_data.len();
                                                act.update_count += 1;
                                                act.nodes_sent_count += filtered_nodes.len();

                                                if detailed_debug {
                                                    debug!("[Position Updates] Sending {} nodes, {} bytes",
                                                           filtered_nodes.len(), binary_data.len());
                                                }

                                                ctx.binary(binary_data);
                                            }

                                            
                                            let next_interval = std::time::Duration::from_millis(actual_interval);
                                            ctx.run_later(next_interval, move |act, ctx| {
                                                
                                                let subscription_msg = format!(
                                                    "{{\"type\":\"subscribe_position_updates\",\"data\":{{\"interval\":{},\"binary\":{}}}}}",
                                                    actual_interval, binary
                                                );
                                                <SocketFlowServer as StreamHandler<Result<ws::Message, ws::ProtocolError>>>::handle(
                                                    act,
                                                    Ok(ws::Message::Text(subscription_msg.into())),
                                                    ctx
                                                );
                                            });
                                        }
                                    }));
                                });
                            }
                            Some("requestPositionUpdates") => {
                                info!("Client requested position updates (legacy format)");
                                
                                let subscription_msg = r#"{"type":"subscribe_position_updates","data":{"interval":60,"binary":true}}"#;
                                <SocketFlowServer as StreamHandler<
                                    Result<ws::Message, ws::ProtocolError>,
                                >>::handle(
                                    self,
                                    Ok(ws::Message::Text(subscription_msg.to_string().into())),
                                    ctx,
                                );
                            }
                            Some("authenticate") => {
                                info!("Client sent authenticate message");

                                let token = msg.get("token").and_then(|t| t.as_str()).map(String::from);
                                let pubkey = msg.get("pubkey").and_then(|p| p.as_str()).map(String::from);

                                if let (Some(token), Some(pubkey)) = (token, pubkey) {
                                    let nostr_service = self.app_state.nostr_service.clone();
                                    let client_id = self.client_id;
                                    let cm_addr = self.client_manager_addr.clone();

                                    // Validate and load user
                                    ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                                        if let Some(ref ns) = nostr_service {
                                            if let Some(user) = ns.get_session(&token).await {
                                                if user.pubkey == pubkey {
                                                    return (Some(user), client_id, cm_addr);
                                                }
                                            }
                                        }
                                        (None, client_id, cm_addr)
                                    }).map(|(user_opt, client_id, cm_addr), act, ctx| {
                                        if let Some(user) = user_opt {
                                            // Update local state
                                            act.pubkey = Some(user.pubkey.clone());
                                            act.is_power_user = user.is_power_user;

                                            // Update ClientCoordinator state
                                            if let Some(cid) = client_id {
                                                use crate::actors::messages::AuthenticateClient;
                                                cm_addr.do_send(AuthenticateClient {
                                                    client_id: cid,
                                                    pubkey: user.pubkey.clone(),
                                                    is_power_user: user.is_power_user,
                                                });
                                            }

                                            // Send confirmation
                                            let response = serde_json::json!({
                                                "type": "authenticate_success",
                                                "pubkey": user.pubkey,
                                                "is_power_user": user.is_power_user,
                                                "timestamp": chrono::Utc::now().timestamp_millis()
                                            });
                                            if let Ok(msg_str) = serde_json::to_string(&response) {
                                                ctx.text(msg_str);
                                            }

                                            info!("Client authenticated: pubkey={}, power_user={}", user.pubkey, user.is_power_user);
                                        } else {
                                            let error_msg = serde_json::json!({
                                                "type": "error",
                                                "message": "Authentication failed: invalid token or pubkey mismatch"
                                            });
                                            if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                                                ctx.text(msg_str);
                                            }
                                            warn!("Authentication failed for client");
                                        }
                                    }));
                                } else {
                                    let error_msg = serde_json::json!({
                                        "type": "error",
                                        "message": "Authentication requires both 'token' and 'pubkey'"
                                    });
                                    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                                        ctx.text(msg_str);
                                    }
                                }
                            }
                            Some("filter_update") => {
                                info!("Client sent filter_update message");

                                if let Some(client_id) = self.client_id {
                                    // Check both nested "filter" key and "data" key (client sends in data)
                                    let filter_data = msg.get("filter")
                                        .or_else(|| msg.get("data"))
                                        .unwrap_or(&msg);

                                    use crate::actors::messages::UpdateClientFilter;
                                    let update = UpdateClientFilter {
                                        client_id,
                                        enabled: filter_data.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                                        quality_threshold: filter_data.get("quality_threshold").and_then(|v| v.as_f64()).unwrap_or(0.7),
                                        authority_threshold: filter_data.get("authority_threshold").and_then(|v| v.as_f64()).unwrap_or(0.5),
                                        filter_by_quality: filter_data.get("filter_by_quality").and_then(|v| v.as_bool()).unwrap_or(true),
                                        filter_by_authority: filter_data.get("filter_by_authority").and_then(|v| v.as_bool()).unwrap_or(false),
                                        filter_mode: filter_data.get("filter_mode").and_then(|v| v.as_str()).unwrap_or("or").to_string(),
                                        max_nodes: filter_data.get("max_nodes").and_then(|v| v.as_i64()).map(|n| n as i32),
                                    };

                                    info!("Processing filter update: enabled={}, quality_threshold={}, filter_by_quality={}",
                                          update.enabled, update.quality_threshold, update.filter_by_quality);

                                    let cm_addr = self.client_manager_addr.clone();
                                    let pubkey = self.pubkey.clone();

                                    // Send to ClientCoordinator
                                    ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                                        match cm_addr.send(update.clone()).await {
                                            Ok(Ok(())) => {
                                                (true, pubkey, update)
                                            }
                                            Ok(Err(e)) => {
                                                error!("Failed to update client filter: {}", e);
                                                (false, pubkey, update)
                                            }
                                            Err(e) => {
                                                error!("Failed to send filter update: {}", e);
                                                (false, pubkey, update)
                                            }
                                        }
                                    }).map(|(success, pubkey_opt, update), act, ctx| {
                                        if success {
                                            // Also persist to Neo4j if authenticated
                                            if let Some(pubkey) = pubkey_opt {
                                                info!("Filter updated for pubkey {}: enabled={}, max_nodes={:?}",
                                                      pubkey, update.enabled, update.max_nodes);

                                                // Save to Neo4j asynchronously
                                                let neo4j_repo = act.app_state.neo4j_settings_repository.clone();
                                                let pubkey_clone = pubkey.clone();
                                                use crate::adapters::neo4j_settings_repository::UserFilter;
                                                let filter = UserFilter {
                                                    pubkey: pubkey_clone.clone(),
                                                    enabled: update.enabled,
                                                    quality_threshold: update.quality_threshold,
                                                    authority_threshold: update.authority_threshold,
                                                    filter_by_quality: update.filter_by_quality,
                                                    filter_by_authority: update.filter_by_authority,
                                                    filter_mode: update.filter_mode.clone(),
                                                    max_nodes: update.max_nodes,
                                                    updated_at: chrono::Utc::now(),
                                                };

                                                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                                                    match neo4j_repo.save_user_filter(&pubkey_clone, &filter).await {
                                                        Ok(()) => {
                                                            info!("✅ Filter persisted to Neo4j for pubkey: {}", pubkey_clone);
                                                        }
                                                        Err(e) => {
                                                            error!("❌ Failed to persist filter to Neo4j: {}", e);
                                                        }
                                                    }
                                                }).map(|_result, _act, _ctx| ()));
                                            }

                                            let response = serde_json::json!({
                                                "type": "filter_update_success",
                                                "enabled": update.enabled,
                                                "timestamp": chrono::Utc::now().timestamp_millis()
                                            });
                                            if let Ok(msg_str) = serde_json::to_string(&response) {
                                                ctx.text(msg_str);
                                            }
                                        } else {
                                            let error_msg = serde_json::json!({
                                                "type": "error",
                                                "message": "Failed to update filter"
                                            });
                                            if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                                                ctx.text(msg_str);
                                            }
                                        }
                                    }));
                                } else {
                                    // Client ID not yet assigned - registration still in progress
                                    warn!("filter_update received but client_id not yet assigned - registration may still be in progress");
                                    let error_msg = serde_json::json!({
                                        "type": "error",
                                        "message": "Client registration in progress, please retry filter update in a moment"
                                    });
                                    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                                        ctx.text(msg_str);
                                    }
                                }
                            }
                            Some("requestSwarmTelemetry") => {
                                info!("Client requested enhanced swarm telemetry");

                                let app_state = self.app_state.clone();

                                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                                    
                                    match crate::handlers::bots_handler::fetch_hive_mind_agents(&app_state, None).await {
                                        Ok(agents) => {
                                            let mut nodes_data = Vec::new();
                                            let mut swarm_metrics = serde_json::json!({
                                                "total_agents": agents.len(),
                                                "active_agents": 0,
                                                "avg_health": 0.0,
                                                "avg_cpu": 0.0,
                                                "avg_workload": 0.0,
                                                "total_tokens": 0,
                                                "swarm_ids": std::collections::HashSet::<String>::new(),
                                            });

                                            let mut active_count = 0;
                                            let mut total_health = 0.0;
                                            let mut total_cpu = 0.0;
                                            let mut total_workload = 0.0;
                                            let total_tokens = 0;
                                            let swarm_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

                                            for (idx, agent) in agents.iter().enumerate() {
                                                if agent.status == "active" {
                                                    active_count += 1;
                                                }
                                                total_health += agent.health;
                                                total_cpu += agent.cpu_usage;
                                                total_workload += agent.workload;
                                                
                                                
                                                
                                                
                                                

                                                
                                                let position = Vec3Data::new(
                                                    (idx as f32 * 100.0).sin() * 500.0,
                                                    (idx as f32 * 100.0).cos() * 500.0,
                                                    0.0
                                                );

                                                let node_data = BinaryNodeData {
                                                    node_id: (1000 + idx) as u32,
                                                    x: position.x,
                                                    y: position.y,
                                                    z: position.z,
                                                    vx: 0.0,
                                                    vy: 0.0,
                                                    vz: 0.0,
                                                };
                                                nodes_data.push(((1000 + idx) as u32, node_data));
                                            }

                                            
                                            if !agents.is_empty() {
                                                swarm_metrics["active_agents"] = serde_json::json!(active_count);
                                                swarm_metrics["avg_health"] = serde_json::json!(total_health / agents.len() as f32);
                                                swarm_metrics["avg_cpu"] = serde_json::json!(total_cpu / agents.len() as f32);
                                                swarm_metrics["avg_workload"] = serde_json::json!(total_workload / agents.len() as f32);
                                                swarm_metrics["total_tokens"] = serde_json::json!(total_tokens);
                                                swarm_metrics["swarm_count"] = serde_json::json!(swarm_ids.len());
                                            }

                                            (nodes_data, swarm_metrics)
                                        }
                                        Err(_) => (vec![], serde_json::json!({}))
                                    }
                                }).map(|(nodes_data, swarm_metrics), _act, ctx| {
                                    
                                    if !nodes_data.is_empty() {
                                        let binary_data = binary_protocol::encode_node_data(&nodes_data);
                                        ctx.binary(binary_data);
                                    }

                                    
                                    let telemetry_response = serde_json::json!({
                                        "type": "swarmTelemetry",
                                        "timestamp": chrono::Utc::now().timestamp_millis(),
                                        "data_source": "live",
                                        "metrics": swarm_metrics,
                                        "node_count": nodes_data.len()
                                    });

                                    if let Ok(msg_str) = serde_json::to_string(&telemetry_response) {
                                        ctx.text(msg_str);
                                    }
                                }));
                            }
                            // Ontology validation and constraint messages
                            Some("ontology_validation") | Some("ontology_validation_report") => {
                                info!("[WebSocket] Ontology validation request received");
                                let ontology_id = msg.get("ontologyId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("default")
                                    .to_string();

                                if let Some(ref ontology_addr) = self.app_state.ontology_actor_addr {
                                    let addr = ontology_addr.clone();
                                    let fut = async move {
                                        use crate::actors::messages::GetOntologyReport;
                                        match addr.send(GetOntologyReport { report_id: Some(ontology_id.clone()) }).await {
                                            Ok(Ok(Some(report))) => {
                                                serde_json::json!({
                                                    "type": "ontology_validation_update",
                                                    "ontologyId": ontology_id,
                                                    "status": "completed",
                                                    "violations": report.violations.len(),
                                                    "inferredTriples": report.inferred_triples.len(),
                                                    "constraints": report.constraint_summary.total_constraints,
                                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                                })
                                            }
                                            Ok(Ok(None)) => {
                                                serde_json::json!({
                                                    "type": "ontology_validation_update",
                                                    "ontologyId": ontology_id,
                                                    "status": "not_found",
                                                    "message": "No validation report available. Run validation first.",
                                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                                })
                                            }
                                            _ => {
                                                serde_json::json!({
                                                    "type": "ontology_validation_update",
                                                    "status": "error",
                                                    "message": "Failed to retrieve validation report",
                                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                                })
                                            }
                                        }
                                    };

                                    let fut = actix::fut::wrap_future::<_, Self>(fut);
                                    ctx.spawn(fut.map(|response, _act, ctx| {
                                        if let Ok(msg_str) = serde_json::to_string(&response) {
                                            ctx.text(msg_str);
                                        }
                                    }));
                                } else {
                                    let response = serde_json::json!({
                                        "type": "ontology_validation_update",
                                        "status": "unavailable",
                                        "message": "Ontology system not initialized"
                                    });
                                    if let Ok(msg_str) = serde_json::to_string(&response) {
                                        ctx.text(msg_str);
                                    }
                                }
                            }
                            Some("ontology_constraint_update") | Some("ontology_constraint_toggle") => {
                                info!("[WebSocket] Ontology constraint update request");
                                let response = serde_json::json!({
                                    "type": "ontology_constraint_update",
                                    "status": "acknowledged",
                                    "message": "Use REST API /api/ontology-physics/enable for constraint management",
                                    "timestamp": chrono::Utc::now().timestamp_millis()
                                });
                                if let Ok(msg_str) = serde_json::to_string(&response) {
                                    ctx.text(msg_str);
                                }
                            }
                            Some("ontology_reasoning") => {
                                info!("[WebSocket] Ontology reasoning request received");
                                if let Some(ref ontology_addr) = self.app_state.ontology_actor_addr {
                                    let addr = ontology_addr.clone();
                                    let ontology_id = msg.get("ontologyId")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let source = msg.get("source")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("websocket")
                                        .to_string();

                                    let fut = async move {
                                        use crate::actors::ontology_actor::TriggerReasoning;
                                        match addr.send(TriggerReasoning { ontology_id, source }).await {
                                            Ok(Ok(job_id)) => serde_json::json!({
                                                "type": "ontology_reasoning_started",
                                                "jobId": job_id,
                                                "timestamp": chrono::Utc::now().timestamp_millis()
                                            }),
                                            _ => serde_json::json!({
                                                "type": "ontology_reasoning_error",
                                                "message": "Failed to trigger reasoning",
                                                "timestamp": chrono::Utc::now().timestamp_millis()
                                            })
                                        }
                                    };
                                    let fut = actix::fut::wrap_future::<_, Self>(fut);
                                    ctx.spawn(fut.map(|response, _act, ctx| {
                                        if let Ok(msg_str) = serde_json::to_string(&response) {
                                            ctx.text(msg_str);
                                        }
                                    }));
                                }
                            }
                            _ => {
                                warn!("[WebSocket] Unknown message type: {:?}", msg);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("[WebSocket] Failed to parse text message: {}", e);
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "message": format!("Failed to parse text message: {}", e)
                        });
                        if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                            ctx.text(msg_str);
                        }
                    }
                }
            }
            Ok(ws::Message::Binary(data)) => {
                
                if !WEBSOCKET_RATE_LIMITER.is_allowed(&self.client_ip) {
                    warn!(
                        "Position update rate limit exceeded for client: {}",
                        self.client_ip
                    );
                    let error_msg = serde_json::json!({
                        "type": "rate_limit_warning",
                        "message": "Update rate too high, some updates may be dropped",
                        "retry_after": WEBSOCKET_RATE_LIMITER.reset_time(&self.client_ip).as_secs()
                    });
                    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                        ctx.text(msg_str);
                    }
                    
                    return;
                }

                
                info!("Received binary message, length: {}", data.len());
                self.last_activity = std::time::Instant::now();

                
                use crate::utils::binary_protocol::{BinaryProtocol, Message as ProtocolMessage};

                match BinaryProtocol::decode_message(&data) {
                    Ok(ProtocolMessage::VoiceData { audio }) => {
                        info!("Received voice data: {} bytes", audio.len());


                        let response = serde_json::json!({
                            "type": "voice_ack",
                            "bytes": audio.len(),
                            "message": "Voice data received but not yet processed"
                        });
                        if let Ok(msg_str) = serde_json::to_string(&response) {
                            ctx.text(msg_str);
                        }
                        return;
                    }
                    Ok(ProtocolMessage::BroadcastAck { sequence_id, nodes_received, timestamp }) => {
                        // True end-to-end backpressure: client confirms receipt of position broadcast
                        // Forward to ClientCoordinatorActor which routes to GPU for token restoration
                        use crate::actors::messages::ClientBroadcastAck;

                        trace!(
                            "Received BroadcastAck: seq={}, nodes={}, timestamp={}",
                            sequence_id, nodes_received, timestamp
                        );

                        // Send ACK to client coordinator for forwarding to GPU actor
                        self.client_manager_addr.do_send(ClientBroadcastAck {
                            sequence_id,
                            nodes_received,
                            timestamp,
                            client_id: self.client_id,
                        });
                        return;
                    }
                    Err(e) => {

                        debug!("New protocol decode failed ({}), trying legacy protocol", e);
                    }
                }

                
                
                
                

                match binary_protocol::decode_node_data(&data) {
                    Ok(nodes) => {
                        info!("Decoded {} nodes from binary message", nodes.len());
                        let _nodes_vec: Vec<_> = nodes.clone().into_iter().collect();

                        
                        
                        {
                            let app_state = self.app_state.clone();
                            let nodes_vec: Vec<_> = nodes.clone().into_iter().collect();

                            let fut = async move {
                                for (node_id, node_data) in &nodes_vec {
                                    
                                    if *node_id < 5 {
                                        debug!(
                                            "Processing binary update for node ID: {} with position [{:.3}, {:.3}, {:.3}]",
                                            node_id, node_data.x, node_data.y, node_data.z
                                        );
                                    }
                                }

                                
                                
                                
                                
                                
                                
                                
                                
                                
                                
                                
                                
                                

                                
                                info!("Received {} node positions from client (feedback loop disabled)", nodes_vec.len());

                                info!("Updated node positions from binary data (preserving server-side properties)");

                                
                                info!("Preparing to recalculate layout after client-side node position update");

                                
                                use crate::actors::messages::GetSettingByPath;
                                let settings_addr = app_state.settings_addr.clone();

                                
                                if let Ok(Ok(_iterations_val)) = settings_addr
                                    .send(GetSettingByPath {
                                        path: "visualisation.graphs.logseq.physics.iterations"
                                            .to_string(),
                                    })
                                    .await
                                {
                                    if let Ok(Ok(_spring_val)) = settings_addr
                                        .send(GetSettingByPath {
                                            path: "visualisation.graphs.logseq.physics.spring_k"
                                                .to_string(),
                                        })
                                        .await
                                    {
                                        if let Ok(Ok(_repulsion_val)) = settings_addr
                                            .send(GetSettingByPath {
                                                path: "visualisation.graphs.logseq.physics.repel_k"
                                                    .to_string(),
                                            })
                                            .await
                                        {
                                            
                                            use crate::actors::messages::SimulationStep;
                                            if let Err(e) = app_state
                                                .graph_service_addr
                                                .send(SimulationStep)
                                                .await
                                            {
                                                error!("Failed to trigger simulation step: {}", e);
                                            } else {
                                                info!(
                                                    "Successfully triggered layout recalculation"
                                                );
                                            }
                                        }
                                    }
                                }
                            };

                            let fut = fut.into_actor(self);
                            ctx.spawn(fut.map(|_, _, _| ()));
                        }
                    }
                    Err(e) => {
                        error!("Failed to decode binary message: {}", e);
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "message": format!("Failed to decode binary message: {}", e),
                            "recoverable": true,
                            "details": {
                                "data_length": data.len(),
                                "expected_item_size": 26,
                                "remainder": data.len() % 26
                            }
                        });
                        if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                            ctx.text(msg_str);
                        }
                        
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                info!("[WebSocket] Client initiated close: {:?}", reason);
                ctx.close(reason); 
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                warn!("[WebSocket] Received unexpected continuation frame");
            }
            Ok(ws::Message::Nop) => {
                debug!("[WebSocket] Received Nop");
            }
            Err(e) => {
                error!("[WebSocket] Error in WebSocket connection: {}", e);
                
                let error_msg = serde_json::json!({
                    "type": "error",
                    "message": format!("WebSocket error: {}", e),
                    "recoverable": true
                });
                if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                    ctx.text(msg_str);
                }
                
            }
        }
    }
}

pub async fn socket_flow_handler(
    req: HttpRequest,
    stream: web::Payload,
    app_state_data: web::Data<AppState>, 
    pre_read_ws_settings: web::Data<PreReadSocketSettings>, 
) -> Result<HttpResponse, actix_web::Error> {
    
    let client_ip = extract_client_id(&req);


    if !WEBSOCKET_RATE_LIMITER.is_allowed(&client_ip) {
        warn!("WebSocket rate limit exceeded for client: {}", client_ip);
        return create_rate_limit_response(&client_ip, &WEBSOCKET_RATE_LIMITER);
    }

    // SECURITY: Validate Origin header to prevent cross-site WebSocket hijacking
    if let Some(origin_header) = req.headers().get("Origin") {
        let origin = origin_header.to_str().unwrap_or("");
        let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| {
                if std::env::var("ALLOW_INSECURE_DEFAULTS").is_ok() {
                    "http://localhost:3000,http://localhost:3001,http://127.0.0.1:3000,http://localhost:5173".to_string()
                } else {
                    "http://localhost:3000".to_string()
                }
            });

        let is_allowed = allowed_origins.split(',')
            .map(|s| s.trim())
            .any(|allowed| allowed == origin);

        if !is_allowed {
            warn!("WebSocket connection rejected - invalid origin: {} (allowed: {})", origin, allowed_origins);
            return Ok(HttpResponse::Forbidden()
                .body(format!("Origin '{}' not allowed for WebSocket connections", origin)));
        }
    } else if std::env::var("ALLOW_INSECURE_DEFAULTS").is_err() {
        // In production, require Origin header for WebSocket connections
        warn!("WebSocket connection rejected - missing Origin header from {}", client_ip);
        return Ok(HttpResponse::BadRequest()
            .body("Origin header required for WebSocket connections"));
    }

    let app_state_arc = app_state_data.into_inner(); 

    
    let client_manager_addr = app_state_arc.client_manager_addr.clone();

    
    use crate::actors::messages::GetSettingByPath;
    let settings_addr = app_state_arc.settings_addr.clone();

    let debug_enabled = match settings_addr
        .send(GetSettingByPath {
            path: "system.debug.enabled".to_string(),
        })
        .await
    {
        Ok(Ok(value)) => value.as_bool().unwrap_or(false),
        _ => false,
    };
    let debug_websocket = match settings_addr
        .send(GetSettingByPath {
            path: "system.debug.enable_websocket_debug".to_string(),
        })
        .await
    {
        Ok(Ok(value)) => value.as_bool().unwrap_or(false),
        _ => false,
    };
    let should_debug = debug_enabled && debug_websocket;

    if should_debug {
        debug!("WebSocket connection attempt from {:?}", req.peer_addr());
    }

    
    if !req.headers().contains_key("Upgrade") {
        return Ok(HttpResponse::BadRequest().body("WebSocket upgrade required"));
    }

    let is_reconnection = req
        .headers()
        .get("X-Client-Session")
        .and_then(|h| h.to_str().ok())
        .is_some();

    // Extract token from query string for authentication
    let token_from_qs = req.query_string()
        .split('&')
        .find_map(|param| {
            let parts: Vec<&str> = param.split('=').collect();
            if parts.len() == 2 && parts[0] == "token" {
                Some(parts[1].to_string())
            } else {
                None
            }
        });

    let mut ws = SocketFlowServer::new(
        app_state_arc.clone(),
        pre_read_ws_settings.get_ref().clone(),
        client_manager_addr,
        client_ip.clone(),
    );

    ws.is_reconnection = is_reconnection;

    // Try to authenticate from query string token
    if let Some(token) = token_from_qs {
        if let Some(ref nostr_service) = app_state_arc.nostr_service {
            if let Some(user) = nostr_service.get_session(&token).await {
                ws.pubkey = Some(user.pubkey.clone());
                ws.is_power_user = user.is_power_user;
                info!("Pre-authenticated WebSocket client via query string: pubkey={}", user.pubkey);
            }
        }
    }

    
    
    match ws::WsResponseBuilder::new(ws, &req, stream)
        .protocols(&["permessage-deflate"])
        .start()
    {
        Ok(response) => {
            info!(
                "[WebSocket] Client {} connected successfully with compression support",
                client_ip
            );
            Ok(response)
        }
        Err(e) => {
            error!(
                "[WebSocket] Failed to start WebSocket for client {}: {}",
                client_ip, e
            );
            Err(e)
        }
    }
}
