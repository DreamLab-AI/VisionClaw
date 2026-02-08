//! Network Backpressure Control for GPU Physics Broadcast
//!
//! Implements token bucket algorithm with acknowledgement-based credit replenishment
//! to prevent buffer overflow when slow WebSocket clients cannot keep pace with
//! 60Hz GPU physics updates.
//!
//! ## Design
//!
//! The GPU runs physics at 60Hz, but network broadcasts target 25Hz via BroadcastOptimizer.
//! When network congestion occurs (slow clients, high latency), the backpressure system:
//!
//! 1. **Token Bucket**: Limits broadcast rate to prevent buffer overflow
//! 2. **Credit System**: Network layer sends acknowledgements to replenish tokens
//! 3. **Time-Based Refill**: Gradual token refill prevents burst after congestion clears
//! 4. **Metrics**: Tracks skipped frames and congestion duration for diagnostics
//!
//! ## Usage
//!
//! ```ignore
//! let mut backpressure = NetworkBackpressure::new(BackpressureConfig::default());
//!
//! // In physics loop:
//! if backpressure.try_acquire() {
//!     // Send broadcast
//!     client_coordinator.do_send(BroadcastPositions { ... });
//! } else {
//!     // Skip broadcast, continue simulation
//!     backpressure.record_skip();
//! }
//!
//! // On network acknowledgement:
//! backpressure.acknowledge(clients_delivered);
//! ```

use log::{debug, trace, warn};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Configuration for network backpressure
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Maximum tokens in bucket (burst capacity)
    pub max_tokens: u32,

    /// Initial tokens on startup
    pub initial_tokens: u32,

    /// Tokens refilled per second (leak rate)
    pub refill_rate_per_sec: f32,

    /// Minimum tokens required to broadcast
    pub broadcast_cost: u32,

    /// Tokens restored per acknowledgement
    pub ack_restore_tokens: u32,

    /// Enable time-based refill (vs ack-only)
    pub enable_time_refill: bool,

    /// Log congestion every N skipped frames
    pub log_interval_frames: u64,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            max_tokens: 100,
            initial_tokens: 100,
            refill_rate_per_sec: 30.0, // 30 tokens/sec = ~30Hz steady state
            broadcast_cost: 1,
            ack_restore_tokens: 1,
            enable_time_refill: true,
            log_interval_frames: 60,
        }
    }
}

/// Token bucket implementation for network backpressure
pub struct TokenBucket {
    /// Current token count (atomic for thread-safe access)
    tokens: AtomicU32,

    /// Maximum tokens
    max_tokens: u32,

    /// Tokens to add per refill interval
    refill_amount: f32,

    /// Accumulated fractional tokens from refills
    fractional_tokens: std::sync::Mutex<f32>,

    /// Last refill timestamp
    last_refill: std::sync::Mutex<Instant>,

    /// Refill interval
    refill_interval: Duration,
}

impl TokenBucket {
    /// Create a new token bucket
    pub fn new(max_tokens: u32, initial_tokens: u32, refill_rate_per_sec: f32) -> Self {
        // Calculate refill interval and amount
        // Target 10Hz refill checks for smooth token restoration
        let refill_interval = Duration::from_millis(100);
        let refill_amount = refill_rate_per_sec * 0.1; // 100ms = 0.1 sec

        Self {
            tokens: AtomicU32::new(initial_tokens.min(max_tokens)),
            max_tokens,
            refill_amount,
            fractional_tokens: std::sync::Mutex::new(0.0),
            last_refill: std::sync::Mutex::new(Instant::now()),
            refill_interval,
        }
    }

    /// Try to acquire tokens for a broadcast
    /// Returns true if tokens were acquired, false if bucket is empty
    pub fn try_acquire(&self, cost: u32) -> bool {
        // First, apply any pending time-based refill
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < cost {
                return false;
            }

            let new_value = current - cost;
            match self.tokens.compare_exchange_weak(
                current,
                new_value,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(_) => continue, // Retry on contention
            }
        }
    }

    /// Add tokens to bucket (from acknowledgements)
    pub fn restore(&self, amount: u32) {
        loop {
            let current = self.tokens.load(Ordering::Acquire);
            let new_value = (current + amount).min(self.max_tokens);
            if new_value == current {
                return; // Already at max
            }

            match self.tokens.compare_exchange_weak(
                current,
                new_value,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    trace!("TokenBucket: Restored {} tokens, now {}/{}", amount, new_value, self.max_tokens);
                    return;
                }
                Err(_) => continue,
            }
        }
    }

    /// Time-based refill (called automatically by try_acquire)
    fn refill(&self) {
        let mut last_refill = match self.last_refill.lock() {
            Ok(guard) => guard,
            Err(_) => return, // Skip refill on lock contention
        };

        let elapsed = last_refill.elapsed();
        if elapsed < self.refill_interval {
            return;
        }

        // Calculate tokens to add based on elapsed time
        let intervals = elapsed.as_secs_f32() / self.refill_interval.as_secs_f32();
        let mut fractional = match self.fractional_tokens.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };

        let total_tokens = (intervals * self.refill_amount) + *fractional;
        let whole_tokens = total_tokens.floor() as u32;
        *fractional = total_tokens - whole_tokens as f32;

        if whole_tokens > 0 {
            self.restore(whole_tokens);
        }

        *last_refill = Instant::now();
    }

    /// Get current token count
    pub fn available(&self) -> u32 {
        self.tokens.load(Ordering::Acquire)
    }

    /// Get bucket utilization (0.0 = empty, 1.0 = full)
    pub fn utilization(&self) -> f32 {
        self.tokens.load(Ordering::Acquire) as f32 / self.max_tokens as f32
    }
}

