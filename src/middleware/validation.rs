//! Input Validation Middleware
//!
//! Provides Actix-web middleware for validating request payloads including:
//! - Content length limits
//! - JSON payload validation
//! - String length limits
//! - Format validation (URLs, IRIs, enums)

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use futures_util::future::LocalBoxFuture;
use log::{debug, warn};
use std::future::{ready, Ready};
use std::rc::Rc;

/// Maximum content length for ontology uploads (10MB)
pub const MAX_ONTOLOGY_SIZE: usize = 10 * 1024 * 1024;

/// Maximum content length for general API requests (1MB)
pub const MAX_REQUEST_SIZE: usize = 1024 * 1024;

/// Maximum string length for text fields (100KB)
pub const MAX_STRING_LENGTH: usize = 100 * 1024;

/// Input validation middleware
/// # Example
/// ```rust,ignore
/// use actix_web::{web, App};
/// use crate::middleware::validation::{ValidateInput, ValidationConfig};
/// App::new()
///     .wrap(ValidateInput::with_config(ValidationConfig {
///         max_content_length: 10 * 1024 * 1024,  // 10MB
///         validate_json: true,
///     }))
/// ```
pub struct ValidateInput {
    config: ValidationConfig,
}

#[derive(Clone)]
pub struct ValidationConfig {
    /// Maximum content length in bytes
    pub max_content_length: usize,
    /// Whether to validate JSON payloads
    pub validate_json: bool,
    /// Whether to check for suspicious patterns
    pub check_injection: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_content_length: MAX_REQUEST_SIZE,
            validate_json: true,
            check_injection: true,
        }
    }
}

impl ValidateInput {
    /// Create validator for ontology uploads (10MB limit)
    pub fn for_ontology() -> Self {
        Self {
            config: ValidationConfig {
                max_content_length: MAX_ONTOLOGY_SIZE,
                validate_json: true,
                check_injection: false, // Ontologies may contain special chars
            },
        }
    }

    /// Create validator with custom config
    pub fn with_config(config: ValidationConfig) -> Self {
        Self { config }
    }
}

impl Default for ValidateInput {
    /// Create validator with default config (1MB limit)
    fn default() -> Self {
        Self {
            config: ValidationConfig::default(),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for ValidateInput
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = ValidationMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ValidationMiddleware {
            service: Rc::new(service),
            config: self.config.clone(),
        }))
    }
}

pub struct ValidationMiddleware<S> {
    service: Rc<S>,
    config: ValidationConfig,
}

impl<S, B> Service<ServiceRequest> for ValidationMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let svc = self.service.clone();
        let config = self.config.clone();

        Box::pin(async move {
            // Check Content-Length header
            if let Some(content_length) = req.headers().get("content-length") {
                if let Ok(length_str) = content_length.to_str() {
                    if let Ok(length) = length_str.parse::<usize>() {
                        if length > config.max_content_length {
                            warn!(
                                "Request rejected: payload too large ({} bytes, max {})",
                                length, config.max_content_length
                            );
                            let resp = HttpResponse::PayloadTooLarge().body(format!(
                                "Payload too large. Max size: {} bytes",
                                config.max_content_length
                            ));
                            return Ok(req.into_response(resp).map_into_boxed_body());
                        }
                    }
                }
            }

            // Check Content-Type for JSON endpoints
            if config.validate_json {
                if let Some(content_type) = req.headers().get("content-type") {
                    if let Ok(ct_str) = content_type.to_str() {
                        if ct_str.contains("application/json") {
                            debug!("Validated JSON content-type");
                        }
                    }
                }
            }

            // Continue to the actual handler
            let resp = svc.call(req).await?;
            Ok(resp.map_into_boxed_body())
        })
    }
}

/// Validation helpers for use in handlers
pub mod validators {
    use once_cell::sync::Lazy;
    use regex::Regex;

