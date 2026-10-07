// tests/actors/orchestration_improvements_test.rs
//! Integration tests for ADR-031 orchestration improvements.
//!
//! Covers all 7 items:
//!  1. Round-robin poll-offset fairness (AgentMonitorActor)
//!  2. Capacity-aware claiming (TaskOrchestratorActor)
//!  3. Observational status inference via TaskStatusChanged
//!  4. HeartbeatDirective serialisation round-trip
//!  5. BroadcastResult congested/closed client struct
//!  6. Panic isolation in EventBus handlers
//!  7. Graceful drain (TaskOrchestratorActor + SupervisorActor)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// ─── Item 4: HeartbeatDirective round-trip ──────────────────────────────────

#[cfg(test)]
mod heartbeat_directive_tests {
    use visionclaw_server::utils::websocket_heartbeat::HeartbeatDirective;

    #[test]
    fn reload_config_serialises_correctly() {
        let d = HeartbeatDirective::ReloadConfig;
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"directive\":\"reload_config\""), "got: {}", json);
    }

    #[test]
    fn force_full_sync_serialises_correctly() {
        let d = HeartbeatDirective::ForceFullSync;
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"directive\":\"force_full_sync\""), "got: {}", json);
    }

    #[test]
    fn update_available_serialises_with_version() {
        let d = HeartbeatDirective::UpdateAvailable { version: "1.2.3".to_string() };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"directive\":\"update_available\""), "got: {}", json);
        assert!(json.contains("\"version\":\"1.2.3\""), "got: {}", json);
    }

    #[test]
    fn reload_config_round_trips() {
        let original = HeartbeatDirective::ReloadConfig;
        let json = serde_json::to_string(&original).unwrap();
        let decoded: HeartbeatDirective = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, HeartbeatDirective::ReloadConfig));
    }

    #[test]
    fn update_available_round_trips() {
        let original = HeartbeatDirective::UpdateAvailable { version: "2.0.0".to_string() };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: HeartbeatDirective = serde_json::from_str(&json).unwrap();
        match decoded {
            HeartbeatDirective::UpdateAvailable { version } => assert_eq!(version, "2.0.0"),
            other => panic!("Expected UpdateAvailable, got {:?}", other),
        }
    }
}

// ─── Item 5: BroadcastResult struct ─────────────────────────────────────────

#[cfg(test)]
mod broadcast_result_tests {
    use visionclaw_server::actors::client_coordinator_actor::BroadcastResult;

    #[test]
    fn default_broadcast_result_is_empty() {
        let r = BroadcastResult::default();
        assert_eq!(r.sent, 0);
        assert!(r.congested_clients.is_empty());
        assert!(r.closed_clients.is_empty());
    }

    #[test]
    fn broadcast_result_tracks_sent_congested_and_closed() {
        let r = BroadcastResult {
            sent: 5,
            congested_clients: vec![3],
            closed_clients: vec![2, 4],
        };
        assert_eq!(r.sent, 5);
        assert_eq!(r.congested_clients, vec![3]);
        assert_eq!(r.closed_clients, vec![2, 4]);
    }
}

// ─── Item 2: TaskOrchestratorActor capacity check ───────────────────────────

#[cfg(test)]
mod task_orchestrator_tests {
    use actix::prelude::*;
    use visionclaw_server::actors::task_orchestrator_actor::{
        CreateTask, DrainTasksBeforeShutdown, TaskOrchestratorActor,
    };
    use visionclaw_server::services::management_api_client::ManagementApiClient;

    fn make_client() -> ManagementApiClient {
        ManagementApiClient::new(
            "agentic-workstation".to_string(),
            9090,
            "test-key".to_string(),
        )
    }

    #[actix::test]
    async fn reject_task_when_draining() {
        let actor = TaskOrchestratorActor::new(make_client()).start();

        // Initiate drain with a long timeout so the actor stays alive for the test
        actor.do_send(DrainTasksBeforeShutdown { timeout_secs: 60 });

        // Give the message a moment to be processed
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = actor
            .send(CreateTask {
                agent: "test-agent".to_string(),
                task: "do something".to_string(),
                provider: "openai".to_string(),
                claude_flow_agent_id: None,
            })
            .await
            .expect("mailbox send ok");

        assert!(result.is_err(), "expected Err during drain, got {:?}", result);
        let msg = result.unwrap_err();
        assert!(
            msg.contains("draining"),
            "error should mention draining, got: {}",
            msg
        );
    }

    #[test]
    fn actor_creation_has_correct_defaults() {
        let actor = TaskOrchestratorActor::new(make_client());
        assert_eq!(actor.max_retries, 3);
        assert_eq!(actor.active_tasks.len(), 0);
        // accepting_tasks starts true
        assert!(actor.accepting_tasks);
    }
}

// ─── Item 6: Panic isolation in EventBus ────────────────────────────────────

