//! Voice conversational grounding & repair — the V3 confidence gate (PRD-023
//! WP-10).
//!
//! The P1 governed voice loop (PTT → STT → intent → signed 31402 →
//! `/v1/voice-intent`) dispatches a spoken command straight to the selected
//! agent. V3 inserts a **confidence gate** in front of that dispatch: a
//! low-confidence or under-specified utterance is NOT dispatched; instead the
//! system speaks a targeted clarification that names *what it heard* and the
//! *ambiguous slot*, holds a pending clarification, and merges the operator's
//! next utterance (the repair turn) before it will dispatch.
//!
//! This module is the pure, self-contained gate + merge logic, unit-tested with
//! fixture transcripts. The per-session pending state that carries a
//! clarification across turns is persisted by
//! [`crate::services::voice_context_manager::VoiceContextManager`] — the
//! previously-dead `ConversationContext.pending_clarification` field
//! (`voice_commands.rs:88`, read at `voice_context_manager.rs:321`), now
//! populated (the WP-10 falsification trigger). The gate is wired into the
//! governed path in
//! `speech_socket_handler::SpeechSocket::process_governed_voice`.
//!
//! ## Two gate triggers (PRD-023 WP-10 AC1)
//!
//! `pending_clarification` is populated when **either**:
//!   1. STT confidence is below the configurable threshold
//!      (`VISIONCLAW_VOICE_CONFIDENCE_THRESHOLD`, default `0.55`); or
//!   2. the **intent gate** is ambiguous — a spawn/stop command whose required
//!      slot (agent type / agent id) cannot be filled. This mirrors the slot
//!      triggers of [`crate::actors::voice_commands::VoiceCommand`]'s parser
//!      (`extract_agent_type` / `extract_agent_id` error paths) so the gate and
//!      the dispatcher agree on what counts as under-specified.

/// The gate default when `VISIONCLAW_VOICE_CONFIDENCE_THRESHOLD` is unset or
/// unparseable. Below this normalised STT confidence, an utterance is held for a
/// clarification turn instead of dispatched.
pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.55;

/// Recognised agent types, mirroring
/// [`crate::actors::voice_commands::VoiceCommand`]'s `extract_agent_type`. Kept
/// in lockstep so the confidence gate and the intent parser classify the same
/// utterances as under-specified.
const KNOWN_AGENT_TYPES: &[&str] = &["researcher", "coder", "analyst", "coordinator", "optimizer"];

/// The token tag + separator used to serialise a [`PendingClarification`] into
/// the `Option<String>` `ConversationContext.pending_clarification` field. SOH
/// (`\u{1}`) never occurs in a spoken transcript, so it is a safe delimiter.
const TOKEN_TAG: &str = "V3";
const TOKEN_SEP: char = '\u{1}';

/// Which slot of the utterance is under-specified. Named so the spoken
/// clarification can call the slot out ("which agent type?").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbiguousSlot {
    /// STT reported the whole utterance below the confidence threshold.
    LowConfidence,
    /// A spawn/add command with no recognisable agent type.
    AgentType,
    /// A stop/remove command with no recognisable agent id.
    AgentId,
}

impl AmbiguousSlot {
    /// Stable machine name (used in the persisted token, the WS payload and the
    /// canary evidence line).
    pub fn slot_name(self) -> &'static str {
        match self {
            AmbiguousSlot::LowConfidence => "confidence",
            AmbiguousSlot::AgentType => "agent_type",
            AmbiguousSlot::AgentId => "agent_id",
        }
    }

    fn from_slot_name(name: &str) -> Option<Self> {
        match name {
            "confidence" => Some(AmbiguousSlot::LowConfidence),
            "agent_type" => Some(AmbiguousSlot::AgentType),
            "agent_id" => Some(AmbiguousSlot::AgentId),
            _ => None,
        }
    }
}

