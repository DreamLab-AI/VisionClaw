// src/settings/auth_extractor.rs
//! Authentication extractor for settings API endpoints
//! Supports dual-auth: NIP-98 Schnorr (primary) + Bearer token (legacy fallback)

use actix_web::{
    dev::Payload, error::ErrorUnauthorized, web, Error as ActixError, FromRequest, HttpRequest,
};
use log::{debug, info, warn};
use std::future::Future;
use std::pin::Pin;

use crate::services::nostr_service::NostrService;

/// Try the dev-mode unauthenticated bypass.
///
/// Per ADR-06 §D1 and resolution T2, this branch only exists in the binary
/// when compiled with `debug_assertions` or `--features dev-auth`. The release
/// build's `try_dev_bypass` is the no-op stub below; no env var, header, or
/// argv can re-enable bypass at runtime.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
fn try_dev_bypass(req: &HttpRequest) -> Option<AuthenticatedUser> {
    let auth = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())?;
    if auth == "Bearer dev-session-token" {
        debug!("dev-auth: Bearer dev-session-token accepted (dev build)");
        let pubkey = req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "dev-user".to_string());
        return Some(AuthenticatedUser {
            pubkey,
            is_power_user: true,
        });
    }
    None
}

/// Release-build stub: dev bypass code is absent from the binary.
#[cfg(not(any(debug_assertions, feature = "dev-auth")))]
#[inline(always)]
fn try_dev_bypass(_req: &HttpRequest) -> Option<AuthenticatedUser> {
    None
}

/// Authenticated user information extracted from NIP-98 or session token
#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub pubkey: String,
    pub is_power_user: bool,
}

impl AuthenticatedUser {
    /// Check if user has power user privileges
    pub fn require_power_user(&self) -> Result<(), ActixError> {
        if self.is_power_user {
            Ok(())
        } else {
            Err(ErrorUnauthorized("Power user access required"))
        }
    }
}

/// Optional authenticated user - allows both authenticated and anonymous access
pub struct OptionalAuth(pub Option<AuthenticatedUser>);

impl FromRequest for AuthenticatedUser {
    type Error = ActixError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // ADR-06 §D1 + resolution T2: dev-bypass is compile-time gated.
        // In release builds (no `dev-auth` feature, no `debug_assertions`),
        // `try_dev_bypass` is a `None`-returning stub stripped by the optimizer.
        // No environment variable can reach this branch — case-sensitive
        // `APP_ENV` / `RUST_ENV` runtime guards are intentionally absent.
        if let Some(user) = try_dev_bypass(req) {
            return Box::pin(async move { Ok(user) });
        }

        // Extract NostrService from app data
        let nostr_service = match req.app_data::<web::Data<NostrService>>() {
            Some(service) => service.clone(),
            None => {
                warn!("NostrService not found in app data");
                return Box::pin(async { Err(ErrorUnauthorized("Authentication service unavailable")) });
            }
        };

        // Extract Authorization header
        let auth_header = match req.headers().get("Authorization") {
            Some(header) => match header.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    debug!("Invalid Authorization header format");
                    return Box::pin(async { Err(ErrorUnauthorized("Invalid authorization header")) });
                }
            },
            None => {
                debug!("Missing Authorization header");
                return Box::pin(async { Err(ErrorUnauthorized("Missing authorization token")) });
            }
        };

        // --- NIP-98 Schnorr auth (primary path) ---
        if auth_header.starts_with("Nostr ") {
            // Reconstruct the request URL for NIP-98 validation
            // Behind a TLS-terminating proxy, connection_info returns internal
            // scheme/host; prefer X-Forwarded-* headers from the proxy.
            let conn_info = req.connection_info();
            let scheme = req.headers()
                .get("X-Forwarded-Proto")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.scheme())
                .to_string();
            let host = req.headers()
                .get("X-Forwarded-Host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.host())
                .to_string();
            let url = format!(
                "{}://{}{}",
                scheme,
                host,
                req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
            );
            let method = req.method().as_str().to_string();

            return Box::pin(async move {
                match nostr_service.verify_nip98_auth(&auth_header, &url, &method, None).await {
                    Ok(user) => {
                        info!("NIP-98 authenticated user: {}", user.pubkey);
                        Ok(AuthenticatedUser {
                            pubkey: user.pubkey,
                            is_power_user: user.is_power_user,
                        })
                    }
                    Err(e) => {
                        warn!("NIP-98 auth failed: {}", e);
                        Err(ErrorUnauthorized(format!("NIP-98 auth failed: {}", e)))
                    }
                }
            });
        }

        // --- Legacy Bearer token path (fallback) ---
        let token = match auth_header.strip_prefix("Bearer ") {
            Some(t) => t.to_string(),
            None => {
                debug!("Authorization header has unrecognized prefix");
                return Box::pin(async { Err(ErrorUnauthorized("Invalid authorization format")) });
            }
        };

        // Extract pubkey from X-Nostr-Pubkey header (required for Bearer path)
        let pubkey = match req.headers().get("X-Nostr-Pubkey") {
            Some(header) => match header.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    debug!("Invalid X-Nostr-Pubkey header format");
                    return Box::pin(async { Err(ErrorUnauthorized("Invalid pubkey header")) });
                }
            },
            None => {
                debug!("Missing X-Nostr-Pubkey header");
                return Box::pin(async { Err(ErrorUnauthorized("Missing pubkey")) });
            }
        };

        // ADR-06 §D1 + resolution T2: dev-mode session-token bypass is
        // compile-time gated. The branch below is stripped from release builds.
        #[cfg(any(debug_assertions, feature = "dev-auth"))]
        {
            if token == "dev-session-token" {
                debug!("dev-auth: Bearer dev-session-token accepted for pubkey: {} (dev build)", pubkey);
                return Box::pin(async move {
                    Ok(AuthenticatedUser {
                        pubkey,
                        is_power_user: true,
                    })
                });
            }
        }
        // In release builds the above block compiles to nothing; the
        // `dev-session-token` string literal is absent from the binary.

        Box::pin(async move {
            // Validate session
            let is_valid = nostr_service.validate_session(&pubkey, &token).await;

            if !is_valid {
                debug!("Session validation failed for pubkey: {}", pubkey);
                return Err(ErrorUnauthorized("Invalid or expired session"));
            }

            // Get user details
            match nostr_service.get_user(&pubkey).await {
                Some(user) => {
                    debug!("Successfully authenticated user: {}", pubkey);
                    Ok(AuthenticatedUser {
                        pubkey: user.pubkey,
                        is_power_user: user.is_power_user,
                    })
                }
                None => {
                    warn!("User not found after successful validation: {}", pubkey);
                    Err(ErrorUnauthorized("User not found"))
                }
            }
        })
    }
}

impl FromRequest for OptionalAuth {
    type Error = ActixError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let fut = AuthenticatedUser::from_request(req, payload);
        Box::pin(async move {
            match fut.await {
                Ok(user) => Ok(OptionalAuth(Some(user))),
                Err(_) => {
                    debug!("Optional authentication: proceeding without authentication");
                    Ok(OptionalAuth(None))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authenticated_user_power_check() {
        let power_user = AuthenticatedUser {
            pubkey: "test_pubkey".to_string(),
            is_power_user: true,
        };
        assert!(power_user.require_power_user().is_ok());

        let regular_user = AuthenticatedUser {
            pubkey: "test_pubkey".to_string(),
            is_power_user: false,
        };
        assert!(regular_user.require_power_user().is_err());
    }
}
