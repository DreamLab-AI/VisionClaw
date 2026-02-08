use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, Sender};
use log::{info, warn, Level};
use serde::{Deserialize, Serialize};
use serde_json:::json;
use std::{
    collections::HashMap,
    fs::{create_dir_all, metadata, remove_file, rename, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use crate::utils:::time;
use crate::utils::json::to_json;

// Structured log entry for JSON logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub component: String,
    pub message: String,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub execution_time_ms: Option<f64>,
    pub memory_usage_mb: Option<f64>,
    pub gpu_metrics: Option<GPULogMetrics>,
}

// GPU-specific log metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPULogMetrics {
    pub kernel_name: Option<String>,
    pub execution_time_us: Option<f64>,
    pub memory_allocated_mb: Option<f64>,
    pub memory_peak_mb: Option<f64>,
    pub gpu_utilization_percent: Option<f32>,
    pub error_count: Option<u32>,
    pub recovery_attempts: Option<u32>,
    pub performance_anomaly: Option<bool>,
}

// Log component types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogComponent {
    Server,
    Client,
    GPU,
    Analytics,
    Memory,
    Network,
    Performance,
    Error,
}

impl LogComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogComponent::Server => "server",
            LogComponent::Client => "client",
            LogComponent::GPU => "gpu",
            LogComponent::Analytics => "analytics",
            LogComponent::Memory => "memory",
            LogComponent::Network => "network",
            LogComponent::Performance => "performance",
            LogComponent::Error => "error",
        }
    }

    pub fn log_file_name(&self) -> String {
        format!("{}.log", self.as_str())
    }
}

// Log rotation configuration
#[derive(Debug, Clone)]
pub struct LogRotationConfig {
    pub max_file_size_mb: u64,
    pub max_files: usize,
    pub compress_rotated: bool,
    pub rotation_interval_hours: u64,
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_file_size_mb: 50,
            max_files: 10,
            compress_rotated: true,
            rotation_interval_hours: 24,
        }
    }
}

// Advanced logging manager
pub struct AdvancedLogger {
    _log_sender: Sender<LogEntry>,
    _log_receivers: Arc<Mutex<HashMap<LogComponent, Receiver<LogEntry>>>>,
    log_writers: Arc<RwLock<HashMap<LogComponent, BufWriter<File>>>>,
    log_dir: PathBuf,
    rotation_config: LogRotationConfig,
    performance_metrics: Arc<Mutex<HashMap<String, Vec<f64>>>>,
    gpu_error_count: Arc<Mutex<u32>>,
    recovery_attempts: Arc<Mutex<u32>>,
}

impl AdvancedLogger {
    pub fn new(log_dir: impl AsRef<Path>) -> io::Result<Self> {
        let log_dir = log_dir.as_ref().to_path_buf();
        create_dir_all(&log_dir)?;
        create_dir_all(log_dir.join("archived"))?;

        let (log_sender, _) = unbounded();
        let log_receivers = Arc::new(Mutex::new(HashMap::new()));
        let log_writers = Arc::new(RwLock::new(HashMap::new()));

        
        let components = [
            LogComponent::Server,
            LogComponent::Client,
            LogComponent::GPU,
            LogComponent::Analytics,
            LogComponent::Memory,
            LogComponent::Network,
            LogComponent::Performance,
            LogComponent::Error,
        ];

        let mut writers_map = HashMap::new();
        for component in &components {
            let file_path = log_dir.join(component.log_file_name());
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)?;
            writers_map.insert(*component, BufWriter::new(file));
        }

