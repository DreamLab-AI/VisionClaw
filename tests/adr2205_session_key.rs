//! ADR-2205 — browser session-key binding, exercised through the real routes.
//!
//! The browser signs ONE NIP-98 token with the user's real signer to bind an
//! ephemeral key (`POST /api/auth/nostr/session-key`); every later request is
//! signed by that ephemeral key and authenticates as the bound principal.
//!
//! Each test uses freshly generated keys because the NIP-98 replay cache is
//! process-global. The one power-user key is installed once, before the first
//! `NostrService` is built, so tests cannot race on `POWER_USER_PUBKEYS`.

use actix_web::{http::StatusCode, test, web, App, HttpRequest, HttpResponse};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use nostr_sdk::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use visionclaw_server::config::feature_access::FeatureAccess;
use visionclaw_server::handlers::{admin_rbac_handler, nostr_handler};
use visionclaw_server::middleware::{get_authenticated_user, RbacGate, RequireAuth};
use visionclaw_server::services::nostr_service::NostrService;

const BIND_PATH: &str = "/api/auth/nostr/session-key";
const HOST: &str = "http://localhost:8080";

fn power_keys() -> &'static Keys {
    static POWER: OnceLock<Keys> = OnceLock::new();
    POWER.get_or_init(|| {
        let keys = Keys::generate();
        std::env::set_var("POWER_USER_PUBKEYS", keys.public_key().to_hex());
        std::env::remove_var("RBAC_PUBLIC_READS");
        std::env::remove_var("RBAC_GATE_MODE");
        std::env::remove_var("VISIONCLAW_DEV_MODE");
        std::env::remove_var("SESSION_KEY_MAX_TTL_SECS");
        keys
    })
}

