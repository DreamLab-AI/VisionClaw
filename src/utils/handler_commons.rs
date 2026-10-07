use crate::utils::time;
use actix_web::{HttpResponse, Result};
use chrono::{DateTime, Utc};
use log::{error, warn};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StandardResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub request_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorResponse {
    pub error_type: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SuccessResponse<T> {
    pub data: T,
    pub message: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub trait HandlerResponse<T: Serialize> {
    fn success(data: T) -> Result<HttpResponse> {
        Ok(HttpResponse::Ok().json(StandardResponse {
            success: true,
            data: Some(data),
            error: None,
            timestamp: time::now(),
            request_id: None,
        }))
    }

    fn success_with_message(data: T, message: String) -> Result<HttpResponse> {
        Ok(HttpResponse::Ok().json(SuccessResponse {
            data,
            message: Some(message),
            timestamp: time::now(),
        }))
    }

    fn internal_error(message: String) -> Result<HttpResponse> {
        error!("Internal server error: {}", message);
        Ok(
            HttpResponse::InternalServerError().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some(message),
                timestamp: time::now(),
                request_id: None,
            }),
        )
    }

    fn bad_request(message: String) -> Result<HttpResponse> {
        warn!("Bad request: {}", message);
        Ok(HttpResponse::BadRequest().json(StandardResponse::<()> {
            success: false,
            data: None,
            error: Some(message),
            timestamp: time::now(),
            request_id: None,
        }))
    }

    fn not_found(message: String) -> Result<HttpResponse> {
        warn!("Not found: {}", message);
        Ok(HttpResponse::NotFound().json(StandardResponse::<()> {
            success: false,
            data: None,
            error: Some(message),
            timestamp: time::now(),
            request_id: None,
        }))
    }

    fn from_error(error: Box<dyn std::error::Error>) -> Result<HttpResponse> {
        Self::internal_error(error.to_string())
    }

    fn from_str_error(error: &str) -> Result<HttpResponse> {
        Self::internal_error(error.to_string())
    }
}

impl<T: Serialize> HandlerResponse<T> for T {}

#[derive(Deserialize, Debug, Clone)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: Some(1),
            limit: Some(50),
            offset: None,
        }
    }
}

impl PaginationParams {
    pub fn get_offset(&self) -> u32 {
        if let Some(offset) = self.offset {
            offset
        } else {
            let page = self.page.unwrap_or(1).max(1);
            let limit = self.limit.unwrap_or(50);
            (page - 1) * limit
        }
    }

    pub fn get_limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(100)
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total_count: u32,
    pub page: u32,
    pub limit: u32,
    pub total_pages: u32,
    pub has_next: bool,
    pub has_prev: bool,
}

impl<T> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, total_count: u32, params: &PaginationParams) -> Self {
        let limit = params.get_limit();
        let current_page = params.page.unwrap_or(1);
        let total_pages = total_count.div_ceil(limit);

        Self {
            items,
            total_count,
            page: current_page,
            limit,
            total_pages,
            has_next: current_page < total_pages,
            has_prev: current_page > 1,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HealthCheckResponse {
    pub status: String,
    pub timestamp: DateTime<Utc>,
    pub version: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub components: Vec<ComponentHealth>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComponentHealth {
    pub name: String,
    pub status: String,
    pub details: Option<String>,
    pub last_check: DateTime<Utc>,
    pub metrics: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum StandardWebSocketMessage<T> {
    #[serde(rename = "data")]
    Data {
        payload: T,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "error")]
    Error {
        message: String,
        error_type: String,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "status")]
    Status {
        status: String,
        message: Option<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "ping")]
    Ping { timestamp: DateTime<Utc> },

    #[serde(rename = "pong")]
    Pong { timestamp: DateTime<Utc> },

    #[serde(rename = "subscribe")]
    Subscribe {
        channels: Vec<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        channels: Vec<String>,
        timestamp: DateTime<Utc>,
    },

    #[serde(rename = "connection_established")]
    ConnectionEstablished {
        client_id: String,
        timestamp: DateTime<Utc>,
    },
}

pub trait Validate {
    type ValidationError;

    fn validate(&self) -> std::result::Result<(), Self::ValidationError>;

    fn validate_for_handler(&self) -> Option<Result<HttpResponse>>
    where
        Self::ValidationError: std::fmt::Display,
    {
        match self.validate() {
            Ok(()) => None,
            Err(e) => Some(<()>::bad_request(e.to_string())),
        }
    }
}

pub fn log_request<T: std::fmt::Debug>(endpoint: &str, request: &T) {
    log::info!("Request to {}: {:?}", endpoint, request);
}

pub fn log_response<T: std::fmt::Debug>(endpoint: &str, response: &T) {
    log::debug!("Response from {}: {:?}", endpoint, response);
}

pub fn convert_to_actix_error(error: Box<dyn std::error::Error + Send + Sync>) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(error)
}

#[macro_export]
macro_rules! handler_error {
    ($msg:expr) => {
        return <()>::internal_error($msg.to_string());
    };
    ($fmt:expr, $($arg:tt)*) => {
        return <()>::internal_error(format!($fmt, $($arg)*));
    };
}

#[macro_export]
macro_rules! handler_success {
    ($data:expr) => {
        return <_>::success($data);
    };
    ($data:expr, $msg:expr) => {
        return <_>::success_with_message($data, $msg.to_string());
    };
}
