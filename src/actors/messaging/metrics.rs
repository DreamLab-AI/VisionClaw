//! Metrics for message tracking

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::MessageKind;

/// Per-message-kind metrics
#[derive(Debug, Default)]
pub struct KindMetrics {
    pub sent_count: AtomicU64,
    pub success_count: AtomicU64,
    pub failure_count: AtomicU64,
    /// Messages that missed their acknowledgement deadline.
    pub timeout_count: AtomicU64,
    /// Acknowledgements that arrived after the deadline.
    pub late_ack_count: AtomicU64,
    pub total_latency_ms: AtomicU64,
}

impl KindMetrics {
    /// Get average latency in milliseconds
    pub fn avg_latency_ms(&self) -> f64 {
        let successes = self.success_count.load(Ordering::Relaxed);
        if successes == 0 {
            return 0.0;
        }

        let total_latency = self.total_latency_ms.load(Ordering::Relaxed);
        total_latency as f64 / successes as f64
    }

    /// Get success rate (0.0 - 1.0)
    pub fn success_rate(&self) -> f64 {
        let sent = self.sent_count.load(Ordering::Relaxed);
        if sent == 0 {
            return 1.0; // No messages sent = 100% success rate
        }

        let success = self.success_count.load(Ordering::Relaxed);
        success as f64 / sent as f64
    }

    /// Get failure rate (0.0 - 1.0)
    pub fn failure_rate(&self) -> f64 {
        let sent = self.sent_count.load(Ordering::Relaxed);
        if sent == 0 {
            return 0.0;
        }

        let failure = self.failure_count.load(Ordering::Relaxed);
        failure as f64 / sent as f64
    }
}

/// Comprehensive metrics for message tracking
#[derive(Debug)]
pub struct MessageMetrics {
    /// Total messages sent
    pub total_sent: AtomicU64,

    /// Total messages acknowledged successfully
    pub total_acked: AtomicU64,

    /// Total messages failed
    pub total_failed: AtomicU64,

    /// Total messages that missed their acknowledgement deadline
    pub total_timed_out: AtomicU64,

    /// Total acknowledgements that arrived after the deadline
    pub total_late_acked: AtomicU64,

    /// Per-message-kind metrics
    by_kind: Arc<RwLock<HashMap<MessageKind, Arc<KindMetrics>>>>,
}

