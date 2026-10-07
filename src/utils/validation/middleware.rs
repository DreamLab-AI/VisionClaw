use crate::utils::validation::rate_limit::{
    create_rate_limit_response, extract_client_id_from_service_request, RateLimitConfig,
    RateLimiter,
};
use crate::utils::validation::sanitization::{CSPUtils, Sanitizer};
use crate::utils::validation::{ValidationError, MAX_REQUEST_SIZE};
use actix_web::web::Bytes;
use actix_web::{
    body::{BoxBody, MessageBody},
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use futures_util::future::LocalBoxFuture;
use log::{debug, info, warn};
use std::rc::Rc;

pub struct RequestSizeLimit {
    max_size: usize,
}

impl RequestSizeLimit {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl Default for RequestSizeLimit {
    fn default() -> Self {
        Self::new(MAX_REQUEST_SIZE)
    }
}

impl<S, B> Transform<S, ServiceRequest> for RequestSizeLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestSizeLimitMiddleware<S>;
    type Future = std::future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        std::future::ready(Ok(RequestSizeLimitMiddleware {
            service,
            max_size: self.max_size,
        }))
    }
}

pub struct RequestSizeLimitMiddleware<S> {
    service: S,
    max_size: usize,
}

impl<S, B> Service<ServiceRequest> for RequestSizeLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let max_size = self.max_size;

        if let Some(content_length) = req.headers().get("content-length") {
            if let Ok(length_str) = content_length.to_str() {
                if let Ok(length) = length_str.parse::<usize>() {
                    if length > max_size {
                        warn!(
                            "Request rejected: Content-Length {} exceeds limit {}",
                            length, max_size
                        );

                        let response = HttpResponse::PayloadTooLarge()
                            .json(serde_json::json!({
                                "error": "request_too_large",
                                "message": format!("Request size {} bytes exceeds limit of {} bytes", length, max_size),
                                "max_size": max_size
                            }));

                        return Box::pin(async move {
                            Ok(req.into_response(response).map_into_boxed_body())
                        });
                    }
                }
            }
        }

        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res.map_into_boxed_body())
        })
    }
}

pub struct SecurityHeaders;

impl<S, B> Transform<S, ServiceRequest> for SecurityHeaders
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = SecurityHeadersMiddleware<S>;
    type Future = std::future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        std::future::ready(Ok(SecurityHeadersMiddleware { service }))
    }
}

pub struct SecurityHeadersMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for SecurityHeadersMiddleware<S>
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
        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;

            let headers = CSPUtils::security_headers();
            for (name, value) in headers {
                let header_name = match actix_web::http::header::HeaderName::from_bytes(
                    name.to_lowercase().as_bytes(),
                ) {
                    Ok(name) => name,
                    Err(_) => continue,
                };
                if let Ok(header_value) = actix_web::http::header::HeaderValue::from_str(value) {
                    res.headers_mut().insert(header_name, header_value);
                }
            }

            res.headers_mut().insert(
                actix_web::http::header::CONTENT_SECURITY_POLICY,
                actix_web::http::header::HeaderValue::from_str(&CSPUtils::generate_csp_header())
                    .unwrap_or_else(|_| {
                        actix_web::http::header::HeaderValue::from_static("default-src 'self'")
                    }),
            );

            Ok(res)
        })
    }
}

pub struct RateLimit {
    limiter: Rc<RateLimiter>,
    #[allow(dead_code)]
    config: RateLimitConfig,
}

impl RateLimit {
    pub fn new(config: RateLimitConfig) -> Self {
        let limiter = RateLimiter::new(config.clone());
        Self {
            limiter: Rc::new(limiter),
            config,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitMiddleware<S>;
    type Future = std::future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        std::future::ready(Ok(RateLimitMiddleware {
            service,
            limiter: self.limiter.clone(),
        }))
    }
}

pub struct RateLimitMiddleware<S> {
    service: S,
    limiter: Rc<RateLimiter>,
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let client_id = extract_client_id_from_service_request(&req);
        let limiter = self.limiter.clone();

        if !limiter.is_allowed(&client_id) {
            let response = create_rate_limit_response(&client_id, &limiter)
                .unwrap_or_else(|_| HttpResponse::TooManyRequests().finish());

            return Box::pin(async move { Ok(req.into_response(response).map_into_boxed_body()) });
        }

