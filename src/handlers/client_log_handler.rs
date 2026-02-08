use actix_web::{web, HttpRequest, HttpResponse};
use chrono:::Local;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::fs:::OpenOptions;
use std::io:::Write;
use uuid:::Uuid;

use crate::telemetry::agent_telemetry::{get_telemetry_logger, CorrelationId};
use crate::ok_json;
use crate:::AppState;

#[derive(Debug, Deserialize, Serialize)]
pub struct LogEntry {
    level: String,
    namespace: String,
    message: String,
    timestamp: String,
    data: Option<serde_json::Value>,
    #[serde(rename = "userAgent")]
    user_agent: Option<String>,
    url: Option<String>,
    stack: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClientLogsPayload {
    logs: Vec<LogEntry>,
    #[serde(rename = "sessionId")]
    session_id: String,
    timestamp: String,
}

pub async fn handle_client_logs(
    req: HttpRequest,
    payload: web::Json<ClientLogsPayload>,
    _app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let log_file_path = "/app/logs/client.log";

    
    let header_session_id = req
        .headers()
        .get("X-Session-ID")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let client_session_id = header_session_id.as_ref().unwrap_or(&payload.session_id);

    
    let correlation_id = Uuid::parse_str(client_session_id).unwrap_or_else(|_| {
        
        let new_corr_id = Uuid::new_v4();
        debug!(
            "Created new correlation ID for session {}: {}",
            client_session_id, new_corr_id
        );
        new_corr_id
    });

    
    if let Some(telemetry) = get_telemetry_logger() {
        let event = crate::telemetry::agent_telemetry::TelemetryEvent::new(
            CorrelationId(correlation_id.to_string()),
            crate::telemetry::agent_telemetry::LogLevel::INFO,
            "client_logs",
            "logs_received",
            &format!(
                "Received {} log entries from client session",
                payload.logs.len()
            ),
            "client_log_handler",
        )
        .with_client_session_id(client_session_id)
        .with_metadata("log_count", serde_json::json!(payload.logs.len()))
        .with_metadata(
            "has_x_session_id_header",
            serde_json::json!(header_session_id.is_some()),
        )
        .with_metadata(
            "correlation_id",
            serde_json::json!(correlation_id.to_string()),
        );

        telemetry.log_event(event);
    }

    
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .map_err(|e| {
            error!("Failed to open client.log: {}", e);
            actix_web::error::ErrorInternalServerError(format!("Failed to open log file: {}", e))
        })?;

    
    for entry in &payload.logs {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");

        
        let log_line = format!(
            "[{}] [{}] [{}] [corr:{}] {} - {} | UA: {} | URL: {}{}\n",
            timestamp,
            entry.level.to_uppercase(),
            entry.namespace,
            correlation_id,
            payload.session_id,
            entry.message,
            entry.user_agent.as_ref().unwrap_or(&"unknown".to_string()),
            entry.url.as_ref().unwrap_or(&"unknown".to_string()),
            if let Some(data) = &entry.data {
                format!(
                    " | Data: {}",
                    serde_json::to_string(data).unwrap_or_default()
                )
            } else {
                String::new()
            }
        );

        
        file.write_all(log_line.as_bytes()).map_err(|e| {
            error!("Failed to write to client.log: {}", e);
            actix_web::error::ErrorInternalServerError(format!("Failed to write log: {}", e))
        })?;

        
        match entry.level.as_str() {
            "error" => error!(
                "[CLIENT:{}] {} - {}",
                correlation_id, entry.namespace, entry.message
            ),
            "warn" => log::warn!(
                "[CLIENT:{}] {} - {}",
                correlation_id,
                entry.namespace,
                entry.message
            ),
            "info" => info!(
                "[CLIENT:{}] {} - {}",
                correlation_id, entry.namespace, entry.message
            ),
            _ => debug!(
                "[CLIENT:{}] {} - {}",
                correlation_id, entry.namespace, entry.message
            ),
        }

        
        if let Some(stack) = &entry.stack {
            let stack_line = format!(
                "[{}] [STACK] [corr:{}] {}\n{}\n",
                timestamp, correlation_id, payload.session_id, stack
            );
            file.write_all(stack_line.as_bytes()).map_err(|e| {
                error!("Failed to write stack trace: {}", e);
                actix_web::error::ErrorInternalServerError(format!("Failed to write stack: {}", e))
            })?;
        }
    }

    
    file.flush().map_err(|e| {
        error!("Failed to flush client.log: {}", e);
        actix_web::error::ErrorInternalServerError(format!("Failed to flush log file: {}", e))
    })?;

    debug!(
        "Received {} log entries from client session {} (correlation: {})",
        payload.logs.len(),
        payload.session_id,
        correlation_id
    );

    ok_json!(serde_json::json!({
        "status": "success",
        "received": payload.logs.len(),
        "correlation_id": correlation_id.to_string(),
        "session_id": client_session_id
    }))
}

