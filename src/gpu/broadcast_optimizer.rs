//! GPU Physics Broadcast Rate Limiting
//!
//! Decides on WHICH physics frames a position snapshot is broadcast, so the broadcast rate
//! can sit below the physics tick rate. That is the only thing this module does.
//!
//! Every broadcast is a FULL SNAPSHOT of every node: clients tween to complete target
//! positions. Delta encoding is prohibited (docs/KNOWN_ISSUES.md BROADCAST-001, PRD-007 §3,
//! ADR-061), and so is any other way of sending a subset (spatial culling was removed with
//! the same rule): what a client receives is decided by its entitlement and its explicit
//! filter, never by whether a node moved or where the camera is.

use log::{debug, info};
use std::time::{Duration, Instant};

/// Default broadcast rate in Hz, defined with the setting that overrides it
/// (`PhysicsSettings::broadcast_fps`, exposed as `broadcastFps` on
/// `/api/settings/physics`).
///
/// 8 Hz is the cadence live clients actually received before the rate
/// limiter was corrected: the old limiter re-armed from "now" after every
/// broadcast, which rounded each interval up to whole physics frames and
/// turned a configured 10 Hz into ~8 Hz. Keeping 8 Hz keeps per-client
/// bandwidth where it was. Each broadcast is a full snapshot (~52 B per node;
/// ~490 KB for the ~9.5k-node corpus graph), so per-client bandwidth scales
/// linearly with the rate: 8 Hz is ~3.9 MB/s, 25 Hz is ~12 MB/s. Clients
/// tween between snapshots, so a higher rate buys smoothness, not
/// correctness.
pub use visionclaw_domain::types::physics_config::DEFAULT_BROADCAST_FPS;

/// Configuration for broadcast optimization
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Target broadcast rate in Hz (below the physics tick rate). See
    /// [`DEFAULT_BROADCAST_FPS`] for the bandwidth each rate costs.
    pub target_fps: u32,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            target_fps: DEFAULT_BROADCAST_FPS,
        }
    }
}

/// Rate limiter that gates broadcasts to the target frequency.
///
/// This purely controls broadcast *timing* — it decides on which frames a
/// full snapshot should be emitted. It does not track or diff positions;
/// the broadcast is always a full snapshot (BROADCAST-001).
///
/// Broadcast slots are phase-locked to the configured interval: each
/// broadcast schedules the next slot one interval after the *previous slot*,
/// not after the frame that happened to take it, so the long-run rate equals
/// `target_fps` even when the physics frame period does not divide the
/// interval. After a stall longer than one interval the limiter re-phases
/// from the late frame rather than bursting to catch up.
pub struct BroadcastRateLimiter {
    /// The earliest instant at which the next broadcast may go out.
    next_due: Instant,
    broadcast_interval: Duration,
    frames_since_broadcast: u32,
}

impl BroadcastRateLimiter {
    /// A limiter whose first frame broadcasts immediately.
    pub fn new(config: &BroadcastConfig) -> Self {
        Self::new_at(config, Instant::now())
    }

    /// [`new`](Self::new) against an explicit clock.
    pub fn new_at(config: &BroadcastConfig, now: Instant) -> Self {
        let fps = u64::from(config.target_fps.max(1));
        Self {
            next_due: now,
            broadcast_interval: Duration::from_micros(1_000_000 / fps),
            frames_since_broadcast: 0,
        }
    }

    /// Check if we should broadcast this frame
    pub fn should_broadcast(&mut self) -> bool {
        self.should_broadcast_at(Instant::now())
    }

    /// [`should_broadcast`](Self::should_broadcast) against an explicit clock.
    pub fn should_broadcast_at(&mut self, now: Instant) -> bool {
        self.frames_since_broadcast += 1;
        if now < self.next_due {
            return false;
        }
        self.next_due += self.broadcast_interval;
        if self.next_due <= now {
            // More than one interval behind (a stall): re-phase, no burst.
            self.next_due = now + self.broadcast_interval;
        }
        self.frames_since_broadcast = 0;
        true
    }

    /// Let the very next frame broadcast.
    pub fn reset(&mut self) {
        self.next_due = Instant::now();
    }

    /// Record a snapshot sent outside `should_broadcast` (a forced or
    /// periodic full broadcast) so the next one waits a full interval.
    pub fn mark_broadcast(&mut self) {
        self.mark_broadcast_at(Instant::now());
    }

    /// [`mark_broadcast`](Self::mark_broadcast) against an explicit clock.
    pub fn mark_broadcast_at(&mut self, now: Instant) {
        self.next_due = now + self.broadcast_interval;
        self.frames_since_broadcast = 0;
    }
}

