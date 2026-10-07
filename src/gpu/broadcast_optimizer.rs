//! GPU Physics Broadcast Optimization
//!
//! Reduces network bandwidth through:
//! - Adaptive broadcast frequency (broadcast rate below the physics tick rate)
//! - Spatial partitioning (visibility culling)
//!
//! The broadcast is FULL-SNAPSHOT ONLY — every broadcast sends complete target
//! positions and clients tween to them. Delta encoding is prohibited by design
//! (docs/KNOWN_ISSUES.md BROADCAST-001, PRD-007 §3, ADR-061); there is no
//! delta/diff path here. Indices returned by the optimizer are visibility-culled,
//! never delta-filtered.

use glam::Vec3;
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

    /// Enable spatial visibility culling
    pub enable_spatial_culling: bool,

    /// Camera frustum bounds for culling (min, max)
    pub camera_bounds: Option<(Vec3, Vec3)>,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            target_fps: DEFAULT_BROADCAST_FPS,
            enable_spatial_culling: false,
            camera_bounds: None,
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

    /// Get rate-limiter statistics
    pub fn get_stats(&self, total_nodes: usize, sent_nodes: usize) -> CompressionStats {
        let reduction_percent = if total_nodes > 0 {
            ((total_nodes - sent_nodes) as f32 / total_nodes as f32) * 100.0
        } else {
            0.0
        };

        CompressionStats {
            total_nodes,
            sent_nodes,
            reduction_percent,
            frames_since_broadcast: self.frames_since_broadcast,
        }
    }
}

/// Statistics for broadcast rate-limiting / culling performance
#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub total_nodes: usize,
    pub sent_nodes: usize,
    pub reduction_percent: f32,
    pub frames_since_broadcast: u32,
}

/// Spatial partitioning for visibility culling
pub struct SpatialCuller {
    enabled: bool,
    camera_bounds: Option<(Vec3, Vec3)>,
}

impl SpatialCuller {
    pub fn new(config: &BroadcastConfig) -> Self {
        Self {
            enabled: config.enable_spatial_culling,
            camera_bounds: config.camera_bounds,
        }
    }

    /// Update camera frustum bounds
    pub fn update_camera_bounds(&mut self, min: Vec3, max: Vec3) {
        self.camera_bounds = Some((min, max));
    }

    /// Filter positions to only visible nodes
    pub fn filter_visible(&self, positions: &[Vec3], node_ids: &[u32]) -> Vec<usize> {
        if !self.enabled {
            // Return all indices if culling disabled
            return (0..node_ids.len()).collect();
        }

        let Some((min, max)) = self.camera_bounds else {
            // No bounds set, return all
            return (0..node_ids.len()).collect();
        };

        let mut visible_indices = Vec::new();

        for (idx, pos) in positions.iter().enumerate() {
            // Simple AABB test
            if pos.x >= min.x
                && pos.x <= max.x
                && pos.y >= min.y
                && pos.y <= max.y
                && pos.z >= min.z
                && pos.z <= max.z
            {
                visible_indices.push(idx);
            }
        }

        visible_indices
    }
}

/// Main broadcast optimizer combining all techniques
pub struct BroadcastOptimizer {
    config: BroadcastConfig,
    rate_limiter: BroadcastRateLimiter,
    spatial_culler: SpatialCuller,
    total_frames_processed: u64,
    total_nodes_sent: u64,
    total_nodes_processed: u64,
}

impl BroadcastOptimizer {
    pub fn new(config: BroadcastConfig) -> Self {
        let rate_limiter = BroadcastRateLimiter::new(&config);
        let spatial_culler = SpatialCuller::new(&config);

        Self {
            config,
            rate_limiter,
            spatial_culler,
            total_frames_processed: 0,
            total_nodes_sent: 0,
            total_nodes_processed: 0,
        }
    }

