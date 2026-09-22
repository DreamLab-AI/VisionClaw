//! REC-1a / REC-1b regression guard (PRD-023 WP-12, CANARY-VC-REC1-ROUTE),
//! amended by ADR-2116.
//!
//! The original guard asserted that `/ontology-agent/propose` stayed behind
//! `RequireAuth::authenticated()`. ADR-2116 retires that route: ontology
//! proposals are forum ActionRequests signed by a human, not authenticated HTTP
//! writes, so there is no longer a mutating route on this surface to gate. The
//! guard is therefore **inverted rather than deleted** — it now asserts the
//! route stays retired (410 Gone, to authenticated and unauthenticated callers
//! alike), which is the property a later refactor could silently break by
//! re-introducing a write path.
//!
//! `/ontology/{load,load-axioms}` are unchanged and still gated
//! `power_user().mutations_only()`. Idiom mirrors the actix `test::init_service`
//! handler tests already in `ontology/mod.rs`.

use actix_web::{http::StatusCode, test, web, App};
use visionclaw_server::handlers::api_handler::ontology as ontology_routes;
use visionclaw_server::handlers::configure_ontology_agent_routes;
use visionclaw_server::services::nostr_service::NostrService;

/// A default `NostrService` with no valid sessions: the auth middleware runs its
/// REAL verification path against it, so an unauthenticated request is rejected
/// by the gate itself — not by a "service missing" shortcut.
fn nostr_data() -> web::Data<NostrService> {
    web::Data::new(NostrService::default())
}

/// The other routes on this surface (and the `/ontology` ingest routes) are
/// still auth-gated; only `/propose` stopped being one.
fn is_auth_rejected(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

#[actix_web::test]
async fn ontology_agent_propose_is_retired_and_answers_410() {
    let app = test::init_service(
        App::new()
            .app_data(nostr_data())
            .configure(configure_ontology_agent_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/ontology-agent/propose")
        .set_json(serde_json::json!({ "note": "x" }))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(
        resp.status(),
        StatusCode::GONE,
        "POST /ontology-agent/propose is retired (ADR-2116) and must answer 410, got {}",
        resp.status()
    );

    // The falsification target: a 401/403 here would mean the write path is
    // back behind an auth gate rather than gone, and a caller would retry with
    // better credentials forever.
    assert!(
        !matches!(
            resp.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ),
        "a retired route must not look like an auth failure"
    );

    // The body must name its replacement: a 410 that says nothing is a 404
    // with better manners.
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"], "route_retired");
    assert!(
        body["replacement"]["command"]
            .as_str()
            .unwrap_or_default()
            .starts_with("vault propose"),
        "the 410 body must point at `vault propose`, got {body}"
    );
}

#[actix_web::test]
async fn ontology_agent_propose_is_retired_for_authenticated_callers_too() {
    // There is no authenticated caller to construct here without a session, and
    // that is the point: the route no longer consults auth at all, so the SAME
    // request that used to be rejected for lacking credentials is now refused
    // for the route being gone. Asserting 410 on a request carrying an
    // Authorization header proves the middleware is off rather than passing.
    let app = test::init_service(
        App::new()
            .app_data(nostr_data())
            .configure(configure_ontology_agent_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/ontology-agent/propose")
        .insert_header(("Authorization", "Nostr deadbeef"))
        .set_json(serde_json::json!({ "note": "x" }))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::GONE);
}

#[actix_web::test]
async fn ontology_agent_read_side_is_not_auth_gated() {
    // Positive control: /propose's gate is SPECIFIC. The read routes stay
    // anonymous (WS-1/ADR-120), so GET /status reaches its handler (which then
    // 5xx's without AppState) rather than being auth-rejected.
    let app = test::init_service(
        App::new()
            .app_data(nostr_data())
            .configure(configure_ontology_agent_routes),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ontology-agent/status")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert!(
        !is_auth_rejected(resp.status()),
        "read-side GET /ontology-agent/status must not be auth-gated, got {}",
        resp.status()
    );
}

#[actix_web::test]
async fn ontology_load_rejects_unauthenticated_ingest() {
    let app = test::init_service(
        App::new()
            .app_data(nostr_data())
            .configure(ontology_routes::config),
    )
    .await;

    for path in ["/ontology/load", "/ontology/load-axioms"] {
        let req = test::TestRequest::post()
            .uri(path)
            .set_json(serde_json::json!({}))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(
            is_auth_rejected(resp.status()),
            "unauthenticated POST {path} must be power_user-gated, got {}",
            resp.status()
        );
    }
}

#[actix_web::test]
async fn ontology_read_get_stays_public() {
    // mutations_only bypasses safe GETs: /ontology/classes reaches its handler
    // (5xx without AppState) rather than being auth-rejected — proving the gate
    // is mutation-specific, not a blanket scope lock.
    let app = test::init_service(
        App::new()
            .app_data(nostr_data())
            .configure(ontology_routes::config),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ontology/classes")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert!(
        !is_auth_rejected(resp.status()),
        "read-side GET /ontology/classes must stay public, got {}",
        resp.status()
    );
}
