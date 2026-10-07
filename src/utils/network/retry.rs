use log::{debug, error, warn};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: usize,

    pub initial_delay: Duration,

    pub max_delay: Duration,

    pub backoff_multiplier: f64,

    pub jitter_factor: f64,

    pub preserve_original_error: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
            preserve_original_error: true,
        }
    }
}

impl RetryConfig {
    pub fn network() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(15),
            backoff_multiplier: 2.0,
            jitter_factor: 0.2,
            preserve_original_error: true,
        }
    }

    pub fn tcp_connection() -> Self {
        Self {
            max_attempts: 6,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 1.5,
            jitter_factor: 0.25,
            preserve_original_error: true,
        }
    }

    pub fn websocket() -> Self {
        Self {
            max_attempts: 4,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter_factor: 0.15,
            preserve_original_error: true,
        }
    }

    pub fn mcp_operations() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(150),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 1.8,
            jitter_factor: 0.1,
            preserve_original_error: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RetryError<E> {
    #[error("All retry attempts exhausted. Last error: {0}")]
    AllAttemptsFailed(E),
    #[error("Retry operation was cancelled")]
    Cancelled,
    #[error("Retry configuration error: {0}")]
    ConfigError(String),
    #[error("Resource exhaustion detected: {0}")]
    ResourceExhaustion(String),
}

pub type RetryResult<T, E> = Result<T, RetryError<E>>;

pub trait RetryableError {
    fn is_retryable(&self) -> bool;
    fn is_transient(&self) -> bool {
        self.is_retryable()
    }
}

impl RetryableError for std::io::Error {
    fn is_retryable(&self) -> bool {
        matches!(
            self.kind(),
            std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::BrokenPipe
        )
    }
}

impl RetryableError for tokio::time::error::Elapsed {
    fn is_retryable(&self) -> bool {
        true
    }
}

impl<E> RetryableError for Box<E>
where
    E: RetryableError,
{
    fn is_retryable(&self) -> bool {
        self.as_ref().is_retryable()
    }
}

impl<E> RetryableError for std::sync::Arc<E>
where
    E: RetryableError,
{
    fn is_retryable(&self) -> bool {
        self.as_ref().is_retryable()
    }
}

// Specific implementation for Arc<std::io::Error> removed to avoid conflicts
// The generic impl<E> RetryableError for Arc<E> covers this case

// Implementation for Arc<dyn std::error::Error + Send + Sync>
impl RetryableError for std::sync::Arc<dyn std::error::Error + Send + Sync> {
    fn is_retryable(&self) -> bool {
        true
    }
}

fn calculate_delay(config: &RetryConfig, attempt: usize) -> Duration {
    if attempt == 0 {
        return Duration::from_millis(0);
    }

    let base_delay = config.initial_delay.as_millis() as f64;
    let exponential_delay = base_delay * config.backoff_multiplier.powi((attempt - 1) as i32);

    let capped_delay = exponential_delay.min(config.max_delay.as_millis() as f64);

    let jitter = if config.jitter_factor > 0.0 {
        let mut rng = rand::thread_rng();
        let jitter_amount = capped_delay * config.jitter_factor;
        rng.gen_range(-jitter_amount..=jitter_amount)
    } else {
        0.0
    };

    let final_delay = (capped_delay + jitter).max(0.0) as u64;
    Duration::from_millis(final_delay)
}

pub async fn retry_with_backoff<F, Fut, T, E>(
    config: RetryConfig,
    mut operation: F,
) -> RetryResult<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError + std::fmt::Debug + Clone,
{
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        debug!("Retry attempt {} of {}", attempt + 1, config.max_attempts);

        if let Err(resource_error) = check_system_resources().await {
            warn!(
                "System resources exhausted, aborting retry: {:?}",
                resource_error
            );
            return Err(RetryError::ConfigError(format!(
                "Resource exhausted: {}",
                resource_error
            )));
        }

        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    debug!("Operation succeeded on attempt {}", attempt + 1);
                }
                return Ok(result);
            }
            Err(error) => {
                if !error.is_retryable() {
                    warn!("Non-retryable error encountered: {:?}", error);
                    return Err(RetryError::AllAttemptsFailed(error));
                }

                if is_resource_exhaustion_error(&error) {
                    error!(
                        "Resource exhaustion detected, aborting retries: {:?}",
                        error
                    );
                    return Err(RetryError::AllAttemptsFailed(error));
                }

                if attempt + 1 >= config.max_attempts {
                    error!("All retry attempts exhausted. Final error: {:?}", error);
                    return Err(RetryError::AllAttemptsFailed(error));
                }

                let delay = calculate_delay(&config, attempt + 1);
                warn!(
                    "Attempt {} failed: {:?}. Retrying in {:?}",
                    attempt + 1,
                    error,
                    delay
                );

                last_error = Some(error);

                if delay > Duration::from_millis(0) {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(RetryError::AllAttemptsFailed(
        last_error.expect("Should have at least one error"),
    ))
}

async fn check_system_resources() -> Result<(), String> {
    if let Ok(fd_count) = count_open_file_descriptors() {
        const FD_WARNING_THRESHOLD: usize = 800;
        const FD_ERROR_THRESHOLD: usize = 950;

        if fd_count > FD_ERROR_THRESHOLD {
            return Err(format!(
                "Too many open file descriptors: {} > {}",
                fd_count, FD_ERROR_THRESHOLD
            ));
        } else if fd_count > FD_WARNING_THRESHOLD {
            warn!(
                "High file descriptor usage: {} (threshold: {})",
                fd_count, FD_WARNING_THRESHOLD
            );
        }
    }

    #[cfg(target_os = "linux")]
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(available_line) = meminfo
            .lines()
            .find(|line| line.starts_with("MemAvailable:"))
        {
            if let Some(available_kb) = available_line.split_whitespace().nth(1) {
                if let Ok(available_kb) = available_kb.parse::<u64>() {
                    const MIN_AVAILABLE_MB: u64 = 100;
                    let available_mb = available_kb / 1024;
                    if available_mb < MIN_AVAILABLE_MB {
                        return Err(format!("Low memory: {}MB available", available_mb));
                    }
                }
            }
        }
    }

    Ok(())
}

