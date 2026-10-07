//! Hexagonal port traits — transport-agnostic abstractions injected by each
//! consumer (server uses Nostr + Actix bus, client uses gdext + tokio channels,
//! tests use mocks).

use crate::error::RoomError;
use crate::types::{Did, RoomId};

/// Bytes signed by the client during the NIP-98 challenge handshake. Server
/// generates a 32-byte nonce and timestamp; client returns the schnorr signature
/// over `(nonce || ts)` per `xr-godot-threat-model.md` T-WS-1 mitigation.
#[derive(Debug, Clone)]
pub struct SignedChallenge {
    pub nonce: [u8; 32],
    pub timestamp_us: u64,
    pub claimed_pubkey_hex: String,
    pub signature_hex: String,
}

pub trait IdentityVerifier: Send + Sync {
    fn verify_signed_challenge(&self, challenge: &SignedChallenge) -> Result<Did, RoomError>;
}

pub trait Broadcaster: Send + Sync {
    fn broadcast(&self, room: &RoomId, frame: &[u8]);
}

#[cfg(test)]
pub mod test_doubles {
    use super::*;
    use std::sync::Mutex;

    pub struct MockIdentityVerifier {
        pub answer: Result<Did, RoomError>,
    }

    impl IdentityVerifier for MockIdentityVerifier {
        fn verify_signed_challenge(&self, _c: &SignedChallenge) -> Result<Did, RoomError> {
            self.answer.clone()
        }
    }

    pub struct ChannelBroadcaster {
        pub frames: Mutex<Vec<(String, Vec<u8>)>>,
    }

    impl ChannelBroadcaster {
        pub fn new() -> Self {
            Self {
                frames: Mutex::new(Vec::new()),
            }
        }
    }

    impl Default for ChannelBroadcaster {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Broadcaster for ChannelBroadcaster {
        fn broadcast(&self, room: &RoomId, frame: &[u8]) {
            self.frames
                .lock()
                .expect("test mutex poisoned")
                .push((room.as_str().to_owned(), frame.to_vec()));
        }
    }
}
