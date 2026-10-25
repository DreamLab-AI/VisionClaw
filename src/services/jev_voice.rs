//! Jev option for voice intent disambiguation (VC-35-S1-01).
//!
//! Sits between the existing [`ClarificationGate`] and the governed dispatch
//! (signed kind-31402 → `/v1/voice-intent`). Jev can do exactly two things,
//! and both only act when they clear the outcome-changing threshold
//! (`VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE`; unset ⇒ accept nothing):
//!
//! 1. **Repair-turn contradiction** — a Noul asks whether the operator's reply
//!    abandons the pending request (a new, unrelated command). If so the reply
//!    is evaluated fresh instead of being appended to a stale held transcript.
//! 2. **Tighten** — a Choice over the known intents (plus `unclear`). A
//!    confident `unclear` holds an utterance the gate would have dispatched.
//!
//! Neither path can release a held utterance, fill a missing mandatory slot,
//! change the target agent, or sign anything. ASR, identity binding, the
//! `ClarificationGate`, the signed dispatch and human authority are unchanged.
//! The transcript is sent; the selected agent's DID is **not** (the question
//! does not need it).
//!
//! [`ClarificationGate`]: crate::services::voice_clarification::ClarificationGate

use serde_json::json;

use visionclaw_jev::config::Item;
use visionclaw_jev::hub::JevHub;
use visionclaw_jev::wire::Question;
use visionclaw_jev::Fallback;
use crate::services::voice_clarification::{hold_unclear, GateOutcome, PendingClarification};

/// Question-set version (logged on fallback).
pub const VOICE_QUESTION_SET: &str = "vc-voice-intent-v1";
/// Choice key for "no known intent is clearly stated".
pub const UNCLEAR: &str = "unclear";
const MAX_TRANSCRIPT_CHARS: usize = 400;

/// Intents the voice parser understands
/// (`actors::voice_commands::VoiceCommand`).
pub const INTENTS: [&str; 7] =
    ["spawn_agent", "agent_status", "list_agents", "stop_agent", "add_node", "help", UNCLEAR];

fn clip(s: &str) -> String {
    s.chars().take(MAX_TRANSCRIPT_CHARS).collect()
}

fn intent_question() -> Question {
    Question::Choice {
        instructions: "Which known voice command does the speech-recognised `transcript` clearly state? Choose `unclear` when no listed command is clearly stated, when the sentence negates a command ('don't stop the agent'), or when it is a question or comment rather than an instruction. Recognition errors are common: judge what a speaker most plausibly meant only when one listed command is clearly the best reading.".into(),
        criteria: std::collections::BTreeMap::from([
            ("spawn_agent".into(), "Create or add a new agent (it may or may not name the type).".into()),
            ("agent_status".into(), "Ask for the status of the system or an agent.".into()),
            ("list_agents".into(), "List or show the agents.".into()),
            ("stop_agent".into(), "Stop or remove an existing agent.".into()),
            ("add_node".into(), "Add a node to the graph.".into()),
            ("help".into(), "Ask for help.".into()),
            (UNCLEAR.into(), "No listed command is clearly stated, or the text is negated, a comment, or noise.".into()),
        ]),
    }
}

fn contradiction_question() -> Question {
    Question::Noul {
        instructions: "The `reply` abandons the `pending_request` and starts a different, unrelated request, rather than answering the clarifying question that was asked about the pending request.".into(),
    }
}

/// Repair turn: does the operator's reply abandon the pending request?
/// `false` for every fallback, which keeps the existing merge.
pub async fn reply_abandons_pending(client: &JevHub, pending: &PendingClarification, reply: &str) -> bool {
    if !client.enabled(Item::VoiceIntent) {
        return false;
    }
    let state = json!({
        "pending_request": clip(&pending.original_transcript),
        "missing": pending.slot.slot_name(),
        "reply": clip(reply),
    });
    let qs = vec![("abandons".to_string(), contradiction_question())];
    let Ok(answers) = client.ask(Item::VoiceIntent, VOICE_QUESTION_SET, state, qs).await else {
        return false;
    };
    match answers.noul("abandons") {
        Ok(p) => match client.accept_outcome(p) {
            Ok(()) => true,
            Err(r) => {
                r.log(Item::VoiceIntent, VOICE_QUESTION_SET);
                false
            }
        },
        Err(r) => {
            r.log(Item::VoiceIntent, VOICE_QUESTION_SET);
            false
        }
    }
}

/// Possibly hold a dispatch. Any outcome other than `Dispatch` is returned
/// unchanged (Jev never releases a held utterance); every fallback returns the
/// gate's outcome unchanged.
pub async fn tighten(client: &JevHub, outcome: GateOutcome) -> GateOutcome {
    let GateOutcome::Dispatch { transcript } = &outcome else {
        return outcome;
    };
    if !client.enabled(Item::VoiceIntent) {
        return outcome;
    }
    match classify(client, transcript).await {
        Ok(true) => hold_unclear(transcript),
        _ => outcome,
    }
}