/// Network backpressure controller
pub struct NetworkBackpressure {
    /// Token bucket for rate limiting
    bucket: TokenBucket,

    /// Configuration
    config: BackpressureConfig,

    /// Frames skipped due to backpressure
    skipped_frames: AtomicU64,

    /// Congestion start time (None if not congested)
    congestion_start: std::sync::Mutex<Option<Instant>>,

    /// Total congestion duration
    total_congestion_duration: std::sync::Mutex<Duration>,

    /// Last log time for periodic congestion reporting
    last_log: std::sync::Mutex<Instant>,

    /// Broadcast sequence number for correlation
    sequence: AtomicU64,
}

impl NetworkBackpressure {
    /// Create a new backpressure controller
    pub fn new(config: BackpressureConfig) -> Self {
        let bucket = TokenBucket::new(
            config.max_tokens,
            config.initial_tokens,
            if config.enable_time_refill {
                config.refill_rate_per_sec
            } else {
                0.0 // Ack-only mode
            },
        );

        Self {
            bucket,
            config,
            skipped_frames: AtomicU64::new(0),
            congestion_start: std::sync::Mutex::new(None),
            total_congestion_duration: std::sync::Mutex::new(Duration::ZERO),
            last_log: std::sync::Mutex::new(Instant::now()),
            sequence: AtomicU64::new(0),
        }
    }

    /// Try to acquire permission for a broadcast
    /// Returns Some(sequence_id) if broadcast is allowed, None if backpressure active
    pub fn try_acquire(&self) -> Option<u64> {
        if self.bucket.try_acquire(self.config.broadcast_cost) {
            // Clear congestion state if we were previously congested
            if let Ok(mut congestion_start) = self.congestion_start.lock() {
                if let Some(start) = congestion_start.take() {
                    let duration = start.elapsed();
                    if let Ok(mut total) = self.total_congestion_duration.lock() {
                        *total += duration;
                    }
                    debug!(
                        "NetworkBackpressure: Congestion cleared after {:.1}ms",
                        duration.as_secs_f32() * 1000.0
                    );
                }
            }

            let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
            Some(seq)
        } else {
            None
        }
    }

    /// Record a skipped frame due to backpressure
    pub fn record_skip(&self) {
        let skipped = self.skipped_frames.fetch_add(1, Ordering::Relaxed) + 1;

        // Record congestion start if not already congested
        if let Ok(mut congestion_start) = self.congestion_start.lock() {
            if congestion_start.is_none() {
                *congestion_start = Some(Instant::now());
                debug!("NetworkBackpressure: Congestion started (tokens: {}/{})",
                       self.bucket.available(), self.config.max_tokens);
            }
        }

        // Periodic logging
        if skipped % self.config.log_interval_frames == 0 {
            if let Ok(mut last_log) = self.last_log.lock() {
                let elapsed = last_log.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    *last_log = Instant::now();
                    warn!(
                        "NetworkBackpressure: {} frames skipped, tokens: {}/{}, utilization: {:.1}%",
                        skipped,
                        self.bucket.available(),
                        self.config.max_tokens,
                        self.bucket.utilization() * 100.0
                    );
                }
            }
        }
    }

    /// Handle acknowledgement from network layer
    /// Called when clients confirm receipt of broadcast
    pub fn acknowledge(&self, clients_delivered: usize) {
        // Restore tokens based on acknowledgement
        // More clients = more confidence in network capacity
        let restore_amount = self.config.ack_restore_tokens;
        self.bucket.restore(restore_amount);

        trace!(
            "NetworkBackpressure: Ack received ({} clients), restored {} tokens, now {}/{}",
            clients_delivered,
            restore_amount,
            self.bucket.available(),
            self.config.max_tokens
        );
    }

    /// Get current backpressure metrics
    pub fn metrics(&self) -> BackpressureMetrics {
        let congestion_duration = self
            .total_congestion_duration
            .lock()
            .map(|d| *d)
            .unwrap_or(Duration::ZERO);

        let current_congestion = self
            .congestion_start
            .lock()
            .ok()
            .and_then(|s| *s)
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);

        BackpressureMetrics {
            available_tokens: self.bucket.available(),
            max_tokens: self.config.max_tokens,
            utilization: self.bucket.utilization(),
            skipped_frames: self.skipped_frames.load(Ordering::Relaxed),
            total_congestion_duration: congestion_duration + current_congestion,
            is_congested: self.bucket.available() == 0,
            sequence: self.sequence.load(Ordering::Relaxed),
        }
    }

    /// Reset metrics (for testing or session reset)
    pub fn reset_metrics(&self) {
        self.skipped_frames.store(0, Ordering::Relaxed);
        if let Ok(mut total) = self.total_congestion_duration.lock() {
            *total = Duration::ZERO;
        }
        if let Ok(mut congestion_start) = self.congestion_start.lock() {
            *congestion_start = None;
        }
    }

    /// Get current sequence number
    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    /// Check if currently congested
    pub fn is_congested(&self) -> bool {
        self.bucket.available() == 0
    }
}

