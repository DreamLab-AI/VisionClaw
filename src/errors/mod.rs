//! Comprehensive error handling for the VisionClaw system
//!
//! This module provides a unified error handling approach to replace
//! all panic! and unwrap() calls with proper error propagation.

use serde::ser::{Serialize, Serializer};
use std::fmt;

fn serialize_io_error<S>(
    error: &std::sync::Arc<std::io::Error>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    error.to_string().serialize(serializer)
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum VisionClawError {
    Actor(ActorError),

    GPU(GPUError),

    Settings(SettingsError),

    Network(NetworkError),

    #[serde(serialize_with = "serialize_io_error")]
    IO(std::sync::Arc<std::io::Error>),

    Serialization(String),

    Speech(SpeechError),

    GitHub(GitHubError),

    Audio(AudioError),

    Resource(ResourceError),

    Performance(PerformanceError),

    Protocol(ProtocolError),

    Database(DatabaseError),

    Validation(ValidationError),

    Parse(ParseError),

    Generic {
        message: String,
        #[serde(skip)]
        source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync + 'static>>,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ActorError {
    StartupFailed {
        actor_name: String,
        reason: String,
    },

    RuntimeFailure {
        actor_name: String,
        reason: String,
    },

    MessageHandlingFailed {
        message_type: String,
        reason: String,
    },

    SupervisionFailed {
        supervisor: String,
        supervised: String,
        reason: String,
    },

    MailboxError {
        actor_name: String,
        reason: String,
    },

    ActorNotAvailable(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum GPUError {
    DeviceInitializationFailed(String),

    MemoryAllocationFailed {
        requested_bytes: usize,
        reason: String,
    },

    KernelExecutionFailed {
        kernel_name: String,
        reason: String,
    },

    DataTransferFailed {
        direction: DataTransferDirection,
        reason: String,
    },

    FallbackToCPU {
        reason: String,
    },

    DriverError(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum DataTransferDirection {
    CPUToGPU,
    GPUToCPU,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SettingsError {
    FileNotFound(String),

    ParseError {
        file_path: String,
        reason: String,
    },

    ValidationFailed {
        setting_path: String,
        reason: String,
    },

    SaveFailed {
        file_path: String,
        reason: String,
    },

    CacheError(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum NetworkError {
    ConnectionFailed {
        host: String,
        port: u16,
        reason: String,
    },

    WebSocketError(String),

    MCPError {
        method: String,
        reason: String,
    },

    HTTPError {
        url: String,
        status: Option<u16>,
        reason: String,
    },

    RequestFailed {
        url: String,
        reason: String,
    },

    Timeout {
        operation: String,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SpeechError {
    InitializationFailed(String),

    TTSFailed { text: String, reason: String },

    STTFailed { reason: String },

    AudioProcessingFailed { reason: String },

    ProviderConfigError { provider: String, reason: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum GitHubError {
    APIRequestFailed {
        url: String,
        status: Option<u16>,
        reason: String,
    },

    AuthenticationFailed(String),

    FileOperationFailed {
        path: String,
        operation: String,
        reason: String,
    },

    BranchOperationFailed {
        branch: String,
        reason: String,
    },

    PullRequestFailed {
        reason: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum AudioError {
    FormatValidationFailed { format: String, reason: String },

    WAVHeaderValidationFailed(String),

    DataProcessingFailed(String),

    JSONProcessingFailed(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ResourceError {
    MonitoringFailed(String),

    AvailabilityCheckFailed(String),

    FileDescriptorLimit { current: usize, limit: usize },

    MemoryLimit { current: u64, limit: u64 },

    ProcessLimit { current: usize, limit: usize },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum PerformanceError {
    BenchmarkFailed {
        benchmark_name: String,
        reason: String,
    },

    ReportGenerationFailed(String),

    MetricCollectionFailed {
        metric: String,
        reason: String,
    },

    ComparisonFailed(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ProtocolError {
    EncodingFailed { data_type: String, reason: String },

    DecodingFailed { data_type: String, reason: String },

    ValidationFailed(String),

    BinaryFormatError(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum DatabaseError {
    ConnectionFailed { database: String, reason: String },

    QueryFailed { query: String, reason: String },

    TransactionFailed { reason: String },

    NotFound { entity: String, id: String },

    ConstraintViolation { constraint: String, reason: String },

    MigrationFailed { version: String, reason: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ValidationError {
    FieldValidation {
        field: String,
        reason: String,
    },

    RequiredField {
        field: String,
    },

    InvalidFormat {
        field: String,
        expected: String,
        actual: String,
    },

    OutOfRange {
        field: String,
        min: String,
        max: String,
        actual: String,
    },

    InvalidLength {
        field: String,
        min: Option<usize>,
        max: Option<usize>,
        actual: usize,
    },

    Custom(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ParseError {
    JSON {
        input: String,
        reason: String,
    },

    TOML {
        input: String,
        reason: String,
    },

    YAML {
        input: String,
        reason: String,
    },

    Integer {
        input: String,
        reason: String,
    },

    Float {
        input: String,
        reason: String,
    },

    Boolean {
        input: String,
    },

    URL {
        input: String,
        reason: String,
    },

    DateTime {
        input: String,
        reason: String,
    },

    Custom {
        format: String,
        input: String,
        reason: String,
    },
}

impl fmt::Display for VisionClawError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VisionClawError::Actor(e) => write!(f, "Actor Error: {}", e),
            VisionClawError::GPU(e) => write!(f, "GPU Error: {}", e),
            VisionClawError::Settings(e) => write!(f, "Settings Error: {}", e),
            VisionClawError::Network(e) => write!(f, "Network Error: {}", e),
            VisionClawError::IO(e) => write!(f, "IO Error: {}", e),
            VisionClawError::Serialization(e) => write!(f, "Serialization Error: {}", e),
            VisionClawError::Speech(e) => write!(f, "Speech Error: {}", e),
            VisionClawError::GitHub(e) => write!(f, "GitHub Error: {}", e),
            VisionClawError::Audio(e) => write!(f, "Audio Error: {}", e),
            VisionClawError::Resource(e) => write!(f, "Resource Error: {}", e),
            VisionClawError::Performance(e) => write!(f, "Performance Error: {}", e),
            VisionClawError::Protocol(e) => write!(f, "Protocol Error: {}", e),
            VisionClawError::Database(e) => write!(f, "Database Error: {}", e),
            VisionClawError::Validation(e) => write!(f, "Validation Error: {}", e),
            VisionClawError::Parse(e) => write!(f, "Parse Error: {}", e),
            VisionClawError::Generic { message, .. } => write!(f, "Error: {}", message),
        }
    }
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ActorError::StartupFailed { actor_name, reason } => {
                write!(f, "Actor '{}' failed to start: {}", actor_name, reason)
            }
            ActorError::RuntimeFailure { actor_name, reason } => {
                write!(f, "Actor '{}' runtime failure: {}", actor_name, reason)
            }
            ActorError::MessageHandlingFailed {
                message_type,
                reason,
            } => write!(f, "Failed to handle '{}' message: {}", message_type, reason),
            ActorError::SupervisionFailed {
                supervisor,
                supervised,
                reason,
            } => write!(
                f,
                "Supervisor '{}' failed to supervise '{}': {}",
                supervisor, supervised, reason
            ),
            ActorError::MailboxError { actor_name, reason } => {
                write!(f, "Mailbox error for actor '{}': {}", actor_name, reason)
            }
            ActorError::ActorNotAvailable(actor_name) => {
                write!(f, "Actor '{}' is not available", actor_name)
            }
        }
    }
}

impl fmt::Display for GPUError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GPUError::DeviceInitializationFailed(reason) => {
                write!(f, "GPU device initialization failed: {}", reason)
            }
            GPUError::MemoryAllocationFailed {
                requested_bytes,
                reason,
            } => write!(
                f,
                "GPU memory allocation failed ({} bytes): {}",
                requested_bytes, reason
            ),
            GPUError::KernelExecutionFailed {
                kernel_name,
                reason,
            } => write!(
                f,
                "GPU kernel '{}' execution failed: {}",
                kernel_name, reason
            ),
            GPUError::DataTransferFailed { direction, reason } => {
                write!(f, "GPU data transfer failed ({:?}): {}", direction, reason)
            }
            GPUError::FallbackToCPU { reason } => {
                write!(f, "Falling back to CPU computation: {}", reason)
            }
            GPUError::DriverError(reason) => write!(f, "GPU driver error: {}", reason),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SettingsError::FileNotFound(path) => write!(f, "Settings file not found: {}", path),
            SettingsError::ParseError { file_path, reason } => write!(
                f,
                "Failed to parse settings file '{}': {}",
                file_path, reason
            ),
            SettingsError::ValidationFailed {
                setting_path,
                reason,
            } => write!(
                f,
                "Settings validation failed for '{}': {}",
                setting_path, reason
            ),
            SettingsError::SaveFailed { file_path, reason } => {
                write!(f, "Failed to save settings to '{}': {}", file_path, reason)
            }
            SettingsError::CacheError(reason) => write!(f, "Settings cache error: {}", reason),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NetworkError::ConnectionFailed { host, port, reason } => {
                write!(f, "Connection to {}:{} failed: {}", host, port, reason)
            }
            NetworkError::WebSocketError(reason) => write!(f, "WebSocket error: {}", reason),
            NetworkError::MCPError { method, reason } => {
                write!(f, "MCP method '{}' failed: {}", method, reason)
            }
            NetworkError::HTTPError {
                url,
                status,
                reason,
            } => write!(
                f,
                "HTTP error for '{}' (status: {:?}): {}",
                url, status, reason
            ),
            NetworkError::Timeout {
                operation,
                timeout_ms,
            } => write!(
                f,
                "Timeout after {}ms for operation: {}",
                timeout_ms, operation
            ),
            NetworkError::RequestFailed { url, reason } => {
                write!(f, "Request to '{}' failed: {}", url, reason)
            }
        }
    }
}

impl fmt::Display for SpeechError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SpeechError::InitializationFailed(reason) => {
                write!(f, "Speech service initialization failed: {}", reason)
            }
            SpeechError::TTSFailed { text, reason } => {
                write!(f, "Text-to-speech failed for '{}': {}", text, reason)
            }
            SpeechError::STTFailed { reason } => write!(f, "Speech-to-text failed: {}", reason),
            SpeechError::AudioProcessingFailed { reason } => {
                write!(f, "Audio processing failed: {}", reason)
            }
            SpeechError::ProviderConfigError { provider, reason } => write!(
                f,
                "Speech provider '{}' configuration error: {}",
                provider, reason
            ),
        }
    }
}

impl fmt::Display for GitHubError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GitHubError::APIRequestFailed {
                url,
                status,
                reason,
            } => write!(
                f,
                "GitHub API request to '{}' failed (status: {:?}): {}",
                url, status, reason
            ),
            GitHubError::AuthenticationFailed(reason) => {
                write!(f, "GitHub authentication failed: {}", reason)
            }
            GitHubError::FileOperationFailed {
                path,
                operation,
                reason,
            } => write!(
                f,
                "GitHub file operation '{}' on '{}' failed: {}",
                operation, path, reason
            ),
            GitHubError::BranchOperationFailed { branch, reason } => write!(
                f,
                "GitHub branch operation on '{}' failed: {}",
                branch, reason
            ),
            GitHubError::PullRequestFailed { reason } => {
                write!(f, "GitHub pull request failed: {}", reason)
            }
        }
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AudioError::FormatValidationFailed { format, reason } => {
                write!(f, "Audio format '{}' validation failed: {}", format, reason)
            }
            AudioError::WAVHeaderValidationFailed(reason) => {
                write!(f, "WAV header validation failed: {}", reason)
            }
            AudioError::DataProcessingFailed(reason) => {
                write!(f, "Audio data processing failed: {}", reason)
            }
            AudioError::JSONProcessingFailed(reason) => {
                write!(f, "Audio JSON processing failed: {}", reason)
            }
        }
    }
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResourceError::MonitoringFailed(reason) => {
                write!(f, "Resource monitoring failed: {}", reason)
            }
            ResourceError::AvailabilityCheckFailed(reason) => {
                write!(f, "Resource availability check failed: {}", reason)
            }
            ResourceError::FileDescriptorLimit { current, limit } => {
                write!(f, "File descriptor limit reached: {}/{}", current, limit)
            }
            ResourceError::MemoryLimit { current, limit } => {
                write!(f, "Memory limit reached: {} bytes/{} bytes", current, limit)
            }
            ResourceError::ProcessLimit { current, limit } => {
                write!(f, "Process limit reached: {}/{}", current, limit)
            }
        }
    }
}

impl fmt::Display for PerformanceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PerformanceError::BenchmarkFailed {
                benchmark_name,
                reason,
            } => write!(f, "Benchmark '{}' failed: {}", benchmark_name, reason),
            PerformanceError::ReportGenerationFailed(reason) => {
                write!(f, "Performance report generation failed: {}", reason)
            }
            PerformanceError::MetricCollectionFailed { metric, reason } => write!(
                f,
                "Performance metric '{}' collection failed: {}",
                metric, reason
            ),
            PerformanceError::ComparisonFailed(reason) => {
                write!(f, "Performance comparison failed: {}", reason)
            }
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProtocolError::EncodingFailed { data_type, reason } => write!(
                f,
                "Protocol encoding failed for '{}': {}",
                data_type, reason
            ),
            ProtocolError::DecodingFailed { data_type, reason } => write!(
                f,
                "Protocol decoding failed for '{}': {}",
                data_type, reason
            ),
            ProtocolError::ValidationFailed(reason) => {
                write!(f, "Protocol validation failed: {}", reason)
            }
            ProtocolError::BinaryFormatError(reason) => {
                write!(f, "Binary format error: {}", reason)
            }
        }
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DatabaseError::ConnectionFailed { database, reason } => {
                write!(
                    f,
                    "Database connection to '{}' failed: {}",
                    database, reason
                )
            }
            DatabaseError::QueryFailed { query, reason } => {
                write!(f, "Database query failed: {} (query: {})", reason, query)
            }
            DatabaseError::TransactionFailed { reason } => {
                write!(f, "Database transaction failed: {}", reason)
            }
            DatabaseError::NotFound { entity, id } => {
                write!(f, "{} with id '{}' not found", entity, id)
            }
            DatabaseError::ConstraintViolation { constraint, reason } => {
                write!(f, "Constraint '{}' violated: {}", constraint, reason)
            }
            DatabaseError::MigrationFailed { version, reason } => {
                write!(f, "Database migration '{}' failed: {}", version, reason)
            }
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidationError::FieldValidation { field, reason } => {
                write!(f, "Field '{}' validation failed: {}", field, reason)
            }
            ValidationError::RequiredField { field } => {
                write!(f, "Required field '{}' is missing", field)
            }
            ValidationError::InvalidFormat {
                field,
                expected,
                actual,
            } => write!(
                f,
                "Field '{}' has invalid format: expected {}, got {}",
                field, expected, actual
            ),
            ValidationError::OutOfRange {
                field,
                min,
                max,
                actual,
            } => write!(
                f,
                "Field '{}' out of range: expected {}-{}, got {}",
                field, min, max, actual
            ),
            ValidationError::InvalidLength {
                field,
                min,
                max,
                actual,
            } => {
                let range = match (min, max) {
                    (Some(min), Some(max)) => format!("{}-{}", min, max),
                    (Some(min), None) => format!(">= {}", min),
                    (None, Some(max)) => format!("<= {}", max),
                    (None, None) => "unknown".to_string(),
                };
                write!(
                    f,
                    "Field '{}' invalid length: expected {}, got {}",
                    field, range, actual
                )
            }
            ValidationError::Custom(msg) => write!(f, "Validation failed: {}", msg),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::JSON { input, reason } => {
                write!(f, "JSON parse error: {} (input: {})", reason, input)
            }
            ParseError::TOML { input, reason } => {
                write!(f, "TOML parse error: {} (input: {})", reason, input)
            }
            ParseError::YAML { input, reason } => {
                write!(f, "YAML parse error: {} (input: {})", reason, input)
            }
            ParseError::Integer { input, reason } => {
                write!(f, "Integer parse error: {} (input: {})", reason, input)
            }
            ParseError::Float { input, reason } => {
                write!(f, "Float parse error: {} (input: {})", reason, input)
            }
            ParseError::Boolean { input } => {
                write!(f, "Boolean parse error: invalid input '{}'", input)
            }
            ParseError::URL { input, reason } => {
                write!(f, "URL parse error: {} (input: {})", reason, input)
            }
            ParseError::DateTime { input, reason } => {
                write!(f, "DateTime parse error: {} (input: {})", reason, input)
            }
            ParseError::Custom {
                format,
                input,
                reason,
            } => write!(f, "{} parse error: {} (input: {})", format, reason, input),
        }
    }
}

impl std::error::Error for VisionClawError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VisionClawError::IO(e) => Some(e),
            VisionClawError::Generic {
                source: Some(source),
                ..
            } => Some(&**source),
            _ => None,
        }
    }
}

impl std::error::Error for ActorError {}
impl std::error::Error for GPUError {}
impl std::error::Error for SettingsError {}
impl std::error::Error for NetworkError {}
impl std::error::Error for SpeechError {}
impl std::error::Error for GitHubError {}
impl std::error::Error for AudioError {}
impl std::error::Error for ResourceError {}
impl std::error::Error for PerformanceError {}
impl std::error::Error for ProtocolError {}
impl std::error::Error for DatabaseError {}
impl std::error::Error for ValidationError {}
impl std::error::Error for ParseError {}

impl From<std::io::Error> for VisionClawError {
    fn from(e: std::io::Error) -> Self {
        VisionClawError::IO(std::sync::Arc::new(e))
    }
}

impl From<ActorError> for VisionClawError {
    fn from(e: ActorError) -> Self {
        VisionClawError::Actor(e)
    }
}

impl From<GPUError> for VisionClawError {
    fn from(e: GPUError) -> Self {
        VisionClawError::GPU(e)
    }
}

impl From<SettingsError> for VisionClawError {
    fn from(e: SettingsError) -> Self {
        VisionClawError::Settings(e)
    }
}

impl From<NetworkError> for VisionClawError {
    fn from(e: NetworkError) -> Self {
        VisionClawError::Network(e)
    }
}

impl From<SpeechError> for VisionClawError {
    fn from(e: SpeechError) -> Self {
        VisionClawError::Speech(e)
    }
}

impl From<GitHubError> for VisionClawError {
    fn from(e: GitHubError) -> Self {
        VisionClawError::GitHub(e)
    }
}

impl From<AudioError> for VisionClawError {
    fn from(e: AudioError) -> Self {
        VisionClawError::Audio(e)
    }
}

impl From<ResourceError> for VisionClawError {
    fn from(e: ResourceError) -> Self {
        VisionClawError::Resource(e)
    }
}

impl From<PerformanceError> for VisionClawError {
    fn from(e: PerformanceError) -> Self {
        VisionClawError::Performance(e)
    }
}

impl From<ProtocolError> for VisionClawError {
    fn from(e: ProtocolError) -> Self {
        VisionClawError::Protocol(e)
    }
}

impl From<DatabaseError> for VisionClawError {
    fn from(e: DatabaseError) -> Self {
        VisionClawError::Database(e)
    }
}

impl From<ValidationError> for VisionClawError {
    fn from(e: ValidationError) -> Self {
        VisionClawError::Validation(e)
    }
}

impl From<ParseError> for VisionClawError {
    fn from(e: ParseError) -> Self {
        VisionClawError::Parse(e)
    }
}

impl From<serde_json::Error> for VisionClawError {
    fn from(e: serde_json::Error) -> Self {
        VisionClawError::Parse(ParseError::JSON {
            input: "".to_string(),
            reason: e.to_string(),
        })
    }
}

impl From<reqwest::Error> for VisionClawError {
    fn from(e: reqwest::Error) -> Self {
        VisionClawError::Network(NetworkError::RequestFailed {
            url: e.url().map(|u| u.to_string()).unwrap_or_default(),
            reason: e.to_string(),
        })
    }
}

impl From<String> for VisionClawError {
    fn from(s: String) -> Self {
        VisionClawError::Generic {
            message: s,
            source: None,
        }
    }
}

impl From<&str> for VisionClawError {
    fn from(s: &str) -> Self {
        VisionClawError::Generic {
            message: s.to_string(),
            source: None,
        }
    }
}

// Convenience type alias for Results
pub type VisionClawResult<T> = Result<T, VisionClawError>;

pub trait ErrorContext<T> {
    fn with_context<F>(self, f: F) -> VisionClawResult<T>
    where
        F: FnOnce() -> String;

    fn with_actor_context(self, actor_name: &str) -> VisionClawResult<T>;

    fn with_gpu_context(self, operation: &str) -> VisionClawResult<T>;
}

impl<T, E> ErrorContext<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context<F>(self, f: F) -> VisionClawResult<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| VisionClawError::Generic {
            message: f(),
            source: Some(std::sync::Arc::new(e)),
        })
    }

    fn with_actor_context(self, actor_name: &str) -> VisionClawResult<T> {
        self.map_err(|e| {
            VisionClawError::Actor(ActorError::RuntimeFailure {
                actor_name: actor_name.to_string(),
                reason: e.to_string(),
            })
        })
    }

    fn with_gpu_context(self, operation: &str) -> VisionClawResult<T> {
        self.map_err(|e| {
            VisionClawError::GPU(GPUError::KernelExecutionFailed {
                kernel_name: operation.to_string(),
                reason: e.to_string(),
            })
        })
    }
}

