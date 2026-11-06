//! VisionClaw actor subsystem
//!
//! Phase A3 extraction — actors whose dependency graph resolves entirely to
//! `visionclaw-domain` (models, errors, utils) with no webxr-internal imports.
//!
//! ## Included (Phase A3)
//! - `supervisor`           — generic actix supervision tree (domain errors only)
//! - `voice_commands`       — voice intent / swarm command types (no external deps)
//!
//! ## Excluded (still in webxr — blocked by webxr-internal deps)
//! - Actors using `crate::actors::messages::*` (messages module has webxr-internal deps:
//!   `config::AppFullSettings`, `gpu::visual_analytics`, `utils::socket_flow_messages`)
//! - Actors using `crate::protocol::v3_frame`, `crate::handlers`, `crate::services`,
//!   `crate::telemetry`, `crate::utils::socket_flow_messages`, `crate::application`
//! - GPU actors using `shared::SharedGPUContext` (depends on webxr `gpu::memory_manager`)

pub mod messages;
pub mod supervisor;
pub mod voice_commands;

// Re-export key types
pub use supervisor::{
    ActorFactory, SupervisedActorInfo, SupervisedActorTrait, SupervisionStrategy, SupervisorActor,
};
pub use voice_commands::{SwarmIntent, SwarmVoiceResponse, VoiceCommand, VoicePreamble};
