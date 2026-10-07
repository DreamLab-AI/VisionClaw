use crate::telemetry::agent_telemetry::{
    get_telemetry_logger, CorrelationId, LogLevel, TelemetryEvent,
};
use crate::utils::network::{
    CircuitBreaker, CircuitBreakerConfig, HealthCheckManager, RetryableError, TimeoutConfig,
};
use log::{debug, error, info, warn};
use serde_json;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

pub struct McpRelayManager {
    circuit_breaker: Arc<CircuitBreaker>,
    health_manager: Arc<HealthCheckManager>,
    #[allow(dead_code)]
    timeout_config: TimeoutConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum McpRelayError {
    #[error("Docker command failed: {0}")]
    DockerCommandFailed(String),
    #[error("Container not found: {0}")]
    ContainerNotFound(String),
    #[error("Service health check failed")]
    HealthCheckFailed,
    #[error("Operation timeout")]
    Timeout,
}

impl RetryableError for McpRelayError {
    fn is_retryable(&self) -> bool {
        match self {
            McpRelayError::DockerCommandFailed(_) => true,
            McpRelayError::ContainerNotFound(_) => false,
            McpRelayError::HealthCheckFailed => true,
            McpRelayError::Timeout => true,
        }
    }
}

impl McpRelayManager {
    pub fn new() -> Self {
        let circuit_breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            failure_rate_threshold: 0.5,
            time_window: std::time::Duration::from_secs(60),
            recovery_timeout: std::time::Duration::from_secs(30),
            success_threshold: 2,
            half_open_max_requests: 3,
            minimum_request_threshold: 5,
        }));

        let health_manager = Arc::new(HealthCheckManager::new());

        Self {
            circuit_breaker,
            health_manager,
            timeout_config: TimeoutConfig::default(),
        }
    }

    pub async fn check_relay_status(&self) -> Result<bool, McpRelayError> {
        let operation = || {
            Box::pin(async {
                Self::check_relay_status_internal()
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            })
        };

        match self.circuit_breaker.execute(operation()).await {
            Ok(result) => Ok(result),
            Err(e) => {
                error!("Circuit breaker failed for MCP relay status check: {:?}", e);
                Err(McpRelayError::HealthCheckFailed)
            }
        }
    }

    async fn check_relay_status_internal() -> Result<bool, McpRelayError> {
        let start_time = Instant::now();
        let correlation_id = CorrelationId::new();

        info!("Checking MCP relay status in multi-agent-container...");

        if let Some(logger) = get_telemetry_logger() {
            logger.log_mcp_message("status_check", "outbound", 0, "initiated");
        }

        let output = Command::new("docker")
            .args(["exec", "multi-agent-container", "pgrep", "-f", "mcp-server"])
            .output();

        let duration_ms = start_time.elapsed().as_millis() as f64;

        match output {
            Ok(result) => {
                let is_running = result.status.success();
                let status = if is_running { "running" } else { "stopped" };

                if is_running {
                    info!("MCP relay is running in multi-agent-container");
                } else {
                    warn!("MCP relay is not running in multi-agent-container");
                }

                if let Some(logger) = get_telemetry_logger() {
                    let event = TelemetryEvent::new(
                        correlation_id,
                        if is_running {
                            LogLevel::INFO
                        } else {
                            LogLevel::WARN
                        },
                        "mcp_bridge",
                        "status_check_result",
                        &format!("MCP relay status check completed: {}", status),
                        "mcp_relay_manager",
                    )
                    .with_duration(duration_ms)
                    .with_metadata("container_status", serde_json::json!(status))
                    .with_metadata("container_name", serde_json::json!("multi-agent-container"))
                    .with_metadata("check_method", serde_json::json!("docker_exec_pgrep"));

                    logger.log_event(event);

                    logger.log_mcp_message("status_check", "inbound", result.stdout.len(), status);
                }

                Ok(is_running)
            }
            Err(e) => {
                error!("Failed to check MCP relay status: {}", e);

                if let Some(logger) = get_telemetry_logger() {
                    let event = TelemetryEvent::new(
                        correlation_id,
                        LogLevel::ERROR,
                        "mcp_bridge",
                        "status_check_error",
                        &format!("MCP relay status check failed: {}", e),
                        "mcp_relay_manager",
                    )
                    .with_duration(duration_ms)
                    .with_metadata("error_type", serde_json::json!("docker_command_failed"))
                    .with_metadata("error_message", serde_json::json!(e.to_string()));

                    logger.log_event(event);

                    logger.log_mcp_message("status_check", "error", 0, "failed");
                }

                Ok(false)
            }
        }
    }

    pub async fn ensure_relay_running(&self) -> Result<(), String> {
        if let Some(health_result) = self.health_manager.check_service_now("mcp-relay").await {
            match health_result.status {
                crate::utils::network::HealthStatus::Healthy => {
                    info!("MCP relay health check passed");
                }
                _ => {
                    warn!("Health check failed for MCP relay: {:?}", health_result);
                }
            }
        } else {
            warn!("No health check configuration found for MCP relay");
        }

        if Self::check_relay_status_internal().await.unwrap_or(false) {
            info!("MCP relay already running, no action needed");
            return Ok(());
        }

        info!("Starting MCP relay in multi-agent-container...");

        let output = Command::new("docker")
            .args([
                "exec",
                "-d",
                "multi-agent-container",
                "bash",
                "-c",
                "cd /app && npm run mcp:start > /tmp/mcp-server.log 2>&1",
            ])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    info!("Successfully started MCP relay in multi-agent-container");

                    std::thread::sleep(std::time::Duration::from_secs(2));

                    if Self::check_relay_status_internal().await.unwrap_or(false) {
                        Ok(())
                    } else {
                        Err("MCP relay started but not running".to_string())
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    Err(format!("Failed to start MCP relay: {}", stderr))
                }
            }
            Err(e) => Err(format!("Failed to execute docker command: {}", e)),
        }
    }

    pub fn get_relay_logs(lines: usize) -> Result<String, String> {
        let output = Command::new("docker")
            .args([
                "exec",
                "multi-agent-container",
                "tail",
                "-n",
                &lines.to_string(),
                "/tmp/mcp-server.log",
            ])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    Ok(String::from_utf8_lossy(&result.stdout).to_string())
                } else {
                    Err(format!(
                        "Failed to get logs: {}",
                        String::from_utf8_lossy(&result.stderr)
                    ))
                }
            }
            Err(e) => Err(format!("Failed to execute docker command: {}", e)),
        }
    }

    pub async fn start_health_monitoring(&self) {
        let health_manager = self.health_manager.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

            loop {
                interval.tick().await;

                if let Some(health_result) = health_manager.check_service_now("mcp-relay").await {
                    match health_result.status {
                        crate::utils::network::HealthStatus::Healthy => {
                            debug!("MCP relay health check passed");
                        }
                        _ => {
                            warn!("MCP relay health check failed: {:?}", health_result);
                        }
                    }
                } else {
                    warn!("No health check configuration found for MCP relay");
                }
            }
        });
    }

    pub fn check_mcp_container() -> bool {
        let output = Command::new("docker")
            .args(["ps", "-q", "-f", "name=multi-agent-container"])
            .output();

        match output {
            Ok(result) => !result.stdout.is_empty(),
            Err(_) => false,
        }
    }
}

impl Default for McpRelayManager {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn ensure_mcp_ready() -> Result<(), String> {
    if !McpRelayManager::check_mcp_container() {
        return Err("multi-agent-container is not running".to_string());
    }

    let manager = McpRelayManager::new();

    manager.ensure_relay_running().await?;

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    Ok(())
}
