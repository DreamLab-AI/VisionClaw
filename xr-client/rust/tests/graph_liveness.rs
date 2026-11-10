//! Graph-socket liveness through the real pump (audit D2): the client pings, a
//! silent peer is dropped at the read deadline, an answering peer is kept, and a
//! large frame that takes longer than the deadline to arrive is not mistaken for
//! silence because its bytes keep flowing.
//!
//! Each test starts a local WebSocket server and drives
//! `transport::spawn_graph_stream_with` (the path `BinaryProtocolClient::connect_to_url`
//! takes) with short injected timings, then watches the inbox the way `poll()` does.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use visionclaw_xr_gdext::binary_protocol::GraphInbound;
use visionclaw_xr_gdext::liveness::LivenessConfig;
use visionclaw_xr_gdext::runtime::runtime;
use visionclaw_xr_gdext::transport::spawn_graph_stream_with;

type Inbox = Arc<Mutex<VecDeque<GraphInbound>>>;

fn quick() -> LivenessConfig {
    LivenessConfig {
        ping_every: Duration::from_millis(100),
        deadline: Duration::from_millis(500),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Connected,
    Disconnected,
    Text(usize),
    Other,
}

/// Drain the inbox until `until` matches or `limit` passes; returns what was seen
/// with the time since start of each event.
fn watch(inbox: &Inbox, limit: Duration, until: impl Fn(&Seen) -> bool) -> Vec<(Seen, Duration)> {
    let t0 = Instant::now();
    let mut seen = Vec::new();
    while t0.elapsed() < limit {
        let drained: Vec<GraphInbound> = inbox.lock().unwrap().drain(..).collect();
        for ev in drained {
            let s = match ev {
                GraphInbound::Connected => Seen::Connected,
                GraphInbound::Disconnected => Seen::Disconnected,
                GraphInbound::Text(t) => Seen::Text(t.len()),
                _ => Seen::Other,
            };
            let stop = until(&s);
            seen.push((s, t0.elapsed()));
            if stop {
                return seen;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

fn listen() -> (TcpListener, String) {
    let listener = runtime()
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let url = format!("ws://{}/wss", listener.local_addr().unwrap());
    (listener, url)
}

#[test]
fn a_silent_server_is_dropped_at_the_read_deadline() {
    let (listener, url) = listen();
    runtime().spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        // Accept, then never read (so never pong) and never write: a half-open peer.
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(ws);
    });
    let inbox: Inbox = Arc::new(Mutex::new(VecDeque::new()));
    let (handle, _tx) = spawn_graph_stream_with(url, String::new(), inbox.clone(), quick());
    let seen = watch(&inbox, Duration::from_secs(5), |s| *s == Seen::Disconnected);
    handle.abort();
    let kinds: Vec<Seen> = seen.iter().map(|(s, _)| s.clone()).collect();
    assert_eq!(kinds, vec![Seen::Connected, Seen::Disconnected], "{seen:?}");
    let gone = seen[1].1;
    assert!(
        gone >= Duration::from_millis(450) && gone < Duration::from_millis(1500),
        "dropped {gone:?} after start; deadline 500 ms"
    );
}

#[test]
fn an_answering_server_is_kept_past_many_deadlines() {
    let (listener, url) = listen();
    let pings = Arc::new(Mutex::new(0u32));
    let pings_srv = pings.clone();
    runtime().spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        // Reading makes tungstenite answer each ping; nothing else is ever sent.
        while let Some(Ok(msg)) = ws.next().await {
            if matches!(msg, Message::Ping(_)) {
                *pings_srv.lock().unwrap() += 1;
            }
        }
    });
    let inbox: Inbox = Arc::new(Mutex::new(VecDeque::new()));
    let (handle, _tx) = spawn_graph_stream_with(url, String::new(), inbox.clone(), quick());
    let seen = watch(&inbox, Duration::from_millis(2500), |s| {
        *s == Seen::Disconnected
    });
    handle.abort();
    let kinds: Vec<Seen> = seen.iter().map(|(s, _)| s.clone()).collect();
    assert_eq!(
        kinds,
        vec![Seen::Connected],
        "kept for 2.5 s = 5 deadlines: {seen:?}"
    );
    let n = *pings.lock().unwrap();
    assert!(
        (15..=30).contains(&n),
        "a ping every 100 ms reached the server: {n}"
    );
}

/// A server-side stream that writes at most 4 KiB per 20 ms: a slow link.
struct SlowWrite<S> {
    inner: S,
    pause: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<S: AsyncRead + Unpin> AsyncRead for SlowWrite<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for SlowWrite<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Some(p) = self.pause.as_mut() {
            if p.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.pause = None;
        }
        let n = buf.len().min(4096);
        let res = Pin::new(&mut self.inner).poll_write(cx, &buf[..n]);
        if matches!(res, Poll::Ready(Ok(_))) {
            self.pause = Some(Box::pin(tokio::time::sleep(Duration::from_millis(20))));
        }
        res
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[test]
fn a_large_frame_slower_than_the_deadline_is_not_silence() {
    let (listener, url) = listen();
    // 320 KiB at 4 KiB per 20 ms is about 1.6 s on the wire: three deadlines.
    let pad = "x".repeat(320 * 1024);
    let body = format!(r#"{{"type":"bulkTest","pad":"{pad}"}}"#);
    let len = body.len();
    runtime().spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let slow = SlowWrite {
            inner: tcp,
            pause: None,
        };
        let mut ws = tokio_tungstenite::accept_async(slow).await.unwrap();
        // No reads while the frame goes out, so no pongs: only its bytes show life.
        ws.send(Message::Text(body)).await.unwrap();
        while let Some(Ok(_)) = ws.next().await {}
    });
    let inbox: Inbox = Arc::new(Mutex::new(VecDeque::new()));
    let (handle, _tx) = spawn_graph_stream_with(url, String::new(), inbox.clone(), quick());
    let seen = watch(&inbox, Duration::from_secs(6), |s| {
        matches!(s, Seen::Text(_) | Seen::Disconnected)
    });
    handle.abort();
    let kinds: Vec<Seen> = seen.iter().map(|(s, _)| s.clone()).collect();
    assert_eq!(kinds, vec![Seen::Connected, Seen::Text(len)], "{seen:?}");
    assert!(
        seen[1].1 > Duration::from_millis(1000),
        "the frame really was slower than the deadline: {seen:?}"
    );
}
