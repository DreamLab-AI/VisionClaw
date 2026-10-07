use serde::{Deserialize, Serialize};
use specta::Type;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    #[serde(alias = "bind_address")]
    pub bind_address: String,
    #[serde(alias = "domain")]
    pub domain: String,
    #[serde(alias = "enable_http2")]
    pub enable_http2: bool,
    #[serde(alias = "enable_rate_limiting")]
    pub enable_rate_limiting: bool,
    #[serde(alias = "enable_tls")]
    pub enable_tls: bool,
    #[serde(alias = "max_request_size")]
    pub max_request_size: usize,
    #[serde(alias = "min_tls_version")]
    pub min_tls_version: String,
    #[serde(alias = "port")]
    pub port: u16,
    #[serde(alias = "rate_limit_requests")]
    pub rate_limit_requests: u32,
    #[serde(alias = "rate_limit_window")]
    pub rate_limit_window: u32,
    #[serde(alias = "tunnel_id")]
    pub tunnel_id: String,
    #[serde(alias = "api_client_timeout")]
    pub api_client_timeout: u64,
    #[serde(alias = "enable_metrics")]
    pub enable_metrics: bool,
    #[serde(alias = "max_concurrent_requests")]
    pub max_concurrent_requests: u32,
    #[serde(alias = "max_retries")]
    pub max_retries: u32,
    #[serde(alias = "metrics_port")]
    pub metrics_port: u16,
    #[serde(alias = "retry_delay")]
    pub retry_delay: u32,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8080,
            domain: String::new(),
            enable_http2: false,
            enable_rate_limiting: false,
            enable_tls: false,
            max_request_size: 10485760,
            min_tls_version: "1.2".to_string(),
            rate_limit_requests: 100,
            rate_limit_window: 60,
            tunnel_id: String::new(),
            api_client_timeout: 30,
            enable_metrics: true,
            max_concurrent_requests: 1000,
            max_retries: 3,
            metrics_port: 9090,
            retry_delay: 1000,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketSettings {
    #[serde(alias = "binary_chunk_size")]
    pub binary_chunk_size: usize,
    #[serde(alias = "binary_update_rate")]
    pub binary_update_rate: u32,
    #[serde(alias = "min_update_rate")]
    pub min_update_rate: u32,
    #[serde(alias = "max_update_rate")]
    pub max_update_rate: u32,
    #[serde(alias = "motion_threshold")]
    pub motion_threshold: f32,
    #[serde(alias = "motion_damping")]
    pub motion_damping: f32,
    #[serde(alias = "binary_message_version")]
    pub binary_message_version: u32,
    #[serde(alias = "compression_enabled")]
    pub compression_enabled: bool,
    #[serde(alias = "compression_threshold")]
    pub compression_threshold: usize,
    #[serde(alias = "heartbeat_interval")]
    pub heartbeat_interval: u64,
    #[serde(alias = "heartbeat_timeout")]
    pub heartbeat_timeout: u64,
    #[serde(alias = "max_connections")]
    pub max_connections: usize,
    #[serde(alias = "max_message_size")]
    pub max_message_size: usize,
    #[serde(alias = "reconnect_attempts")]
    pub reconnect_attempts: u32,
    #[serde(alias = "reconnect_delay")]
    pub reconnect_delay: u64,
    #[serde(alias = "update_rate")]
    pub update_rate: u32,
}

impl Default for WebSocketSettings {
    fn default() -> Self {
        Self {
            binary_chunk_size: 2048,
            binary_update_rate: 30,
            min_update_rate: 5,
            max_update_rate: 60,
            motion_threshold: 0.05,
            motion_damping: 0.9,
            binary_message_version: 1,
            compression_enabled: false,
            compression_threshold: 512,
            heartbeat_interval: 10000,
            heartbeat_timeout: 600000,
            max_connections: 100,
            max_message_size: 10485760,
            reconnect_attempts: 5,
            reconnect_delay: 1000,
            update_rate: 60,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    #[serde(alias = "allowed_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(alias = "audit_log_path")]
    pub audit_log_path: String,
    #[serde(alias = "cookie_httponly")]
    pub cookie_httponly: bool,
    #[serde(alias = "cookie_samesite")]
    pub cookie_samesite: String,
    #[serde(alias = "cookie_secure")]
    pub cookie_secure: bool,
    #[serde(alias = "csrf_token_timeout")]
    pub csrf_token_timeout: u32,
    #[serde(alias = "enable_audit_logging")]
    pub enable_audit_logging: bool,
    #[serde(alias = "enable_request_validation")]
    pub enable_request_validation: bool,
    #[serde(alias = "session_timeout")]
    pub session_timeout: u32,
}

// Simple debug settings for server-side control
#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate, Default)]
#[serde(rename_all = "camelCase")]
pub struct DebugSettings {
    #[serde(default, alias = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettings {
    #[validate(nested)]
    #[serde(alias = "network")]
    pub network: NetworkSettings,
    #[validate(nested)]
    #[serde(alias = "websocket")]
    pub websocket: WebSocketSettings,
    #[validate(nested)]
    #[serde(alias = "security")]
    pub security: SecuritySettings,
    #[validate(nested)]
    #[serde(alias = "debug")]
    pub debug: DebugSettings,
    #[serde(default, alias = "persist_settings")]
    pub persist_settings: bool,
    #[serde(skip_serializing_if = "Option::is_none", alias = "custom_backend_url")]
    pub custom_backend_url: Option<String>,
}
