//! CUDA Error Handling Module
//!
//! Provides comprehensive error checking and recovery for all CUDA operations.
//! Implements proper error propagation, automatic cleanup, and fallback mechanisms.

use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use log::{error, warn, info, debug};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudaError {
    Success = 0,
    InvalidValue = 1,
    OutOfMemory = 2,
    NotInitialized = 3,
    DeInitialized = 4,
    ProfilerDisabled = 5,
    ProfilerNotInitialized = 6,
    ProfilerAlreadyStarted = 7,
    ProfilerAlreadyStopped = 8,
    InvalidConfiguration = 9,
    InvalidPitchValue = 12,
    InvalidSymbol = 13,
    InvalidHostPointer = 16,
    InvalidDevicePointer = 17,
    InvalidTexture = 18,
    InvalidTextureBinding = 19,
    InvalidChannelDescriptor = 20,
    InvalidMemcpyDirection = 21,
    AddressOfConstant = 22,
    TextureFetchFailed = 23,
    TextureNotBound = 24,
    SynchronizationError = 25,
    InvalidFilterSetting = 26,
    InvalidNormSetting = 27,
    MixedDeviceExecution = 28,
    CudartUnloading = 29,
    Unknown = 30,
    NotYetImplemented = 31,
    MemoryValueTooLarge = 32,
    InvalidResourceHandle = 33,
    NotReady = 34,
    InsufficientDriver = 35,
    SetOnActiveProcess = 36,
    InvalidSurface = 37,
    NoDevice = 38,
    ECCUncorrectable = 39,
    SharedObjectSymbolNotFound = 40,
    SharedObjectInitFailed = 41,
    UnsupportedLimit = 42,
    DuplicateVariableName = 43,
    DuplicateTextureName = 44,
    DuplicateSurfaceName = 45,
    DevicesUnavailable = 46,
    IncompatibleDriverContext = 47,
    MissingConfiguration = 48,
    PriorLaunchFailure = 49,
    InvalidDeviceFunction = 50,
    NoKernelImageForDevice = 51,
    InvalidKernelImage = 52,
    NoKernelImageForDevice2 = 53,
    InvalidContext = 54,
    ContextAlreadyCurrent = 55,
    MapFailed = 56,
    UnmapFailed = 57,
    ArrayIsMapped = 58,
    AlreadyMapped = 59,
    NoBinaryForGpu = 60,
    AlreadyAcquired = 61,
    NotMapped = 62,
    NotMappedAsArray = 63,
    NotMappedAsPointer = 64,
    ECCUnavailable = 65,
    UnsupportedLimit2 = 66,
    DeviceAlreadyInUse = 67,
    PeerAccessUnsupported = 68,
    InvalidPtx = 69,
    InvalidGraphicsContext = 70,
    NvlinkUncorrectable = 71,
    JitCompilerNotFound = 72,
    UnsupportedPtxVersion = 73,
    JitCompilationDisabled = 74,
    UnsupportedExecAffinity = 75,
    LaunchFailure = 719,
    UnknownError = 999,
}

