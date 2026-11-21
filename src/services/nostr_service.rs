use crate::config::feature_access::FeatureAccess;
use crate::models::protected_settings::{ApiKeys, NostrUser};
use crate::services::session_keys::{
    BindError, SessionKeyStore, SessionLookup, SESSION_KEY_DEFAULT_MAX_TTL_SECS,
};
#[cfg(feature = "redis")]
use crate::utils::json::from_json;
use crate::utils::json::to_json;
use crate::utils::nip98::{
    parse_auth_header, validate_nip98_token, validate_nip98_token_bound, BodyBinding,
    Nip98ValidationResult,
};
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
    /// ADR-2205: a request signed as a session key whose binding is gone
    /// (expired, or lost to a restart). The client re-binds once and retries.
    #[error("session_key_expired")]
    SessionKeyExpired,
    /// ADR-2205: a session key attempted something it is never allowed to do.
    #[error("session_key_scope: {0}")]
    SessionKeyScope(String),
    /// ADR-2205: a bind request was malformed or refused.
    #[error("session key bind refused: {0}")]
    SessionKeyBind(String),
    /// ADR-2205: only established principals (configured power users, or
    /// holders of an explicit RBAC role) may bind a session key.
    #[error("session_key_not_admitted")]
    SessionKeyNotAdmitted,
    /// ADR-2205: too many binds from one principal.
    #[error("session key bind rate limit exceeded")]
    SessionKeyRateLimited,
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
            NostrError::SessionKeyExpired => {
                state.serialize_field("type", "SessionKeyExpired")?;
                state.serialize_field("message", "session_key_expired")?;
            }
            NostrError::SessionKeyScope(msg) => {
                state.serialize_field("type", "SessionKeyScope")?;
                state.serialize_field("message", msg)?;
            }
            NostrError::SessionKeyBind(msg) => {
                state.serialize_field("type", "SessionKeyBind")?;
                state.serialize_field("message", msg)?;
            }
            NostrError::SessionKeyNotAdmitted => {
                state.serialize_field("type", "SessionKeyNotAdmitted")?;
                state.serialize_field("message", "session_key_not_admitted")?;
            }
            NostrError::SessionKeyRateLimited => {
                state.serialize_field("type", "SessionKeyRateLimited")?;
                state.serialize_field("message", "session key bind rate limit exceeded")?;
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

/// Kind of the proof-of-possession event a session key signs over its own bind.
const SESSION_KEY_PROOF_KIND: u16 = 27236;

/// Check a bind's proof of possession (ADR-2205): a NIP-01 event signed BY the
/// session key, of kind 27236, whose tags name the principal, the bind URL and
/// the requested lifetime, with a nonce, created within the NIP-98 freshness
/// window. The enclosing bind token is single-use and body-bound, so the proof
/// cannot be replayed under another principal or terms.
fn verify_bind_proof(
    proof: &str,
    session_pubkey: &str,
    principal: &str,
    request_url: &str,
    expires_in: u64,
    now: i64,
) -> Result<(), &'static str> {
    let event = Event::from_json(proof).map_err(|_| "proof is not a Nostr event")?;
    event.verify().map_err(|_| "bad signature")?;
    if event.kind != Kind::Custom(SESSION_KEY_PROOF_KIND) {
        return Err("wrong kind");
    }
    if !event.pubkey.to_hex().eq_ignore_ascii_case(session_pubkey) {
        return Err("not signed by the session key");
    }
    if (now - event.created_at.as_u64() as i64).abs() > 60 {
        return Err("stale");
    }
    let tag = |name: &str| -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().is_some_and(|k| k == name))
            .and_then(|t| t.as_slice().get(1).cloned())
    };
    if !tag("principal").is_some_and(|v| v.eq_ignore_ascii_case(principal)) {
        return Err("names a different principal");
    }
    if tag("u").as_deref() != Some(request_url) {
        return Err("names a different URL");
    }
    if tag("expires").as_deref() != Some(expires_in.to_string().as_str()) {
        return Err("names a different lifetime");
    }
    if tag("nonce").is_none_or(|n| n.is_empty()) {
        return Err("missing nonce");
    }
    Ok(())
}

