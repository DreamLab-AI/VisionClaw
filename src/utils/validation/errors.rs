use crate::utils::time;
use actix_web::{HttpResponse, ResponseError};
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedValidationError {
    pub error_type: String,
    pub field_path: String,
    pub message: String,
    pub error_code: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
    pub suggestions: Option<Vec<String>>,
    pub timestamp: String,
}

impl DetailedValidationError {
    pub fn new(field_path: &str, message: &str, error_code: &str) -> Self {
        Self {
            error_type: "validation_error".to_string(),
            field_path: field_path.to_string(),
            message: message.to_string(),
            error_code: error_code.to_string(),
            context: None,
            suggestions: None,
            timestamp: time::format_iso8601(&time::now()),
        }
    }

    pub fn with_context(mut self, context: HashMap<String, serde_json::Value>) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = Some(suggestions);
        self
    }

    pub fn invalid_type(field_path: &str, expected: &str, actual: &str) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "expected_type".to_string(),
            serde_json::Value::String(expected.to_string()),
        );
        context.insert(
            "actual_type".to_string(),
            serde_json::Value::String(actual.to_string()),
        );

        Self::new(
            field_path,
            &format!("Expected {}, got {}", expected, actual),
            "INVALID_TYPE",
        )
        .with_context(context)
        .with_suggestions(vec![
            format!(
                "Ensure the field '{}' is of type '{}'",
                field_path, expected
            ),
            "Check your request payload format".to_string(),
        ])
    }

    pub fn out_of_range(field_path: &str, value: f64, min: f64, max: f64) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "value".to_string(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );
        context.insert(
            "min".to_string(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(min).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );
        context.insert(
            "max".to_string(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(max).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );

        Self::new(
            field_path,
            &format!("Value {} is out of range [{}, {}]", value, min, max),
            "OUT_OF_RANGE",
        )
        .with_context(context)
        .with_suggestions(vec![
            format!("Ensure '{}' is between {} and {}", field_path, min, max),
            "Adjust the value to be within the acceptable range".to_string(),
        ])
    }

    pub fn pattern_mismatch(field_path: &str, pattern: &str, value: &str) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "pattern".to_string(),
            serde_json::Value::String(pattern.to_string()),
        );
        context.insert(
            "value".to_string(),
            serde_json::Value::String(value.to_string()),
        );

        Self::new(
            field_path,
            &format!(
                "Value '{}' does not match required pattern: {}",
                value, pattern
            ),
            "PATTERN_MISMATCH",
        )
        .with_context(context)
        .with_suggestions(vec![
            format!("Ensure '{}' matches the pattern: {}", field_path, pattern),
            "Check the format requirements for this field".to_string(),
        ])
    }

    pub fn missing_required_field(field_path: &str) -> Self {
        Self::new(
            field_path,
            "This field is required",
            "REQUIRED_FIELD_MISSING",
        )
        .with_suggestions(vec![
            format!("Add the required field '{}'", field_path),
            "Check the API documentation for required fields".to_string(),
        ])
    }

    pub fn malicious_content(field_path: &str, detected_threat: &str) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "threat_type".to_string(),
            serde_json::Value::String(detected_threat.to_string()),
        );

        Self::new(
            field_path,
            &format!(
                "Potentially malicious content detected: {}",
                detected_threat
            ),
            "MALICIOUS_CONTENT",
        )
        .with_context(context)
        .with_suggestions(vec![
            "Remove any script tags, SQL injection attempts, or path traversal patterns"
                .to_string(),
            "Use only safe, alphanumeric characters where possible".to_string(),
            "Contact support if you believe this is a false positive".to_string(),
        ])
    }

    pub fn rate_limit_exceeded(client_id: &str, limit: u32, window: &str) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "client_id".to_string(),
            serde_json::Value::String(client_id.to_string()),
        );
        context.insert(
            "limit".to_string(),
            serde_json::Value::Number(serde_json::Number::from(limit)),
        );
        context.insert(
            "window".to_string(),
            serde_json::Value::String(window.to_string()),
        );

        Self::new(
            "request",
            &format!("Rate limit of {} requests per {} exceeded", limit, window),
            "RATE_LIMIT_EXCEEDED",
        )
        .with_context(context)
        .with_suggestions(vec![
            "Reduce the frequency of your requests".to_string(),
            "Implement exponential backoff in your client".to_string(),
            "Consider upgrading your rate limit if you need higher throughput".to_string(),
        ])
    }

    pub fn request_too_large(size: usize, max_size: usize) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "request_size".to_string(),
            serde_json::Value::Number(serde_json::Number::from(size)),
        );
        context.insert(
            "max_size".to_string(),
            serde_json::Value::Number(serde_json::Number::from(max_size)),
        );

        Self::new(
            "request",
            &format!(
                "Request size {} bytes exceeds maximum allowed size {} bytes",
                size, max_size
            ),
            "REQUEST_TOO_LARGE",
        )
        .with_context(context)
        .with_suggestions(vec![
            "Reduce the size of your request payload".to_string(),
            "Consider paginating large datasets".to_string(),
            "Remove unnecessary fields from your request".to_string(),
        ])
    }

    pub fn authentication_failed(reason: &str) -> Self {
        Self::new(
            "authentication",
            &format!("Authentication failed: {}", reason),
            "AUTHENTICATION_FAILED",
        )
        .with_suggestions(vec![
            "Check your authentication credentials".to_string(),
            "Ensure your session hasn't expired".to_string(),
            "Verify you have the required permissions".to_string(),
        ])
    }

    pub fn authorization_failed(resource: &str, required_permission: &str) -> Self {
        let mut context = HashMap::new();
        context.insert(
            "resource".to_string(),
            serde_json::Value::String(resource.to_string()),
        );
        context.insert(
            "required_permission".to_string(),
            serde_json::Value::String(required_permission.to_string()),
        );

        Self::new(
            "authorization",
            &format!(
                "Access denied to resource '{}'. Required permission: '{}'",
                resource, required_permission
            ),
            "AUTHORIZATION_FAILED",
        )
        .with_context(context)
        .with_suggestions(vec![
            format!("Ensure you have '{}' permission", required_permission),
            "Contact an administrator to request access".to_string(),
            "Verify you're accessing the correct resource".to_string(),
        ])
    }

    pub fn to_http_response(&self) -> HttpResponse {
        self.error_response()
    }
}

