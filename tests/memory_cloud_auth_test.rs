//! Memory-cloud access control under the most permissive dev posture.
//!
//! Every `/api/memory-cloud*` endpoint exposes private memory or its shape,
//! so each one requires a NIP-98-signed power user (Admin role or a
//! `POWER_USER_PUBKEYS` key) **even with** `VISIONCLAW_DEV_MODE=1`,
//! `DEV_AUTH_LOOPBACK=1` and `RBAC_PUBLIC_READS=1`. Dev mode stays in force
//! for the rest of the API; only the memory cloud ignores its shortcuts.
//!
//! Own test binary: it sets process environment before building the app, and
//! a single test function keeps that free of races.

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
async fn dev_mode_does_not_unlock_the_memory_cloud() {
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
        // Anonymous: dev mode would admit it anywhere else.
        let resp = test::call_service(&app, request(method, uri).to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}: anonymous caller under dev mode"
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
