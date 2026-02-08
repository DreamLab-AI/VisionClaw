//! Message tracking with timeout and retry logic

use log::{debug, error, info, warn};
use std::any::Any;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::sync::mpsc;

use super::{MessageId, MessageAck, AckStatus, MessageMetrics};

/// Types of critical messages that need tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// GPU graph data update
    UpdateGPUGraphData,

    /// Upload constraints to GPU
    UploadConstraintsToGPU,

    /// Compute physics forces
    ComputeForces,

    /// Initialize GPU context
    InitializeGPU,

    /// Update node positions
    UpdateNodePositions,

    /// Set shared GPU context
    SetSharedGPUContext,
}

impl MessageKind {
    /// Get default timeout for this message kind
    pub fn default_timeout(&self) -> Duration {
        match self {
            MessageKind::UpdateGPUGraphData => Duration::from_secs(2),
            MessageKind::UploadConstraintsToGPU => Duration::from_secs(3),
            MessageKind::ComputeForces => Duration::from_secs(5),
            MessageKind::InitializeGPU => Duration::from_secs(10),
            MessageKind::UpdateNodePositions => Duration::from_secs(1),
            MessageKind::SetSharedGPUContext => Duration::from_secs(10),
        }
    }

    /// Get default max retries for this message kind
    pub fn default_max_retries(&self) -> u32 {
        match self {
            MessageKind::UpdateGPUGraphData => 5, // Critical for consistency
            MessageKind::UploadConstraintsToGPU => 3,
            MessageKind::ComputeForces => 3,
            MessageKind::InitializeGPU => 5, // Critical for startup
            MessageKind::UpdateNodePositions => 2, // High frequency
            MessageKind::SetSharedGPUContext => 5, // Critical
        }
    }

    /// Get message kind name for logging
    pub fn name(&self) -> &'static str {
        match self {
            MessageKind::UpdateGPUGraphData => "UpdateGPUGraphData",
            MessageKind::UploadConstraintsToGPU => "UploadConstraintsToGPU",
            MessageKind::ComputeForces => "ComputeForces",
            MessageKind::InitializeGPU => "InitializeGPU",
            MessageKind::UpdateNodePositions => "UpdateNodePositions",
            MessageKind::SetSharedGPUContext => "SetSharedGPUContext",
        }
    }
}

/// Pending message awaiting acknowledgment
pub struct PendingMessage {
    pub id: MessageId,
    pub kind: MessageKind,
    pub sent_at: Instant,
    pub timeout: Duration,
    pub retry_count: u32,
    pub max_retries: u32,
}

impl PendingMessage {
    /// Check if message has timed out
    pub fn is_timed_out(&self) -> bool {
        self.sent_at.elapsed() > self.timeout
    }

    /// Check if message can be retried
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Get age of the message
    pub fn age(&self) -> Duration {
        self.sent_at.elapsed()
    }
}

/// Request to retry a failed message
#[derive(Debug)]
pub struct RetryRequest {
    pub message_id: MessageId,
    pub kind: MessageKind,
    pub attempt: u32,
}

/// Tracks outstanding messages with timeouts and retry logic
/// # Example
/// ```rust
/// use webxr::actors::messaging::{MessageId, MessageKind, MessageTracker};
/// use std::time::Duration;
/// let tracker = MessageTracker::new();
/// let msg_id = MessageId::new();
/// tracker.track(
///     msg_id,
///     MessageKind::UpdateGPUGraphData,
///     Duration::from_secs(5),
///     3,
/// ).await;
/// ```
#[derive(Clone)]
pub struct MessageTracker {
    /// Pending messages awaiting acknowledgment
    pending: Arc<RwLock<HashMap<MessageId, PendingMessage>>>,

    /// Channel for retry requests
    retry_tx: mpsc::UnboundedSender<RetryRequest>,
    retry_rx: Arc<RwLock<mpsc::UnboundedReceiver<RetryRequest>>>,

    /// Metrics for monitoring
    metrics: Arc<MessageMetrics>,

    /// Flag to stop background tasks
    shutdown: Arc<RwLock<bool>>,
}

