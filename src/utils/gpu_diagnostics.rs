// use crate::utils::unified_gpu_compute::UnifiedGPUCompute;
use cust::context::Context;
use cust::device::Device;
use cust::module::Module;
use log::{error, info, warn};
use std::env;
use std::io::{Error, ErrorKind};
use std::path::Path;
use visionclaw_gpu::ptx_loader as ptx;

pub fn ptx_module_smoke_test() -> String {
    let mut report = String::new();
    report.push_str("==== GPU PTX MODULE SMOKE TEST ====\n");

    match ptx::load_ptx_sync() {
        Ok(ptx_content) => {
            report.push_str(&format!("PTX loaded ({} bytes)\n", ptx_content.len()));

            let device = match Device::get_device(0) {
                Ok(d) => {
                    report.push_str("CUDA device(0) acquired\n");
                    d
                }
                Err(e) => {
                    report.push_str(&format!("❌ Failed to get CUDA device: {}\n", e));
                    return report;
                }
            };
            let _ctx = match Context::new(device) {
                Ok(c) => {
                    report.push_str("CUDA context created\n");
                    c
                }
                Err(e) => {
                    report.push_str(&format!("❌ Failed to create CUDA context: {}\n", e));
                    return report;
                }
            };

            match Module::from_ptx(&ptx_content, &[]) {
                Ok(module) => {
                    report.push_str("PTX module created successfully\n");

                    let kernels = [
                        "build_grid_kernel",
                        "compute_cell_bounds_kernel",
                        "force_pass_kernel",
                        "integrate_pass_kernel",
                        "relaxation_step_kernel",
                    ];
                    let mut missing = Vec::new();
                    for k in kernels {
                        if module.get_function(k).is_err() {
                            missing.push(k.to_string());
                        }
                    }
                    if missing.is_empty() {
                        report.push_str("Smoke test PASSED: all expected kernels found\n");
                    } else {
                        report.push_str(&format!(
                            "⚠️ Smoke test PARTIAL: missing kernels: {:?}\n",
                            missing
                        ));
                    }
                }
                Err(e) => {
                    let diag = diagnose_ptx_error(&format!("Module::from_ptx error: {}", e));
                    report.push_str(&format!("❌ Failed to create module: {}\n{}", e, diag));
                    return report;
                }
            }
        }
        Err(e) => {
            report.push_str(&format!("❌ Failed to load PTX: {}\n", e));
            return report;
        }
    }
    report
}

