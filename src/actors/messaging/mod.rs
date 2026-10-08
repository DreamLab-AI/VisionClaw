//! Actor Message Acknowledgment Protocol (H4)
//!
//! Acknowledgement-deadline tracking for critical actor messages:
//! - correlation ids on tracked sends, acked by the receiver
//! - one WARN per missed deadline; nothing is resent
//! - late acks recognised and counted
//! - per-kind metrics

pub mod message_ack;
pub mod message_id;
pub mod message_tracker;
pub mod metrics;

pub use message_ack::{AckStatus, MessageAck};
pub use message_id::MessageId;
pub use message_tracker::{MessageKind, MessageTracker, PendingMessage};
pub use metrics::MessageMetrics;
