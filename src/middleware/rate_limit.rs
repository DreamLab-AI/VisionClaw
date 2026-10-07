//! Rate Limiting Middleware
//!
//! Protects endpoints from abuse and DoS attacks by limiting the number of requests
//! from a single IP address or user within a time window.
//!
//! ## Features
//! - Sliding window rate limiting algorithm
//! - Per-IP or per-user rate limiting
//! - Configurable limits per endpoint type
//! - In-memory storage with automatic cleanup
//! - 429 Too Many Requests response when exceeded
//!
//! ## Example
//! ```rust,ignore
//! use actix_web::{web, App};
//! use visionclaw::middleware::RateLimit;
//!
//! App::new()
//!     .service(
//!         web::scope("/api")
//!             .wrap(RateLimit::per_minute(100))  // 100 requests per minute
//!             .route("/data", web::get().to(handler))
//!     );
//! ```

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorTooManyRequests,
    Error, HttpMessage,
};
use futures::future::LocalBoxFuture;
use std::collections::{HashMap, VecDeque};
use std::future::{ready, Ready};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed
    pub max_requests: usize,

    /// Time window for the rate limit
    pub window: Duration,

    /// Whether to use authenticated user ID instead of IP
    pub use_user_id: bool,

    /// Custom error message
    pub error_message: Option<String>,
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            use_user_id: false,
            error_message: None,
        }
    }

    /// Use authenticated user ID for rate limiting instead of IP
    pub fn with_user_id(mut self) -> Self {
        self.use_user_id = true;
        self
    }

    /// Set a custom error message
    pub fn with_message(mut self, message: String) -> Self {
        self.error_message = Some(message);
        self
    }
}

/// Rate limiter middleware.
///
/// Clones share one request history (the state is behind an `Arc`), so a
/// limiter built once outside `HttpServer::new` enforces one budget across
/// every worker instead of one per worker.
#[derive(Clone)]
pub struct RateLimit {
    config: RateLimitConfig,
    state: Arc<RwLock<RateLimitState>>,
}

/// Internal state for tracking request counts
#[derive(Debug)]
struct RateLimitState {
    /// Map of identifier (IP or user ID) to request history (bounded by max_requests)
    requests: HashMap<String, VecDeque<Instant>>,

    /// Last cleanup time
    last_cleanup: Instant,
}

impl RateLimitState {
    fn new() -> Self {
        Self {
            requests: HashMap::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Check if a request should be allowed and record it.
    /// Uses VecDeque with bounded capacity to prevent unbounded memory growth under flood.
    fn check_and_record(&mut self, identifier: &str, config: &RateLimitConfig) -> bool {
        let now = Instant::now();
        let window_start = now - config.window;

        // Get or create request history for this identifier
        let history = self.requests.entry(identifier.to_string()).or_default();

        // Pop expired entries from the front (oldest first)
        while let Some(&front) = history.front() {
            if front <= window_start {
                history.pop_front();
            } else {
                break;
            }
        }

        // Check if we're under the limit
        if history.len() < config.max_requests {
            history.push_back(now);
            true
        } else {
            false
        }
    }

    /// Clean up old entries to prevent memory growth
    fn cleanup(&mut self, config: &RateLimitConfig) {
        let now = Instant::now();

        // Only cleanup every minute
        if now.duration_since(self.last_cleanup) < Duration::from_secs(60) {
            return;
        }

        let window_start = now - config.window - Duration::from_secs(60); // Extra margin

        // Remove identifiers with no recent requests
        self.requests.retain(|_, history| {
            history.retain(|&timestamp| timestamp > window_start);
            !history.is_empty()
        });

        self.last_cleanup = now;

        debug!(
            "Rate limit cleanup: {} active identifiers",
            self.requests.len()
        );
    }
}

impl RateLimit {
    /// Create a new rate limiter with custom configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RateLimitState::new())),
        }
    }

    /// Create a rate limiter allowing N requests per minute
    pub fn per_minute(max_requests: usize) -> Self {
        Self::new(RateLimitConfig::new(max_requests, Duration::from_secs(60)))
    }

    /// Create a rate limiter allowing N requests per hour
    pub fn per_hour(max_requests: usize) -> Self {
        Self::new(RateLimitConfig::new(
            max_requests,
            Duration::from_secs(3600),
        ))
    }

    /// Create a rate limiter allowing N requests per second (for very restrictive endpoints)
    pub fn per_second(max_requests: usize) -> Self {
        Self::new(RateLimitConfig::new(max_requests, Duration::from_secs(1)))
    }
}

impl Default for RateLimit {
    /// Create a default rate limiter (100 requests per minute)
    fn default() -> Self {
        Self::per_minute(100)
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimitService {
            service: Arc::new(service),
            config: self.config.clone(),
            state: Arc::clone(&self.state),
        }))
    }
}

