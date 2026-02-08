//! Agent Telemetry Module - Structured logging with correlation IDs
//!
//! This module provides comprehensive telemetry and structured logging for the WebXR
//! graph visualization system, including agent lifecycle, GPU operations, and MCP bridge.

use crate:::time;
use crate::to_json;
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections:::HashMap;
use std::fs::{create_dir_all, OpenOptions};
use std::io:::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid:::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CorrelationId(pub String);

impl CorrelationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_agent_id(agent_id: &str) -> Self {
        Self(format!("agent-{}", agent_id))
    }

    pub fn from_session_uuid(uuid: &str) -> Self {
        Self(format!("session-{}", uuid))
    }

    pub fn from_swarm_id(swarm_id: &str) -> Self {
        Self(format!("swarm-{}", swarm_id))
    }

    
    
    pub fn from_client_session(client_session_id: &str) -> Self {
        Self(format!("client-{}", client_session_id))
    }

    
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LogLevel {
    TRACE, 
    DEBUG, 
    INFO,  
    WARN,  
    ERROR, 
}

impl From<log::Level> for LogLevel {
    fn from(level: log::Level) -> Self {
        match level {
            log::Level::Trace => LogLevel::TRACE,
            log::Level::Debug => LogLevel::DEBUG,
            log::Level::Info => LogLevel::INFO,
            log::Level::Warn => LogLevel::WARN,
            log::Level::Error => LogLevel::ERROR,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub magnitude: f32,
}

impl Position3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        let magnitude = (x * x + y * y + z * z).sqrt();
        Self { x, y, z, magnitude }
    }

    pub fn origin() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    pub fn is_origin(&self) -> bool {
        self.magnitude < f32::EPSILON
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    
    pub timestamp: DateTime<Utc>,
    pub timestamp_micros: u128,

    
    pub correlation_id: CorrelationId,

    
    pub level: LogLevel,

    
    pub category: String,

    
    pub event_type: String,

    
    pub message: String,

    
    pub metadata: HashMap<String, serde_json::Value>,

    
    pub agent_id: Option<String>,
    pub component: String,

    
    pub duration_ms: Option<f64>,
    pub memory_usage_bytes: Option<u64>,

    
    pub position: Option<Position3D>,
    pub position_delta: Option<Position3D>,

    
    pub gpu_kernel: Option<String>,
    pub gpu_execution_time_ms: Option<f64>,
    pub gpu_memory_mb: Option<f32>,

    
    pub mcp_message_type: Option<String>,
    pub mcp_direction: Option<String>, 
    pub mcp_payload_size: Option<usize>,

    
    pub session_uuid: Option<String>,
    pub swarm_id: Option<String>,

    
    
    pub client_session_id: Option<String>,
}

impl TelemetryEvent {
    
