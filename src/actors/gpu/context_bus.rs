//! GPU Context Event Bus for broadcast-based distribution
//!
//! This module provides an event-based mechanism for distributing SharedGPUContext
//! to independent GPU subsystem actors, eliminating the single point of failure
//! that GPUManagerActor previously represented.
//!
//! ## Architecture
//! ```text
//! GPUContextBus (Publisher)
//!       │
//!       ├──► PhysicsSubsystem (Subscriber)
//!       │       ├── ForceComputeActor
//!       │       ├── StressMajorizationActor
//!       │       └── ConstraintActor
//!       │
//!       ├──► AnalyticsSubsystem (Subscriber)
//!       │       ├── ClusteringActor
//!       │       ├── AnomalyDetectionActor
//!       │       └── PageRankActor
//!       │
//!       └──► GraphSubsystem (Subscriber)
//!               ├── ShortestPathActor
//!               └── ConnectedComponentsActor
//! ```

use std::sync::Arc;
use tokio::sync::broadcast;

use super::shared::SharedGPUContext;

/// Event broadcast when GPU context becomes available
#[derive(Clone)]
pub struct GPUContextReady {
    /// The shared GPU context for all actors
    pub context: Arc<SharedGPUContext>,
    /// Timestamp when context was created
    pub created_at: std::time::Instant,
    /// Device ordinal (for multi-GPU systems)
    pub device_ordinal: u32,
}

impl GPUContextReady {
    /// Create a new GPUContextReady event
    pub fn new(context: Arc<SharedGPUContext>, device_ordinal: u32) -> Self {
        Self {
            context,
            created_at: std::time::Instant::now(),
            device_ordinal,
        }
    }
}

impl std::fmt::Debug for GPUContextReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GPUContextReady")
            .field("device_ordinal", &self.device_ordinal)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Event bus for broadcasting GPU context availability to all subsystems
/// This replaces the centralized GPUManagerActor approach with a decentralized
/// event-driven architecture where each subsystem can independently receive
/// and manage its GPU context.
pub struct GPUContextBus {
    sender: broadcast::Sender<GPUContextReady>,
    /// Track the number of active subscribers
    subscriber_count: std::sync::atomic::AtomicUsize,
}

impl GPUContextBus {
    /// Create a new GPU context bus with the specified channel capacity
    /// The capacity determines how many messages can be buffered before
    /// slow receivers start losing messages.
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    /// Create a new GPU context bus with custom channel capacity
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscriber_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Publish GPU context to all subscribers
    /// Returns the number of receivers that received the message.
    /// If no receivers are subscribed, returns 0.
    pub fn publish(&self, context: Arc<SharedGPUContext>) -> usize {
        self.publish_with_device(context, 0)
    }

    /// Publish GPU context with device ordinal for multi-GPU systems
    pub fn publish_with_device(
        &self,
        context: Arc<SharedGPUContext>,
        device_ordinal: u32,
    ) -> usize {
        let event = GPUContextReady::new(context, device_ordinal);
        self.sender.send(event).unwrap_or_default()
    }

    /// Subscribe to GPU context events
    /// Returns a receiver that will receive all future GPUContextReady events.
    pub fn subscribe(&self) -> broadcast::Receiver<GPUContextReady> {
        self.subscriber_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.sender.subscribe()
    }

    /// Get the current number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Check if any subscribers are waiting for context
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}

impl Default for GPUContextBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for GPUContextBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            subscriber_count: std::sync::atomic::AtomicUsize::new(
                self.subscriber_count
                    .load(std::sync::atomic::Ordering::SeqCst),
            ),
        }
    }
}

/// Helper trait for actors that need to receive GPU context
#[async_trait::async_trait]
pub trait GPUContextSubscriber: Send + Sync {
    /// Called when GPU context becomes available
    async fn on_gpu_context_ready(&mut self, context: Arc<SharedGPUContext>) -> Result<(), String>;

    /// Called when GPU context is lost or invalidated
    async fn on_gpu_context_lost(&mut self) -> Result<(), String> {
        Ok(()) // Default no-op implementation
    }
}

/// Wrapper for managing GPU context subscription lifecycle
pub struct GPUContextSubscription {
    receiver: broadcast::Receiver<GPUContextReady>,
    current_context: Option<Arc<SharedGPUContext>>,
}

impl GPUContextSubscription {
    /// Create a new subscription from a bus
    pub fn new(bus: &GPUContextBus) -> Self {
        Self {
            receiver: bus.subscribe(),
            current_context: None,
        }
    }

    /// Wait for the next GPU context event
    pub async fn recv(&mut self) -> Result<GPUContextReady, broadcast::error::RecvError> {
        let event = self.receiver.recv().await?;
        self.current_context = Some(event.context.clone());
        Ok(event)
    }

    /// Try to receive without blocking
    pub fn try_recv(&mut self) -> Result<GPUContextReady, broadcast::error::TryRecvError> {
        let event = self.receiver.try_recv()?;
        self.current_context = Some(event.context.clone());
        Ok(event)
    }

    /// Get the current cached context (if any)
    pub fn current_context(&self) -> Option<&Arc<SharedGPUContext>> {
        self.current_context.as_ref()
    }

    /// Check if we have a valid context
    pub fn has_context(&self) -> bool {
        self.current_context.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bus_subscribe_publish() {
        let bus = GPUContextBus::new();
        let mut sub = bus.subscribe();

        // No context yet
        assert!(sub.try_recv().is_err());
    }

    #[test]
    fn test_subscriber_count() {
        let bus = GPUContextBus::new();
        assert_eq!(bus.subscriber_count(), 0);

        let _sub1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let _sub2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
    }

    #[test]
    fn test_bus_clone() {
        let bus1 = GPUContextBus::new();
        let _sub1 = bus1.subscribe();

        let bus2 = bus1.clone();
        assert_eq!(bus2.subscriber_count(), 1);
    }
}