impl From<c_int> for CudaError {
    fn from(code: c_int) -> Self {
        match code {
            0 => CudaError::Success,
            1 => CudaError::InvalidValue,
            2 => CudaError::OutOfMemory,
            3 => CudaError::NotInitialized,
            4 => CudaError::DeInitialized,
            5 => CudaError::ProfilerDisabled,
            6 => CudaError::ProfilerNotInitialized,
            7 => CudaError::ProfilerAlreadyStarted,
            8 => CudaError::ProfilerAlreadyStopped,
            9 => CudaError::InvalidConfiguration,
            12 => CudaError::InvalidPitchValue,
            13 => CudaError::InvalidSymbol,
            16 => CudaError::InvalidHostPointer,
            17 => CudaError::InvalidDevicePointer,
            18 => CudaError::InvalidTexture,
            19 => CudaError::InvalidTextureBinding,
            20 => CudaError::InvalidChannelDescriptor,
            21 => CudaError::InvalidMemcpyDirection,
            22 => CudaError::AddressOfConstant,
            23 => CudaError::TextureFetchFailed,
            24 => CudaError::TextureNotBound,
            25 => CudaError::SynchronizationError,
            26 => CudaError::InvalidFilterSetting,
            27 => CudaError::InvalidNormSetting,
            28 => CudaError::MixedDeviceExecution,
            29 => CudaError::CudartUnloading,
            30 => CudaError::Unknown,
            31 => CudaError::NotYetImplemented,
            32 => CudaError::MemoryValueTooLarge,
            33 => CudaError::InvalidResourceHandle,
            34 => CudaError::NotReady,
            35 => CudaError::InsufficientDriver,
            36 => CudaError::SetOnActiveProcess,
            37 => CudaError::InvalidSurface,
            38 => CudaError::NoDevice,
            39 => CudaError::ECCUncorrectable,
            40 => CudaError::SharedObjectSymbolNotFound,
            41 => CudaError::SharedObjectInitFailed,
            42 => CudaError::UnsupportedLimit,
            43 => CudaError::DuplicateVariableName,
            44 => CudaError::DuplicateTextureName,
            45 => CudaError::DuplicateSurfaceName,
            46 => CudaError::DevicesUnavailable,
            47 => CudaError::IncompatibleDriverContext,
            48 => CudaError::MissingConfiguration,
            49 => CudaError::PriorLaunchFailure,
            50 => CudaError::InvalidDeviceFunction,
            51 => CudaError::NoKernelImageForDevice,
            52 => CudaError::InvalidKernelImage,
            53 => CudaError::NoKernelImageForDevice2,
            54 => CudaError::InvalidContext,
            55 => CudaError::ContextAlreadyCurrent,
            56 => CudaError::MapFailed,
            57 => CudaError::UnmapFailed,
            58 => CudaError::ArrayIsMapped,
            59 => CudaError::AlreadyMapped,
            60 => CudaError::NoBinaryForGpu,
            61 => CudaError::AlreadyAcquired,
            62 => CudaError::NotMapped,
            63 => CudaError::NotMappedAsArray,
            64 => CudaError::NotMappedAsPointer,
            65 => CudaError::ECCUnavailable,
            66 => CudaError::UnsupportedLimit2,
            67 => CudaError::DeviceAlreadyInUse,
            68 => CudaError::PeerAccessUnsupported,
            69 => CudaError::InvalidPtx,
            70 => CudaError::InvalidGraphicsContext,
            71 => CudaError::NvlinkUncorrectable,
            72 => CudaError::JitCompilerNotFound,
            73 => CudaError::UnsupportedPtxVersion,
            74 => CudaError::JitCompilationDisabled,
            75 => CudaError::UnsupportedExecAffinity,
            719 => CudaError::LaunchFailure,
            _ => CudaError::UnknownError,
        }
    }
}

impl std::fmt::Display for CudaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CudaError::Success => write!(f, "CUDA operation completed successfully"),
            CudaError::InvalidValue => write!(f, "CUDA invalid value error"),
            CudaError::OutOfMemory => write!(f, "CUDA out of memory error"),
            CudaError::NotInitialized => write!(f, "CUDA not initialized error"),
            CudaError::DeInitialized => write!(f, "CUDA deinitialized error"),
            CudaError::LaunchFailure => write!(f, "CUDA kernel launch failure"),
            CudaError::NoDevice => write!(f, "CUDA no device available"),
            CudaError::InvalidConfiguration => write!(f, "CUDA invalid configuration"),
            CudaError::InvalidDevicePointer => write!(f, "CUDA invalid device pointer"),
            CudaError::InvalidHostPointer => write!(f, "CUDA invalid host pointer"),
            CudaError::SynchronizationError => write!(f, "CUDA synchronization error"),
            _ => write!(f, "CUDA error: {:?}", self),
        }
    }
}

impl std::error::Error for CudaError {}

#[derive(Debug, Clone, Copy)]
pub enum RecoveryStrategy {
    
    Retry,
    
    FallbackToCPU,
    
    ResetContext,
    
    Abort,
}

pub struct CudaErrorHandler {
    error_count: Arc<AtomicU32>,
    last_error_time: Arc<std::sync::Mutex<Option<Instant>>>,
    max_errors_per_minute: u32,
    fallback_threshold: u32,
    context_reset_threshold: u32,
}

impl CudaErrorHandler {
    pub fn new() -> Self {
        Self {
            error_count: Arc::new(AtomicU32::new(0)),
            last_error_time: Arc::new(std::sync::Mutex::new(None)),
            max_errors_per_minute: 10,
            fallback_threshold: 5,
            context_reset_threshold: 15,
        }
    }