pub fn run_gpu_diagnostics() -> String {
    let mut report = String::new();
    report.push_str("==== GPU DIAGNOSTIC REPORT (Phase 0 Enhanced) ====\n");

    report.push_str("PTX Build Environment:\n");
    match std::env::var("VISIONCLAW_PTX_PATH") {
        Ok(path) => {
            report.push_str(&format!("  VISIONCLAW_PTX_PATH = {}\n", path));
            if std::path::Path::new(&path).exists() {
                match std::fs::metadata(&path) {
                    Ok(metadata) => {
                        report.push_str(&format!(
                            "  PTX file exists, size: {} bytes\n",
                            metadata.len()
                        ));
                        info!(
                            "GPU Diagnostic: PTX file exists at {} ({} bytes)",
                            path,
                            metadata.len()
                        );
                    }
                    Err(e) => {
                        report
                            .push_str(&format!("  ❌ PTX file exists but metadata error: {}\n", e));
                        error!("GPU Diagnostic: PTX metadata error: {}", e);
                    }
                }
            } else {
                report.push_str(&format!("  ❌ PTX file does not exist at: {}\n", path));
                error!("GPU Diagnostic: PTX file missing at {}", path);
            }
        }
        Err(_) => {
            report.push_str("  ❌ VISIONCLAW_PTX_PATH not set - build.rs may have failed\n");
            error!("GPU Diagnostic: VISIONCLAW_PTX_PATH environment variable not set");
        }
    }

    report.push_str(&format!(
        "\nEffective fallback CUDA arch (for runtime PTX compile): sm_{}\n",
        ptx::effective_cuda_arch()
    ));
    report.push_str("\nRuntime Environment Variables:\n");
    for var in &[
        "NVIDIA_GPU_UUID",
        "NVIDIA_VISIBLE_DEVICES",
        "CUDA_VISIBLE_DEVICES",
    ] {
        match env::var(var) {
            Ok(val) => {
                report.push_str(&format!("  {} = {}\n", var, val));
                info!("GPU Diagnostic: {} = {}", var, val);
            }
            Err(_) => {
                report.push_str(&format!("  {} = <not set>\n", var));
                warn!("GPU Diagnostic: {} not set", var);
            }
        }
    }

    let ptx_paths = [
        "/app/src/utils/ptx/visionclaw_unified.ptx",
        "./src/utils/ptx/visionclaw_unified.ptx",
    ];
    report.push_str("\nPTX File Status:\n");
    let mut ptx_found = false;

    for path in &ptx_paths {
        if Path::new(path).exists() {
            ptx_found = true;
            report.push_str(&format!("  PTX file found at: {}\n", path));
            info!("GPU Diagnostic: PTX file found at {}", path);

            match std::fs::metadata(path) {
                Ok(metadata) => {
                    report.push_str(&format!("     Size: {} bytes\n", metadata.len()));
                    info!("GPU Diagnostic: PTX file size = {} bytes", metadata.len());
                }
                Err(e) => {
                    report.push_str(&format!("     Error getting file info: {}\n", e));
                    warn!("GPU Diagnostic: Error getting PTX file info: {}", e);
                }
            }
        } else {
            report.push_str(&format!("  ❌ PTX file NOT found at: {}\n", path));
            warn!("GPU Diagnostic: PTX file NOT found at {}", path);
        }
    }

    if !ptx_found {
        error!("GPU Diagnostic: No PTX file found at any expected location");

        report.push_str("  ⚠️ CRITICAL ERROR: No PTX file found. GPU physics will not work.\n");
    }

    report.push_str("\nCUDA Device Detection:\n");

    report.push_str("  ⚠️ GPU testing temporarily disabled - cust crate not available\n");

    report.push_str("=============================\n");
    info!("GPU diagnostic report complete");
    report
}

pub fn validate_ptx_content(ptx_content: &str) -> Result<(), String> {
    if ptx_content.trim().is_empty() {
        return Err("PTX content is empty".to_string());
    }

    if !ptx_content.contains(".version") {
        return Err("PTX content missing .version directive".to_string());
    }

    if !ptx_content.contains(".target") {
        return Err("PTX content missing .target directive".to_string());
    }

    let required_kernels = [
        "build_grid_kernel",
        "compute_cell_bounds_kernel",
        "force_pass_kernel",
        "integrate_pass_kernel",
        "relaxation_step_kernel",
    ];

    for kernel in &required_kernels {
        if !ptx_content.contains(kernel) {
            warn!(
                "PTX validation: missing expected kernel function: {}",
                kernel
            );
        }
    }

    info!(
        "PTX validation successful: {} bytes, contains required directives",
        ptx_content.len()
    );
    Ok(())
}

pub fn diagnose_ptx_error(error: &str) -> String {
    let mut diagnosis = String::new();
    diagnosis.push_str("PTX Error Diagnosis:\n");

    if error.contains("device kernel image is invalid") {
        diagnosis.push_str("  ⚠️  'device kernel image is invalid' error detected\n");
        diagnosis.push_str("  Possible causes:\n");
        diagnosis.push_str("    - PTX architecture mismatch (check CUDA_ARCH)\n");
        diagnosis.push_str("    - Corrupted PTX file\n");
        diagnosis.push_str("    - CUDA driver/runtime version mismatch\n");
        diagnosis.push_str("  🛠️  Solutions:\n");
        diagnosis.push_str("    - Rebuild with correct CUDA_ARCH (75, 80, 86, etc.)\n");
        diagnosis.push_str("    - Check CUDA driver version with nvidia-smi\n");
        diagnosis.push_str("    - Verify PTX file integrity\n");
    } else if error.contains("no kernel image is available") {
        diagnosis.push_str("  ⚠️  'no kernel image is available' error detected\n");
        diagnosis.push_str("  Possible causes:\n");
        diagnosis.push_str("    - PTX compilation failed\n");
        diagnosis.push_str("    - Wrong GPU architecture target\n");
        diagnosis.push_str("  🛠️  Solutions:\n");
        diagnosis.push_str("    - Check nvcc compilation output\n");
        diagnosis.push_str("    - Set correct CUDA_ARCH environment variable\n");
    } else if error.contains("Module::from_ptx") {
        diagnosis.push_str("  ⚠️  Module creation from PTX failed\n");
        diagnosis.push_str("  Possible causes:\n");
        diagnosis.push_str("    - Invalid PTX syntax\n");
        diagnosis.push_str("    - Missing kernel functions\n");
        diagnosis.push_str("  🛠️  Solutions:\n");
        diagnosis.push_str("    - Validate PTX content manually\n");
        diagnosis.push_str("    - Check CUDA compilation warnings\n");
    }

    diagnosis.push('\n');
    error!("PTX Error Diagnosed: {}", diagnosis);
    diagnosis
}

