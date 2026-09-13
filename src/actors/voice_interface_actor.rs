//! VoiceInterfaceActor — spoken interface configuration (ADR-110 extension).
//!
//! Routes spoken view/graph configuration requests from the local Whisper STT
//! stream to the **same settings assistant** that the Control Center command
//! box drives (`POST /api/bots/settings-command` → agentbox LLM → settings
//! REST API), and confirms over local PocketTts TTS. One assistant, two mouths:
//! typed in the UX control centre, or spoken inside the immersive session.
//!
//! Intent detection is deliberately conservative — a configuration verb AND an
//! interface noun must both be present — so ordinary conversation about the
//! graph is never hijacked (elevation harvesting owns that signal instead).

use std::sync::Arc;

use actix::prelude::*;
use log::{info, warn};

use crate::actors::task_orchestrator_actor::TaskOrchestratorActor;
use crate::actors::CreateTask;
use crate::handlers::bots_handler::settings_assistant_task;
use crate::services::speech_service::SpeechService;
use crate::types::speech::SpeechOptions;

#[derive(Message)]
#[rtype(result = "()")]
struct VoiceLine(String);

/// Verbs that signal the user wants to CHANGE something.
const CONFIG_VERBS: &[&str] = &[
    "set ",
    "increase ",
    "decrease ",
    "reduce ",
    "raise ",
    "lower ",
    "show ",
    "hide ",
    "turn on",
    "turn off",
    "enable ",
    "disable ",
    "switch to",
    "change ",
    "dim ",
    "brighten ",
    "bigger",
    "smaller",
    "speed up",
    "slow down",
    "reset ",
];

/// Interface nouns the verbs must act on. Keeps "increase the budget" or
/// "show me the door" out of the settings assistant.
const INTERFACE_NOUNS: &[&str] = &[
    "node",
    "edge",
    "label",
    "graph",
    "physics",
    "spring",
    "repulsion",
    "gravity",
    "damping",
    "layout",
    "bloom",
    "glow",
    "colour",
    "color",
    "background",
    "camera",
    "ontology",
    "knowledge",
    "agent",
    "hologram",
    "light",
    "ambient",
    "opacity",
    "size",
    "separation",
    "cluster",
    "hull",
    "view",
    "interface",
    "panel",
    "setting",
];

/// Detect a spoken interface-configuration command. Returns the command text
/// to hand to the settings assistant.
pub fn parse_interface_intent(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let has_verb = CONFIG_VERBS.iter().any(|v| lower.contains(v));
    let has_noun = INTERFACE_NOUNS.iter().any(|n| lower.contains(n));
    if has_verb && has_noun {
        Some(text.trim().to_string())
    } else {
        None
    }
}

pub struct VoiceInterfaceActor {
    orchestrator: Addr<TaskOrchestratorActor>,
    speech: Arc<SpeechService>,
}

impl VoiceInterfaceActor {
    /// Starts only when the local speech stack is up — voice in, voice out.
    pub fn new(
        orchestrator: Addr<TaskOrchestratorActor>,
        speech: Option<Arc<SpeechService>>,
    ) -> Option<Self> {
        Some(Self {
            orchestrator,
            speech: speech?,
        })
    }

    fn speak(&self, text: String) {
        let speech = self.speech.clone();
        tokio::spawn(async move {
            if let Err(e) = speech.text_to_speech(text, SpeechOptions::default()).await {
                warn!("[VoiceInterface] TTS confirmation failed: {e}");
            }
        });
    }
}

impl Actor for VoiceInterfaceActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("[VoiceInterface] voice → settings assistant bridge active");
        let addr = ctx.address();
        let speech = self.speech.clone();
        tokio::spawn(async move {
            let mut rx = speech.subscribe_to_transcriptions();
            loop {
                match rx.recv().await {
                    Ok(line) => addr.do_send(VoiceLine(line)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[VoiceInterface] transcription stream lagged ({n} skipped)");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}

impl Handler<VoiceLine> for VoiceInterfaceActor {
    type Result = ();

    fn handle(&mut self, VoiceLine(line): VoiceLine, ctx: &mut Self::Context) {
        let Some(command) = parse_interface_intent(&line) else {
            return;
        };
        info!("[VoiceInterface] spoken configuration request: \"{command}\"");
        let settings_base = std::env::var("VISIONCLAW_INTERNAL_URL")
            .unwrap_or_else(|_| "http://visionclaw_container:4000".to_string());
        let provider = std::env::var("PRIMARY_PROVIDER").unwrap_or_else(|_| "gemini".to_string());
        let task = settings_assistant_task(&command, "", &settings_base);
        let orchestrator = self.orchestrator.clone();

        self.speak("Adjusting the interface.".to_string());
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                orchestrator
                    .send(CreateTask {
                        agent: "researcher".to_string(),
                        task,
                        provider,
                        // Voice settings-assistant dispatch is role-only; no
                        // claude-flow agent id exists at creation time.
                        claude_flow_agent_id: None,
                    })
                    .await
            })
            .map(|result, act, _ctx| match result {
                Ok(Ok(resp)) => {
                    info!("[VoiceInterface] settings assistant task {}", resp.task_id);
                }
                Ok(Err(e)) => {
                    warn!("[VoiceInterface] settings assistant dispatch failed: {e}");
                    act.speak("Sorry — the settings assistant is unavailable.".into());
                }
                Err(e) => warn!("[VoiceInterface] orchestrator mailbox error: {e}"),
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_verb_plus_interface_noun_triggers() {
        assert_eq!(
            parse_interface_intent("increase the spring strength a bit"),
            Some("increase the spring strength a bit".to_string())
        );
        assert!(parse_interface_intent("hide the ontology nodes").is_some());
        assert!(parse_interface_intent("turn off bloom").is_some());
        assert!(parse_interface_intent("make the labels bigger").is_some());
    }

    #[test]
    fn ordinary_speech_does_not_trigger() {
        // noun without verb
        assert!(parse_interface_intent("the knowledge graph looks great").is_none());
        // verb without interface noun
        assert!(parse_interface_intent("increase the budget for next year").is_none());
        // neither
        assert!(parse_interface_intent("let's talk about finality mechanisms").is_none());
    }
}