/// Rate limiter plus counters. Owns no position data and never selects nodes.
pub struct BroadcastOptimizer {
    config: BroadcastConfig,
    rate_limiter: BroadcastRateLimiter,
    total_frames_processed: u64,
    total_snapshots_broadcast: u64,
}

impl BroadcastOptimizer {
    pub fn new(config: BroadcastConfig) -> Self {
        let rate_limiter = BroadcastRateLimiter::new(&config);
        Self { config, rate_limiter, total_frames_processed: 0, total_snapshots_broadcast: 0 }
    }

    /// Called once per physics frame. Returns true when a full snapshot of every node should be
    /// broadcast on this frame.
    pub fn should_broadcast(&mut self) -> bool {
        self.total_frames_processed += 1;
        let go = self.rate_limiter.should_broadcast();
        if go {
            self.total_snapshots_broadcast += 1;
        }
        go
    }

    /// Get overall counters.
    pub fn get_performance_stats(&self) -> BroadcastPerformanceStats {
        BroadcastPerformanceStats {
            total_frames_processed: self.total_frames_processed,
            total_snapshots_broadcast: self.total_snapshots_broadcast,
            target_fps: self.config.target_fps,
        }
    }

    /// Change the broadcast rate at runtime. Rejects a rate outside
    /// [`physics_bounds::BROADCAST_FPS`](crate::actors::gpu::physics_bounds::BROADCAST_FPS)
    /// without changing anything.
    pub fn configure(&mut self, target_fps: Option<u32>) -> Result<(), String> {
        use crate::actors::gpu::physics_bounds::{within, BROADCAST_FPS};
        let target_fps = target_fps.unwrap_or(self.config.target_fps);
        if !within(target_fps as f32, BROADCAST_FPS) {
            return Err(format!(
                "Invalid target_fps: {} (must be {}-{})",
                target_fps, BROADCAST_FPS.0, BROADCAST_FPS.1
            ));
        }
        self.update_config(BroadcastConfig { target_fps });
        Ok(())
    }

    /// Update configuration at runtime
    pub fn update_config(&mut self, config: BroadcastConfig) {
        info!("BroadcastOptimizer: target FPS {} -> {}", self.config.target_fps, config.target_fps);
        self.config = config;
        self.rate_limiter = BroadcastRateLimiter::new(&self.config);
    }

    /// Reset the broadcast rate-limit timer so the next frame broadcasts
    /// immediately. Call this when simulation parameters change or a new
    /// client connects, so the next full snapshot is emitted without waiting
    /// for the rate-limit interval. Not for use after a broadcast that was
    /// just sent: that is [`mark_broadcast`](Self::mark_broadcast).
    pub fn reset_broadcast_timer(&mut self) {
        debug!("BroadcastOptimizer: broadcast timer reset — next frame broadcasts a full snapshot");
        self.rate_limiter.reset();
    }

    /// Record a full snapshot sent outside `should_broadcast` (FastSettle final,
    /// periodic or `ForceFullBroadcast`), so the rate limiter waits a full
    /// interval before the next one instead of duplicating it.
    pub fn mark_broadcast(&mut self) {
        self.rate_limiter.mark_broadcast();
    }

    /// The configuration currently in force.
    pub fn config(&self) -> &BroadcastConfig {
        &self.config
    }
}

#[derive(Debug, Clone)]
pub struct BroadcastPerformanceStats {
    pub total_frames_processed: u64,
    pub total_snapshots_broadcast: u64,
    pub target_fps: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the limiter with frames every `frame_ms` for `secs` seconds of
    /// simulated time and return how many broadcasts it allowed.
    fn broadcasts_over(
        limiter: &mut BroadcastRateLimiter,
        start: Instant,
        frame_ms: u64,
        secs: u64,
    ) -> u64 {
        let frames = secs * 1000 / frame_ms;
        (1..=frames)
            .filter(|i| limiter.should_broadcast_at(start + Duration::from_millis(i * frame_ms)))
            .count() as u64
    }

    /// The measured broadcast interval matches the configured rate even when
    /// the physics frame period does not divide it. 8 Hz over 40 ms frames
    /// (25 Hz physics, the live cadence) must give 8 broadcasts a second, not
    /// the 6.25 a reset-to-now limiter produces by rounding every interval up
    /// to a whole number of frames.
    #[test]
    fn measured_interval_matches_configured_rate() {
        for (target_fps, frame_ms) in [(8u32, 40u64), (10, 40), (25, 16), (8, 16), (60, 5)] {
            let config = BroadcastConfig {
                target_fps,
            };
            let start = Instant::now();
            let mut limiter = BroadcastRateLimiter::new_at(&config, start);
            let secs = 30;
            let n = broadcasts_over(&mut limiter, start, frame_ms, secs);
            let expected = u64::from(target_fps) * secs;
            assert!(
                n.abs_diff(expected) <= 1,
                "{target_fps} Hz over {frame_ms} ms frames: {n} broadcasts in {secs} s, expected {expected}"
            );
        }
    }

