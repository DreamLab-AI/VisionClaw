use log::{debug, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

pub struct MCPConnectionPool {
    connections: Arc<RwLock<HashMap<String, PooledConnection>>>,
    #[allow(dead_code)]
    max_connections_per_host: usize,
    connection_timeout: Duration,
    idle_timeout: Duration,
    semaphore: Arc<Semaphore>,
}

struct PooledConnection {
    #[allow(dead_code)]
    stream: TcpStream,
    last_used: Instant,
    in_use: bool,
}

impl MCPConnectionPool {
    pub fn new(
        max_connections_per_host: usize,
        connection_timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            max_connections_per_host,
            connection_timeout,
            idle_timeout,
            semaphore: Arc::new(Semaphore::new(max_connections_per_host * 10)),
        }
    }

    pub async fn get_connection(
        &self,
        host: &str,
        port: u16,
    ) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
        let _permit = self.semaphore.acquire().await?;
        let key = format!("{}:{}", host, port);

        {
            let mut connections = self.connections.write().await;
            if let Some(conn) = connections.get_mut(&key) {
                if !conn.in_use && conn.last_used.elapsed() < self.idle_timeout {
                    conn.in_use = true;
                    conn.last_used = Instant::now();
                    debug!("Reusing existing connection to {}", key);

                    return Err("Connection reuse needs proper implementation".into());
                }
            }
        }

        debug!("Creating new connection to {}", key);
        let stream = tokio::time::timeout(
            self.connection_timeout,
            TcpStream::connect(format!("{}:{}", host, port)),
        )
        .await??;

        {
            let _connections = self.connections.write().await;
        }

        Ok(stream)
    }

    pub async fn return_connection(&self, host: &str, port: u16) {
        let key = format!("{}:{}", host, port);
        let mut connections = self.connections.write().await;
        if let Some(conn) = connections.get_mut(&key) {
            conn.in_use = false;
            conn.last_used = Instant::now();
            debug!("Returned connection to pool: {}", key);
        }
    }

    pub async fn cleanup_idle_connections(&self) {
        let mut connections = self.connections.write().await;
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for (key, conn) in connections.iter() {
            if !conn.in_use && now.duration_since(conn.last_used) > self.idle_timeout {
                to_remove.push(key.clone());
            }
        }

        for key in to_remove {
            connections.remove(&key);
            debug!("Removed idle connection: {}", key);
        }
    }

    pub fn start_cleanup_task(self: Arc<Self>, cancellation_token: CancellationToken) {
        let pool = self;
        let cleanup_interval = Duration::from_secs(60);

        tokio::spawn({
            let pool = Arc::clone(&pool);
            let token = cancellation_token.clone();

            async move {
                let mut interval = tokio::time::interval(cleanup_interval);

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            pool.cleanup_idle_connections().await;
                        }
                        _ = token.cancelled() => {
                            debug!("Connection pool cleanup task cancelled");
                            break;
                        }
                    }
                }
            }
        });
    }
}

pub fn spawn_with_cancellation<F>(
    future: F,
    cancellation_token: CancellationToken,
) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        tokio::select! {
            _ = future => {
                debug!("Task completed successfully");
            }
            _ = cancellation_token.cancelled() => {
                debug!("Task cancelled by cancellation token");
            }
        }
    })
}

pub fn spawn_with_timeout_and_cancellation<F, T>(
    future: F,
    timeout: Duration,
    cancellation_token: CancellationToken,
) -> tokio::task::JoinHandle<Result<T, SpawnError>>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(async move {
        tokio::select! {
            result = tokio::time::timeout(timeout, future) => {
                match result {
                    Ok(value) => Ok(value),
                    Err(_) => Err(SpawnError::Timeout),
                }
            }
            _ = cancellation_token.cancelled() => {
                Err(SpawnError::Cancelled)
            }
        }
    })
}

#[derive(Debug)]
pub enum SpawnError {
    Timeout,
    Cancelled,
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::Timeout => write!(f, "Task timed out"),
            SpawnError::Cancelled => write!(f, "Task was cancelled"),
        }
    }
}

impl std::error::Error for SpawnError {}

pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    global_cancellation: CancellationToken,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            global_cancellation: CancellationToken::new(),
        }
    }

    pub async fn spawn_task<F>(&self, name: String, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = spawn_with_cancellation(future, self.global_cancellation.clone());

        let mut tasks = self.tasks.write().await;
        if let Some(old_handle) = tasks.insert(name.clone(), handle) {
            old_handle.abort();
            debug!("Replaced existing task: {}", name);
        } else {
            debug!("Started new task: {}", name);
        }
    }

    pub async fn cancel_task(&self, name: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(handle) = tasks.remove(name) {
            handle.abort();
            debug!("Cancelled task: {}", name);
            true
        } else {
            warn!("Task not found for cancellation: {}", name);
            false
        }
    }

    pub async fn cancel_all_tasks(&self) {
        self.global_cancellation.cancel();

        let mut tasks = self.tasks.write().await;
        let task_count = tasks.len();

        for (name, handle) in tasks.drain() {
            handle.abort();
            debug!("Cancelled task during shutdown: {}", name);
        }

        debug!("Cancelled {} tasks during shutdown", task_count);
    }

    pub async fn get_task_count(&self) -> usize {
        self.tasks.read().await.len()
    }

    pub async fn wait_for_all_tasks(&self, timeout: Option<Duration>) {
        let tasks = {
            let mut tasks_guard = self.tasks.write().await;
            tasks_guard
                .drain()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>()
        };

        if let Some(timeout_duration) = timeout {
            let _ = tokio::time::timeout(timeout_duration, async {
                for handle in tasks {
                    let _ = handle.await;
                }
            })
            .await;
        } else {
            for handle in tasks {
                let _ = handle.await;
            }
        }

        debug!("All tasks completed or timed out");
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_TASK_MANAGER: tokio::sync::OnceCell<TaskManager> = tokio::sync::OnceCell::const_new();

pub async fn get_global_task_manager() -> &'static TaskManager {
    GLOBAL_TASK_MANAGER
        .get_or_init(|| async { TaskManager::new() })
        .await
}

pub async fn spawn_managed_task<F>(name: String, future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let manager = get_global_task_manager().await;
    manager.spawn_task(name, future).await;
}

pub async fn graceful_shutdown(timeout: Duration) {
    let manager = get_global_task_manager().await;

    debug!("Starting graceful shutdown...");
    manager.cancel_all_tasks().await;

    debug!("Waiting for tasks to complete...");
    manager.wait_for_all_tasks(Some(timeout)).await;

    debug!("Graceful shutdown completed");
}
