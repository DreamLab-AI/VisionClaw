use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitBreakerState {
    
    Closed,
    
    Open,
    
    HalfOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    
    pub failure_threshold: usize,
    
    pub failure_rate_threshold: f64,
    
    pub time_window: Duration,
    
    pub recovery_timeout: Duration,
    
    pub success_threshold: usize,
    
    pub half_open_max_requests: usize,
    
    pub minimum_request_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            failure_rate_threshold: 0.5,
            time_window: Duration::from_secs(60),
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 3,
            half_open_max_requests: 5,
            minimum_request_threshold: 10,
        }
    }
}

impl CircuitBreakerConfig {
    
    pub fn network() -> Self {
        Self {
            failure_threshold: 3,
            failure_rate_threshold: 0.6,
            time_window: Duration::from_secs(30),
            recovery_timeout: Duration::from_secs(15),
            success_threshold: 2,
            half_open_max_requests: 3,
            minimum_request_threshold: 5,
        }
    }

    
    pub fn tcp_connection() -> Self {
        Self {
            failure_threshold: 5,
            failure_rate_threshold: 0.7,
            time_window: Duration::from_secs(60),
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 3,
            half_open_max_requests: 5,
            minimum_request_threshold: 8,
        }
    }

    
    pub fn websocket() -> Self {
        Self {
            failure_threshold: 4,
            failure_rate_threshold: 0.5,
            time_window: Duration::from_secs(45),
            recovery_timeout: Duration::from_secs(20),
            success_threshold: 2,
            half_open_max_requests: 4,
            minimum_request_threshold: 6,
        }
    }

    
    pub fn mcp_operations() -> Self {
        Self {
            failure_threshold: 3,
            failure_rate_threshold: 0.4,
            time_window: Duration::from_secs(30),
            recovery_timeout: Duration::from_secs(10),
            success_threshold: 2,
            half_open_max_requests: 3,
            minimum_request_threshold: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    pub state: CircuitBreakerState,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub rejected_requests: u64,
    pub failure_rate: f64,
    pub consecutive_failures: usize,
    pub last_failure_time: Option<SystemTime>,
    pub state_changed_at: SystemTime,
    pub time_since_state_change: Duration,
}

#[derive(Debug)]
pub enum RequestOutcome {
    Success,
    Failure,
    Timeout,
    Rejected,
}

#[derive(Debug)]
struct RequestRecord {
    timestamp: Instant,
    success: bool,
}

pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
    consecutive_failures: Arc<AtomicUsize>,
    total_requests: Arc<AtomicU64>,
    successful_requests: Arc<AtomicU64>,
    failed_requests: Arc<AtomicU64>,
    rejected_requests: Arc<AtomicU64>,
    half_open_requests: Arc<AtomicUsize>,
    half_open_successes: Arc<AtomicUsize>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    state_changed_at: Arc<RwLock<Instant>>,
    request_history: Arc<RwLock<Vec<RequestRecord>>>,
}

impl CircuitBreaker {
    