    /// After a stall longer than one interval the limiter broadcasts once and
    /// re-phases; it does not fire a burst to catch up on missed slots.
    #[test]
    fn a_stall_does_not_cause_a_catch_up_burst() {
        let config = BroadcastConfig {
            target_fps: 8,
            ..BroadcastConfig::default()
        };
        let start = Instant::now();
        let mut limiter = BroadcastRateLimiter::new_at(&config, start);
        assert!(limiter.should_broadcast_at(start));
        let after_stall = start + Duration::from_secs(2);
        assert!(limiter.should_broadcast_at(after_stall));
        assert!(!limiter.should_broadcast_at(after_stall + Duration::from_millis(40)));
        assert!(!limiter.should_broadcast_at(after_stall + Duration::from_millis(80)));
    }

    /// `reset_broadcast_timer` does what its doc says: the very next frame
    /// broadcasts, even when one was sent a moment ago.
    #[test]
    fn reset_makes_the_next_frame_broadcast() {
        let mut optimizer = BroadcastOptimizer::new(BroadcastConfig::default());
        std::thread::sleep(Duration::from_millis(130));
        assert!(optimizer.should_broadcast(), "interval elapsed");
        assert!(!optimizer.should_broadcast(), "inside the interval");
        optimizer.reset_broadcast_timer();
        assert!(optimizer.should_broadcast(), "next frame after a reset");
        assert!(!optimizer.should_broadcast(), "then rate-limited again");
    }

    /// `mark_broadcast` records an out-of-band snapshot (FastSettle final,
    /// periodic, ForceFullBroadcast) so the limiter waits a full interval
    /// instead of sending a duplicate on the next frame.
    #[test]
    fn mark_broadcast_starts_a_full_interval() {
        let config = BroadcastConfig {
            target_fps: 8,
            ..BroadcastConfig::default()
        };
        let start = Instant::now();
        let mut limiter = BroadcastRateLimiter::new_at(&config, start);
        limiter.mark_broadcast_at(start + Duration::from_millis(500));
        assert!(!limiter.should_broadcast_at(start + Duration::from_millis(540)));
        assert!(!limiter.should_broadcast_at(start + Duration::from_millis(620)));
        assert!(limiter.should_broadcast_at(start + Duration::from_millis(625)));
    }

    /// The default is the ~8 Hz live clients have always received, so fixing
    /// the limiter does not change per-client bandwidth.
    #[test]
    fn default_rate_is_8_hz() {
        assert_eq!(BroadcastConfig::default().target_fps, 8);
    }

    /// The runtime setter (what `ConfigureBroadcastOptimization` and the physics settings route
    /// drive) changes the measured rate live and rejects out-of-range rates.
    #[test]
    fn configure_changes_the_rate_live_and_validates() {
        let mut optimizer = BroadcastOptimizer::new(BroadcastConfig::default());
        optimizer.configure(Some(12)).unwrap();
        assert_eq!(optimizer.config().target_fps, 12);

        let start = Instant::now();
        optimizer.rate_limiter = BroadcastRateLimiter::new_at(optimizer.config(), start);
        let n = broadcasts_over(&mut optimizer.rate_limiter, start, 16, 10);
        assert!(n.abs_diff(120) <= 1, "12 Hz for 10 s gave {n}");

        for bad in [0, 61] {
            assert!(optimizer.configure(Some(bad)).is_err());
            assert_eq!(optimizer.config().target_fps, 12, "a rejected rate changes nothing");
        }
        optimizer.configure(None).unwrap();
        assert_eq!(optimizer.config().target_fps, 12, "rate kept");
    }

    /// The optimizer owns no position data and has no way to select nodes: it answers one
    /// question per frame, "broadcast the complete snapshot now?". Rate-limited frames are
    /// skipped whole; broadcast frames are always the whole graph.
    #[test]
    fn the_optimizer_gates_whole_snapshots_and_never_selects_nodes() {
        let mut optimizer = BroadcastOptimizer::new(BroadcastConfig { target_fps: 60 });
        std::thread::sleep(Duration::from_millis(20));
        assert!(optimizer.should_broadcast(), "frame after the interval broadcasts");
        assert!(!optimizer.should_broadcast(), "frame inside the interval is skipped whole");
        std::thread::sleep(Duration::from_millis(20));
        assert!(optimizer.should_broadcast());
        let stats = optimizer.get_performance_stats();
        assert_eq!(stats.total_frames_processed, 3);
        assert_eq!(stats.total_snapshots_broadcast, 2);
    }
}
