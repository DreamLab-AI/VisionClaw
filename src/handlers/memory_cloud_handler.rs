//! Live memory cloud endpoints (`/api/memory-cloud*`).
//!
//! - `GET  /api/memory-cloud` — stratified, PCA-projected snapshot (JSON)
//! - `GET  /api/memory-cloud/vectors?snapshot=<id>` — the snapshot's
//!   L2-normalised vectors as little-endian f32 (409 when `<id>` is stale)
//! - `POST /api/memory-cloud/query` — embed a query and return the sidecar's
//!   own HNSW top-k (per-pubkey rate limited)
//! - `GET  /api/memory-cloud/health` — cached sidecar/embedder status,
//!   per-namespace counts and the index-versus-exact recall probe
//!
//! Wire types: `client/src/features/visualisation/memoryCloud/types.ts`,
//! mirrored by `visionclaw_memory_cloud::wire`. All logic lives in
//! `services::memory_cloud_service` and the `visionclaw-memory-cloud` crate.
//!
//! **Access (ADR-2133).** Every endpoint, health included, exposes private
//! memory or its shape, so each requires a **NIP-98-signed power user**
//! (`Admin` role or a `POWER_USER_PUBKEYS` key), even with
//! `RBAC_PUBLIC_READS=1` or `DEV_AUTH_LOOPBACK` (a header-chosen pubkey proves
//! nothing). The one exception is the dev bypass, `VISIONCLAW_DEV_MODE=1`:
//! the operator's choice for a local dev box, it admits every caller as a
//! power user. It exists only in debug and `dev-auth` builds, and a release
//! build refuses to boot with the variable set (`utils::auth`).
//!
//! **Errors.** Clients get fixed messages; driver and connection detail goes
//! to the log only.

use std::collections::HashMap;
use std::time::Duration;

use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER};
use actix_web::middleware::{from_fn, Next};
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use log::warn;

use crate::middleware::rate_limit::RateLimitConfig;
use crate::middleware::{get_authenticated_user, AuthenticatedUser, RateLimit};
use crate::services::memory_cloud_service::{MemoryCloudError, MemoryCloudService};
use crate::services::nostr_service::NostrService;
use crate::utils::auth::{
    dev_full_bypass_active, effective_access_level, nip98_request_url, AccessLevel, DEV_MODE_PUBKEY,
};
use visionclaw_memory_cloud::validate::validate_query;
use visionclaw_memory_cloud::wire::{ErrorBody, MemoryCloudQueryRequest};

/// Required level for every memory-cloud endpoint.
const PRIVATE_LEVEL: AccessLevel = AccessLevel::PowerUser;

/// Admit a NIP-98-signed power user, or anyone under the dev bypass.
///
/// Under `VISIONCLAW_DEV_MODE=1` every caller is admitted as the dev-mode
/// identity (its query budget is then shared). Other dev shortcuts are refused.
///
/// An `AuthenticatedUser` left by `RbacGate` is reused only when the request
/// carries a NIP-98 header and the identity is not the dev-mode sentinel:
/// with a `Nostr` header `verify_access` can only have produced it from that
/// signature, and re-verifying would trip the single-use replay cache. In
/// every other case (public read, dev bypass, gate in report mode) the
/// signature is verified here directly, without `verify_access` and its
/// dev shortcuts.
async fn require_power_user(req: &HttpRequest) -> Result<String, HttpResponse> {
    if dev_full_bypass_active() {
        return Ok(DEV_MODE_PUBKEY.to_string());
    }
    let Some(nostr) = req.app_data::<web::Data<NostrService>>() else {
        warn!("[MemoryCloud] NostrService missing from app data; refusing");
        return Err(unauthorised());
    };
    let Some(header) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("Nostr "))
    else {
        return Err(unauthorised());
    };
    let pubkey = match get_authenticated_user(req) {
        Some(user) if user.pubkey != DEV_MODE_PUBKEY => user.pubkey,
        _ => {
            let url = nip98_request_url(req);
            match nostr
                .verify_nip98_auth(header, &url, req.method().as_str(), None)
                .await
            {
                Ok(user) => user.pubkey,
                Err(e) => {
                    warn!("[MemoryCloud] NIP-98 verification failed: {e}");
                    return Err(unauthorised());
                }
            }
        }
    };
    if effective_access_level(&pubkey, nostr)
        .await
        .has_permission(&PRIVATE_LEVEL)
    {
        Ok(pubkey)
    } else {
        Err(HttpResponse::Forbidden().json(ErrorBody::new(
            "the memory cloud requires a power user or Admin role",
        )))
    }
}

/// Admission in front of the query rate limit: refuses non-power callers
/// before they spend budget, and records the **verified** pubkey so the limit
/// is per signer. (`RbacGate` under dev mode tags every caller with one
/// sentinel identity, which would otherwise make the budget shared.)
async fn admit_power_user(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    match require_power_user(req.request()).await {
        Ok(pubkey) => {
            req.extensions_mut().insert(AuthenticatedUser { pubkey });
            Ok(next.call(req).await?.map_into_boxed_body())
        }
        Err(denied) => Ok(req.into_response(denied)),
    }
}

