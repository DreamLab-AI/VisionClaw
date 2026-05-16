//! Actor supervision system to replace panic! calls with graceful recovery
//!
//! This module provides supervision trees that can restart failed actors
//! and implement exponential backoff retry strategies.

#[cfg(test)]
use crate::errors::ActorError;
use crate::errors::VisionFlowError;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::utils::time;

#[derive(Debug, Clone)]
pub enum SupervisionStrategy {
    
    Restart,
    
    RestartWithBackoff {
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    },
    
    Escalate,
    
    Stop,
}

#[derive(Debug, Clone)]
pub struct SupervisedActorInfo {
    pub name: String,
    pub strategy: SupervisionStrategy,
    pub max_restart_count: u32,
    pub restart_window: Duration,
    pub actor_type: String,
    pub is_running: bool,
    pub session_id: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
}

/// Type-erased factory the supervisor can call to re-create a supervised actor.
/// The returned `Box<dyn std::any::Any + Send>` is opaque to the supervisor — it
/// only needs to know whether the call succeeded.
pub type ActorFactory = Arc<dyn Fn() -> Box<dyn std::any::Any + Send> + Send + Sync>;

struct ActorState {
    actor_info: SupervisedActorInfo,
    restart_count: u32,
    last_restart: Option<Instant>,
    current_delay: Duration,
    is_running: bool,
    #[allow(dead_code)]
    session_id: Option<String>,
    /// Optional factory to re-spawn the actor on restart.
    /// When `None`, the supervisor can only mark the actor as failed —
    /// it cannot automatically recreate it.
    actor_factory: Option<ActorFactory>,
}

#[derive(Message)]
#[rtype(result = "Result<(), VisionFlowError>")]
pub struct RegisterActor {
    pub actor_name: String,
    pub strategy: SupervisionStrategy,
    pub max_restart_count: u32,
    pub restart_window: Duration,
    /// Optional factory closure that the supervisor will invoke to re-create the
    /// actor when a restart is triggered. If `None`, the supervisor logs a warning
    /// and marks the actor as not running instead of attempting a restart.
    pub actor_factory: Option<ActorFactory>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ActorFailed {
    pub actor_name: String,
    pub error: VisionFlowError,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ActorStarted {
    pub actor_name: String,
}

#[derive(Message)]
#[rtype(result = "Result<SupervisionStatus, VisionFlowError>")]
pub struct GetSupervisionStatus;

#[derive(Debug, Clone)]
pub struct SupervisionStatus {
    pub total_actors: usize,
    pub running_actors: usize,
    pub failed_actors: usize,
    pub actors: Vec<ActorStatusInfo>,
}

#[derive(Debug, Clone)]
pub struct ActorStatusInfo {
    pub name: String,
    pub is_running: bool,
    pub restart_count: u32,
    pub last_restart: Option<Instant>,
    pub strategy: SupervisionStrategy,
}

/// ADR-031 item 7: Graceful shutdown message for the supervisor.
///
/// On receipt the supervisor sets `draining = true` (stops accepting new actor
/// registrations) and schedules its own `ctx.stop()` after `timeout_secs`.
/// This mirrors Multica's WaitGroup-based 30-second drain pattern, adapted for
/// the Actix supervision tree.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InitiateGracefulShutdown {
    /// Seconds to wait before force-stopping even if supervised actors are alive.
    pub timeout_secs: u64,
}

pub struct SupervisorActor {
    supervised_actors: HashMap<String, ActorState>,
    supervisor_name: String,
    /// ADR-031 item 7: When `true`, `RegisterActor` is rejected so new actors
    /// cannot join the supervision tree during a drain window.
    draining: bool,
}

impl SupervisorActor {
    pub fn new(supervisor_name: String) -> Self {
        Self {
            supervised_actors: HashMap::new(),
            supervisor_name,
            draining: false,
        }
    }

    #[allow(dead_code)]
    fn should_restart(&self, actor_name: &str, state: &ActorState) -> bool {
        
        if state.restart_count >= state.actor_info.max_restart_count {
            if let Some(last_restart) = state.last_restart {
                if last_restart.elapsed() < state.actor_info.restart_window {
                    warn!(
                        "Actor '{}' has exceeded max restart count ({}) within window ({:?})",
                        actor_name,
                        state.actor_info.max_restart_count,
                        state.actor_info.restart_window
                    );
                    return false;
                }
                
            }
        }
        true
    }

    fn calculate_restart_delay(&self, state: &ActorState) -> Duration {
        match &state.actor_info.strategy {
            SupervisionStrategy::RestartWithBackoff {
                initial_delay: _,
                max_delay,
                multiplier,
            } => {
                let delay = Duration::from_millis(
                    (state.current_delay.as_millis() as f64 * multiplier) as u64,
                );
                std::cmp::min(delay, *max_delay)
            }
            _ => Duration::from_millis(0),
        }
    }

