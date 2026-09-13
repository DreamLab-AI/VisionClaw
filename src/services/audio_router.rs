//! Audio Router — User-scoped voice channel multiplexer
//!
//! Routes audio between four planes:
//!   Plane 1: User mic → Turbo Whisper STT → agent commands (private per-user)
//!   Plane 2: Agent response → PocketTts TTS → owner's ears (private per-user)
//!   Plane 3: User mic → LiveKit SFU → all users (public spatial voice chat)
//!   Plane 4: Agent TTS → LiveKit SFU at agent position → all users (public spatial)
//!
//! Each user gets an isolated session with their own broadcast channels.
//! Push-to-talk (PTT) controls whether mic audio goes to Plane 1 (agent commands)
//! or Plane 3 (voice chat). When PTT is held, audio routes to STT for agent control.
//! When PTT is released, audio routes to LiveKit for spatial voice chat.

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Per-user voice session with isolated audio channels
#[derive(Debug)]
pub struct UserVoiceSession {
    pub user_id: String,
    /// Private channel: TTS audio meant only for this user
    pub private_audio_tx: broadcast::Sender<Vec<u8>>,
    /// Private channel: transcription results for this user
    pub transcription_tx: broadcast::Sender<String>,
    /// Agent IDs owned by this user
    pub owned_agents: Vec<String>,
    /// Whether the user is currently in PTT (push-to-talk) mode
    pub ptt_active: bool,
    /// COM-15 / D6: the selected agent this PTT session is bound to. When
    /// `Some(did:nostr)`, a spoken command dispatches a signed 31402 targeted at
    /// that agent (`VoiceIntentClient`); when `None`, PTT is unbound and a
    /// command falls back to the global settings-assistant path. This is the
    /// selected-agent binding the register found missing — PTT is no longer
    /// globally scoped (PRD-023 WP-5 AC1, DDD `VoicePttSession` boundary).
    pub selected_agent_did: Option<String>,
    /// LiveKit participant ID for spatial audio
    pub livekit_participant_id: Option<String>,
    /// User's 3D position in the Vircadia world (for spatial audio)
    pub spatial_position: [f32; 3],
}

/// Agent voice identity — each agent has a distinct voice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVoiceIdentity {
    pub agent_id: String,
    pub agent_type: String,
    pub owner_user_id: String,
    /// PocketTts voice preset ID (e.g., "alba", "am_adam")
    pub voice_id: String,
    /// Speech speed multiplier
    pub speed: f32,
    /// Agent's 3D position in Vircadia world
    pub position: [f32; 3],
    /// Whether voice is public (all users hear spatially) or private (owner only)
    pub public_voice: bool,
}

/// Audio Router: manages per-user sessions and agent voice routing
pub struct AudioRouter {
    /// Active user sessions keyed by user_id
    sessions: Arc<RwLock<HashMap<String, UserVoiceSession>>>,
    /// Agent voice identities keyed by agent_id
    agent_voices: Arc<RwLock<HashMap<String, AgentVoiceIdentity>>>,
    /// Default agent voice presets by agent_type
    default_voice_presets: Arc<RwLock<HashMap<String, VoicePreset>>>,
    /// Global audio broadcast for legacy compatibility (non-user-scoped clients)
    global_audio_tx: broadcast::Sender<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicePreset {
    pub voice_id: String,
    pub speed: f32,
}

/// Default voice presets for different agent types
fn default_agent_voice_presets() -> HashMap<String, VoicePreset> {
    let mut presets = HashMap::new();
    presets.insert(
        "researcher".to_string(),
        VoicePreset {
            voice_id: "alba".to_string(),
            speed: 1.0,
        },
    );
    presets.insert(
        "coder".to_string(),
        VoicePreset {
            voice_id: "am_adam".to_string(),
            speed: 1.1,
        },
    );
    presets.insert(
        "analyst".to_string(),
        VoicePreset {
            voice_id: "bf_emma".to_string(),
            speed: 1.0,
        },
    );
    presets.insert(
        "optimizer".to_string(),
        VoicePreset {
            voice_id: "am_michael".to_string(),
            speed: 0.95,
        },
    );
    presets.insert(
        "coordinator".to_string(),
        VoicePreset {
            voice_id: "alba".to_string(),
            speed: 1.0,
        },
    );
    presets
}

impl AudioRouter {
    pub fn new() -> Self {
        let (global_audio_tx, _) = broadcast::channel(100);

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            agent_voices: Arc::new(RwLock::new(HashMap::new())),
            default_voice_presets: Arc::new(RwLock::new(default_agent_voice_presets())),
            global_audio_tx,
        }
    }