    /// Process positions and return indices of nodes to broadcast.
    ///
    /// Returns `(should_broadcast, visible_indices)`. This is a full-snapshot
    /// broadcast; `visible_indices` are visibility-culled, never delta-filtered.
    /// When spatial culling is disabled (the default) `visible_indices` contains
    /// every node index, i.e. the complete snapshot.
    pub fn process_frame(
        &mut self,
        positions: &[(Vec3, Vec3)], // (position, velocity)
        node_ids: &[u32],
    ) -> (bool, Vec<usize>) {
        self.total_frames_processed += 1;

        // Rate-limit: only broadcast on frames within the target frequency.
        if !self.rate_limiter.should_broadcast() {
            return (false, Vec::new());
        }

        // Apply spatial culling to determine which nodes are visible. When
        // culling is disabled this returns all indices (full snapshot).
        let visible_indices = if self.spatial_culler.enabled {
            let pos_only: Vec<Vec3> = positions.iter().map(|(p, _)| *p).collect();
            self.spatial_culler.filter_visible(&pos_only, node_ids)
        } else {
            (0..node_ids.len()).collect()
        };

        self.total_nodes_sent += visible_indices.len() as u64;
        self.total_nodes_processed += node_ids.len() as u64;

        (true, visible_indices)
    }

    /// Get overall performance statistics
    pub fn get_performance_stats(&self) -> BroadcastPerformanceStats {
        let avg_reduction = if self.total_nodes_processed > 0 {
            ((self.total_nodes_processed - self.total_nodes_sent) as f64
                / self.total_nodes_processed as f64)
                * 100.0
        } else {
            0.0
        };

        BroadcastPerformanceStats {
            total_frames_processed: self.total_frames_processed,
            total_nodes_sent: self.total_nodes_sent,
            total_nodes_processed: self.total_nodes_processed,
            average_bandwidth_reduction: avg_reduction as f32,
            target_fps: self.config.target_fps,
        }
    }

    /// Change the rate and/or culling at runtime, leaving any field passed as
    /// `None` as it is. Rejects a rate outside
    /// [`physics_bounds::BROADCAST_FPS`](crate::actors::gpu::physics_bounds::BROADCAST_FPS)
    /// without changing anything. Camera bounds are kept.
    pub fn configure(
        &mut self,
        target_fps: Option<u32>,
        enable_spatial_culling: Option<bool>,
    ) -> Result<(), String> {
        use crate::actors::gpu::physics_bounds::{within, BROADCAST_FPS};
        let target_fps = target_fps.unwrap_or(self.config.target_fps);
        if !within(target_fps as f32, BROADCAST_FPS) {
            return Err(format!(
                "Invalid target_fps: {} (must be {}-{})",
                target_fps, BROADCAST_FPS.0, BROADCAST_FPS.1
            ));
        }
        let config = BroadcastConfig {
            target_fps,
            enable_spatial_culling: enable_spatial_culling
                .unwrap_or(self.config.enable_spatial_culling),
            camera_bounds: self.config.camera_bounds,
        };
        self.update_config(config);
        Ok(())
    }

    /// Update configuration at runtime
    pub fn update_config(&mut self, config: BroadcastConfig) {
        info!("BroadcastOptimizer: Updating configuration");
        info!(
            "  Target FPS: {} -> {}",
            self.config.target_fps, config.target_fps
        );

        self.config = config;
        self.rate_limiter = BroadcastRateLimiter::new(&self.config);
        self.spatial_culler = SpatialCuller::new(&self.config);
    }

