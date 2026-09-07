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
    // LAN-local full dev mode (VISIONCLAW_DEV_MODE=1): grant dev admin to every
    // request with no header/signature/peer check — the settings-write parity of
    // the REST `verify_access` full bypass. See `utils::auth::dev_full_bypass_active`.
    if crate::utils::auth::dev_full_bypass_active() {
        debug!("dev-mode: VISIONCLAW_DEV_MODE full bypass — settings extractor granting dev admin");
        return Some(AuthenticatedUser {
            pubkey: crate::utils::auth::DEV_MODE_PUBKEY.to_string(),
            is_power_user: true,
        });
    }
    let auth = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())?;
    if auth == "Bearer dev-session-token" {
        // Route through the single shared gate: dev build + DEV_AUTH_LOOPBACK=1 +
        // loopback peer. Never accept the literal token ungated.
        if !crate::utils::auth::dev_bypass_permitted(req) {
            warn!(
                "dev-auth: rejected Bearer dev-session-token — requires DEV_AUTH_LOOPBACK=1 and a loopback peer (peer={:?})",
                req.peer_addr()
            );
            return None;
        }
        debug!("dev-auth: Bearer dev-session-token accepted (loopback + DEV_AUTH_LOOPBACK)");
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
                return Box::pin(async {
                    Err(ErrorUnauthorized("Authentication service unavailable"))
                });
            }
        };

        // RbacGate/RequireAuth already consumed the single-use NIP-98 event.
        // Only server-side request extensions carry this identity; headers
        // cannot populate it. Reuse authentication, preserving user privileges.
        if let Some(identity) = crate::middleware::auth::get_authenticated_user(req) {
            return Box::pin(async move {
                let user = nostr_service
                    .get_user(&identity.pubkey)
                    .await
                    .ok_or_else(|| ErrorUnauthorized("Authenticated user unavailable"))?;
                Ok(AuthenticatedUser {
                    pubkey: user.pubkey,
                    is_power_user: user.is_power_user,
                })
            });
        }

        // Extract Authorization header
        let auth_header = match req.headers().get("Authorization") {
            Some(header) => match header.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    debug!("Invalid Authorization header format");
                    return Box::pin(async {
                        Err(ErrorUnauthorized("Invalid authorization header"))
                    });
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
            let scheme = req
                .headers()
                .get("X-Forwarded-Proto")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.scheme())
                .to_string();
            let host = req
                .headers()
                .get("X-Forwarded-Host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.host())
                .to_string();
            let url = format!(
                "{}://{}{}",
                scheme,
                host,
                req.uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            );
            let method = req.method().as_str().to_string();

            return Box::pin(async move {
                match nostr_service
                    .verify_nip98_auth(&auth_header, &url, &method, None)
                    .await
                {
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
                if !crate::utils::auth::dev_bypass_permitted(req) {
                    warn!(
                        "dev-auth: rejected dev-session-token for pubkey {} — requires DEV_AUTH_LOOPBACK=1 and a loopback peer (peer={:?})",
                        pubkey,
                        req.peer_addr()
                    );
                    return Box::pin(async {
                        Err(ErrorUnauthorized("Invalid or expired session"))
                    });
                }
                debug!(
                    "dev-auth: Bearer dev-session-token accepted for pubkey: {} (loopback + DEV_AUTH_LOOPBACK)",
                    pubkey
                );
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

    #[actix_web::test]
    async fn middleware_identity_is_reused_but_token_cannot_cross_requests() {
        use crate::utils::nip98::{generate_nip98_token, Nip98Config};
        use actix_web::{test::TestRequest, HttpMessage};
        let service = web::Data::new(NostrService::new());
        let keys = nostr_sdk::prelude::Keys::generate();
        let token = generate_nip98_token(
            &keys,
            &Nip98Config {
                url: "http://localhost/api/bots/update".into(),
                method: "POST".into(),
                body: None,
            },
        )
        .unwrap();
        let header = format!("Nostr {token}");
        let user = service
            .verify_nip98_auth(&header, "http://localhost/api/bots/update", "POST", None)
            .await
            .unwrap();
        let request = || {
            TestRequest::post()
                .uri("/api/bots/update")
                .insert_header(("Host", "localhost"))
                .insert_header(("Authorization", header.clone()))
                .app_data(service.clone())
                .to_http_request()
        };
        let req = request();
        req.extensions_mut()
            .insert(crate::middleware::auth::AuthenticatedUser {
                pubkey: user.pubkey.clone(),
            });
        let extracted = AuthenticatedUser::from_request(&req, &mut actix_web::dev::Payload::None)
            .await
            .unwrap();
        assert_eq!(extracted.pubkey, user.pubkey);
        assert_eq!(extracted.is_power_user, user.is_power_user);
        assert!(
            AuthenticatedUser::from_request(&request(), &mut actix_web::dev::Payload::None)
                .await
                .is_err()
        );
    }

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

    /// The REST extractor's dev-token path must go through the same loopback +
    /// opt-in gate: a `Bearer dev-session-token` from a non-loopback peer, or
    /// without `DEV_AUTH_LOOPBACK=1`, yields no `AuthenticatedUser`. Cases run
    /// sequentially — they share a process env var.
    #[cfg(any(debug_assertions, feature = "dev-auth"))]
    #[test]
    fn try_dev_bypass_is_loopback_and_optin_gated() {
        use actix_web::test::TestRequest;

        let build = |peer: &str| {
            TestRequest::default()
                .insert_header(("Authorization", "Bearer dev-session-token"))
                .peer_addr(peer.parse().unwrap())
                .to_http_request()
        };

        std::env::set_var("DEV_AUTH_LOOPBACK", "1");
        assert!(
            try_dev_bypass(&build("127.0.0.1:5000")).is_some(),
            "loopback + opt-in must yield a dev user"
        );
        assert!(
            try_dev_bypass(&build("203.0.113.7:5000")).is_none(),
            "non-loopback peer must be refused even with opt-in"
        );

        std::env::remove_var("DEV_AUTH_LOOPBACK");
        assert!(
            try_dev_bypass(&build("127.0.0.1:5000")).is_none(),
            "no opt-in must refuse even from loopback"
        );
    }
}