/// `Ok(true)` when Jev confidently judges the transcript `unclear`.
async fn classify(client: &JevHub, transcript: &str) -> Result<bool, Fallback> {
    let state = json!({ "transcript": clip(transcript) });
    let qs = vec![("intent".to_string(), intent_question())];
    let answers = client.ask(Item::VoiceIntent, VOICE_QUESTION_SET, state, qs).await?;
    let c = answers.choice("intent", &INTENTS)?;
    if c.choice != UNCLEAR {
        return Ok(false);
    }
    client.accept_outcome(c.confidence)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_jev::client::testing::{MockTransport, Script};
    use visionclaw_jev::client::TransportError;
    use visionclaw_jev::hub::testing::enabled_hub;
    use crate::services::voice_clarification::{AmbiguousSlot, ClarificationGate, DEFAULT_CONFIDENCE_THRESHOLD};

    fn intent_body(c: &str, conf: f64) -> String {
        json!({"model":"m","answers":{"intent":{"type":"choice","choice":c,"confidence":conf,"probabilities":{}}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
    fn noul_body(p: f64) -> String {
        json!({"model":"m","answers":{"abandons":{"type":"noul","noul":p}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
    fn client(script: Script, auto: Option<&str>) -> (JevHub, std::sync::Arc<MockTransport>) {
        let t = MockTransport::new(script);
        let extra: Vec<(&str, &str)> = auto.map(|a| vec![("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", a)]).unwrap_or_default();
        (enabled_hub(&extra, t.clone()), t)
    }
    fn gate() -> ClarificationGate {
        ClarificationGate::new(DEFAULT_CONFIDENCE_THRESHOLD)
    }
    fn dispatch() -> GateOutcome {
        gate().evaluate("spawn a researcher agent", Some(0.9))
    }
    fn pending() -> PendingClarification {
        PendingClarification { slot: AmbiguousSlot::AgentType, original_transcript: "spawn a".into() }
    }

    #[tokio::test]
    async fn wire_shape() {
        let (c, t) = client(Script::Reply(200, intent_body("spawn_agent", 0.9)), Some("0.9"));
        let _ = tighten(&c, dispatch()).await;
        let b = t.last_body().unwrap();
        assert!(b["questions"]["intent"]["criteria"].is_object());
        assert!(b["questions"]["intent"]["criteria"].get(UNCLEAR).is_some());
        assert_eq!(b["state"]["transcript"], "spawn a researcher agent");
        assert!(!b.to_string().contains("did:nostr"), "agent DID is never sent");
        let (c, t) = client(Script::Reply(200, noul_body(0.9)), Some("0.9"));
        let _ = reply_abandons_pending(&c, &pending(), "stop agent coder").await;
        let b = t.last_body().unwrap();
        assert_eq!(b["questions"]["abandons"]["type"], "noul");
        assert_eq!(b["state"]["pending_request"], "spawn a");
    }

    #[tokio::test]
    async fn accept_paths() {
        let (c, _) = client(Script::Reply(200, intent_body(UNCLEAR, 0.97)), Some("0.9"));
        match tighten(&c, dispatch()).await {
            GateOutcome::Clarify { pending, .. } => assert_eq!(pending.slot, AmbiguousSlot::LowConfidence),
            other => panic!("expected hold, got {other:?}"),
        }
        let (c, _) = client(Script::Reply(200, noul_body(0.97)), Some("0.9"));
        assert!(reply_abandons_pending(&c, &pending(), "stop agent coder").await);
    }

    #[tokio::test]
    async fn fallbacks_equal_pre_change_outcome() {
        let before = dispatch();
        let held = gate().evaluate("spawn a", Some(0.9)); // missing slot: gate already holds
        let t = MockTransport::new(Script::Reply(200, intent_body(UNCLEAR, 1.0)));
        let off = JevHub::with_transport(|_| None, t.clone());
        assert_eq!(tighten(&off, dispatch()).await, before);
        assert!(!reply_abandons_pending(&off, &pending(), "x").await);
        assert_eq!(t.calls(), 0);
        // default threshold accepts nothing
        let (c, _) = client(Script::Reply(200, intent_body(UNCLEAR, 1.0)), None);
        assert_eq!(tighten(&c, dispatch()).await, before);
        let (c, _) = client(Script::Reply(200, noul_body(1.0)), None);
        assert!(!reply_abandons_pending(&c, &pending(), "x").await);
        for s in [
            Script::Fail(TransportError::Timeout),
            Script::Reply(500, "".into()),
            Script::Reply(200, "junk".into()),
            Script::Reply(200, intent_body("fire_missiles", 0.99)),
            Script::Reply(200, intent_body(UNCLEAR, 0.2)),
            Script::Reply(200, intent_body("spawn_agent", 0.99)),
        ] {
            let (c, _) = client(s, Some("0.9"));
            assert_eq!(tighten(&c, dispatch()).await, before);
        }
        for s in [
            Script::Fail(TransportError::Timeout),
            Script::Reply(429, "".into()),
            Script::Reply(200, "junk".into()),
            Script::Reply(200, noul_body(0.3)),
        ] {
            let (c, _) = client(s, Some("0.9"));
            assert!(!reply_abandons_pending(&c, &pending(), "x").await);
        }
        // A held utterance is never released and a missing slot never filled,
        // whatever Jev says.
        let (c, t) = client(Script::Reply(200, intent_body("spawn_agent", 1.0)), Some("0.1"));
        assert_eq!(tighten(&c, held.clone()).await, held);
        assert_eq!(t.calls(), 0, "held outcomes are not even sent");
    }
}
