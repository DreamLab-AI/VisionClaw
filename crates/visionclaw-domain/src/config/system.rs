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

/// `/wss` socket settings. Only what the socket server reads lives here: the
/// heartbeat. The position-stream rate is `PhysicsSettings::broadcast_fps`.
/// Keys retired on 2026-10-07 because nothing read them (`binaryChunkSize`,
/// `binaryUpdateRate`, `minUpdateRate`, `maxUpdateRate`, `motionThreshold`,
/// `motionDamping`, `binaryMessageVersion`, `compressionEnabled`,
/// `compressionThreshold`, `maxConnections`, `maxMessageSize`,
/// `reconnectAttempts`, `reconnectDelay`, `updateRate`) are ignored if an
/// older settings file still carries them.
#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketSettings {
    /// Server ping interval in ms (raised to at least 1000).
    #[serde(default = "default_heartbeat_interval", alias = "heartbeat_interval")]
    pub heartbeat_interval: u64,
    /// Close a socket after this many ms with no inbound frame (raised to at
    /// least two intervals).
    #[serde(default = "default_heartbeat_timeout", alias = "heartbeat_timeout")]
    pub heartbeat_timeout: u64,
}

fn default_heartbeat_interval() -> u64 {
    10_000
}

fn default_heartbeat_timeout() -> u64 {
    600_000
}

impl Default for WebSocketSettings {
    fn default() -> Self {
        Self {
            heartbeat_interval: default_heartbeat_interval(),
            heartbeat_timeout: default_heartbeat_timeout(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `system.websocket` holds only settings the socket server reads: the
    /// heartbeat ping interval and idle timeout. The update-rate and motion
    /// knobs are gone (the one position-stream rate is physics.broadcastFps),
    /// as are the never-read chunk, compression, reconnect and limit fields.
    #[test]
    fn websocket_settings_hold_only_the_heartbeat() {
        let json = serde_json::to_value(WebSocketSettings::default()).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["heartbeatInterval", "heartbeatTimeout"]);
        let d = WebSocketSettings::default();
        assert_eq!(
            (d.heartbeat_interval, d.heartbeat_timeout),
            (10_000, 600_000)
        );
    }

    /// A settings.yaml written before the removal still loads: retired keys
    /// are ignored and missing heartbeat keys take their defaults.
    #[test]
    fn retired_websocket_keys_still_deserialise() {
        let legacy = serde_json::json!({
            "binaryChunkSize": 2048, "binaryUpdateRate": 30, "minUpdateRate": 5,
            "maxUpdateRate": 60, "motionThreshold": 0.05, "motionDamping": 0.9,
            "binaryMessageVersion": 1, "compressionEnabled": false,
            "compressionThreshold": 512, "heartbeatInterval": 15000,
            "maxConnections": 100, "maxMessageSize": 10485760,
            "reconnectAttempts": 5, "reconnectDelay": 1000, "updateRate": 60
        });
        let ws: WebSocketSettings = serde_json::from_value(legacy).unwrap();
        assert_eq!(ws.heartbeat_interval, 15_000);
        assert_eq!(ws.heartbeat_timeout, 600_000);
        let snake: WebSocketSettings =
            serde_json::from_value(serde_json::json!({ "heartbeat_timeout": 30000 })).unwrap();
        assert_eq!(snake.heartbeat_timeout, 30_000);
    }
}