fn count_open_file_descriptors() -> Result<usize, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        match fs::read_dir("/proc/self/fd") {
            Ok(entries) => Ok(entries.count().saturating_sub(1)),
            Err(e) => Err(e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(10)
    }
}

fn is_resource_exhaustion_error<E: std::fmt::Debug>(error: &E) -> bool {
    let error_str = format!("{:?}", error).to_lowercase();
    error_str.contains("too many open files")
        || error_str.contains("resource temporarily unavailable")
        || error_str.contains("no buffer space available")
        || error_str.contains("out of memory")
        || error_str.contains("enfile")
        || error_str.contains("emfile")
}

pub async fn retry_network_operation<F, Fut, T, E>(operation: F) -> RetryResult<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError + std::fmt::Debug + Clone,
{
    retry_with_backoff(RetryConfig::network(), operation).await
}

pub async fn retry_tcp_connection<F, Fut, T, E>(operation: F) -> RetryResult<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError + std::fmt::Debug + Clone,
{
    retry_with_backoff(RetryConfig::tcp_connection(), operation).await
}

pub async fn retry_websocket_operation<F, Fut, T, E>(operation: F) -> RetryResult<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError + std::fmt::Debug + Clone,
{
    retry_with_backoff(RetryConfig::websocket(), operation).await
}

pub async fn retry_mcp_operation<F, Fut, T, E>(operation: F) -> RetryResult<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError + std::fmt::Debug + Clone,
{
    retry_with_backoff(RetryConfig::mcp_operations(), operation).await
}

pub async fn retry_with_timeout<F, Fut, T, E>(
    config: RetryConfig,
    timeout: Duration,
    operation: F,
) -> RetryResult<T, E>
where
    F: FnMut() -> Fut + Send,
    Fut: Future<Output = Result<T, E>> + Send,
    T: Send,
    E: RetryableError + std::fmt::Debug + Clone + Send,
{
    let retry_future = retry_with_backoff(config, operation);

    match tokio::time::timeout(timeout, retry_future).await {
        Ok(result) => result,
        Err(_) => Err(RetryError::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Debug, Clone)]
    struct TestError {
        retryable: bool,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError(retryable: {})", self.retryable)
        }
    }

    impl std::error::Error for TestError {}

    impl RetryableError for TestError {
        fn is_retryable(&self) -> bool {
            self.retryable
        }
    }

    #[tokio::test]
    async fn test_successful_operation() {
        let result = retry_with_backoff(RetryConfig::default(), || async {
            Ok::<i32, TestError>(42)
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_until_success() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = retry_with_backoff(
            RetryConfig {
                max_attempts: 3,
                initial_delay: Duration::from_millis(1),
                ..Default::default()
            },
            move || {
                let counter = counter_clone.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    if count < 2 {
                        Err(TestError { retryable: true })
                    } else {
                        Ok(42)
                    }
                }
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_non_retryable_error() {
        let result = retry_with_backoff(RetryConfig::default(), || async {
            Err::<i32, TestError>(TestError { retryable: false })
        })
        .await;

        assert!(result.is_err());
        match result {
            Err(RetryError::AllAttemptsFailed(_)) => (),
            _ => panic!("Expected AllAttemptsFailed error"),
        }
    }

    #[tokio::test]
    async fn test_all_attempts_exhausted() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = retry_with_backoff(
            RetryConfig {
                max_attempts: 2,
                initial_delay: Duration::from_millis(1),
                ..Default::default()
            },
            move || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, TestError>(TestError { retryable: true })
                }
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_delay_calculation() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter_factor: 0.0,
            ..Default::default()
        };

        assert_eq!(calculate_delay(&config, 0), Duration::from_millis(0));
        assert_eq!(calculate_delay(&config, 1), Duration::from_millis(100));
        assert_eq!(calculate_delay(&config, 2), Duration::from_millis(200));
        assert_eq!(calculate_delay(&config, 3), Duration::from_millis(400));
    }
}