// Helper macros for common error patterns

/// Create a validation error
#[macro_export]
macro_rules! validation_error {
    ($field:expr, $reason:expr) => {
        $crate::errors::VisionClawError::Validation(
            $crate::errors::ValidationError::FieldValidation {
                field: $field.to_string(),
                reason: $reason.to_string(),
            },
        )
    };
}

/// Create a parse error
#[macro_export]
macro_rules! parse_error {
    (json, $input:expr, $reason:expr) => {
        $crate::errors::VisionClawError::Parse($crate::errors::ParseError::JSON {
            input: $input.to_string(),
            reason: $reason.to_string(),
        })
    };
    (integer, $input:expr) => {
        $crate::errors::VisionClawError::Parse($crate::errors::ParseError::Integer {
            input: $input.to_string(),
            reason: "invalid integer format".to_string(),
        })
    };
}

/// Create a database error
#[macro_export]
macro_rules! db_error {
    (not_found, $entity:expr, $id:expr) => {
        $crate::errors::VisionClawError::Database($crate::errors::DatabaseError::NotFound {
            entity: $entity.to_string(),
            id: $id.to_string(),
        })
    };
    (query_failed, $query:expr, $reason:expr) => {
        $crate::errors::VisionClawError::Database($crate::errors::DatabaseError::QueryFailed {
            query: $query.to_string(),
            reason: $reason.to_string(),
        })
    };
}

