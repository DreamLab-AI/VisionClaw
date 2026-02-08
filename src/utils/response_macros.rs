/// HTTP Response Standardization Macros
/// These macros provide a consistent interface for creating HTTP responses
/// using the HandlerResponse trait. All handlers MUST use these macros
/// instead of direct HttpResponse construction.
/// Author: API Specialist Agent
/// Task: Phase 1, Task 1.4 - HTTP Response Standardization
/// # Usage
/// These macros are exported at crate level with `#[macro_export]`.
/// Import them directly from crate root:
/// ```rust
/// use crate::{ok_json, error_json, service_unavailable};
/// ```

/// Success response with JSON data (200 OK)
/// # Examples
/// ```
/// use crate::ok_json;
/// let user = User { id: 1, name: "Alice" };
/// ok_json!(user)
/// ```
#[macro_export]
macro_rules! ok_json {
    ($data:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use crate::utils::handler_commons::StandardResponse;

            Ok::<HttpResponse, Error>(HttpResponse::Ok().json(StandardResponse {
                success: true,
                data: Some($data),
                error: None,
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Created response with JSON data (201 Created)
/// # Examples
/// ```
/// use crate::created_json;
/// let new_item = Item { id: 42, name: "New Item" };
/// created_json!(new_item)
/// ```
#[macro_export]
macro_rules! created_json {
    ($data:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use crate::utils::handler_commons::StandardResponse;

            Ok::<HttpResponse, Error>(HttpResponse::Created().json(StandardResponse {
                success: true,
                data: Some($data),
                error: None,
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Internal server error response (500)
/// # Examples
/// ```
/// use crate::error_json;
/// error_json!("Database connection failed")
/// error_json!("Database error", e.to_string())  // With details
/// ```
#[macro_export]
macro_rules! error_json {
    ($msg:expr) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::internal_error($msg.to_string())
        }
    };
    ($msg:expr, $details:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::error;

            let details_str = format!("{}", $details);
            error!("Internal server error: {} - {}", $msg, details_str);
            Ok::<HttpResponse, Error>(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": $msg,
                "message": details_str
            })))
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::internal_error(format!($fmt, $($arg)*))
        }
    };
}

/// Bad request error response (400)
/// # Examples
/// ```
/// use crate::bad_request;
/// bad_request!("Invalid input parameters")
/// bad_request!("Validation error", error_details)  // With details
/// ```
#[macro_export]
macro_rules! bad_request {
    ($msg:expr) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::bad_request($msg.to_string())
        }
    };
    ($msg:expr, $details:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;

            warn!("Bad request: {} - {}", $msg, $details);
            Ok::<HttpResponse, Error>(HttpResponse::BadRequest().json(serde_json::json!({
                "error": $msg,
                "message": $details
            })))
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::bad_request(format!($fmt, $($arg)*))
        }
    };
}

/// Not found error response (404)
/// # Examples
/// ```
/// use crate::not_found;
/// not_found!("User not found")
/// not_found!("Resource not found", resource_id)  // With details
/// ```
#[macro_export]
macro_rules! not_found {
    ($msg:expr) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::not_found($msg.to_string())
        }
    };
    ($msg:expr, $details:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;

            warn!("Not found: {} - {}", $msg, $details);
            Ok::<HttpResponse, Error>(HttpResponse::NotFound().json(serde_json::json!({
                "error": $msg,
                "message": $details
            })))
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            use crate::utils::handler_commons::HandlerResponse;
            <()>::not_found(format!($fmt, $($arg)*))
        }
    };
}

