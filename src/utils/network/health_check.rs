use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,

    Degraded,

    Unhealthy,

    Unknown,
}

impl HealthStatus {
    pub fn is_usable(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Degraded)
    }

    pub fn is_critical(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub check_interval: Duration,

    pub check_timeout: Duration,

    pub healthy_threshold: usize,

    pub unhealthy_threshold: usize,

    pub enable_tcp_check: bool,

    pub enable_http_check: bool,

    pub http_health_path: Option<String>,

    pub startup_grace_period: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            check_timeout: Duration::from_secs(5),
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            enable_tcp_check: true,
            enable_http_check: false,
            http_health_path: Some("/health".to_string()),
            startup_grace_period: Duration::from_secs(10),
        }
    }
}

impl HealthCheckConfig {
    pub fn critical_service() -> Self {
        Self {
            check_interval: Duration::from_secs(10),
            check_timeout: Duration::from_secs(3),
            healthy_threshold: 1,
            unhealthy_threshold: 2,
            enable_tcp_check: true,
            enable_http_check: true,
            http_health_path: Some("/health".to_string()),
            startup_grace_period: Duration::from_secs(5),
        }
    }

    pub fn background_service() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            check_timeout: Duration::from_secs(10),
            healthy_threshold: 3,
            unhealthy_threshold: 5,
            enable_tcp_check: true,
            enable_http_check: false,
            http_health_path: None,
            startup_grace_period: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub service_name: String,
    pub check_type: String,
    pub status: HealthStatus,
    pub response_time: Duration,
    pub message: Option<String>,
    pub timestamp: SystemTime,
    pub details: HashMap<String, String>,
}

impl HealthCheckResult {
    pub fn healthy(service_name: String, check_type: String, response_time: Duration) -> Self {
        Self {
            service_name,
            check_type,
            status: HealthStatus::Healthy,
            response_time,
            message: None,
            timestamp: SystemTime::now(),
            details: HashMap::new(),
        }
    }

    pub fn unhealthy(
        service_name: String,
        check_type: String,
        response_time: Duration,
        message: String,
    ) -> Self {
        Self {
            service_name,
            check_type,
            status: HealthStatus::Unhealthy,
            response_time,
            message: Some(message),
            timestamp: SystemTime::now(),
            details: HashMap::new(),
        }
    }