/// Helper function to convert Option to Result with better error messages
pub trait OptionExt<T> {
    /// Convert Option to Result with a custom error message
    fn ok_or_error(self, message: impl Into<String>) -> VisionClawResult<T>;

    /// Convert Option to Result with a validation error
    fn ok_or_validation(self, field: impl Into<String>) -> VisionClawResult<T>;

    /// Convert Option to Result with a not found error
    fn ok_or_not_found(
        self,
        entity: impl Into<String>,
        id: impl Into<String>,
    ) -> VisionClawResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_error(self, message: impl Into<String>) -> VisionClawResult<T> {
        self.ok_or_else(|| VisionClawError::Generic {
            message: message.into(),
            source: None,
        })
    }

    fn ok_or_validation(self, field: impl Into<String>) -> VisionClawResult<T> {
        self.ok_or_else(|| {
            VisionClawError::Validation(ValidationError::RequiredField {
                field: field.into(),
            })
        })
    }

    fn ok_or_not_found(
        self,
        entity: impl Into<String>,
        id: impl Into<String>,
    ) -> VisionClawResult<T> {
        self.ok_or_else(|| {
            VisionClawError::Database(DatabaseError::NotFound {
                entity: entity.into(),
                id: id.into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let actor_error = VisionClawError::Actor(ActorError::StartupFailed {
            actor_name: "TestActor".to_string(),
            reason: "Init failed".to_string(),
        });

        assert!(actor_error.to_string().contains("TestActor"));
        assert!(actor_error.to_string().contains("Init failed"));
    }

    #[test]
    fn test_error_context() {
        let result: Result<(), std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        ));

        let with_context = result.with_context(|| "Failed to read configuration".to_string());
        assert!(with_context.is_err());

        if let Err(VisionClawError::Generic { message, .. }) = with_context {
            assert_eq!(message, "Failed to read configuration");
        } else {
            panic!("Expected Generic error with context");
        }
    }
}
