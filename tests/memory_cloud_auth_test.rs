//! Memory-cloud access control.
//!
//! Every `/api/memory-cloud*` endpoint exposes private memory or its shape.
//! Under the dev bypass (`VISIONCLAW_DEV_MODE=1`, compiled into debug and
//! `dev-auth` builds only; a release build refuses to boot with it set) every
//! caller is admitted as a power user, so a local operator needs no signer.
//! Without it, each endpoint requires a NIP-98-signed power user (Admin role
//! or a `POWER_USER_PUBKEYS` key) even with `DEV_AUTH_LOOPBACK=1` and
//! `RBAC_PUBLIC_READS=1`.
//!
//! Own test binary: it sets process environment before building each app,
//! and a single test function keeps that free of races.

use actix_web::{http::StatusCode, test, web, App};
use nostr_sdk::prelude::Keys;
use visionclaw_memory_cloud::config::MemoryCloudConfig;
use visionclaw_server::middleware::RbacGate;
use visionclaw_server::services::memory_cloud_service::MemoryCloudService;
use visionclaw_server::services::nostr_service::NostrService;
use visionclaw_server::utils::auth::{dev_full_bypass_active, nip98_request_url};
use visionclaw_server::utils::nip98::{build_auth_header, generate_nip98_token, Nip98Config};

/// A NIP-98 `Authorization` value signed over the URL the server rebuilds.
fn signed(keys: &Keys, method: &str, uri: &str) -> String {
    let probe = test::TestRequest::default().uri(uri).to_http_request();
    let token = generate_nip98_token(
        keys,
        &Nip98Config {
            url: nip98_request_url(&probe),
            method: method.to_string(),
            body: None,
        },
    )
    .expect("sign NIP-98 token");
    build_auth_header(&token)
}

/// `(method, uri)` for every memory-cloud endpoint. Each URI carries a
/// per-endpoint query string so tokens never collide on event id.
const ENDPOINTS: [(&str, &str); 4] = [
    ("GET", "/api/memory-cloud?t=snapshot"),
    ("GET", "/api/memory-cloud/vectors?snapshot=abc"),
    ("POST", "/api/memory-cloud/query?t=query"),
    ("GET", "/api/memory-cloud/health?t=health"),
];

fn request(method: &str, uri: &str) -> test::TestRequest {
    let req = match method {
        "POST" => test::TestRequest::post().set_json(serde_json::json!({ "text": "hello" })),
        _ => test::TestRequest::get(),
    };
    req.uri(uri)
}

