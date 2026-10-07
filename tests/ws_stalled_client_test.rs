//! A `/wss`-style client that stays connected but never reads is closed by
//! the server once it has been congested for its stall timeout, and the
//! coordinator's client count drops.
//!
//! Real actix-web server, real `ClientCoordinatorActor`, real
//! `transport::capture_transport` on-connect hook; the session actor mirrors
//! how `SocketFlowServer` registers. The peer is a raw TCP client that does
//! the WebSocket handshake and then never reads, so the server's writes stall
//! and actix stops running the session actor: only the transport closer can
//! end the connection.

use actix::prelude::*;
use actix_web::{web, App, HttpRequest, HttpServer};
use actix_web_actors::ws;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use visionclaw_server::actors::client_coordinator_actor::ClientCoordinatorActor;
use visionclaw_server::actors::messages::{
    BroadcastNodePositions, ClientRecipients, CloseClientSession, GetClientCount, RegisterClient,
    SendInitialGraphLoad, SendToClientBinary, SendToClientText,
};
use visionclaw_server::handlers::socket_flow_handler::transport::{
    capture_transport, ConnTransport,
};

const STALL_TIMEOUT: Duration = Duration::from_secs(1);

struct Session {
    coordinator: Addr<ClientCoordinatorActor>,
    transport: Option<visionclaw_server::actors::messages::TransportCloser>,
    stopped: Arc<AtomicBool>,
}

impl Actor for Session {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        let mut recipients = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.recipient(),
        )
        .with_stall_timeout(STALL_TIMEOUT);
        if let Some(t) = self.transport.clone() {
            recipients = recipients.with_transport(t);
        }
        self.coordinator.do_send(RegisterClient { recipients });
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Session {
    fn handle(&mut self, _: Result<ws::Message, ws::ProtocolError>, _: &mut Self::Context) {}
}
impl Handler<SendToClientBinary> for Session {
    type Result = ();
    fn handle(&mut self, m: SendToClientBinary, ctx: &mut Self::Context) {
        ctx.binary(m.0);
    }
}
impl Handler<SendToClientText> for Session {
    type Result = ();
    fn handle(&mut self, m: SendToClientText, ctx: &mut Self::Context) {
        ctx.text(m.0);
    }
}
impl Handler<SendInitialGraphLoad> for Session {
    type Result = ();
    fn handle(&mut self, _: SendInitialGraphLoad, _: &mut Self::Context) {}
}
impl Handler<CloseClientSession> for Session {
    type Result = ();
    fn handle(&mut self, _: CloseClientSession, ctx: &mut Self::Context) {
        ctx.close(None);
        ctx.stop();
    }
}

async fn client_count(c: &Addr<ClientCoordinatorActor>) -> usize {
    c.send(GetClientCount).await.unwrap().unwrap()
}

#[actix_rt::test]
async fn a_client_that_never_reads_is_closed_after_the_stall_timeout() {
    let coordinator = ClientCoordinatorActor::new().start();
    let stopped = Arc::new(AtomicBool::new(false));

    let (coord, stop_flag) = (coordinator.clone(), stopped.clone());
    let server = HttpServer::new(move || {
        let (coord, stop_flag) = (coord.clone(), stop_flag.clone());
        App::new().route(
            "/wss",
            web::get().to(move |req: HttpRequest, stream: web::Payload| {
                let (coord, stop_flag) = (coord.clone(), stop_flag.clone());
                async move {
                    let transport = req.conn_data::<ConnTransport>().map(|t| t.closer());
                    ws::start(
                        Session {
                            coordinator: coord,
                            transport,
                            stopped: stop_flag,
                        },
                        &req,
                        stream,
                    )
                }
            }),
        )
    })
    .on_connect(capture_transport)
    .workers(1)
    .bind(("127.0.0.1", 0))
    .unwrap();
    let addr = server.addrs()[0];
    let srv = server.run();
    let handle = srv.handle();
    actix_rt::spawn(srv);
    // Let the server start its worker before the blocking client I/O below
    // (the client's std reads would otherwise block this runtime first).
    actix_rt::time::sleep(Duration::from_millis(300)).await;

    // Handshake, then never read again.
    let mut peer = std::net::TcpStream::connect(addr).unwrap();
    write!(
        peer,
        "GET /wss HTTP/1.1\r\nHost: t\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .unwrap();
    let mut buf = [0u8; 512];
    let n = peer.read(&mut buf).unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 101"));

    let registered_by = Instant::now() + Duration::from_secs(2);
    while client_count(&coordinator).await == 0 {
        assert!(Instant::now() < registered_by, "session never registered");
        actix_rt::time::sleep(Duration::from_millis(20)).await;
    }

    // Stream ~490 KB snapshots at 8 Hz, as the live broadcast does.
    let frame = vec![5u8; 490_000];
    let started = Instant::now();
    let deadline = started + Duration::from_secs(20);
    while !stopped.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "the never-reading client was not closed within 20 s"
        );
        coordinator.do_send(BroadcastNodePositions {
            positions: frame.clone(),
        });
        actix_rt::time::sleep(Duration::from_millis(125)).await;
    }
    let closed_after = started.elapsed();
    eprintln!("never-reading client closed {closed_after:?} after streaming began");
    assert_eq!(
        client_count(&coordinator).await,
        0,
        "coordinator dropped it"
    );

    // The peer sees the connection gone: draining what was buffered ends in
    // EOF or a reset, never in "would block" on a live socket.
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut sink = vec![0u8; 1 << 20];
    let outcome = loop {
        match peer.read(&mut sink) {
            Ok(0) => break "eof",
            Ok(_) => continue,
            Err(e) if e.kind() == ErrorKind::ConnectionReset => break "reset",
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                break "still open"
            }
            Err(_) => break "error",
        }
    };
    assert_ne!(
        outcome, "still open",
        "the server must have closed the socket"
    );

    handle.stop(false).await;
}