/// A NIP-98 `Authorization` value. `session` adds the signed session marker
/// naming the principal the session key claims to act for.
fn token(
    keys: &Keys,
    method: &str,
    path: &str,
    body: Option<&str>,
    session: Option<&str>,
) -> String {
    let mut tags = vec![
        Tag::custom(TagKind::Custom("u".into()), vec![format!("{HOST}{path}")]),
        Tag::custom(TagKind::Custom("method".into()), vec![method.to_string()]),
        Tag::custom(
            TagKind::Custom("nonce".into()),
            vec![uuid::Uuid::new_v4().to_string()],
        ),
    ];
    if let Some(body) = body {
        let hash = hex::encode(Sha256::digest(body.as_bytes()));
        tags.push(Tag::custom(TagKind::Custom("payload".into()), vec![hash]));
    }
    if let Some(principal) = session {
        tags.push(Tag::custom(
            TagKind::Custom("session".into()),
            vec![principal.to_string()],
        ));
    }
    let event = EventBuilder::new(Kind::Custom(27235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .expect("sign");
    format!("Nostr {}", B64.encode(event.as_json()))
}

async fn probe(req: HttpRequest) -> HttpResponse {
    let who = get_authenticated_user(&req)
        .map(|u| u.pubkey)
        .unwrap_or_default();
    let via = visionclaw_server::middleware::auth::via_session_key(&req);
    HttpResponse::Ok().json(json!({ "pubkey": who, "viaSessionKey": via }))
}

macro_rules! app {
    () => {{
        let _ = power_keys();
        test::init_service(
            App::new()
                .app_data(web::Data::new(NostrService::new()))
                .app_data(web::Data::new(FeatureAccess::from_env()))
                .service(
                    web::scope("/api")
                        .wrap(RbacGate::from_env())
                        .configure(nostr_handler::config)
                        .configure(admin_rbac_handler::configure_routes)
                        .service(
                            web::resource("/probe/power")
                                .wrap(RequireAuth::power_user())
                                .route(web::get().to(probe)),
                        )
                        // A mutation: the owner a write would stamp is the
                        // identity this returns.
                        .route("/probe/write", web::post().to(probe)),
                ),
        )
        .await
    }};
}

macro_rules! bind {
    ($app:expr, $signer:expr, $session:expr, $expires_in:expr) => {{
        let body = json!({ "sessionPubkey": $session.public_key().to_hex(), "expiresIn": $expires_in })
            .to_string();
        let auth = token($signer, "POST", BIND_PATH, Some(&body), None);
        let req = test::TestRequest::post()
            .uri(BIND_PATH)
            .insert_header(("Host", "localhost:8080"))
            .insert_header(("Authorization", auth))
            .insert_header(("Content-Type", "application/json"))
            .set_payload(body)
            .to_request();
        test::call_service(&$app, req).await
    }};
}

macro_rules! get_req {
    ($path:expr, $auth:expr) => {
        test::TestRequest::get()
            .uri($path)
            .insert_header(("Host", "localhost:8080"))
            .insert_header(("Authorization", $auth))
            .to_request()
    };
}

async fn json_of(resp: actix_web::dev::ServiceResponse) -> Value {
    let bytes = test::read_body(resp).await;
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

#[actix_web::test]
async fn bound_session_key_authorises_as_the_power_user_principal() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    let resp = bind!(app, p, &s, 600);
    assert_eq!(resp.status(), StatusCode::OK, "bind must succeed");
    let grant = json_of(resp).await;
    assert_eq!(grant["sessionPubkey"], s.public_key().to_hex());
    assert_eq!(grant["principal"], p.public_key().to_hex());
    assert!(grant["expiresAt"].as_i64().unwrap() > 0);

    let auth = token(
        &s,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::OK, "principal is a power user");
    let body = json_of(resp).await;
    assert_eq!(body["pubkey"], p.public_key().to_hex());
    assert_eq!(body["viaSessionKey"], true);
}

#[actix_web::test]
async fn a_write_via_session_key_records_the_principal_as_owner() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    assert_eq!(bind!(app, p, &s, 600).status(), StatusCode::OK);
    let body = "{}";
    let auth = token(
        &s,
        "POST",
        "/api/probe/write",
        Some(body),
        Some(&p.public_key().to_hex()),
    );
    let req = test::TestRequest::post()
        .uri("/api/probe/write")
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(body)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_of(resp).await;
    assert_eq!(v["pubkey"], p.public_key().to_hex());
    assert_ne!(v["pubkey"], s.public_key().to_hex());
}

#[actix_web::test]
async fn a_replayed_session_key_token_is_rejected() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    assert_eq!(bind!(app, p, &s, 600).status(), StatusCode::OK);
    let auth = token(
        &s,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let first = test::call_service(&app, get_req!("/api/probe/power", auth.clone())).await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn a_bind_whose_body_was_swapped_is_rejected_and_binds_nothing() {
    let app = app!();
    let p = power_keys();
    let honest = Keys::generate();
    let attacker = Keys::generate();
    let signed_body =
        json!({ "sessionPubkey": honest.public_key().to_hex(), "expiresIn": 600 }).to_string();
    let swapped =
        json!({ "sessionPubkey": attacker.public_key().to_hex(), "expiresIn": 600 }).to_string();
    let auth = token(p, "POST", BIND_PATH, Some(&signed_body), None);
    let req = test::TestRequest::post()
        .uri(BIND_PATH)
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(swapped)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // The attacker key is not bound: its marked request is not accepted.
    let auth = token(
        &attacker,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn a_bind_with_no_payload_tag_is_rejected() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    let body = json!({ "sessionPubkey": s.public_key().to_hex(), "expiresIn": 600 }).to_string();
    let auth = token(p, "POST", BIND_PATH, None, None);
    let req = test::TestRequest::post()
        .uri(BIND_PATH)
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(body)
        .to_request();
    assert_eq!(
        test::call_service(&app, req).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
async fn an_expired_binding_returns_the_distinct_session_key_expired_401() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    assert_eq!(bind!(app, p, &s, 1).status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    let auth = token(
        &s,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_of(resp).await["code"], "session_key_expired");
}

#[actix_web::test]
async fn a_marked_key_that_was_never_bound_gets_session_key_expired_not_an_identity() {
    // The restart case: the server forgot the binding. The client must see the
    // distinct 401 and re-bind, not silently become a fresh user.
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    let auth = token(
        &s,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_of(resp).await["code"], "session_key_expired");
}

#[actix_web::test]
async fn an_unbound_key_never_becomes_a_power_user() {
    let app = app!();
    let p = power_keys();
    let random = Keys::generate();
    // Unmarked: ordinary NIP-98 as itself, which is not a power user.
    let auth = token(&random, "GET", "/api/probe/power", None, None);
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // Claiming the power principal without a binding gets nothing either.
    let auth = token(
        &random,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn a_bound_key_cannot_claim_a_different_principal() {
    let app = app!();
    let p = power_keys();
    let user = Keys::generate();
    let s = Keys::generate();
    assert_eq!(bind!(app, &user, &s, 600).status(), StatusCode::OK);
    let auth = token(
        &s,
        "GET",
        "/api/probe/power",
        None,
        Some(&p.public_key().to_hex()),
    );
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    // Unmarked use of a bound key is refused as well.
    let auth = token(&s, "GET", "/api/probe/power", None, None);
    let resp = test::call_service(&app, get_req!("/api/probe/power", auth)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn session_keys_are_refused_on_the_deny_list() {
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    assert_eq!(bind!(app, p, &s, 600).status(), StatusCode::OK);
    let principal = p.public_key().to_hex();

    // api-keys (RequireAuth route under the public /api/auth scope).
    let auth = token(
        &s,
        "GET",
        "/api/auth/nostr/api-keys",
        None,
        Some(&principal),
    );
    let r = test::call_service(&app, get_req!("/api/auth/nostr/api-keys", auth)).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "api-keys");
    assert_eq!(json_of(r).await["code"], "session_key_scope");

    // RBAC role administration.
    let role_path = format!(
        "/api/admin/rbac/users/{}/role",
        Keys::generate().public_key().to_hex()
    );
    let body = json!({ "role": "viewer" }).to_string();
    let auth = token(&s, "PUT", &role_path, Some(&body), Some(&principal));
    let req = test::TestRequest::put()
        .uri(&role_path)
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(body)
        .to_request();
    let r = test::call_service(&app, req).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "rbac role");
    assert_eq!(json_of(r).await["code"], "session_key_scope");

    // Binding another session key.
    let other = Keys::generate();
    let body = json!({ "sessionPubkey": other.public_key().to_hex(), "expiresIn": 60 }).to_string();
    let auth = token(&s, "POST", BIND_PATH, Some(&body), Some(&principal));
    let req = test::TestRequest::post()
        .uri(BIND_PATH)
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(body)
        .to_request();
    let r = test::call_service(&app, req).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "bind");
    assert_eq!(json_of(r).await["code"], "session_key_scope");
}

#[actix_web::test]
async fn power_user_status_reports_the_principal_for_a_session_key() {
    // `/api/auth/nostr/power-user-status` used to trust `X-Nostr-Pubkey`.
    let app = app!();
    let p = power_keys();
    let s = Keys::generate();
    assert_eq!(bind!(app, p, &s, 600).status(), StatusCode::OK);
    let path = "/api/auth/nostr/power-user-status";
    let auth = token(&s, "GET", path, None, Some(&p.public_key().to_hex()));
    let req = test::TestRequest::get()
        .uri(path)
        .insert_header(("Host", "localhost:8080"))
        .insert_header(("Authorization", auth))
        .insert_header(("X-Nostr-Pubkey", s.public_key().to_hex()))
        .to_request();
    let r = test::call_service(&app, req).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json_of(r).await["data"]["is_power_user"], true);
}