/// A clarification the gate is holding, waiting for the operator's repair turn.
/// Serialises to a compact token so it fits the existing
/// `ConversationContext.pending_clarification: Option<String>` field without a
/// schema change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingClarification {
    /// The utterance the gate declined to dispatch (the thing being repaired).
    pub original_transcript: String,
    /// Which slot was under-specified.
    pub slot: AmbiguousSlot,
}

impl PendingClarification {
    /// Encode for `ConversationContext.pending_clarification`.
    pub fn to_token(&self) -> String {
        format!(
            "{TOKEN_TAG}{TOKEN_SEP}{}{TOKEN_SEP}{}",
            self.slot.slot_name(),
            self.original_transcript
        )
    }

    /// Decode a token previously produced by [`Self::to_token`]. Returns `None`
    /// for anything that is not a V3 pending token (so a stray value in the
    /// field is treated as "no pending clarification", not a panic).
    pub fn from_token(token: &str) -> Option<Self> {
        let mut parts = token.splitn(3, TOKEN_SEP);
        if parts.next()? != TOKEN_TAG {
            return None;
        }
        let slot = AmbiguousSlot::from_slot_name(parts.next()?)?;
        let original_transcript = parts.next()?.to_string();
        Some(Self {
            original_transcript,
            slot,
        })
    }
}

/// The gate's verdict for one utterance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    /// Confidence and intent are adequate — dispatch this (possibly merged)
    /// transcript on the governed path.
    Dispatch { transcript: String },
    /// Under-specified — do NOT dispatch. Speak `prompt` (which names what was
    /// heard and the ambiguous slot) and hold `pending` for the repair turn.
    Clarify {
        prompt: String,
        pending: PendingClarification,
    },
}

/// The confidence gate. A plain threshold holder — cheap to copy, so `AppState`
/// keeps one and hands it to the voice path by value.
#[derive(Debug, Clone, Copy)]
pub struct ClarificationGate {
    threshold: f32,
}

impl ClarificationGate {
    /// The env var that overrides the default threshold.
    pub const ENV_THRESHOLD: &'static str = "VISIONCLAW_VOICE_CONFIDENCE_THRESHOLD";

    /// Build with an explicit threshold, clamped to `[0.0, 1.0]`.
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold: if threshold.is_finite() {
                threshold.clamp(0.0, 1.0)
            } else {
                DEFAULT_CONFIDENCE_THRESHOLD
            },
        }
    }

    /// Build from `VISIONCLAW_VOICE_CONFIDENCE_THRESHOLD`, falling back to
    /// [`DEFAULT_CONFIDENCE_THRESHOLD`] when unset or unparseable.
    pub fn from_env() -> Self {
        let threshold = std::env::var(Self::ENV_THRESHOLD)
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(DEFAULT_CONFIDENCE_THRESHOLD);
        Self { threshold }
    }

    /// The active threshold.
    pub fn threshold(self) -> f32 {
        self.threshold
    }

    /// First-turn evaluation: no clarification is pending.
    pub fn evaluate(self, transcript: &str, confidence: Option<f32>) -> GateOutcome {
        self.gate(transcript.trim().to_string(), confidence)
    }

    /// Repair-turn evaluation: merge the operator's `reply` into the held
    /// clarification, then re-gate the merged transcript. If the merge resolves
    /// the ambiguity (adequate confidence AND slot filled) it dispatches the
    /// merged transcript; otherwise it clarifies again.
    pub fn merge(
        self,
        pending: &PendingClarification,
        reply: &str,
        reply_confidence: Option<f32>,
    ) -> GateOutcome {
        let merged = merge_transcript(pending, reply);
        self.gate(merged, reply_confidence)
    }

    /// The shared gate: STT-confidence check first, then the intent-slot check.
    fn gate(self, transcript: String, confidence: Option<f32>) -> GateOutcome {
        // 1. STT confidence gate. `None` means the STT layer did not report a
        //    confidence — treat as adequate (do not block a command on missing
        //    telemetry); the intent gate below still runs.
        if let Some(c) = confidence {
            if c < self.threshold {
                let slot = AmbiguousSlot::LowConfidence;
                return GateOutcome::Clarify {
                    prompt: prompt_for(slot, &transcript),
                    pending: PendingClarification {
                        slot,
                        original_transcript: transcript,
                    },
                };
            }
        }
        // 2. Intent-slot gate.
        if let Some(slot) = detect_missing_slot(&transcript) {
            return GateOutcome::Clarify {
                prompt: prompt_for(slot, &transcript),
                pending: PendingClarification {
                    slot,
                    original_transcript: transcript,
                },
            };
        }
        GateOutcome::Dispatch { transcript }
    }
}

