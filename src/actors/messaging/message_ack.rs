//! Message acknowledgment types

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use super::MessageId;

/// Status of message acknowledgment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AckStatus {
    /// Message processed successfully
    Success,

    /// Message partially processed (some operations failed)
    PartialSuccess { reason: String },

    /// Message processing failed
    Failed { error: String },
}

impl AckStatus {
    /// Check if the status represents success
    pub fn is_success(&self) -> bool {
        matches!(self, AckStatus::Success)
    }

    /// Check if the status represents failure
    pub fn is_failure(&self) -> bool {
        matches!(self, AckStatus::Failed { .. })
    }

    /// Get error message if status is Failed
    pub fn error(&self) -> Option<&str> {
        match self {
            AckStatus::Failed { error } => Some(error),
            _ => None,
        }
    }
}

/// Generic acknowledgment message sent by actors after processing
/// # Example
/// ```
/// use visionclaw_server::actors::messaging::{MessageId, MessageAck, AckStatus};
/// use std::time::Instant;
/// use std::collections::HashMap;
/// let ack = MessageAck {
///     correlation_id: MessageId::new(),
///     status: AckStatus::Success,
///     timestamp: Instant::now(),
///     metadata: HashMap::new(),
/// };
/// ```
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct MessageAck {
    /// ID of the message being acknowledged
    pub correlation_id: MessageId,

    /// Status of message processing
    pub status: AckStatus,

    /// When the acknowledgment was created
    pub timestamp: Instant,

    /// Additional metadata (e.g., processing time, node count)
    pub metadata: HashMap<String, String>,
}

impl MessageAck {
    /// Create a new success acknowledgment
    pub fn success(correlation_id: MessageId) -> Self {
        Self {
            correlation_id,
            status: AckStatus::Success,
            timestamp: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new failure acknowledgment
    pub fn failure(correlation_id: MessageId, error: String) -> Self {
        Self {
            correlation_id,
            status: AckStatus::Failed { error },
            timestamp: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new partial success acknowledgment
    pub fn partial_success(correlation_id: MessageId, reason: String) -> Self {
        Self {
            correlation_id,
            status: AckStatus::PartialSuccess { reason },
            timestamp: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the acknowledgment
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ack_status_checks() {
        let success = AckStatus::Success;
        assert!(success.is_success());
        assert!(!success.is_failure());

        let failed = AckStatus::Failed {
            error: "test error".to_string(),
        };
        assert!(!failed.is_success());
        assert!(failed.is_failure());
        assert_eq!(failed.error(), Some("test error"));
    }

    #[test]
    fn test_message_ack_builders() {
        let msg_id = MessageId::new();

        let success_ack = MessageAck::success(msg_id);
        assert!(success_ack.status.is_success());

        let failure_ack = MessageAck::failure(msg_id, "error".to_string());
        assert!(failure_ack.status.is_failure());

        let partial_ack = MessageAck::partial_success(msg_id, "reason".to_string());
        assert!(!partial_ack.status.is_success());
        assert!(!partial_ack.status.is_failure());
    }

    #[test]
    fn test_message_ack_metadata() {
        let msg_id = MessageId::new();
        let ack = MessageAck::success(msg_id)
            .with_metadata("node_count", "1000")
            .with_metadata("processing_time_ms", "42");

        assert_eq!(ack.metadata.get("node_count"), Some(&"1000".to_string()));
        assert_eq!(
            ack.metadata.get("processing_time_ms"),
            Some(&"42".to_string())
        );
    }
}
