//! Graph-socket liveness: the client's own ping and read deadline (audit D2).
//!
//! The `/wss` pump used to answer the server's pings and never send its own, with
//! no read deadline. A half-open TCP connection (headset doffed, Wi-Fi roam, the
//! server host vanishing without a FIN) then showed "connected" over a frozen graph
//! until someone pressed Reconnect.
//!
//! Two parts:
//! - [`ReadClock`] wraps the TCP stream under the WebSocket and stamps the instant
//!   of every read that delivered bytes. Liveness is measured in bytes, not whole
//!   messages, so a 30 MB `initialGraphLoad` that takes longer than the deadline to
//!   arrive keeps the connection alive for as long as its bytes keep flowing.
//! - [`Liveness`] is the pure state machine the pump consults: send a ping every
//!   [`LivenessConfig::ping_every`]; declare the socket dead once nothing at all has
//!   been read for [`LivenessConfig::deadline`]. The server pongs a client ping
//!   (`src/handlers/socket_flow_handler/mod.rs`), so an idle but healthy socket still
//!   reads a few bytes every ping interval.
//!
//! Time is passed in, never read, so the machine is tested with constructed
//! instants (`tests` below) and the pump with injected timings
//! (`tests/graph_liveness.rs`).

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Timings for one graph socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivenessConfig {
    /// How often the client pings. Several pings fit in one deadline, so one lost
    /// pong never ends a healthy connection.
    pub ping_every: Duration,
    /// How long the socket may go without reading a single byte before it is
    /// declared dead and closed (the scene then redials with backoff).
    pub deadline: Duration,
}

impl Default for LivenessConfig {
    fn default() -> Self {
        Self {
            ping_every: Duration::from_secs(3),
            deadline: Duration::from_secs(10),
        }
    }
}

/// What the pump should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessAction {
    /// Nothing is due; consult the machine again at this instant (or sooner, when
    /// a message arrives).
    Wait(Instant),
    /// Send a WebSocket ping now.
    Ping,
    /// Nothing read for the whole deadline: close the socket.
    Expired,
}

/// The ping/deadline state machine for one connection.
#[derive(Debug, Clone)]
pub struct Liveness {
    config: LivenessConfig,
    last_ping: Instant,
}

impl Liveness {
    /// A machine for a connection that opened at `now` (the upgrade counts as the
    /// first read, so the first ping is one interval away).
    pub fn new(config: LivenessConfig, now: Instant) -> Self {
        Self {
            config,
            last_ping: now,
        }
    }

    /// The timings in force.
    pub fn config(&self) -> LivenessConfig {
        self.config
    }

    /// Decide at `now`, given the instant bytes were last read. A `Ping` answer
    /// records the ping as sent at `now`.
    pub fn next(&mut self, now: Instant, last_read: Instant) -> LivenessAction {
        if now.saturating_duration_since(last_read) >= self.config.deadline {
            return LivenessAction::Expired;
        }
        if now.saturating_duration_since(self.last_ping) >= self.config.ping_every {
            self.last_ping = now;
            return LivenessAction::Ping;
        }
        let ping_due = self.last_ping + self.config.ping_every;
        let dead_at = last_read + self.config.deadline;
        LivenessAction::Wait(ping_due.min(dead_at))
    }
}

/// The instant a [`ReadClock`] last delivered bytes, shared with the pump.
#[derive(Debug)]
pub struct ReadProgress {
    origin: Instant,
    /// Nanoseconds after `origin` of the last read that delivered bytes.
    last_ns: AtomicU64,
}

impl ReadProgress {
    /// Progress for a connection opened at `now`.
    pub fn new(now: Instant) -> Arc<Self> {
        Arc::new(Self {
            origin: now,
            last_ns: AtomicU64::new(0),
        })
    }

