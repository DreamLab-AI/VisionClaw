//! Server proxy for the browser-facing Jev option (`/api/jev/*`).
//!
//! The browser never holds a vendor key and never chooses a destination or a
//! question: it posts plain command/palette data here, the server builds the
//! typed questions from fixed, versioned sets
//! ([`visionclaw_jev::ui`]) and resolves the endpoint per the contract
//! ([`visionclaw_jev::config::resolve`]). Every route is authenticated, bounded,
//! and fail-open: any failure answers `200 {"used": false, "reason": …}` so the
//! client keeps its pre-change behaviour. With the flag off the routes answer
//! `used:false` without building a request.

use actix_web::{web, HttpResponse, Responder};
use serde_json::{json, Value};

use visionclaw_jev::config::Item;
use visionclaw_jev::hub::JevHub;
use visionclaw_jev::ui::{
    rank_palette, understand_command, CommandRequest, PaletteRequest, MAX_PALETTE_CANDIDATES,
};
use visionclaw_jev::Fallback;

fn not_used(reason: &Fallback) -> Value {
    json!({ "used": false, "reason": reason.tag() })
}

/// Which items are on and whether an endpoint resolves. Carries no endpoint
/// URL, model name or key.
pub fn status_body(client: &JevHub) -> Value {
    let mut enabled = serde_json::Map::new();
    for i in Item::ALL {
        enabled.insert(i.name().to_string(), Value::Bool(client.enabled(i)));
    }
    json!({
        "enabled": enabled,
        "available": Item::ALL.iter().any(|&i| client.available(i)),
    })
}

/// Core of `POST /api/jev/command` (separated from actix for testing).
pub async fn command_body(client: &JevHub, req: &CommandRequest) -> Value {
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
pub async fn palette_body(client: &JevHub, req: &PaletteRequest) -> Value {
    match rank_palette(client, req).await {
        Ok(r) => {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            v["used"] = Value::Bool(true);
            v
        }
        Err(r) => not_used(&r),
    }
}

/// Core of the review-only edge API. Never writes graph data or proposals.
pub async fn edge_body(client: &JevHub, req: &visionclaw_jev::edge::EdgeReviewRequest) -> Value {
    let evidence = match req.evidence() {
        Ok(evidence) => evidence,
        Err(reason) => return not_used(&reason),
    };
    match visionclaw_jev::edge::suggest_edge(client, &evidence).await {
        Ok(proposal) => json!({"used": true, "advisory": true, "authority": "none", "proposal": proposal}),
        Err(reason) => not_used(&reason),
    }
}

/// `POST /api/jev/edge`: authenticated source-evidence review, no mutation.
pub async fn edge(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevHub>,
    req: web::Json<visionclaw_jev::edge::EdgeReviewRequest>,
) -> impl Responder {
    HttpResponse::Ok().json(edge_body(&client, &req).await)
}

/// `GET /api/jev/status`
pub async fn status(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevHub>,
) -> impl Responder {
    HttpResponse::Ok().json(status_body(&client))
}

/// `POST /api/jev/command`
pub async fn command(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevHub>,
    req: web::Json<CommandRequest>,
) -> impl Responder {
    HttpResponse::Ok().json(command_body(&client, &req).await)
}

/// `POST /api/jev/palette`
pub async fn palette(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    client: web::Data<JevHub>,
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
            .route("/palette", web::post().to(palette))
            .route("/edge", web::post().to(edge)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_jev::client::testing::{MockTransport, Script};
    use visionclaw_jev::hub::testing::enabled_hub;

    #[actix_web::test]
    async fn edge_route_requires_authentication() {
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(JevHub::disabled()))
                .configure(configure_jev_routes),
        ).await;
        let req = actix_web::test::TestRequest::post().uri("/jev/edge")
            .set_json(json!({"source_label":"Ada", "target_label":"Acme", "evidence":"Ada works at Acme."}))
            .to_request();
        assert_eq!(actix_web::test::call_service(&app, req).await.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn edge_review_is_advisory_and_validates_before_transport() {
        let t = MockTransport::new(Script::Reply(200, json!({"answers":{
            "predicate":{"choice":"mv:worksAt","confidence":0.99},
            "asserted":{"noul":0.99}}}).to_string()));
        let c = enabled_hub(&[], t.clone());
        let mut req = visionclaw_jev::edge::EdgeReviewRequest {
            source_label: "Ada".into(), target_label: "Acme".into(),
            source_class: Some("mv:Person".into()), target_class: Some("mv:Company".into()),
            evidence: "Ada works at Acme.".into(),
        };
        let result = edge_body(&c, &req).await;
        assert_eq!(result["used"], true);
        assert_eq!(result["authority"], "none");
        assert_eq!(result["proposal"]["predicate"], "mv:worksAt");
        req.evidence = "x".repeat(1201);
        assert_eq!(edge_body(&c, &req).await["reason"], "malformed");
        assert_eq!(t.calls(), 1);
    }

    fn creq() -> CommandRequest {
        CommandRequest { command: "show help".into(), candidates: vec![] }
    }

    #[tokio::test]
    async fn status_reveals_no_secrets_and_defaults_off() {
        let c = JevHub::disabled();
        let s = status_body(&c);
        assert!(s["enabled"].as_object().unwrap().values().all(|v| v == false));
        assert_eq!(s["available"], false);
        let on = enabled_hub(&[], MockTransport::new(Script::Reply(200, "{}".into())));
        let s = status_body(&on).to_string();
        assert!(!s.contains("127.0.0.1") && !s.contains("jev-test"));
    }

    #[tokio::test]
    async fn flag_off_route_is_used_false_and_sends_nothing() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let c = JevHub::with_transport(|_| None, t.clone());
        let v = command_body(&c, &creq()).await;
        assert_eq!(v, json!({"used": false, "reason": "flag-off"}));
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn command_route_accept_and_error_shapes() {
        let ok = json!({"model":"m","answers":{"intent":{"type":"choice","choice":"help","confidence":0.9,"probabilities":{"help":1.0}},
            "action_class":{"type":"choice","choice":"clarify","confidence":0.9,"probabilities":{}}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string();
        let c = enabled_hub(&[], MockTransport::new(Script::Reply(200, ok)));
        let v = command_body(&c, &creq()).await;
        assert_eq!(v["used"], true);
        assert_eq!(v["intent"]["id"], "help");
        let c = enabled_hub(&[], MockTransport::new(Script::Reply(500, "".into())));
        assert_eq!(command_body(&c, &creq()).await, json!({"used": false, "reason": "http-500"}));
    }
}