    /// Update camera bounds for spatial culling
    pub fn update_camera_bounds(&mut self, min: Vec3, max: Vec3) {
        self.spatial_culler.update_camera_bounds(min, max);
        debug!(
            "BroadcastOptimizer: Camera bounds updated to [{:?}, {:?}]",
            min, max
        );
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

    /// Record a full snapshot sent outside `process_frame` (FastSettle final,
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
    pub total_nodes_sent: u64,
    pub total_nodes_processed: u64,
    pub average_bandwidth_reduction: f32,
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
                ..BroadcastConfig::default()
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
        let positions = vec![(Vec3::ZERO, Vec3::ZERO)];
        let ids = vec![0];
        std::thread::sleep(Duration::from_millis(130));
        assert!(
            optimizer.process_frame(&positions, &ids).0,
            "interval elapsed"
        );
        assert!(
            !optimizer.process_frame(&positions, &ids).0,
            "inside the interval"
        );
        optimizer.reset_broadcast_timer();
        assert!(
            optimizer.process_frame(&positions, &ids).0,
            "next frame after a reset"
        );
        assert!(
            !optimizer.process_frame(&positions, &ids).0,
            "then rate-limited again"
        );
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

    /// The runtime setter (what `ConfigureBroadcastOptimization` and the
    /// physics settings route drive) changes the measured rate live, keeps
    /// spatial culling unless told otherwise, and rejects out-of-range rates.
    #[test]
    fn configure_changes_the_rate_live_and_validates() {
        let mut optimizer = BroadcastOptimizer::new(BroadcastConfig {
            enable_spatial_culling: true,
            ..BroadcastConfig::default()
        });
        optimizer.configure(Some(12), None).unwrap();
        assert_eq!(optimizer.config().target_fps, 12);
        assert!(optimizer.config().enable_spatial_culling, "culling kept");

        let start = Instant::now();
        optimizer.rate_limiter = BroadcastRateLimiter::new_at(optimizer.config(), start);
        let n = broadcasts_over(&mut optimizer.rate_limiter, start, 16, 10);
        assert!(n.abs_diff(120) <= 1, "12 Hz for 10 s gave {n}");

        for bad in [0, 61] {
            assert!(optimizer.configure(Some(bad), None).is_err());
            assert_eq!(
                optimizer.config().target_fps,
                12,
                "a rejected rate changes nothing"
            );
        }
        optimizer.configure(None, Some(false)).unwrap();
        assert_eq!(optimizer.config().target_fps, 12, "rate kept");
        assert!(!optimizer.config().enable_spatial_culling);
    }

    #[test]
    fn test_spatial_culling() {
        let config = BroadcastConfig {
            target_fps: 30,
            enable_spatial_culling: true,
            camera_bounds: Some((Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0))),
        };

        let culler = SpatialCuller::new(&config);

        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),  // Inside
            Vec3::new(15.0, 0.0, 0.0), // Outside
            Vec3::new(5.0, 5.0, 5.0),  // Inside
            Vec3::new(0.0, 20.0, 0.0), // Outside
        ];
        let node_ids = vec![0, 1, 2, 3];

        let visible = culler.filter_visible(&positions, &node_ids);
        assert_eq!(visible.len(), 2, "Only 2 nodes should be visible");
        assert!(visible.contains(&0));
        assert!(visible.contains(&2));
    }

    #[test]
    fn test_broadcast_optimizer_integration() {
        let config = BroadcastConfig {
            target_fps: 60, // High rate for testing
            enable_spatial_culling: false,
            camera_bounds: None,
        };

        let mut optimizer = BroadcastOptimizer::new(config);

        // Simulate multiple frames
        let positions = vec![
            (Vec3::new(0.0, 0.0, 0.0), Vec3::ZERO),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO),
        ];
        let node_ids = vec![0, 1];

        // First frame after the interval elapses should broadcast the full snapshot.
        std::thread::sleep(Duration::from_millis(20));
        let (should_broadcast, indices) = optimizer.process_frame(&positions, &node_ids);
        assert!(should_broadcast, "Frame after interval should broadcast");
        assert_eq!(indices.len(), 2, "Full snapshot: all node indices returned");

        // A frame taken immediately afterwards is inside the rate-limit interval
        // and must be gated out (no broadcast, no indices).
        let (should_broadcast, indices) = optimizer.process_frame(&positions, &node_ids);
        assert!(
            !should_broadcast,
            "Frame inside interval should be rate-limited"
        );
        assert!(indices.is_empty(), "Rate-limited frame returns no indices");

        // After the interval elapses again the full snapshot is emitted — every
        // node, never a delta-filtered subset.
        std::thread::sleep(Duration::from_millis(20));
        let (should_broadcast, indices) = optimizer.process_frame(&positions, &node_ids);
        assert!(
            should_broadcast,
            "Frame after interval should broadcast again"
        );
        assert_eq!(indices.len(), 2, "Full snapshot always returns all nodes");
    }
}