        if let Ok(mut writers_guard) = log_writers.write() {
            *writers_guard = writers_map;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to acquire log writers lock",
            ));
        }

        Ok(Self {
            _log_sender: log_sender,
            _log_receivers: log_receivers,
            log_writers,
            log_dir,
            rotation_config: LogRotationConfig::default(),
            performance_metrics: Arc::new(Mutex::new(HashMap::new())),
            gpu_error_count: Arc::new(Mutex::new(0)),
            recovery_attempts: Arc::new(Mutex::new(0)),
        })
    }

    pub fn log_structured(
        &self,
        component: LogComponent,
        level: Level,
        message: &str,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) {
        let entry = LogEntry {
            timestamp: time::now(),
            level: level.to_string(),
            component: component.as_str().to_string(),
            message: message.to_string(),
            metadata,
            execution_time_ms: None,
            memory_usage_mb: None,
            gpu_metrics: None,
        };

        self.write_log_entry(component, &entry);
    }

    pub fn log_gpu_kernel(
        &self,
        kernel_name: &str,
        execution_time_us: f64,
        memory_allocated_mb: f64,
        memory_peak_mb: f64,
    ) {
        
        let gpu_error_count = self.gpu_error_count.lock().unwrap_or_else(|poisoned| {
            warn!("GPU error count mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        let recovery_attempts = self.recovery_attempts.lock().unwrap_or_else(|poisoned| {
            warn!("Recovery attempts mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        let mut metrics_guard = self.performance_metrics.lock().unwrap_or_else(|poisoned| {
            warn!("Performance metrics mutex was poisoned, recovering");
            poisoned.into_inner()
        });

        
        let current_error_count = *gpu_error_count;
        let current_recovery_attempts = *recovery_attempts;

        
        let performance_anomaly = self.detect_performance_anomaly_with_metrics(
            kernel_name,
            execution_time_us,
            &metrics_guard,
        );

        
        metrics_guard
            .entry(kernel_name.to_string())
            .or_insert_with(Vec::new)
            .push(execution_time_us);

        
        if let Some(kernel_metrics) = metrics_guard.get_mut(kernel_name) {
            if kernel_metrics.len() > 100 {
                kernel_metrics.remove(0);
            }
        }

        
        drop(metrics_guard);

        let gpu_metrics = GPULogMetrics {
            kernel_name: Some(kernel_name.to_string()),
            execution_time_us: Some(execution_time_us),
            memory_allocated_mb: Some(memory_allocated_mb),
            memory_peak_mb: Some(memory_peak_mb),
            gpu_utilization_percent: None,
            error_count: Some(current_error_count),
            recovery_attempts: Some(current_recovery_attempts),
            performance_anomaly: Some(performance_anomaly),
        };

        
        drop(gpu_error_count);
        drop(recovery_attempts);

        let entry = LogEntry {
            timestamp: time::now(),
            level: "INFO".to_string(),
            component: "gpu".to_string(),
            message: format!(
                "Kernel {} executed in {:.2}μs",
                kernel_name, execution_time_us
            ),
            metadata: None,
            execution_time_ms: Some(execution_time_us / 1000.0),
            memory_usage_mb: Some(memory_allocated_mb),
            gpu_metrics: Some(gpu_metrics),
        };

        self.write_log_entry(LogComponent::GPU, &entry);
    }

    pub fn log_gpu_error(&self, error_msg: &str, recovery_attempted: bool) {
        
        let mut gpu_error_count_guard = self.gpu_error_count.lock().unwrap_or_else(|poisoned| {
            warn!("GPU error count mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        let mut recovery_attempts_guard =
            self.recovery_attempts.lock().unwrap_or_else(|poisoned| {
                warn!("Recovery attempts mutex was poisoned, recovering");
                poisoned.into_inner()
            });

        
        *gpu_error_count_guard += 1;
        if recovery_attempted {
            *recovery_attempts_guard += 1;
        }

        
        let current_error_count = *gpu_error_count_guard;
        let current_recovery_attempts = *recovery_attempts_guard;

        
        drop(gpu_error_count_guard);
        drop(recovery_attempts_guard);

        let gpu_metrics = GPULogMetrics {
            kernel_name: None,
            execution_time_us: None,
            memory_allocated_mb: None,
            memory_peak_mb: None,
            gpu_utilization_percent: None,
            error_count: Some(current_error_count),
            recovery_attempts: Some(current_recovery_attempts),
            performance_anomaly: None,
        };

        let entry = LogEntry {
            timestamp: time::now(),
            level: "ERROR".to_string(),
            component: "gpu".to_string(),
            message: error_msg.to_string(),
            metadata: Some(
                [("recovery_attempted".to_string(), json!(recovery_attempted))]
                    .into_iter()
                    .collect(),
            ),
            execution_time_ms: None,
            memory_usage_mb: None,
            gpu_metrics: Some(gpu_metrics),
        };

        self.write_log_entry(LogComponent::GPU, &entry);
        self.write_log_entry(LogComponent::Error, &entry);
    }

    pub fn log_memory_event(&self, event_type: &str, allocated_mb: f64, peak_mb: f64) {
        let metadata = [
            ("event_type".to_string(), json!(event_type)),
            ("allocated_mb".to_string(), json!(allocated_mb)),
            ("peak_mb".to_string(), json!(peak_mb)),
        ]
        .into_iter()
        .collect();

        let entry = LogEntry {
            timestamp: time::now(),
            level: "INFO".to_string(),
            component: "memory".to_string(),
            message: format!(
                "Memory {} - Allocated: {:.2}MB, Peak: {:.2}MB",
                event_type, allocated_mb, peak_mb
            ),
            metadata: Some(metadata),
            execution_time_ms: None,
            memory_usage_mb: Some(allocated_mb),
            gpu_metrics: None,
        };

        self.write_log_entry(LogComponent::Memory, &entry);
    }

    pub fn log_performance(&self, operation: &str, duration_ms: f64, throughput: Option<f64>) {
        let mut metadata = HashMap::new();
        metadata.insert("operation".to_string(), json!(operation));
        metadata.insert("duration_ms".to_string(), json!(duration_ms));
        if let Some(tp) = throughput {
            metadata.insert("throughput".to_string(), json!(tp));
        }

        let entry = LogEntry {
            timestamp: time::now(),
            level: "INFO".to_string(),
            component: "performance".to_string(),
            message: format!("Operation {} completed in {:.2}ms", operation, duration_ms),
            metadata: Some(metadata),
            execution_time_ms: Some(duration_ms),
            memory_usage_mb: None,
            gpu_metrics: None,
        };

        self.write_log_entry(LogComponent::Performance, &entry);
    }

    fn write_log_entry(&self, component: LogComponent, entry: &LogEntry) {
        let json_line =
            to_json(entry).unwrap_or_else(|_| "Invalid log entry".to_string());

        
        if let Ok(mut writers) = self.log_writers.try_write() {
            if let Some(writer) = writers.get_mut(&component) {
                let _ = writeln!(writer, "{}", json_line);
                let _ = writer.flush();
            }
        }
        

        
        self.check_and_rotate_logs(component);
    }

    fn check_and_rotate_logs(&self, component: LogComponent) {
        let file_path = self.log_dir.join(component.log_file_name());

        if let Ok(metadata) = metadata(&file_path) {
            let size_mb = metadata.len() / (1024 * 1024);
            if size_mb >= self.rotation_config.max_file_size_mb {
                self.rotate_log_file(component);
            }
        }
    }

    fn rotate_log_file(&self, component: LogComponent) {
        let base_name = component.log_file_name();
        let current_path = self.log_dir.join(&base_name);
        let timestamp = time::now().format("%Y%m%d_%H%M%S");
        let archived_name = format!("{}_{}.log", component.as_str(), timestamp);
        let archived_path = self.log_dir.join("archived").join(archived_name);

        
        if rename(&current_path, &archived_path).is_ok() {
            
            if let Ok(new_file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&current_path)
            {
                if let Ok(mut writers) = self.log_writers.write() {
                    writers.insert(component, BufWriter::new(new_file));
                } else {
                    warn!(
                        "Failed to acquire writers lock during log rotation for {:?}",
                        component
                    );
                }
            }

            
            self.cleanup_old_logs(component);
        }
    }

    fn cleanup_old_logs(&self, component: LogComponent) {
        let archived_dir = self.log_dir.join("archived");
        if let Ok(entries) = std::fs::read_dir(&archived_dir) {
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{}_", component.as_str()))
                })
                .collect();

            if log_files.len() > self.rotation_config.max_files {
                log_files.sort_by(|a, b| {
                    let a_time = a
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let b_time = b
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    a_time.cmp(&b_time)
                });

                
                for file_entry in log_files
                    .iter()
                    .take(log_files.len() - self.rotation_config.max_files)
                {
                    let _ = remove_file(file_entry.path());
                }
            }
        }
    }

    fn detect_performance_anomaly(&self, kernel_name: &str, execution_time_us: f64) -> bool {
        if let Ok(metrics) = self.performance_metrics.try_lock() {
            self.detect_performance_anomaly_with_metrics(kernel_name, execution_time_us, &metrics)
        } else {
            
            false
        }
    }

    fn detect_performance_anomaly_with_metrics(
        &self,
        kernel_name: &str,
        execution_time_us: f64,
        metrics: &HashMap<String, Vec<f64>>,
    ) -> bool {
        if let Some(kernel_metrics) = metrics.get(kernel_name) {
            if kernel_metrics.len() > 10 {
                let avg: f64 = kernel_metrics.iter().sum::<f64>() / kernel_metrics.len() as f64;
                let variance: f64 = kernel_metrics
                    .iter()
                    .map(|x| (x - avg).powi(2))
                    .sum::<f64>()
                    / kernel_metrics.len() as f64;
                let std_dev = variance.sqrt();

                
                execution_time_us > avg + (3.0 * std_dev)
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn get_performance_summary(&self) -> HashMap<String, serde_json::Value> {
        let mut summary = HashMap::new();

        
        let metrics_result = self.performance_metrics.try_lock();
        let gpu_error_result = self.gpu_error_count.try_lock();
        let recovery_result = self.recovery_attempts.try_lock();

        
        if let Ok(metrics) = metrics_result {
            for (kernel_name, times) in metrics.iter() {
                if !times.is_empty() {
                    let avg = times.iter().sum::<f64>() / times.len() as f64;
                    let min = times.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                    let max = times.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

                    summary.insert(
                        kernel_name.clone(),
                        json!({
                            "avg_time_us": avg,
                            "min_time_us": min,
                            "max_time_us": max,
                            "sample_count": times.len(),
                            "total_time_us": times.iter().sum::<f64>()
                        }),
                    );
                }
            }
        } else {
            summary.insert(
                "metrics_unavailable".to_string(),
                json!("Performance metrics locked, try again later"),
            );
        }

        
        if let Ok(gpu_errors) = gpu_error_result {
            summary.insert("gpu_errors".to_string(), json!(*gpu_errors));
        }

        if let Ok(recovery_attempts) = recovery_result {
            summary.insert("recovery_attempts".to_string(), json!(*recovery_attempts));
        }

        summary
    }
}

// Global logger instance
static ADVANCED_LOGGER: once_cell::sync::OnceCell<AdvancedLogger> =
    once_cell::sync::OnceCell::new();

pub fn init_advanced_logging() -> io::Result<()> {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
    let advanced_logger = AdvancedLogger::new(&log_dir)?;

    if ADVANCED_LOGGER.set(advanced_logger).is_err() {
        warn!("Advanced logger was already initialized");
    }

    info!(
        "Advanced logging system initialized with log directory: {}",
        log_dir
    );
    Ok(())
}

// Public API functions
pub fn log_gpu_kernel(
    kernel_name: &str,
    execution_time_us: f64,
    memory_allocated_mb: f64,
    memory_peak_mb: f64,
) {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.log_gpu_kernel(
            kernel_name,
            execution_time_us,
            memory_allocated_mb,
            memory_peak_mb,
        );
    }
}

pub fn log_gpu_error(error_msg: &str, recovery_attempted: bool) {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.log_gpu_error(error_msg, recovery_attempted);
    }
}

pub fn log_memory_event(event_type: &str, allocated_mb: f64, peak_mb: f64) {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.log_memory_event(event_type, allocated_mb, peak_mb);
    }
}

pub fn log_performance(operation: &str, duration_ms: f64, throughput: Option<f64>) {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.log_performance(operation, duration_ms, throughput);
    }
}

pub fn log_structured(
    component: LogComponent,
    level: Level,
    message: &str,
    metadata: Option<HashMap<String, serde_json::Value>>,
) {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.log_structured(component, level, message, metadata);
    }
}

