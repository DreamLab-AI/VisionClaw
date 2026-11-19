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
    /// When true, only mutating methods (POST/PUT/PATCH/DELETE) are gated; safe
    /// reads (GET/HEAD/OPTIONS) pass through unauthenticated. This lets a single
    /// scope mix public reads with privileged writes on the same path prefix —
    /// actix-web does NOT fall through duplicate-prefix scopes, so per-method
    /// auth on a shared prefix has to be expressed inside one middleware.
    mutations_only: bool,
}

impl RequireAuth {
    /// Require authenticated user (any valid session)
    pub fn authenticated() -> Self {
        Self {
            level: AccessLevel::Authenticated,
            mutations_only: false,
        }
    }

    /// Require power user access
    pub fn power_user() -> Self {
        Self {
            level: AccessLevel::PowerUser,
            mutations_only: false,
        }
    }

    /// Require read-only access (any authenticated user)
    pub fn read_only() -> Self {
        Self {
            level: AccessLevel::ReadOnly,
            mutations_only: false,
        }
    }

    /// Require graph write access
    pub fn write_graph() -> Self {
        Self {
            level: AccessLevel::WriteGraph,
            mutations_only: false,
        }
    }

    /// Require settings write access
    pub fn write_settings() -> Self {
        Self {
            level: AccessLevel::WriteSettings,
            mutations_only: false,
        }
    }

    /// Require admin access
    pub fn admin() -> Self {
        Self {
            level: AccessLevel::Admin,
            mutations_only: false,
        }
    }

    /// Gate only mutating methods (POST/PUT/PATCH/DELETE) at the configured
    /// level; safe methods (GET/HEAD/OPTIONS) are left public. Wrap a whole
    /// mixed-method scope with this to protect writes while keeping reads open.
    pub fn mutations_only(mut self) -> Self {
        self.mutations_only = true;
        self
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
            mutations_only: self.mutations_only,
        }))
    }
}

pub struct AuthMiddleware<S> {
    service: Rc<S>,
    level: AccessLevel,
    mutations_only: bool,
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

        // In mutations_only mode, safe methods bypass auth entirely so public
        // reads keep working on a mixed-method scope.
        if self.mutations_only && req.method().is_safe() {
            return Box::pin(async move {
                let resp = svc.call(req).await?;
                Ok(resp.map_into_boxed_body())
            });
        }

        Box::pin(async move {
            // Extract NostrService from app data
            let nostr_service = match req.app_data::<actix_web::web::Data<NostrService>>() {
                Some(service) => service.clone(),
                None => {
                    warn!("NostrService not found in app data - authentication cannot proceed");
                    let resp = HttpResponse::Unauthorized().body("Unauthorized");
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

/// Request-extension marker: the request authenticated through a bound browser
/// session key (ADR-2205), not the principal's own signer. Inserted by
/// `verify_access`, so a layered re-check sees it too.
#[derive(Clone, Debug)]
pub struct SessionKeyIdentity {
    /// The ephemeral key that signed.
    pub session_pubkey: String,
}

/// Whether the request authenticated through a bound browser session key
/// (ADR-2205) rather than the principal's own signer.
pub fn via_session_key(req: &actix_web::HttpRequest) -> bool {
    req.extensions().get::<SessionKeyIdentity>().is_some()
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
