//! Voice adapters (ADR-2143 §8): nemotron ASR, pocket-tts and the Loom fallback.
//!
//! All three connect only through the [`crate::egress::EgressAllowlist`].
//! The `/ws/voice` route that drives them lives in the binary, because it
//! needs the floor arbiter and the command executor.

pub mod asr;
pub mod loom;
pub mod tts;