    /// Check for CUDA errors from the last operation
    pub fn check_error(&self, operation_name: &str) -> Result<(), CudaError> {
        // SAFETY: cudaGetLastError() is safe to call because:
        // 1. It is a read-only FFI call that queries the CUDA runtime's last error state
        // 2. It does not modify any memory or device state
        // 3. The CUDA runtime is initialized before any operations that could set errors
        // 4. The returned error code is immediately converted to a safe Rust enum
        let error_code = unsafe { cudaGetLastError() };
        let cuda_error = CudaError::from(error_code);

        if cuda_error == CudaError::Success {
            return Ok(());
        }

        
        let error_count = self.error_count.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();

        
        if let Ok(mut last_time) = self.last_error_time.lock() {
            *last_time = Some(now);
        }

        error!("CUDA error in {}: {} (error #{} total)", operation_name, cuda_error, error_count + 1);

        
        let strategy = self.determine_recovery_strategy(&cuda_error, error_count + 1);

        match strategy {
            RecoveryStrategy::Retry => {
                warn!("Attempting to retry {} after CUDA error", operation_name);
                // SAFETY: cudaGetLastError() is safe to call because:
                // 1. It clears the last error state in the CUDA runtime (idempotent operation)
                // 2. No memory is accessed or modified beyond the CUDA runtime's internal state
                // 3. This prepares the runtime for a clean retry attempt
                unsafe { cudaGetLastError(); }
                return Err(cuda_error);
            }
            RecoveryStrategy::FallbackToCPU => {
                warn!("Falling back to CPU for {} due to repeated CUDA errors", operation_name);
                return Err(cuda_error);
            }
            RecoveryStrategy::ResetContext => {
                warn!("Resetting CUDA context for {} due to critical error", operation_name);
                self.reset_cuda_context();
                return Err(cuda_error);
            }
            RecoveryStrategy::Abort => {
                error!("Aborting {} due to unrecoverable CUDA error", operation_name);
                return Err(cuda_error);
            }
        }
    }

    /// Synchronize the CUDA device, ensuring all previous operations complete
    pub fn synchronize_device(&self, operation_name: &str) -> Result<(), CudaError> {
        // SAFETY: cudaDeviceSynchronize() is safe to call because:
        // 1. It blocks until all previously issued CUDA operations complete
        // 2. It does not take any pointer arguments or modify user memory
        // 3. The CUDA runtime manages all synchronization internally
        // 4. Any errors are returned as error codes, not via undefined behavior
        unsafe {
            let result = cudaDeviceSynchronize();
            if result != 0 {
                let cuda_error = CudaError::from(result);
                error!("CUDA synchronization failed in {}: {}", operation_name, cuda_error);
                return Err(cuda_error);
            }
        }

        
        self.check_error(&format!("{}_sync", operation_name))
    }

    
    pub fn get_error_stats(&self) -> (u32, Option<Duration>) {
        let error_count = self.error_count.load(Ordering::Relaxed);
        let time_since_last = if let Ok(last_time) = self.last_error_time.lock() {
            last_time.map(|t| t.elapsed())
        } else {
            None
        };

        (error_count, time_since_last)
    }

    
    pub fn reset_stats(&self) {
        self.error_count.store(0, Ordering::Relaxed);
        if let Ok(mut last_time) = self.last_error_time.lock() {
            *last_time = None;
        }
        info!("CUDA error statistics reset");
    }

    
    pub fn should_fallback_to_cpu(&self) -> bool {
        let error_count = self.error_count.load(Ordering::Relaxed);
        error_count >= self.fallback_threshold
    }

    fn determine_recovery_strategy(&self, error: &CudaError, error_count: u32) -> RecoveryStrategy {
        match error {
            
            CudaError::OutOfMemory | CudaError::MemoryValueTooLarge => {
                if error_count >= 2 {
                    RecoveryStrategy::FallbackToCPU
                } else {
                    RecoveryStrategy::Retry
                }
            }

            
            CudaError::NotInitialized | CudaError::DeInitialized | CudaError::InvalidContext => {
                if error_count >= self.context_reset_threshold {
                    RecoveryStrategy::Abort
                } else {
                    RecoveryStrategy::ResetContext
                }
            }

            
            CudaError::LaunchFailure | CudaError::InvalidConfiguration => {
                if error_count >= 3 {
                    RecoveryStrategy::FallbackToCPU
                } else {
                    RecoveryStrategy::Retry
                }
            }

            
            CudaError::NoDevice | CudaError::DevicesUnavailable => {
                RecoveryStrategy::FallbackToCPU
            }

            
            CudaError::ECCUncorrectable | CudaError::NvlinkUncorrectable => {
                RecoveryStrategy::Abort
            }

            
            _ => {
                if error_count >= self.fallback_threshold {
                    RecoveryStrategy::FallbackToCPU
                } else {
                    RecoveryStrategy::Retry
                }
            }
        }
    }

