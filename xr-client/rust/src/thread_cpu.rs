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
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
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

/// Parses a sysfs CPU list (`"0-5,24-29"`) into ascending CPU indices.
/// Malformed or reversed ranges are skipped rather than panicking.
pub fn parse_cpu_list(list: &str) -> Vec<usize> {
    let mut cpus = Vec::new();
    for part in list.trim().split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    if a <= b {
                        cpus.extend(a..=b);
                    }
                }
            }
            None => {
                if let Ok(c) = part.parse::<usize>() {
                    cpus.push(c);
                }
            }
        }
    }
    cpus.sort_unstable();
    cpus.dedup();
    cpus
}

/// Pins the calling thread to the CPUs sharing its current last-level (L3)
/// cache, returning that set; `None` where the topology or the call is
/// unavailable (the thread is then left as it was).
///
/// For the benchmark's CPU gate only. On a multi-L3 host (HP's Threadripper
/// 7965WX: four 6-core domains) the scheduler sometimes migrates the main thread
/// across domains; the pack's working set is then cold and the next two frames
/// run ~8x slower on-CPU (0.5 -> 4.2 ms, thread CPU ≈ wall, so not preemption).
/// How often depends on host load, which made the p99 gate intermittent
/// (2026-10-07: 2.62 ms against 2.0 once; 0 spikes in 10 pinned runs, 3-25 in
/// each of 10 unpinned). The gate budgets the pack itself for a single-cluster
/// headset, so the benchmark measures it inside one domain.
#[cfg(target_os = "linux")]
pub fn pin_current_thread_to_l3() -> Option<Vec<usize>> {
    // SAFETY: sched_getcpu has no preconditions; it returns -1 on failure.
    let cpu = unsafe { libc::sched_getcpu() };
    if cpu < 0 {
        return None;
    }
    let path = format!("/sys/devices/system/cpu/cpu{cpu}/cache/index3/shared_cpu_list");
    // Only CPUs the thread may already use (a container's cpuset narrows the
    // domain; the kernel would drop the rest silently), so the returned set is
    // exactly the affinity that results.
    let size = std::mem::size_of::<libc::cpu_set_t>();
    // SAFETY: an all-zero cpu_set_t is the empty set; pid 0 is the calling thread.
    let mut allowed: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::sched_getaffinity(0, size, &mut allowed) } != 0 {
        return None;
    }
    let domain: Vec<usize> = parse_cpu_list(&std::fs::read_to_string(path).ok()?)
        .into_iter()
        // SAFETY: CPU_ISSET reads index c < CPU_SETSIZE of a valid set.
        .filter(|&c| c < libc::CPU_SETSIZE as usize && unsafe { libc::CPU_ISSET(c, &allowed) })
        .collect();
    if !domain.contains(&(cpu as usize)) {
        return None;
    }
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for &c in &domain {
        // SAFETY: every c is below CPU_SETSIZE (filtered above).
        unsafe { libc::CPU_SET(c, &mut set) };
    }
    // SAFETY: pid 0 is the calling thread; `set` is a valid cpu_set_t of the size passed.
    let rc = unsafe { libc::sched_setaffinity(0, size, &set) };
    (rc == 0).then_some(domain)
}

/// No portable affinity call off Linux: the thread is left as it was.
#[cfg(not(target_os = "linux"))]
pub fn pin_current_thread_to_l3() -> Option<Vec<usize>> {
    None
}

/// Measures one span in both thread-CPU and wall time.
#[derive(Debug, Clone, Copy)]
pub struct CpuStopwatch {
    cpu0: u64,
    wall0: Instant,
}

impl CpuStopwatch {
    pub fn start() -> Self {
        Self {
            cpu0: thread_cpu_ns(),
            wall0: Instant::now(),
        }
    }

    /// `(thread CPU, wall)` elapsed since `start`.
    pub fn stop(&self) -> (Duration, Duration) {
        (
            Duration::from_nanos(thread_cpu_ns().saturating_sub(self.cpu0)),
            self.wall0.elapsed(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_parse_like_sysfs_writes_them() {
        assert_eq!(parse_cpu_list("0-5,24-29"), vec![0, 1, 2, 3, 4, 5, 24, 25, 26, 27, 28, 29]);
        assert_eq!(parse_cpu_list("7"), vec![7]);
        assert_eq!(parse_cpu_list("0,2-3\n"), vec![0, 2, 3]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
        assert_eq!(parse_cpu_list("3-1,x,4"), vec![4], "malformed ranges are skipped, never panic");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pinning_keeps_the_thread_inside_its_l3_domain() {
        // its own thread, so the harness thread keeps its affinity
        std::thread::spawn(|| {
            let Some(domain) = pin_current_thread_to_l3() else {
                return; // no sysfs cache topology (some containers): nothing to pin
            };
            let cpu = unsafe { libc::sched_getcpu() };
            assert!(cpu >= 0 && domain.contains(&(cpu as usize)), "still on a CPU of the domain");
            let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
            let rc = unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) };
            assert_eq!(rc, 0);
            let allowed: Vec<usize> = (0..libc::CPU_SETSIZE as usize).filter(|&c| unsafe { libc::CPU_ISSET(c, &set) }).collect();
            assert_eq!(allowed, domain, "affinity is exactly the returned set (L3 domain within the cpuset)");
        })
        .join()
        .unwrap();
    }
}