    pub fn new(
        correlation_id: CorrelationId,
        level: LogLevel,
        category: &str,
        event_type: &str,
        message: &str,
        component: &str,
    ) -> Self {
        let now = time::now();
        let timestamp_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();

        Self {
            timestamp: now,
            timestamp_micros,
            correlation_id,
            level,
            category: category.to_string(),
            event_type: event_type.to_string(),
            message: message.to_string(),
            metadata: HashMap::new(),
            agent_id: None,
            component: component.to_string(),
            duration_ms: None,
            memory_usage_bytes: None,
            position: None,
            position_delta: None,
            gpu_kernel: None,
            gpu_execution_time_ms: None,
            gpu_memory_mb: None,
            mcp_message_type: None,
            mcp_direction: None,
            mcp_payload_size: None,
            session_uuid: None,
            swarm_id: None,
            client_session_id: None,
        }
    }

    
    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }

    
    pub fn with_session_uuid(mut self, session_uuid: &str) -> Self {
        self.session_uuid = Some(session_uuid.to_string());
        self
    }

    
    pub fn with_swarm_id(mut self, swarm_id: &str) -> Self {
        self.swarm_id = Some(swarm_id.to_string());
        self
    }

    
    pub fn with_client_session_id(mut self, client_session_id: &str) -> Self {
        self.client_session_id = Some(client_session_id.to_string());
        self
    }

    
    pub fn with_agent_id(mut self, agent_id: &str) -> Self {
        self.agent_id = Some(agent_id.to_string());
        self
    }

    
    pub fn with_position(mut self, position: Position3D) -> Self {
        self.position = Some(position);
        self
    }

    
    pub fn with_position_delta(mut self, old_pos: Position3D, new_pos: Position3D) -> Self {
        let delta = Position3D::new(
            new_pos.x - old_pos.x,
            new_pos.y - old_pos.y,
            new_pos.z - old_pos.z,
        );
        self.position_delta = Some(delta);
        self.position = Some(new_pos);
        self
    }

    
    pub fn with_gpu_info(mut self, kernel: &str, execution_time_ms: f64, memory_mb: f32) -> Self {
        self.gpu_kernel = Some(kernel.to_string());
        self.gpu_execution_time_ms = Some(execution_time_ms);
        self.gpu_memory_mb = Some(memory_mb);
        self
    }

    
    pub fn with_mcp_info(
        mut self,
        message_type: &str,
        direction: &str,
        payload_size: usize,
    ) -> Self {
        self.mcp_message_type = Some(message_type.to_string());
        self.mcp_direction = Some(direction.to_string());
        self.mcp_payload_size = Some(payload_size);
        self
    }

    
    pub fn with_duration(mut self, duration_ms: f64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

#[derive(Clone)]
pub struct AgentTelemetryLogger {
    log_dir: String,
    correlation_contexts: Arc<Mutex<HashMap<String, CorrelationId>>>,
    event_buffer: Arc<Mutex<Vec<TelemetryEvent>>>,
    buffer_size: usize,
}

impl AgentTelemetryLogger {
    
    pub fn new(log_dir: &str, buffer_size: usize) -> Result<Self, std::io::Error> {
        
        create_dir_all(log_dir)?;

        Ok(Self {
            log_dir: log_dir.to_string(),
            correlation_contexts: Arc::new(Mutex::new(HashMap::new())),
            event_buffer: Arc::new(Mutex::new(Vec::with_capacity(buffer_size))),
            buffer_size,
        })
    }

    
    pub fn set_correlation_context(&self, context_key: &str, correlation_id: CorrelationId) {
        if let Ok(mut contexts) = self.correlation_contexts.lock() {
            contexts.insert(context_key.to_string(), correlation_id);
        }
    }

    
    pub fn get_correlation_context(&self, context_key: &str) -> Option<CorrelationId> {
        self.correlation_contexts
            .lock()
            .ok()?
            .get(context_key)
            .cloned()
    }

    
    pub fn log_event(&self, event: TelemetryEvent) {
        
        let json_str = to_json(&event)
            .unwrap_or_else(|e| format!(r#"{{"error": "Failed to serialize event: {}"}}"#, e));

        match event.level {
            LogLevel::TRACE => trace!("TELEMETRY: {}", json_str),
            LogLevel::DEBUG => debug!("TELEMETRY: {}", json_str),
            LogLevel::INFO => info!("TELEMETRY: {}", json_str),
            LogLevel::WARN => warn!("TELEMETRY: {}", json_str),
            LogLevel::ERROR => error!("TELEMETRY: {}", json_str),
        }

        
        if let Ok(mut buffer) = self.event_buffer.lock() {
            buffer.push(event);

            
            if buffer.len() >= self.buffer_size {
                self.flush_buffer_to_file(&mut buffer);
            }
        }
    }

    
    
    pub fn from_client_session(
        &self,
        client_session_id: &str,
        _bridge: Option<()>, 
        level: LogLevel,
        category: &str,
        event_type: &str,
        message: &str,
        component: &str,
    ) -> TelemetryEvent {
        
        let correlation_id = {
            debug!(
                "Creating fallback correlation ID for client session {}",
                client_session_id
            );
            CorrelationId::from_client_session(client_session_id)
        };

        TelemetryEvent::new(
            correlation_id,
            level,
            category,
            event_type,
            message,
            component,
        )
        .with_client_session_id(client_session_id)
    }

    
    pub fn log_agent_spawn(
        &self,
        agent_id: &str,
        session_uuid: Option<&str>,
        initial_position: Position3D,
        metadata: HashMap<String, serde_json::Value>,
    ) {
        let correlation_id = if let Some(uuid) = session_uuid {
            CorrelationId::from_session_uuid(uuid)
        } else {
            CorrelationId::from_agent_id(agent_id)
        };

        self.set_correlation_context(agent_id, correlation_id.clone());

        let mut event = TelemetryEvent::new(
            correlation_id,
            LogLevel::INFO,
            "agent_lifecycle",
            "agent_spawn",
            &format!(
                "Agent {} spawned at position ({}, {}, {})",
                agent_id, initial_position.x, initial_position.y, initial_position.z
            ),
            "client_manager_actor",
        )
        .with_agent_id(agent_id)
        .with_position(initial_position.clone());

        
        if let Some(uuid) = session_uuid {
            event = event.with_session_uuid(uuid);
        }

        
        for (key, value) in metadata {
            event = event.with_metadata(&key, value);
        }

        
        if initial_position.is_origin() {
            event = event.with_metadata(
                "position_debug",
                serde_json::json!({
                    "is_origin": true,
                    "magnitude": initial_position.magnitude,
                    "debug_message": "ORIGIN POSITION BUG: Agent spawned at (0,0,0)"
                }),
            );
        }

        self.log_event(event);
    }

    
    pub fn log_position_update(
        &self,
        agent_id: &str,
        old_position: Position3D,
        new_position: Position3D,
        source: &str,
    ) {
        let correlation_id = self
            .get_correlation_context(agent_id)
            .unwrap_or_else(|| CorrelationId::from_agent_id(agent_id));

        let event = TelemetryEvent::new(
            correlation_id,
            LogLevel::TRACE,
            "position_tracking",
            "position_update",
            &format!(
                "Agent {} position updated by {} from ({}, {}, {}) to ({}, {}, {})",
                agent_id,
                source,
                old_position.x,
                old_position.y,
                old_position.z,
                new_position.x,
                new_position.y,
                new_position.z
            ),
            source,
        )
        .with_agent_id(agent_id)
        .with_position_delta(old_position, new_position)
        .with_metadata("source", serde_json::json!(source));

        self.log_event(event);
    }

    
    pub fn log_gpu_execution(
        &self,
        kernel_name: &str,
        node_count: u32,
        execution_time_ms: f64,
        memory_mb: f32,
    ) {
        let correlation_id = CorrelationId::new();

        let event = TelemetryEvent::new(
            correlation_id,
            LogLevel::DEBUG,
            "gpu_compute",
            "kernel_execution",
            &format!(
                "GPU kernel {} executed on {} nodes in {:.2}ms",
                kernel_name, node_count, execution_time_ms
            ),
            "gpu_manager_actor",
        )
        .with_gpu_info(kernel_name, execution_time_ms, memory_mb)
        .with_metadata("node_count", serde_json::json!(node_count))
        .with_duration(execution_time_ms);

        self.log_event(event);
    }

    
    pub fn log_mcp_message(
        &self,
        message_type: &str,
        direction: &str,
        payload_size: usize,
        status: &str,
    ) {
        let correlation_id = CorrelationId::new();

        let event = TelemetryEvent::new(
            correlation_id,
            LogLevel::DEBUG,
            "mcp_bridge",
            "message_flow",
            &format!(
                "MCP {} message {} ({} bytes): {}",
                direction, message_type, payload_size, status
            ),
            "mcp_relay_manager",
        )
        .with_mcp_info(message_type, direction, payload_size)
        .with_metadata("status", serde_json::json!(status));

        self.log_event(event);
    }

    
    pub fn log_graph_state_change(
        &self,
        session_uuid: Option<&str>,
        change_type: &str,
        node_count: u32,
        edge_count: u32,
        details: HashMap<String, serde_json::Value>,
    ) {
        let correlation_id = if let Some(uuid) = session_uuid {
            CorrelationId::from_session_uuid(uuid)
        } else {
            CorrelationId::new()
        };

        let mut event = TelemetryEvent::new(
            correlation_id,
            LogLevel::INFO,
            "graph_state",
            "state_change",
            &format!(
                "Graph state changed: {} (nodes: {}, edges: {})",
                change_type, node_count, edge_count
            ),
            "graph_service_actor",
        )
        .with_metadata("change_type", serde_json::json!(change_type))
        .with_metadata("node_count", serde_json::json!(node_count))
        .with_metadata("edge_count", serde_json::json!(edge_count));

        
        if let Some(uuid) = session_uuid {
            event = event.with_session_uuid(uuid);
        }

        
        for (key, value) in details {
            event = event.with_metadata(&key, value);
        }

        self.log_event(event);
    }

    
    fn flush_buffer_to_file(&self, buffer: &mut Vec<TelemetryEvent>) {
        if buffer.is_empty() {
            return;
        }

        let file_path = format!(
            "{}/agent_telemetry_{}.jsonl",
            self.log_dir,
            time::now().format("%Y-%m-%d_%H")
        );

        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
        {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to open telemetry file {}: {}", file_path, e);
                buffer.clear();
                return;
            }
        };

        for event in buffer.iter() {
            if let Ok(json_line) = to_json(event) {
                if let Err(e) = writeln!(file, "{}", json_line) {
                    error!("Failed to write telemetry event to file: {}", e);
                    break;
                }
            }
        }

        if let Err(e) = file.flush() {
            error!("Failed to flush telemetry file: {}", e);
        }

        buffer.clear();
    }

    
    pub fn flush(&self) {
        if let Ok(mut buffer) = self.event_buffer.lock() {
            self.flush_buffer_to_file(&mut buffer);
        }
    }
}

static GLOBAL_TELEMETRY_LOGGER: std::sync::OnceLock<AgentTelemetryLogger> = std::sync::OnceLock::new();

pub fn init_telemetry_logger(log_dir: &str, buffer_size: usize) -> Result<(), std::io::Error> {
    match GLOBAL_TELEMETRY_LOGGER.get() {
        Some(_) => {
            info!("Telemetry logger already initialized");
        }
        None => {
            match AgentTelemetryLogger::new(log_dir, buffer_size) {
                Ok(logger) => {
                    let _ = GLOBAL_TELEMETRY_LOGGER.set(logger);
                    info!(
                        "Telemetry logger initialized with log directory: {}",
                        log_dir
                    );
                }
                Err(e) => {
                    error!("Failed to initialize telemetry logger: {}", e);
                }
            }
        }
    }
    Ok(())
}

pub fn get_telemetry_logger() -> Option<&'static AgentTelemetryLogger> {
    GLOBAL_TELEMETRY_LOGGER.get()
}

#[macro_export]
macro_rules! telemetry_info {
    ($correlation_id:expr, $category:expr, $event_type:expr, $message:expr, $component:expr) => {
        if let Some(logger) = $crate::telemetry::agent_telemetry::get_telemetry_logger() {
            let event = $crate::telemetry::agent_telemetry::TelemetryEvent::new(
                $correlation_id,
                $crate::telemetry::agent_telemetry::LogLevel::INFO,
                $category,
                $event_type,
                $message,
                $component,
            );
            logger.log_event(event);
        }
    };
}

#[macro_export]
macro_rules! telemetry_debug {
    ($correlation_id:expr, $category:expr, $event_type:expr, $message:expr, $component:expr) => {
        if let Some(logger) = $crate::telemetry::agent_telemetry::get_telemetry_logger() {
            let event = $crate::telemetry::agent_telemetry::TelemetryEvent::new(
                $correlation_id,
                $crate::telemetry::agent_telemetry::LogLevel::DEBUG,
                $category,
                $event_type,
                $message,
                $component,
            );
            logger.log_event(event);
        }
    };
}

#[macro_export]
macro_rules! telemetry_trace {
    ($correlation_id:expr, $category:expr, $event_type:expr, $message:expr, $component:expr) => {
        if let Some(logger) = $crate::telemetry::agent_telemetry::get_telemetry_logger() {
            let event = $crate::telemetry::agent_telemetry::TelemetryEvent::new(
                $correlation_id,
                $crate::telemetry::agent_telemetry::LogLevel::TRACE,
                $category,
                $event_type,
                $message,
                $component,
            );
            logger.log_event(event);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
use crate::utils::json::{from_json, to_json};

    #[test]
    fn test_position_3d() {
        let pos = Position3D::new(3.0, 4.0, 0.0);
        assert_eq!(pos.magnitude, 5.0);

        let origin = Position3D::origin();
        assert!(origin.is_origin());
    }

    #[test]
    fn test_correlation_id() {
        let id = CorrelationId::new();
        assert!(!id.as_str().is_empty());

        let agent_id = CorrelationId::from_agent_id("test_agent");
        assert!(agent_id.as_str().contains("agent-test_agent"));
    }

    #[test]
    fn test_telemetry_event_creation() {
        let correlation_id = CorrelationId::new();
        let event = TelemetryEvent::new(
            correlation_id,
            LogLevel::INFO,
            "test",
            "test_event",
            "Test message",
            "test_component",
        );

        assert_eq!(event.category, "test");
        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.message, "Test message");
        assert_eq!(event.component, "test_component");
    }
}