    fn reset_cuda_context(&self) {
        warn!("Attempting CUDA context reset");
        // SAFETY: cudaDeviceReset() is safe to call because:
        // 1. It destroys all allocations and resets all state on the current device
        // 2. All CUDA resources (buffers, streams, events) become invalid after this call
        // 3. The caller (CudaErrorHandler) does not hold any CUDA resources directly
        // 4. This is only called as a recovery mechanism after severe errors
        // 5. After reset, the CUDA runtime will reinitialize on the next CUDA call
        // WARNING: Any code holding CudaMemoryGuard or other CUDA resources MUST
        // ensure those resources are not used after context reset
        unsafe {
            let result = cudaDeviceReset();
            if result == 0 {
                info!("CUDA context reset successfully");
                self.error_count.store(0, Ordering::Relaxed);
            } else {
                error!("Failed to reset CUDA context: error code {}", result);
            }
        }
    }
}

impl Default for CudaErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CudaMemoryGuard {
    ptr: *mut c_void,
    size: usize,
    name: String,
    error_handler: Arc<CudaErrorHandler>,
}

impl CudaMemoryGuard {
    pub fn new(size: usize, name: String, error_handler: Arc<CudaErrorHandler>) -> Result<Self, CudaError> {
        let mut ptr: *mut c_void = std::ptr::null_mut();

        // SAFETY: cudaMalloc is safe to call because:
        // 1. We pass a valid mutable pointer to receive the allocated device pointer
        // 2. `size` is a valid usize representing the requested allocation size
        // 3. On success, cudaMalloc writes a valid device pointer to `ptr`
        // 4. On failure, cudaMalloc returns an error code and `ptr` remains null
        // 5. The allocated memory is tracked and will be freed in Drop
        unsafe {
            let result = cudaMalloc(&mut ptr as *mut *mut c_void, size);
            if result != 0 {
                let cuda_error = CudaError::from(result);
                error!("Failed to allocate {} bytes for {}: {}", size, name, cuda_error);
                return Err(cuda_error);
            }
        }

        info!("Allocated {} bytes for {} at {:?}", size, name, ptr);

        Ok(Self {
            ptr,
            size,
            name,
            error_handler,
        })
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Copy data from host memory to this GPU buffer.
    ///
    /// # Safety
    /// - `host_data` must be a valid pointer to at least `size` bytes of readable memory
    /// - The memory at `host_data` must remain valid and unmodified during the copy
    /// - `host_data` must be properly aligned for the data type being copied
    pub unsafe fn copy_from_host(&self, host_data: *const c_void, size: usize) -> Result<(), CudaError> {
        if size > self.size {
            error!("Attempting to copy {} bytes to buffer of size {}", size, self.size);
            return Err(CudaError::InvalidValue);
        }

        // SAFETY: cudaMemcpy (host-to-device) is safe here because:
        // 1. `self.ptr` is a valid device pointer allocated via cudaMalloc in Self::new()
        // 2. `host_data` validity is guaranteed by the caller (per function's safety contract)
        // 3. `size` has been verified to not exceed the allocated buffer size
        // 4. CUDA_MEMCPY_HOST_TO_DEVICE is the correct direction enum for this operation
        // 5. The copy is synchronous - host_data can be modified after this call returns
        unsafe {
            let result = cudaMemcpy(self.ptr, host_data, size, CUDA_MEMCPY_HOST_TO_DEVICE);
            if result != 0 {
                let cuda_error = CudaError::from(result);
                error!("Failed to copy {} bytes to {}: {}", size, self.name, cuda_error);
                return Err(cuda_error);
            }
        }

        self.error_handler.check_error(&format!("copy_to_{}", self.name))?;

        debug!("Copied {} bytes to {}", size, self.name);
        Ok(())
    }

    /// Copy data from this GPU buffer to host memory.
    ///
    /// # Safety
    /// - `host_data` must be a valid pointer to at least `size` bytes of writable memory
    /// - The memory at `host_data` must be exclusively owned by the caller during the copy
    /// - `host_data` must be properly aligned for the data type being copied
    pub unsafe fn copy_to_host(&self, host_data: *mut c_void, size: usize) -> Result<(), CudaError> {
        if size > self.size {
            error!("Attempting to copy {} bytes from buffer of size {}", size, self.size);
            return Err(CudaError::InvalidValue);
        }

        // SAFETY: cudaMemcpy (device-to-host) is safe here because:
        // 1. `host_data` validity is guaranteed by the caller (per function's safety contract)
        // 2. `self.ptr` is a valid device pointer allocated via cudaMalloc in Self::new()
        // 3. `size` has been verified to not exceed the allocated buffer size
        // 4. CUDA_MEMCPY_DEVICE_TO_HOST is the correct direction enum for this operation
        // 5. The copy is synchronous - host_data contains valid data after this call returns
        unsafe {
            let result = cudaMemcpy(host_data, self.ptr, size, CUDA_MEMCPY_DEVICE_TO_HOST);
            if result != 0 {
                let cuda_error = CudaError::from(result);
                error!("Failed to copy {} bytes from {}: {}", size, self.name, cuda_error);
                return Err(cuda_error);
            }
        }

        self.error_handler.check_error(&format!("copy_from_{}", self.name))?;

        debug!("Copied {} bytes from {}", size, self.name);
        Ok(())
    }
}

impl Drop for CudaMemoryGuard {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: cudaFree is safe to call because:
            // 1. `self.ptr` was allocated via cudaMalloc in Self::new() and is non-null
            // 2. This is the only place where this pointer is freed (single ownership)
            // 3. After Drop completes, `self.ptr` is no longer accessible
            // 4. cudaFree handles the case where the CUDA context has been destroyed
            // 5. Even if cudaFree fails, the memory is leaked (not double-freed)
            unsafe {
                let result = cudaFree(self.ptr);
                if result != 0 {
                    error!("Failed to free CUDA memory for {}: error code {}", self.name, result);
                } else {
                    debug!("Freed {} bytes for {}", self.size, self.name);
                }
            }
        }
    }
}