pub struct RateLimitService<S> {
    service: Arc<S>,
    config: RateLimitConfig,
    state: Arc<RwLock<RateLimitState>>,
}

impl<S, B> Service<ServiceRequest> for RateLimitService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Arc::clone(&self.service);
        let config = self.config.clone();
        let state = Arc::clone(&self.state);

        Box::pin(async move {
            // Extract identifier (IP or user ID)
            let identifier = {
                if config.use_user_id {
                    if let Some(user) = req
                        .extensions()
                        .get::<crate::middleware::AuthenticatedUser>()
                    {
                        user.pubkey.clone()
                    } else {
                        req.connection_info()
                            .realip_remote_addr()
                            .unwrap_or("unknown")
                            .to_string()
                    }
                } else {
                    req.connection_info()
                        .realip_remote_addr()
                        .unwrap_or("unknown")
                        .to_string()
                }
            };

            // Check rate limit
            let allowed = {
                let mut state = state.write().await;

                // Periodic cleanup
                state.cleanup(&config);

                // Check and record request
                state.check_and_record(&identifier, &config)
            };

            if !allowed {
                warn!(
                    "Rate limit exceeded for {}: {} requests per {:?}",
                    identifier, config.max_requests, config.window
                );

                let error_msg = config.error_message.unwrap_or_else(|| {
                    format!(
                        "Rate limit exceeded: maximum {} requests per {:?}",
                        config.max_requests, config.window
                    )
                });

                return Err(ErrorTooManyRequests(error_msg));
            }

            debug!(
                "Rate limit check passed for {} (limit: {} per {:?})",
                identifier, config.max_requests, config.window
            );

            // Allow request to proceed
            service.call(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App, HttpResponse};

    #[actix_web::test]
    async fn test_rate_limit_allows_under_limit() {
        let app = test::init_service(
            App::new()
                .wrap(RateLimit::per_second(5)) // 5 requests per second
                .route(
                    "/",
                    web::get().to(|| async { HttpResponse::Ok().body("OK") }),
                ),
        )
        .await;

        // Make 5 requests - all should succeed
        for _ in 0..5 {
            let req = test::TestRequest::get().uri("/").to_request();
            let resp = test::call_service(&app, req).await;
            assert!(resp.status().is_success());
        }
    }

    #[actix_web::test]
    async fn test_rate_limit_blocks_over_limit() {
        let app = test::init_service(
            App::new()
                .wrap(RateLimit::per_second(2)) // 2 requests per second
                .route(
                    "/",
                    web::get().to(|| async { HttpResponse::Ok().body("OK") }),
                ),
        )
        .await;

        // Make 2 requests - should succeed
        for _ in 0..2 {
            let req = test::TestRequest::get().uri("/").to_request();
            let resp = test::try_call_service(&app, req).await;
            assert!(resp.is_ok());
            assert!(resp.unwrap().status().is_success());
        }

        // 3rd request should be blocked
        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::try_call_service(&app, req).await;
        // The middleware returns an error, so try_call_service returns Err
        assert!(resp.is_err());
    }

    #[actix_web::test]
    async fn test_rate_limit_sliding_window() {
        let app = test::init_service(
            App::new()
                .wrap(RateLimit::new(RateLimitConfig::new(
                    2,
                    Duration::from_millis(500),
                )))
                .route(
                    "/",
                    web::get().to(|| async { HttpResponse::Ok().body("OK") }),
                ),
        )
        .await;

        // Make 2 requests - should succeed
        for _ in 0..2 {
            let req = test::TestRequest::get().uri("/").to_request();
            let resp = test::call_service(&app, req).await;
            assert!(resp.status().is_success());
        }

        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(600)).await;

        // Should be able to make 2 more requests
        for _ in 0..2 {
            let req = test::TestRequest::get().uri("/").to_request();
            let resp = test::call_service(&app, req).await;
            assert!(resp.status().is_success());
        }
    }

    #[actix_web::test]
    async fn test_rate_limit_custom_message() {
        let config = RateLimitConfig::new(1, Duration::from_secs(1))
            .with_message("Custom error message".to_string());

        let app = test::init_service(App::new().wrap(RateLimit::new(config)).route(
            "/",
            web::get().to(|| async { HttpResponse::Ok().body("OK") }),
        ))
        .await;

        // First request succeeds
        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::try_call_service(&app, req).await;
        assert!(resp.is_ok());

        // Second request should be blocked with custom message
        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::try_call_service(&app, req).await;
        // The middleware returns an error, so try_call_service returns Err
        assert!(resp.is_err());
    }
}
