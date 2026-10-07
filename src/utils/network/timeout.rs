use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{timeout, Instant, Sleep};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub connect_timeout: Duration,

    pub request_timeout: Duration,

    pub read_timeout: Duration,

    pub write_timeout: Duration,

    pub keepalive_timeout: Duration,

    pub total_timeout: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(15),
            write_timeout: Duration::from_secs(10),
            keepalive_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(120),
        }
    }
}

impl TimeoutConfig {
    pub fn low_latency() -> Self {
        Self {
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(3),
            write_timeout: Duration::from_secs(2),
            keepalive_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(10),
        }
    }

    pub fn high_throughput() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(60),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(20),
            keepalive_timeout: Duration::from_secs(300),
            total_timeout: Duration::from_secs(300),
        }
    }

    pub fn tcp_connection() -> Self {
        Self {
            connect_timeout: Duration::from_secs(8),
            request_timeout: Duration::from_secs(25),
            read_timeout: Duration::from_secs(12),
            write_timeout: Duration::from_secs(8),
            keepalive_timeout: Duration::from_secs(120),
            total_timeout: Duration::from_secs(60),
        }
    }

    pub fn websocket() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(20),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(5),
            keepalive_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(120),
        }
    }

    pub fn mcp_operations() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(5),
            keepalive_timeout: Duration::from_secs(45),
            total_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
pub enum TimeoutResult<T> {
    Success(T),

    Timeout,

    Error(Box<dyn std::error::Error + Send + Sync>),
}

impl<T> TimeoutResult<T> {
    pub fn is_success(&self) -> bool {
        matches!(self, TimeoutResult::Success(_))
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, TimeoutResult::Timeout)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, TimeoutResult::Error(_))
    }

    pub fn into_result(self) -> Result<T, TimeoutError> {
        match self {
            TimeoutResult::Success(value) => Ok(value),
            TimeoutResult::Timeout => Err(TimeoutError::Timeout),
            TimeoutResult::Error(err) => Err(TimeoutError::OperationFailed(err.to_string())),
        }
    }

    pub fn success(self) -> Option<T> {
        match self {
            TimeoutResult::Success(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TimeoutError {
    #[error("Operation timed out")]
    Timeout,
    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub async fn with_timeout<F, T>(duration: Duration, future: F) -> TimeoutResult<T>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    match timeout(duration, future).await {
        Ok(Ok(value)) => TimeoutResult::Success(value),
        Ok(Err(err)) => TimeoutResult::Error(err),
        Err(_) => TimeoutResult::Timeout,
    }
}

pub async fn with_config_timeout<F, T>(
    config: &TimeoutConfig,
    operation_type: TimeoutType,
    future: F,
) -> TimeoutResult<T>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    let timeout_duration = match operation_type {
        TimeoutType::Connect => config.connect_timeout,
        TimeoutType::Request => config.request_timeout,
        TimeoutType::Read => config.read_timeout,
        TimeoutType::Write => config.write_timeout,
        TimeoutType::Keepalive => config.keepalive_timeout,
        TimeoutType::Total => config.total_timeout,
    };

    debug!(
        "Setting {} timeout to {:?}",
        operation_type.as_str(),
        timeout_duration
    );
    with_timeout(timeout_duration, future).await
}

#[derive(Debug, Clone, Copy)]
pub enum TimeoutType {
    Connect,
    Request,
    Read,
    Write,
    Keepalive,
    Total,
}

impl TimeoutType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeoutType::Connect => "connect",
            TimeoutType::Request => "request",
            TimeoutType::Read => "read",
            TimeoutType::Write => "write",
            TimeoutType::Keepalive => "keepalive",
            TimeoutType::Total => "total",
        }
    }
}

pub struct TimeoutGuard {
    total_timeout: Duration,
    started_at: Instant,
    config: TimeoutConfig,
}

impl TimeoutGuard {
    pub fn new(config: TimeoutConfig) -> Self {
        Self {
            total_timeout: config.total_timeout,
            started_at: Instant::now(),
            config,
        }
    }

