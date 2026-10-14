//! `/api/agents/demo`: start, stop and read the shared agents demo (ADR-2142).
//!
//! One server-side director produces the demo; both clients show it, and either client's button
//! calls this same endpoint. State is the director's: a client that did not start the demo
//! still sees it running by reading `GET`.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::actors::agent_demo_director::{GetAgentDemoStatus, StartAgentDemo, StopAgentDemo};
use crate::app_state::AppState;
use crate::settings::auth_extractor::AuthenticatedUser;

#[derive(Debug, Deserialize)]
pub struct SetDemo {
    pub running: bool,
}

/// `GET /api/agents/demo`: whether the demo is running, and the reserved agent ids.
pub async fn get_demo(state: web::Data<AppState>) -> impl Responder {
    match state.agent_demo_addr.send(GetAgentDemoStatus).await {
        Ok(status) => HttpResponse::Ok().json(status),
        Err(e) => HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// `POST /api/agents/demo` with `{"running": true|false}`: start or stop it for everyone.
pub async fn set_demo(
    _auth: AuthenticatedUser,
    state: web::Data<AppState>,
    body: web::Json<SetDemo>,
) -> impl Responder {
    if body.running {
        match state.agent_demo_addr.send(StartAgentDemo).await {
            Ok(Ok(status)) => HttpResponse::Ok().json(status),
            Ok(Err(e)) => HttpResponse::Conflict().json(serde_json::json!({ "error": e })),
            Err(e) => HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": e.to_string() })),
        }
    } else {
        match state.agent_demo_addr.send(StopAgentDemo).await {
            Ok(status) => HttpResponse::Ok().json(status),
            Err(e) => HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": e.to_string() })),
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/agents/demo")
            .route(web::get().to(get_demo))
            .route(web::post().to(set_demo)),
    );
}