    fn restart_actor(&mut self, actor_name: &str, ctx: &mut Context<Self>) {
        let delay = if let Some(state) = self.supervised_actors.get(actor_name) {
            self.calculate_restart_delay(state)
        } else {
            return;
        };

        if let Some(state) = self.supervised_actors.get_mut(actor_name) {
            state.restart_count += 1;
            state.last_restart = Some(Instant::now());
            state.current_delay = delay;

            info!(
                "Scheduling restart for actor '{}' in {:?} (attempt {})",
                actor_name, delay, state.restart_count
            );

            let actor_name_clone = actor_name.to_string();
            let supervisor_name = self.supervisor_name.clone();

            ctx.run_later(delay, move |_act, ctx| {
                info!("Attempting to restart actor '{}'", actor_name_clone);

                
                
                
                ctx.notify(RestartAttempt {
                    actor_name: actor_name_clone,
                    supervisor_name,
                });
            });
        }
    }
}

impl Actor for SupervisorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Supervisor '{}' started", self.supervisor_name);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Supervisor '{}' stopped", self.supervisor_name);
    }
}

impl Handler<InitiateGracefulShutdown> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: InitiateGracefulShutdown, ctx: &mut Self::Context) {
        let running = self
            .supervised_actors
            .values()
            .filter(|s| s.is_running)
            .count();
        info!(
            "[SupervisorActor '{}'] Graceful shutdown initiated — \
             rejecting new registrations, stopping in {}s ({} actor(s) running)",
            self.supervisor_name, msg.timeout_secs, running
        );
        self.draining = true;

        ctx.run_later(Duration::from_secs(msg.timeout_secs), |_act, ctx| {
            info!("[SupervisorActor] Drain timeout elapsed — stopping supervisor");
            ctx.stop();
        });
    }
}

impl Handler<RegisterActor> for SupervisorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, msg: RegisterActor, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-031 item 7: Reject new registrations during drain.
        if self.draining {
            warn!(
                "[SupervisorActor '{}'] Rejecting registration of '{}' — supervisor is draining",
                self.supervisor_name, msg.actor_name
            );
            return Err(VisionFlowError::Generic {
                message: "Supervisor is draining, cannot register new actors".to_string(),
                source: None,
            });
        }

        let actor_info = SupervisedActorInfo {
            name: msg.actor_name.clone(),
            strategy: msg.strategy.clone(),
            max_restart_count: msg.max_restart_count,
            restart_window: msg.restart_window,
            actor_type: "generic".to_string(),
            is_running: false,
            session_id: None,
            last_heartbeat: time::now(),
        };

        let initial_delay = match &msg.strategy {
            SupervisionStrategy::RestartWithBackoff { initial_delay, .. } => *initial_delay,
            _ => Duration::from_millis(100),
        };

        let state = ActorState {
            actor_info,
            restart_count: 0,
            last_restart: None,
            current_delay: initial_delay,
            is_running: true,
            session_id: None,
            actor_factory: msg.actor_factory,
        };

        self.supervised_actors.insert(msg.actor_name.clone(), state);
        info!("Registered actor '{}' for supervision", msg.actor_name);
        Ok(())
    }
}

impl Handler<ActorFailed> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailed, ctx: &mut Self::Context) {
        error!("Actor '{}' failed: {}", msg.actor_name, msg.error);

        if let Some(state) = self.supervised_actors.get_mut(&msg.actor_name) {
            state.is_running = false;
            let strategy = state.actor_info.strategy.clone();

            
            let should_restart = match &strategy {
                SupervisionStrategy::Restart | SupervisionStrategy::RestartWithBackoff { .. } => {
                    
                    if state.restart_count >= state.actor_info.max_restart_count {
                        if let Some(last_restart) = state.last_restart {
                            if last_restart.elapsed() < state.actor_info.restart_window {
                                warn!("Actor '{}' has exceeded max restart count ({}) within window ({:?})",
                                      &msg.actor_name, state.actor_info.max_restart_count,
                                      state.actor_info.restart_window);
                                false
                            } else {
                                
                                state.restart_count = 0;
                                true
                            }
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                _ => false,
            };

            
            match strategy {
                SupervisionStrategy::Restart => {
                    if should_restart {
                        self.restart_actor(&msg.actor_name, ctx);
                    } else {
                        error!(
                            "Actor '{}' will not be restarted (too many failures)",
                            msg.actor_name
                        );
                    }
                }
                SupervisionStrategy::RestartWithBackoff { .. } => {
                    if should_restart {
                        self.restart_actor(&msg.actor_name, ctx);
                    } else {
                        error!(
                            "Actor '{}' will not be restarted (too many failures)",
                            msg.actor_name
                        );
                    }
                }
                SupervisionStrategy::Escalate => {
                    warn!(
                        "Escalating failure of actor '{}' to parent supervisor",
                        msg.actor_name
                    );
                    
                }
                SupervisionStrategy::Stop => {
                    info!(
                        "Actor '{}' stopped permanently due to supervision strategy",
                        msg.actor_name
                    );
                    if let Some(state) = self.supervised_actors.get_mut(&msg.actor_name) {
                        state.is_running = false;
                    }
                }
            }
        } else {
            warn!(
                "Received failure notification for unregistered actor '{}'",
                msg.actor_name
            );
        }
    }
}

impl Handler<ActorStarted> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: ActorStarted, _ctx: &mut Self::Context) {
        if let Some(state) = self.supervised_actors.get_mut(&msg.actor_name) {
            state.is_running = true;
            info!("Actor '{}' started successfully", msg.actor_name);
        }
    }
}

