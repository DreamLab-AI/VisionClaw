//! Request timeout middleware
//!
//! Ensures all HTTP requests complete within a reasonable timeframe
//! to prevent hanging connections and resource exhaustion.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
};
use futures::future::LocalBoxFuture;
use log::error;
use std::future::{ready, Ready};
use std::time::Duration;

use std::collections::HashMap;

/// Configuration for per-endpoint timeout overrides
#[derive(Clone)]
pub struct TimeoutConfig {
    pub default_timeout: Duration,
    pub endpoint_overrides: HashMap<String, Duration>,
}

impl TimeoutConfig {
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            default_timeout,
            endpoint_overrides: HashMap::new(),
        }
    }

    pub fn with_override(mut self, path: &str, timeout: Duration) -> Self {
        self.endpoint_overrides.insert(path.to_string(), timeout);
        self
    }

    pub fn get_timeout(&self, path: &str) -> Duration {
        self.endpoint_overrides
            .get(path)
            .copied()
            .unwrap_or(self.default_timeout)
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[derive(Default)]
pub struct TimeoutMiddleware {
    config: TimeoutConfig,
}

impl TimeoutMiddleware {
    pub fn new(timeout: Duration) -> Self {
        Self {
            config: TimeoutConfig::new(timeout),
        }
    }

    pub fn with_config(config: TimeoutConfig) -> Self {
        Self { config }
    }
}

impl<S, B> Transform<S, ServiceRequest> for TimeoutMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = TimeoutMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TimeoutMiddlewareService {
            service,
            config: self.config.clone(),
        }))
    }
}

pub struct TimeoutMiddlewareService<S> {
    service: S,
    config: TimeoutConfig,
}

impl<S, B> Service<ServiceRequest> for TimeoutMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path().to_string();
        let timeout_duration = self.config.get_timeout(&path);
        let fut = self.service.call(req);

        Box::pin(async move {
            match tokio::time::timeout(timeout_duration, fut).await {
                Ok(result) => result,
                Err(_) => {
                    error!(
                        "Request to {} timed out after {:?}ms - request exceeded maximum processing time",
                        path,
                        timeout_duration.as_millis()
                    );

                    Err(actix_web::error::ErrorGatewayTimeout(format!(
                        "Request to {} timed out after {}ms",
                        path,
                        timeout_duration.as_millis()
                    )))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App, HttpResponse};

    #[actix_web::test]
    async fn test_timeout_middleware_success() {
        let app = test::init_service(
            App::new()
                .wrap(TimeoutMiddleware::new(Duration::from_secs(5)))
                .route(
                    "/",
                    web::get().to(|| async { HttpResponse::Ok().body("OK") }),
                ),
        )
        .await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_timeout_middleware_timeout() {
        let app = test::init_service(
            App::new()
                .wrap(TimeoutMiddleware::new(Duration::from_millis(100)))
                .route(
                    "/slow",
                    web::get().to(|| async {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        HttpResponse::Ok().body("Never reached")
                    }),
                ),
        )
        .await;

        let req = test::TestRequest::get().uri("/slow").to_request();
        let resp = test::try_call_service(&app, req).await;
        // The middleware returns an error on timeout, so try_call_service returns Err
        assert!(resp.is_err());
    }
}
