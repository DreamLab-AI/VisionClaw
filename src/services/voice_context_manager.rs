//! Voice conversation context management for multi-turn interactions
//!
//! This module manages conversation state and context for voice commands,
//! enabling multi-turn conversations and context-aware responses.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::actors::voice_commands::{ConversationContext, SwarmIntent};
use crate::utils::time;

pub struct VoiceContextManager {
    sessions: Arc<RwLock<HashMap<String, VoiceSession>>>,

    max_session_duration: ChronoDuration,

    max_sessions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSession {
    pub session_id: String,

    pub user_id: Option<String>,

    pub created_at: DateTime<Utc>,

    pub last_activity: DateTime<Utc>,

    pub conversation_history: Vec<(String, String)>,

    pub context: ConversationContext,

    pub metadata: HashMap<String, String>,

    pub active_swarms: Vec<String>,

    pub pending_operations: Vec<PendingOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperation {
    pub operation_id: String,

    pub operation_type: String,

    pub parameters: HashMap<String, String>,

    pub created_at: DateTime<Utc>,

    pub expected_completion: Option<DateTime<Utc>>,

    pub status: OperationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

impl VoiceContextManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_session_duration: ChronoDuration::hours(2),
            max_sessions: 100,
        }
    }

    pub async fn get_or_create_session(
        &self,
        session_id: Option<String>,
        user_id: Option<String>,
    ) -> String {
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(&session_id) {
            session.last_activity = time::now();
            debug!("Retrieved existing voice session: {}", session_id);
            return session_id;
        }

        let session = VoiceSession {
            session_id: session_id.clone(),
            user_id,
            created_at: time::now(),
            last_activity: time::now(),
            conversation_history: Vec::new(),
            context: ConversationContext {
                session_id: session_id.clone(),
                history: Vec::new(),
                current_agents: Vec::new(),
                pending_clarification: None,
                turn_count: 0,
            },
            metadata: HashMap::new(),
            active_swarms: Vec::new(),
            pending_operations: Vec::new(),
        };

        if sessions.len() >= self.max_sessions {
            self.cleanup_old_sessions_internal(&mut sessions).await;
        }

        sessions.insert(session_id.clone(), session);
        info!("Created new voice session: {}", session_id);

        session_id
    }