/// A NIP-98 verification result (ADR-2205): the authenticated user, which for a
/// session-key request is the key's **principal**, and the signing session key
/// when one was used.
#[derive(Debug, Clone)]
pub struct VerifiedNip98 {
    /// The identity every downstream check (RBAC, owner stamping) must use.
    pub user: NostrUser,
    /// The ephemeral key that signed, when the request came through a binding.
    pub via_session_key: Option<String>,
}

/// What a successful bind granted.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionKeyGrant {
    /// The bound session pubkey (lower-case hex).
    pub session_pubkey: String,
    /// The principal it acts for.
    pub principal: String,
    /// Absolute expiry, Unix seconds.
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct NostrService {
    /// In-memory user cache (always maintained for fast access)
    users: Arc<RwLock<HashMap<String, NostrUser>>>,
    power_user_pubkeys: Vec<String>,
    token_expiry: i64,
    legacy_sessions_enabled: bool,
    /// ADR-2205 bound browser session keys (in memory: a restart forces a re-bind).
    session_keys: Arc<SessionKeyStore>,
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
                        url.rsplit('@').next().unwrap_or(&url)
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
            session_keys: Arc::new(SessionKeyStore::new(
                std::env::var("SESSION_KEY_MAX_TTL_SECS")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(SESSION_KEY_DEFAULT_MAX_TTL_SECS),
            )),
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
            if self.redis_client.is_some() {
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
                    std::io::Error::other(e.to_string()),
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

    /// Test hook: create a user with a fresh session without a signed event.
    #[cfg(test)]
    pub(crate) async fn create_user_for_test(&self, pubkey: &str) -> Result<NostrUser, NostrError> {
        self.get_or_create_user_from_pubkey(pubkey).await
    }

    /// Test hook: treat exactly `pubkeys` as power users, regardless of env.
    #[cfg(test)]
    pub(crate) fn with_power_users(mut self, pubkeys: Vec<String>) -> Self {
        self.power_user_pubkeys = pubkeys;
        self
    }

    /// Test hook: force legacy opaque sessions on or off regardless of env.
    #[cfg(test)]
    pub(crate) fn with_legacy_sessions(mut self, enabled: bool) -> Self {
        self.legacy_sessions_enabled = enabled;
        self
    }

    pub async fn get_user(&self, pubkey: &str) -> Option<NostrUser> {
        let users = self.users.read().await;
        users.get(pubkey).cloned()
    }

    /// Patch the caller's stored provider keys: `None` leaves a key unchanged,
    /// an empty string clears it, any other value replaces it.
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
            // Patch semantics: an absent key is left unchanged, a non-empty
            // string replaces it, an empty string clears it. Saving one
            // provider must not erase the others.
            fn patch(slot: &mut Option<String>, incoming: Option<String>) {
                match incoming {
                    None => {}
                    Some(v) if v.is_empty() => *slot = None,
                    Some(v) => *slot = Some(v),
                }
            }
            patch(&mut user.api_keys.perplexity, api_keys.perplexity);
            patch(&mut user.api_keys.openai, api_keys.openai);
            patch(&mut user.api_keys.ragflow, api_keys.ragflow);
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
            #[cfg(feature = "redis")]
            let updated_user = user.clone();
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
            #[cfg(feature = "redis")]
            let old_token = user.session_token.clone();
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
    /// A token signed by a bound browser session key (ADR-2205) authenticates
    /// as the key's principal, so every caller of this method receives the
    /// principal's [`NostrUser`] (power-user flag included); use
    /// [`Self::verify_nip98_identity`] when the caller must know which.
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
        self.verify_nip98_identity(auth_header, request_url, request_method, request_body)
            .await
            .map(|verified| verified.user)
    }

    /// [`Self::verify_nip98_auth`] that also reports whether a bound session
    /// key signed the request (ADR-2205).
    ///
    /// | token | binding | result |
    /// |---|---|---|
    /// | `session` tag naming P | live, principal P | the user P, `via_session_key` set |
    /// | `session` tag | none live (expired, restart, never bound) | [`NostrError::SessionKeyExpired`] |
    /// | `session` tag naming P | live, principal other than P | `Nip98Error` |
    /// | no tag | live | `Nip98Error`: a bound key only acts as its principal |
    /// | no tag | none | the signer as itself (XR client, agents, plain browsers) |
    pub async fn verify_nip98_identity(
        &self,
        auth_header: &str,
        request_url: &str,
        request_method: &str,
        request_body: Option<&str>,
    ) -> Result<VerifiedNip98, NostrError> {
        let token = parse_auth_header(auth_header).ok_or_else(|| {
            NostrError::Nip98Error("Invalid Authorization header format".to_string())
        })?;

        let validation = validate_nip98_token(token, request_url, request_method, request_body)
            .map_err(|e| NostrError::Nip98Error(e.to_string()))?;

        debug!(
            "NIP-98 token validated for pubkey: {}..., url: {}, method: {}",
            &validation.pubkey[..16.min(validation.pubkey.len())],
            validation.url,
            validation.method
        );

        // Resolve the principal BEFORE creating any user, so an ephemeral
        // session key is never registered as a user in its own right.
        let lookup = self
            .session_keys
            .lookup(&validation.pubkey, time::timestamp_seconds());
        let (principal, via_session_key) = match (validation.session_principal.as_deref(), lookup) {
            (Some(claimed), SessionLookup::Live(binding))
                if binding.principal.eq_ignore_ascii_case(claimed) =>
            {
                (binding.principal, Some(validation.pubkey.clone()))
            }
            (Some(_), SessionLookup::Live(_)) => {
                return Err(NostrError::Nip98Error(
                    "session key is bound to a different principal".to_string(),
                ))
            }
            (Some(_), SessionLookup::Unbound) => return Err(NostrError::SessionKeyExpired),
            (None, SessionLookup::Live(_)) => {
                return Err(NostrError::Nip98Error(
                    "a bound session key must carry the session tag".to_string(),
                ))
            }
            (None, SessionLookup::Unbound) => (validation.pubkey.clone(), None),
        };

        // Get or create the user
        let user = self.get_or_create_user_from_pubkey(&principal).await?;

        info!(
            "NIP-98 auth successful for pubkey: {}, is_power_user: {}, via_session_key: {}",
            user.pubkey,
            user.is_power_user,
            via_session_key.is_some()
        );

        Ok(VerifiedNip98 {
            user,
            via_session_key,
        })
    }

    /// Bind a browser session key to the signer of this request (ADR-2205).
    ///
    /// `auth_header` must be a NIP-98 token over `POST request_url` whose
    /// `payload` tag is the hash of exactly `body`
    /// ([`BodyBinding::Required`]), so a captured token cannot bind a different
    /// key. `body` is `{"sessionPubkey": "<64 hex>", "expiresIn": <secs>}`.
    /// A session key can never bind another session key.
    pub async fn bind_session_key(
        &self,
        auth_header: &str,
        request_url: &str,
        body: &str,
    ) -> Result<SessionKeyGrant, NostrError> {
        let token = parse_auth_header(auth_header).ok_or_else(|| {
            NostrError::Nip98Error("Invalid Authorization header format".to_string())
        })?;
        let validation = validate_nip98_token_bound(
            token,
            request_url,
            "POST",
            Some(body),
            BodyBinding::Required,
        )
        .map_err(|e| NostrError::Nip98Error(e.to_string()))?;

        let now = time::timestamp_seconds();
        if validation.session_principal.is_some()
            || matches!(
                self.session_keys.lookup(&validation.pubkey, now),
                SessionLookup::Live(_)
            )
        {
            return Err(NostrError::SessionKeyScope(
                "a session key cannot bind another session key".to_string(),
            ));
        }

        // Admission: a bind creates server state, so only established
        // principals may bind. Otherwise unlimited fresh keypairs could each
        // become a principal and fill the table. (`approved_pubkeys` is no
        // signal: every authenticating key is added to it.) Checked before any
        // user is created.
        if !self.is_admitted_principal(&validation.pubkey).await {
            return Err(NostrError::SessionKeyNotAdmitted);
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct BindRequest {
            session_pubkey: String,
            expires_in: u64,
            proof: String,
        }
        let request: BindRequest = serde_json::from_str(body)
            .map_err(|e| NostrError::SessionKeyBind(format!("invalid body: {e}")))?;

        // Proof of possession: the session key itself must sign, for this
        // principal, URL and lifetime. Without it anyone could bind a key they
        // do not hold, including a victim's long-term key (targeted lockout).
        verify_bind_proof(
            &request.proof,
            &request.session_pubkey,
            &validation.pubkey,
            request_url,
            request.expires_in,
            now,
        )
        .map_err(|why| NostrError::SessionKeyBind(format!("invalid proof of possession: {why}")))?;

        // Defence in depth: a key that is already an account (has
        // authenticated as itself, or is a configured power user) is never an
        // ephemeral session key.
        let candidate = request.session_pubkey.to_ascii_lowercase();
        if self
            .power_user_pubkeys
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&candidate))
            || self.get_user(&candidate).await.is_some()
        {
            return Err(NostrError::SessionKeyBind(
                "sessionPubkey is an existing account key".to_string(),
            ));
        }

        // The principal's user exists before the binding does, so a failure
        // here can never leave an orphan binding behind.
        self.get_or_create_user_from_pubkey(&validation.pubkey)
            .await?;

        let binding = self
            .session_keys
            .bind(
                &request.session_pubkey,
                &validation.pubkey,
                request.expires_in,
                now,
            )
            .map_err(|e| {
                NostrError::SessionKeyBind(
                    match e {
                        BindError::InvalidSessionPubkey => {
                            "sessionPubkey must be 64 hex characters"
                        }
                        BindError::SessionKeyIsPrincipal => {
                            "sessionPubkey must differ from the signing key"
                        }
                        BindError::BoundToAnotherPrincipal => {
                            "sessionPubkey is already bound to another principal"
                        }
                        BindError::TableFull => "session key table is full",
                        BindError::RateLimited | BindError::GlobalRateLimited => {
                            return NostrError::SessionKeyRateLimited
                        }
                    }
                    .to_string(),
                )
            })?;

        info!(
            "Bound session key {}... to principal {}... until {}",
            &request.session_pubkey[..16],
            &validation.pubkey[..16.min(validation.pubkey.len())],
            binding.expires_at
        );
        Ok(SessionKeyGrant {
            session_pubkey: request.session_pubkey.to_ascii_lowercase(),
            principal: binding.principal,
            expires_at: binding.expires_at,
        })
    }

    /// Whether `pubkey` is an established principal that may bind a session
    /// key: a configured power user, or the holder of an explicit RBAC role.
    async fn is_admitted_principal(&self, pubkey: &str) -> bool {
        if self
            .power_user_pubkeys
            .iter()
            .any(|p| p.eq_ignore_ascii_case(pubkey))
        {
            return true;
        }
        match crate::services::role_store::global_role_store() {
            Some(store) => matches!(store.get(pubkey).await, Ok(Some(_))),
            None => false,
        }
    }

    /// Number of session-key bindings held (live or not yet swept).
    pub fn session_key_count(&self) -> usize {
        self.session_keys.len()
    }

    /// Revoke session-key bindings (ADR-2205). A request signed by a session
    /// key drops that key; one signed by the principal drops all of its keys.
    /// Returns how many bindings were removed.
    pub async fn revoke_session_key(
        &self,
        auth_header: &str,
        request_url: &str,
    ) -> Result<usize, NostrError> {
        let verified = self
            .verify_nip98_identity(auth_header, request_url, "DELETE", None)
            .await?;
        Ok(match verified.via_session_key {
            Some(session_pubkey) => usize::from(self.session_keys.revoke_key(&session_pubkey)),
            None => self.session_keys.revoke_principal(&verified.user.pubkey),
        })
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
