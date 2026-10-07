//! Broker case-queue WebSocket events (REC-2 / D3, ADR-130 Decision 2).
//!
//! Two JSON text frames ride the existing multiplexed graph socket — the same
//! [`BroadcastMessage`](crate::actors::messages::BroadcastMessage) idiom the
//! enrichment-decision audit frame already uses — so a control-centre case
//! queue (P1) can subscribe without a second transport:
//!
//!   * `broker:new_case`      — a case entered the queue (channel `inbox`).
//!   * `broker:case_decided`  — a queued case reached a decision
//!     (channel `case:{id}`).
//!
//! The envelope shape (`{type, channel, payload}`) is carried forward verbatim
//! from the superseded `crashbug` `BrokerActor` broadcast so a future consumer
//! keys off a stable contract; the transport that produced it there (an
//! actor + Neo4j) is the part ADR-130 Decision 2 dropped, not the wire shape.
//!
//! Emission is fire-and-forget: a dropped broadcast never blocks a decision.

use actix::Addr;
use serde_json::{json, Value};

use crate::actors::messages::BroadcastMessage;
use crate::actors::ClientCoordinatorActor;

/// Build the `broker:new_case` envelope (channel `inbox`). Pure so the wire
/// shape is unit-testable without an actor.
pub fn new_case_envelope(case_id: &str, title: &str, category: &str) -> String {
    let envelope = json!({
        "type": "broker:new_case",
        "channel": "inbox",
        "payload": {
            "caseId": case_id,
            "title": title,
            "category": category,
        },
    });
    envelope.to_string()
}

/// Build the `broker:case_decided` envelope (channel `case:{case_id}`).
/// `action` is a kernel [`DecisionOutcome::action_str`] value; `share_plan` is
/// the optional [`ShareTransitionPlan`] JSON for contributor-mesh-share cases.
///
/// [`DecisionOutcome::action_str`]: crate::domain::broker::DecisionOutcome::action_str
/// [`ShareTransitionPlan`]: crate::domain::broker::ShareTransitionPlan
pub fn case_decided_envelope(
    case_id: &str,
    decision_id: &str,
    action: &str,
    share_plan: Option<Value>,
) -> String {
    let envelope = json!({
        "type": "broker:case_decided",
        "channel": format!("case:{case_id}"),
        "payload": {
            "caseId": case_id,
            "decisionId": decision_id,
            "action": action,
            "sharePlan": share_plan,
        },
    });
    envelope.to_string()
}

/// Broadcast `broker:new_case` to every connected client.
pub fn broadcast_new_case(
    coordinator: &Addr<ClientCoordinatorActor>,
    case_id: &str,
    title: &str,
    category: &str,
) {
    coordinator.do_send(BroadcastMessage {
        message: new_case_envelope(case_id, title, category),
    });
}

/// Broadcast `broker:case_decided` to every connected client.
pub fn broadcast_case_decided(
    coordinator: &Addr<ClientCoordinatorActor>,
    case_id: &str,
    decision_id: &str,
    action: &str,
    share_plan: Option<Value>,
) {
    coordinator.do_send(BroadcastMessage {
        message: case_decided_envelope(case_id, decision_id, action, share_plan),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_case_envelope_carries_type_channel_payload() {
        let raw = new_case_envelope("case-7", "Elevate concept", "knowledge_enrichment");
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["type"], "broker:new_case");
        assert_eq!(v["channel"], "inbox");
        assert_eq!(v["payload"]["caseId"], "case-7");
        assert_eq!(v["payload"]["title"], "Elevate concept");
        assert_eq!(v["payload"]["category"], "knowledge_enrichment");
    }

    #[test]
    fn case_decided_envelope_channels_on_case_id() {
        let raw = case_decided_envelope("case-7", "dec-1", "approve", None);
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["type"], "broker:case_decided");
        assert_eq!(v["channel"], "case:case-7");
        assert_eq!(v["payload"]["decisionId"], "dec-1");
        assert_eq!(v["payload"]["action"], "approve");
        assert!(v["payload"]["sharePlan"].is_null());
    }

    #[test]
    fn case_decided_envelope_embeds_share_plan() {
        let plan = json!({"from": "private", "to": "team", "approvedBy": "bob"});
        let raw = case_decided_envelope("case-9", "dec-2", "approve", Some(plan));
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["payload"]["sharePlan"]["from"], "private");
        assert_eq!(v["payload"]["sharePlan"]["to"], "team");
    }
}
