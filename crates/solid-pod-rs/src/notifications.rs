//! Solid Notifications Protocol — Phase 2 scope.
//!
//! This module ships the trait signature and a simple in-memory
//! subscription registry. The full Notifications Protocol (WebSocket
//! subscriptions, webhook delivery, subscription discovery) is the
//! Phase 2 deliverable for solid-pod-rs.
//!
//! See: <https://solid.github.io/notifications/protocol/>

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::PodError;

/// Notification channel types supported by Solid Notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ChannelType {
    WebSocketChannel2023,
    WebhookChannel2023,
}

/// A subscription record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub topic: String,
    pub channel_type: ChannelType,
    /// For webhooks: the callback URL. For websockets: the assigned
    /// channel URL.
    pub receive_from: String,
}

/// Describes a resource change (created/updated/deleted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeNotification {
    #[serde(rename = "type")]
    pub kind: String,
    pub object: String,
    pub published: String,
}

/// Phase 2 contract. Phase 1 ships `InMemoryNotifications` as a
/// concrete implementation suitable for tests.
#[async_trait]
pub trait Notifications: Send + Sync {
    /// Register a subscription for a topic.
    ///
    /// TODO(phase-2): Negotiate the channel type via Solid
    /// subscription discovery, return a proper `receive_from` URL,
    /// and persist subscriptions across restarts.
    async fn subscribe(&self, subscription: Subscription) -> Result<(), PodError>;

    /// Remove a subscription.
    async fn unsubscribe(&self, id: &str) -> Result<(), PodError>;

    /// Deliver a notification to all subscribers of `topic`.
    ///
    /// TODO(phase-2): HTTP POST delivery for webhooks; WebSocket
    /// frame delivery for WebSocketChannel2023; retry + dead-letter
    /// handling; delivery metrics.
    async fn publish(
        &self,
        topic: &str,
        notification: ChangeNotification,
    ) -> Result<(), PodError>;
}

/// In-memory implementation suitable for tests and for the Phase 1
/// example server. Retained subscriptions are discarded on drop.
#[derive(Default, Clone)]
pub struct InMemoryNotifications {
    inner: Arc<RwLock<HashMap<String, Vec<Subscription>>>>,
}

impl InMemoryNotifications {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Notifications for InMemoryNotifications {
    async fn subscribe(&self, subscription: Subscription) -> Result<(), PodError> {
        let mut guard = self.inner.write().await;
        guard
            .entry(subscription.topic.clone())
            .or_default()
            .push(subscription);
        Ok(())
    }

    async fn unsubscribe(&self, id: &str) -> Result<(), PodError> {
        let mut guard = self.inner.write().await;
        for subs in guard.values_mut() {
            subs.retain(|s| s.id != id);
        }
        Ok(())
    }

    async fn publish(
        &self,
        topic: &str,
        _notification: ChangeNotification,
    ) -> Result<(), PodError> {
        // TODO(phase-2): actually deliver. For Phase 1 we simply
        // observe that the topic exists.
        let guard = self.inner.read().await;
        let _ = guard.get(topic);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_unsubscribe_roundtrip() {
        let n = InMemoryNotifications::new();
        let sub = Subscription {
            id: "sub-1".into(),
            topic: "/public/".into(),
            channel_type: ChannelType::WebhookChannel2023,
            receive_from: "https://example.com/hook".into(),
        };
        n.subscribe(sub.clone()).await.unwrap();
        n.unsubscribe("sub-1").await.unwrap();
        n.publish(
            "/public/",
            ChangeNotification {
                kind: "Update".into(),
                object: "/public/x".into(),
                published: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await
        .unwrap();
    }
}