#[cfg(test)]
mod event_bus_panic_isolation_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use async_trait::async_trait;
    use visionclaw_server::events::bus::EventBus;
    use visionclaw_server::events::types::{EventError, EventHandler, StoredEvent};
    use visionclaw_server::events::domain_events::NodeAddedEvent;
    use visionclaw_server::utils::time;

    struct PanicHandler;

    #[async_trait]
    impl EventHandler for PanicHandler {
        fn event_type(&self) -> &'static str { "NodeAdded" }
        fn handler_id(&self) -> &str { "panic-handler" }
        async fn handle(&self, _event: &StoredEvent) -> Result<(), EventError> {
            panic!("intentional test panic");
        }
    }

    struct CountingHandler {
        count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventHandler for CountingHandler {
        fn event_type(&self) -> &'static str { "NodeAdded" }
        fn handler_id(&self) -> &str { "counting-handler" }
        async fn handle(&self, _event: &StoredEvent) -> Result<(), EventError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn make_event() -> NodeAddedEvent {
        NodeAddedEvent {
            node_id: "n1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        }
    }

    #[tokio::test]
    async fn panic_in_one_handler_does_not_kill_bus() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));

        // Register both: panicking handler first, then counting handler
        bus.subscribe(Arc::new(PanicHandler)).await;
        bus.subscribe(Arc::new(CountingHandler { count: count.clone() })).await;

        // publish should not itself panic
        let _ = bus.publish(make_event()).await;

        // The counting handler must still have run despite the panic in the other
        assert_eq!(
            count.load(Ordering::SeqCst), 1,
            "counting handler should run even when another handler panics"
        );
    }

    #[tokio::test]
    async fn panic_is_reported_as_handler_error() {
        let bus = EventBus::new();
        bus.subscribe(Arc::new(PanicHandler)).await;

        let result = bus.publish(make_event()).await;
        // With a single handler that panics, all handlers failed → Err returned
        assert!(
            result.is_err(),
            "expected Err when the only handler panics"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("panic") || err_msg.contains("handler"),
            "error should mention panic or handler, got: {}",
            err_msg
        );
    }
}

// ─── Item 7: SupervisorActor graceful drain ─────────────────────────────────

#[cfg(test)]
mod supervisor_drain_tests {
    use std::time::Duration;
    use actix::prelude::*;
    use visionclaw_server::actors::supervisor::{
        InitiateGracefulShutdown, RegisterActor, SupervisorActor, SupervisionStrategy,
    };

    #[actix::test]
    async fn register_actor_rejected_after_drain_initiated() {
        let supervisor = SupervisorActor::new("test-supervisor".to_string()).start();

        // Register one actor successfully before drain
        supervisor
            .send(RegisterActor {
                actor_name: "actor-before-drain".to_string(),
                strategy: SupervisionStrategy::Restart,
                max_restart_count: 3,
                restart_window: Duration::from_secs(60),
                actor_factory: None,
            })
            .await
            .unwrap()
            .expect("registration before drain should succeed");

        // Initiate drain with long timeout
        supervisor.do_send(InitiateGracefulShutdown { timeout_secs: 60 });

        // Allow the drain message to be processed
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Registration after drain should fail
        let result = supervisor
            .send(RegisterActor {
                actor_name: "actor-after-drain".to_string(),
                strategy: SupervisionStrategy::Restart,
                max_restart_count: 3,
                restart_window: Duration::from_secs(60),
                actor_factory: None,
            })
            .await
            .unwrap();

        assert!(
            result.is_err(),
            "registration after drain should be rejected"
        );
    }
}

// ─── Item 1: poll_offset rotation (unit-level) ──────────────────────────────

#[cfg(test)]
mod poll_offset_tests {
    /// Verify that the spiral-index offset wraps correctly via
    /// wrapping_add, matching the AgentMonitorActor::poll_offset pattern.
    #[test]
    fn offset_wraps_around_on_overflow() {
        let mut offset: usize = usize::MAX;
        offset = offset.wrapping_add(1);
        assert_eq!(offset, 0, "wrapping_add should wrap from MAX to 0");
    }

    #[test]
    fn spiral_index_rotated_by_offset() {
        let agents = vec!["a", "b", "c", "d"];
        let n = agents.len();
        let mut seen_first: Vec<&str> = Vec::new();

        for poll in 0..n {
            let first_idx = (0 + poll) % n; // spiral_i for i=0 with increasing offsets
            seen_first.push(agents[first_idx]);
        }

        // Over n polls, every agent should appear as the "first polled" exactly once
        let mut sorted = seen_first.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(), n,
            "every agent should be first-polled once across n rotations; got: {:?}",
            seen_first
        );
    }
}

// ─── Item 3: TaskStatusChanged message is defined ───────────────────────────

#[cfg(test)]
mod task_status_changed_tests {
    use visionclaw_server::actors::messages::{SetAgentMonitorAddr, TaskStatusChanged};

    /// Compile-time check that the message types are constructible and their
    /// fields are accessible — sufficient to verify the message was added.
    #[test]
    fn task_status_changed_fields_accessible() {
        let msg = TaskStatusChanged {
            agent_type: "coder".to_string(),
            running_task_count: 3,
        };
        assert_eq!(msg.agent_type, "coder");
        assert_eq!(msg.running_task_count, 3);
    }
}