    pub fn remaining_time(&self) -> Option<Duration> {
        let elapsed = self.started_at.elapsed();
        self.total_timeout.checked_sub(elapsed)
    }

    pub fn timeout_for(&self, operation_type: TimeoutType) -> Option<Duration> {
        let operation_timeout = match operation_type {
            TimeoutType::Connect => self.config.connect_timeout,
            TimeoutType::Request => self.config.request_timeout,
            TimeoutType::Read => self.config.read_timeout,
            TimeoutType::Write => self.config.write_timeout,
            TimeoutType::Keepalive => self.config.keepalive_timeout,
            TimeoutType::Total => return self.remaining_time(),
        };

        self.remaining_time()
            .map(|remaining| operation_timeout.min(remaining))
    }

    pub fn is_expired(&self) -> bool {
        self.remaining_time().is_none()
    }

    pub async fn execute<F, T>(&self, operation_type: TimeoutType, future: F) -> TimeoutResult<T>
    where
        F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
    {
        if self.is_expired() {
            warn!("TimeoutGuard: Total timeout already exceeded");
            return TimeoutResult::Timeout;
        }

        match self.timeout_for(operation_type) {
            Some(timeout_duration) => {
                debug!(
                    "TimeoutGuard: Executing {} operation with {:?} timeout",
                    operation_type.as_str(),
                    timeout_duration
                );
                with_timeout(timeout_duration, future).await
            }
            None => {
                warn!(
                    "TimeoutGuard: No time remaining for {} operation",
                    operation_type.as_str()
                );
                TimeoutResult::Timeout
            }
        }
    }
}

pub async fn connect_with_timeout<F, T>(
    config: &TimeoutConfig,
    future: F,
) -> Result<T, TimeoutError>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    with_config_timeout(config, TimeoutType::Connect, future)
        .await
        .into_result()
}

pub async fn request_with_timeout<F, T>(
    config: &TimeoutConfig,
    future: F,
) -> Result<T, TimeoutError>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    with_config_timeout(config, TimeoutType::Request, future)
        .await
        .into_result()
}

pub async fn read_with_timeout<F, T>(config: &TimeoutConfig, future: F) -> Result<T, TimeoutError>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    with_config_timeout(config, TimeoutType::Read, future)
        .await
        .into_result()
}

pub async fn write_with_timeout<F, T>(config: &TimeoutConfig, future: F) -> Result<T, TimeoutError>
where
    F: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    with_config_timeout(config, TimeoutType::Write, future)
        .await
        .into_result()
}

pub struct AdaptiveTimeout<F> {
    future: Pin<Box<F>>,
    sleep: Pin<Box<Sleep>>,
    timeout_duration: Duration,
    started_at: Instant,
}

impl<F> AdaptiveTimeout<F>
where
    F: Future,
{
    pub fn new(future: F, initial_timeout: Duration) -> Self {
        let sleep = Box::pin(tokio::time::sleep(initial_timeout));
        Self {
            future: Box::pin(future),
            sleep,
            timeout_duration: initial_timeout,
            started_at: Instant::now(),
        }
    }

    pub fn extend_timeout(&mut self, additional_time: Duration) {
        let new_timeout = self.timeout_duration + additional_time;
        let elapsed = self.started_at.elapsed();

        if elapsed < new_timeout {
            let remaining = new_timeout - elapsed;
            self.sleep = Box::pin(tokio::time::sleep(remaining));
            self.timeout_duration = new_timeout;
            debug!(
                "Extended timeout by {:?}, new timeout: {:?}",
                additional_time, new_timeout
            );
        }
    }
}

impl<F> Future for AdaptiveTimeout<F>
where
    F: Future,
{
    type Output = Result<F::Output, TimeoutError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(output) = self.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }

        if self.sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(TimeoutError::Timeout));
        }

        Poll::Pending
    }
}

pub struct BatchTimeoutManager {
    total_timeout: Duration,
    started_at: Instant,
    operation_timeouts: Vec<Duration>,
    completed_operations: usize,
}

