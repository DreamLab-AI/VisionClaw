//! Acknowledgement-deadline tracking for critical actor messages (H4).
//!
//! The sender records a correlation id when it `do_send`s a tracked message;
//! the receiver answers with a [`MessageAck`]. A message with no ack by its
//! deadline is reported once (WARN) and dropped from the pending set.
//!
//! Nothing is ever resent. Actix delivers in-process `do_send` reliably, so a
//! missed deadline means a slow handler or a missing ack path, not a lost
//! message, and re-sending would repeat work (InitializeGPU re-initialises
//! the GPU). An ack that arrives after its deadline is counted as late.

use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::{AckStatus, MessageAck, MessageId, MessageMetrics};

/// How long a timed-out id is remembered so a late ack can be recognised.
const LATE_ACK_MEMORY: Duration = Duration::from_secs(600);

/// The message types a sender tracks. Each has an ack path in its receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// GPU graph data update (acked by ForceComputeActor).
    UpdateGPUGraphData,

    /// Upload constraints to GPU (acked by ForceComputeActor).
    UploadConstraintsToGPU,

    /// Initialise GPU compute (acked by ForceComputeActor).
    InitializeGPU,
}

impl MessageKind {
    /// How long the receiver has to acknowledge this kind.
    pub fn default_timeout(&self) -> Duration {
        match self {
            MessageKind::UpdateGPUGraphData => Duration::from_secs(2),
            MessageKind::UploadConstraintsToGPU => Duration::from_secs(3),
            MessageKind::InitializeGPU => Duration::from_secs(10),
        }
    }

    /// Message kind name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            MessageKind::UpdateGPUGraphData => "UpdateGPUGraphData",
            MessageKind::UploadConstraintsToGPU => "UploadConstraintsToGPU",
            MessageKind::InitializeGPU => "InitializeGPU",
        }
    }
}

/// A tracked message awaiting acknowledgement.
pub struct PendingMessage {
    pub id: MessageId,
    pub kind: MessageKind,
    pub sent_at: Instant,
    pub timeout: Duration,
}

impl PendingMessage {
    /// Whether the deadline has passed.
    pub fn is_timed_out(&self) -> bool {
        self.sent_at.elapsed() > self.timeout
    }

    /// Age of the message.
    pub fn age(&self) -> Duration {
        self.sent_at.elapsed()
    }
}

/// Tracks outstanding messages against their acknowledgement deadlines.
/// # Example
/// ```rust,ignore
/// use visionclaw_server::actors::messaging::{MessageId, MessageKind, MessageTracker};
/// let tracker = MessageTracker::new();
/// let msg_id = MessageId::new();
/// tracker.track_default(msg_id, MessageKind::UpdateGPUGraphData).await;
/// ```
#[derive(Clone)]
pub struct MessageTracker {
    /// Messages awaiting acknowledgement.
    pending: Arc<RwLock<HashMap<MessageId, PendingMessage>>>,

    /// Messages that missed their deadline, kept for [`LATE_ACK_MEMORY`].
    timed_out: Arc<RwLock<HashMap<MessageId, PendingMessage>>>,

    /// Metrics for monitoring.
    metrics: Arc<MessageMetrics>,

    /// Flag to stop the background checker.
    shutdown: Arc<RwLock<bool>>,
}

impl MessageTracker {
    /// Create a new message tracker.
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            timed_out: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(MessageMetrics::new()),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Track a sent message that must be acknowledged within `timeout`.
    pub async fn track(&self, id: MessageId, kind: MessageKind, timeout: Duration) {
        debug!("Tracking message {} ({})", id, kind.name());
        self.pending.write().await.insert(
            id,
            PendingMessage {
                id,
                kind,
                sent_at: Instant::now(),
                timeout,
            },
        );
        self.metrics.record_sent(kind);
    }

    /// Track a message with its kind's default deadline.
    pub async fn track_default(&self, id: MessageId, kind: MessageKind) {
        self.track(id, kind, kind.default_timeout()).await;
    }

    /// Record an acknowledgement.
    pub async fn acknowledge(&self, ack: MessageAck) {
        let msg_id = ack.correlation_id;

        if let Some(msg) = self.pending.write().await.remove(&msg_id) {
            let latency = msg.age();
            match ack.status {
                AckStatus::Success => {
                    debug!(
                        "Message {} ({}) acknowledged ({}ms)",
                        msg_id,
                        msg.kind.name(),
                        latency.as_millis()
                    );
                    self.metrics.record_success(msg.kind, latency);
                }
                AckStatus::PartialSuccess { ref reason } => {
                    warn!(
                        "Message {} ({}) partially succeeded: {}",
                        msg_id,
                        msg.kind.name(),
                        reason
                    );
                    self.metrics.record_success(msg.kind, latency);
                }
                AckStatus::Failed { ref error } => {
                    error!("Message {} ({}) failed: {}", msg_id, msg.kind.name(), error);
                    self.metrics.record_failure(msg.kind);
                }
            }
            return;
        }

        if let Some(msg) = self.timed_out.write().await.remove(&msg_id) {
            info!(
                "Message {} ({}) acknowledged late: {}ms after sending, deadline {}ms",
                msg_id,
                msg.kind.name(),
                msg.age().as_millis(),
                msg.timeout.as_millis()
            );
            self.metrics.record_late_ack(msg.kind);
            return;
        }

        debug!("Acknowledgement for untracked message {}", msg_id);
    }