        let fut = self.service.call(req);
        Box::pin(async move {
            let mut res = fut.await?;

            let remaining = limiter.remaining_tokens(&client_id);
            let reset_time = limiter.reset_time(&client_id);

            res.headers_mut().insert(
                actix_web::http::header::HeaderName::from_static("x-ratelimit-remaining"),
                actix_web::http::header::HeaderValue::from_str(&remaining.to_string())
                    .expect("Invalid header value"),
            );

            if let Ok(reset_val) =
                actix_web::http::header::HeaderValue::from_str(&reset_time.as_secs().to_string())
            {
                res.headers_mut().insert(
                    actix_web::http::header::HeaderName::from_static("x-ratelimit-reset"),
                    reset_val,
                );
            }

            Ok(res.map_into_boxed_body())
        })
    }
}

pub struct InputSanitizer;

impl<S, B> Transform<S, ServiceRequest> for InputSanitizer
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + Clone + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = InputSanitizerMiddleware<S>;
    type Future = std::future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        std::future::ready(Ok(InputSanitizerMiddleware { service }))
    }
}

pub struct InputSanitizerMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for InputSanitizerMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + Clone + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let is_json = req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.contains("application/json"))
            .unwrap_or(false);

        if !is_json {
            let fut = self.service.call(req);
            return Box::pin(async move { fut.await.map(|res| res.map_into_boxed_body()) });
        }

        let service = self.service.clone();
        Box::pin(async move {
            let payload = req.extract::<Bytes>().await;

            match payload {
                Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                    Ok(mut json_value) => match Sanitizer::sanitize_json(&mut json_value) {
                        Ok(()) => match serde_json::to_vec(&json_value) {
                            Ok(sanitized_bytes) => {
                                let new_payload = actix_web::dev::Payload::from(sanitized_bytes);
                                req.set_payload(new_payload);

                                debug!("Request payload sanitized successfully");
                            }
                            Err(e) => {
                                warn!("Failed to re-serialize sanitized JSON: {}", e);
                                let response =
                                    HttpResponse::BadRequest().json(ValidationError::new(
                                        "payload",
                                        "Failed to process request payload",
                                        "SERIALIZATION_ERROR",
                                    ));
                                return Ok(req.into_response(response));
                            }
                        },
                        Err(validation_error) => {
                            warn!("Input sanitization failed: {}", validation_error);
                            return Ok(req.into_response(validation_error.to_http_response()));
                        }
                    },
                    Err(e) => {
                        debug!(
                            "Request payload is not valid JSON ({}), skipping sanitization",
                            e
                        );
                    }
                },
                Err(e) => {
                    warn!("Failed to extract request payload: {}", e);
                }
            }

            service.call(req).await.map(|res| res.map_into_boxed_body())
        })
    }
}

pub struct ValidationMiddlewareFactory;

impl ValidationMiddlewareFactory {
    pub fn create_api_middleware() -> RequestSizeLimit {
        RequestSizeLimit::default()
    }

    pub fn create_settings_middleware() -> RateLimit {
        use crate::utils::validation::rate_limit::EndpointRateLimits;

        RateLimit::new(EndpointRateLimits::settings_update())
    }

    pub fn create_ragflow_middleware() -> RateLimit {
        use crate::utils::validation::rate_limit::EndpointRateLimits;

        RateLimit::new(EndpointRateLimits::ragflow_chat())
    }
}

pub struct ValidationLogging;

impl<S, B> Transform<S, ServiceRequest> for ValidationLogging
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ValidationLoggingMiddleware<S>;
    type Future = std::future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        std::future::ready(Ok(ValidationLoggingMiddleware { service }))
    }
}

pub struct ValidationLoggingMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for ValidationLoggingMiddleware<S>
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
        let method = req.method().clone();
        let uri = req.uri().clone();
        let client_id = extract_client_id_from_service_request(&req);

        debug!(
            "Validation middleware processing request: {} {} from {}",
            method, uri, client_id
        );

        let fut = self.service.call(req);

        Box::pin(async move {
            let start_time = std::time::Instant::now();
            let res = fut.await;
            let duration = start_time.elapsed();

            match &res {
                Ok(response) => {
                    let status = response.status();
                    if status.is_client_error() || status.is_server_error() {
                        warn!(
                            "Request failed: {} {} -> {} ({}ms) from {}",
                            method,
                            uri,
                            status,
                            duration.as_millis(),
                            client_id
                        );
                    } else {
                        info!(
                            "Request processed: {} {} -> {} ({}ms) from {}",
                            method,
                            uri,
                            status,
                            duration.as_millis(),
                            client_id
                        );
                    }
                }
                Err(error) => {
                    warn!(
                        "Request error: {} {} -> error: {} ({}ms) from {}",
                        method,
                        uri,
                        error,
                        duration.as_millis(),
                        client_id
                    );
                }
            }

            res
        })
    }
}
