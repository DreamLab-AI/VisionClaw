#![cfg(feature = "solid-pod-embed")]

use actix_web::{http::StatusCode, test, web, App};
use nostr_sdk::prelude::Keys;
use std::sync::Arc;
use visionclaw_server::handlers::pay_handler::{
    configure_pay_routes, FsExchangeStore, FsPaymentStore, VcPayConfig,
};
use visionclaw_server::utils::nip98::{build_auth_header, generate_nip98_token, Nip98Config};

fn auth(keys: &Keys, url: &str, method: &str, body: Option<&str>) -> String {
    build_auth_header(
        &generate_nip98_token(
            keys,
            &Nip98Config {
                url: url.into(),
                method: method.into(),
                body: body.map(str::to_owned),
            },
        )
        .unwrap(),
    )
}

#[actix_web::test]
async fn raw_body_auth_preserves_json_content_type_and_size_limits() {
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .configure(configure_pay_routes),
    )
    .await;
    let url = "http://localhost/pay/.estimate";
    let body = r#"{"endpoint":"/api/inference/run"}"#;
    let header = auth(&Keys::generate(), url, "POST", Some(body));
    for (content_type, expected) in [
        ("text/plain", StatusCode::BAD_REQUEST),
        ("application/json", StatusCode::OK),
    ] {
        let req = test::TestRequest::post()
            .uri(url)
            .insert_header(("Content-Type", content_type))
            .insert_header(("Authorization", header.clone()))
            .set_payload(body)
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), expected);
    }
    let oversized = format!("{{\"endpoint\":\"{}\"}}", "x".repeat(2 * 1024 * 1024));
    let req = test::TestRequest::post()
        .uri(url)
        .insert_header(("Content-Type", "application/json"))
        .set_payload(oversized)
        .to_request();
    assert_eq!(
        test::call_service(&app, req).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[actix_web::test]
async fn balance_accepts_external_url_and_rejects_replay() {
    let dir = tempfile::tempdir().unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .app_data(web::Data::new(Arc::new(
                FsPaymentStore::new(dir.path()).unwrap(),
            )))
            .configure(configure_pay_routes),
    )
    .await;
    let header = auth(
        &Keys::generate(),
        "https://public.test:3001/pay/.balance",
        "GET",
        None,
    );
    for expected in [StatusCode::OK, StatusCode::UNAUTHORIZED] {
        let req = test::TestRequest::get()
            .uri("/pay/.balance")
            .insert_header(("Host", "internal:8080"))
            .insert_header(("X-Forwarded-Host", "public.test:3001"))
            .insert_header(("X-Forwarded-Proto", "https"))
            .insert_header(("Authorization", header.clone()))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), expected);
    }
}

#[actix_web::test]
async fn trading_requires_exact_body_and_single_use_without_burning_rejected_token() {
    let dir = tempfile::tempdir().unwrap();
    let exchange = Arc::new(FsExchangeStore::new(dir.path()).unwrap());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .app_data(web::Data::new(Arc::new(
                FsPaymentStore::new(dir.path()).unwrap(),
            )))
            .app_data(web::Data::new(exchange.clone()))
            .configure(configure_pay_routes),
    )
    .await;
    // Deliberate whitespace: hash the transmitted bytes, not reserialised JSON.
    let body =
        " {\"sell_currency\":\"sat\", \"sell_amount\":2,\"buy_currency\":\"token\",\"price\":3} ";
    let changed = body.replace("\"price\":3", "\"price\":300");
    let url = "http://localhost/pay/.sell";
    let header = auth(&Keys::generate(), url, "POST", Some(body));
    let unbound = auth(&Keys::generate(), url, "POST", None);
    for (token, payload, expected) in [
        (&unbound, body, StatusCode::UNAUTHORIZED),
        (&header, changed.as_str(), StatusCode::UNAUTHORIZED),
        (&header, body, StatusCode::CREATED),
        (&header, body, StatusCode::UNAUTHORIZED),
    ] {
        let req = test::TestRequest::post()
            .uri(url)
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Authorization", token.clone()))
            .set_payload(payload.to_owned())
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), expected);
    }
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("exchange.json")).unwrap()).unwrap();
    // One accepted order is the only exchange mutation in this sequence.
    assert_eq!(stored["order_book"]["orders"].as_array().unwrap().len(), 1);
}

