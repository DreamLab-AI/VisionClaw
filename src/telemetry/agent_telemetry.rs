//! Agent Telemetry Module - Structured logging with correlation IDs
//!
//! Pure-data types (`CorrelationId`, `LogLevel`, `Position3D`, `TelemetryEvent`)
//! now live in `visionclaw_domain::telemetry`. This file re-exports them and
//! provides the I/O infrastructure (file sink, global static, emit macros).

use crate::to_json;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::sync::{Arc, Mutex};

// Re-export pure-data types so existing callers `use crate::telemetry::*` keep working.
pub use visionclaw_domain::telemetry::{CorrelationId, LogLevel, Position3D, TelemetryEvent};

// ── AgentTelemetryLogger ────────────────────────────────────────────────────────

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
            visionclaw_domain::utils::time::now().format("%Y-%m-%d_%H")
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

// ── Global singleton ────────────────────────────────────────────────────────────

static GLOBAL_TELEMETRY_LOGGER: once_cell::sync::OnceCell<AgentTelemetryLogger> =
    once_cell::sync::OnceCell::new();

pub fn init_telemetry_logger(log_dir: &str, buffer_size: usize) -> Result<(), std::io::Error> {
    GLOBAL_TELEMETRY_LOGGER.get_or_try_init(
        || -> Result<AgentTelemetryLogger, std::io::Error> {
            let logger = AgentTelemetryLogger::new(log_dir, buffer_size)?;
            info!(
                "Telemetry logger initialized with log directory: {}",
                log_dir
            );
            Ok(logger)
        },
    )?;
    Ok(())
}

pub fn get_telemetry_logger() -> Option<&'static AgentTelemetryLogger> {
    GLOBAL_TELEMETRY_LOGGER.get()
}

// ── Emit macros ─────────────────────────────────────────────────────────────────

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