    pub fn with_details(mut self, details: HashMap<String, String>) -> Self {
        self.details = details;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealthInfo {
    pub service_name: String,
    pub current_status: HealthStatus,
    pub last_check: Option<SystemTime>,
    pub consecutive_successes: usize,
    pub consecutive_failures: usize,
    pub total_checks: u64,
    pub successful_checks: u64,
    pub failed_checks: u64,
    pub average_response_time: Duration,
    pub last_success: Option<SystemTime>,
    pub last_failure: Option<SystemTime>,
    pub uptime_percentage: f64,
    pub endpoint: String,
    pub check_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub config: HealthCheckConfig,
    pub additional_endpoints: Vec<String>,
}

impl ServiceEndpoint {
    pub fn new(name: String, host: String, port: u16) -> Self {
        Self {
            name,
            host,
            port,
            config: HealthCheckConfig::default(),
            additional_endpoints: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: HealthCheckConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_http_endpoint(mut self, endpoint: String) -> Self {
        self.additional_endpoints.push(endpoint);
        self
    }

    pub fn tcp_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub struct HealthCheckManager {
    services: Arc<RwLock<HashMap<String, ServiceEndpoint>>>,
    health_info: Arc<RwLock<HashMap<String, ServiceHealthInfo>>>,
    check_handles: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    started_at: Instant,
}

impl HealthCheckManager {
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            health_info: Arc::new(RwLock::new(HashMap::new())),
            check_handles: Arc::new(Mutex::new(HashMap::new())),
            started_at: Instant::now(),
        }
    }

    pub async fn register_service(&self, endpoint: ServiceEndpoint) {
        let service_name = endpoint.name.clone();
        info!(
            "Registering service for health monitoring: {}",
            service_name
        );

        self.services
            .write()
            .await
            .insert(service_name.clone(), endpoint.clone());

        let health_info = ServiceHealthInfo {
            service_name: service_name.clone(),
            current_status: HealthStatus::Unknown,
            last_check: None,
            consecutive_successes: 0,
            consecutive_failures: 0,
            total_checks: 0,
            successful_checks: 0,
            failed_checks: 0,
            average_response_time: Duration::from_millis(0),
            last_success: None,
            last_failure: None,
            uptime_percentage: 0.0,
            endpoint: endpoint.tcp_address(),
            check_types: self.get_enabled_check_types(&endpoint.config),
        };

        self.health_info
            .write()
            .await
            .insert(service_name.clone(), health_info);

        self.start_health_check_task(service_name, endpoint).await;
    }

    pub async fn unregister_service(&self, service_name: &str) {
        info!(
            "Unregistering service from health monitoring: {}",
            service_name
        );

        self.services.write().await.remove(service_name);
        self.health_info.write().await.remove(service_name);

        let mut handles = self.check_handles.lock().await;
        if let Some(handle) = handles.remove(service_name) {
            handle.abort();
        }
    }

    pub async fn get_service_health(&self, service_name: &str) -> Option<ServiceHealthInfo> {
        self.health_info.read().await.get(service_name).cloned()
    }

    pub async fn get_all_health(&self) -> HashMap<String, ServiceHealthInfo> {
        self.health_info.read().await.clone()
    }

    pub async fn get_unhealthy_services(&self) -> Vec<ServiceHealthInfo> {
        self.health_info
            .read()
            .await
            .values()
            .filter(|info| info.current_status == HealthStatus::Unhealthy)
            .cloned()
            .collect()
    }

    pub async fn check_service_now(&self, service_name: &str) -> Option<HealthCheckResult> {
        let services = self.services.read().await;
        let endpoint = services.get(service_name)?;

        let result = self.perform_health_check(endpoint).await;
        self.update_health_info(&result).await;

        Some(result)
    }

    pub async fn are_critical_services_healthy(&self) -> bool {
        let health_info = self.health_info.read().await;

        health_info
            .values()
            .all(|info| info.current_status.is_usable())
    }

    pub async fn get_system_health_summary(&self) -> SystemHealthSummary {
        let health_info = self.health_info.read().await;

        let total_services = health_info.len();
        let healthy_services = health_info
            .values()
            .filter(|info| info.current_status == HealthStatus::Healthy)
            .count();
        let degraded_services = health_info
            .values()
            .filter(|info| info.current_status == HealthStatus::Degraded)
            .count();
        let unhealthy_services = health_info
            .values()
            .filter(|info| info.current_status == HealthStatus::Unhealthy)
            .count();

        let overall_status = if unhealthy_services > 0 {
            HealthStatus::Unhealthy
        } else if degraded_services > 0 {
            HealthStatus::Degraded
        } else if healthy_services > 0 {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unknown
        };

        SystemHealthSummary {
            overall_status,
            total_services,
            healthy_services,
            degraded_services,
            unhealthy_services,
            uptime: self.started_at.elapsed(),
            last_updated: SystemTime::now(),
        }
    }

    pub async fn shutdown(&self) {
        info!("Shutting down health check manager");

        let mut handles = self.check_handles.lock().await;
        for (service_name, handle) in handles.drain() {
            debug!("Stopping health check for service: {}", service_name);
            handle.abort();
        }
    }

    async fn start_health_check_task(&self, service_name: String, endpoint: ServiceEndpoint) {
        let services = self.services.clone();
        let health_info = self.health_info.clone();
        let service_name_clone = service_name.clone();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(endpoint.config.startup_grace_period).await;

            let mut interval = tokio::time::interval(endpoint.config.check_interval);

            loop {
                interval.tick().await;

                let current_endpoint = {
                    let services_guard = services.read().await;
                    if let Some(ep) = services_guard.get(&service_name_clone) {
                        ep.clone()
                    } else {
                        break;
                    }
                };

                let manager = HealthCheckManager {
                    services: services.clone(),
                    health_info: health_info.clone(),
                    check_handles: Arc::new(Mutex::new(HashMap::new())),
                    started_at: Instant::now(),
                };

                let result = manager.perform_health_check(&current_endpoint).await;
                manager.update_health_info(&result).await;
            }
        });

        self.check_handles.lock().await.insert(service_name, handle);
    }

    async fn perform_health_check(&self, endpoint: &ServiceEndpoint) -> HealthCheckResult {
        let start_time = Instant::now();
        let service_name = endpoint.name.clone();

        if endpoint.config.enable_tcp_check {
            match self.tcp_health_check(endpoint).await {
                Ok(response_time) => {
                    return HealthCheckResult::healthy(
                        service_name,
                        "tcp".to_string(),
                        response_time,
                    );
                }
                Err(err) => {
                    return HealthCheckResult::unhealthy(
                        service_name,
                        "tcp".to_string(),
                        start_time.elapsed(),
                        err,
                    );
                }
            }
        }

        if endpoint.config.enable_http_check {
            match self.http_health_check(endpoint).await {
                Ok(response_time) => {
                    return HealthCheckResult::healthy(
                        service_name,
                        "http".to_string(),
                        response_time,
                    );
                }
                Err(err) => {
                    return HealthCheckResult::unhealthy(
                        service_name,
                        "http".to_string(),
                        start_time.elapsed(),
                        err,
                    );
                }
            }
        }

        HealthCheckResult::unhealthy(
            service_name,
            "none".to_string(),
            start_time.elapsed(),
            "No health checks configured".to_string(),
        )
    }

    async fn tcp_health_check(&self, endpoint: &ServiceEndpoint) -> Result<Duration, String> {
        let start_time = Instant::now();
        let address = endpoint.tcp_address();

        match timeout(endpoint.config.check_timeout, TcpStream::connect(&address)).await {
            Ok(Ok(_stream)) => {
                let response_time = start_time.elapsed();
                debug!(
                    "TCP health check successful for {}: {:?}",
                    endpoint.name, response_time
                );
                Ok(response_time)
            }
            Ok(Err(err)) => {
                let error_msg = format!("TCP connection failed to {}: {}", address, err);
                debug!("{}", error_msg);
                Err(error_msg)
            }
            Err(_) => {
                let error_msg = format!("TCP connection timeout to {}", address);
                debug!("{}", error_msg);
                Err(error_msg)
            }
        }
    }

    async fn http_health_check(&self, endpoint: &ServiceEndpoint) -> Result<Duration, String> {
        let _start_time = Instant::now();

        let health_path = endpoint
            .config
            .http_health_path
            .as_deref()
            .unwrap_or("/health");

        let url = if endpoint.additional_endpoints.is_empty() {
            format!("http://{}:{}{}", endpoint.host, endpoint.port, health_path)
        } else {
            format!("{}{}", endpoint.additional_endpoints[0], health_path)
        };

        debug!("HTTP health check would connect to: {}", url);

        self.tcp_health_check(endpoint).await
    }

    async fn update_health_info(&self, result: &HealthCheckResult) {
        let mut health_info = self.health_info.write().await;

        if let Some(info) = health_info.get_mut(&result.service_name) {
            info.last_check = Some(result.timestamp);
            info.total_checks += 1;

            match result.status {
                HealthStatus::Healthy => {
                    info.successful_checks += 1;
                    info.consecutive_successes += 1;
                    info.consecutive_failures = 0;
                    info.last_success = Some(result.timestamp);

                    if info.consecutive_successes >= info.consecutive_successes.max(1)
                        && info.current_status != HealthStatus::Healthy
                    {
                        info!("Service {} is now healthy", result.service_name);
                        info.current_status = HealthStatus::Healthy;
                    }
                }
                HealthStatus::Unhealthy => {
                    info.failed_checks += 1;
                    info.consecutive_failures += 1;
                    info.consecutive_successes = 0;
                    info.last_failure = Some(result.timestamp);

                    let services = self.services.read().await;
                    if let Some(endpoint) = services.get(&result.service_name) {
                        if info.consecutive_failures >= endpoint.config.unhealthy_threshold
                            && info.current_status != HealthStatus::Unhealthy
                        {
                            warn!(
                                "Service {} is now unhealthy: {:?}",
                                result.service_name, result.message
                            );
                            info.current_status = HealthStatus::Unhealthy;
                        }
                    }
                }
                _ => {}
            }

            if info.total_checks > 0 {
                let total_time =
                    info.average_response_time.as_millis() as u64 * (info.total_checks - 1);
                let new_total = total_time + result.response_time.as_millis() as u64;
                info.average_response_time = Duration::from_millis(new_total / info.total_checks);
            }

            info.uptime_percentage = if info.total_checks > 0 {
                (info.successful_checks as f64 / info.total_checks as f64) * 100.0
            } else {
                0.0
            };
        }
    }

    fn get_enabled_check_types(&self, config: &HealthCheckConfig) -> Vec<String> {
        let mut types = Vec::new();

        if config.enable_tcp_check {
            types.push("tcp".to_string());
        }

        if config.enable_http_check {
            types.push("http".to_string());
        }

        if types.is_empty() {
            types.push("none".to_string());
        }

        types
    }
}

impl Default for HealthCheckManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealthSummary {
    pub overall_status: HealthStatus,
    pub total_services: usize,
    pub healthy_services: usize,
    pub degraded_services: usize,
    pub unhealthy_services: usize,
    pub uptime: Duration,
    pub last_updated: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_health_check_manager_creation() {
        let manager = HealthCheckManager::new();
        let summary = manager.get_system_health_summary().await;

        assert_eq!(summary.total_services, 0);
        assert_eq!(summary.overall_status, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_service_registration() {
        let manager = HealthCheckManager::new();

        let endpoint =
            ServiceEndpoint::new("test-service".to_string(), "127.0.0.1".to_string(), 8080);

        manager.register_service(endpoint).await;

        let health = manager.get_service_health("test-service").await;
        assert!(health.is_some());

        let health_info = health.unwrap();
        assert_eq!(health_info.service_name, "test-service");
        assert_eq!(health_info.current_status, HealthStatus::Unknown);

        manager.unregister_service("test-service").await;

        let health_after = manager.get_service_health("test-service").await;
        assert!(health_after.is_none());
    }

    #[test]
    fn test_health_status_methods() {
        assert!(HealthStatus::Healthy.is_usable());
        assert!(HealthStatus::Degraded.is_usable());
        assert!(!HealthStatus::Unhealthy.is_usable());
        assert!(!HealthStatus::Unknown.is_usable());

        assert!(!HealthStatus::Healthy.is_critical());
        assert!(!HealthStatus::Degraded.is_critical());
        assert!(HealthStatus::Unhealthy.is_critical());
        assert!(!HealthStatus::Unknown.is_critical());
    }

    #[tokio::test]
    async fn test_service_endpoint_creation() {
        let endpoint = ServiceEndpoint::new("test".to_string(), "localhost".to_string(), 9000)
            .with_config(HealthCheckConfig::critical_service())
            .with_http_endpoint("http://localhost:9000".to_string());

        assert_eq!(endpoint.name, "test");
        assert_eq!(endpoint.tcp_address(), "localhost:9000");
        assert_eq!(endpoint.additional_endpoints.len(), 1);
        assert_eq!(endpoint.config.check_interval, Duration::from_secs(10));
    }
}