#[actix_web::test]
async fn all_payment_posts_require_payload_binding() {
    let dir = tempfile::tempdir().unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .app_data(web::Data::new(Arc::new(
                FsPaymentStore::new(dir.path()).unwrap(),
            )))
            .app_data(web::Data::new(Arc::new(
                FsExchangeStore::new(dir.path()).unwrap(),
            )))
            .configure(configure_pay_routes),
    )
    .await;
    for (path, body) in [
        (".estimate", r#"{"endpoint":"/api/inference/run"}"#),
        (".swap", r#"{"order_id":"absent"}"#),
        (
            ".pool/swap?a=sat&b=token",
            r#"{"from_currency":"sat","amount":2}"#,
        ),
        (
            ".pool/liquidity",
            r#"{"currency_a":"sat","currency_b":"token","amount_a":2,"amount_b":3}"#,
        ),
    ] {
        let url = format!("http://localhost/pay/{path}");
        let req = test::TestRequest::post()
            .uri(&url)
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Authorization", auth(&Keys::generate(), &url, "POST", None)))
            .set_payload(body)
            .to_request();
        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
        let bound = auth(&Keys::generate(), &url, "POST", Some(body));
        let req = test::TestRequest::post()
            .uri(&url)
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Authorization", bound.clone()))
            .set_payload(body)
            .to_request();
        let status = test::call_service(&app, req).await.status();
        if path == ".estimate" {
            assert_eq!(status, StatusCode::OK);
        } else {
            assert!(
                matches!(
                    status,
                    StatusCode::BAD_REQUEST | StatusCode::PAYMENT_REQUIRED
                ),
                "{path}: {status}"
            );
        }
        let retry = test::TestRequest::post()
            .uri(&url)
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Authorization", bound))
            .set_payload(body)
            .to_request();
        assert_eq!(
            test::call_service(&app, retry).await.status(),
            StatusCode::UNAUTHORIZED,
            "accepted authentication is spent even if the operation failed: {path}"
        );
    }
    assert!(!dir.path().join("exchange.json").exists());
    assert!(!dir.path().join("ledger.json").exists());
}

#[actix_web::test]
async fn replay_cannot_charge_a_resource_twice() {
    use solid_pod_rs::payments::{pubkey_to_did, WebLedger};
    let dir = tempfile::tempdir().unwrap();
    let keys = Keys::generate();
    let did = pubkey_to_did(&keys.public_key().to_hex());
    let mut ledger = WebLedger::new("auth-regression");
    ledger
        .credit_by_outpoint(&did, "satoshi", &"07".repeat(32), 0, 10)
        .unwrap();
    std::fs::write(
        dir.path().join("ledger.json"),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    let store = Arc::new(FsPaymentStore::new(dir.path()).unwrap());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .app_data(web::Data::new(store.clone()))
            .configure(configure_pay_routes),
    )
    .await;
    let url = "http://localhost/pay/document";
    let header = auth(&keys, url, "GET", None);
    for expected in [StatusCode::OK, StatusCode::UNAUTHORIZED] {
        let req = test::TestRequest::get()
            .uri(url)
            .insert_header(("Authorization", header.clone()))
            .to_request();
        let response = test::call_service(&app, req).await;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            assert_eq!(response.headers().get("x-balance").unwrap(), "9");
            assert_eq!(response.headers().get("x-cost").unwrap(), "1");
        }
    }
    assert_eq!(store.get_balance(&did).await, 9);
}

#[actix_web::test]
async fn request_binding_failures_do_not_consume_a_valid_estimate_token() {
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(VcPayConfig {
                enabled: true,
                ..Default::default()
            }))
            .configure(configure_pay_routes),
    )
    .await;
    let url = "http://localhost/pay/.estimate?tier=one";
    let body = r#"{"endpoint":"/api/inference/run"}"#;
    let header = auth(&Keys::generate(), url, "POST", Some(body));
    for (request_url, expected) in [
        (
            "http://other.test/pay/.estimate?tier=one",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "http://localhost/pay/.estimate?tier=two",
            StatusCode::UNAUTHORIZED,
        ),
        (url, StatusCode::OK),
    ] {
        let req = test::TestRequest::post()
            .uri(request_url)
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Authorization", header.clone()))
            .set_payload(body)
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), expected);
    }
}