    /// Register a new user voice session
    pub async fn register_user(
        &self,
        user_id: &str,
    ) -> (broadcast::Receiver<Vec<u8>>, broadcast::Receiver<String>) {
        let mut sessions = self.sessions.write().await;

        if let Some(existing) = sessions.get(user_id) {
            info!(
                "User {} already registered, returning existing channels",
                user_id
            );
            return (
                existing.private_audio_tx.subscribe(),
                existing.transcription_tx.subscribe(),
            );
        }

        let (audio_tx, audio_rx) = broadcast::channel(100);
        let (transcription_tx, transcription_rx) = broadcast::channel(100);

        let session = UserVoiceSession {
            user_id: user_id.to_string(),
            private_audio_tx: audio_tx,
            transcription_tx,
            owned_agents: Vec::new(),
            ptt_active: false,
            selected_agent_did: None,
            livekit_participant_id: None,
            spatial_position: [0.0, 0.0, 0.0],
        };

        sessions.insert(user_id.to_string(), session);
        info!("Registered voice session for user {}", user_id);

        (audio_rx, transcription_rx)
    }

    /// Unregister a user voice session
    pub async fn unregister_user(&self, user_id: &str) {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(user_id).is_some() {
            info!("Unregistered voice session for user {}", user_id);
        }

        // Clean up any agents owned by this user
        let mut agents = self.agent_voices.write().await;
        agents.retain(|_, v| v.owner_user_id != user_id);
    }

