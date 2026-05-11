// ptx.rs - unified PTX loading and runtime compilation utilities
// This module centralizes PTX acquisition for CUDA kernel modules.
// Strategy:
// 1) Prefer build-time PTX pointed to by environment variables (set by build.rs).
// 2) If unavailable, corrupted, or in Docker (DOCKER_ENV set), compile on-the-fly via nvcc -ptx.
// 3) Support multiple PTX modules for different kernel sets.

use log::{info, warn};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub const DEFAULT_CUDA_ARCH: &str = "75";
pub const CUDA_ARCH_ENV: &str = "CUDA_ARCH";
pub const DOCKER_ENV_VAR: &str = "DOCKER_ENV";

/// Cached runtime GPU compute capability (e.g. "89" for sm_89).
static RUNTIME_CUDA_ARCH: OnceLock<String> = OnceLock::new();

/// Cached max PTX ISA version the installed driver supports.
static RUNTIME_MAX_PTX_ISA: OnceLock<(u32, u32)> = OnceLock::new();

/// Detects the GPU compute capability at runtime by querying `nvidia-smi`.
/// The result is cached for the lifetime of the process.
/// Falls back to `DEFAULT_CUDA_ARCH` ("75") if detection fails.
pub fn detect_runtime_cuda_arch() -> &'static str {
    RUNTIME_CUDA_ARCH.get_or_init(|| {
        // Environment variable override takes highest priority
        if let Ok(env_arch) = std::env::var(CUDA_ARCH_ENV) {
            info!("Using CUDA arch from {} env var: sm_{}", CUDA_ARCH_ENV, env_arch);
            return env_arch;
        }

        match Command::new("nvidia-smi")
            .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // nvidia-smi returns e.g. "8.9\n" — strip and convert "8.9" -> "89"
                let trimmed = stdout.trim();
                // Take only the first GPU if multiple are present
                let first_line = trimmed.lines().next().unwrap_or(trimmed);
                let arch = first_line.replace('.', "");
                if arch.chars().all(|c| c.is_ascii_digit()) && !arch.is_empty() {
                    info!("Detected runtime GPU compute capability: sm_{}", arch);
                    arch
                } else {
                    warn!(
                        "nvidia-smi returned unparseable compute capability '{}', falling back to sm_{}",
                        first_line, DEFAULT_CUDA_ARCH
                    );
                    DEFAULT_CUDA_ARCH.to_string()
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    "nvidia-smi failed (exit {:?}): {}. Falling back to sm_{}",
                    output.status.code(),
                    stderr.trim(),
                    DEFAULT_CUDA_ARCH
                );
                DEFAULT_CUDA_ARCH.to_string()
            }
            Err(e) => {
                warn!(
                    "Failed to run nvidia-smi: {}. Falling back to sm_{}",
                    e, DEFAULT_CUDA_ARCH
                );
                DEFAULT_CUDA_ARCH.to_string()
            }
        }
    })
}

/// Detects the maximum PTX ISA version the installed CUDA driver supports.
/// Parses `nvidia-smi` output to determine the CUDA driver version and maps it
/// to the highest PTX ISA the driver can JIT-compile.
///
/// Mapping (CUDA driver major.minor -> max PTX ISA):
///   - 13.0 -> (9, 0)
///   - 13.1 -> (9, 1)
///   - 13.2+ -> (9, 2)
///   - 12.x  -> (8, x) approximately; we cap at (8, 5) as a safe default
///   - Unknown / older -> (9, 0) as a conservative fallback
///
/// The result is cached for the lifetime of the process.
pub fn detect_max_ptx_isa() -> (u32, u32) {
    *RUNTIME_MAX_PTX_ISA.get_or_init(|| {
        match Command::new("nvidia-smi")
            .args(["--query-gpu=driver_version", "--format=csv,noheader"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                let first_line = trimmed.lines().next().unwrap_or(trimmed);
                parse_driver_to_max_isa(first_line)
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    "nvidia-smi driver query failed (exit {:?}): {}. Using ISA fallback (9, 0)",
                    output.status.code(),
                    stderr.trim()
                );
                (9, 0)
            }
            Err(e) => {
                warn!(
                    "Failed to run nvidia-smi for driver detection: {}. Using ISA fallback (9, 0)",
                    e
                );
                (9, 0)
            }
        }
    })
}

