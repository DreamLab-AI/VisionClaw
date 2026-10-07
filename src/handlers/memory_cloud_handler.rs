//! Live memory cloud endpoints (`/api/memory-cloud*`).
//!
//! - `GET  /api/memory-cloud` — stratified, PCA-projected snapshot (JSON)
//! - `GET  /api/memory-cloud/vectors?snapshot=<id>` — the snapshot's
//!   L2-normalised vectors as little-endian f32 (409 when `<id>` is stale)
//! - `POST /api/memory-cloud/query` — embed a query and return the sidecar's
//!   own HNSW top-k
//! - `GET  /api/memory-cloud/health` — sidecar/embedder status, per-namespace
//!   counts and the cached index-versus-exact recall probe
//!
//! Wire types: `client/src/features/visualisation/memoryCloud/types.ts`,
//! mirrored by `visionclaw_memory_cloud::wire`. All logic lives in
//! `services::memory_cloud_service` and the `visionclaw-memory-cloud` crate.
//!
//! **Access.** Snapshot, vectors and query expose private memory (keys,
//! embeddings, value snippets), so they require a power user (`Admin` role or
//! power-user pubkey) — the same bar as the broker inbox, because any fresh
//! NIP-98 keypair already reaches the default `Editor` tier. The check runs
//! regardless of `RBAC_PUBLIC_READS`. Health carries only aggregate counts
//! for non-excluded namespaces and follows the central `/api` read policy.

use std::collections::HashMap;

use actix_web::http::header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER};
use actix_web::{web, HttpRequest, HttpResponse};
use log::warn;

use crate::middleware::get_authenticated_user;
use crate::services::memory_cloud_service::{MemoryCloudError, MemoryCloudService};
use crate::services::nostr_service::NostrService;
use crate::utils::auth::{effective_access_level, verify_access, AccessLevel};
use visionclaw_memory_cloud::validate::validate_query;
use visionclaw_memory_cloud::wire::{ErrorBody, MemoryCloudQueryRequest};

/// Required level for the private-data endpoints.
const PRIVATE_LEVEL: AccessLevel = AccessLevel::PowerUser;

/// Admit the caller to a private-data endpoint.
///
/// If `RbacGate` already verified this request's NIP-98 signature it left an
/// `AuthenticatedUser` behind; re-verifying would trip the single-use replay
/// cache, so only that pubkey's role is resolved. Otherwise (a public read let
/// through by `RBAC_PUBLIC_READS=1`) the request is verified here.
async fn require_private_reader(req: &HttpRequest) -> Result<(), HttpResponse> {
    let Some(nostr) = req.app_data::<web::Data<NostrService>>() else {
        warn!("[MemoryCloud] NostrService missing from app data; refusing");
        return Err(HttpResponse::Unauthorized().json(ErrorBody::new("authentication unavailable")));
    };
    match get_authenticated_user(req) {
        Some(user) => {
            if effective_access_level(&user.pubkey, nostr)
                .await
                .has_permission(&PRIVATE_LEVEL)
            {
                Ok(())
            } else {
                Err(HttpResponse::Forbidden().json(ErrorBody::new(
                    "the memory cloud requires a power user or Admin role",
                )))
            }
        }
        None => verify_access(req, nostr, PRIVATE_LEVEL).await.map(|_| ()),
    }
}

fn error_response(err: &MemoryCloudError) -> HttpResponse {
    let body = ErrorBody::new(err.to_string());
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
    if let Err(denied) = require_private_reader(&req).await {
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
    if let Err(denied) = require_private_reader(&req).await {
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
        .insert_header((CACHE_CONTROL, "private, max-age=86400, immutable"))
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
    if let Err(denied) = require_private_reader(&req).await {
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

/// `GET /api/memory-cloud/health` — always 200; problems are reported inline.
pub async fn get_health(service: web::Data<MemoryCloudService>) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header((CACHE_CONTROL, "no-store"))
        .json(service.health().await)
}

/// Register the routes under the `/api` scope. The shared
/// `web::Data<MemoryCloudService>` is installed once in `main.rs` so all
/// workers share one cache and one refresher.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/memory-cloud", web::get().to(get_snapshot))
        .route("/memory-cloud/vectors", web::get().to(get_vectors))
        .route("/memory-cloud/query", web::post().to(post_query))
        .route("/memory-cloud/health", web::get().to(get_health));
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use visionclaw_memory_cloud::config::MemoryCloudConfig;

    fn unconfigured() -> web::Data<MemoryCloudService> {
        web::Data::from(MemoryCloudService::new(
            MemoryCloudConfig::from_lookup(|_| None),
            None,
        ))
    }

    #[actix_web::test]
    async fn health_reports_unconfigured_sidecar_with_200() {
        let app = test::init_service(
            App::new()
                .app_data(unconfigured())
                .service(web::scope("/api").configure(configure_routes)),
        )
        .await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/memory-cloud/health")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["sidecar"]["reachable"], false);
        assert!(body["sidecar"]["error"]
            .as_str()
            .unwrap()
            .contains("RUVECTOR_PG_CONNINFO"));
        assert_eq!(body["recallProbe"], serde_json::Value::Null);
    }

    #[actix_web::test]
    async fn private_endpoints_refuse_anonymous_callers() {
        let app = test::init_service(
            App::new()
                .app_data(unconfigured())
                .app_data(web::Data::new(NostrService::default()))
                .service(web::scope("/api").configure(configure_routes)),
        )
        .await;
        for req in [
            test::TestRequest::get().uri("/api/memory-cloud"),
            test::TestRequest::get().uri("/api/memory-cloud/vectors?snapshot=x"),
            test::TestRequest::post()
                .uri("/api/memory-cloud/query")
                .set_json(serde_json::json!({ "text": "hello" })),
        ] {
            let resp = test::call_service(&app, req.to_request()).await;
            // Under VISIONCLAW_DEV_MODE=1 (dev builds) everything is admitted
            // and the unconfigured service answers 503 instead.
            let status = resp.status().as_u16();
            assert!(
                [401, 403, 503].contains(&status),
                "unexpected status {status}"
            );
            if !crate::utils::auth::dev_full_bypass_active() {
                assert!([401, 403].contains(&status), "anonymous got {status}");
            }
        }
    }
}
