use crate::config::feature_access::FeatureAccess;
use crate::models::protected_settings::{ApiKeys, NostrUser};
use crate::utils::json::to_json;
use crate::utils::nip98::{parse_auth_header, validate_nip98_token, Nip98ValidationResult};
use crate::utils::time;
use log::{debug, error, info, warn};
use nostr_sdk::{event::Error as EventError, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

#[cfg(feature = "redis")]
use redis::{AsyncCommands, Client as RedisClient};

#[derive(Debug, Error)]
pub enum NostrError {
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("User not found")]
    UserNotFound,
    #[error("Invalid token")]
    InvalidToken,
    #[error("Session expired")]
    SessionExpired,
    #[error("Power user operation not allowed")]
    PowerUserOperation,
    #[error("Nostr event error: {0}")]
    NostrError(#[from] EventError),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("NIP-98 validation error: {0}")]
    Nip98Error(String),
}

impl Serialize for NostrError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("NostrError", 2)?;
        match self {
            NostrError::InvalidEvent(msg) => {
                state.serialize_field("type", "InvalidEvent")?;
                state.serialize_field("message", msg)?;
            }
            NostrError::InvalidSignature => {
                state.serialize_field("type", "InvalidSignature")?;
                state.serialize_field("message", "Invalid signature")?;
            }
            NostrError::UserNotFound => {
                state.serialize_field("type", "UserNotFound")?;
                state.serialize_field("message", "User not found")?;
            }
            NostrError::InvalidToken => {
                state.serialize_field("type", "InvalidToken")?;
                state.serialize_field("message", "Invalid token")?;
            }
            NostrError::SessionExpired => {
                state.serialize_field("type", "SessionExpired")?;
                state.serialize_field("message", "Session expired")?;
            }
            NostrError::PowerUserOperation => {
                state.serialize_field("type", "PowerUserOperation")?;
                state.serialize_field("message", "Power user operation not allowed")?;
            }
            NostrError::NostrError(e) => {
                state.serialize_field("type", "NostrError")?;
                state.serialize_field("message", &e.to_string())?;
            }
            NostrError::JsonError(e) => {
                state.serialize_field("type", "JsonError")?;
                state.serialize_field("message", &e.to_string())?;
            }
            NostrError::Nip98Error(msg) => {
                state.serialize_field("type", "Nip98Error")?;
                state.serialize_field("message", msg)?;
            }
        }
        state.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub sig: String,
    pub created_at: i64,
    pub kind: i32,
    pub tags: Vec<Vec<String>>,
}

/// Redis key prefixes for session storage
#[cfg(feature = "redis")]
mod redis_keys {
    /// Session token -> pubkey mapping (for token lookups)
    pub const SESSION_TOKEN: &str = "nostr:session:token:";
    /// Pubkey -> user data mapping (for user storage)
    pub const USER_DATA: &str = "nostr:user:data:";
    /// Pubkey -> session token mapping (for reverse lookups)
    pub const USER_SESSION: &str = "nostr:user:session:";
}

/// Legacy opaque sessions are an explicit migration escape hatch; NIP-98 is
/// the default. Capture this once at service construction, not per request.
fn legacy_sessions_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

#[derive(Clone)]
pub struct NostrService {
    /// In-memory user cache (always maintained for fast access)
    users: Arc<RwLock<HashMap<String, NostrUser>>>,
    power_user_pubkeys: Vec<String>,
    token_expiry: i64,
    legacy_sessions_enabled: bool,
    feature_access: Arc<RwLock<FeatureAccess>>,
    /// Redis client for persistent session storage (optional)
    #[cfg(feature = "redis")]
    redis_client: Option<RedisClient>,
}