impl std::fmt::Display for DetailedValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.error_code, self.field_path, self.message
        )
    }
}

impl std::error::Error for DetailedValidationError {}

// `ValidationResult` boxes its error; these let `?` lift the unboxed
// sources straight into it.
impl From<crate::utils::validation::ValidationError> for Box<DetailedValidationError> {
    fn from(err: crate::utils::validation::ValidationError) -> Self {
        Box::new(err.into())
    }
}

impl From<ValidationErrorType> for Box<DetailedValidationError> {
    fn from(error_type: ValidationErrorType) -> Self {
        Box::new(error_type.into())
    }
}

impl From<crate::utils::validation::ValidationError> for DetailedValidationError {
    fn from(err: crate::utils::validation::ValidationError) -> Self {
        Self {
            error_type: "validation_error".to_string(),
            field_path: err.field,
            message: err.message,
            error_code: err.error_code,
            context: err.details.map(|details| {
                details
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect()
            }),
            suggestions: None,
            timestamp: time::format_iso8601(&time::now()),
        }
    }
}

impl ResponseError for DetailedValidationError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self.error_code.as_str() {
            "AUTHENTICATION_FAILED" => actix_web::http::StatusCode::UNAUTHORIZED,
            "AUTHORIZATION_FAILED" => actix_web::http::StatusCode::FORBIDDEN,
            "RATE_LIMIT_EXCEEDED" => actix_web::http::StatusCode::TOO_MANY_REQUESTS,
            "REQUEST_TOO_LARGE" => actix_web::http::StatusCode::PAYLOAD_TOO_LARGE,
            "MALICIOUS_CONTENT" => actix_web::http::StatusCode::FORBIDDEN,
            _ => actix_web::http::StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        error!("Validation error: {}", self);

