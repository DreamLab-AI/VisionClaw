//! Boot readiness of the stored graph.
//!
//! HTTP and WebSocket listeners come up before `GraphStateActor` has loaded the
//! stored graph (`ReloadGraphFromDatabase`, a few seconds later). A client that
//! asked for the initial graph in that window used to receive an empty graph and
//! nothing re-sent the real one. The initial-graph paths wait on this gate
//! instead, and answer "loading" or "failed" rather than an empty graph.
//!
//! The gate is owned by `AppState` and handed to the actor that loads the graph;
//! there is no process-wide state.

use std::time::Duration;
use tokio::sync::watch;

/// REST requests wait less than the client's 10 s request timeout, so the
/// caller receives the 503 rather than timing out first.
pub const REST_GRAPH_WAIT: Duration = Duration::from_secs(8);

/// Where the first graph load stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphLoad {
    /// No load has completed yet.
    Loading,
    /// The stored graph is in the actor.
    Ready,
    /// The latest load attempt failed.
    Failed,
}

/// Shared latch over [`GraphLoad`]; cheap to clone behind an `Arc`.
#[derive(Debug)]
pub struct GraphReadiness {
    tx: watch::Sender<GraphLoad>,
}

impl Default for GraphReadiness {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphReadiness {
    /// A fresh gate in [`GraphLoad::Loading`].
    pub fn new() -> Self {
        Self {
            tx: watch::Sender::new(GraphLoad::Loading),
        }
    }

    /// Record that the graph has loaded; wakes every waiter.
    pub fn mark_ready(&self) {
        self.tx.send_replace(GraphLoad::Ready);
    }

    /// Record a failed load; wakes waiters so they answer at once. A later
    /// successful load moves the gate to ready.
    pub fn mark_failed(&self) {
        self.tx.send_if_modified(|s| {
            // A graph already served stays served: a failed reload keeps the old state.
            if *s == GraphLoad::Ready {
                false
            } else {
                *s = GraphLoad::Failed;
                true
            }
        });
    }

    /// Current state.
    pub fn state(&self) -> GraphLoad {
        *self.tx.borrow()
    }

    /// Wait until the state leaves [`GraphLoad::Loading`], or `timeout` passes
    /// (then [`GraphLoad::Loading`] is returned).
    pub async fn wait(&self, timeout: Duration) -> GraphLoad {
        let mut rx = self.tx.subscribe();
        let settled = tokio::time::timeout(timeout, async {
            rx.wait_for(|s| *s != GraphLoad::Loading).await.map(|s| *s)
        })
        .await;
        match settled {
            Ok(Ok(state)) => state,
            _ => self.state(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn before_first_load_the_gate_reports_loading() {
        let g = GraphReadiness::new();
        assert_eq!(g.state(), GraphLoad::Loading);
        assert_eq!(g.wait(Duration::from_millis(30)).await, GraphLoad::Loading);
    }

    #[tokio::test]
    async fn waiter_is_released_when_graph_loads() {
        let g = Arc::new(GraphReadiness::new());
        let g2 = g.clone();
        let waiter = tokio::spawn(async move { g2.wait(Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        g.mark_ready();
        assert_eq!(waiter.await.unwrap(), GraphLoad::Ready);
    }

    #[tokio::test]
    async fn failure_releases_waiters_at_once_and_a_later_load_recovers() {
        let g = GraphReadiness::new();
        g.mark_failed();
        assert_eq!(g.wait(Duration::from_secs(5)).await, GraphLoad::Failed);
        g.mark_ready();
        assert_eq!(g.state(), GraphLoad::Ready);
        g.mark_failed();
        assert_eq!(g.state(), GraphLoad::Ready, "a served graph stays served");
    }
}
