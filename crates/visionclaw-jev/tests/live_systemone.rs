//! Synthetic-only integration probe; no corpus or user data is sent.
use std::sync::Arc;
use serde_json::json;
use visionclaw_jev::{client::HttpTransport, config::Item, hub::JevHub, wire::Question};

#[tokio::test]
#[ignore = "requires the local System One façade"]
async fn local_facade_answers_all_three_primitives() {
    let endpoint = std::env::var("SYSTEM_ONE_SMOKE_ENDPOINT")
        .unwrap_or_else(|_| "http://systemone:8097/v1/systemone".into());
    let client = JevHub::with_transport(move |key| match key {
        "VISIONCLAW_JEV" | "VISIONCLAW_JEV_LOCAL" => Some("1".into()),
        "VISIONCLAW_JEV_ENDPOINT" => Some(endpoint.clone()),
        "VISIONCLAW_JEV_MODEL" => Some("laya-typed-decisions".into()),
        "VISIONCLAW_JEV_TIMEOUT_MS" => Some("4000".into()),
        _ => None,
    }, Arc::new(HttpTransport::new().unwrap()));
    let answers = client.ask(Item::HealthTriage, "deployment-smoke/v1", json!("The switch is enabled."), vec![
        ("choice".into(), Question::Choice {
            instructions: "Which state is the switch in?".into(),
            criteria: [("on".into(), "The switch is enabled.".into()), ("off".into(), "The switch is disabled.".into())].into(),
        }),
        ("score".into(), Question::Score {
            instructions: "How well does the statement support that the switch is enabled?".into(),
            criteria: vec!["No support".into(), "Partial support".into(), "Explicit support".into()],
        }),
        ("noul".into(), Question::Noul { instructions: "Is the switch enabled?".into() }),
    ]).await.unwrap();
    answers.choice("choice", &["on", "off"]).unwrap();
    answers.score("score", 3).unwrap();
    answers.noul("noul").unwrap();
}
