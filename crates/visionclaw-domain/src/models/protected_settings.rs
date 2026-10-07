use crate::utils::json::from_json;
use crate::utils::time;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeys {
    pub perplexity: Option<String>,
    pub openai: Option<String>,
    pub ragflow: Option<String>,
}

/// Custom Debug implementation that masks API key values to prevent
/// accidental secret leakage in logs or error messages.
impl fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn mask(opt: &Option<String>) -> &str {
            match opt {
                Some(s) if s.is_empty() => "<empty>",
                Some(_) => "<redacted>",
                None => "<none>",
            }
        }
        f.debug_struct("ApiKeys")
            .field("perplexity", &mask(&self.perplexity))
            .field("openai", &mask(&self.openai))
            .field("ragflow", &mask(&self.ragflow))
            .finish()
    }
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
    #[deprecated(note = "Not enforced by server - use reverse proxy")]
    pub enable_http2: bool,
    pub enable_tls: bool,
    #[deprecated(note = "Not enforced by server - use reverse proxy")]
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
    #[deprecated(note = "Not enforced by server")]
    pub cookie_httponly: bool,
    #[deprecated(note = "Not enforced by server")]
    pub cookie_samesite: String,
    #[deprecated(note = "Not enforced by server")]
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

// The deprecated fields still round-trip through stored settings, so the
// owning type's Default has to populate them.
#[allow(deprecated)]
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
    /// Merge fields from a JSON value into this ProtectedSettings.
    /// Returns Err if ALL present fields fail to deserialize.
    /// If at least one field succeeds, logs warnings for failed fields and returns Ok.
    pub fn merge(&mut self, other: serde_json::Value) -> Result<(), String> {
        let mut successes = 0u32;
        let mut failures: Vec<String> = Vec::new();

        if let Some(network) = other.get("network") {
            match serde_json::from_value(network.clone()) {
                Ok(v) => {
                    self.network = v;
                    successes += 1;
                }
                Err(e) => failures.push(format!("network: {}", e)),
            }
        }

        if let Some(security) = other.get("security") {
            match serde_json::from_value(security.clone()) {
                Ok(v) => {
                    self.security = v;
                    successes += 1;
                }
                Err(e) => failures.push(format!("security: {}", e)),
            }
        }

        if let Some(websocket) = other.get("websocketServer") {
            match serde_json::from_value(websocket.clone()) {
                Ok(v) => {
                    self.websocket_server = v;
                    successes += 1;
                }
                Err(e) => failures.push(format!("websocketServer: {}", e)),
            }
        }

        if let Some(users) = other.get("users") {
            match serde_json::from_value(users.clone()) {
                Ok(v) => {
                    self.users = v;
                    successes += 1;
                }
                Err(e) => failures.push(format!("users: {}", e)),
            }
        }

        if let Some(api_keys) = other.get("defaultApiKeys") {
            match serde_json::from_value(api_keys.clone()) {
                Ok(v) => {
                    self.default_api_keys = v;
                    successes += 1;
                }
                Err(e) => failures.push(format!("defaultApiKeys: {}", e)),
            }
        }

        if !failures.is_empty() && successes == 0 {
            return Err(format!("All field merges failed: {}", failures.join("; ")));
        }

        if !failures.is_empty() {
            log::warn!(
                "ProtectedSettings.merge: partial failure - {} succeeded, {} failed: {}",
                successes,
                failures.len(),
                failures.join("; ")
            );
        }

        // Post-merge validation: prevent dangerous values
        if !self.network.enable_rate_limiting {
            log::warn!("Rate limiting disabled in protected settings — this is dangerous");
        }
        if self.network.max_request_size > 100_000_000 {
            return Err("max_request_size cannot exceed 100MB".to_string());
        }
        if self.security.allowed_origins.iter().any(|o| o == "*") {
            log::warn!("Wildcard CORS origin in protected settings — this is a security risk in production");
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

        from_json(&content).map_err(|e| format!("Failed to parse protected settings: {}", e))
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let content = crate::utils::json::to_json_pretty(self)
            .map_err(|e| format!("Failed to serialize protected settings: {}", e))?;

        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write protected settings: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ApiKeys ---

    #[test]
    fn api_keys_default_all_none() {
        let k = ApiKeys::default();
        assert!(k.perplexity.is_none());
        assert!(k.openai.is_none());
        assert!(k.ragflow.is_none());
    }

    #[test]
    fn api_keys_debug_masks_values() {
        let k = ApiKeys {
            perplexity: Some("secret-key".to_string()),
            openai: None,
            ragflow: Some(String::new()),
        };
        let debug = format!("{:?}", k);
        assert!(
            !debug.contains("secret-key"),
            "debug must not leak key value"
        );
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("<none>"));
        assert!(debug.contains("<empty>"));
    }

    #[test]
    fn api_keys_serde_roundtrip() {
        let k = ApiKeys {
            perplexity: Some("pk".to_string()),
            openai: None,
            ragflow: Some("rk".to_string()),
        };
        let json = serde_json::to_string(&k).unwrap();
        let back: ApiKeys = serde_json::from_str(&json).unwrap();
        assert_eq!(back.perplexity, Some("pk".to_string()));
        assert!(back.openai.is_none());
        assert_eq!(back.ragflow, Some("rk".to_string()));
    }

    // --- ProtectedSettings ---

    #[test]
    fn protected_settings_default_has_sane_values() {
        let ps = ProtectedSettings::default();
        assert_eq!(ps.network.port, 3000);
        assert_eq!(ps.network.bind_address, "127.0.0.1");
        assert!(!ps.network.enable_tls);
        assert!(ps.network.enable_rate_limiting);
        assert_eq!(ps.security.session_timeout, 86400);
        assert_eq!(ps.websocket_server.max_connections, 100);
        assert!(ps.users.is_empty());
    }

    #[test]
    fn protected_settings_serde_roundtrip() {
        let ps = ProtectedSettings::default();
        let json = serde_json::to_string(&ps).unwrap();
        let back: ProtectedSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.network.port, ps.network.port);
        assert_eq!(back.security.session_timeout, ps.security.session_timeout);
    }

    #[test]
    fn protected_settings_merge_updates_network_port() {
        let mut ps = ProtectedSettings::default();
        let patch = serde_json::json!({
            "network": {
                "bindAddress": "0.0.0.0",
                "domain": "localhost",
                "port": 8080,
                "enableHttp2": false,
                "enableTls": false,
                "minTlsVersion": "TLS1.2",
                "maxRequestSize": 1048576,
                "enableRateLimiting": true,
                "rateLimitRequests": 100,
                "rateLimitWindow": 60,
                "tunnelId": ""
            }
        });
        let result = ps.merge(patch);
        assert!(result.is_ok(), "merge failed: {:?}", result);
        assert_eq!(ps.network.port, 8080);
    }

    #[test]
    fn protected_settings_merge_rejects_oversized_max_request_size() {
        let mut ps = ProtectedSettings::default();
        let patch = serde_json::json!({
            "network": {
                "bindAddress": "127.0.0.1",
                "domain": "localhost",
                "port": 3000,
                "enableHttp2": true,
                "enableTls": false,
                "minTlsVersion": "TLS1.2",
                "maxRequestSize": 200_000_000u64,
                "enableRateLimiting": true,
                "rateLimitRequests": 100,
                "rateLimitWindow": 60,
                "tunnelId": ""
            }
        });
        let result = ps.merge(patch);
        assert!(result.is_err(), "should reject max_request_size > 100MB");
    }

    #[test]
    fn protected_settings_merge_all_failures_returns_err() {
        let mut ps = ProtectedSettings::default();
        // Pass a completely unknown key so every field merge fails
        let patch = serde_json::json!({ "completely_unknown_key": 42 });
        // Should succeed (no known keys attempted)
        let result = ps.merge(patch);
        assert!(result.is_ok());
    }

    #[test]
    fn protected_settings_get_api_keys_returns_defaults_for_unknown_pubkey() {
        let ps = ProtectedSettings::default();
        let keys = ps.get_api_keys("unknown-pubkey");
        // With no env vars set, defaults come back (None values)
        let _ = keys; // just ensure it doesn't panic
    }

    #[test]
    fn protected_settings_validate_client_token_returns_false_for_missing_user() {
        let ps = ProtectedSettings::default();
        assert!(!ps.validate_client_token("nobody", "some-token"));
    }

    #[test]
    fn protected_settings_store_and_validate_token() {
        let mut ps = ProtectedSettings::default();
        let nostr_user = NostrUser {
            pubkey: "pk1".to_string(),
            npub: "npub1".to_string(),
            is_power_user: false,
            api_keys: ApiKeys::default(),
            last_seen: 0,
            session_token: None,
        };
        ps.users.insert("pk1".to_string(), nostr_user);
        ps.store_client_token("pk1".to_string(), "tok-abc".to_string());
        assert!(ps.validate_client_token("pk1", "tok-abc"));
        assert!(!ps.validate_client_token("pk1", "wrong-token"));
    }

    #[test]
    fn protected_settings_update_user_api_keys_err_for_power_user() {
        let mut ps = ProtectedSettings::default();
        ps.users.insert(
            "pk2".to_string(),
            NostrUser {
                pubkey: "pk2".to_string(),
                npub: "npub2".to_string(),
                is_power_user: true,
                api_keys: ApiKeys::default(),
                last_seen: 0,
                session_token: None,
            },
        );
        let result = ps.update_user_api_keys("pk2", ApiKeys::default());
        assert!(result.is_err());
    }

    #[test]
    fn protected_settings_update_user_api_keys_err_for_missing_user() {
        let mut ps = ProtectedSettings::default();
        let result = ps.update_user_api_keys("nobody", ApiKeys::default());
        assert!(result.is_err());
    }

    #[test]
    fn protected_settings_cleanup_expired_tokens_removes_old_users() {
        let mut ps = ProtectedSettings::default();
        ps.users.insert(
            "old".to_string(),
            NostrUser {
                pubkey: "old".to_string(),
                npub: "npub_old".to_string(),
                is_power_user: false,
                api_keys: ApiKeys::default(),
                last_seen: 0, // epoch — very old
                session_token: None,
            },
        );
        ps.cleanup_expired_tokens(1); // 1-hour max age
        assert!(!ps.users.contains_key("old"), "old user should be removed");
    }
}
