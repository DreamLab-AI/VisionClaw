//! Dev-only layout experiment control (ADR-2204 D5, pre-registered ontology-mode test).
//!
//! `POST /api/dev/layout-experiment` sets or clears the [`LayoutExperiment`] on the running
//! layout and `GET` reports it with the withheld hierarchy pairs, so a layout can be scored on
//! pairs it never saw. The whole module exists only under
//! `#[cfg(any(debug_assertions, feature = "dev-auth"))]`: a release build has no such route.
//! Every call needs the power-user identity, like other privileged writes. Setting an
//! experiment re-uploads edge weights only; positions are never touched.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use visionclaw_domain::weighting::LayoutExperiment;

use crate::actors::messages::{GetLayoutExperiment, SetLayoutExperiment};
use crate::settings::auth_extractor::AuthenticatedUser;
use crate::AppState;

/// Body of `POST /api/dev/layout-experiment`: the experiment to run, or `null` to clear it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetExperimentBody {
    /// The experiment; `null` or absent clears.
    #[serde(default)]
    pub experiment: Option<LayoutExperiment>,
}

/// Reject a body whose experiment is malformed, before it reaches the actor.
pub fn check_body(body: &SetExperimentBody) -> Result<(), &'static str> {
    match body.experiment {
        Some(x) if !x.is_valid() => Err("holdoutFraction must be a finite number in 0..=1"),
        _ => Ok(()),
    }
}

fn denied() -> HttpResponse {
    HttpResponse::Forbidden().json(json!({ "error": "power-user identity required" }))
}

/// `POST /api/dev/layout-experiment`
pub async fn set_layout_experiment(
    auth: AuthenticatedUser,
    state: web::Data<AppState>,
    body: web::Json<SetExperimentBody>,
) -> impl Responder {
    if !auth.is_power_user {
        return denied();
    }
    if let Err(why) = check_body(&body) {
        return HttpResponse::BadRequest().json(json!({ "error": why }));
    }
    let Some(addr) = state.get_gpu_compute_addr().await else {
        return HttpResponse::ServiceUnavailable()
            .json(json!({ "error": "force compute actor not available" }));
    };
    match addr
        .send(SetLayoutExperiment {
            experiment: body.experiment,
        })
        .await
    {
        Ok(Ok(())) => report(&state).await,
        Ok(Err(e)) => HttpResponse::InternalServerError().json(json!({ "error": e })),
        Err(e) => HttpResponse::ServiceUnavailable().json(json!({ "error": e.to_string() })),
    }
}

/// `GET /api/dev/layout-experiment`
pub async fn get_layout_experiment(
    auth: AuthenticatedUser,
    state: web::Data<AppState>,
) -> impl Responder {
    if !auth.is_power_user {
        return denied();
    }
    report(&state).await
}

async fn report(state: &web::Data<AppState>) -> HttpResponse {
    let Some(addr) = state.get_gpu_compute_addr().await else {
        return HttpResponse::ServiceUnavailable()
            .json(json!({ "error": "force compute actor not available" }));
    };
    match addr.send(GetLayoutExperiment).await {
        Ok(Ok(r)) => HttpResponse::Ok().json(r),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(json!({ "error": e })),
        Err(e) => HttpResponse::ServiceUnavailable().json(json!({ "error": e.to_string() })),
    }
}

/// Register the dev routes.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/dev/layout-experiment")
            .route(web::get().to(get_layout_experiment))
            .route(web::post().to(set_layout_experiment)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> Result<SetExperimentBody, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn a_well_formed_experiment_parses_in_camel_case_and_null_arm_defaults_off() {
        let b = body(r#"{"experiment":{"holdoutSeed":7,"holdoutFraction":0.1}}"#).unwrap();
        let x = b.experiment.unwrap();
        assert_eq!(
            (x.holdout_seed, x.holdout_fraction, x.null_arm),
            (7, 0.1, false)
        );
        assert!(check_body(&b).is_ok());
    }

    #[test]
    fn null_and_absent_clear_the_experiment() {
        assert!(body(r#"{"experiment":null}"#).unwrap().experiment.is_none());
        assert!(body("{}").unwrap().experiment.is_none());
    }

    #[test]
    fn a_fraction_outside_zero_to_one_is_rejected() {
        for f in ["-0.1", "1.5"] {
            let b = body(&format!(
                r#"{{"experiment":{{"holdoutSeed":1,"holdoutFraction":{f}}}}}"#
            ))
            .unwrap();
            assert!(check_body(&b).is_err(), "{f}");
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(body(r#"{"experiment":{"holdoutSeed":1,"holdoutFraction":0.1,"x":1}}"#).is_err());
    }
}