    pub fn new(config: CircuitBreakerConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::Closed)),
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            total_requests: Arc::new(AtomicU64::new(0)),
            successful_requests: Arc::new(AtomicU64::new(0)),
            failed_requests: Arc::new(AtomicU64::new(0)),
            rejected_requests: Arc::new(AtomicU64::new(0)),
            half_open_requests: Arc::new(AtomicUsize::new(0)),
            half_open_successes: Arc::new(AtomicUsize::new(0)),
            last_failure_time: Arc::new(RwLock::new(None)),
            state_changed_at: Arc::new(RwLock::new(now)),
            request_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    
    pub fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    
    pub fn network() -> Self {
        Self::new(CircuitBreakerConfig::network())
    }

    
    pub fn tcp_connection() -> Self {
        Self::new(CircuitBreakerConfig::tcp_connection())
    }

    
    pub fn websocket() -> Self {
        Self::new(CircuitBreakerConfig::websocket())
    }

    
    pub fn mcp_operations() -> Self {
        Self::new(CircuitBreakerConfig::mcp_operations())
    }

    
    pub async fn can_execute(&self) -> bool {
        let state = *self.state.read().await;

        match state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                
                if let Some(last_failure) = *self.last_failure_time.read().await {
                    if last_failure.elapsed() >= self.config.recovery_timeout {
                        self.transition_to_half_open().await;
                        return true;
                    }
                }
                false
            }
            CircuitBreakerState::HalfOpen => {
                let current_requests = self.half_open_requests.load(Ordering::Acquire);
                current_requests < self.config.half_open_max_requests
            }
        }
    }

    
    pub async fn record_request(&self, outcome: RequestOutcome) {
        let now = Instant::now();
        let state = *self.state.read().await;

        self.total_requests.fetch_add(1, Ordering::Release);

        match outcome {
            RequestOutcome::Success => {
                self.successful_requests.fetch_add(1, Ordering::Release);
                self.consecutive_failures.store(0, Ordering::Release);

                
                self.add_to_history(now, true).await;

                if state == CircuitBreakerState::HalfOpen {
                    let successes = self.half_open_successes.fetch_add(1, Ordering::Release) + 1;
                    if successes >= self.config.success_threshold {
                        self.transition_to_closed().await;
                    }
                }

                debug!("Circuit breaker: Request succeeded");
            }
            RequestOutcome::Failure | RequestOutcome::Timeout => {
                self.failed_requests.fetch_add(1, Ordering::Release);
                let consecutive = self.consecutive_failures.fetch_add(1, Ordering::Release) + 1;

                
                *self.last_failure_time.write().await = Some(now);

                
                self.add_to_history(now, false).await;

                warn!(
                    "Circuit breaker: Request failed (consecutive: {})",
                    consecutive
                );

                
                if state != CircuitBreakerState::Open {
                    if self.should_open_circuit().await {
                        self.transition_to_open().await;
                    }
                }
            }
            RequestOutcome::Rejected => {
                self.rejected_requests.fetch_add(1, Ordering::Release);
                debug!("Circuit breaker: Request rejected");
            }
        }
    }

    
    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        if !self.can_execute().await {
            self.record_request(RequestOutcome::Rejected).await;
            return Err(CircuitBreakerError::CircuitOpen);
        }

        let state = *self.state.read().await;
        if state == CircuitBreakerState::HalfOpen {
            self.half_open_requests.fetch_add(1, Ordering::Release);
        }

        match operation.await {
            Ok(result) => {
                self.record_request(RequestOutcome::Success).await;
                Ok(result)
            }
            Err(error) => {
                self.record_request(RequestOutcome::Failure).await;
                Err(CircuitBreakerError::OperationFailed(error))
            }
        }
    }

    
    pub async fn stats(&self) -> CircuitBreakerStats {
        let state = *self.state.read().await;
        let state_changed_at = *self.state_changed_at.read().await;
        let failure_rate = self.calculate_current_failure_rate().await;

        CircuitBreakerStats {
            state,
            total_requests: self.total_requests.load(Ordering::Acquire),
            successful_requests: self.successful_requests.load(Ordering::Acquire),
            failed_requests: self.failed_requests.load(Ordering::Acquire),
            rejected_requests: self.rejected_requests.load(Ordering::Acquire),
            failure_rate,
            consecutive_failures: self.consecutive_failures.load(Ordering::Acquire),
            last_failure_time: self
                .last_failure_time
                .read()
                .await
                .map(|instant| SystemTime::now() - instant.elapsed()),
            state_changed_at: SystemTime::now() - state_changed_at.elapsed(),
            time_since_state_change: state_changed_at.elapsed(),
        }
    }

    
    pub async fn reset(&self) {
        info!("Resetting circuit breaker");
        *self.state.write().await = CircuitBreakerState::Closed;
        self.consecutive_failures.store(0, Ordering::Release);
        self.half_open_requests.store(0, Ordering::Release);
        self.half_open_successes.store(0, Ordering::Release);
        *self.last_failure_time.write().await = None;
        *self.state_changed_at.write().await = Instant::now();
        self.request_history.write().await.clear();
    }

    

    async fn should_open_circuit(&self) -> bool {
        let consecutive = self.consecutive_failures.load(Ordering::Acquire);
        if consecutive >= self.config.failure_threshold {
            return true;
        }

        let failure_rate = self.calculate_current_failure_rate().await;
        let total_in_window = self.count_requests_in_window().await;

        failure_rate >= self.config.failure_rate_threshold
            && total_in_window >= self.config.minimum_request_threshold
    }

    async fn transition_to_open(&self) -> bool {
        let mut state = self.state.write().await;
        if *state != CircuitBreakerState::Open {
            warn!("Circuit breaker transitioning to OPEN state");
            *state = CircuitBreakerState::Open;
            *self.state_changed_at.write().await = Instant::now();
            true
        } else {
            false
        }
    }

    async fn transition_to_half_open(&self) -> bool {
        let mut state = self.state.write().await;
        if *state == CircuitBreakerState::Open {
            info!("Circuit breaker transitioning to HALF_OPEN state");
            *state = CircuitBreakerState::HalfOpen;
            self.half_open_requests.store(0, Ordering::Release);
            self.half_open_successes.store(0, Ordering::Release);
            *self.state_changed_at.write().await = Instant::now();
            true
        } else {
            false
        }
    }

    async fn transition_to_closed(&self) -> bool {
        let mut state = self.state.write().await;
        if *state != CircuitBreakerState::Closed {
            info!("Circuit breaker transitioning to CLOSED state");
            *state = CircuitBreakerState::Closed;
            self.consecutive_failures.store(0, Ordering::Release);
            self.half_open_requests.store(0, Ordering::Release);
            self.half_open_successes.store(0, Ordering::Release);
            *self.state_changed_at.write().await = Instant::now();
            true
        } else {
            false
        }
    }

    async fn add_to_history(&self, timestamp: Instant, success: bool) {
        let mut history = self.request_history.write().await;
        history.push(RequestRecord { timestamp, success });

        
        let cutoff = timestamp - self.config.time_window;
        history.retain(|record| record.timestamp > cutoff);
    }

    async fn calculate_current_failure_rate(&self) -> f64 {
        let history = self.request_history.read().await;
        if history.is_empty() {
            return 0.0;
        }

        let failures = history.iter().filter(|r| !r.success).count();
        failures as f64 / history.len() as f64
    }

    async fn count_requests_in_window(&self) -> usize {
        let history = self.request_history.read().await;
        history.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open - requests are being rejected")]
    CircuitOpen,
    #[error("Operation failed: {0}")]
    OperationFailed(E),
}