impl NostrService {
    pub fn new() -> Self {
        let power_users = std::env::var("POWER_USER_PUBKEYS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let token_expiry = std::env::var("AUTH_TOKEN_EXPIRY")
            .unwrap_or_else(|_| "3600".to_string())
            .parse()
            .unwrap_or(3600);

        let feature_access = Arc::new(RwLock::new(FeatureAccess::from_env()));

        // Initialize Redis client for session persistence
        #[cfg(feature = "redis")]
        let redis_client = match std::env::var("REDIS_URL") {
            Ok(url) => match RedisClient::open(url.clone()) {
                Ok(client) => {
                    info!(
                        "[NostrService] Connected to Redis for session persistence: {}",
                        url.split('@').last().unwrap_or(&url)
                    );
                    Some(client)
                }
                Err(e) => {
                    warn!("[NostrService] Failed to connect to Redis, sessions will be in-memory only: {}", e);
                    None
                }
            },
            Err(_) => {
                debug!("[NostrService] No REDIS_URL configured, sessions will be in-memory only");
                None
            }
        };

        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            power_user_pubkeys: power_users,
            feature_access,
            token_expiry,
            legacy_sessions_enabled: legacy_sessions_enabled(
                std::env::var("VISIONCLAW_LEGACY_SESSIONS").ok().as_deref(),
            ),
            #[cfg(feature = "redis")]
            redis_client,
        }
    }

    /// Initialize service and restore sessions from Redis
    pub async fn initialize(&self) -> Result<usize, NostrError> {
        #[cfg(feature = "redis")]
        {
            if let Some(ref client) = self.redis_client {
                return self.restore_sessions_from_redis().await;
            }
        }
        Ok(0)
    }

    /// Restore all active sessions from Redis on startup
    #[cfg(feature = "redis")]
    async fn restore_sessions_from_redis(&self) -> Result<usize, NostrError> {
        let client = match &self.redis_client {
            Some(c) => c,
            None => return Ok(0),
        };

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| NostrError::InvalidEvent(format!("Redis connection failed: {}", e)))?;

        // Scan for all user data keys
        let pattern = format!("{}*", redis_keys::USER_DATA);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();

        let mut restored_count = 0;
        let mut users = self.users.write().await;

        for key in keys {
            let user_json: Option<String> = conn.get(&key).await.unwrap_or(None);
            if let Some(json) = user_json {
                match from_json::<NostrUser>(&json) {
                    Ok(user) => {
                        // Check if session is still valid (not expired)
                        let now = time::timestamp_seconds();
                        if now - user.last_seen <= self.token_expiry {
                            info!(
                                "[NostrService] Restored session for user: {}",
                                &user.pubkey[..16.min(user.pubkey.len())]
                            );
                            users.insert(user.pubkey.clone(), user);
                            restored_count += 1;
                        } else {
                            debug!(
                                "[NostrService] Skipping expired session for user: {}",
                                &key[redis_keys::USER_DATA.len()..]
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            "[NostrService] Failed to deserialize user from Redis: {}",
                            e
                        );
                    }
                }
            }
        }

        info!(
            "[NostrService] Restored {} active sessions from Redis",
            restored_count
        );
        Ok(restored_count)
    }

