use crate::utils::json::{from_json, to_json};
use crate::utils::time;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebSocketEnvelope<T> {
    pub message_type: String,
    pub payload: T,
    pub timestamp: DateTime<Utc>,
    pub client_id: Option<String>,
    pub request_id: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum StandardWebSocketMessage {
    #[serde(rename = "ping")]
    Ping {
        timestamp: DateTime<Utc>,
        client_id: Option<String>,
    },

    #[serde(rename = "pong")]
    Pong {
        timestamp: DateTime<Utc>,
        client_id: Option<String>,
    },

    #[serde(rename = "connection_established")]
    ConnectionEstablished {
        client_id: String,
        timestamp: DateTime<Utc>,
        capabilities: Vec<String>,
    },

    #[serde(rename = "connection_closed")]
    ConnectionClosed {
        client_id: String,
        reason: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "subscribe")]
    Subscribe {
        channels: Vec<String>,
        client_id: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        channels: Vec<String>,
        client_id: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "subscription_ack")]
    SubscriptionAck {
        channels: Vec<String>,
        client_id: String,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "data_update")]
    DataUpdate {
        channel: String,
        data: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "status_update")]
    StatusUpdate {
        channel: String,
        status: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "error")]
    Error {
        error_type: String,
        message: String,
        details: Option<serde_json::Value>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "request")]
    Request {
        request_id: String,
        method: String,
        params: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "response")]
    Response {
        request_id: String,
        success: bool,
        data: Option<serde_json::Value>,
        error: Option<String>,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum HealthWebSocketMessage {
    #[serde(rename = "health_status")]
    HealthStatus {
        status: String,
        components: Vec<ComponentStatus>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "performance_metrics")]
    PerformanceMetrics {
        metrics: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "alert")]
    Alert {
        severity: String,
        message: String,
        component: Option<String>,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComponentStatus {
    pub name: String,
    pub status: String,
    pub details: Option<String>,
    pub metrics: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum McpWebSocketMessage {
    #[serde(rename = "mcp_command")]
    McpCommand {
        command: String,
        params: serde_json::Value,
        request_id: String,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "mcp_response")]
    McpResponse {
        request_id: String,
        success: bool,
        result: Option<serde_json::Value>,
        error: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "agent_status")]
    AgentStatus {
        agent_id: String,
        status: String,
        details: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "swarm_update")]
    SwarmUpdate {
        swarm_id: String,
        status: String,
        agents: Vec<serde_json::Value>,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SpeechWebSocketMessage {
    #[serde(rename = "audio_data")]
    AudioData {
        format: String,
        sample_rate: u32,
        channels: u8,
        data: Vec<u8>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "speech_recognition")]
    SpeechRecognition {
        text: String,
        confidence: f32,
        language: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "speech_synthesis")]
    SpeechSynthesis {
        text: String,
        voice: Option<String>,
        format: String,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum BotsWebSocketMessage {
    #[serde(rename = "node_update")]
    NodeUpdate {
        node_id: String,
        position: [f32; 3],
        properties: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "edge_update")]
    EdgeUpdate {
        edge_id: String,
        from_node: String,
        to_node: String,
        properties: serde_json::Value,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "graph_state")]
    GraphState {
        nodes: Vec<serde_json::Value>,
        edges: Vec<serde_json::Value>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "telemetry")]
    Telemetry {
        fps: f32,
        node_count: u32,
        edge_count: u32,
        gpu_usage: Option<f32>,
        memory_usage: Option<f32>,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum OntologyWebSocketMessage {
    #[serde(rename = "ontology_load")]
    LoadOntology {
        graph_id: String,
        source: OntologySource,
        mapping_config: Option<String>,
        physics_config: Option<OntologyPhysicsConfig>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_validation")]
    ValidateGraph {
        graph_id: String,
        status: ValidationStatus,
        consistency: bool,
        violations: Vec<ValidationViolation>,
        metrics: Option<OntologyMetrics>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_constraint_update")]
    ApplyConstraints {
        graph_id: String,
        constraints: Vec<serde_json::Value>,
        enable_gpu: bool,
        convergence_threshold: f64,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_constraint_toggle")]
    ToggleConstraintGroup {
        graph_id: String,
        group_name: String,
        enabled: bool,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_validation_report")]
    GetValidationReport {
        graph_id: String,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_reasoning")]
    ReasoningRequest {
        graph_id: String,
        reasoner: String,
        inference_level: String,
        materialize_inferences: bool,
        timeout_ms: u64,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ontology_query")]
    Query {
        graph_id: String,
        query_type: String,
        subject_uri: String,
        include_inferred: bool,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OntologySource {
    pub format: String,
    pub uri: Option<String>,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OntologyPhysicsConfig {
    pub enable_constraints: bool,
    pub constraint_groups: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    Processing,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidationViolation {
    pub violation_type: String,
    pub severity: String,
    pub nodes: Vec<u32>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OntologyMetrics {
    pub class_count: u32,
    pub property_count: u32,
    pub axiom_count: u32,
    pub reasoning_time_ms: u64,
}

pub trait ToStandardMessage {
    fn to_standard_envelope(
        &self,
        client_id: Option<String>,
    ) -> WebSocketEnvelope<serde_json::Value>;
}

impl ToStandardMessage for StandardWebSocketMessage {
    fn to_standard_envelope(
        &self,
        client_id: Option<String>,
    ) -> WebSocketEnvelope<serde_json::Value> {
        WebSocketEnvelope {
            message_type: match self {
                StandardWebSocketMessage::Ping { .. } => "ping".to_string(),
                StandardWebSocketMessage::Pong { .. } => "pong".to_string(),
                StandardWebSocketMessage::ConnectionEstablished { .. } => {
                    "connection_established".to_string()
                }
                StandardWebSocketMessage::ConnectionClosed { .. } => {
                    "connection_closed".to_string()
                }
                StandardWebSocketMessage::Subscribe { .. } => "subscribe".to_string(),
                StandardWebSocketMessage::Unsubscribe { .. } => "unsubscribe".to_string(),
                StandardWebSocketMessage::SubscriptionAck { .. } => "subscription_ack".to_string(),
                StandardWebSocketMessage::DataUpdate { .. } => "data_update".to_string(),
                StandardWebSocketMessage::StatusUpdate { .. } => "status_update".to_string(),
                StandardWebSocketMessage::Error { .. } => "error".to_string(),
                StandardWebSocketMessage::Request { .. } => "request".to_string(),
                StandardWebSocketMessage::Response { .. } => "response".to_string(),
            },
            payload: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
            timestamp: time::now(),
            client_id,
            request_id: None,
            metadata: None,
        }
    }
}

pub fn serialize_message<T: Serialize>(message: &T) -> Result<String, serde_json::Error> {
    to_json(message).map_err(|e| serde_json::Error::io(std::io::Error::other(e.to_string())))
}

pub fn deserialize_message<T: for<'de> Deserialize<'de>>(
    data: &str,
) -> Result<T, serde_json::Error> {
    from_json(data).map_err(|e| serde_json::Error::io(std::io::Error::other(e.to_string())))
}

pub fn create_error_message(error_type: &str, message: &str) -> StandardWebSocketMessage {
    StandardWebSocketMessage::Error {
        error_type: error_type.to_string(),
        message: message.to_string(),
        details: None,
        timestamp: time::now(),
    }
}

pub fn create_ping_message(client_id: Option<String>) -> StandardWebSocketMessage {
    StandardWebSocketMessage::Ping {
        timestamp: time::now(),
        client_id,
    }
}

pub fn create_pong_message(client_id: Option<String>) -> StandardWebSocketMessage {
    StandardWebSocketMessage::Pong {
        timestamp: time::now(),
        client_id,
    }
}

pub fn validate_message_format(data: &str) -> Result<bool, String> {
    match serde_json::from_str::<StandardWebSocketMessage>(data) {
        Ok(_) => Ok(true),
        Err(e) => Err(format!("Invalid message format: {}", e)),
    }
}

#[derive(Debug, Clone)]
pub struct ChannelManager {
    pub available_channels: Vec<String>,
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self {
            available_channels: vec![
                "health".to_string(),
                "performance".to_string(),
                "telemetry".to_string(),
                "graph".to_string(),
                "mcp".to_string(),
                "speech".to_string(),
                "system".to_string(),
                "ontology".to_string(),
            ],
        }
    }
}

impl ChannelManager {
    pub fn validate_channel(&self, channel: &str) -> bool {
        self.available_channels.contains(&channel.to_string())
    }

    pub fn validate_channels(&self, channels: &[String]) -> Vec<String> {
        channels
            .iter()
            .filter(|&channel| self.validate_channel(channel))
            .cloned()
            .collect()
    }
}