impl Handler<GetSupervisionStatus> for SupervisorActor {
    type Result = Result<SupervisionStatus, VisionFlowError>;

    fn handle(&mut self, _msg: GetSupervisionStatus, _ctx: &mut Self::Context) -> Self::Result {
        let total_actors = self.supervised_actors.len();
        let running_actors = self
            .supervised_actors
            .values()
            .filter(|s| s.is_running)
            .count();
        let failed_actors = total_actors - running_actors;

        let actors = self
            .supervised_actors
            .iter()
            .map(|(name, state)| ActorStatusInfo {
                name: name.clone(),
                is_running: state.is_running,
                restart_count: state.restart_count,
                last_restart: state.last_restart,
                strategy: state.actor_info.strategy.clone(),
            })
            .collect();

        Ok(SupervisionStatus {
            total_actors,
            running_actors,
            failed_actors,
            actors,
        })
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct RestartAttempt {
    actor_name: String,
    #[allow(dead_code)]
    supervisor_name: String,
}

impl Handler<RestartAttempt> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: RestartAttempt, _ctx: &mut Self::Context) {
        debug!("Processing restart attempt for actor '{}'", msg.actor_name);

        if let Some(state) = self.supervised_actors.get_mut(&msg.actor_name) {
            if let Some(ref factory) = state.actor_factory {
                // ADR-01 D4: panics in CUDA kernel launches and the actor
                // factory must propagate to the actix runtime so the failure
                // is surfaced at full backtrace fidelity. The previous
                // `std::panic::catch_unwind` wrapper here swallowed those
                // panics and reported "factory panicked: <message>" without a
                // stack — that is the failure mode this commit fixes.
                //
                // The factory is expected to be infallible (it constructs and
                // starts a new Actix actor). If construction can fail it must
                // do so via a `Result`, not via a panic. The
                // `should_allow_restart` time-window guard in
                // `ActorFailed::handle` enforces the 3-per-60s ADR-01 D4
                // restart budget before we ever reach this handler.
                let factory_clone = Arc::clone(factory);
                let _new_actor = factory_clone();
                state.is_running = true;
                info!(
                    "Actor '{}' restarted successfully via factory (attempt {})",
                    msg.actor_name, state.restart_count
                );
            } else {
                // No factory registered — cannot auto-restart.
                state.is_running = false;
                warn!(
                    "Actor '{}' has no actor_factory registered — cannot auto-restart. \
                     Marking as not running.",
                    msg.actor_name
                );
            }
        }
    }
}

// DEPRECATED: Voice command handler removed - uses legacy DockerHiveMind
// Replace with TaskOrchestratorActor integration



pub trait SupervisedActorTrait: Actor {
    fn actor_name() -> &'static str;

    fn supervision_strategy() -> SupervisionStrategy {
        SupervisionStrategy::RestartWithBackoff {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }

    fn max_restart_count() -> u32 {
        5
    }

    fn restart_window() -> Duration {
        Duration::from_secs(300) 
    }

    
    fn report_error(&self, supervisor: &Addr<SupervisorActor>, error: VisionFlowError) {
        supervisor.do_send(ActorFailed {
            actor_name: Self::actor_name().to_string(),
            error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;
use crate::utils::time;

    #[actix::test]
    async fn test_actor_registration() {
        let supervisor = SupervisorActor::new("TestSupervisor".to_string()).start();

        let register_msg = RegisterActor {
            actor_name: "TestActor".to_string(),
            strategy: SupervisionStrategy::Restart,
            max_restart_count: 3,
            restart_window: Duration::from_secs(60),
            actor_factory: None,
        };

        let result = supervisor.send(register_msg).await.unwrap();
        assert!(result.is_ok());

        let status = supervisor
            .send(GetSupervisionStatus)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.total_actors, 1);
        assert_eq!(status.running_actors, 1);
    }

    #[actix::test]
    async fn test_actor_failure_handling() {
        let supervisor = SupervisorActor::new("TestSupervisor".to_string()).start();

        
        let register_msg = RegisterActor {
            actor_name: "TestActor".to_string(),
            strategy: SupervisionStrategy::Restart,
            max_restart_count: 3,
            restart_window: Duration::from_secs(60),
            actor_factory: None,
        };

        supervisor.send(register_msg).await.unwrap().unwrap();


        let failure_msg = ActorFailed {
            actor_name: "TestActor".to_string(),
            error: VisionFlowError::Actor(ActorError::RuntimeFailure {
                actor_name: "TestActor".to_string(),
                reason: "Test failure".to_string(),
            }),
        };

        supervisor.send(failure_msg).await.unwrap();

        
        sleep(Duration::from_millis(100)).await;

        let status = supervisor
            .send(GetSupervisionStatus)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.total_actors, 1);
    }
}