/// Parses a driver version string (e.g. "560.35.03" or "13.0.1") into the
/// CUDA toolkit major.minor and maps it to the max supported PTX ISA.
///
/// nvidia-smi driver_version returns the actual driver version (e.g. "560.35.03"),
/// but we also handle the CUDA version string format for robustness.
/// The CUDA version is available from `nvidia-smi` header or can be queried separately.
fn parse_driver_to_max_isa(driver_version: &str) -> (u32, u32) {
    // Try to get CUDA version directly (more reliable mapping)
    if let Some(cuda_isa) = query_cuda_version_isa() {
        return cuda_isa;
    }

    // Fallback: parse driver version and use known driver-to-CUDA mappings
    let parts: Vec<&str> = driver_version.split('.').collect();
    if parts.len() >= 2 {
        if let Ok(driver_major) = parts[0].parse::<u32>() {
            // NVIDIA driver major version to CUDA toolkit mapping (approximate):
            // Driver 560+ -> CUDA 13.x
            // Driver 550+ -> CUDA 12.8+
            // Driver 535+ -> CUDA 12.2+
            // Driver 525+ -> CUDA 12.0+
            if driver_major >= 560 {
                // CUDA 13.x territory - query more precisely
                return query_cuda_version_isa().unwrap_or((9, 0));
            } else if driver_major >= 525 {
                // CUDA 12.x territory
                return (8, 5);
            }
        }
    }

    info!(
        "Could not map driver version '{}' to PTX ISA, defaulting to (9, 0)",
        driver_version
    );
    (9, 0)
}

/// Queries `nvidia-smi` for the CUDA version and maps it to max PTX ISA.
fn query_cuda_version_isa() -> Option<(u32, u32)> {
    // nvidia-smi prints "CUDA Version: XX.Y" in its default output
    let output = Command::new("nvidia-smi").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Look for "CUDA Version: XX.Y"
    let cuda_marker = "CUDA Version: ";
    let pos = stdout.find(cuda_marker)?;
    let after = &stdout[pos + cuda_marker.len()..];
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(after.len());
    let ver_str = &after[..end];
    let parts: Vec<&str> = ver_str.split('.').collect();
    if parts.len() >= 2 {
        let major: u32 = parts[0].parse().ok()?;
        let minor: u32 = parts[1].parse().ok()?;
        let isa = cuda_version_to_max_isa(major, minor);
        info!(
            "Detected CUDA version {}.{}, max PTX ISA: {}.{}",
            major, minor, isa.0, isa.1
        );
        return Some(isa);
    }
    None
}

