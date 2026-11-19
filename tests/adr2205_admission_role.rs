//! ADR-2205 admission through the RBAC role store: a principal holding an
//! explicit role row may bind a session key; a stranger may not. Own test
//! binary because the role store is process-global.

use actix_web::{http::StatusCode, test, web, App};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use nostr_sdk::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use visionclaw_server::handlers::nostr_handler;
use visionclaw_server::models::rbac::UserRole;
use visionclaw_server::services::nostr_service::NostrService;
use visionclaw_server::services::role_store::{set_global_role_store, RoleStore};

const BIND_PATH: &str = "/api/auth/nostr/session-key";
const HOST: &str = "http://localhost:8080";

fn nip98(keys: &Keys, body: &str) -> String {
    let tags = vec![
        Tag::custom(
            TagKind::Custom("u".into()),
            vec![format!("{HOST}{BIND_PATH}")],
        ),
        Tag::custom(TagKind::Custom("method".into()), vec!["POST".to_string()]),
        Tag::custom(
            TagKind::Custom("nonce".into()),
            vec![uuid::Uuid::new_v4().to_string()],
        ),
        Tag::custom(
            TagKind::Custom("payload".into()),
            vec![hex::encode(Sha256::digest(body.as_bytes()))],
        ),
    ];
    let event = EventBuilder::new(Kind::Custom(27235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    format!("Nostr {}", B64.encode(event.as_json()))
}

fn body(session: &Keys, principal: &Keys) -> String {
    let proof = EventBuilder::new(Kind::Custom(27236), "")
        .tags(vec![
            Tag::custom(
                TagKind::Custom("principal".into()),
                vec![principal.public_key().to_hex()],
            ),
            Tag::custom(
                TagKind::Custom("u".into()),
                vec![format!("{HOST}{BIND_PATH}")],
            ),
            Tag::custom(TagKind::Custom("expires".into()), vec!["600".to_string()]),
            Tag::custom(
                TagKind::Custom("nonce".into()),
                vec![uuid::Uuid::new_v4().to_string()],
            ),
        ])
        .sign_with_keys(session)
        .unwrap()
        .as_json();
    json!({ "sessionPubkey": session.public_key().to_hex(), "expiresIn": 600, "proof": proof })
        .to_string()
}

#[actix_web::test]
async fn a_role_holder_may_bind_and_a_stranger_may_not() {
    std::env::remove_var("POWER_USER_PUBKEYS");
    let conn = Arc::new(tokio_rusqlite::Connection::open_in_memory().await.unwrap());
    let store = Arc::new(RoleStore::new(conn).await.unwrap());
    let member = Keys::generate();
    store
        .set(&member.public_key().to_hex(), UserRole::Editor, None)
        .await
        .unwrap();
    let _ = set_global_role_store(store);

    let svc = web::Data::new(NostrService::new());
    let app = test::init_service(
        App::new()
            .app_data(svc.clone())
            .service(web::scope("/api").configure(nostr_handler::config)),
    )
    .await;

    for (principal, expected) in [
        (&member, StatusCode::OK),
        (&Keys::generate(), StatusCode::FORBIDDEN),
    ] {
        let session = Keys::generate();
        let b = body(&session, principal);
        let req = test::TestRequest::post()
            .uri(BIND_PATH)
            .insert_header(("Host", "localhost:8080"))
            .insert_header(("Authorization", nip98(principal, &b)))
            .insert_header(("Content-Type", "application/json"))
            .set_payload(b)
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), expected);
        if expected == StatusCode::FORBIDDEN {
            let v: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
            assert_eq!(v["code"], "session_key_not_admitted");
        }
    }
    assert_eq!(svc.session_key_count(), 1);
}