    /// Report every message past its deadline once and stop tracking it.
    /// The background checker calls this; tests call it directly.
    pub async fn check_timeouts(&self) {
        Self::check_timeouts_inner(&self.pending, &self.timed_out, &self.metrics).await;
    }

    async fn check_timeouts_inner(
        pending: &RwLock<HashMap<MessageId, PendingMessage>>,
        timed_out: &RwLock<HashMap<MessageId, PendingMessage>>,
        metrics: &MessageMetrics,
    ) {
        let expired: Vec<PendingMessage> = {
            let mut pending = pending.write().await;
            let ids: Vec<MessageId> = pending
                .iter()
                .filter(|(_, m)| m.is_timed_out())
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id))
                .collect()
        };

        let mut timed_out = timed_out.write().await;
        timed_out.retain(|_, m| m.age() < LATE_ACK_MEMORY);
        for msg in expired {
            warn!(
                "Message {} ({}) not acknowledged within {}ms; nothing is resent (a late ack will be logged)",
                msg.id,
                msg.kind.name(),
                msg.timeout.as_millis()
            );
            metrics.record_timeout(msg.kind);
            timed_out.insert(msg.id, msg);
        }
    }

    /// Start the background deadline checker (every 500 ms).
    pub fn start_timeout_checker(&self) {
        let pending = Arc::clone(&self.pending);
        let timed_out = Arc::clone(&self.timed_out);
        let metrics = Arc::clone(&self.metrics);
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                if *shutdown.read().await {
                    info!("MessageTracker timeout checker shutting down");
                    break;
                }
                Self::check_timeouts_inner(&pending, &timed_out, &metrics).await;
            }
        });
    }

    /// Whether a message is still awaiting acknowledgement.
    pub async fn is_pending(&self, id: MessageId) -> bool {
        self.pending.read().await.contains_key(&id)
    }

    /// Number of messages awaiting acknowledgement.
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Metrics.
    pub fn metrics(&self) -> Arc<MessageMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Stop the background checker.
    pub async fn shutdown(&self) {
        *self.shutdown.write().await = true;
    }
}

impl Default for MessageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[tokio::test]
    async fn test_track_and_acknowledge() {
        let tracker = MessageTracker::new();
        let msg_id = MessageId::new();

        tracker
            .track_default(msg_id, MessageKind::UpdateGPUGraphData)
            .await;

        assert!(tracker.is_pending(msg_id).await);
        assert_eq!(tracker.pending_count().await, 1);

        tracker.acknowledge(MessageAck::success(msg_id)).await;

        assert!(!tracker.is_pending(msg_id).await);
        assert_eq!(tracker.pending_count().await, 0);
        assert_eq!(tracker.metrics().total_acked.load(Relaxed), 1);
    }

    /// Nothing is ever resent, so a missed deadline is reported once and the
    /// message stops being pending: no "scheduling retry" that does nothing.
    #[tokio::test]
    async fn a_timed_out_message_is_reported_once_and_never_rescheduled() {
        let tracker = MessageTracker::new();
        let msg_id = MessageId::new();
        tracker
            .track(
                msg_id,
                MessageKind::InitializeGPU,
                Duration::from_millis(10),
            )
            .await;

        tokio::time::sleep(Duration::from_millis(30)).await;
        tracker.check_timeouts().await;
        tracker.check_timeouts().await;

        assert!(!tracker.is_pending(msg_id).await);
        assert_eq!(tracker.metrics().total_timed_out.load(Relaxed), 1);
    }

    /// An ack that arrives after the deadline is recognised as late, not
    /// reported as an ack for an unknown message.
    #[tokio::test]
    async fn an_ack_after_the_deadline_is_counted_as_late() {
        let tracker = MessageTracker::new();
        let msg_id = MessageId::new();
        tracker
            .track(
                msg_id,
                MessageKind::UpdateGPUGraphData,
                Duration::from_millis(10),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        tracker.check_timeouts().await;

        tracker.acknowledge(MessageAck::success(msg_id)).await;

        let metrics = tracker.metrics();
        assert_eq!(metrics.total_timed_out.load(Relaxed), 1);
        assert_eq!(metrics.total_late_acked.load(Relaxed), 1);
        assert_eq!(metrics.total_acked.load(Relaxed), 0);
    }

    #[tokio::test]
    async fn an_ack_for_an_untracked_message_changes_nothing() {
        let tracker = MessageTracker::new();
        tracker
            .acknowledge(MessageAck::success(MessageId::new()))
            .await;
        let metrics = tracker.metrics();
        assert_eq!(metrics.total_acked.load(Relaxed), 0);
        assert_eq!(metrics.total_late_acked.load(Relaxed), 0);
    }

    #[test]
    fn test_message_kind_defaults() {
        let kind = MessageKind::InitializeGPU;
        assert_eq!(kind.default_timeout(), Duration::from_secs(10));
        assert_eq!(kind.name(), "InitializeGPU");
    }
}
