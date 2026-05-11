use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Check if GPU feature is enabled
    let gpu_enabled = env::var("CARGO_FEATURE_GPU").is_ok();

    if !gpu_enabled {
        println!("cargo:warning=GPU feature disabled, skipping CUDA compilation");
        return;
    }

    // All CUDA source files that need compilation
    let cuda_files = [
        "src/utils/visionflow_unified.cu",
        "src/utils/gpu_clustering_kernels.cu",
        "src/utils/dynamic_grid.cu",
        "src/utils/gpu_aabb_reduction.cu",
        "src/utils/gpu_landmark_apsp.cu",
        "src/utils/sssp_compact.cu",
        "src/utils/visionflow_unified_stability.cu",
        "src/utils/ontology_constraints.cu",
        "src/utils/semantic_forces.cu",
        "src/utils/pagerank.cu",
        "src/utils/gpu_connected_components.cu",
        "src/utils/nan_guard.cu",
    ];

    // Only rebuild if CUDA files change
    for cuda_file in &cuda_files {
        println!("cargo:rerun-if-changed={}", cuda_file);
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/utils/kernel_timing.cuh");

    // Content-hash CUDA files to detect bind-mount overlay changes that cargo's
    // mtime-based rerun-if-changed misses (Docker image build vs host mount).
    // Export hashes so ptx.rs can verify PTX was built from current source.
    // P1-24: Use BLAKE3 for stable cross-version CUDA source hashing.
    // The runtime side (ptx.rs) uses blake3::hash() with the same 16-char hex format.
    use std::io::Read;
    for cuda_file in &cuda_files {
        if let Ok(mut f) = std::fs::File::open(cuda_file) {
            let mut contents = Vec::new();
            if f.read_to_end(&mut contents).is_ok() {
                let hash = blake3::hash(&contents);
                let hex16 = &hash.to_hex()[..16];
                let stem = Path::new(cuda_file).file_stem().unwrap().to_str().unwrap();
                let env_var = format!("{}_CUDA_HASH", stem.to_uppercase());
                println!("cargo:rustc-env={}={}", env_var, hex16);
            }
        }
    }

    // Get build configuration
    let out_dir = env::var("OUT_DIR").unwrap();
    let cuda_path = env::var("CUDA_PATH")
        .or_else(|_| env::var("CUDA_HOME"))
        .unwrap_or_else(|_| {
            // ADR-070 D1.5: fail clearly if CUDA is not found
            let default = "/opt/cuda";
            if !Path::new(default).exists() {
                panic!(
                    "CUDA_PATH/CUDA_HOME not set and {} does not exist. \
                     Install CUDA toolkit or set CUDA_PATH.",
                    default
                );
            }
            default.to_string()
        });

    // ADR-070 D1.5: Detect nvcc vs driver version mismatch
    if let Ok(nvcc_out) = Command::new("nvcc").arg("--version").output() {
        let nvcc_version = String::from_utf8_lossy(&nvcc_out.stdout);
        if let Ok(smi_out) = Command::new("nvidia-smi")
            .args(["--query-gpu=driver_version", "--format=csv,noheader", "--id=0"])
            .output()
        {
            let driver_version = String::from_utf8_lossy(&smi_out.stdout);
            println!(
                "cargo:warning=CUDA toolkit: {}, Driver: {} — verify PTX JIT compatibility",
                nvcc_version.lines().last().unwrap_or("unknown").trim(),
                driver_version.trim()
            );
        }
    }

    // Determine CUDA architecture.
    // In Docker builds (DOCKER_ENV set), NEVER auto-detect via nvidia-smi because the
    // build machine's GPU (e.g. sm_89) differs from the runtime GPU (e.g. sm_86).
    // Instead, default to sm_75 — a portable baseline whose PTX JIT-compiles to any
    // sm_75+ GPU at runtime.  The CUDA_ARCH env var overrides in all cases.
    let is_docker = env::var("DOCKER_ENV").is_ok();
    let cuda_arch = env::var("CUDA_ARCH").unwrap_or_else(|_| {
        if is_docker {
            println!("Docker build detected — skipping nvidia-smi GPU detection, using portable sm_75");
            return "75".to_string();
        }
        // Native (non-Docker) build: try to auto-detect GPU compute capability
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=compute_cap", "--format=csv,noheader", "--id=0"])
            .output()
        {
            if output.status.success() {
                let raw = String::from_utf8_lossy(&output.stdout);
                if let Some(cap) = raw.lines().next() {
                    let cap = cap.trim();
                    // nvidia-smi returns "8.6" → we need "86"
                    let arch = cap.replace('.', "");
                    if !arch.is_empty() {
                        println!("Auto-detected GPU compute capability: {} (sm_{})", cap, arch);
                        return arch;
                    }
                }
            }
        }
        "75".to_string()
    });
    println!("Using CUDA architecture: sm_{}", cuda_arch);

    // Compile all CUDA files to PTX
    println!("Compiling {} CUDA kernels to PTX...", cuda_files.len());

    for cuda_file in &cuda_files {
        let cuda_src = Path::new(cuda_file);
        let file_name = cuda_src.file_stem().unwrap().to_str().unwrap();
        let ptx_output = PathBuf::from(&out_dir).join(format!("{}.ptx", file_name));

        println!("Compiling {} to PTX...", file_name);
        println!(
            "NVCC Command: nvcc -ptx -arch sm_{} -o {} {} --use_fast_math -O3",
            cuda_arch,
            ptx_output.display(),
            cuda_src.display()
        );

        let nvcc_output = Command::new("nvcc")
            .args([
                "-ptx",
                "-arch",
                &format!("sm_{}", cuda_arch),
                "-o",
                ptx_output.to_str().unwrap(),
                cuda_src.to_str().unwrap(),
                "--use_fast_math",
                "-O3",
            ])
            .output()
            .expect("Failed to execute nvcc - is CUDA toolkit installed and in PATH?");

        if !nvcc_output.status.success() {
            eprintln!(
                "NVCC STDOUT: {}",
                String::from_utf8_lossy(&nvcc_output.stdout)
            );
            eprintln!(
                "NVCC STDERR: {}",
                String::from_utf8_lossy(&nvcc_output.stderr)
            );
            panic!("CUDA PTX compilation failed for {} with exit code: {:?}. Check CUDA installation and source file.",
                   file_name, nvcc_output.status.code());
        }

        // ADR-070 D1.5: Downgrade PTX ISA version to 9.0 for driver compatibility.
        // CUDA toolkit 13.x emits .version 9.x but the host driver may only JIT up to 9.0.
        // This is safe: sm_86 kernels don't use ISA 9.1+ features.
        // Fixed: previous substring code could panic if PTX file ended early.
        if let Ok(ptx_text) = std::fs::read_to_string(&ptx_output) {
            if let Some(pos) = ptx_text.find(".version 9.") {
                let end = (pos + 13).min(ptx_text.len());
                let version_str = &ptx_text[pos..end];
                if !version_str.starts_with(".version 9.0") {
                    // Find the end of the version directive (next whitespace or newline)
                    let directive_end = ptx_text[pos..]
                        .find(|c: char| c == '\n' || c == '\r')
                        .map(|off| pos + off)
                        .unwrap_or(ptx_text.len());
                    let fixed = format!(
                        "{}.version 9.0{}",
                        &ptx_text[..pos],
                        &ptx_text[directive_end..]
                    );
                    std::fs::write(&ptx_output, &fixed).expect("Failed to write downgraded PTX");
                    println!(
                        "PTX Build: Downgraded '{}' -> '.version 9.0' for {}",
                        ptx_text[pos..directive_end].trim(),
                        file_name
                    );
                }
            }
        }

        // Verify the PTX file was created
        match std::fs::metadata(&ptx_output) {
            Ok(metadata) => {
                println!(
                    "PTX Build: {} created, size: {} bytes",
                    file_name,
                    metadata.len()
                );
                if metadata.len() == 0 {
                    panic!("PTX file {} was created but is empty - CUDA compilation may have failed silently", file_name);
                }

                // Export PTX path as environment variable
                let env_var = format!("{}_PTX_PATH", file_name.to_uppercase());
                println!("cargo:rustc-env={}={}", env_var, ptx_output.display());
                println!("PTX Build: Exported {}={}", env_var, ptx_output.display());
            }
            Err(e) => {
                panic!(
                    "PTX file {} was not created despite successful nvcc status: {}",
                    file_name, e
                );
            }
        }
    }

    println!("All PTX compilation successful!");

    // CUDA source files that export host-callable FFI symbols and need linking
    let link_sources = [
        ("src/utils/visionflow_unified.cu", "thrust_wrapper"),
        ("src/utils/semantic_forces.cu", "semantic_forces"),
        ("src/utils/pagerank.cu", "pagerank"),
        ("src/utils/gpu_connected_components.cu", "gpu_connected_components"),
        ("src/utils/nan_guard.cu", "nan_guard"),
    ];

    let mut obj_files: Vec<PathBuf> = Vec::new();

    for (src_path, obj_name) in &link_sources {
        let cuda_src = Path::new(src_path);
        let obj_output = PathBuf::from(&out_dir).join(format!("{}.o", obj_name));

        // Use -gencode to produce CUBIN + embedded PTX for portability.
        // The PTX allows JIT compilation on GPUs with higher compute capability
        // than the build target (e.g. built for sm_75, runs on sm_86).
        // ISA version is downgraded post-compilation in build.rs (see below).
        let gencode_flag = format!(
            "-gencode=arch=compute_{0},code=[sm_{0},compute_{0}]",
            cuda_arch
        );
        println!("Compiling {} to object file (gencode: {})...", obj_name, gencode_flag);
        let obj_status = Command::new("nvcc")
            .args([
                "-c",
                &gencode_flag,
                "-o",
                obj_output.to_str().unwrap(),
                cuda_src.to_str().unwrap(),
                "--use_fast_math",
                "-O3",
                "-Xcompiler",
                "-fPIC",
                "-dc", // Enable device code linking
            ])
            .status()
            .expect(&format!("Failed to compile {}", obj_name));

        if !obj_status.success() {
            panic!("{} compilation failed", obj_name);
        }

        obj_files.push(obj_output);
    }

    // Device link all object files together (required for cross-module device calls)
    let dlink_output = PathBuf::from(&out_dir).join("cuda_dlink.o");
    let dlink_gencode = format!("-gencode=arch=compute_{0},code=[sm_{0},compute_{0}]", cuda_arch);
    println!("Device linking {} CUDA object files ({})...", obj_files.len(), dlink_gencode);
    let mut dlink_args: Vec<String> = vec![
        "-dlink".to_string(),
        dlink_gencode,
    ];
    for obj in &obj_files {
        dlink_args.push(obj.to_str().unwrap().to_string());
    }
    dlink_args.push("-o".to_string());
    dlink_args.push(dlink_output.to_str().unwrap().to_string());

    let dlink_status = Command::new("nvcc")
        .args(&dlink_args)
        .status()
        .expect("Failed to device link");

    if !dlink_status.success() {
        panic!("Device linking failed");
    }

    // Create static library from all object files + device link output
    let lib_output = PathBuf::from(&out_dir).join("libthrust_wrapper.a");
    println!("Creating static library...");
    let mut ar_args: Vec<String> = vec![
        "rcs".to_string(),
        lib_output.to_str().unwrap().to_string(),
    ];
    for obj in &obj_files {
        ar_args.push(obj.to_str().unwrap().to_string());
    }
    ar_args.push(dlink_output.to_str().unwrap().to_string());

    let ar_status = Command::new("ar")
        .args(&ar_args)
        .status()
        .expect("Failed to create static library");

    if !ar_status.success() {
        panic!("Failed to create static library");
    }

    // Link the static library
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=thrust_wrapper");

    // Link CUDA libraries
    println!("cargo:rustc-link-search=native={}/lib64", cuda_path);
    println!("cargo:rustc-link-search=native={}/lib64/stubs", cuda_path);
    println!("cargo:rustc-link-lib=cudart");
    println!("cargo:rustc-link-lib=cuda");
    println!("cargo:rustc-link-lib=cudadevrt"); // Device runtime for Thrust

    // Link C++ standard library for Thrust
    println!("cargo:rustc-link-lib=stdc++");

    println!("CUDA build complete!");
}
