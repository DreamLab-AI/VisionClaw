//! Process-global pub/sub seam for memory accesses (ADR-2135).
//!
//! `POST /api/memory-flash` (and its batch form) publishes one
//! [`MemoryActivity`] per flash here, beside the `memory_flash` text frame it
//! broadcasts to clients. The force actor subscribes so agents in the
//! separated layout drift towards the memory cloud while memory is being
//! used. It mirrors [`super::hub`]: a bounded broadcast channel, oldest
//! frames dropped under backpressure (the drift is a recency signal).

use once_cell::sync::Lazy;
use tokio::sync::broadcast;

/// One memory access, as far as the layout cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryActivity {
    /// The acting agent's wire id when the producer named it (flag bits
    /// allowed); `None` credits every agent present equally.
    pub agent_id: Option<u32>,
}

const HUB_CAPACITY: usize = 256;

static MEMORY_ACTIVITY_HUB: Lazy<broadcast::Sender<MemoryActivity>> =
    Lazy::new(|| broadcast::channel(HUB_CAPACITY).0);

/// Publish a memory access; returns how many subscribers it reached.
pub fn publish(event: MemoryActivity) -> usize {
    MEMORY_ACTIVITY_HUB.send(event).unwrap_or(0)
}

/// Subscribe to memory accesses.
pub fn subscribe() -> broadcast::Receiver<MemoryActivity> {
    MEMORY_ACTIVITY_HUB.subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_receive_published_activity() {
        let mut rx = subscribe();
        let reached = publish(MemoryActivity {
            agent_id: Some(0x8000_0003),
        });
        assert!(reached >= 1);
        // Other tests in the process may publish too; find ours.
        loop {
            let got = rx.recv().await.expect("open hub");
            if got.agent_id == Some(0x8000_0003) {
                break;
            }
        }
    }
}
