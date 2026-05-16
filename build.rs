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
    ];

    // Only rebuild if CUDA files change
    for cuda_file in &cuda_files {
        println!("cargo:rerun-if-changed={}", cuda_file);
    }
    println!("cargo:rerun-if-changed=build.rs");

    // Get build configuration
    let out_dir = env::var("OUT_DIR").unwrap();
    let cuda_path = env::var("CUDA_PATH")
        .or_else(|_| env::var("CUDA_HOME"))
        .unwrap_or_else(|_| "/opt/cuda".to_string());

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

    // Find a CUDA-compatible host compiler (nvcc supports up to GCC 14).
    // CachyOS ships GCC 16 which is too new; look for an older GCC first.
    let cuda_host_compiler = [
        "/usr/bin/g++-13", "/usr/bin/g++-14",
        "/opt/cuda/bin/gcc", "/usr/local/bin/g++-13",
    ]
    .iter()
    .find(|p| Path::new(p).exists())
    .map(|s| s.to_string());

    if let Some(ref cc) = cuda_host_compiler {
        println!("PTX Build: Using CUDA-compatible host compiler: {}", cc);
    } else {
        println!("PTX Build: No older GCC found, using system default with compat flags");
    }

    // Compile all CUDA files to PTX
    println!("Compiling {} CUDA kernels to PTX...", cuda_files.len());

    for cuda_file in &cuda_files {
        let cuda_src = Path::new(cuda_file);
        let file_name = cuda_src.file_stem().unwrap().to_str().unwrap();
        let ptx_output = PathBuf::from(&out_dir).join(format!("{}.ptx", file_name));

        println!("Compiling {} to PTX...", file_name);

        let mut nvcc_args: Vec<String> = vec![
            "-ptx".into(),
            "-arch".into(), format!("sm_{}", cuda_arch),
            "-o".into(), ptx_output.to_str().unwrap().into(),
            cuda_src.to_str().unwrap().into(),
            "--use_fast_math".into(),
            "-O3".into(),
            "-std=c++17".into(),
            "--allow-unsupported-compiler".into(),
            "--expt-relaxed-constexpr".into(),
        ];

        if let Some(ref cc) = cuda_host_compiler {
            nvcc_args.push("--compiler-bindir".into());
            nvcc_args.push(cc.clone());
        }

        let nvcc_output = Command::new("nvcc")
            .args(&nvcc_args)
            .output()
            .expect("Failed to execute nvcc - is CUDA toolkit installed and in PATH?");

        if !nvcc_output.status.success() {
            let stderr = String::from_utf8_lossy(&nvcc_output.stderr);
            eprintln!("NVCC STDERR: {}", stderr);

            // Fallback: check for pre-compiled PTX (from Docker image build or prior build)
            let fallback_paths = [
                format!("src/utils/ptx/{}.ptx", file_name),
                format!("/app/src/utils/ptx/{}.ptx", file_name),
            ];
            let fallback = fallback_paths.iter().find(|p| Path::new(p).exists());

            if let Some(fb) = fallback {
                println!("cargo:warning=NVCC failed for {} — using pre-compiled PTX from {}", file_name, fb);
                std::fs::copy(fb, &ptx_output).expect("Failed to copy fallback PTX");
            } else {
                panic!("CUDA PTX compilation failed for {} (exit {:?}) and no fallback PTX found.\n\
                        Install gcc-13 or gcc-14 for CUDA compatibility: pacman -S gcc13",
                       file_name, nvcc_output.status.code());
            }
        }

        // Downgrade PTX ISA version to 9.0 for driver compatibility.
        // CUDA toolkit 13.x emits .version 9.x but the host driver may only JIT up to 9.0.
        // This is safe: sm_86 kernels don't use ISA 9.1+ features.
        if let Ok(ptx_text) = std::fs::read_to_string(&ptx_output) {
            // Match any .version 9.N where N > 0
            if let Some(pos) = ptx_text.find(".version 9.") {
                let version_str = &ptx_text[pos..pos+13.min(ptx_text.len() - pos)];
                if version_str != ".version 9.0" {
                    let fixed = ptx_text[..pos].to_string() + ".version 9.0" + &ptx_text[pos + 12..];
                    std::fs::write(&ptx_output, fixed).expect("Failed to write downgraded PTX");
                    println!("PTX Build: Downgraded {} -> 9.0 for {}", version_str.trim(), file_name);
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
    ];

    let mut obj_files: Vec<PathBuf> = Vec::new();

    for (src_path, obj_name) in &link_sources {
        let cuda_src = Path::new(src_path);
        let obj_output = PathBuf::from(&out_dir).join(format!("{}.o", obj_name));

        let gencode_flag = format!(
            "-gencode=arch=compute_{0},code=[sm_{0},compute_{0}]",
            cuda_arch
        );
        println!("Compiling {} to object file (gencode: {})...", obj_name, gencode_flag);

        let mut obj_args: Vec<String> = vec![
            "-c".into(),
            gencode_flag,
            "-o".into(), obj_output.to_str().unwrap().into(),
            cuda_src.to_str().unwrap().into(),
            "--use_fast_math".into(),
            "-O3".into(),
            "-Xcompiler".into(), "-fPIC".into(),
            "-dc".into(),
            "-std=c++17".into(),
            "--allow-unsupported-compiler".into(),
            "--expt-relaxed-constexpr".into(),
        ];

        if let Some(ref cc) = cuda_host_compiler {
            obj_args.push("--compiler-bindir".into());
            obj_args.push(cc.clone());
        }

        let obj_status = Command::new("nvcc")
            .args(&obj_args)
            .output()
            .expect(&format!("Failed to compile {}", obj_name));

        if !obj_status.status.success() {
            let stderr = String::from_utf8_lossy(&obj_status.stderr);
            println!("cargo:warning=NVCC object compilation failed for {}: {}", obj_name, stderr.lines().last().unwrap_or("unknown error"));
            println!("cargo:warning=Skipping native CUDA linking — GPU functions will use PTX JIT at runtime");
            obj_files.clear();
            break;
        }

        obj_files.push(obj_output);
    }

    if !obj_files.is_empty() {
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
        println!("cargo:rustc-link-lib=cudadevrt");
        println!("cargo:rustc-link-lib=stdc++");

        println!("CUDA build complete (native linking)!");
    } else {
        println!("cargo:warning=Native CUDA object compilation unavailable (GCC too new for nvcc)");
        println!("cargo:warning=GPU features will use PTX JIT — FFI functions stubbed");

        // Compile C stub providing no-op FFI symbols so the Rust linker is satisfied
        let stub_src = Path::new("src/utils/cuda_ffi_stubs.c");
        let stub_obj = PathBuf::from(&out_dir).join("cuda_ffi_stubs.o");
        let stub_lib = PathBuf::from(&out_dir).join("libthrust_wrapper.a");

        let cc = cuda_host_compiler.as_deref().unwrap_or("gcc");
        let cc_status = Command::new(cc)
            .args(["-c", "-fPIC", "-o"])
            .arg(stub_obj.to_str().unwrap())
            .arg(stub_src.to_str().unwrap())
            .status()
            .expect("Failed to compile FFI stubs with gcc");

        if !cc_status.success() {
            panic!("Failed to compile cuda_ffi_stubs.c — cannot provide FFI symbols");
        }

        let ar_status = Command::new("ar")
            .args(["rcs"])
            .arg(stub_lib.to_str().unwrap())
            .arg(stub_obj.to_str().unwrap())
            .status()
            .expect("Failed to create stub library");

        if !ar_status.success() {
            panic!("Failed to create libthrust_wrapper.a from stubs");
        }

        println!("cargo:rustc-link-search=native={}", out_dir);
        println!("cargo:rustc-link-lib=static=thrust_wrapper");

        // Link CUDA runtime for PTX loading at runtime
        println!("cargo:rustc-link-search=native={}/lib64", cuda_path);
        println!("cargo:rustc-link-search=native={}/lib64/stubs", cuda_path);
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cuda");
        println!("cargo:rustc-link-lib=stdc++");

        println!("CUDA build complete (PTX-only mode with FFI stubs)!");
    }
}