pub fn validate_kernel_launch(
    kernel_name: &str,
    grid_size: u32,
    block_size: u32,
    num_nodes: usize,
) -> Result<(), String> {
    if grid_size == 0 {
        return Err(format!("Invalid grid size 0 for kernel {}", kernel_name));
    }

    if block_size == 0 || block_size > 1024 {
        return Err(format!(
            "Invalid block size {} for kernel {} (must be 1-1024)",
            block_size, kernel_name
        ));
    }

    if num_nodes == 0 {
        return Err(format!("Cannot launch kernel {} with 0 nodes", kernel_name));
    }

    let total_threads = grid_size as usize * block_size as usize;
    if total_threads < num_nodes {
        warn!(
            "Kernel {} may have insufficient threads: {} total, {} nodes",
            kernel_name, total_threads, num_nodes
        );
    }

    info!(
        "Kernel launch validation passed: {} (grid: {}, block: {}, nodes: {})",
        kernel_name, grid_size, block_size, num_nodes
    );
    Ok(())
}

pub fn create_gpu_metrics_report() -> String {
    let mut report = String::new();
    report.push_str("==== GPU METRICS REPORT ====\n");

    report.push_str("Memory Usage:\n");
    report.push_str("  Device Memory: N/A (requires CUDA context)\n");
    report.push_str("  Host Memory: N/A (requires implementation)\n");

    report.push_str("\nKernel Performance:\n");
    report.push_str("  Last kernel times: N/A (requires timing implementation)\n");

    report.push_str("\nGPU Utilization:\n");
    report.push_str("  GPU Usage: N/A (requires nvidia-ml-py or similar)\n");

    report.push_str("==============================\n");
    report
}

pub fn fix_cuda_environment() -> Result<(), Error> {
    info!("Attempting to fix CUDA environment...");

    if env::var("CUDA_VISIBLE_DEVICES").is_err() {
        info!("CUDA_VISIBLE_DEVICES not set, setting to 0");

        unsafe { env::set_var("CUDA_VISIBLE_DEVICES", "0") };
    }

    let primary_path = "/app/src/utils/ptx/visionclaw_unified.ptx";
    let alternative_path = "./src/utils/ptx/visionclaw_unified.ptx";

    if !Path::new(primary_path).exists() {
        info!("Primary PTX file not found at {}", primary_path);

        if Path::new(alternative_path).exists() {
            info!(
                "Alternative PTX file found at {}, attempting to create symlink",
                alternative_path
            );

            let alt_path_abs = std::fs::canonicalize(alternative_path)
                .map_err(|e| Error::other(format!("Failed to get canonical path: {}", e)))?;

            let dir_path = Path::new(primary_path)
                .parent()
                .ok_or_else(|| Error::other("Invalid PTX path"))?;

            if !dir_path.exists() {
                std::fs::create_dir_all(dir_path)
                    .map_err(|e| Error::other(format!("Failed to create PTX directory: {}", e)))?;
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&alt_path_abs, primary_path)
                .map_err(|e| Error::other(format!("Failed to create symlink: {}", e)))?;

            #[cfg(not(unix))]
            std::fs::copy(&alt_path_abs, primary_path).map_err(|e| {
                Error::new(ErrorKind::Other, format!("Failed to copy PTX file: {}", e))
            })?;

            info!("Successfully created PTX file at {}", primary_path);
        } else {
            return Err(Error::new(
                ErrorKind::NotFound,
                "No PTX file found anywhere. GPU physics will not work.",
            ));
        }
    }

    info!("CUDA environment has been fixed");
    Ok(())
}
