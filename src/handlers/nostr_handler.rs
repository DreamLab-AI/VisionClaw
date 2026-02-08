use crate::app_state::AppState;
use crate::config::feature_access::FeatureAccess;
use crate::models::protected_settings::ApiKeys;
use crate::services::nostr_service::{AuthEvent, NostrError, NostrService};
use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::{
    ok_json, created_json, error_json, bad_request, not_found,
    unauthorized, forbidden, conflict, no_content, accepted,
    too_many_requests, service_unavailable, payload_too_large
};



#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub user: UserResponseDTO,
    pub token: String,
    pub expires_at: i64,
    pub features: Vec<String>,
}

// Data transfer object for user response that matches client expectations
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponseDTO {
    pub pubkey: String,
    pub npub: Option<String>,
    pub is_power_user: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub valid: bool,
    pub user: Option<UserResponseDTO>,
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeysRequest {
    pub perplexity: Option<String>,
    pub openai: Option<String>,
    pub ragflow: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRequest {
    pub pubkey: String,
    pub token: String,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth/nostr") 
            .route("", web::post().to(login))
            .route("", web::delete().to(logout))
            .route("/verify", web::post().to(verify))
            .route("/refresh", web::post().to(refresh))
            .route("/api-keys", web::post().to(update_api_keys))
            .route("/api-keys", web::get().to(get_api_keys))
            .route("/power-user-status", web::get().to(check_power_user_status))
            .route("/features", web::get().to(get_available_features))
            .route("/features/{feature}", web::get().to(check_feature_access)),
    );
}

async fn check_power_user_status(
    req: HttpRequest,
    feature_access: web::Data<FeatureAccess>,
) -> Result<HttpResponse, actix_web::Error> {
    let pubkey = req
        .headers()
        .get("X-Nostr-Pubkey")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if pubkey.is_empty() {
        return bad_request!("Missing Nostr pubkey");
    }

    ok_json!(json!({
        "is_power_user": feature_access.is_power_user(pubkey)
    }))
}

async fn get_available_features(
    req: HttpRequest,
    feature_access: web::Data<FeatureAccess>,
) -> Result<HttpResponse, actix_web::Error> {
    let pubkey = req
        .headers()
        .get("X-Nostr-Pubkey")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if pubkey.is_empty() {
        return bad_request!("Missing Nostr pubkey");
    }

    let features = feature_access.get_available_features(pubkey);
    ok_json!(json!({
        "features": features
    }))
}

async fn check_feature_access(
    req: HttpRequest,
    feature_access: web::Data<FeatureAccess>,
    feature: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let pubkey = req
        .headers()
        .get("X-Nostr-Pubkey")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if pubkey.is_empty() {
        return bad_request!("Missing Nostr pubkey");
    }

    ok_json!(json!({
        "has_access": feature_access.has_feature_access(pubkey, &feature)
    }))
}

async fn login(
    event: web::Json<AuthEvent>,
    nostr_service: web::Data<NostrService>,
    feature_access: web::Data<FeatureAccess>,
) -> Result<HttpResponse, actix_web::Error> {
    match nostr_service.verify_auth_event(event.into_inner()).await {
        Ok(user) => {
            let token = user.session_token.clone().unwrap_or_default();
            let expires_at = user.last_seen
                + std::env::var("AUTH_TOKEN_EXPIRY")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse::<i64>()
                    .unwrap_or(3600);

            
            let features = feature_access.get_available_features(&user.pubkey);

            
            let user_dto = UserResponseDTO {
                pubkey: user.pubkey.clone(),
                npub: Some(user.npub.clone()),
                is_power_user: user.is_power_user,
            };

            ok_json!(AuthResponse {
                user: user_dto,
                token,
                expires_at,
                features,
            })
        }
        Err(NostrError::InvalidSignature) => Ok(HttpResponse::Unauthorized().json(json!({
            "error": "Invalid signature"
        }))),
        Err(e) => error_json!("Authentication error: {}", e),
    }
}

async fn logout(
    req: web::Json<ValidateRequest>,
    nostr_service: web::Data<NostrService>,
) -> Result<HttpResponse, actix_web::Error> {
    
    if !nostr_service
        .validate_session(&req.pubkey, &req.token)
        .await
    {
        return Ok(HttpResponse::Unauthorized().json(json!({
            "error": "Invalid session"
        })));
    }

    match nostr_service.logout(&req.pubkey).await {
        Ok(_) => ok_json!(json!({
            "message": "Logged out successfully"
        })),
        Err(e) => error_json!("Logout error: {}", e),
    }
}

async fn verify(
    req: web::Json<ValidateRequest>,
    nostr_service: web::Data<NostrService>,
    feature_access: web::Data<FeatureAccess>,
) -> Result<HttpResponse, actix_web::Error> {
    let is_valid = nostr_service
        .validate_session(&req.pubkey, &req.token)
        .await;
    let user = if is_valid {
        nostr_service
            .get_user(&req.pubkey)
            .await
            .map(|u| UserResponseDTO {
                pubkey: u.pubkey,
                npub: Some(u.npub),
                is_power_user: u.is_power_user,
            })
    } else {
        None
    };

    
    let features = if is_valid {
        feature_access.get_available_features(&req.pubkey)
    } else {
        Vec::new()
    };

    ok_json!(VerifyResponse {
        valid: is_valid,
        user,
        features,
    })
}

async fn refresh(
    req: web::Json<ValidateRequest>,
    nostr_service: web::Data<NostrService>,
    feature_access: web::Data<FeatureAccess>,
) -> Result<HttpResponse, actix_web::Error> {
    
    if !nostr_service
        .validate_session(&req.pubkey, &req.token)
        .await
    {
        return Ok(HttpResponse::Unauthorized().json(json!({
            "error": "Invalid session"
        })));
    }

    match nostr_service.refresh_session(&req.pubkey).await {
        Ok(new_token) => {
            if let Some(user) = nostr_service.get_user(&req.pubkey).await {
                let expires_at = user.last_seen
                    + std::env::var("AUTH_TOKEN_EXPIRY")
                        .unwrap_or_else(|_| "3600".to_string())
                        .parse::<i64>()
                        .unwrap_or(3600);
                
                let features = feature_access.get_available_features(&req.pubkey);

                ok_json!(AuthResponse {
                    user: UserResponseDTO {
                        pubkey: user.pubkey.clone(),
                        npub: Some(user.npub.clone()),
                        is_power_user: user.is_power_user,
                    },
                    token: new_token,
                    expires_at,
                    features,
                })
            } else {
                error_json!("User not found after refresh")
            }
        }
        Err(e) => error_json!("Session refresh error: {}", e),
    }
}

async fn update_api_keys(
    req: web::Json<ApiKeysRequest>,
    nostr_service: web::Data<NostrService>,
    pubkey: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let api_keys = ApiKeys {
        perplexity: req.perplexity.clone(),
        openai: req.openai.clone(),
        ragflow: req.ragflow.clone(),
    };

    match nostr_service.update_user_api_keys(&pubkey, api_keys).await {
        Ok(user) => {
            let user_dto = UserResponseDTO {
                pubkey: user.pubkey.clone(),
                npub: Some(user.npub.clone()),
                is_power_user: user.is_power_user,
            };
            ok_json!(user_dto)
        }
        Err(NostrError::UserNotFound) => not_found!("User not found"),
        Err(NostrError::PowerUserOperation) => Ok(HttpResponse::Forbidden().json(json!({
            "error": "Cannot update API keys for power users"
        }))),
        Err(e) => error_json!("Failed to update API keys: {}", e),
    }
}

async fn get_api_keys(
    state: web::Data<AppState>,
    pubkey: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let api_keys = state.get_api_keys(&pubkey).await;

    ok_json!(api_keys)
}

// Add the handler to app_state initialization
pub async fn init_nostr_service(app_state: &mut AppState) {
    let nostr_service = NostrService::new();

    // Initialize and restore sessions from Redis (if available)
    match nostr_service.initialize().await {
        Ok(count) => {
            if count > 0 {
                log::info!("[NostrService] Restored {} sessions from persistent storage", count);
            }
            if nostr_service.has_redis() {
                log::info!("[NostrService] Redis session persistence enabled");
            } else {
                log::warn!("[NostrService] No Redis configured - sessions will be lost on restart");
            }
        }
        Err(e) => {
            log::error!("[NostrService] Failed to restore sessions: {}", e);
        }
    }

    // Start session cleanup task
    let service_clone = nostr_service.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            service_clone.cleanup_sessions(24).await;
        }
    });

    app_state.nostr_service = Some(web::Data::new(nostr_service));
}