/// Hold `transcript` for a repair turn because the Jev option judged it
/// unclear (VC-35-S1-01). This can only make the gate *stricter*: it never
/// releases a held utterance or fills a missing slot.
pub fn hold_unclear(transcript: &str) -> GateOutcome {
    let transcript = transcript.trim().to_string();
    let slot = AmbiguousSlot::LowConfidence;
    GateOutcome::Clarify {
        prompt: prompt_for(slot, &transcript),
        pending: PendingClarification { slot, original_transcript: transcript },
    }
}

/// Merge a repair reply into the held clarification.
///
///   * `LowConfidence` — the whole utterance was mis-heard, so the repair is a
///     re-utterance that **replaces** the original.
///   * `AgentType` / `AgentId` — a slot was missing, so the repair **fills** it:
///     the reply is appended to the original, giving a merged transcript that
///     carries both the verb and the resolved slot.
fn merge_transcript(pending: &PendingClarification, reply: &str) -> String {
    let reply = reply.trim();
    match pending.slot {
        AmbiguousSlot::LowConfidence => reply.to_string(),
        AmbiguousSlot::AgentType | AmbiguousSlot::AgentId => {
            let orig = pending.original_transcript.trim();
            if orig.is_empty() {
                reply.to_string()
            } else if reply.is_empty() {
                orig.to_string()
            } else {
                format!("{orig} {reply}")
            }
        }
    }
}

/// The intent-slot gate. Returns the under-specified slot for a spawn/stop
/// command that names no agent, mirroring the parser's `extract_agent_type` /
/// `extract_agent_id` error paths. Returns `None` for a well-formed or
/// non-spawn/stop command (which then dispatches).
pub fn detect_missing_slot(transcript: &str) -> Option<AmbiguousSlot> {
    let lower = transcript.to_lowercase();
    let is_spawn = lower.contains("add agent") || lower.contains("spawn");
    let is_stop = lower.contains("stop agent") || lower.contains("remove agent");

    if is_spawn && !has_agent_reference(&lower) {
        return Some(AmbiguousSlot::AgentType);
    }
    if is_stop && !has_agent_reference(&lower) {
        return Some(AmbiguousSlot::AgentId);
    }
    None
}

/// True when the utterance names a concrete agent: a known type, or a non-empty
/// word after `"agent "` (the parser's free-form fallback).
fn has_agent_reference(lower: &str) -> bool {
    if KNOWN_AGENT_TYPES.iter().any(|t| lower.contains(t)) {
        return true;
    }
    if let Some(pos) = lower.find("agent ") {
        if let Some(word) = lower[pos + "agent ".len()..].split_whitespace().next() {
            return !word.is_empty();
        }
    }
    false
}

