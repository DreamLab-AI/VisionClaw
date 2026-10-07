//! Agent-event ingest (ADR-059, VisionClaw side).
//!
//! The canonical agent-action wire envelope, mirrored from the agentbox
//! schema source (`management-api/utils/agent-event-publisher.js`). Phase 1
//! lands the schema; Phase 2 adds the `/wss/agent-events` transport that
//! consumes it. agentbox ADR-014 is the producer half of this contract.
//!
//! Phase 2 (this increment): [`ingest`] is the authenticated `/wss/agent-events`
//! handler that parses + validates inbound `notifications/agent_action` and
//! publishes each envelope to the process-global [`hub`]. The GPU beam render
//! actor (`AgentBeamActor`) subscribes to the hub and is shipped; the
//! attractive "gluon" transient edge is a separate, deferred sub-feature (see
//! `src/actors/agent_beam_actor.rs:327`). The `:9500` state-poll path
//! (`bots_client.rs`) remains live and load-bearing, not deprecated — cutting
//! it over to this transport is an unbuilt, scoped follow-on (ADR-2084).

pub mod hub;
pub mod ingest;
pub mod provenance;
pub mod schema;

pub use ingest::agent_events_ws;
pub use provenance::{IngestProvenance, ProvenanceStatus};
pub use schema::{AgentActionEnvelope, AgentActionNotification, AgentActionParams};