/// Maps CUDA toolkit version to the maximum PTX ISA the driver can JIT-compile.
fn cuda_version_to_max_isa(cuda_major: u32, cuda_minor: u32) -> (u32, u32) {
    match (cuda_major, cuda_minor) {
        (13, 0) => (9, 0),
        (13, 1) => (9, 1),
        (13, minor) if minor >= 2 => (9, 2),
        (12, minor) if minor >= 6 => (8, 5),
        (12, minor) if minor >= 4 => (8, 4),
        (12, minor) if minor >= 2 => (8, 2),
        (12, _) => (8, 0),
        (11, _) => (7, 8),
        // Future-proof: if CUDA 14+ appears, allow ISA 9.2 as a safe ceiling
        (major, _) if major > 13 => (9, 2),
        _ => (9, 0), // conservative default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PTXModule {
    VisionflowUnified,
    GpuClusteringKernels,
    DynamicGrid,
    GpuAabbReduction,
    GpuLandmarkApsp,
    SsspCompact,
    VisionflowUnifiedStability,
    OntologyConstraints,
    Pagerank,
    GpuConnectedComponents,
}

impl PTXModule {
    pub fn source_file(&self) -> &'static str {
        match self {
            PTXModule::VisionflowUnified => "visionflow_unified.cu",
            PTXModule::GpuClusteringKernels => "gpu_clustering_kernels.cu",
            PTXModule::DynamicGrid => "dynamic_grid.cu",
            PTXModule::GpuAabbReduction => "gpu_aabb_reduction.cu",
            PTXModule::GpuLandmarkApsp => "gpu_landmark_apsp.cu",
            PTXModule::SsspCompact => "sssp_compact.cu",
            PTXModule::VisionflowUnifiedStability => "visionflow_unified_stability.cu",
            PTXModule::OntologyConstraints => "ontology_constraints.cu",
            PTXModule::Pagerank => "pagerank.cu",
            PTXModule::GpuConnectedComponents => "gpu_connected_components.cu",
        }
    }

    pub fn env_var(&self) -> &'static str {
        match self {
            PTXModule::VisionflowUnified => "VISIONFLOW_UNIFIED_PTX_PATH",
            PTXModule::GpuClusteringKernels => "GPU_CLUSTERING_KERNELS_PTX_PATH",
            PTXModule::DynamicGrid => "DYNAMIC_GRID_PTX_PATH",
            PTXModule::GpuAabbReduction => "GPU_AABB_REDUCTION_PTX_PATH",
            PTXModule::GpuLandmarkApsp => "GPU_LANDMARK_APSP_PTX_PATH",
            PTXModule::SsspCompact => "SSSP_COMPACT_PTX_PATH",
            PTXModule::VisionflowUnifiedStability => "VISIONFLOW_UNIFIED_STABILITY_PTX_PATH",
            PTXModule::OntologyConstraints => "ONTOLOGY_CONSTRAINTS_PTX_PATH",
            PTXModule::Pagerank => "PAGERANK_PTX_PATH",
            PTXModule::GpuConnectedComponents => "GPU_CONNECTED_COMPONENTS_PTX_PATH",
        }
    }

    pub fn all_modules() -> Vec<PTXModule> {
        vec![
            PTXModule::VisionflowUnified,
            PTXModule::GpuClusteringKernels,
            PTXModule::DynamicGrid,
            PTXModule::GpuAabbReduction,
            PTXModule::GpuLandmarkApsp,
            PTXModule::SsspCompact,
            PTXModule::VisionflowUnifiedStability,
            PTXModule::OntologyConstraints,
            PTXModule::Pagerank,
            PTXModule::GpuConnectedComponents,
        ]
    }
}

// Build-time exported paths and content hashes from build.rs (if present)
pub static COMPILED_PTX_PATH: Option<&'static str> = option_env!("VISIONFLOW_UNIFIED_PTX_PATH");

/// Content hashes of CUDA source files at build time. Used to detect stale PTX
/// from Docker cached target volumes when bind-mounted source has changed.
pub static VISIONFLOW_UNIFIED_CUDA_HASH: Option<&'static str> = option_env!("VISIONFLOW_UNIFIED_CUDA_HASH");
pub static GPU_CLUSTERING_KERNELS_CUDA_HASH: Option<&'static str> = option_env!("GPU_CLUSTERING_KERNELS_CUDA_HASH");

/// Verify CUDA source hash matches the build-time hash. Returns true if hashes
/// match or if verification is unavailable (no hash compiled in).
/// Verify CUDA source hash using BLAKE3 for cross-Rust-version stability (P1-24).
pub fn verify_cuda_source_hash(module: PTXModule) -> bool {
    let build_hash = match module {
        PTXModule::VisionflowUnified => VISIONFLOW_UNIFIED_CUDA_HASH,
        PTXModule::GpuClusteringKernels => GPU_CLUSTERING_KERNELS_CUDA_HASH,
        _ => None, // Other modules don't have hash verification yet
    };

    let Some(expected) = build_hash else { return true };

    let source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/utils")
        .join(module.source_file());

    match std::fs::read(&source_path) {
        Ok(contents) => {
            let hash = blake3::hash(&contents);
            let actual = &hash.to_hex()[..16];
            if actual != expected {
                warn!(
                    "CUDA source hash mismatch for {:?}: build={} runtime={}. PTX may be stale!",
                    module, expected, actual
                );
                return false;
            }
            true
        }
        Err(_) => true, // Can't read source (e.g. production) — trust the build
    }
}

pub fn get_compiled_ptx_path(module: PTXModule) -> Option<PathBuf> {
    std::env::var(module.env_var()).ok().map(PathBuf::from)
}

pub fn get_compiled_ptx_path_legacy() -> Option<PathBuf> {
    COMPILED_PTX_PATH.map(PathBuf::from)
}

/// Returns the effective CUDA architecture for compilation.
/// Priority: CUDA_ARCH env var > runtime GPU detection > DEFAULT_CUDA_ARCH.
pub fn effective_cuda_arch() -> String {
    detect_runtime_cuda_arch().to_string()
}