pub struct CircuitBreakerRegistry {
    breakers: Arc<RwLock<std::collections::HashMap<String, Arc<CircuitBreaker>>>>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    
    pub async fn get_or_create(
        &self,
        name: &str,
        config: CircuitBreakerConfig,
    ) -> Arc<CircuitBreaker> {
        let mut breakers = self.breakers.write().await;

        if let Some(breaker) = breakers.get(name) {
            breaker.clone()
        } else {
            let breaker = Arc::new(CircuitBreaker::new(config));
            breakers.insert(name.to_string(), breaker.clone());
            info!("Created new circuit breaker: {}", name);
            breaker
        }
    }

    
    pub async fn get_all_stats(&self) -> std::collections::HashMap<String, CircuitBreakerStats> {
        let breakers = self.breakers.read().await;
        let mut stats = std::collections::HashMap::new();

        for (name, breaker) in breakers.iter() {
            stats.insert(name.clone(), breaker.stats().await);
        }

        stats
    }

    
    pub async fn reset_all(&self) {
        let breakers = self.breakers.read().await;
        for breaker in breakers.values() {
            breaker.reset().await;
        }
        info!("Reset all circuit breakers");
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });

        assert!(breaker.can_execute().await);

        
        breaker.record_request(RequestOutcome::Success).await;
        let stats = breaker.stats().await;
        assert_eq!(stats.state, CircuitBreakerState::Closed);
        assert_eq!(stats.successful_requests, 1);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_on_failures() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(100),
            ..Default::default()
        });

        
        breaker.record_request(RequestOutcome::Failure).await;
        assert_eq!(breaker.stats().await.state, CircuitBreakerState::Closed);

        
        breaker.record_request(RequestOutcome::Failure).await;
        assert_eq!(breaker.stats().await.state, CircuitBreakerState::Open);
        assert!(!breaker.can_execute().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_transition() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 1,
            ..Default::default()
        });

        
        breaker.record_request(RequestOutcome::Failure).await;
        assert_eq!(breaker.stats().await.state, CircuitBreakerState::Open);

        
        sleep(Duration::from_millis(60)).await;

        
        assert!(breaker.can_execute().await);
        assert_eq!(breaker.stats().await.state, CircuitBreakerState::HalfOpen);

        
        breaker.record_request(RequestOutcome::Success).await;
        assert_eq!(breaker.stats().await.state, CircuitBreakerState::Closed);
    }

    #[tokio::test]
    async fn test_execute_function() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        });

        
        let result = breaker.execute(async { Ok::<i32, &'static str>(42) }).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        
        let result = breaker
            .execute(async { Err::<i32, &'static str>("error") })
            .await;
        assert!(matches!(
            result,
            Err(CircuitBreakerError::OperationFailed(_))
        ));

        
        let result = breaker.execute(async { Ok::<i32, &'static str>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::CircuitOpen)));
    }
}
