//! Spatial voice routing. Owns the authoritative in-memory map of per-avatar
//! voice-track positions and the listener transform, and answers the spatial
//! queries scene-side code drives each frame. The livekit-android AAR media
//! transport (PRD-008 §5.5) consumes this state through these methods; this
//! module owns the routing maths and lifecycle regardless of the media backend.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;
#[cfg(not(test))]
use tracing::info;

#[cfg(not(test))]
use godot::prelude::*;

#[derive(Debug, Clone, Error)]
pub enum VoiceError {
    #[error("track for did {did} not attached")]
    UnknownTrack { did: String },
    #[error("voice router state lock poisoned")]
    Lock,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListenerTransform {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTrackState {
    pub did: String,
    pub position: [f32; 3],
    pub muted: bool,
}

pub struct SpatialVoiceRouterCore {
    tracks: Mutex<HashMap<String, VoiceTrackState>>,
    listener: Mutex<ListenerTransform>,
}

impl SpatialVoiceRouterCore {
    pub fn new() -> Self {
        Self {
            tracks: Mutex::new(HashMap::new()),
            listener: Mutex::new(ListenerTransform {
                position: [0.0; 3],
                forward: [0.0, 0.0, -1.0],
                up: [0.0, 1.0, 0.0],
            }),
        }
    }

    pub fn attach_track(&self, did: String, position: [f32; 3]) -> Result<(), VoiceError> {
        let mut tracks = self.tracks.lock().map_err(|_| VoiceError::Lock)?;
        tracks.insert(
            did.clone(),
            VoiceTrackState {
                did,
                position,
                muted: false,
            },
        );
        Ok(())
    }

    pub fn detach_track(&self, did: &str) -> Result<(), VoiceError> {
        let mut tracks = self.tracks.lock().map_err(|_| VoiceError::Lock)?;
        tracks.remove(did).ok_or_else(|| VoiceError::UnknownTrack {
            did: did.to_owned(),
        })?;
        Ok(())
    }

    pub fn update_track_position(&self, did: &str, position: [f32; 3]) -> Result<(), VoiceError> {
        let mut tracks = self.tracks.lock().map_err(|_| VoiceError::Lock)?;
        let track = tracks
            .get_mut(did)
            .ok_or_else(|| VoiceError::UnknownTrack {
                did: did.to_owned(),
            })?;
        track.position = position;
        Ok(())
    }

    pub fn set_track_muted(&self, did: &str, muted: bool) -> Result<(), VoiceError> {
        let mut tracks = self.tracks.lock().map_err(|_| VoiceError::Lock)?;
        let track = tracks
            .get_mut(did)
            .ok_or_else(|| VoiceError::UnknownTrack {
                did: did.to_owned(),
            })?;
        track.muted = muted;
        Ok(())
    }

    pub fn update_listener(&self, t: ListenerTransform) -> Result<(), VoiceError> {
        let mut listener = self.listener.lock().map_err(|_| VoiceError::Lock)?;
        *listener = t;
        Ok(())
    }

    pub fn track_count(&self) -> usize {
        self.tracks.lock().map(|t| t.len()).unwrap_or(0)
    }

    pub fn snapshot(&self) -> Vec<VoiceTrackState> {
        match self.tracks.lock() {
            Ok(t) => t.values().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn listener_snapshot(&self) -> ListenerTransform {
        self.listener
            .lock()
            .map(|l| *l)
            .unwrap_or(ListenerTransform {
                position: [0.0; 3],
                forward: [0.0, 0.0, -1.0],
                up: [0.0, 1.0, 0.0],
            })
    }
}

impl Default for SpatialVoiceRouterCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct SpatialVoiceRouter {
    core: SpatialVoiceRouterCore,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl SpatialVoiceRouter {
    #[signal]
    fn voice_activity(did: GString, active: bool);

    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            core: SpatialVoiceRouterCore::new(),
            base,
        })
    }

    #[func]
    fn attach_track(&mut self, did: GString, position: Vector3) -> bool {
        match self
            .core
            .attach_track(did.to_string(), [position.x, position.y, position.z])
        {
            Ok(()) => {
                info!(did = %did, "voice track attached");
                true
            }
            Err(_) => false,
        }
    }

    #[func]
    fn detach_track(&mut self, did: GString) -> bool {
        self.core.detach_track(&did.to_string()).is_ok()
    }

    #[func]
    fn update_track_position(&mut self, did: GString, position: Vector3) -> bool {
        self.core
            .update_track_position(&did.to_string(), [position.x, position.y, position.z])
            .is_ok()
    }

    #[func]
    fn set_track_muted(&mut self, did: GString, muted: bool) -> bool {
        self.core.set_track_muted(&did.to_string(), muted).is_ok()
    }

    #[func]
    fn track_count(&self) -> i64 {
        self.core.track_count() as i64
    }

    #[func]
    fn update_listener(&mut self, position: Vector3, forward: Vector3, up: Vector3) -> bool {
        self.core
            .update_listener(ListenerTransform {
                position: [position.x, position.y, position.z],
                forward: [forward.x, forward.y, forward.z],
                up: [up.x, up.y, up.z],
            })
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn attach_increments_count() {
        let r = SpatialVoiceRouterCore::new();
        assert_eq!(r.track_count(), 0);
        r.attach_track("did:nostr:aaa".into(), [1.0, 2.0, 3.0])
            .unwrap();
        assert_eq!(r.track_count(), 1);
    }

    #[test]
    fn detach_removes_track() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [0.0; 3]).unwrap();
        r.detach_track("did:nostr:aaa").unwrap();
        assert_eq!(r.track_count(), 0);
    }

    #[test]
    fn detach_unknown_errors() {
        let r = SpatialVoiceRouterCore::new();
        let err = r.detach_track("did:nostr:nope").unwrap_err();
        assert!(matches!(err, VoiceError::UnknownTrack { .. }));
    }

    #[test]
    fn update_position_changes_snapshot() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [0.0; 3]).unwrap();
        r.update_track_position("did:nostr:aaa", [9.0, 8.0, 7.0])
            .unwrap();
        let snap = r.snapshot();
        assert_eq!(snap[0].position, [9.0, 8.0, 7.0]);
    }

    #[test]
    fn mute_toggle_persists() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [0.0; 3]).unwrap();
        r.set_track_muted("did:nostr:aaa", true).unwrap();
        assert!(r.snapshot()[0].muted);
    }

    #[test]
    fn update_listener_changes_state() {
        let r = SpatialVoiceRouterCore::new();
        let t = ListenerTransform {
            position: [5.0, 10.0, 15.0],
            forward: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
        };
        r.update_listener(t).unwrap();
        let snap = r.listener_snapshot();
        assert_eq!(snap.position, [5.0, 10.0, 15.0]);
        assert_eq!(snap.forward, [1.0, 0.0, 0.0]);
        assert_eq!(snap.up, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn snapshot_returns_all_tracks() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [1.0, 0.0, 0.0])
            .unwrap();
        r.attach_track("did:nostr:bbb".into(), [0.0, 1.0, 0.0])
            .unwrap();
        r.attach_track("did:nostr:ccc".into(), [0.0, 0.0, 1.0])
            .unwrap();
        let snap = r.snapshot();
        assert_eq!(snap.len(), 3);
    }

    #[test]
    fn snapshot_after_detach() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [0.0; 3]).unwrap();
        r.attach_track("did:nostr:bbb".into(), [1.0; 3]).unwrap();
        r.detach_track("did:nostr:aaa").unwrap();
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].did, "did:nostr:bbb");
    }

    #[test]
    fn default_trait_matches_new() {
        let r = SpatialVoiceRouterCore::default();
        assert_eq!(r.track_count(), 0);
    }

    #[test]
    fn concurrent_attach_from_two_threads() {
        let r = Arc::new(SpatialVoiceRouterCore::new());
        let mut handles = Vec::new();
        for batch in 0..2 {
            let r = Arc::clone(&r);
            handles.push(std::thread::spawn(move || {
                for i in 0..10 {
                    let did = format!("did:nostr:t{}n{}", batch, i);
                    r.attach_track(did, [batch as f32, i as f32, 0.0]).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(r.track_count(), 20);
    }

    #[test]
    fn update_position_unknown_track_errors() {
        let r = SpatialVoiceRouterCore::new();
        let err = r
            .update_track_position("did:nostr:ghost", [1.0, 2.0, 3.0])
            .unwrap_err();
        assert!(matches!(err, VoiceError::UnknownTrack { .. }));
    }

    #[test]
    fn set_muted_unknown_track_errors() {
        let r = SpatialVoiceRouterCore::new();
        let err = r.set_track_muted("did:nostr:ghost", true).unwrap_err();
        assert!(matches!(err, VoiceError::UnknownTrack { .. }));
    }

    #[test]
    fn attach_same_did_twice_overwrites() {
        let r = SpatialVoiceRouterCore::new();
        r.attach_track("did:nostr:aaa".into(), [0.0, 0.0, 0.0])
            .unwrap();
        r.attach_track("did:nostr:aaa".into(), [1.0, 1.0, 1.0])
            .unwrap();
        assert_eq!(r.track_count(), 1);
        let snap = r.snapshot();
        assert_eq!(snap[0].position, [1.0, 1.0, 1.0]);
    }
}
