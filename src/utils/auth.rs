use actix_web::{HttpRequest, HttpResponse};
use log::warn;
use tracing::{debug, info};
use uuid:::Uuid;
use crate::services::nostr_service:::NostrService;

#[derive(Clone, Debug)]
pub enum AccessLevel {
    Authenticated,
    PowerUser,
}

pub async fn verify_access(
    req: &HttpRequest,
    nostr_service: &NostrService,
    required_level: AccessLevel,
) -> Result<String, HttpResponse> {
    
    let request_id = req.headers()
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&Uuid::new_v4().to_string())
        .to_string();

    
    let pubkey = match req.headers().get("X-Nostr-Pubkey") {
        Some(value) => value.to_str().unwrap_or("").to_string(),
        None => {
            warn!("Missing Nostr pubkey in request headers");
            debug!(
                request_id = %request_id,
                "Authentication failed - missing pubkey header"
            );
            return Err(HttpResponse::Forbidden().body("Authentication required"));
        }
    };

    
    let token = match req.headers().get("X-Nostr-Token") {
        Some(value) => value.to_str().unwrap_or("").to_string(),
        None => {
            warn!("Missing Nostr token in request headers");
            debug!(
                request_id = %request_id,
                has_pubkey = true,
                "Authentication failed - missing token header"
            );
            return Err(HttpResponse::Forbidden().body("Authentication required"));
        }
    };

    
    debug!(
        request_id = %request_id,
        has_pubkey = !pubkey.is_empty(),
        has_token = !token.is_empty(),
        pubkey_prefix = %&pubkey.chars().take(8).collect::<String>(),
        "Authentication headers extracted"
    );

    
    if !nostr_service.validate_session(&pubkey, &token).await {
        warn!("Invalid or expired session for user {}", pubkey);
        debug!(
            request_id = %request_id,
            pubkey = %pubkey,
            "Session validation failed"
        );
        return Err(HttpResponse::Unauthorized().body("Invalid or expired session"));
    }

    info!(
        request_id = %request_id,
        pubkey = %pubkey,
        "Session validated successfully"
    );

    
    match required_level {
        AccessLevel::Authenticated => {
            
            debug!(
                request_id = %request_id,
                pubkey = %pubkey,
                access_level = "authenticated",
                "Access granted"
            );
            Ok(pubkey)
        }
        AccessLevel::PowerUser => {
            if nostr_service.is_power_user(&pubkey).await {
                debug!(
                    request_id = %request_id,
                    pubkey = %pubkey,
                    access_level = "power_user",
                    "Power user access granted"
                );
                Ok(pubkey)
            } else {
                warn!("Non-power user {} attempted restricted operation", pubkey);
                debug!(
                    request_id = %request_id,
                    pubkey = %pubkey,
                    "Power user access denied"
                );
                Err(HttpResponse::Forbidden().body("This operation requires power user access"))
            }
        }
    }
}

// Helper function for handlers that require power user access
pub async fn verify_power_user(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::PowerUser).await
}

// Helper function for handlers that require authentication
pub async fn verify_authenticated(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::Authenticated).await
}