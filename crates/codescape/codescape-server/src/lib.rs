//! Codescape server library: the I/O-light building blocks of the
//! `codescape-server` binary.
//!
//! The binary wires these to the ingest, layout and collaboration crates; this
//! library holds everything that can be reasoned about and tested on its own:
//!
//! * [`config`]: `CODESCAPE_*` configuration and the data directory layout.
//! * [`auth`]: the constant-time ingest token, the request-verifier seam (NIP-98
//!   lands in P6), and a token-bucket rate limiter.
//! * [`cache`]: the bounded per-revision cache behind snapshot and projection reuse.
//! * [`egress`]: the outbound allowlist every client goes through.
//! * [`persist`]: atomic writes, warm positions and actor ids.
//! * [`wire`]: per-client frame selection and `0x34` ack pacing.
//! * [`xr`]: `/wss` emulation for the XR client (`initialGraphLoad`, text
//!   messages, `0x23` mapping).
//! * [`http`]: isolation headers, static bundle with SPA fallback, 404 list.
//! * [`ingest_api`]: the `:4101` presence-ingest app and the presence-text policy.
//! * [`origin`]: the WebSocket `Origin` allowlist.
//! * [`proxy`]: opt-in reverse-proxy trust (forwarded host, origin, client address).
//! * [`xr_presence`]: the `/ws/presence` room hub and `0x43` pose codec for XR avatars.
//! * [`voice`]: nemotron ASR, pocket-tts and Loom adapters.
//!
//! # Example
//!
//! ```
//! use codescape_server::wire::{AckWindow, Admission, FrameSelector, SeqCounter};
//! use codescape_model::NodeRecord;
//!
//! let mut sel = FrameSelector::default();
//! let mut win = AckWindow::default();
//! let mut seq = SeqCounter::default();
//! let records = vec![NodeRecord::plain(1, [0.0, 0.0, 0.0])];
//! assert_eq!(win.admit(0), Admission::Send);
//! let pick = sel.select(&records, 0, usize::MAX);
//! assert!(pick.keyframe);
//! let s = seq.advance();
//! let frame = codescape_model::wire::v5_frame(s, &pick.records);
//! win.on_sent(s, 0);
//! assert_eq!(frame.len(), 1 + 8 + 52);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod cache;
pub mod config;
pub mod egress;
pub mod http;
pub mod ingest_api;
pub mod origin;
pub mod persist;
pub mod proxy;
pub mod voice;
pub mod wire;
pub mod xr;
pub mod xr_presence;

/// Server version reported on `/api/health` and in `Hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Unix time in milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