    /// Persist user session to Redis
    #[cfg(feature = "redis")]
    async fn persist_session(&self, user: &NostrUser) {
        let client = match &self.redis_client {
            Some(c) => c,
            None => return,
        };

        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "[NostrService] Failed to get Redis connection for session persist: {}",
                    e
                );
                return;
            }
        };

        let user_json = match to_json(user) {
            Ok(j) => j,
            Err(e) => {
                error!("[NostrService] Failed to serialize user for Redis: {}", e);
                return;
            }
        };

        let ttl_secs = self.token_expiry as u64;

        // Store user data with TTL
        let user_key = format!("{}{}", redis_keys::USER_DATA, user.pubkey);
        if let Err(e) = conn
            .set_ex::<_, _, ()>(&user_key, &user_json, ttl_secs)
            .await
        {
            warn!("[NostrService] Failed to store user data in Redis: {}", e);
            return;
        }

        // Store session token -> pubkey mapping for token lookups
        if let Some(ref token) = user.session_token {
            let token_key = format!("{}{}", redis_keys::SESSION_TOKEN, token);
            if let Err(e) = conn
                .set_ex::<_, _, ()>(&token_key, &user.pubkey, ttl_secs)
                .await
            {
                warn!(
                    "[NostrService] Failed to store session token in Redis: {}",
                    e
                );
            }

            // Store pubkey -> session token for reverse lookups
            let session_key = format!("{}{}", redis_keys::USER_SESSION, user.pubkey);
            if let Err(e) = conn.set_ex::<_, _, ()>(&session_key, token, ttl_secs).await {
                warn!(
                    "[NostrService] Failed to store user session mapping in Redis: {}",
                    e
                );
            }
        }

        debug!(
            "[NostrService] Persisted session to Redis for user: {}",
            &user.pubkey[..16.min(user.pubkey.len())]
        );
    }

    /// Remove session from Redis
    #[cfg(feature = "redis")]
    async fn remove_session_from_redis(&self, pubkey: &str, token: Option<&str>) {
        let client = match &self.redis_client {
            Some(c) => c,
            None => return,
        };

        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "[NostrService] Failed to get Redis connection for session removal: {}",
                    e
                );
                return;
            }
        };

        // Remove user data
        let user_key = format!("{}{}", redis_keys::USER_DATA, pubkey);
        let _: Result<(), _> = conn.del(&user_key).await;

        // Remove session token mapping
        if let Some(token) = token {
            let token_key = format!("{}{}", redis_keys::SESSION_TOKEN, token);
            let _: Result<(), _> = conn.del(&token_key).await;
        }

        // Remove user session mapping
        let session_key = format!("{}{}", redis_keys::USER_SESSION, pubkey);
        let _: Result<(), _> = conn.del(&session_key).await;

        debug!(
            "[NostrService] Removed session from Redis for user: {}",
            &pubkey[..16.min(pubkey.len())]
        );
    }

    /// Check if Redis is available for session persistence
    #[cfg(feature = "redis")]
    pub fn has_redis(&self) -> bool {
        self.redis_client.is_some()
    }

    #[cfg(not(feature = "redis"))]
    pub fn has_redis(&self) -> bool {
        false
    }

    pub async fn verify_auth_event(&self, event: AuthEvent) -> Result<NostrUser, NostrError> {
        debug!(
            "Verifying auth event with id: {} and pubkey: {}",
            event.id, event.pubkey
        );

        let json_str = match to_json(&event) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize auth event with id {}: {}", event.id, e);
                return Err(NostrError::JsonError(serde_json::Error::io(
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                )));
            }
        };

        debug!(
            "Event JSON for verification (truncated): {}...",
            if json_str.len() > 100 {
                &json_str[0..100]
            } else {
                &json_str
            }
        );

        let nostr_event = match Event::from_json(&json_str) {
            Ok(e) => e,
            Err(e) => {
                error!(
                    "Failed to parse Nostr event for pubkey {}: {}",
                    event.pubkey, e
                );
                return Err(NostrError::InvalidEvent(format!(
                    "Parse error for event {}: {}",
                    event.id, e
                )));
            }
        };

        if let Err(e) = nostr_event.verify() {
            error!(
                "Signature verification failed for pubkey {}: {}",
                event.pubkey, e
            );
            return Err(NostrError::InvalidSignature);
        }

        let mut feature_access = self.feature_access.write().await;
        if feature_access.register_new_user(&event.pubkey) {
            info!("Registered new user with basic access: {}", event.pubkey);
        }

        let now = time::now();
        let is_power_user = self.power_user_pubkeys.contains(&event.pubkey);

        let session_token = Uuid::new_v4().to_string();

        let user = NostrUser {
            pubkey: event.pubkey.clone(),
            npub: nostr_event
                .pubkey
                .to_bech32()
                .map_err(|_| NostrError::NostrError(EventError::InvalidId))?,
            is_power_user,
            api_keys: ApiKeys::default(),
            last_seen: now.timestamp(),
            session_token: self.legacy_sessions_enabled.then_some(session_token),
        };

        info!(
            "Created/updated user: pubkey={}, is_power_user={}",
            user.pubkey, user.is_power_user
        );

        // Store in memory
        let mut users = self.users.write().await;
        users.insert(user.pubkey.clone(), user.clone());
        drop(users);

        // Persist to Redis for session survival across restarts
        #[cfg(feature = "redis")]
        self.persist_session(&user).await;

        Ok(user)
    }

    pub async fn get_user(&self, pubkey: &str) -> Option<NostrUser> {
        let users = self.users.read().await;
        users.get(pubkey).cloned()
    }

    pub async fn update_user_api_keys(
        &self,
        pubkey: &str,
        api_keys: ApiKeys,
    ) -> Result<NostrUser, NostrError> {
        let mut users = self.users.write().await;

        if let Some(user) = users.get_mut(pubkey) {
            if user.is_power_user {
                return Err(NostrError::PowerUserOperation);
            }
            user.api_keys = api_keys;
            user.last_seen = time::timestamp_seconds();
            let updated_user = user.clone();
            drop(users);

            // Persist updated user to Redis
            #[cfg(feature = "redis")]
            self.persist_session(&updated_user).await;

            Ok(updated_user)
        } else {
            Err(NostrError::UserNotFound)
        }
    }

    pub async fn validate_session(&self, pubkey: &str, token: &str) -> bool {
        if !self.legacy_sessions_enabled || token.is_empty() {
            return false;
        }
        if let Some(user) = self.get_user(pubkey).await {
            if let Some(session_token) = user.session_token {
                let now = time::timestamp_seconds();
                // ADR-2044: one freshness rule, shared with `get_session`.
                if Self::session_is_fresh(user.last_seen, now, self.token_expiry) {
                    return session_token == token;
                }
            }
        }
        false
    }

    pub async fn refresh_session(&self, pubkey: &str) -> Result<String, NostrError> {
        if !self.legacy_sessions_enabled {
            return Err(NostrError::SessionExpired);
        }
        let mut users = self.users.write().await;

        if let Some(user) = users.get_mut(pubkey) {
            // Remove old token from Redis before creating new one
            #[cfg(feature = "redis")]
            let old_token = user.session_token.clone();

            let now = time::timestamp_seconds();
            let new_token = Uuid::new_v4().to_string();
            user.session_token = Some(new_token.clone());
            user.last_seen = now;
            let _updated_user = user.clone();
            drop(users);

            // Persist refreshed session to Redis
            #[cfg(feature = "redis")]
            {
                // Remove old token mapping
                if let Some(ref old) = old_token {
                    self.remove_session_from_redis(pubkey, Some(old)).await;
                }
                self.persist_session(&updated_user).await;
            }

            Ok(new_token)
        } else {
            Err(NostrError::UserNotFound)
        }
    }

    pub async fn logout(&self, pubkey: &str) -> Result<(), NostrError> {
        let mut users = self.users.write().await;

        if let Some(user) = users.get_mut(pubkey) {
            let _old_token = user.session_token.clone();
            user.session_token = None;
            user.last_seen = time::timestamp_seconds();
            drop(users);

            // Remove session from Redis
            #[cfg(feature = "redis")]
            self.remove_session_from_redis(pubkey, old_token.as_deref())
                .await;

            Ok(())
        } else {
            Err(NostrError::UserNotFound)
        }
    }

    pub async fn cleanup_sessions(&self, max_age_hours: i64) {
        let now = time::now();
        let mut users = self.users.write().await;

        users.retain(|_, user| {
            let age = now.timestamp() - user.last_seen;
            age < (max_age_hours * 3600)
        });
    }

    pub async fn is_power_user(&self, pubkey: &str) -> bool {
        if let Some(user) = self.get_user(pubkey).await {
            user.is_power_user
        } else {
            false
        }
    }

    /// Get session by token (looks up user by token).
    ///
    /// ADR-2044: this enforces the SAME `AUTH_TOKEN_EXPIRY` window as
    /// [`Self::validate_session`]. It previously returned any user whose
    /// `session_token` matched, with no expiry check at all, so a token
    /// presented on a WebSocket upgrade stayed valid until logout or overwrite
    /// while the REST path expired it — two different lifetimes for one
    /// credential. Every caller of this function is an authentication path
    /// (`solid_proxy_handler`, the `/wss` upgrade, `filter_auth`,
    /// `agent_events::ingest`), so the check fails closed: an expired token
    /// resolves to `None` exactly as an unknown token does.
    pub async fn get_session(&self, token: &str) -> Option<NostrUser> {
        if !self.legacy_sessions_enabled || token.is_empty() {
            return None;
        }
        let users = self.users.read().await;
        let token_string = token.to_string();
        let now = time::timestamp_seconds();
        users
            .values()
            .find(|user| {
                user.session_token.as_ref() == Some(&token_string)
                    && Self::session_is_fresh(user.last_seen, now, self.token_expiry)
            })
            .cloned()
    }

    /// Is a session still inside its `AUTH_TOKEN_EXPIRY` window?
    ///
    /// ADR-2044: shared by [`Self::get_session`] and [`Self::validate_session`]
    /// so the WebSocket and REST realms cannot drift apart again. A `last_seen`
    /// in the future (clock skew, or a clock stepped backwards) is treated as
    /// stale rather than as an unbounded lease — the age is compared on its
    /// absolute value so neither direction of skew can widen the window.
    fn session_is_fresh(last_seen: i64, now: i64, token_expiry: i64) -> bool {
        now.saturating_sub(last_seen).abs() <= token_expiry
    }

    /// Validate NIP-98 HTTP authentication from Authorization header
    /// This validates tokens for Solid server requests per NIP-98 spec.
    /// Tokens must be signed, unexpired (60s max), and match the request URL/method.
    /// # Arguments
    /// * `auth_header` - Full Authorization header (e.g., "Nostr <base64>")
    /// * `request_url` - The URL the request was made to
    /// * `request_method` - The HTTP method (GET, POST, PUT, etc.)
    /// * `request_body` - Optional request body for payload verification
    /// # Returns
    /// The authenticated NostrUser or an error
    pub async fn verify_nip98_auth(
        &self,
        auth_header: &str,
        request_url: &str,
        request_method: &str,
        request_body: Option<&str>,
    ) -> Result<NostrUser, NostrError> {
        // Parse the Authorization header
        let token = parse_auth_header(auth_header).ok_or_else(|| {
            NostrError::Nip98Error("Invalid Authorization header format".to_string())
        })?;

        // Validate the NIP-98 token
        let validation = validate_nip98_token(token, request_url, request_method, request_body)
            .map_err(|e| NostrError::Nip98Error(e.to_string()))?;

        debug!(
            "NIP-98 token validated for pubkey: {}..., url: {}, method: {}",
            &validation.pubkey[..16.min(validation.pubkey.len())],
            validation.url,
            validation.method
        );

        // Get or create the user
        let user = self
            .get_or_create_user_from_pubkey(&validation.pubkey)
            .await?;

        info!(
            "NIP-98 auth successful for pubkey: {}, is_power_user: {}",
            user.pubkey, user.is_power_user
        );

        Ok(user)
    }

    /// Get existing user or create a new one from pubkey
    async fn get_or_create_user_from_pubkey(&self, pubkey: &str) -> Result<NostrUser, NostrError> {
        // Check if user exists
        if let Some(_user) = self.get_user(pubkey).await {
            // Update last_seen
            let mut users = self.users.write().await;
            if let Some(user) = users.get_mut(pubkey) {
                user.last_seen = time::timestamp_seconds();
                let updated_user = user.clone();
                drop(users);

                // Persist updated last_seen to Redis
                #[cfg(feature = "redis")]
                self.persist_session(&updated_user).await;

                return Ok(updated_user);
            }
            return Err(NostrError::UserNotFound);
        }

        // Register new user with feature access
        let mut feature_access = self.feature_access.write().await;
        if feature_access.register_new_user(pubkey) {
            info!(
                "Registered new user via NIP-98 with basic access: {}",
                pubkey
            );
        }
        drop(feature_access);

        let is_power_user = self.power_user_pubkeys.contains(&pubkey.to_string());
        let session_token = Uuid::new_v4().to_string();

        // Convert hex pubkey to npub (bech32)
        let npub = match PublicKey::from_hex(pubkey) {
            Ok(pk) => pk.to_bech32().unwrap_or_else(|_| pubkey.to_string()),
            Err(_) => {
                warn!("Could not convert pubkey to npub: {}", pubkey);
                pubkey.to_string()
            }
        };

        let user = NostrUser {
            pubkey: pubkey.to_string(),
            npub,
            is_power_user,
            api_keys: ApiKeys::default(),
            last_seen: time::timestamp_seconds(),
            session_token: self.legacy_sessions_enabled.then_some(session_token),
        };

        // Store the new user
        let mut users = self.users.write().await;
        users.insert(pubkey.to_string(), user.clone());
        drop(users);

        // Persist new user session to Redis
        #[cfg(feature = "redis")]
        self.persist_session(&user).await;

        info!(
            "Created new user via NIP-98: pubkey={}, is_power_user={}",
            user.pubkey, user.is_power_user
        );

        Ok(user)
    }

    /// Validate NIP-98 token and return just the validation result (no user creation)
    /// Use this for stateless validation when you only need to verify the token.
    pub fn validate_nip98_token_only(
        &self,
        auth_header: &str,
        request_url: &str,
        request_method: &str,
        request_body: Option<&str>,
    ) -> Result<Nip98ValidationResult, NostrError> {
        let token = parse_auth_header(auth_header).ok_or_else(|| {
            NostrError::Nip98Error("Invalid Authorization header format".to_string())
        })?;

        validate_nip98_token(token, request_url, request_method, request_body)
            .map_err(|e| NostrError::Nip98Error(e.to_string()))
    }
}