/// Backpressure metrics for monitoring
#[derive(Debug, Clone)]
pub struct BackpressureMetrics {
    /// Available tokens in bucket
    pub available_tokens: u32,

    /// Maximum tokens
    pub max_tokens: u32,

    /// Bucket utilization (0.0-1.0)
    pub utilization: f32,

    /// Total frames skipped due to backpressure
    pub skipped_frames: u64,

    /// Total time spent in congestion
    pub total_congestion_duration: Duration,

    /// Currently congested
    pub is_congested: bool,

    /// Current broadcast sequence
    pub sequence: u64,
}

impl BackpressureMetrics {
    /// Convert to JSON for API responses
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "available_tokens": self.available_tokens,
            "max_tokens": self.max_tokens,
            "utilization": self.utilization,
            "skipped_frames": self.skipped_frames,
            "congestion_duration_ms": self.total_congestion_duration.as_millis(),
            "is_congested": self.is_congested,
            "sequence": self.sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_token_bucket_acquire() {
        let bucket = TokenBucket::new(10, 10, 0.0); // No refill for testing

        // Should acquire 10 tokens
        for _ in 0..10 {
            assert!(bucket.try_acquire(1));
        }

        // Bucket empty
        assert!(!bucket.try_acquire(1));
        assert_eq!(bucket.available(), 0);
    }

    #[test]
    fn test_token_bucket_restore() {
        let bucket = TokenBucket::new(10, 0, 0.0);
        assert_eq!(bucket.available(), 0);

        bucket.restore(5);
        assert_eq!(bucket.available(), 5);

        bucket.restore(10); // Should cap at max
        assert_eq!(bucket.available(), 10);
    }

    #[test]
    fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(100, 0, 100.0); // 100 tokens/sec

        // Wait for refill
        thread::sleep(Duration::from_millis(150));
        bucket.try_acquire(0); // Trigger refill check

        // Should have gained ~15 tokens (100 * 0.15)
        let available = bucket.available();
        assert!(available >= 10 && available <= 20, "Got {} tokens", available);
    }

    #[test]
    fn test_backpressure_congestion() {
        let config = BackpressureConfig {
            max_tokens: 5,
            initial_tokens: 5,
            refill_rate_per_sec: 0.0, // No refill
            broadcast_cost: 1,
            ack_restore_tokens: 1,
            enable_time_refill: false,
            log_interval_frames: 100,
        };

        let bp = NetworkBackpressure::new(config);

        // Use all tokens
        for i in 0..5 {
            assert!(bp.try_acquire().is_some(), "Should acquire at iteration {}", i);
        }

        // Now congested
        assert!(bp.try_acquire().is_none());
        assert!(bp.is_congested());

        bp.record_skip();
        assert_eq!(bp.metrics().skipped_frames, 1);

        // Acknowledge restores a token
        bp.acknowledge(1);
        assert!(!bp.is_congested());
        assert!(bp.try_acquire().is_some());
    }

    #[test]
    fn test_backpressure_sequence() {
        let bp = NetworkBackpressure::new(BackpressureConfig::default());

        let seq1 = bp.try_acquire().unwrap();
        let seq2 = bp.try_acquire().unwrap();
        let seq3 = bp.try_acquire().unwrap();

        assert_eq!(seq1, 0);
        assert_eq!(seq2, 1);
        assert_eq!(seq3, 2);
    }
}
