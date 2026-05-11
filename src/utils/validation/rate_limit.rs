use actix_web::{dev::ServiceRequest, HttpRequest, HttpResponse, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Maximum number of tracked IP addresses to prevent unbounded memory growth
/// SECURITY FIX: Prevents DoS via memory exhaustion from distributed attacks
const MAX_TRACKED_CLIENTS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
    pub cleanup_interval: Duration,
    pub ban_duration: Duration,
    pub max_violations: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            burst_size: 10,
            cleanup_interval: Duration::from_secs(300),
            ban_duration: Duration::from_secs(3600),
            max_violations: 5,
        }
    }
}

#[derive(Debug, Clone)]
struct RateLimitEntry {
    tokens: u32,
    last_refill: Instant,
    violation_count: u32,
    banned_until: Option<Instant>,
}

impl RateLimitEntry {
    fn new(config: &RateLimitConfig) -> Self {
        Self {
            tokens: config.burst_size,
            last_refill: Instant::now(),
            violation_count: 0,
            banned_until: None,
        }
    }

    fn is_banned(&self) -> bool {
        if let Some(banned_until) = self.banned_until {
            Instant::now() < banned_until
        } else {
            false
        }
    }

    fn refill_tokens(&mut self, config: &RateLimitConfig) {
        let now = Instant::now();
        let time_passed = now.duration_since(self.last_refill);
        let tokens_to_add = (time_passed.as_secs() * config.requests_per_minute as u64 / 60) as u32;

        if tokens_to_add > 0 {
            self.tokens = (self.tokens + tokens_to_add).min(config.burst_size);
            self.last_refill = now;
        }
    }

    fn try_consume_token(&mut self, config: &RateLimitConfig) -> bool {
        if self.is_banned() {
            return false;
        }

        self.refill_tokens(config);

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            self.violation_count += 1;

            if self.violation_count >= config.max_violations {
                self.banned_until = Some(Instant::now() + config.ban_duration);
                warn!("Client banned due to rate limit violations");
            }

            false
        }
    }
}

