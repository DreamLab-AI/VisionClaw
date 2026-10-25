//! Server proxy for the browser-facing Jev option (`/api/jev/*`).
//!
//! The browser never holds a vendor key and never chooses a destination or a
//! question: it posts plain command/palette data here, the server builds the
//! typed questions from fixed, versioned sets
//! ([`crate::services::jev_ui`]) and resolves the endpoint per the contract
//! ([`crate::services::system_one`]). Every route is authenticated, bounded,
//! and fail-open: any failure answers `200 {"used": false, "reason": …}` so the
//! client keeps its pre-change behaviour. With the flag off the routes answer
//! `used:false` without building a request.

use actix_web::{web, HttpResponse, Responder};
use serde_json::{json, Value};

use crate::services::jev_ui::{
    rank_palette, understand_command, CommandRequest, PaletteRequest, MAX_PALETTE_CANDIDATES,
};
use crate::services::system_one::{FallbackReason, JevClient, JevItem};

fn not_used(reason: &FallbackReason) -> Value {
    json!({ "used": false, "reason": reason.as_token() })
}

/// Which items are on and whether an endpoint resolves. Carries no endpoint
/// URL, model name or key.
pub fn status_body(client: &JevClient) -> Value {
    let mut enabled = serde_json::Map::new();
    for i in JevItem::ALL {
        enabled.insert(i.slug().to_string(), Value::Bool(client.enabled(i)));
    }
    json!({
        "enabled": enabled,
        "available": client.settings().endpoint(false).is_ok(),
    })
}

/// Core of `POST /api/jev/command` (separated from actix for testing).
pub async fn command_body(client: &JevClient, req: &CommandRequest) -> Value {
    match understand_command(client, req).await {
        Ok(u) => {
            let mut v = serde_json::to_value(u).unwrap_or_else(|_| json!({}));
            v["used"] = Value::Bool(true);
            v
        }
        Err(r) => not_used(&r),
    }
}

/// Core of `POST /api/jev/palette`.
pub async fn palette_body(client: &JevClient, req: &PaletteRequest) -> Value {
    match rank_palette(client, req).await {
        Ok(r) => {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            v["used"] = Value::Bool(true);
            v
        }
        Err(r) => not_used(&r),
    }
}

/// `GET /api/jev/status`
pub async fn status(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevClient>,
) -> impl Responder {
    HttpResponse::Ok().json(status_body(&client))
}

/// `POST /api/jev/command`
pub async fn command(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevClient>,
    req: web::Json<CommandRequest>,
) -> impl Responder {
    HttpResponse::Ok().json(command_body(&client, &req).await)
}

/// `POST /api/jev/palette`
pub async fn palette(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevClient>,
    req: web::Json<PaletteRequest>,
) -> impl Responder {
    debug_assert!(MAX_PALETTE_CANDIDATES > 0);
    HttpResponse::Ok().json(palette_body(&client, &req).await)
}

/// Register `/jev/*` under the `/api` scope.
pub fn configure_jev_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/jev")
            .route("/status", web::get().to(status))
            .route("/command", web::post().to(command))
            .route("/palette", web::post().to(palette)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::system_one::testing::*;
    use crate::services::system_one::JevSettings;

    fn creq() -> CommandRequest {
        CommandRequest { command: "show help".into(), candidates: vec![] }
    }

    #[tokio::test]
    async fn status_reveals_no_secrets_and_defaults_off() {
        let c = JevClient::disabled();
        let s = status_body(&c);
        assert!(s["enabled"].as_object().unwrap().values().all(|v| v == false));
        assert_eq!(s["available"], false);
        let on = JevClient::with_transport(enabled_settings(&[]), MockTransport::new(Script::Reply(200, "{}".into())));
        let s = status_body(&on).to_string();
        assert!(!s.contains("127.0.0.1") && !s.contains("jev-test"));
    }

    #[tokio::test]
    async fn flag_off_route_is_used_false_and_sends_nothing() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let c = JevClient::with_transport(JevSettings::disabled(), t.clone());
        let v = command_body(&c, &creq()).await;
        assert_eq!(v, json!({"used": false, "reason": "flag-off"}));
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn command_route_accept_and_error_shapes() {
        let ok = json!({"model":"m","answers":{"intent":{"type":"choice","choice":"help","confidence":0.9,"probabilities":{"help":1.0}},
            "action_class":{"type":"choice","choice":"clarify","confidence":0.9,"probabilities":{}}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string();
        let c = JevClient::with_transport(enabled_settings(&[]), MockTransport::new(Script::Reply(200, ok)));
        let v = command_body(&c, &creq()).await;
        assert_eq!(v["used"], true);
        assert_eq!(v["intent"]["id"], "help");
        let c = JevClient::with_transport(enabled_settings(&[]), MockTransport::new(Script::Reply(500, "".into())));
        assert_eq!(command_body(&c, &creq()).await, json!({"used": false, "reason": "http-500"}));
    }
}