fn unauthorised() -> HttpResponse {
    HttpResponse::Unauthorized().json(ErrorBody::new(
        "a NIP-98 signature from a power user is required",
    ))
}

/// Log the detailed error; answer with its fixed public message.
fn error_response(err: &MemoryCloudError) -> HttpResponse {
    warn!("[MemoryCloud] request failed: {err}");
    let body = ErrorBody::new(err.public_message());
    match err {
        MemoryCloudError::Building => HttpResponse::ServiceUnavailable()
            .insert_header((RETRY_AFTER, "10"))
            .json(body),
        MemoryCloudError::Unconfigured(_)
        | MemoryCloudError::Sidecar(_)
        | MemoryCloudError::Embedder(_) => HttpResponse::ServiceUnavailable().json(body),
    }
}

/// `GET /api/memory-cloud`
pub async fn get_snapshot(
    req: HttpRequest,
    service: web::Data<MemoryCloudService>,
) -> HttpResponse {
    if let Err(denied) = require_power_user(&req).await {
        return denied;
    }
    match service.into_inner().snapshot().await {
        Ok(snap) => HttpResponse::Ok()
            .insert_header((CONTENT_TYPE, "application/json"))
            .insert_header((CACHE_CONTROL, "private, no-cache"))
            .body(snap.json.clone()),
        Err(e) => error_response(&e),
    }
}

/// `GET /api/memory-cloud/vectors?snapshot=<id>`
pub async fn get_vectors(
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>,
    service: web::Data<MemoryCloudService>,
) -> HttpResponse {
    if let Err(denied) = require_power_user(&req).await {
        return denied;
    }
    let Some(wanted) = query.get("snapshot").filter(|s| !s.is_empty()) else {
        return HttpResponse::BadRequest()
            .json(ErrorBody::new("query parameter 'snapshot' is required"));
    };
    let snap = match service.into_inner().snapshot().await {
        Ok(snap) => snap,
        Err(e) => return error_response(&e),
    };
    if wanted != snap.id() {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("snapshot '{wanted}' is no longer current"),
            "currentSnapshotId": snap.id(),
        }));
    }
    HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/octet-stream"))
        .insert_header((CACHE_CONTROL, "no-store"))
        .insert_header(("X-Memory-Cloud-Dim", snap.built.snapshot.dim.to_string()))
        .insert_header((
            "X-Memory-Cloud-Count",
            snap.built.snapshot.count.to_string(),
        ))
        .body(snap.blob.clone())
}

/// `POST /api/memory-cloud/query`
pub async fn post_query(
    req: HttpRequest,
    body: web::Json<MemoryCloudQueryRequest>,
    service: web::Data<MemoryCloudService>,
) -> HttpResponse {
    if let Err(denied) = require_power_user(&req).await {
        return denied;
    }
    let validated = match validate_query(&body, &service.config().excluded) {
        Ok(q) => q,
        Err(e) => return HttpResponse::BadRequest().json(ErrorBody::new(e.to_string())),
    };
    match service.into_inner().query(validated).await {
        Ok(resp) => HttpResponse::Ok()
            .insert_header((CACHE_CONTROL, "private, no-store"))
            .json(resp),
        Err(e) => error_response(&e),
    }
}

/// `GET /api/memory-cloud/health` — 200 with problems reported inline,
/// served from cached state only.
pub async fn get_health(req: HttpRequest, service: web::Data<MemoryCloudService>) -> HttpResponse {
    if let Err(denied) = require_power_user(&req).await {
        return denied;
    }
    HttpResponse::Ok()
        .insert_header((CACHE_CONTROL, "no-store"))
        .json(service.into_inner().health().await)
}

/// The per-pubkey budget for `POST /api/memory-cloud/query`.
///
/// Build it once, outside `HttpServer::new`, and pass clones to
/// [`configure_routes`]: clones share one history, so the budget holds
/// across workers. Keyed on the pubkey that admission verified, so only
/// admitted power users spend budget and each spends their own.
pub fn query_rate_limit(per_minute: usize) -> RateLimit {
    RateLimit::new(
        RateLimitConfig::new(per_minute, Duration::from_secs(60))
            .with_user_id()
            .with_message("memory cloud query budget exhausted; retry in a minute".into()),
    )
}

/// Register the routes under the `/api` scope. The shared
/// `web::Data<MemoryCloudService>` is installed once in `main.rs` so all
/// workers share one cache and one refresher; `query_limit` comes from
/// [`query_rate_limit`].
pub fn configure_routes(query_limit: RateLimit) -> impl FnOnce(&mut web::ServiceConfig) {
    move |cfg| {
        cfg.route("/memory-cloud", web::get().to(get_snapshot))
            .route("/memory-cloud/vectors", web::get().to(get_vectors))
            .service(
                // `wrap` order: the last is outermost, so admission runs
                // before the limiter.
                web::resource("/memory-cloud/query")
                    .wrap(query_limit)
                    .wrap(from_fn(admit_power_user))
                    .route(web::post().to(post_query)),
            )
            .route("/memory-cloud/health", web::get().to(get_health));
    }
}
