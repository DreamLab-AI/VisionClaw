//! `RbacGate` over `/api` + `RequireAuth` on an inner scope must verify a
//! NIP-98 token exactly once.
//!
//! NIP-98 event ids are single-use (`utils::nip98` replay cache). When the
//! central gate has already verified the request, an inner `RequireAuth`
//! that verifies the same `Authorization` header again sees its own event id
//! as a replay and answers 401 — locking every NIP-98 caller out of routes
//! such as the broker inbox and `/api/broker/cases/{id}/decide`.
//!
//! The test drives the real broker scope (`configure_broker_inbox_routes`)
//! under the real gate with the default environment (`RBAC_PUBLIC_READS`
//! unset, i.e. reads gated). It lives in its own test binary because it sets
//! `POWER_USER_PUBKEYS` before constructing `NostrService`; a single test
//! function keeps the environment free of races.

use actix_web::{http::StatusCode, test, web, App, HttpResponse};
use nostr_sdk::prelude::Keys;
use visionclaw_server::middleware::{RbacGate, RequireAuth};
use visionclaw_server::services::nostr_service::NostrService;
use visionclaw_server::settings::auth_extractor::AuthenticatedUser;
use visionclaw_server::utils::auth::nip98_request_url;
use visionclaw_server::utils::nip98::{build_auth_header, generate_nip98_token, Nip98Config};

/// Mirrors `decide_as_operator`'s authentication: the same scope wrapping and
/// the same `AuthenticatedUser` extractor, without the decision core's
/// `AppState` dependencies.
async fn whoami(user: AuthenticatedUser) -> HttpResponse {
    HttpResponse::Ok().body(user.pubkey)
}

/// A NIP-98 `Authorization` value signed over exactly the URL the server
/// rebuilds for `uri`.
fn signed(keys: &Keys, method: &str, uri: &str) -> String {
    let probe = test::TestRequest::default().uri(uri).to_http_request();
    let url = nip98_request_url(&probe);
    let token = generate_nip98_token(
        keys,
        &Nip98Config {
            url,
            method: method.to_string(),
            body: None,
        },
    )
    .expect("sign NIP-98 token");
    build_auth_header(&token)
}

#[actix_web::test]
async fn gate_and_require_auth_verify_a_nip98_token_once() {
    let power = Keys::generate();
    let editor = Keys::generate();
    // Default deployment posture: enforce mode, reads gated, no dev bypass.
    std::env::set_var("POWER_USER_PUBKEYS", power.public_key().to_hex());
    std::env::remove_var("RBAC_PUBLIC_READS");
    std::env::remove_var("RBAC_GATE_MODE");
    std::env::remove_var("VISIONCLAW_DEV_MODE");

    let nostr = web::Data::new(NostrService::new());
    let app = test::init_service(
        App::new().app_data(nostr.clone()).service(
            web::scope("/api")
                .wrap(RbacGate::from_env())
                .configure(visionclaw_server::handlers::configure_broker_inbox_routes)
                .service(
                    web::scope("/probe")
                        .wrap(RequireAuth::power_user())
                        .route("/decide", web::post().to(whoami)),
                ),
        ),
    )
    .await;

    // 1. A power user's signed read of the real broker inbox succeeds.
    let uri = "/api/broker/inbox";
    let req = test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", signed(&power, "GET", uri)))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status();
    let body = test::read_body(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "power user locked out of the broker inbox: {}",
        String::from_utf8_lossy(&body)
    );

    // 2. A signed POST through the decide-shaped stack reaches the handler,
    //    and the extractor sees the signer.
    let uri = "/api/probe/decide";
    let req = test::TestRequest::post()
        .uri(uri)
        .insert_header(("Authorization", signed(&power, "POST", uri)))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status();
    let body = test::read_body(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "power user locked out of a decide-shaped POST: {}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(body, power.public_key().to_hex().as_bytes());

    // 3. A valid signer below PowerUser passes the gate but is refused by the
    //    inner level check with 403 — not a 401 replay.
    let uri = "/api/broker/inbox";
    let req = test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", signed(&editor, "GET", uri)))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 4. A replayed token is still refused (the gate's own check). The event
    //    id hashes signer, second, URL and method, so a distinct query string
    //    keeps this token from colliding with case 1's within the same second.
    let uri = "/api/broker/inbox?case=replay";
    let header = signed(&power, "GET", uri);
    let first = test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", header.clone()))
        .to_request();
    assert_eq!(
        test::call_service(&app, first).await.status(),
        StatusCode::OK
    );
    let replay = test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", header))
        .to_request();
    assert_eq!(
        test::call_service(&app, replay).await.status(),
        StatusCode::UNAUTHORIZED
    );

    // 5. An unsigned request never reaches the inner scope.
    let req = test::TestRequest::get()
        .uri("/api/broker/inbox")
        .to_request();
    let status = test::call_service(&app, req).await.status();
    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN,
        "anonymous got {status}"
    );
}