impl MessageTracker {
    /// Create a new message tracker
    pub fn new() -> Self {
        let (retry_tx, retry_rx) = mpsc::unbounded_channel();

        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            retry_tx,
            retry_rx: Arc::new(RwLock::new(retry_rx)),
            metrics: Arc::new(MessageMetrics::new()),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Track a new message
    /// # Arguments
    /// * `id` - Unique message identifier
    /// * `kind` - Type of message
    /// * `timeout` - How long to wait for acknowledgment
    /// * `max_retries` - Maximum number of retry attempts
    pub async fn track(
        &self,
        id: MessageId,
        kind: MessageKind,
        timeout: Duration,
        max_retries: u32,
    ) {
        let message = PendingMessage {
            id,
            kind,
            sent_at: Instant::now(),
            timeout,
            retry_count: 0,
            max_retries,
        };

        debug!("Tracking message {} ({})", id, kind.name());

        self.pending.write().await.insert(id, message);
        self.metrics.record_sent(kind);
    }

    /// Track a message with default timeout and retries
    pub async fn track_default(&self, id: MessageId, kind: MessageKind) {
        self.track(
            id,
            kind,
            kind.default_timeout(),
            kind.default_max_retries(),
        )
        .await;
    }

    /// Acknowledge a message (removes from pending)
    pub async fn acknowledge(&self, ack: MessageAck) {
        let msg_id = ack.correlation_id;

        let mut pending = self.pending.write().await;

        if let Some(msg) = pending.remove(&msg_id) {
            let latency = msg.sent_at.elapsed();

            match ack.status {
                AckStatus::Success => {
                    debug!(
                        "Message {} ({}) acknowledged successfully ({}ms)",
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
                    error!(
                        "Message {} ({}) failed: {}",
                        msg_id,
                        msg.kind.name(),
                        error
                    );
                    self.metrics.record_failure(msg.kind);
                }
                AckStatus::Retrying { attempt } => {
                    debug!(
                        "Message {} ({}) retrying (attempt {})",
                        msg_id,
                        msg.kind.name(),
                        attempt
                    );
                    // Put back in pending with updated retry count
                    pending.insert(
                        msg_id,
                        PendingMessage {
                            retry_count: attempt,
                            sent_at: Instant::now(), // Reset timeout
                            ..msg
                        },
                    );
                    self.metrics.record_retry(msg.kind);
                }
            }
        } else {
            warn!(
                "Received acknowledgment for unknown message: {}",
                msg_id
            );
        }
    }

    /// Check for timed out messages and trigger retries
    pub async fn check_timeouts(&self) {
        let mut pending = self.pending.write().await;
        let mut timed_out = Vec::new();

        // Find timed out messages
        for (id, msg) in pending.iter() {
            if msg.is_timed_out() {
                timed_out.push((*id, msg.kind, msg.retry_count));
            }
        }

        // Handle timeouts
        for (id, kind, retry_count) in timed_out {
            if let Some(mut msg) = pending.remove(&id) {
                if msg.can_retry() {
                    warn!(
                        "Message {} ({}) timed out after {}ms, scheduling retry {}/{}",
                        id,
                        kind.name(),
                        msg.age().as_millis(),
                        retry_count + 1,
                        msg.max_retries
                    );

                    // Schedule retry
                    if let Err(e) = self.retry_tx.send(RetryRequest {
                        message_id: id,
                        kind,
                        attempt: retry_count + 1,
                    }) {
                        error!("Failed to schedule retry: {}", e);
                    }

                    // Update retry count and reinsert
                    msg.retry_count += 1;
                    msg.sent_at = Instant::now(); // Reset timeout
                    pending.insert(id, msg);
                    self.metrics.record_retry(kind);
                } else {
                    error!(
                        "Message {} ({}) exhausted retries after {} attempts",
                        id,
                        kind.name(),
                        msg.max_retries
                    );
                    self.metrics.record_failure(kind);
                }
            }
        }
    }

    /// Start background task to check for timeouts
    pub fn start_timeout_checker(&self) {
        let pending = Arc::clone(&self.pending);
        let metrics = Arc::clone(&self.metrics);
        let retry_tx = self.retry_tx.clone();
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));

            loop {
                interval.tick().await;

                // Check shutdown flag
                if *shutdown.read().await {
                    info!("MessageTracker timeout checker shutting down");
                    break;
                }

                // Check for timeouts
                let mut pending_write = pending.write().await;
                let mut timed_out = Vec::new();

                for (id, msg) in pending_write.iter() {
                    if msg.is_timed_out() {
                        timed_out.push((*id, msg.kind, msg.retry_count));
                    }
                }

                // Handle timeouts
                for (id, kind, retry_count) in timed_out {
                    if let Some(mut msg) = pending_write.remove(&id) {
                        if msg.can_retry() {
                            warn!(
                                "Message {} ({}) timed out, retrying {}/{}",
                                id,
                                kind.name(),
                                retry_count + 1,
                                msg.max_retries
                            );

                            // Schedule retry
                            if let Err(e) = retry_tx.send(RetryRequest {
                                message_id: id,
                                kind,
                                attempt: retry_count + 1,
                            }) {
                                error!("Failed to schedule retry: {}", e);
                            }

                            // Update and reinsert
                            msg.retry_count += 1;
                            msg.sent_at = Instant::now();
                            pending_write.insert(id, msg);
                            metrics.record_retry(kind);
                        } else {
                            error!(
                                "Message {} ({}) exhausted retries",
                                id,
                                kind.name()
                            );
                            metrics.record_failure(kind);
                        }
                    }
                }
            }
        });
    }

    /// Calculate exponential backoff delay for retry
    pub fn calculate_retry_delay(attempt: u32) -> Duration {
        let base_delay = Duration::from_millis(100);
        let max_delay = Duration::from_secs(30);

        let delay = base_delay * 2u32.pow(attempt.saturating_sub(1));
        delay.min(max_delay)
    }

    /// Check if a message is currently pending
    pub async fn is_pending(&self, id: MessageId) -> bool {
        self.pending.read().await.contains_key(&id)
    }

    /// Get count of pending messages
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Get metrics
    pub fn metrics(&self) -> Arc<MessageMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Shutdown the tracker
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

    #[tokio::test]
    async fn test_track_and_acknowledge() {
        let tracker = MessageTracker::new();
        let msg_id = MessageId::new();

        tracker
            .track_default(msg_id, MessageKind::UpdateGPUGraphData)
            .await;

        assert!(tracker.is_pending(msg_id).await);
        assert_eq!(tracker.pending_count().await, 1);

        // Acknowledge
        let ack = MessageAck::success(msg_id);
        tracker.acknowledge(ack).await;

        assert!(!tracker.is_pending(msg_id).await);
        assert_eq!(tracker.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_timeout_detection() {
        let tracker = MessageTracker::new();
        let msg_id = MessageId::new();

        // Track with very short timeout
        tracker
            .track(
                msg_id,
                MessageKind::ComputeForces,
                Duration::from_millis(10),
                3,
            )
            .await;

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check timeouts
        tracker.check_timeouts().await;

        // Message should still be pending (with retry)
        assert!(tracker.is_pending(msg_id).await);

        // Verify retry was recorded
        let metrics = tracker.metrics();
        assert!(metrics.total_retried.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_retry_delay_calculation() {
        assert_eq!(
            MessageTracker::calculate_retry_delay(1),
            Duration::from_millis(100)
        );
        assert_eq!(
            MessageTracker::calculate_retry_delay(2),
            Duration::from_millis(200)
        );
        assert_eq!(
            MessageTracker::calculate_retry_delay(3),
            Duration::from_millis(400)
        );
        assert_eq!(
            MessageTracker::calculate_retry_delay(10),
            Duration::from_secs(30)
        ); // Capped
    }

    #[test]
    fn test_message_kind_defaults() {
        let kind = MessageKind::InitializeGPU;
        assert_eq!(kind.default_timeout(), Duration::from_secs(10));
        assert_eq!(kind.default_max_retries(), 5);
        assert_eq!(kind.name(), "InitializeGPU");
    }
}