    /// Record progress at `now`. Used by the reader, and by the pump after work
    /// that kept it from reading (a large topology parse), so time spent parsing is
    /// never mistaken for a silent peer.
    pub fn touch(&self, now: Instant) {
        let ns = now.saturating_duration_since(self.origin).as_nanos();
        self.last_ns
            .fetch_max(u64::try_from(ns).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// When bytes were last read (or progress last touched).
    pub fn last_read(&self) -> Instant {
        self.origin + Duration::from_nanos(self.last_ns.load(Ordering::Relaxed))
    }
}

/// A stream wrapper that stamps [`ReadProgress`] on every read that delivers bytes.
#[derive(Debug)]
pub struct ReadClock<S> {
    inner: S,
    progress: Arc<ReadProgress>,
}

impl<S> ReadClock<S> {
    /// Wrap `inner`, reporting reads to `progress`.
    pub fn new(inner: S, progress: Arc<ReadProgress>) -> Self {
        Self { inner, progress }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ReadClock<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let res = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(res, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            self.progress.touch(Instant::now());
        }
        res
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for ReadClock<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LivenessConfig {
        LivenessConfig {
            ping_every: Duration::from_secs(3),
            deadline: Duration::from_secs(10),
        }
    }

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_fresh_connection_waits_for_the_first_ping() {
        let t0 = Instant::now();
        let mut l = Liveness::new(cfg(), t0);
        assert_eq!(l.next(t0, t0), LivenessAction::Wait(t0 + s(3)));
    }

    #[test]
    fn it_pings_every_interval_while_the_peer_answers() {
        let t0 = Instant::now();
        let mut l = Liveness::new(cfg(), t0);
        let mut pings = 0;
        // One minute in 100 ms steps; the peer's pong arrives 50 ms after each ping.
        let mut last_read = t0;
        let mut pong_at: Option<Instant> = None;
        for step in 0..600u64 {
            let now = t0 + Duration::from_millis(step * 100);
            if let Some(p) = pong_at {
                if now >= p {
                    last_read = p;
                    pong_at = None;
                }
            }
            match l.next(now, last_read) {
                LivenessAction::Ping => {
                    pings += 1;
                    pong_at = Some(now + Duration::from_millis(50));
                }
                LivenessAction::Expired => panic!("expired at step {step} with a live peer"),
                LivenessAction::Wait(_) => {}
            }
        }
        assert_eq!(pings, 19, "a ping every 3 s over 59.9 s");
    }

    #[test]
    fn a_silent_peer_expires_at_the_deadline_not_before() {
        let t0 = Instant::now();
        let mut l = Liveness::new(cfg(), t0);
        // Pings go out but nothing ever comes back.
        assert_eq!(l.next(t0 + s(3), t0), LivenessAction::Ping);
        assert_eq!(l.next(t0 + s(6), t0), LivenessAction::Ping);
        assert_eq!(l.next(t0 + s(9), t0), LivenessAction::Ping);
        let just_before = t0 + s(10) - Duration::from_millis(1);
        assert_eq!(l.next(just_before, t0), LivenessAction::Wait(t0 + s(10)));
        assert_eq!(l.next(t0 + s(10), t0), LivenessAction::Expired);
    }

    #[test]
    fn wait_never_overshoots_the_deadline() {
        let t0 = Instant::now();
        let mut l = Liveness::new(
            LivenessConfig {
                ping_every: s(30),
                deadline: s(10),
            },
            t0,
        );
        assert_eq!(l.next(t0 + s(4), t0), LivenessAction::Wait(t0 + s(10)));
    }

    #[test]
    fn a_resumed_process_finds_a_stale_socket_already_expired() {
        // The app was paused (headset doffed): no step ran for two minutes. The first
        // decision after resume closes the socket instead of trusting it.
        let t0 = Instant::now();
        let mut l = Liveness::new(cfg(), t0);
        assert_eq!(
            l.next(t0 + s(1), t0 + s(1)),
            LivenessAction::Wait(t0 + s(3))
        );
        assert_eq!(l.next(t0 + s(121), t0 + s(1)), LivenessAction::Expired);
    }

    #[test]
    fn progress_records_the_latest_touch_and_never_goes_back() {
        let t0 = Instant::now();
        let p = ReadProgress::new(t0);
        assert_eq!(p.last_read(), t0);
        p.touch(t0 + s(5));
        assert_eq!(p.last_read(), t0 + s(5));
        p.touch(t0 + s(2));
        assert_eq!(p.last_read(), t0 + s(5), "a late, older touch is ignored");
    }

    #[tokio::test]
    async fn read_clock_stamps_reads_that_deliver_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (a, mut b) = tokio::io::duplex(64);
        let t0 = Instant::now();
        let progress = ReadProgress::new(t0);
        let mut clocked = ReadClock::new(a, progress.clone());
        tokio::time::sleep(Duration::from_millis(20)).await;
        b.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        clocked.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        assert!(progress.last_read() >= t0 + Duration::from_millis(20));
        // Writes pass through and do not count as reads.
        let stamped = progress.last_read();
        clocked.write_all(b"x").await.unwrap();
        let mut one = [0u8; 1];
        b.read_exact(&mut one).await.unwrap();
        assert_eq!(progress.last_read(), stamped);
    }
}