pub fn get_performance_summary() -> HashMap<String, serde_json::Value> {
    if let Some(logger) = ADVANCED_LOGGER.get() {
        logger.get_performance_summary()
    } else {
        HashMap::new()
    }
}

/// Check if debug logging is enabled
/// This function checks both the DEBUG_ENABLED environment variable
/// and the application settings to determine if debug logging should be active.
pub fn is_debug_enabled() -> bool {
    // Check environment variable first
    if let Ok(val) = std::env::var("DEBUG_ENABLED") {
        return val.parse::<bool>().unwrap_or(false);
    }

    // Fall back to application settings
    if let Ok(settings) = crate::config::AppFullSettings::new() {
        return settings.system.debug.enabled;
    }

    // Default to false if neither is available
    false
}

// Convenience macros for structured logging
#[macro_export]
macro_rules! log_gpu {
    ($kernel:expr, $time_us:expr, $mem_mb:expr, $peak_mb:expr) => {
        $crate::utils::advanced_logging::log_gpu_kernel($kernel, $time_us, $mem_mb, $peak_mb);
    };
}

#[macro_export]
macro_rules! log_perf {
    ($operation:expr, $duration_ms:expr) => {
        $crate::utils::advanced_logging::log_performance($operation, $duration_ms, None);
    };
    ($operation:expr, $duration_ms:expr, $throughput:expr) => {
        $crate::utils::advanced_logging::log_performance(
            $operation,
            $duration_ms,
            Some($throughput),
        );
    };
}

