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
        // Dev mode bypass: allow unauthenticated settings writes when explicitly enabled
        // SECURITY: Requires SETTINGS_AUTH_BYPASS=true in environment (only set in dev compose)
        // SECURITY: Bypass is IGNORED when APP_ENV=production or RUST_ENV=production
        let bypass_enabled = std::env::var("SETTINGS_AUTH_BYPASS").unwrap_or_default() == "true";
        if bypass_enabled {
            let is_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false)
                || std::env::var("RUST_ENV").map(|v| v == "production").unwrap_or(false);
            if is_production {
                warn!("SETTINGS_AUTH_BYPASS is set but ignored in production mode");
                // fall through to normal auth
            } else {
                debug!("Settings auth bypass enabled (SETTINGS_AUTH_BYPASS=true) - using dev user");
                return Box::pin(async {
                    Ok(AuthenticatedUser {
                        pubkey: "dev-user".to_string(),
                        is_power_user: true,
                    })
                });
            }
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

        // Dev-mode session bypass - requires SETTINGS_AUTH_BYPASS in environment
        // SECURITY: Bypass is IGNORED when APP_ENV=production or RUST_ENV=production
        if std::env::var("SETTINGS_AUTH_BYPASS").unwrap_or_default() == "true"
            && token == "dev-session-token"
        {
            let is_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false)
                || std::env::var("RUST_ENV").map(|v| v == "production").unwrap_or(false);
            if is_production {
                warn!("SETTINGS_AUTH_BYPASS session bypass ignored in production mode");
                // fall through to normal session validation
            } else {
                debug!("Dev-mode session token accepted for pubkey: {}", pubkey);
                return Box::pin(async move {
                    Ok(AuthenticatedUser {
                        pubkey,
                        is_power_user: true,
                    })
                });
            }
        }

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
