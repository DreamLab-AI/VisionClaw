//! Per-thread CPU time for the frame-pack timing gate.
//!
//! Wall-clock timing charges a frame for time the thread spent descheduled: on
//! a loaded host the pack's p99 then measures the scheduler, not the pack (HP,
//! 2026-10-07: wall p99 2.04 ms against a p50 of 0.51 ms at load ≈ 6). The gate
//! asserts on `CLOCK_THREAD_CPUTIME_ID` instead — cycles this thread actually
//! ran — while wall time is still reported beside it so preemption stays
//! visible. Android (Quest) and Linux both provide the clock; elsewhere the
//! wall clock is the fallback.

use std::time::{Duration, Instant};

/// CPU time consumed by the calling thread, in nanoseconds.
#[cfg(unix)]
pub fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid, writable timespec; CLOCK_THREAD_CPUTIME_ID is
    // supported on Linux and Android. On failure the struct stays zeroed.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}

/// Wall-clock fallback where no per-thread CPU clock exists.
#[cfg(not(unix))]
pub fn thread_cpu_ns() -> u64 {
    use std::sync::OnceLock;
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Measures one span in both thread-CPU and wall time.
#[derive(Debug, Clone, Copy)]
pub struct CpuStopwatch {
    cpu0: u64,
    wall0: Instant,
}

impl CpuStopwatch {
    pub fn start() -> Self {
        Self { cpu0: thread_cpu_ns(), wall0: Instant::now() }
    }

    /// `(thread CPU, wall)` elapsed since `start`.
    pub fn stop(&self) -> (Duration, Duration) {
        (Duration::from_nanos(thread_cpu_ns().saturating_sub(self.cpu0)), self.wall0.elapsed())
    }
}