        HttpResponse::build(self.status_code()).json(serde_json::json!({
            "error": self.error_type,
            "field": self.field_path,
            "message": self.message,
            "code": self.error_code,
            "context": self.context,
            "suggestions": self.suggestions,
            "timestamp": self.timestamp,
            "help": "Check the API documentation for detailed field requirements"
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrorCollection {
    pub errors: Vec<DetailedValidationError>,
    pub error_count: usize,
    pub timestamp: String,
}

impl ValidationErrorCollection {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            error_count: 0,
            timestamp: time::format_iso8601(&time::now()),
        }
    }

    pub fn add_error(&mut self, error: DetailedValidationError) {
        self.errors.push(error);
        self.error_count = self.errors.len();
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn has_fatal_errors(&self) -> bool {
        self.errors.iter().any(|e| {
            matches!(
                e.error_code.as_str(),
                "MALICIOUS_CONTENT" | "AUTHENTICATION_FAILED" | "AUTHORIZATION_FAILED"
            )
        })
    }

    pub fn get_field_errors(&self, field_path: &str) -> Vec<&DetailedValidationError> {
        self.errors
            .iter()
            .filter(|e| {
                e.field_path == field_path || e.field_path.starts_with(&format!("{}.", field_path))
            })
            .collect()
    }

    pub fn merge(&mut self, other: ValidationErrorCollection) {
        self.errors.extend(other.errors);
        self.error_count = self.errors.len();
    }
}

impl Default for ValidationErrorCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ValidationErrorCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ValidationErrorCollection with {} errors",
            self.error_count
        )?;
        for (i, error) in self.errors.iter().enumerate() {
            write!(f, "\n  {}: {}", i + 1, error)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrorCollection {}

impl ResponseError for ValidationErrorCollection {
    fn status_code(&self) -> actix_web::http::StatusCode {
        if self.has_fatal_errors() {
            actix_web::http::StatusCode::FORBIDDEN
        } else {
            actix_web::http::StatusCode::BAD_REQUEST
        }
    }

    fn error_response(&self) -> HttpResponse {
        error!("Multiple validation errors: {}", self);

        let grouped_errors = self.group_errors_by_field();

        HttpResponse::build(self.status_code()).json(serde_json::json!({
            "error": "multiple_validation_errors",
            "message": format!("{} validation errors occurred", self.error_count),
            "errors": self.errors,
            "grouped_errors": grouped_errors,
            "error_count": self.error_count,
            "timestamp": self.timestamp,
            "help": "Fix all validation errors and resubmit the request"
        }))
    }
}

impl ValidationErrorCollection {
    fn group_errors_by_field(&self) -> HashMap<String, Vec<&DetailedValidationError>> {
        let mut grouped = HashMap::new();

        for error in &self.errors {
            grouped
                .entry(error.field_path.clone())
                .or_insert_with(Vec::new)
                .push(error);
        }

        grouped
    }
}

#[derive(Debug, Clone)]
pub enum ValidationErrorType {
    Required(String),
    Type {
        field: String,
        expected: String,
        actual: String,
    },
    Range {
        field: String,
        value: f64,
        min: f64,
        max: f64,
    },
    Pattern {
        field: String,
        pattern: String,
        value: String,
    },
    Length {
        field: String,
        length: usize,
        min: Option<usize>,
        max: Option<usize>,
    },
    Custom {
        field: String,
        message: String,
        code: String,
    },
}

impl From<ValidationErrorType> for DetailedValidationError {
    fn from(error_type: ValidationErrorType) -> Self {
        match error_type {
            ValidationErrorType::Required(field) => {
                DetailedValidationError::missing_required_field(&field)
            }
            ValidationErrorType::Type {
                field,
                expected,
                actual,
            } => DetailedValidationError::invalid_type(&field, &expected, &actual),
            ValidationErrorType::Range {
                field,
                value,
                min,
                max,
            } => DetailedValidationError::out_of_range(&field, value, min, max),
            ValidationErrorType::Pattern {
                field,
                pattern,
                value,
            } => DetailedValidationError::pattern_mismatch(&field, &pattern, &value),
            ValidationErrorType::Length {
                field,
                length,
                min,
                max,
            } => {
                let message = match (min, max) {
                    (Some(min_len), Some(max_len)) => {
                        format!(
                            "Length {} is not between {} and {}",
                            length, min_len, max_len
                        )
                    }
                    (Some(min_len), None) => {
                        format!("Length {} is less than minimum {}", length, min_len)
                    }
                    (None, Some(max_len)) => {
                        format!("Length {} exceeds maximum {}", length, max_len)
                    }
                    (None, None) => {
                        format!("Invalid length {}", length)
                    }
                };

                let mut context = HashMap::new();
                context.insert(
                    "length".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(length)),
                );
                if let Some(min_len) = min {
                    context.insert(
                        "min_length".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(min_len)),
                    );
                }
                if let Some(max_len) = max {
                    context.insert(
                        "max_length".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(max_len)),
                    );
                }

                DetailedValidationError::new(&field, &message, "INVALID_LENGTH")
                    .with_context(context)
            }
            ValidationErrorType::Custom {
                field,
                message,
                code,
            } => DetailedValidationError::new(&field, &message, &code),
        }
    }
}
