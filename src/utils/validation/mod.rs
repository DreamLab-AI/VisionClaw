pub mod errors;
pub mod middleware;
pub mod position_validator;
pub mod rate_limit;
pub mod sanitization;
pub mod schemas;

use errors::DetailedValidationError;

use actix_web::HttpResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_REQUEST_SIZE: usize = 16 * 1024 * 1024;

pub const MAX_STRING_LENGTH: usize = 10_000;

pub const MAX_ARRAY_SIZE: usize = 1000;

pub const MAX_NESTING_DEPTH: usize = 10;

/// Result of a validation step. The error is boxed: `DetailedValidationError`
/// is ~192 bytes and would otherwise bloat every `Ok` path that carries it.
pub type ValidationResult<T> = Result<T, Box<DetailedValidationError>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub error_code: String,
    pub details: Option<HashMap<String, String>>,
}

impl ValidationError {
    pub fn new(field: &str, message: &str, error_code: &str) -> Self {
        Self {
            field: field.to_string(),
            message: message.to_string(),
            error_code: error_code.to_string(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: HashMap<String, String>) -> Self {
        self.details = Some(details);
        self
    }

    pub fn invalid_format(field: &str) -> Self {
        Self::new(field, "Invalid format", "INVALID_FORMAT")
    }

    pub fn required_field(field: &str) -> Self {
        Self::new(field, "Field is required", "REQUIRED_FIELD")
    }

    pub fn out_of_range(field: &str, min: f64, max: f64) -> Self {
        let message = format!("Value must be between {} and {}", min, max);
        Self::new(field, &message, "OUT_OF_RANGE")
    }

    pub fn too_long(field: &str, max_length: usize) -> Self {
        let message = format!("Maximum length is {} characters", max_length);
        Self::new(field, &message, "TOO_LONG")
    }

    pub fn invalid_pattern(field: &str, pattern: &str) -> Self {
        let message = format!("Must match pattern: {}", pattern);
        Self::new(field, &message, "INVALID_PATTERN")
    }

    pub fn malicious_content(field: &str) -> Self {
        Self::new(
            field,
            "Content contains potentially malicious data",
            "MALICIOUS_CONTENT",
        )
    }

    pub fn to_http_response(&self) -> HttpResponse {
        HttpResponse::BadRequest().json(serde_json::json!({
            "error": "validation_failed",
            "field": self.field,
            "message": self.message,
            "code": self.error_code,
            "details": self.details
        }))
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Validation error in field '{}': {}",
            self.field, self.message
        )
    }
}

impl std::error::Error for ValidationError {}

impl actix_web::ResponseError for ValidationError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self.error_code.as_str() {
            "REQUIRED_FIELD" | "INVALID_FORMAT" | "TOO_LONG" | "TOO_SHORT" | "OUT_OF_RANGE" => {
                actix_web::http::StatusCode::BAD_REQUEST
            }
            "UNAUTHORIZED" => actix_web::http::StatusCode::UNAUTHORIZED,
            "FORBIDDEN" => actix_web::http::StatusCode::FORBIDDEN,
            _ => actix_web::http::StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(serde_json::json!({
            "error": "validation_error",
            "field": self.field,
            "message": self.message,
            "error_code": self.error_code,
            "details": self.details
        }))
    }
}

#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub max_depth: usize,
    pub current_depth: usize,
    pub field_path: Vec<String>,
}

impl ValidationContext {
    pub fn new() -> Self {
        Self {
            max_depth: MAX_NESTING_DEPTH,
            current_depth: 0,
            field_path: Vec::new(),
        }
    }

    pub fn enter_field(&mut self, field: &str) -> ValidationResult<()> {
        if self.current_depth >= self.max_depth {
            return Err(DetailedValidationError::new(
                &self.get_path(),
                "Maximum nesting depth exceeded",
                "MAX_DEPTH_EXCEEDED",
            )
            .into());
        }
        self.field_path.push(field.to_string());
        self.current_depth += 1;
        Ok(())
    }

    pub fn exit_field(&mut self) {
        if !self.field_path.is_empty() {
            self.field_path.pop();
            self.current_depth = self.current_depth.saturating_sub(1);
        }
    }

    pub fn get_path(&self) -> String {
        if self.field_path.is_empty() {
            "root".to_string()
        } else {
            self.field_path.join(".")
        }
    }
}

impl Default for ValidationContext {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Validator<T> {
    fn validate(&self, value: &T, ctx: &mut ValidationContext) -> ValidationResult<()>;
}

pub struct ValidationUtils;

impl ValidationUtils {
    pub fn validate_string_length(
        value: &str,
        max_length: usize,
        field: &str,
    ) -> ValidationResult<()> {
        if value.len() > max_length {
            return Err(DetailedValidationError::new(
                field,
                &format!("Maximum length is {} characters", max_length),
                "TOO_LONG",
            )
            .into());
        }
        Ok(())
    }

    pub fn validate_numeric_range<T>(value: T, min: T, max: T, field: &str) -> ValidationResult<()>
    where
        T: PartialOrd + Copy + Into<f64>,
    {
        if value < min || value > max {
            return Err(DetailedValidationError::out_of_range(
                field,
                value.into(),
                min.into(),
                max.into(),
            )
            .into());
        }
        Ok(())
    }

    pub fn validate_required<'a, T>(value: &'a Option<T>, field: &str) -> ValidationResult<&'a T> {
        match value {
            Some(v) => Ok(v),
            None => Err(DetailedValidationError::missing_required_field(field).into()),
        }
    }

    pub fn validate_array_size<T>(
        array: &[T],
        max_size: usize,
        field: &str,
    ) -> ValidationResult<()> {
        if array.len() > max_size {
            return Err(DetailedValidationError::new(
                field,
                &format!("Array exceeds maximum size of {}", max_size),
                "ARRAY_TOO_LARGE",
            )
            .into());
        }
        Ok(())
    }

    pub fn validate_email(email: &str, field: &str) -> ValidationResult<()> {
        let email_regex = regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
            .map_err(|_| {
                DetailedValidationError::new(field, "Invalid email regex", "REGEX_ERROR")
            })?;

        if !email_regex.is_match(email) {
            return Err(DetailedValidationError::pattern_mismatch(
                field,
                "valid email address",
                email,
            )
            .into());
        }
        Ok(())
    }

    pub fn validate_url(url: &str, field: &str) -> ValidationResult<()> {
        if url.parse::<url::Url>().is_err() {
            return Err(DetailedValidationError::pattern_mismatch(field, "valid URL", url).into());
        }
        Ok(())
    }

    pub fn validate_hex_color(color: &str, field: &str) -> ValidationResult<()> {
        let hex_regex = regex::Regex::new(r"^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$").map_err(|_| {
            DetailedValidationError::new(field, "Invalid color regex", "REGEX_ERROR")
        })?;

        if !hex_regex.is_match(color) {
            return Err(DetailedValidationError::pattern_mismatch(
                field,
                "hex color (e.g., #ffffff)",
                color,
            )
            .into());
        }
        Ok(())
    }

    pub fn validate_uuid(uuid: &str, field: &str) -> ValidationResult<()> {
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(
                DetailedValidationError::pattern_mismatch(field, "valid UUID", uuid).into(),
            );
        }
        Ok(())
    }
}