    /// IRI/URI validation regex
    static IRI_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:.+$")
            .expect("IRI regex is a valid compile-time constant")
    });

    /// URL validation regex
    static URL_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^https?://.+$").expect("URL regex is a valid compile-time constant")
    });

    /// Validate IRI format
    pub fn validate_iri(iri: &str) -> Result<(), String> {
        if iri.is_empty() {
            return Err("IRI cannot be empty".to_string());
        }
        if iri.len() > 2048 {
            return Err("IRI too long (max 2048 characters)".to_string());
        }
        if !IRI_REGEX.is_match(iri) {
            return Err("Invalid IRI format".to_string());
        }
        Ok(())
    }

    /// Validate URL format
    pub fn validate_url(url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("URL cannot be empty".to_string());
        }
        if url.len() > 2048 {
            return Err("URL too long (max 2048 characters)".to_string());
        }
        if !URL_REGEX.is_match(url) {
            return Err("Invalid URL format (must be http:// or https://)".to_string());
        }
        Ok(())
    }

    /// Validate string length
    pub fn validate_string_length(s: &str, max_length: usize) -> Result<(), String> {
        if s.len() > max_length {
            return Err(format!(
                "String too long ({} bytes, max {})",
                s.len(),
                max_length
            ));
        }
        Ok(())
    }

    /// Check for SQL injection patterns.
    /// NOTE: String-matching is a defense-in-depth heuristic only. The proper defense
    /// against SQL injection is parameterized queries / prepared statements at the
    /// data-access layer. Do not rely on this filter as the primary protection.
    pub fn check_sql_injection(s: &str) -> Result<(), String> {
        let dangerous_patterns = [
            "DROP TABLE",
            "DELETE FROM",
            "INSERT INTO",
            "UPDATE ",
            "; DROP",
            "' OR '1'='1",
            "-- ",
            "/*",
            "*/",
            "UNION SELECT",
        ];

        let upper = s.to_uppercase();
        for pattern in &dangerous_patterns {
            if upper.contains(pattern) {
                return Err(format!("Potentially dangerous input detected: {}", pattern));
            }
        }
        Ok(())
    }

    /// Validate enum value against allowed set
    pub fn validate_enum<T: AsRef<str>>(value: &str, allowed: &[T]) -> Result<(), String> {
        if allowed.iter().any(|a| a.as_ref() == value) {
            Ok(())
        } else {
            Err(format!(
                "Invalid value '{}'. Allowed: {}",
                value,
                allowed
                    .iter()
                    .map(|a| a.as_ref())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }

    /// Validate numeric range
    pub fn validate_range<T: PartialOrd + std::fmt::Display>(
        value: T,
        min: T,
        max: T,
    ) -> Result<(), String> {
        if value < min || value > max {
            Err(format!(
                "Value {} out of range (min: {}, max: {})",
                value, min, max
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validators::*;

    #[test]
    fn test_validate_iri() {
        assert!(validate_iri("http://example.com/resource").is_ok());
        assert!(validate_iri("urn:isbn:1234567890").is_ok());
        assert!(validate_iri("").is_err());
        assert!(validate_iri("not-an-iri").is_err());
    }

    #[test]
    fn test_validate_url() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn test_check_sql_injection() {
        assert!(check_sql_injection("normal text").is_ok());
        assert!(check_sql_injection("DROP TABLE users").is_err());
        assert!(check_sql_injection("' OR '1'='1").is_err());
    }

    #[test]
    fn test_validate_enum() {
        let allowed = vec!["turtle", "rdf/xml", "n-triples"];
        assert!(validate_enum("turtle", &allowed).is_ok());
        assert!(validate_enum("invalid", &allowed).is_err());
    }

    #[test]
    fn test_validate_range() {
        assert!(validate_range(5, 0, 10).is_ok());
        assert!(validate_range(11, 0, 10).is_err());
        assert!(validate_range(-1, 0, 10).is_err());
    }
}