impl MessageMetrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self {
            total_sent: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_late_acked: AtomicU64::new(0),
            by_kind: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a sent message
    pub fn record_sent(&self, kind: MessageKind) {
        self.total_sent.fetch_add(1, Ordering::Relaxed);

        // Update per-kind metrics
        let by_kind = Arc::clone(&self.by_kind);
        tokio::spawn(async move {
            let mut map = by_kind.write().await;
            let metrics = map
                .entry(kind)
                .or_insert_with(|| Arc::new(KindMetrics::default()));
            metrics.sent_count.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Record a successful acknowledgment
    pub fn record_success(&self, kind: MessageKind, latency: Duration) {
        self.total_acked.fetch_add(1, Ordering::Relaxed);

        let latency_ms = latency.as_millis() as u64;
        let by_kind = Arc::clone(&self.by_kind);

        tokio::spawn(async move {
            let mut map = by_kind.write().await;
            let metrics = map
                .entry(kind)
                .or_insert_with(|| Arc::new(KindMetrics::default()));
            metrics.success_count.fetch_add(1, Ordering::Relaxed);
            metrics
                .total_latency_ms
                .fetch_add(latency_ms, Ordering::Relaxed);
        });
    }

    /// Record a failed message
    pub fn record_failure(&self, kind: MessageKind) {
        self.total_failed.fetch_add(1, Ordering::Relaxed);

        let by_kind = Arc::clone(&self.by_kind);
        tokio::spawn(async move {
            let mut map = by_kind.write().await;
            let metrics = map
                .entry(kind)
                .or_insert_with(|| Arc::new(KindMetrics::default()));
            metrics.failure_count.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Record a missed acknowledgement deadline
    pub fn record_timeout(&self, kind: MessageKind) {
        self.total_timed_out.fetch_add(1, Ordering::Relaxed);

        let by_kind = Arc::clone(&self.by_kind);
        tokio::spawn(async move {
            let mut map = by_kind.write().await;
            let metrics = map
                .entry(kind)
                .or_insert_with(|| Arc::new(KindMetrics::default()));
            metrics.timeout_count.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Record an acknowledgement that arrived after its deadline
    pub fn record_late_ack(&self, kind: MessageKind) {
        self.total_late_acked.fetch_add(1, Ordering::Relaxed);

        let by_kind = Arc::clone(&self.by_kind);
        tokio::spawn(async move {
            let mut map = by_kind.write().await;
            let metrics = map
                .entry(kind)
                .or_insert_with(|| Arc::new(KindMetrics::default()));
            metrics.late_ack_count.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Get metrics for a specific message kind
    pub async fn get_kind_metrics(&self, kind: MessageKind) -> Option<Arc<KindMetrics>> {
        self.by_kind.read().await.get(&kind).cloned()
    }

    /// Get all message kinds with metrics
    pub async fn all_kinds(&self) -> Vec<MessageKind> {
        self.by_kind.read().await.keys().copied().collect()
    }

    /// Get overall success rate
    pub fn overall_success_rate(&self) -> f64 {
        let sent = self.total_sent.load(Ordering::Relaxed);
        if sent == 0 {
            return 1.0;
        }

        let acked = self.total_acked.load(Ordering::Relaxed);
        acked as f64 / sent as f64
    }

    /// Get overall failure rate
    pub fn overall_failure_rate(&self) -> f64 {
        let sent = self.total_sent.load(Ordering::Relaxed);
        if sent == 0 {
            return 0.0;
        }

        let failed = self.total_failed.load(Ordering::Relaxed);
        failed as f64 / sent as f64
    }

    /// Get summary statistics
    pub async fn summary(&self) -> MetricsSummary {
        let mut kind_summaries = Vec::new();

        for kind in self.all_kinds().await {
            if let Some(metrics) = self.get_kind_metrics(kind).await {
                kind_summaries.push(KindSummary {
                    kind,
                    sent: metrics.sent_count.load(Ordering::Relaxed),
                    success: metrics.success_count.load(Ordering::Relaxed),
                    failure: metrics.failure_count.load(Ordering::Relaxed),
                    timed_out: metrics.timeout_count.load(Ordering::Relaxed),
                    late_acked: metrics.late_ack_count.load(Ordering::Relaxed),
                    avg_latency_ms: metrics.avg_latency_ms(),
                    success_rate: metrics.success_rate(),
                });
            }
        }

        MetricsSummary {
            total_sent: self.total_sent.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_failed: self.total_failed.load(Ordering::Relaxed),
            total_timed_out: self.total_timed_out.load(Ordering::Relaxed),
            total_late_acked: self.total_late_acked.load(Ordering::Relaxed),
            overall_success_rate: self.overall_success_rate(),
            overall_failure_rate: self.overall_failure_rate(),
            by_kind: kind_summaries,
        }
    }
}

impl Default for MessageMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of metrics for a specific message kind
#[derive(Debug, Clone)]
pub struct KindSummary {
    pub kind: MessageKind,
    pub sent: u64,
    pub success: u64,
    pub failure: u64,
    pub timed_out: u64,
    pub late_acked: u64,
    pub avg_latency_ms: f64,
    pub success_rate: f64,
}

/// Overall metrics summary
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub total_sent: u64,
    pub total_acked: u64,
    pub total_failed: u64,
    pub total_timed_out: u64,
    pub total_late_acked: u64,
    pub overall_success_rate: f64,
    pub overall_failure_rate: f64,
    pub by_kind: Vec<KindSummary>,
}

impl std::fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Message Tracking Metrics:")?;
        writeln!(f, "  Total Sent: {}", self.total_sent)?;
        writeln!(f, "  Total Acknowledged: {}", self.total_acked)?;
        writeln!(f, "  Total Failed: {}", self.total_failed)?;
        writeln!(f, "  Total Timed Out: {}", self.total_timed_out)?;
        writeln!(f, "  Total Late Acks: {}", self.total_late_acked)?;
        writeln!(
            f,
            "  Overall Success Rate: {:.2}%",
            self.overall_success_rate * 100.0
        )?;
        writeln!(
            f,
            "  Overall Failure Rate: {:.2}%",
            self.overall_failure_rate * 100.0
        )?;

        if !self.by_kind.is_empty() {
            writeln!(f, "\n  By Message Kind:")?;
            for kind in &self.by_kind {
                writeln!(f, "    {}:", kind.kind.name())?;
                writeln!(f, "      Sent: {}", kind.sent)?;
                writeln!(f, "      Success: {}", kind.success)?;
                writeln!(f, "      Failure: {}", kind.failure)?;
                writeln!(f, "      Timed Out: {}", kind.timed_out)?;
                writeln!(f, "      Late Acks: {}", kind.late_acked)?;
                writeln!(f, "      Avg Latency: {:.2}ms", kind.avg_latency_ms)?;
                writeln!(f, "      Success Rate: {:.2}%", kind.success_rate * 100.0)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_recording() {
        let metrics = MessageMetrics::new();

        // Record sent
        metrics.record_sent(MessageKind::UpdateGPUGraphData);
        assert_eq!(metrics.total_sent.load(Ordering::Relaxed), 1);

        // Record success
        metrics.record_success(MessageKind::UpdateGPUGraphData, Duration::from_millis(100));

        // Wait for async updates
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert_eq!(metrics.total_acked.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_success_rate_calculation() {
        let metrics = MessageMetrics::new();

        // Send 10, succeed 8, fail 2
        for _ in 0..10 {
            metrics.record_sent(MessageKind::UploadConstraintsToGPU);
        }
        for _ in 0..8 {
            metrics.record_success(
                MessageKind::UploadConstraintsToGPU,
                Duration::from_millis(50),
            );
        }
        for _ in 0..2 {
            metrics.record_failure(MessageKind::UploadConstraintsToGPU);
        }

        // Wait for async updates
        tokio::time::sleep(Duration::from_millis(10)).await;

        let rate = metrics.overall_success_rate();
        assert!((rate - 0.8).abs() < 0.01); // ~80% success rate
    }

    #[tokio::test]
    async fn test_metrics_summary() {
        let metrics = MessageMetrics::new();

        metrics.record_sent(MessageKind::InitializeGPU);
        metrics.record_success(MessageKind::InitializeGPU, Duration::from_millis(500));

        // Wait for async updates
        tokio::time::sleep(Duration::from_millis(10)).await;

        let summary = metrics.summary().await;
        assert_eq!(summary.total_sent, 1);
        assert_eq!(summary.total_acked, 1);
        assert!(!summary.by_kind.is_empty());
    }
}