#[actix_web::test]
async fn dev_mode_admits_everyone_and_otherwise_a_signed_power_user_is_required() {
    let power = Keys::generate();
    let power_b = Keys::generate();
    let editor = Keys::generate();
    std::env::set_var("VISIONCLAW_DEV_MODE", "1");
    std::env::set_var("DEV_AUTH_LOOPBACK", "1");
    std::env::set_var("RBAC_PUBLIC_READS", "1");
    std::env::remove_var("RBAC_GATE_MODE");
    std::env::set_var(
        "POWER_USER_PUBKEYS",
        format!(
            "{},{}",
            power.public_key().to_hex(),
            power_b.public_key().to_hex()
        ),
    );
    assert!(
        dev_full_bypass_active(),
        "this test must run with the dev bypass live (debug build)"
    );

    // Phase 1: dev bypass on. Every endpoint admits an anonymous caller,
    // which then gets the unconfigured-store answer (data) or 200 (health).
    {
        let nostr = web::Data::new(NostrService::new());
        let service = web::Data::from(MemoryCloudService::new(
            MemoryCloudConfig::from_lookup(|_| None),
            None,
        ));
        let app = test::init_service(
            App::new()
                .app_data(nostr.clone())
                .app_data(service.clone())
                .service(
                    web::scope("/api")
                        .wrap(RbacGate::from_env())
                        .configure(visionclaw_server::handlers::configure_memory_cloud_routes(
                            visionclaw_server::handlers::memory_cloud_query_rate_limit(30),
                        )),
                ),
        )
        .await;
        for (method, uri) in ENDPOINTS {
            let resp = test::call_service(&app, request(method, uri).to_request()).await;
            let want = if uri.contains("/health") {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            assert_eq!(resp.status(), want, "{method} {uri}: anonymous caller under dev mode");
        }
    }

    // Phase 2: dev bypass off; only a signed power user gets in.
    std::env::remove_var("VISIONCLAW_DEV_MODE");
    assert!(!dev_full_bypass_active(), "dev bypass must be off for phase 2");

    let nostr = web::Data::new(NostrService::new());
    // No RUVECTOR_PG_CONNINFO: an admitted caller reaches the handler and
    // gets the unconfigured-store answer, which proves admission.
    let service = web::Data::from(MemoryCloudService::new(
        MemoryCloudConfig::from_lookup(|_| None),
        None,
    ));
    let app = test::init_service(
        App::new()
            .app_data(nostr.clone())
            .app_data(service.clone())
            .service(
                web::scope("/api")
                    .wrap(RbacGate::from_env())
                    // A budget of two queries per pubkey per minute.
                    .configure(visionclaw_server::handlers::configure_memory_cloud_routes(
                        visionclaw_server::handlers::memory_cloud_query_rate_limit(2),
                    )),
            ),
    )
    .await;

    for (method, uri) in ENDPOINTS {
        // Anonymous.
        let resp = test::call_service(&app, request(method, uri).to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}: anonymous caller"
        );

        // The dev session token naming a power-user pubkey is not a signature.
        let resp = test::call_service(
            &app,
            request(method, uri)
                .insert_header(("Authorization", "Bearer dev-session-token"))
                .insert_header(("X-Nostr-Pubkey", power.public_key().to_hex()))
                .peer_addr("127.0.0.1:40000".parse().unwrap())
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}: dev session token"
        );

        // A genuine signer below PowerUser is refused.
        let resp = test::call_service(
            &app,
            request(method, uri)
                .insert_header(("Authorization", signed(&editor, method, uri)))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{method} {uri}: Editor signer"
        );
    }

    // A signed power user is admitted. The data endpoints then report the
    // unconfigured store generically, with no driver or environment detail.
    for (method, uri) in &ENDPOINTS[..3] {
        let uri = format!("{uri}&who=power");
        let resp = test::call_service(
            &app,
            request(method, &uri)
                .insert_header(("Authorization", signed(&power, method, &uri)))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {uri}: power user"
        );
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body,
            serde_json::json!({ "error": "memory store not configured" })
        );
    }

    // Health for a power user: 200, fixed category, no URL.
    let uri = "/api/memory-cloud/health?who=power";
    let resp = test::call_service(
        &app,
        request("GET", uri)
            .insert_header(("Authorization", signed(&power, "GET", uri)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["sidecar"]["reachable"], false);
    assert_eq!(body["sidecar"]["error"], "not_configured");
    assert_eq!(
        body["embedder"],
        serde_json::json!({ "model": "bge-small-en-v1.5", "reachable": false })
    );

    // Per-pubkey query budget. Power user A has spent one query above; the
    // refused anonymous, dev-token and Editor attempts spent nothing.
    let query = |keys: &Keys, n: u32| {
        let uri = format!("/api/memory-cloud/query?n={n}");
        request("POST", &uri)
            .insert_header(("Authorization", signed(keys, "POST", &uri)))
            .to_request()
    };
    let resp = test::call_service(&app, query(&power, 2)).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "A's second query"
    );
    // RateLimit refuses with an `Err(ErrorTooManyRequests)`, which the server
    // renders as a 429 response.
    let status = match test::try_call_service(&app, query(&power, 3)).await {
        Ok(resp) => resp.status(),
        Err(e) => e.as_response_error().status_code(),
    };
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "A's third query");
    // B has a budget of their own.
    let resp = test::call_service(&app, query(&power_b, 1)).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "B's first query"
    );
}