impl BatchTimeoutManager {
    pub fn new(total_timeout: Duration, expected_operations: usize) -> Self {
        let per_operation_timeout = total_timeout / expected_operations as u32;
        let operation_timeouts = vec![per_operation_timeout; expected_operations];

        Self {
            total_timeout,
            started_at: Instant::now(),
            operation_timeouts,
            completed_operations: 0,
        }
    }

    pub fn next_operation_timeout(&mut self) -> Option<Duration> {
        if self.completed_operations >= self.operation_timeouts.len() {
            return None;
        }

        let remaining_total = self.total_timeout.checked_sub(self.started_at.elapsed())?;
        let remaining_operations = self.operation_timeouts.len() - self.completed_operations;

        if remaining_operations == 0 {
            return Some(remaining_total);
        }

        let per_operation = remaining_total / remaining_operations as u32;
        let planned_timeout = self.operation_timeouts[self.completed_operations];

        Some(per_operation.min(planned_timeout))
    }

    pub fn mark_operation_completed(&mut self) {
        self.completed_operations += 1;
    }

    pub fn has_time_remaining(&self) -> bool {
        self.started_at.elapsed() < self.total_timeout
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_timeout_result_success() {
        let result = with_timeout(Duration::from_secs(1), async {
            Ok::<i32, Box<dyn std::error::Error + Send + Sync>>(42)
        })
        .await;

        assert!(result.is_success());
        assert_eq!(result.success().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_timeout_result_timeout() {
        let result = with_timeout(Duration::from_millis(10), async {
            sleep(Duration::from_millis(50)).await;
            Ok::<i32, Box<dyn std::error::Error + Send + Sync>>(42)
        })
        .await;

        assert!(result.is_timeout());
        assert!(result.success().is_none());
    }

    #[tokio::test]
    async fn test_timeout_guard() {
        let config = TimeoutConfig {
            total_timeout: Duration::from_millis(100),
            connect_timeout: Duration::from_millis(50),
            ..Default::default()
        };

        let guard = TimeoutGuard::new(config);

        assert!(!guard.is_expired());
        assert!(guard.remaining_time().is_some());

        let connect_timeout = guard.timeout_for(TimeoutType::Connect);
        assert!(connect_timeout.is_some());
    }

    #[tokio::test]
    async fn test_timeout_guard_expiry() {
        let config = TimeoutConfig {
            total_timeout: Duration::from_millis(10),
            ..Default::default()
        };

        let guard = TimeoutGuard::new(config);

        sleep(Duration::from_millis(20)).await;

        assert!(guard.is_expired());
        assert!(guard.remaining_time().is_none());
    }

    #[tokio::test]
    async fn test_batch_timeout_manager() {
        let mut manager = BatchTimeoutManager::new(Duration::from_millis(100), 3);

        assert!(manager.has_time_remaining());

        let timeout1 = manager.next_operation_timeout();
        assert!(timeout1.is_some());

        manager.mark_operation_completed();

        let timeout2 = manager.next_operation_timeout();
        assert!(timeout2.is_some());
    }

    #[test]
    fn test_timeout_config_presets() {
        let low_latency = TimeoutConfig::low_latency();
        assert!(low_latency.connect_timeout < Duration::from_secs(5));

        let high_throughput = TimeoutConfig::high_throughput();
        assert!(high_throughput.total_timeout > Duration::from_secs(60));

        let tcp = TimeoutConfig::tcp_connection();
        assert!(tcp.connect_timeout < Duration::from_secs(15));
    }

    #[tokio::test]
    async fn test_adaptive_timeout() {
        // Test 1: Verify timeout occurs when future takes too long
        let adaptive = AdaptiveTimeout::new(
            async {
                sleep(Duration::from_millis(50)).await;
                42
            },
            Duration::from_millis(10),
        );

        let result = adaptive.await;
        assert!(result.is_err()); // Should timeout

        // Test 2: Verify success when timeout is sufficient
        let adaptive = AdaptiveTimeout::new(
            async {
                sleep(Duration::from_millis(10)).await;
                42
            },
            Duration::from_millis(50),
        );

        let result = adaptive.await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}