// External CUDA runtime function declarations
extern "C" {
    fn cudaGetLastError() -> c_int;
    fn cudaDeviceSynchronize() -> c_int;
    fn cudaDeviceReset() -> c_int;
    fn cudaMalloc(devPtr: *mut *mut c_void, size: usize) -> c_int;
    fn cudaFree(devPtr: *mut c_void) -> c_int;
    fn cudaMemcpy(dst: *mut c_void, src: *const c_void, count: usize, kind: c_int) -> c_int;
    fn cudaGetErrorString(error: c_int) -> *const c_char;
}

// CUDA memory copy directions
const CUDA_MEMCPY_HOST_TO_DEVICE: c_int = 1;
const CUDA_MEMCPY_DEVICE_TO_HOST: c_int = 2;
const CUDA_MEMCPY_DEVICE_TO_DEVICE: c_int = 3;

#[macro_export]
macro_rules! cuda_check {
    ($handler:expr, $operation:expr, $op_name:expr) => {{
        let result = $operation;
        if result != 0 {
            let cuda_error = CudaError::from(result);
            error!("CUDA operation {} failed: {}", $op_name, cuda_error);
            return Err(cuda_error);
        }
        $handler.check_error($op_name)?;
    }};
}

static GLOBAL_CUDA_ERROR_HANDLER: std::sync::OnceLock<Arc<CudaErrorHandler>> = std::sync::OnceLock::new();

pub fn get_global_cuda_error_handler() -> Arc<CudaErrorHandler> {
    GLOBAL_CUDA_ERROR_HANDLER
        .get_or_init(|| Arc::new(CudaErrorHandler::new()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cuda_error_conversion() {
        assert_eq!(CudaError::from(0), CudaError::Success);
        assert_eq!(CudaError::from(1), CudaError::InvalidValue);
        assert_eq!(CudaError::from(2), CudaError::OutOfMemory);
        assert_eq!(CudaError::from(999), CudaError::UnknownError);
    }

    #[test]
    fn test_error_handler_creation() {
        let handler = CudaErrorHandler::new();
        let (count, time) = handler.get_error_stats();
        assert_eq!(count, 0);
        assert!(time.is_none());
    }

    #[test]
    fn test_fallback_threshold() {
        let handler = CudaErrorHandler::new();
        assert!(!handler.should_fallback_to_cpu());

        
        for _ in 0..5 {
            handler.error_count.fetch_add(1, Ordering::Relaxed);
        }
        assert!(handler.should_fallback_to_cpu());
    }
}