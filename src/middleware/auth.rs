//! Authentication Middleware
//!
//! Provides Actix-web middleware for enforcing authentication on protected routes.
//! Uses Nostr-based authentication with session validation.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, HttpResponse,
};
use futures_util::future::LocalBoxFuture;
use log::{debug, warn};
use std::future::{ready, Ready};
use std::rc::Rc;

use crate::services::nostr_service::NostrService;
use crate::utils::auth::{verify_access, AccessLevel};

/// Authentication middleware that enforces Nostr-based session validation
/// # Example
/// ```rust,ignore
/// use actix_web::{web, App};
/// use crate::middleware::auth::RequireAuth;
/// App::new()
///     .wrap(RequireAuth::authenticated())  // Require any authenticated user
///     .route("/protected", web::get().to(handler))
/// ```
pub struct RequireAuth {
    level: AccessLevel,
}

impl RequireAuth {
    /// Require authenticated user (any valid session)
    pub fn authenticated() -> Self {
        Self {
            level: AccessLevel::Authenticated,
        }
    }

    /// Require power user access
    pub fn power_user() -> Self {
        Self {
            level: AccessLevel::PowerUser,
        }
    }

    /// Require read-only access (any authenticated user)
    pub fn read_only() -> Self {
        Self {
            level: AccessLevel::ReadOnly,
        }
    }

    /// Require graph write access
    pub fn write_graph() -> Self {
        Self {
            level: AccessLevel::WriteGraph,
        }
    }

    /// Require settings write access
    pub fn write_settings() -> Self {
        Self {
            level: AccessLevel::WriteSettings,
        }
    }

    /// Require admin access
    pub fn admin() -> Self {
        Self {
            level: AccessLevel::Admin,
        }
    }

    /// Allow both signed and anonymous callers.
    ///
    /// Behaviour:
    /// - Signed requests (valid NIP-98 `Authorization: Nostr …`): verified
    ///   normally and the caller's pubkey is stored in request extensions.
    /// - Anonymous requests (no auth headers at all): pass through with an
    ///   empty-string pubkey marker. Handlers call
    ///   `get_authenticated_user(&req)` and check `pubkey.is_empty()` to
    ///   distinguish anonymous from signed.
    /// - Malformed auth attempts (header present but invalid): rejected 401.
    ///
    /// Gated by the `NIP98_OPTIONAL_AUTH=true` env var. When the flag is
    /// false/unset, this variant transparently behaves like
    /// `RequireAuth::authenticated()` — the rollback lever for the sprint.
    pub fn optional() -> Self {
        Self {
            level: AccessLevel::Optional,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RequireAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddleware {
            service: Rc::new(service),
            level: self.level.clone(),
        }))
    }
}

pub struct AuthMiddleware<S> {
    service: Rc<S>,
    level: AccessLevel,
}

impl<S, B> Service<ServiceRequest> for AuthMiddleware<S>
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
        let level = self.level.clone();

        Box::pin(async move {
            // Extract NostrService from app data
            let nostr_service = match req.app_data::<actix_web::web::Data<NostrService>>() {
                Some(service) => service.clone(),
                None => {
                    warn!("NostrService not found in app data - authentication cannot proceed");
                    let resp = HttpResponse::Unauthorized()
                        .body("Unauthorized");
                    return Ok(req.into_response(resp).map_into_boxed_body());
                }
            };

            // Verify access level — delegates to the unified verify_access
            // which handles all AccessLevel variants including scoped permissions
            let result = verify_access(req.request(), &nostr_service, level).await;

            match result {
                Ok(pubkey) => {
                    // Store authenticated pubkey in request extensions for handlers to use
                    req.extensions_mut().insert(AuthenticatedUser { pubkey });

                    debug!("Authentication successful, proceeding with request");

                    // Continue to the actual handler
                    let resp = svc.call(req).await?;
                    Ok(resp.map_into_boxed_body())
                }
                Err(response) => {
                    // Authentication failed - return error response
                    Ok(req.into_response(response).map_into_boxed_body())
                }
            }
        })
    }
}

/// Authenticated user information stored in request extensions
#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub pubkey: String,
}

/// Extract authenticated user from request extensions (for use in handlers)
/// # Example
/// ```rust,ignore
/// use actix_web::{web, HttpRequest};
/// use crate::middleware::auth::get_authenticated_user;
/// async fn handler(req: HttpRequest) -> impl Responder {
///     let user = get_authenticated_user(&req)?;
///     // Use user.pubkey
/// }
/// ```
pub fn get_authenticated_user(req: &actix_web::HttpRequest) -> Option<AuthenticatedUser> {
    req.extensions().get::<AuthenticatedUser>().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_require_auth_levels() {
        let auth = RequireAuth::authenticated();
        assert!(matches!(auth.level, AccessLevel::Authenticated));

        let power = RequireAuth::power_user();
        assert!(matches!(power.level, AccessLevel::PowerUser));

        let read = RequireAuth::read_only();
        assert!(matches!(read.level, AccessLevel::ReadOnly));

        let write_graph = RequireAuth::write_graph();
        assert!(matches!(write_graph.level, AccessLevel::WriteGraph));

        let write_settings = RequireAuth::write_settings();
        assert!(matches!(write_settings.level, AccessLevel::WriteSettings));

        let admin = RequireAuth::admin();
        assert!(matches!(admin.level, AccessLevel::Admin));
    }

    #[test]
    fn test_access_level_permissions() {
        // Admin has all permissions
        assert!(AccessLevel::Admin.has_permission(&AccessLevel::ReadOnly));
        assert!(AccessLevel::Admin.has_permission(&AccessLevel::WriteGraph));
        assert!(AccessLevel::Admin.has_permission(&AccessLevel::WriteSettings));
        assert!(AccessLevel::Admin.has_permission(&AccessLevel::Admin));
        assert!(AccessLevel::Admin.has_permission(&AccessLevel::PowerUser));

        // Authenticated has read + write graph but not write settings or admin
        assert!(AccessLevel::Authenticated.has_permission(&AccessLevel::ReadOnly));
        assert!(AccessLevel::Authenticated.has_permission(&AccessLevel::WriteGraph));
        assert!(!AccessLevel::Authenticated.has_permission(&AccessLevel::WriteSettings));
        assert!(!AccessLevel::Authenticated.has_permission(&AccessLevel::Admin));

        // ReadOnly can read but not write
        assert!(AccessLevel::ReadOnly.has_permission(&AccessLevel::ReadOnly));
        assert!(!AccessLevel::ReadOnly.has_permission(&AccessLevel::WriteGraph));
        assert!(!AccessLevel::ReadOnly.has_permission(&AccessLevel::Admin));

        // PowerUser has everything (maps to Admin)
        assert!(AccessLevel::PowerUser.has_permission(&AccessLevel::Admin));
        assert!(AccessLevel::PowerUser.has_permission(&AccessLevel::WriteSettings));
    }
}