    /// Set PTT (push-to-talk) state for a user, leaving the selected-agent
    /// binding untouched.
    pub async fn set_ptt(&self, user_id: &str, active: bool) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(user_id) {
            session.ptt_active = active;
            debug!(
                "User {} PTT: {}",
                user_id,
                if active { "ACTIVE" } else { "RELEASED" }
            );
        }
    }

    /// COM-15 / D6: set PTT state AND the selected-agent binding in one message,
    /// the way the PTT-start message threads a graph selection into the session.
    /// A `selected_agent_did` of `Some(x)` is stored only if `x` is a canonical
    /// `did:nostr` (verify before trust, DDD invariant 2); a non-DID clears the
    /// binding and warns rather than binding to a spoofable label. `None` leaves
    /// the existing binding in place (a bare PTT toggle keeps the target).
    pub async fn set_ptt_with_target(
        &self,
        user_id: &str,
        active: bool,
        selected_agent_did: Option<String>,
    ) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(user_id) {
            session.ptt_active = active;
            if let Some(claim) = selected_agent_did {
                session.selected_agent_did = validate_target_did(&claim, user_id);
            }
            debug!(
                "User {} PTT: {} (bound → {:?})",
                user_id,
                if active { "ACTIVE" } else { "RELEASED" },
                session.selected_agent_did
            );
        }
    }

    /// COM-15 / D6: bind (or clear) the selected agent for a user's PTT session
    /// without changing the PTT toggle. `None` clears the binding; a non-DID
    /// claim is refused (binding cleared) — a hashed label is never a governed
    /// target (ADR-037 D7).
    pub async fn bind_selected_agent(&self, user_id: &str, selected_agent_did: Option<String>) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(user_id) {
            session.selected_agent_did = match selected_agent_did {
                Some(claim) => validate_target_did(&claim, user_id),
                None => None,
            };
        }
    }

    /// The `did:nostr` the user's PTT session is currently bound to, if any.
    pub async fn selected_agent_did(&self, user_id: &str) -> Option<String> {
        let sessions = self.sessions.read().await;
        sessions
            .get(user_id)
            .and_then(|s| s.selected_agent_did.clone())
    }

    /// Check if a user's PTT is active
    pub async fn is_ptt_active(&self, user_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.get(user_id).map(|s| s.ptt_active).unwrap_or(false)
    }

    /// Register an agent with a voice identity
    pub async fn register_agent(
        &self,
        agent_id: &str,
        agent_type: &str,
        owner_user_id: &str,
        position: [f32; 3],
        public_voice: bool,
    ) {
        let presets = self.default_voice_presets.read().await;
        let preset = presets.get(agent_type).cloned().unwrap_or(VoicePreset {
            voice_id: "alba".to_string(),
            speed: 1.0,
        });

        let identity = AgentVoiceIdentity {
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            owner_user_id: owner_user_id.to_string(),
            voice_id: preset.voice_id,
            speed: preset.speed,
            position,
            public_voice,
        };

        // Add agent to owner's session
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(owner_user_id) {
                if !session.owned_agents.contains(&agent_id.to_string()) {
                    session.owned_agents.push(agent_id.to_string());
                }
            }
        }

        self.agent_voices
            .write()
            .await
            .insert(agent_id.to_string(), identity);
        info!(
            "Registered agent {} (type={}) for user {}",
            agent_id, agent_type, owner_user_id
        );
    }

    /// Update an agent's spatial position
    pub async fn update_agent_position(&self, agent_id: &str, position: [f32; 3]) {
        let mut agents = self.agent_voices.write().await;
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.position = position;
        }
    }

    /// Get voice identity for an agent (used to select PocketTts voice preset for TTS)
    pub async fn get_agent_voice(&self, agent_id: &str) -> Option<AgentVoiceIdentity> {
        self.agent_voices.read().await.get(agent_id).cloned()
    }

    /// Route TTS audio to the correct destination based on agent ownership and publicity
    pub async fn route_agent_audio(
        &self,
        agent_id: &str,
        audio_data: Vec<u8>,
    ) -> Result<(), String> {
        let agents = self.agent_voices.read().await;
        let agent = agents
            .get(agent_id)
            .ok_or_else(|| format!("Unknown agent: {}", agent_id))?;

        if agent.public_voice {
            // Plane 4: spatial audio — send to global broadcast AND private channel
            // (LiveKit injection happens at the client/bridge layer)
            let _ = self.global_audio_tx.send(audio_data.clone());

            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&agent.owner_user_id) {
                let _ = session.private_audio_tx.send(audio_data);
            }
            debug!("Routed public spatial audio for agent {}", agent_id);
        } else {
            // Plane 2: private response — only to owner
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&agent.owner_user_id) {
                session.private_audio_tx.send(audio_data).map_err(|e| {
                    format!(
                        "Failed to send private audio to user {}: {}",
                        agent.owner_user_id, e
                    )
                })?;
                debug!(
                    "Routed private audio for agent {} to user {}",
                    agent_id, agent.owner_user_id
                );
            } else {
                warn!(
                    "No session for agent {} owner {}",
                    agent_id, agent.owner_user_id
                );
            }
        }

        Ok(())
    }

    /// Route transcription text to the correct user
    pub async fn route_transcription(&self, user_id: &str, text: String) -> Result<(), String> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(user_id) {
            session
                .transcription_tx
                .send(text)
                .map_err(|e| format!("Failed to send transcription to user {}: {}", user_id, e))?;
        } else {
            warn!("No session for user {} — transcription dropped", user_id);
        }
        Ok(())
    }

    /// Get a subscriber for a specific user's private audio channel
    pub async fn subscribe_user_audio(
        &self,
        user_id: &str,
    ) -> Option<broadcast::Receiver<Vec<u8>>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(user_id)
            .map(|s| s.private_audio_tx.subscribe())
    }

    /// Get a subscriber for a specific user's transcription channel
    pub async fn subscribe_user_transcriptions(
        &self,
        user_id: &str,
    ) -> Option<broadcast::Receiver<String>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(user_id)
            .map(|s| s.transcription_tx.subscribe())
    }

    /// Get the global audio broadcast for legacy (non-user-scoped) clients
    pub fn subscribe_global_audio(&self) -> broadcast::Receiver<Vec<u8>> {
        self.global_audio_tx.subscribe()
    }

    /// Get all agents owned by a user
    pub async fn get_user_agents(&self, user_id: &str) -> Vec<AgentVoiceIdentity> {
        let sessions = self.sessions.read().await;
        let agent_ids = match sessions.get(user_id) {
            Some(session) => session.owned_agents.clone(),
            None => return Vec::new(),
        };
        drop(sessions);

        let agents = self.agent_voices.read().await;
        agent_ids
            .iter()
            .filter_map(|id| agents.get(id).cloned())
            .collect()
    }

    /// Update user's spatial position (for Vircadia presence sync)
    pub async fn update_user_position(&self, user_id: &str, position: [f32; 3]) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(user_id) {
            session.spatial_position = position;
        }
    }

    /// Set a user's LiveKit participant ID
    pub async fn set_livekit_participant(&self, user_id: &str, participant_id: String) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(user_id) {
            session.livekit_participant_id = Some(participant_id);
        }
    }

    /// Get summary of active voice sessions (for monitoring)
    pub async fn get_status(&self) -> AudioRouterStatus {
        let sessions = self.sessions.read().await;
        let agents = self.agent_voices.read().await;

        AudioRouterStatus {
            active_users: sessions.len(),
            active_agents: agents.len(),
            users_with_ptt: sessions.values().filter(|s| s.ptt_active).count(),
            spatial_agents: agents.values().filter(|a| a.public_voice).count(),
            ptt_bound_to_agent: sessions
                .values()
                .filter(|s| s.selected_agent_did.is_some())
                .count(),
        }
    }
}