    /// V3 (PRD-023 WP-10): assign this session's pending clarification — the
    /// previously-dead `ConversationContext.pending_clarification` field, now
    /// populated when the confidence gate holds an utterance for a repair turn
    /// (the WP-10 falsification trigger: "falsified if `pending_clarification`
    /// is still never assigned"). `Some(token)` holds a clarification; `None`
    /// clears it. Creates the session if absent so a clarification can be held
    /// before any completed turn.
    pub async fn set_pending_clarification(&self, session_id: &str, token: Option<String>) {
        // Ensure the session exists (own lock scope), then assign.
        self.get_or_create_session(Some(session_id.to_string()), None)
            .await;
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.context.pending_clarification = token;
            session.last_activity = time::now();
            debug!(
                "[voice-clarification] pending_clarification set for session {}",
                session_id
            );
        }
    }

    /// V3 (PRD-023 WP-10): read **and clear** this session's pending
    /// clarification. `Some(token)` means the next utterance is a repair turn
    /// (merge it); `None` means the utterance takes the normal dispatch path.
    pub async fn take_pending_clarification(&self, session_id: &str) -> Option<String> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)?;
        let token = session.context.pending_clarification.take();
        if token.is_some() {
            session.last_activity = time::now();
        }
        token
    }

    pub async fn add_conversation_turn(
        &self,
        session_id: &str,
        user_input: String,
        assistant_response: String,
        intent: Option<SwarmIntent>,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            session
                .conversation_history
                .push((user_input.clone(), assistant_response.clone()));
            session
                .context
                .history
                .push((user_input, assistant_response));
            session.context.turn_count += 1;
            session.last_activity = time::now();

            if let Some(SwarmIntent::SpawnAgent { agent_type, .. }) = intent {
                session.context.current_agents.push(agent_type);
            }

            debug!(
                "Added conversation turn to session {}: {} turns total",
                session_id, session.context.turn_count
            );
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    pub async fn add_pending_operation(
        &self,
        session_id: &str,
        operation_type: String,
        parameters: HashMap<String, String>,
        expected_completion: Option<DateTime<Utc>>,
    ) -> Result<String, String> {
        let operation_id = Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            let operation = PendingOperation {
                operation_id: operation_id.clone(),
                operation_type,
                parameters,
                created_at: time::now(),
                expected_completion,
                status: OperationStatus::Pending,
            };

            session.pending_operations.push(operation);
            session.last_activity = time::now();

            debug!(
                "Added pending operation {} to session {}",
                operation_id, session_id
            );
            Ok(operation_id)
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    pub async fn update_operation_status(
        &self,
        session_id: &str,
        operation_id: &str,
        status: OperationStatus,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            if let Some(operation) = session
                .pending_operations
                .iter_mut()
                .find(|op| op.operation_id == operation_id)
            {
                operation.status = status;
                session.last_activity = time::now();
                debug!(
                    "Updated operation {} status in session {}",
                    operation_id, session_id
                );
                Ok(())
            } else {
                Err(format!(
                    "Operation {} not found in session {}",
                    operation_id, session_id
                ))
            }
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    pub async fn get_context(&self, session_id: &str) -> Option<ConversationContext> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|session| session.context.clone())
    }

    pub async fn get_session_metadata(&self, session_id: &str) -> Option<HashMap<String, String>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|session| session.metadata.clone())
    }

    pub async fn add_session_metadata(
        &self,
        session_id: &str,
        key: String,
        value: String,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            session.metadata.insert(key, value);
            session.last_activity = time::now();
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    pub async fn get_pending_operations(&self, session_id: &str) -> Vec<PendingOperation> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|session| session.pending_operations.clone())
            .unwrap_or_default()
    }

    pub async fn cleanup_expired_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        self.cleanup_old_sessions_internal(&mut sessions).await;
    }

    async fn cleanup_old_sessions_internal(&self, sessions: &mut HashMap<String, VoiceSession>) {
        let now = time::now();
        let mut expired_sessions = Vec::new();

        for (session_id, session) in sessions.iter() {
            let session_age = now.signed_duration_since(session.last_activity);
            if session_age > self.max_session_duration {
                expired_sessions.push(session_id.clone());
            }
        }

        for session_id in expired_sessions {
            sessions.remove(&session_id);
            info!("Cleaned up expired voice session: {}", session_id);
        }
    }

    pub async fn get_active_session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    pub async fn needs_follow_up(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(session_id) {
            if !session.pending_operations.is_empty() {
                return true;
            }

            if session.context.pending_clarification.is_some() {
                return true;
            }

            if session.context.turn_count > 0 {
                if let Some((_, last_response)) = session.conversation_history.last() {
                    if last_response.ends_with('?') {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub async fn generate_contextual_response(
        &self,
        session_id: &str,
        base_response: &str,
    ) -> String {
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(session_id) {
            let mut response = base_response.to_string();

            if !session.pending_operations.is_empty() {
                let pending_count = session
                    .pending_operations
                    .iter()
                    .filter(|op| {
                        matches!(
                            op.status,
                            OperationStatus::Pending | OperationStatus::InProgress
                        )
                    })
                    .count();

                if pending_count > 0 {
                    response.push_str(&format!(
                        " You have {} operations in progress.",
                        pending_count
                    ));
                }
            }

            if session.context.turn_count > 3 {
                response.push_str(" We've been working together for a while on this.");
            }

            response
        } else {
            base_response.to_string()
        }
    }
}

impl Default for VoiceContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let manager = VoiceContextManager::new();
        let session_id = manager
            .get_or_create_session(None, Some("user123".to_string()))
            .await;

        assert!(!session_id.is_empty());

        let context = manager.get_context(&session_id).await;
        assert!(context.is_some());
        assert_eq!(context.unwrap().turn_count, 0);
    }

    #[tokio::test]
    async fn test_conversation_turns() {
        let manager = VoiceContextManager::new();
        let session_id = manager.get_or_create_session(None, None).await;

        manager
            .add_conversation_turn(
                &session_id,
                "spawn a researcher agent".to_string(),
                "I've spawned a researcher agent for you.".to_string(),
                None,
            )
            .await
            .unwrap();

        let context = manager.get_context(&session_id).await.unwrap();
        assert_eq!(context.turn_count, 1);
        assert_eq!(context.history.len(), 1);
    }

    #[tokio::test]
    async fn test_pending_clarification_is_assigned_and_taken() {
        // V3 (PRD-023 WP-10): proves ConversationContext.pending_clarification is
        // now assigned (the falsification trigger) — set populates the field a
        // context read observes, and take reads-and-clears it.
        let manager = VoiceContextManager::new();
        let session_id = "clar-session".to_string();

        // Nothing pending initially.
        assert!(manager
            .take_pending_clarification(&session_id)
            .await
            .is_none());

        // Set populates the field (creates the session if absent).
        manager
            .set_pending_clarification(
                &session_id,
                Some("V3\u{1}agent_type\u{1}spawn a".to_string()),
            )
            .await;
        let ctx = manager
            .get_context(&session_id)
            .await
            .expect("session exists");
        assert_eq!(
            ctx.pending_clarification.as_deref(),
            Some("V3\u{1}agent_type\u{1}spawn a")
        );
        // needs_follow_up now observes the pending clarification (line ~321).
        assert!(manager.needs_follow_up(&session_id).await);

        // Take reads and clears it — the next call sees nothing pending.
        let taken = manager.take_pending_clarification(&session_id).await;
        assert_eq!(taken.as_deref(), Some("V3\u{1}agent_type\u{1}spawn a"));
        assert!(manager
            .take_pending_clarification(&session_id)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_pending_operations() {
        let manager = VoiceContextManager::new();
        let session_id = manager.get_or_create_session(None, None).await;

        let mut params = HashMap::new();
        params.insert("agent_type".to_string(), "researcher".to_string());

        let operation_id = manager
            .add_pending_operation(&session_id, "spawn_agent".to_string(), params, None)
            .await
            .unwrap();

        let operations = manager.get_pending_operations(&session_id).await;
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_id, operation_id);
    }
}