/// Downgrades the PTX ISA version header when it exceeds what the installed driver
/// can handle.  This avoids `CUDA_ERROR_INVALID_PTX` (222) on systems where the
/// toolkit is newer than the driver.
///
/// The max supported ISA is detected at runtime via `detect_max_ptx_isa()` which
/// queries the installed CUDA driver version. Falls back to ISA 9.0 if detection
/// fails.
pub fn downgrade_ptx_isa_if_needed(ptx: String) -> String {
    let (max_major, max_minor) = detect_max_ptx_isa();
    let max_isa_str = format!("{}.{}", max_major, max_minor);

    // Find the .version directive and check if it needs downgrading
    if let Some(ver_start) = ptx.find(".version ") {
        let after = &ptx[ver_start + 9..];
        // Extract the version string (e.g. "9.2")
        let ver_end = after.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(after.len());
        let ver_str = &after[..ver_end];
        let parts: Vec<&str> = ver_str.split('.').collect();
        if parts.len() == 2 {
            if let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                if major > max_major || (major == max_major && minor > max_minor) {
                    let old_directive = format!(".version {}", ver_str);
                    let new_directive = format!(".version {}", max_isa_str);
                    let fixed = ptx.replacen(&old_directive, &new_directive, 1);
                    info!(
                        "PTX ISA downgrade: {} -> {} (driver supports up to {}.{})",
                        old_directive, new_directive, max_major, max_minor
                    );
                    return fixed;
                }
            }
        }
    }
    ptx
}

/// Validates PTX assembly code structure
pub fn validate_ptx(ptx: &str) -> Result<(), String> {
    if !ptx.contains(".version") {
        return Err("PTX validation failed: missing .version directive".into());
    }
    if !ptx.contains(".target") {
        return Err("PTX validation failed: missing .target directive".into());
    }
    if !ptx.contains(".entry ") {
        return Err("PTX validation failed: missing kernel entry points (.entry directive)".into());
    }
    Ok(())
}

pub fn load_ptx_module_sync(module: PTXModule) -> Result<String, String> {
    info!("load_ptx_module_sync: Loading PTX for {:?}", module);

    let raw = load_ptx_module_sync_raw(module)?;
    Ok(downgrade_ptx_isa_if_needed(raw))
}

fn load_ptx_module_sync_raw(module: PTXModule) -> Result<String, String> {
    if std::env::var(DOCKER_ENV_VAR).is_ok() {
        info!("Docker environment detected, checking for pre-compiled PTX first");

        if let Ok(content) = load_precompiled_ptx(module) {
            return Ok(content);
        }

        info!("Pre-compiled PTX not found, using runtime compilation");
        return compile_ptx_fallback_sync_module(module);
    }

    if let Some(path) = get_compiled_ptx_path(module) {
        match fs::read_to_string(&path) {
            Ok(content) => {
                if let Err(e) = validate_ptx(&content) {
                    warn!(
                        "Build-time PTX at {} failed validation: {}. Trying alternatives.",
                        path.display(),
                        e
                    );
                } else {
                    info!("Loaded build-time PTX from {}", path.display());
                    return Ok(content);
                }
            }
            Err(read_err) => {
                warn!(
                    "Failed to read build-time PTX at {}: {}. Trying alternatives.",
                    path.display(),
                    read_err
                );
            }
        }
    }

    if let Ok(content) = load_precompiled_ptx(module) {
        return Ok(content);
    }

    warn!(
        "No pre-compiled PTX found for {:?}. Falling back to runtime compile.",
        module
    );
    compile_ptx_fallback_sync_module(module)
}