pub struct RateLimiter {
    clients: Arc<RwLock<HashMap<String, RateLimitEntry>>>,
    config: RateLimitConfig,
    last_cleanup: Arc<RwLock<Instant>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        let limiter = Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            config,
            last_cleanup: Arc::new(RwLock::new(Instant::now())),
        };


        limiter.start_cleanup_task();
        limiter
    }

    /// Probabilistic cleanup to supplement the interval-based cleanup.
    /// Called on each request with low probability to amortize cleanup cost.
    fn maybe_probabilistic_cleanup(&self) {
        // ~1% probability using a cheap thread-local counter instead of hashing.
        // The old DefaultHasher approach was replaced (P1-24) and this counter
        // gives better distribution than hashing a near-zero Instant::elapsed().
        use std::cell::Cell;
        thread_local! {
            static COUNTER: Cell<u64> = const { Cell::new(0) };
        }
        COUNTER.with(|c| {
            let val = c.get().wrapping_add(1);
            c.set(val);
            if val % 100 == 0 {
                self.cleanup_if_needed();
            }
        });
    }

    /// Check if the client is allowed to make a request
    pub fn is_allowed(&self, client_id: &str) -> bool {
        // Probabilistic cleanup on each request (1% chance)
        self.maybe_probabilistic_cleanup();

        // Also do interval-based cleanup
        self.cleanup_if_needed();

        match self.clients.write() {
            Ok(mut clients) => {
                // SECURITY FIX: Enforce max tracked clients to prevent memory exhaustion
                if !clients.contains_key(client_id) && clients.len() >= MAX_TRACKED_CLIENTS {
                    // Evict oldest non-banned entry to make room
                    let oldest_key = clients
                        .iter()
                        .filter(|(_, entry)| !entry.is_banned())
                        .min_by_key(|(_, entry)| entry.last_refill)
                        .map(|(key, _)| key.clone());

                    if let Some(key) = oldest_key {
                        debug!("Evicting oldest rate limit entry to stay within bounds: {}", key);
                        clients.remove(&key);
                    } else {
                        // All entries are banned, can't evict - fail open for new client
                        warn!("Rate limiter at capacity with all banned entries, allowing new client");
                        return true;
                    }
                }

                let entry = clients
                    .entry(client_id.to_string())
                    .or_insert_with(|| RateLimitEntry::new(&self.config));

                let allowed = entry.try_consume_token(&self.config);

                if !allowed {
                    warn!("Rate limit exceeded for client: {}", client_id);
                }

                allowed
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (is_allowed): {} - Allowing request", e);
                // Fail open: allow the request to continue
                true
            }
        }
    }

    /// Get the number of remaining tokens for a client
    pub fn remaining_tokens(&self, client_id: &str) -> u32 {
        match self.clients.write() {
            Ok(mut clients) => {
                let entry = clients
                    .entry(client_id.to_string())
                    .or_insert_with(|| RateLimitEntry::new(&self.config));

                entry.refill_tokens(&self.config);
                entry.tokens
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (remaining_tokens): {} - Returning burst size", e);
                self.config.burst_size
            }
        }
    }

    /// Get the time until the next token refill
    pub fn reset_time(&self, client_id: &str) -> Duration {
        match self.clients.read() {
            Ok(clients) => {
                if let Some(entry) = clients.get(client_id) {
                    let time_since_refill = Instant::now().duration_since(entry.last_refill);
                    let time_to_next_token =
                        Duration::from_secs(60 / self.config.requests_per_minute as u64);
                    time_to_next_token.saturating_sub(time_since_refill)
                } else {
                    Duration::from_secs(0)
                }
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (reset_time): {} - Returning 0", e);
                Duration::from_secs(0)
            }
        }
    }

    /// Check if a client is currently banned
    pub fn is_banned(&self, client_id: &str) -> bool {
        match self.clients.read() {
            Ok(clients) => {
                clients
                    .get(client_id)
                    .map(|entry| entry.is_banned())
                    .unwrap_or(false)
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (is_banned): {} - Returning false", e);
                false
            }
        }
    }

    /// Clean up expired entries if needed
    fn cleanup_if_needed(&self) {
        let now = Instant::now();

        // Check if cleanup is needed
        match self.last_cleanup.write() {
            Ok(mut last_cleanup) => {
                if now.duration_since(*last_cleanup) < self.config.cleanup_interval {
                    return;
                }
                *last_cleanup = now;
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (cleanup_if_needed/last_cleanup): {} - Skipping cleanup", e);
                return;
            }
        }

        // Perform cleanup
        match self.clients.write() {
            Ok(mut clients) => {
                let before_count = clients.len();

                clients.retain(|_, entry| {
                    // Keep entries that are either still banned or recently active
                    !entry.is_banned()
                        || entry
                            .banned_until
                            .map(|until| now < until + Duration::from_secs(3600))
                            .unwrap_or(true)
                });

                let after_count = clients.len();
                if before_count != after_count {
                    debug!(
                        "Rate limiter cleanup: removed {} expired entries",
                        before_count - after_count
                    );
                }
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (cleanup_if_needed/clients): {} - Skipping cleanup", e);
            }
        }
    }

    /// Start a background task to periodically clean up expired entries
    fn start_cleanup_task(&self) {
        let clients = self.clients.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.cleanup_interval);

            loop {
                interval.tick().await;

                match clients.write() {
                    Ok(mut clients_guard) => {
                        let before_count = clients_guard.len();
                        let now = Instant::now();

                        clients_guard.retain(|_, entry| {
                            // Keep banned clients within extended grace period
                            let keep_banned = entry
                                .banned_until
                                .map(|until| now < until + Duration::from_secs(3600))
                                .unwrap_or(false);

                            let keep_active =
                                now.duration_since(entry.last_refill) < Duration::from_secs(1800); // 30 min

                            keep_banned || keep_active
                        });

                        let after_count = clients_guard.len();
                        if before_count != after_count {
                            info!(
                                "Rate limiter background cleanup: removed {} expired entries",
                                before_count - after_count
                            );
                        }
                    }
                    Err(e) => {
                        warn!("RwLock poisoned in rate limiter background task: {} - Skipping cleanup cycle", e);
                    }
                }
            }
        });
    }

    /// Get statistics about the rate limiter
    pub fn get_stats(&self) -> RateLimitStats {
        match self.clients.read() {
            Ok(clients) => {
                let now = Instant::now();

                let total_clients = clients.len();
                let banned_clients = clients.values().filter(|entry| entry.is_banned()).count();
                let active_clients = clients
                    .values()
                    .filter(|entry| now.duration_since(entry.last_refill) < Duration::from_secs(300))
                    .count();

                RateLimitStats {
                    total_clients,
                    banned_clients,
                    active_clients,
                    config: self.config.clone(),
                }
            }
            Err(e) => {
                warn!("RwLock poisoned in rate limiter (get_stats): {} - Returning empty stats", e);
                RateLimitStats {
                    total_clients: 0,
                    banned_clients: 0,
                    active_clients: 0,
                    config: self.config.clone(),
                }
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RateLimitStats {
    pub total_clients: usize,
    pub banned_clients: usize,
    pub active_clients: usize,
    pub config: RateLimitConfig,
}

pub fn extract_client_id(req: &HttpRequest) -> String {

    let real_ip = req
        .headers()
        .get("X-Real-IP")
        .or_else(|| req.headers().get("X-Forwarded-For"))
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok());


    let ip = real_ip.or_else(|| req.peer_addr().map(|addr| addr.ip()));

    match ip {
        Some(addr) => addr.to_string(),
        None => "unknown".to_string(),
    }
}

pub fn extract_client_id_from_service_request(req: &ServiceRequest) -> String {
    let real_ip = req
        .headers()
        .get("X-Real-IP")
        .or_else(|| req.headers().get("X-Forwarded-For"))
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok());

    let ip = real_ip.or_else(|| req.peer_addr().map(|addr| addr.ip()));

    match ip {
        Some(addr) => addr.to_string(),
        None => "unknown".to_string(),
    }
}

pub fn create_rate_limit_response(client_id: &str, limiter: &RateLimiter) -> Result<HttpResponse> {
    let remaining = limiter.remaining_tokens(client_id);
    let reset_time = limiter.reset_time(client_id);
    let retry_after = reset_time.as_secs();

    let response =
        if limiter.is_banned(client_id) {
            HttpResponse::TooManyRequests()
            .insert_header(("X-RateLimit-Limit", limiter.config.requests_per_minute.to_string()))
            .insert_header(("X-RateLimit-Remaining", "0"))
            .insert_header(("X-RateLimit-Reset", reset_time.as_secs().to_string()))
            .insert_header(("Retry-After", retry_after.to_string()))
            .json(serde_json::json!({
                "error": "rate_limit_exceeded",
                "message": "Client is temporarily banned due to excessive rate limit violations",
                "retry_after": retry_after
            }))
        } else {
            HttpResponse::TooManyRequests()
                .insert_header((
                    "X-RateLimit-Limit",
                    limiter.config.requests_per_minute.to_string(),
                ))
                .insert_header(("X-RateLimit-Remaining", remaining.to_string()))
                .insert_header(("X-RateLimit-Reset", reset_time.as_secs().to_string()))
                .insert_header(("Retry-After", retry_after.to_string()))
                .json(serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": "Too many requests, please slow down",
                    "retry_after": retry_after
                }))
        };

    Ok(response)
}