/// Accept `claim` as a PTT target only if it is a canonical `did:nostr`
/// (ADR-125 I1). A non-DID is refused (returns `None`, warn-logged) so a
/// spoofable label never becomes the target of a governed voice command
/// (verify before trust, DDD invariant 2).
fn validate_target_did(claim: &str, user_id: &str) -> Option<String> {
    if matches!(
        crate::uri::parse(claim),
        Ok(crate::uri::ParsedUri::DidNostr { .. })
    ) {
        Some(claim.to_string())
    } else {
        warn!("User {user_id} PTT target '{claim}' is not a did:nostr — binding refused");
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRouterStatus {
    pub active_users: usize,
    pub active_agents: usize,
    pub users_with_ptt: usize,
    pub spatial_agents: usize,
    /// COM-15 / D6: sessions whose PTT is bound to a selected agent's
    /// `did:nostr` (proof PTT is no longer globally scoped).
    pub ptt_bound_to_agent: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const DID_A: &str =
        "did:nostr:1111111111111111111111111111111111111111111111111111111111111111";
    const DID_B: &str =
        "did:nostr:2222222222222222222222222222222222222222222222222222222222222222";

    /// COM-15 / D6 AC1: the PTT-start message binds the selected agent's
    /// did:nostr onto the session; a spoken command now has a verifiable target.
    #[tokio::test]
    async fn ptt_binds_the_selected_agent_did() {
        let router = AudioRouter::new();
        let _ = router.register_user("alice").await;

        router
            .set_ptt_with_target("alice", true, Some(DID_A.to_string()))
            .await;

        assert!(router.is_ptt_active("alice").await);
        assert_eq!(
            router.selected_agent_did("alice").await.as_deref(),
            Some(DID_A)
        );
    }

    /// The register's open finding — PTT globally scoped — is refuted: two users
    /// carry two distinct bindings with no crosstalk. Alice's target is Alice's,
    /// not a process-global toggle.
    #[tokio::test]
    async fn ptt_is_not_globally_scoped() {
        let router = AudioRouter::new();
        let _ = router.register_user("alice").await;
        let _ = router.register_user("bob").await;

        router
            .bind_selected_agent("alice", Some(DID_A.to_string()))
            .await;
        router
            .bind_selected_agent("bob", Some(DID_B.to_string()))
            .await;

        assert_eq!(
            router.selected_agent_did("alice").await.as_deref(),
            Some(DID_A)
        );
        assert_eq!(
            router.selected_agent_did("bob").await.as_deref(),
            Some(DID_B)
        );

        let status = router.get_status().await;
        assert_eq!(
            status.ptt_bound_to_agent, 2,
            "both sessions bound to an agent"
        );
    }

    /// Verify before trust: a hashed nickname / free-text label is refused as a
    /// PTT target, so a governed command can never be addressed at a spoofable
    /// label (ADR-037 D7, DDD invariant 2).
    #[tokio::test]
    async fn non_did_target_is_refused() {
        let router = AudioRouter::new();
        let _ = router.register_user("alice").await;

        router
            .set_ptt_with_target("alice", true, Some("researcher-7".to_string()))
            .await;

        assert!(
            router.is_ptt_active("alice").await,
            "PTT toggles regardless"
        );
        assert_eq!(
            router.selected_agent_did("alice").await,
            None,
            "a non-did target must not bind"
        );
    }

    /// Clearing the binding (deselect) unbinds the target while leaving PTT
    /// alone; a bare toggle with `None` target keeps an existing binding.
    #[tokio::test]
    async fn binding_clears_on_deselect_and_survives_bare_toggle() {
        let router = AudioRouter::new();
        let _ = router.register_user("alice").await;

        router
            .bind_selected_agent("alice", Some(DID_A.to_string()))
            .await;
        // Bare toggle (None target) preserves the binding.
        router.set_ptt_with_target("alice", false, None).await;
        assert_eq!(
            router.selected_agent_did("alice").await.as_deref(),
            Some(DID_A)
        );

        // Deselect clears it.
        router.bind_selected_agent("alice", None).await;
        assert_eq!(router.selected_agent_did("alice").await, None);
    }
}