fn load_precompiled_ptx(module: PTXModule) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ptx_file = module.source_file().replace(".cu", ".ptx");

    // Check env var override first, then build output (always fresh), then
    // pre-compiled source tree copies (may be stale in Docker image layers).
    let mut ptx_paths = Vec::new();

    // 1. Environment variable override (highest priority)
    if let Ok(env_path) = std::env::var(module.env_var()) {
        ptx_paths.push(PathBuf::from(env_path));
    }

    // 2. Build output directory (recompiled by build.rs from current .cu source)
    //    OUT_DIR is only available at build time, so scan target/*/build/*/out/
    for profile in &["release", "debug"] {
        let build_dir = PathBuf::from(manifest_dir).join("target").join(profile).join("build");
        if let Ok(entries) = std::fs::read_dir(&build_dir) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("out").join(&ptx_file);
                if candidate.exists() {
                    ptx_paths.push(candidate);
                }
            }
        }
    }

    // 3. Pre-compiled source tree copies (may be stale in Docker overlay)
    ptx_paths.extend([
        PathBuf::from(manifest_dir).join("src/utils/ptx").join(&ptx_file),
        PathBuf::from("/app/src/utils/ptx").join(&ptx_file),
        PathBuf::from("./src/utils/ptx").join(&ptx_file),
    ]);

    for path in ptx_paths {
        if let Ok(content) = fs::read_to_string(&path) {
            if validate_ptx(&content).is_ok() {
                info!("Loaded pre-compiled PTX from {}", path.display());
                return Ok(content);
            }
        }
    }

    Err(format!("Pre-compiled PTX not found for {:?}", module))
}

pub fn load_ptx_sync() -> Result<String, String> {
    load_ptx_module_sync(PTXModule::VisionflowUnified)
}

pub fn load_all_ptx_modules_sync() -> Result<HashMap<PTXModule, String>, String> {
    let mut modules = HashMap::new();
    let mut failed: Vec<(PTXModule, String)> = Vec::new();

    for module in PTXModule::all_modules() {
        match load_ptx_module_sync(module) {
            Ok(content) => {
                info!(
                    "Successfully loaded PTX for {:?}, size: {} bytes",
                    module,
                    content.len()
                );
                modules.insert(module, content);
            }
            Err(e) => {
                warn!(
                    "Failed to load PTX module {:?}: {}. Feature will be unavailable.",
                    module, e
                );
                failed.push((module, e));
            }
        }
    }

    if modules.is_empty() {
        return Err(format!(
            "No PTX modules loaded successfully. Failures: {:?}",
            failed
        ));
    }

    info!(
        "Loaded {}/{} PTX modules",
        modules.len(),
        modules.len() + failed.len()
    );

    Ok(modules)
}

pub async fn load_ptx() -> Result<String, String> {
    
    
    load_ptx_sync()
}

pub fn compile_ptx_fallback_sync_module(module: PTXModule) -> Result<String, String> {
    info!(
        "compile_ptx_fallback_sync_module: Starting runtime PTX compilation for {:?}",
        module
    );
    let arch = effective_cuda_arch();
    info!("Using CUDA architecture: sm_{}", arch);

    
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cu_path = Path::new(manifest_dir)
        .join("src")
        .join("utils")
        .join(module.source_file());

    if !cu_path.exists() {
        return Err(format!(
            "CUDA source not found at {}. Ensure the path is correct.",
            cu_path.display()
        ));
    }

    // Use unique temp filename to avoid race conditions
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_nanos();
    let ptx_file = format!(
        "ptx_{}_{}.ptx",
        module.source_file().replace(".cu", ""),
        unique
    );
    let out_path = std::env::temp_dir().join(&ptx_file);

    let nvcc = "nvcc";
    let arch_flag = format!("-arch=sm_{}", arch);

    let output = Command::new(nvcc)
        .args(["-ptx", "-std=c++17"])
        .arg(arch_flag)
        .arg(&cu_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("Failed to spawn nvcc: {}", e))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "nvcc failed for {:?} (code {:?}). Command: nvcc -ptx -std=c++17 -arch=sm_{} {} -o {}\nstdout:\n{}\nstderr:\n{}",
            module,
            output.status.code(),
            arch,
            cu_path.display(),
            out_path.display(),
            stdout,
            stderr
        ));
    }

    let ptx_content = fs::read_to_string(&out_path).map_err(|e| {
        format!(
            "Failed to read generated PTX at {}: {}",
            out_path.display(),
            e
        )
    })?;

    validate_ptx(&ptx_content)?;
    info!(
        "Successfully compiled PTX for {:?}, size: {} bytes",
        module,
        ptx_content.len()
    );
    Ok(ptx_content)
}

pub fn compile_ptx_fallback_sync() -> Result<String, String> {
    compile_ptx_fallback_sync_module(PTXModule::VisionflowUnified)
}