pub struct EndpointRateLimits;

impl EndpointRateLimits {

    pub fn settings_update() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 30,
            burst_size: 5,
            ..Default::default()
        }
    }


    pub fn ragflow_chat() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 20,
            burst_size: 3,
            ..Default::default()
        }
    }


    pub fn bots_operations() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 40,
            burst_size: 8,
            ..Default::default()
        }
    }


    pub fn health_check() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 120,
            burst_size: 20,
            ..Default::default()
        }
    }


    pub fn socket_flow_updates() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 300,
            burst_size: 50,
            cleanup_interval: Duration::from_secs(600),
            ban_duration: Duration::from_secs(600),
            max_violations: 10,
        }
    }


    pub fn default() -> RateLimitConfig {
        RateLimitConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let config = RateLimitConfig {
            requests_per_minute: 60,
            burst_size: 5,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let client_id = "test_client";


        for _ in 0..5 {
            assert!(limiter.is_allowed(client_id));
        }


        assert!(!limiter.is_allowed(client_id));
    }

    #[tokio::test]
    async fn test_rate_limiter_refill() {
        let config = RateLimitConfig {
            requests_per_minute: 60,
            burst_size: 1,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let client_id = "test_client_refill";


        assert!(limiter.is_allowed(client_id));
        assert!(!limiter.is_allowed(client_id));


        tokio::time::sleep(Duration::from_secs(2)).await;


        assert!(limiter.is_allowed(client_id));
    }

    #[tokio::test]
    async fn test_ban_after_violations() {
        let config = RateLimitConfig {
            requests_per_minute: 60,
            burst_size: 1,
            max_violations: 2,
            ban_duration: Duration::from_secs(1),
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let client_id = "test_client_ban";


        assert!(limiter.is_allowed(client_id));
        assert!(!limiter.is_allowed(client_id));
        assert!(!limiter.is_allowed(client_id));


        assert!(limiter.is_banned(client_id));
        assert!(!limiter.is_allowed(client_id));
    }
}
