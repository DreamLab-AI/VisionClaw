//! Close a `/wss` connection from outside its session actor.
//!
//! When a peer stops reading, actix's HTTP dispatcher blocks on the socket
//! write and stops polling the response body, and the `SocketFlowServer`
//! actor lives inside that body. The actor then does not run at all: its
//! heartbeat timer does not fire and no message sent to it is handled, not
//! even one queued past the full mailbox with `do_send`. The connection would
//! stay open for as long as the peer's TCP stack keeps acknowledging.
//!
//! [`capture_transport`] runs in `HttpServer::on_connect` and keeps a dup of
//! the connection's fd in the connection data. [`ConnTransport::closer`] turns
//! it into a [`TransportCloser`] the client coordinator can call: `shutdown(2)`
//! acts on the socket, not the fd, so the dispatcher's next I/O on the real fd
//! fails, the connection is dropped and the actor is stopped (its `stopped()`
//! unregisters it).

use std::any::Any;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;

use actix_web::dev::Extensions;
use log::{debug, warn};

use crate::actors::messages::TransportCloser;

/// A dup of one connection's socket fd, stored in its connection data.
#[derive(Clone)]
pub struct ConnTransport(Arc<OwnedFd>);

impl ConnTransport {
    /// A closer that shuts the connection's socket down (both directions).
    pub fn closer(&self) -> TransportCloser {
        let fd = self.0.clone();
        TransportCloser::new(move || match fd.try_clone() {
            Ok(dup) => {
                let socket = std::net::TcpStream::from(dup);
                if let Err(e) = socket.shutdown(std::net::Shutdown::Both) {
                    debug!("[WebSocket] transport shutdown: {e} (already closed?)");
                }
            }
            Err(e) => warn!("[WebSocket] could not dup the connection fd to close it: {e}"),
        })
    }
}

/// `HttpServer::on_connect` hook: remember a dup of a plain-TCP connection's
/// fd. Other transports (TLS, Unix sockets) are left without one; their
/// stalled clients are still evicted, but only closed when the session runs.
pub fn capture_transport(conn: &dyn Any, ext: &mut Extensions) {
    if let Some(stream) = conn.downcast_ref::<actix_web::rt::net::TcpStream>() {
        match stream.as_fd().try_clone_to_owned() {
            Ok(fd) => {
                ext.insert(ConnTransport(Arc::new(fd)));
            }
            Err(e) => warn!("[WebSocket] could not dup a new connection's fd: {e}"),
        }
    }
}