impl Default for NostrService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod session_expiry_tests {
    use super::NostrService;

    // ADR-2044: `get_session` and `validate_session` must share one freshness
    // rule. These exercise the shared helper directly — it is pure over
    // (last_seen, now, token_expiry), so the whole matrix is testable without
    // constructing a service or touching Redis.
    const EXPIRY: i64 = 3600;
    const NOW: i64 = 1_757_000_000;

    #[test]
    fn fresh_session_inside_the_window_is_accepted() {
        assert!(NostrService::session_is_fresh(NOW - 1, NOW, EXPIRY));
        assert!(NostrService::session_is_fresh(
            NOW - EXPIRY + 1,
            NOW,
            EXPIRY
        ));
    }

    #[test]
    fn session_exactly_at_the_boundary_is_accepted() {
        assert!(NostrService::session_is_fresh(NOW - EXPIRY, NOW, EXPIRY));
    }

    #[test]
    fn session_past_the_window_is_rejected() {
        assert!(!NostrService::session_is_fresh(
            NOW - EXPIRY - 1,
            NOW,
            EXPIRY
        ));
        assert!(!NostrService::session_is_fresh(
            NOW - 10 * EXPIRY,
            NOW,
            EXPIRY
        ));
    }

    #[test]
    fn future_last_seen_beyond_the_window_is_rejected_not_granted() {
        // A clock stepped backwards must not turn into an unbounded lease:
        // the age is compared on its absolute value.
        assert!(!NostrService::session_is_fresh(
            NOW + EXPIRY + 1,
            NOW,
            EXPIRY
        ));
    }