/// Success response with custom message
/// # Examples
/// ```
/// use crate::success_msg;
/// let data = ProcessResult { processed: 100 };
/// success_msg!(data, "Processing completed successfully")
/// ```
#[macro_export]
macro_rules! success_msg {
    ($data:expr, $msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use crate::utils::handler_commons::StandardResponse;

            Ok::<HttpResponse, Error>(HttpResponse::Ok().json(StandardResponse {
                success: true,
                data: Some($data),
                error: None,
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Unauthorized error response (401)
/// # Examples
/// ```
/// use crate::unauthorized;
/// unauthorized!("Invalid authentication token")
/// ```
#[macro_export]
macro_rules! unauthorized {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Unauthorized access: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::Unauthorized().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            let msg = format!($fmt, $($arg)*);
            warn!("Unauthorized access: {}", msg);
            Ok::<HttpResponse, Error>(HttpResponse::Unauthorized().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some(msg),
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Forbidden error response (403)
/// # Examples
/// ```
/// use crate::forbidden;
/// forbidden!("Insufficient permissions")
/// ```
#[macro_export]
macro_rules! forbidden {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Forbidden access: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::Forbidden().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Conflict error response (409)
/// # Examples
/// ```
/// use crate::conflict;
/// conflict!("Resource already exists")
/// ```
#[macro_export]
macro_rules! conflict {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Conflict: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::Conflict().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::time::now(),
                request_id: None,
            }))
        }
    };
}

/// No content response (204)
/// # Examples
/// ```
/// use crate::no_content;
/// no_content!()
/// ```
#[macro_export]
macro_rules! no_content {
    () => {
        {
            use actix_web::{HttpResponse, Error};
            Ok::<HttpResponse, Error>(HttpResponse::NoContent().finish())
        }
    };
}

/// Too Many Requests error response (429)
/// # Examples
/// ```
/// use crate::too_many_requests;
/// too_many_requests!("Rate limit exceeded")
/// ```
#[macro_export]
macro_rules! too_many_requests {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Too many requests: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::TooManyRequests().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::utils::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Service Unavailable error response (503)
/// # Examples
/// ```
/// use crate::service_unavailable;
/// service_unavailable!("Service temporarily unavailable")
/// ```
#[macro_export]
macro_rules! service_unavailable {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Service unavailable: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::ServiceUnavailable().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::utils::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Payload Too Large error response (413)
/// # Examples
/// ```
/// use crate::payload_too_large;
/// payload_too_large!("Request body exceeds maximum size")
/// ```
#[macro_export]
macro_rules! payload_too_large {
    ($msg:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use log::warn;
            use crate::utils::handler_commons::StandardResponse;

            warn!("Payload too large: {}", $msg);
            Ok::<HttpResponse, Error>(HttpResponse::PayloadTooLarge().json(StandardResponse::<()> {
                success: false,
                data: None,
                error: Some($msg.to_string()),
                timestamp: crate::utils::time::now(),
                request_id: None,
            }))
        }
    };
}

/// Accepted response (202)
/// # Examples
/// ```
/// use crate::accepted;
/// accepted!(TaskInfo { id: 123, status: "pending" })
/// ```
#[macro_export]
macro_rules! accepted {
    ($data:expr) => {
        {
            use actix_web::{HttpResponse, Error};
            use crate::utils::handler_commons::StandardResponse;

            Ok::<HttpResponse, Error>(HttpResponse::Accepted().json(StandardResponse {
                success: true,
                data: Some($data),
                error: None,
                timestamp: crate::utils::time::now(),
                request_id: None,
            }))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use serde::{Deserialize, Serialize};
use crate::utils::time;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    struct TestData {
        id: u32,
        name: String,
    }

    #[test]
    fn test_ok_json_macro() {
        let data = TestData {
            id: 1,
            name: "Test".to_string(),
        };
        let result = ok_json!(data);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_error_json_macro() {
        let result = error_json!("Test error");
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_bad_request_macro() {
        let result = bad_request!("Invalid input");
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_not_found_macro() {
        let result = not_found!("Resource not found");
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_created_json_macro() {
        let data = TestData {
            id: 2,
            name: "Created".to_string(),
        };
        let result = created_json!(data);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[test]
    fn test_unauthorized_macro() {
        let result = unauthorized!("Invalid token");
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_forbidden_macro() {
        let result = forbidden!("Access denied");
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_no_content_macro() {
        let result = no_content!();
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[test]
    fn test_error_json_with_formatting() {
        let result = error_json!("Error code: {}", 500);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