/// The spoken clarification. Every variant names *what was heard* (in quotes)
/// and *the ambiguous slot*, per the WP-10 acceptance criterion.
fn prompt_for(slot: AmbiguousSlot, heard: &str) -> String {
    let heard = heard.trim();
    match slot {
        AmbiguousSlot::LowConfidence => {
            format!(
                "I heard \"{heard}\", but I'm not confident I caught that. Could you say it again?"
            )
        }
        AmbiguousSlot::AgentType => {
            format!("I heard \"{heard}\", but not which agent to spawn. Which agent type — for example researcher or coder?")
        }
        AmbiguousSlot::AgentId => {
            format!("I heard \"{heard}\", but not which agent to stop. Which agent should I stop?")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- fixture transcripts -------------------------------------------------

    const CLEAR_COMMAND: &str = "spawn a researcher agent";
    const SPAWN_NO_TYPE: &str = "spawn a";
    const STOP_NO_ID: &str = "stop agent";
    const HIGH_CONF: f32 = 0.92;
    const LOW_CONF: f32 = 0.30;

    fn gate() -> ClarificationGate {
        ClarificationGate::new(DEFAULT_CONFIDENCE_THRESHOLD)
    }

    // ---- first-turn gate -----------------------------------------------------

    #[test]
    fn high_confidence_clear_command_dispatches() {
        let out = gate().evaluate(CLEAR_COMMAND, Some(HIGH_CONF));
        assert_eq!(
            out,
            GateOutcome::Dispatch {
                transcript: CLEAR_COMMAND.to_string()
            }
        );
    }

    #[test]
    fn missing_confidence_does_not_block_a_clear_command() {
        // A None confidence (STT reported nothing) must not hold a well-formed
        // command — the intent gate still runs, but this command is complete.
        let out = gate().evaluate(CLEAR_COMMAND, None);
        assert!(matches!(out, GateOutcome::Dispatch { .. }));
    }

    #[test]
    fn low_confidence_holds_and_names_what_it_heard() {
        let out = gate().evaluate(CLEAR_COMMAND, Some(LOW_CONF));
        match out {
            GateOutcome::Clarify { prompt, pending } => {
                assert_eq!(pending.slot, AmbiguousSlot::LowConfidence);
                assert_eq!(pending.original_transcript, CLEAR_COMMAND);
                // The prompt names what it heard AND the ambiguous slot.
                assert!(prompt.contains(CLEAR_COMMAND), "names heard text: {prompt}");
                assert!(prompt.contains("say it again"), "names the slot: {prompt}");
            }
            other => panic!("expected Clarify, got {other:?}"),
        }
    }

    #[test]
    fn missing_agent_type_is_an_intent_gate_clarification() {
        // High STT confidence, but the intent is under-specified: no agent type.
        let out = gate().evaluate(SPAWN_NO_TYPE, Some(HIGH_CONF));
        match out {
            GateOutcome::Clarify { prompt, pending } => {
                assert_eq!(pending.slot, AmbiguousSlot::AgentType);
                assert!(prompt.contains("which agent") || prompt.contains("Which agent"));
                assert!(prompt.contains(SPAWN_NO_TYPE), "names heard text: {prompt}");
            }
            other => panic!("expected Clarify(AgentType), got {other:?}"),
        }
    }

    #[test]
    fn missing_agent_id_on_stop_is_a_clarification() {
        let out = gate().evaluate(STOP_NO_ID, Some(HIGH_CONF));
        match out {
            GateOutcome::Clarify { pending, .. } => {
                assert_eq!(pending.slot, AmbiguousSlot::AgentId);
            }
            other => panic!("expected Clarify(AgentId), got {other:?}"),
        }
    }

    #[test]
    fn a_non_spawn_command_with_no_slot_dispatches() {
        // "what is the status" is not a spawn/stop, so the intent gate does not
        // hold it even though it names no agent.
        let out = gate().evaluate("what is the status", Some(HIGH_CONF));
        assert!(matches!(out, GateOutcome::Dispatch { .. }));
    }

    // ---- repair-turn merge ---------------------------------------------------

    #[test]
    fn low_confidence_repair_reutterance_merges_and_dispatches() {
        let g = gate();
        // Turn 1: low confidence → hold.
        let first = g.evaluate("spawn researcher", Some(LOW_CONF));
        let pending = match first {
            GateOutcome::Clarify { pending, .. } => pending,
            other => panic!("expected Clarify, got {other:?}"),
        };
        // Turn 2 (repair): a clear re-utterance replaces the mis-heard original.
        let repair = g.merge(&pending, "spawn a researcher agent", Some(HIGH_CONF));
        assert_eq!(
            repair,
            GateOutcome::Dispatch {
                transcript: "spawn a researcher agent".to_string()
            }
        );
    }

    #[test]
    fn missing_slot_repair_fills_the_slot_and_dispatches() {
        let g = gate();
        // Turn 1: spawn with no type → hold on the agent_type slot.
        let pending = match g.evaluate("spawn an agent", Some(HIGH_CONF)) {
            GateOutcome::Clarify { pending, .. } => pending,
            other => panic!("expected Clarify, got {other:?}"),
        };
        assert_eq!(pending.slot, AmbiguousSlot::AgentType);
        // Turn 2 (repair): "researcher" fills the slot; merged transcript carries
        // both the verb and the resolved type, and now dispatches.
        let repair = g.merge(&pending, "researcher", Some(HIGH_CONF));
        match repair {
            GateOutcome::Dispatch { transcript } => {
                assert!(transcript.contains("spawn"), "keeps the verb: {transcript}");
                assert!(
                    transcript.contains("researcher"),
                    "fills the slot: {transcript}"
                );
            }
            other => panic!("expected Dispatch after repair, got {other:?}"),
        }
    }

    #[test]
    fn repair_that_is_still_ambiguous_clarifies_again() {
        let g = gate();
        // Bare "spawn" holds on the agent_type slot with no "agent <word>" token
        // to fill, so a filler repair that names no known type stays ambiguous.
        let pending = match g.evaluate("spawn", Some(HIGH_CONF)) {
            GateOutcome::Clarify { pending, .. } => pending,
            other => panic!("expected Clarify, got {other:?}"),
        };
        assert_eq!(pending.slot, AmbiguousSlot::AgentType);
        // Merged "spawn please" still names no recognised agent → re-clarify.
        let repair = g.merge(&pending, "please", Some(HIGH_CONF));
        assert!(
            matches!(repair, GateOutcome::Clarify { .. }),
            "an unresolved repair re-clarifies"
        );
    }

    // ---- token round-trip (per-session persistence) --------------------------

    #[test]
    fn pending_token_round_trips() {
        for slot in [
            AmbiguousSlot::LowConfidence,
            AmbiguousSlot::AgentType,
            AmbiguousSlot::AgentId,
        ] {
            let p = PendingClarification {
                original_transcript: "spawn a researcher agent named x".to_string(),
                slot,
            };
            let decoded = PendingClarification::from_token(&p.to_token()).expect("round-trip");
            assert_eq!(decoded, p);
        }
    }

    #[test]
    fn from_token_rejects_foreign_values() {
        // A stray value in the pending_clarification field is "no pending", not a
        // crash or a mis-decoded slot.
        assert!(PendingClarification::from_token("some old free text").is_none());
        assert!(PendingClarification::from_token("").is_none());
        assert!(
            PendingClarification::from_token(&format!("V3{TOKEN_SEP}bogus{TOKEN_SEP}x")).is_none()
        );
    }

    // ---- configurable threshold ---------------------------------------------

    #[test]
    fn threshold_is_configurable() {
        // A permissive gate (threshold 0) dispatches even low-confidence input;
        // a strict gate (threshold 1) holds even high-confidence input.
        assert!(matches!(
            ClarificationGate::new(0.0).evaluate(CLEAR_COMMAND, Some(0.01)),
            GateOutcome::Dispatch { .. }
        ));
        assert!(matches!(
            ClarificationGate::new(1.0).evaluate(CLEAR_COMMAND, Some(0.99)),
            GateOutcome::Clarify { .. }
        ));
    }

    #[test]
    fn threshold_is_clamped() {
        assert_eq!(ClarificationGate::new(5.0).threshold(), 1.0);
        assert_eq!(ClarificationGate::new(-2.0).threshold(), 0.0);
        assert_eq!(
            ClarificationGate::new(f32::NAN).threshold(),
            DEFAULT_CONFIDENCE_THRESHOLD
        );
    }
}