    #[test]
    fn small_forward_skew_stays_inside_the_window() {
        assert!(NostrService::session_is_fresh(NOW + 1, NOW, EXPIRY));
    }
}

#[cfg(test)]
mod session_sunset_tests {
    use super::*;
    #[test]
    fn compatibility_is_explicit_and_fail_closed() {
        for value in [None, Some(""), Some("false"), Some("garbage")] {
            assert!(!legacy_sessions_enabled(value));
        }
        assert!(legacy_sessions_enabled(Some("1")));
    }
    #[tokio::test]
    async fn disabled_sessions_reject_even_known_fresh_tokens() {
        let mut service = NostrService::new();
        service.legacy_sessions_enabled = false;
        let keys = Keys::generate();
        let mut user = service
            .get_or_create_user_from_pubkey(&keys.public_key().to_hex())
            .await
            .unwrap();
        user.session_token = Some("old-token".into());
        let pubkey = user.pubkey.clone();
        service.users.write().await.insert(pubkey.clone(), user);
        assert!(!service.validate_session(&pubkey, "old-token").await);
        assert!(service.get_session("old-token").await.is_none());
        assert!(service.refresh_session(&pubkey).await.is_err());
        service.legacy_sessions_enabled = true;
        assert!(service.validate_session(&pubkey, "old-token").await);
    }
}
