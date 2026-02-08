use serde::{Deserialize, Serialize};
use crate::utils::json::{from_json, to_json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeys {
    pub perplexity: Option<String>,
    pub openai: Option<String>,
    pub ragflow: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NostrUser {
    pub pubkey: String,
    pub npub: String,
    pub is_power_user: bool,
    pub api_keys: ApiKeys,
    pub last_seen: i64,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedSettings {
    pub network: NetworkSettings,
    pub security: SecuritySettings,
    pub websocket_server: WebSocketServerSettings,
    pub users: std::collections::HashMap<String, NostrUser>,
    pub default_api_keys: ApiKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    pub bind_address: String,
    pub domain: String,
    pub port: u16,
    pub enable_http2: bool,
    pub enable_tls: bool,
    pub min_tls_version: String,
    pub max_request_size: usize,
    pub enable_rate_limiting: bool,
    pub rate_limit_requests: u32,
    pub rate_limit_window: u32,
    pub tunnel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    pub allowed_origins: Vec<String>,
    pub audit_log_path: String,
    pub cookie_httponly: bool,
    pub cookie_samesite: String,
    pub cookie_secure: bool,
    pub csrf_token_timeout: u32,
    pub enable_audit_logging: bool,
    pub enable_request_validation: bool,
    pub session_timeout: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketServerSettings {
    pub max_connections: usize,
    pub max_message_size: usize,
    pub url: String,
}

impl Default for ApiKeys {
    fn default() -> Self {
        Self {
            perplexity: None,
            openai: None,
            ragflow: None,
        }
    }
}

impl Default for ProtectedSettings {
    fn default() -> Self {
        Self {
            network: NetworkSettings {
                bind_address: "127.0.0.1".to_string(),
                domain: "localhost".to_string(),
                port: 3000,
                enable_http2: true,
                enable_tls: false,
                min_tls_version: "TLS1.2".to_string(),
                max_request_size: 10 * 1024 * 1024, 
                enable_rate_limiting: true,
                rate_limit_requests: 100,
                rate_limit_window: 60,
                tunnel_id: String::new(),
            },
            security: SecuritySettings {
                allowed_origins: vec!["http://localhost:3000".to_string()],
                audit_log_path: "./audit.log".to_string(),
                cookie_httponly: true,
                cookie_samesite: "Lax".to_string(),
                cookie_secure: false,
                csrf_token_timeout: 3600,
                enable_audit_logging: true,
                enable_request_validation: true,
                session_timeout: 86400,
            },
            websocket_server: WebSocketServerSettings {
                max_connections: 100,
                max_message_size: 32 * 1024 * 1024, 
                url: String::new(),
            },
            users: std::collections::HashMap::new(),
            default_api_keys: ApiKeys::default(),
        }
    }
}

impl ProtectedSettings {
    pub fn merge(&mut self, other: serde_json::Value) -> Result<(), String> {
        if let Some(network) = other.get("network") {
            if let Ok(network_settings) = serde_json::from_value(network.clone()) {
                self.network = network_settings;
            }
        }

        if let Some(security) = other.get("security") {
            if let Ok(security_settings) = serde_json::from_value(security.clone()) {
                self.security = security_settings;
            }
        }

        if let Some(websocket) = other.get("websocketServer") {
            if let Ok(websocket_settings) = serde_json::from_value(websocket.clone()) {
                self.websocket_server = websocket_settings;
            }
        }

        if let Some(users) = other.get("users") {
            if let Ok(user_settings) = serde_json::from_value(users.clone()) {
                self.users = user_settings;
            }
        }

        if let Some(api_keys) = other.get("defaultApiKeys") {
            if let Ok(keys) = serde_json::from_value(api_keys.clone()) {
                self.default_api_keys = keys;
            }
        }

        Ok(())
    }

    pub fn get_api_keys(&self, pubkey: &str) -> ApiKeys {
        if let Some(user) = self.users.get(pubkey) {
            if user.is_power_user {
                
                ApiKeys {
                    perplexity: std::env::var("PERPLEXITY_API_KEY").ok(),
                    openai: std::env::var("OPENAI_API_KEY").ok(),
                    ragflow: std::env::var("RAGFLOW_API_KEY").ok(),
                }
            } else {
                
                user.api_keys.clone()
            }
        } else {
            
            self.default_api_keys.clone()
        }
    }

    pub fn validate_client_token(&self, pubkey: &str, token: &str) -> bool {
        if let Some(user) = self.users.get(pubkey) {
            if let Some(session_token) = &user.session_token {
                return session_token == token;
            }
        }
        false
    }

    pub fn store_client_token(&mut self, pubkey: String, token: String) {
        if let Some(user) = self.users.get_mut(&pubkey) {
            user.session_token = Some(token);
            user.last_seen = time::timestamp_seconds();
        }
    }

    pub fn cleanup_expired_tokens(&mut self, max_age_hours: i64) {
        let now = time::timestamp_seconds();
        let max_age_secs = max_age_hours * 3600;

        self.users
            .retain(|_, user| now - user.last_seen < max_age_secs);
    }

    pub fn update_user_api_keys(
        &mut self,
        pubkey: &str,
        api_keys: ApiKeys,
    ) -> Result<NostrUser, String> {
        if let Some(user) = self.users.get_mut(pubkey) {
            if !user.is_power_user {
                user.api_keys = api_keys;
                user.last_seen = time::timestamp_seconds();
                Ok(user.clone())
            } else {
                Err("Cannot update API keys for power users".to_string())
            }
        } else {
            Err("User not found".to_string())
        }
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read protected settings: {}", e))?;

        from_json(&content)
            .map_err(|e| format!("Failed to parse protected settings: {}", e))
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let content = crate::utils::json::to_json_pretty(self)
            .map_err(|e| format!("Failed to serialize protected settings: {}", e))?;

        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write protected settings: {}", e))
    }
}
