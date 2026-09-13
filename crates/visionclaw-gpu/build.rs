//! build.rs for visionclaw-gpu — ADR-090 Phase 3
//!
//! Compiles all CUDA kernels to PTX and (where possible) to native object files
//! for linking. All .cu sources now live at `src/cuda_sources/` within this crate.
//!
//! Lifted from the root webxr build.rs; the root build.rs retains a stub that
//! delegates CUDA compilation to this crate via the workspace dep.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

// ADR-2030: the PTX acceptance policy is compiled from ONE source of truth,
// shared with the library so its behaviour is unit-tested. A build script cannot
// depend on the crate it builds, so the module is included by path; it is
// `std`-only, so this costs nothing. See `src/ptx_policy.rs` for the policy and
// the tests that pin it.
include!("src/ptx_policy.rs");

fn main() {
    // Check if GPU feature is enabled
    let gpu_enabled = env::var("CARGO_FEATURE_GPU").is_ok();

    if !gpu_enabled {
        println!("cargo:warning=visionclaw-gpu: GPU feature disabled, skipping CUDA compilation");
        return;
    }

    // All CUDA source files — paths are relative to this crate root
    let cuda_files = [
        "src/cuda_sources/visionclaw_unified.cu",
        "src/cuda_sources/gpu_clustering_kernels.cu",
        // dynamic_grid.cu contains only __host__ helpers (no launchable kernels).
        // It must not be emitted/validated as a device PTX module.
        "src/cuda_sources/gpu_aabb_reduction.cu",
        "src/cuda_sources/gpu_landmark_apsp.cu",
        "src/cuda_sources/sssp_compact.cu",
        "src/cuda_sources/semantic_forces.cu",
        "src/cuda_sources/pagerank.cu",
        "src/cuda_sources/gpu_connected_components.cu",
    ];

    // Rebuild triggers
    for cuda_file in &cuda_files {
        println!("cargo:rerun-if-changed={}", cuda_file);
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CUDA_ARCH");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");
    println!("cargo:rerun-if-env-changed=DOCKER_ENV");

    let out_dir = env::var("OUT_DIR").unwrap();
    let cuda_path = env::var("CUDA_PATH")
        .or_else(|_| env::var("CUDA_HOME"))
        .unwrap_or_else(|_| "/opt/cuda".to_string());

    // CUDA architecture selection.
    // Docker builds: never auto-detect (build GPU != runtime GPU). Default sm_75 portable baseline.
    let is_docker = env::var("DOCKER_ENV").is_ok();
    let cuda_arch = env::var("CUDA_ARCH").unwrap_or_else(|_| {
        if is_docker {
            println!("cargo:warning=visionclaw-gpu: Docker build — using portable sm_75 (set CUDA_ARCH to override)");
            return "75".to_string();
        }
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=compute_cap", "--format=csv,noheader", "--id=0"])
            .output()
        {
            if output.status.success() {
                let raw = String::from_utf8_lossy(&output.stdout);
                if let Some(cap) = raw.lines().next() {
                    let arch = cap.trim().replace('.', "");
                    if !arch.is_empty() {
                        println!("cargo:warning=visionclaw-gpu: Auto-detected GPU sm_{}", arch);
                        return arch;
                    }
                }
            }
        }
        "75".to_string()
    });
    println!(
        "cargo:warning=visionclaw-gpu: Building for sm_{}",
        cuda_arch
    );

    // Find a CUDA-compatible host compiler (nvcc supports up to GCC 14).
    // CachyOS ships GCC 16 which is too new.
    let cuda_host_compiler = [
        "/usr/bin/g++-13",
        "/usr/bin/g++-14",
        "/opt/cuda/bin/gcc",
        "/usr/local/bin/g++-13",
    ]
    .iter()
    .find(|p| Path::new(p).exists())
    .map(|s| s.to_string());

    if let Some(ref cc) = cuda_host_compiler {
        println!(
            "cargo:warning=visionclaw-gpu: Using CUDA host compiler: {}",
            cc
        );
    }

    // ── Phase 1: PTX compilation ──────────────────────────────────────────────
    println!(
        "cargo:warning=visionclaw-gpu: Compiling {} CUDA kernels to PTX",
        cuda_files.len()
    );

    // ADR-2030 build manifest: one line per module recording where its PTX came
    // from, its declared ISA and its content tags before/after the rewrite.
    // Written to OUT_DIR and exported so a release can answer "which module is
    // actually loaded, and was it compiled or a fallback?" without guessing from
    // file modification times.
    let mut artefacts: Vec<PtxArtefact> = Vec::new();

    for cuda_file in &cuda_files {
        let cuda_src = Path::new(cuda_file);
        let file_name = cuda_src.file_stem().unwrap().to_str().unwrap();
        let ptx_output = PathBuf::from(&out_dir).join(format!("{}.ptx", file_name));

        let mut nvcc_args: Vec<String> = vec![
            "-ptx".into(),
            "-arch".into(),
            format!("sm_{}", cuda_arch),
            "-o".into(),
            ptx_output.to_str().unwrap().into(),
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

        // ADR-2030: classify launch failure separately from compiler failure.
        // Previously `.expect(...)` panicked when nvcc was ABSENT — before the
        // fallback was ever consulted — which is precisely the case the bundled
        // pre-compiled PTX exists to cover.
        let spawned = Command::new("nvcc").args(&nvcc_args).output();
        let outcome = match &spawned {
            Ok(out) => NvccOutcome::classify(None, out.status.success(), out.status.code()),
            Err(e) => NvccOutcome::classify(Some(e.to_string()), false, None),
        };
        if let Ok(out) = &spawned {
            if !out.status.success() {
                eprintln!("NVCC STDERR: {}", String::from_utf8_lossy(&out.stderr));
            }
        }

        let mut provenance = PtxProvenance::Compiled;
        let mut source_path = cuda_src.display().to_string();

        if outcome.needs_fallback() {
            println!(
                "cargo:warning=visionclaw-gpu: {} — {}",
                file_name,
                outcome.diagnosis()
            );

            // Fallback: pre-compiled PTX bundled with the crate or from /app image
            let fallback_paths = [
                format!("src/ptx/{}.ptx", file_name),
                format!("/app/src/utils/ptx/{}.ptx", file_name),
                // Legacy path — Docker image may still have them here
                format!("/app/crates/visionclaw-gpu/src/ptx/{}.ptx", file_name),
            ];
            let fallback = fallback_paths.iter().find(|p| Path::new(p).exists());

            if let Some(fb) = fallback {
                println!(
                    "cargo:warning=visionclaw-gpu: {} — using pre-compiled PTX from {}",
                    file_name, fb
                );
                std::fs::copy(fb, &ptx_output).expect("Failed to copy fallback PTX");
                provenance = PtxProvenance::for_fallback(&outcome).expect("failing outcome");
                source_path = fb.clone();
            } else {
                panic!(
                    "PTX unavailable for {}: {}. No fallback PTX found at any of {:?}.\n\
                     Install gcc-13 or gcc-14 (pacman -S gcc13), or ship a pre-compiled \
                     module at src/ptx/{}.ptx",
                    file_name,
                    outcome.diagnosis(),
                    fallback_paths,
                    file_name
                );
            }
        }

        // Read once, then apply the ISA policy and the validation gate to the
        // bytes we actually have — whether compiled or fallen back to.
        let original = std::fs::read_to_string(&ptx_output)
            .unwrap_or_else(|e| panic!("PTX file {} not readable: {}", file_name, e));
        let original_tag = content_tag(original.as_bytes());

        // Downgrade the declared PTX ISA for driver compatibility. The rewrite is
        // by parsed token span, not a fixed-width splice — the old code turned a
        // two-digit minor such as 9.10 into 9.00, a LOWER version than either the
        // original or the target.
        let (final_text, isa) = match rewrite_ptx_version(&original, TARGET_PTX_ISA) {
            VersionRewrite::Unchanged { version } => {
                // Reported as unchanged, not as a downgrade: the old code warned
                // about a "downgrade" on content it had not touched.
                (original.clone(), version)
            }
            VersionRewrite::Rewritten { from, to, text } => {
                std::fs::write(&ptx_output, &text).expect("Failed to write downgraded PTX");
                println!(
                    "cargo:warning=visionclaw-gpu: {} — declared ISA {} rewritten to {} \
                     (declared-version change only; instruction support is not proven)",
                    file_name, from, to
                );
                (text, to)
            }
            VersionRewrite::Defective(defect) => panic!(
                "PTX for {} is unusable after the {} phase: {}",
                file_name,
                provenance.as_str(),
                defect
            ),
        };

        // Validate structure and required symbols. A non-empty file is NOT a
        // valid one: a successful compiler writing arbitrary text used to pass
        // the length-only gate.
        let required: &[&str] = if file_name == "visionclaw_unified" {
            &REQUIRED_UNIFIED_SYMBOLS
        } else {
            &[]
        };
        if let Err(defect) = validate_ptx(&final_text, required) {
            panic!(
                "PTX validation failed for {} (provenance {}): {}",
                file_name,
                provenance.as_str(),
                defect
            );
        }

        artefacts.push(PtxArtefact {
            module: file_name.to_string(),
            source: source_path,
            provenance,
            isa,
            original_tag,
            rewritten_tag: content_tag(final_text.as_bytes()),
        });

        let env_var = format!("{}_PTX_PATH", file_name.to_uppercase());
        println!("cargo:rustc-env={}={}", env_var, ptx_output.display());
    }

    // Emit the build manifest so the selected modules and their content identity
    // are recorded per build rather than inferred from file modification times.
    let manifest_path = PathBuf::from(&out_dir).join("ptx-build-manifest.txt");
    let manifest_body: String = artefacts
        .iter()
        .map(|a| format!("{}\n", a.manifest_line()))
        .collect();
    std::fs::write(&manifest_path, &manifest_body).expect("Failed to write PTX build manifest");
    println!(
        "cargo:rustc-env=VISIONCLAW_PTX_MANIFEST={}",
        manifest_path.display()
    );
    for a in &artefacts {
        println!("cargo:warning=visionclaw-gpu: {}", a.manifest_line());
    }

    println!(
        "cargo:warning=visionclaw-gpu: All PTX compilation done ({} modules)",
        artefacts.len()
    );

    // ── Phase 2: Native linking (FFI symbols) ─────────────────────────────────
    // These four .cu files export host-callable FFI symbols and must be linked
    // as a static library so the webxr binary can call them.
    let link_sources = [
        ("src/cuda_sources/visionclaw_unified.cu", "thrust_wrapper"),
        ("src/cuda_sources/semantic_forces.cu", "semantic_forces"),
        ("src/cuda_sources/pagerank.cu", "pagerank"),
        (
            "src/cuda_sources/gpu_connected_components.cu",
            "gpu_connected_components",
        ),
    ];

    let mut obj_files: Vec<PathBuf> = Vec::new();

    for (src_path, obj_name) in &link_sources {
        let cuda_src = Path::new(src_path);
        let obj_output = PathBuf::from(&out_dir).join(format!("{}.o", obj_name));
        let gencode = format!(
            "-gencode=arch=compute_{0},code=[sm_{0},compute_{0}]",
            cuda_arch
        );

        let mut obj_args: Vec<String> = vec![
            "-c".into(),
            gencode,
            "-o".into(),
            obj_output.to_str().unwrap().into(),
            cuda_src.to_str().unwrap().into(),
            "--use_fast_math".into(),
            "-O3".into(),
            "-Xcompiler".into(),
            "-fPIC".into(),
            "-dc".into(),
            "-std=c++17".into(),
            "--allow-unsupported-compiler".into(),
            "--expt-relaxed-constexpr".into(),
        ];

        if let Some(ref cc) = cuda_host_compiler {
            obj_args.push("--compiler-bindir".into());
            obj_args.push(cc.clone());
        }

        let result = Command::new("nvcc")
            .args(&obj_args)
            .output()
            .expect(&format!("Failed to compile {}", obj_name));

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            println!(
                "cargo:warning=visionclaw-gpu: Native object compilation failed for {}: {}",
                obj_name,
                stderr.lines().last().unwrap_or("unknown error")
            );
            println!("cargo:warning=visionclaw-gpu: Falling back to PTX-only JIT mode");
            obj_files.clear();
            break;
        }
        obj_files.push(obj_output);
    }

    if !obj_files.is_empty() {
        let dlink_output = PathBuf::from(&out_dir).join("cuda_dlink.o");
        let dlink_gencode = format!(
            "-gencode=arch=compute_{0},code=[sm_{0},compute_{0}]",
            cuda_arch
        );
        let mut dlink_args = vec!["-dlink".to_string(), dlink_gencode];
        for obj in &obj_files {
            dlink_args.push(obj.to_str().unwrap().to_string());
        }
        dlink_args.extend(["-o".to_string(), dlink_output.to_str().unwrap().to_string()]);

        let dlink_status = Command::new("nvcc")
            .args(&dlink_args)
            .status()
            .expect("Device link failed");
        if !dlink_status.success() {
            panic!("Device linking step failed");
        }

        let lib_output = PathBuf::from(&out_dir).join("libthrust_wrapper.a");
        let mut ar_args = vec!["rcs".to_string(), lib_output.to_str().unwrap().to_string()];
        for obj in &obj_files {
            ar_args.push(obj.to_str().unwrap().to_string());
        }
        ar_args.push(dlink_output.to_str().unwrap().to_string());

        let ar_status = Command::new("ar")
            .args(&ar_args)
            .status()
            .expect("ar failed");
        if !ar_status.success() {
            panic!("Failed to create libthrust_wrapper.a");
        }

        println!("cargo:rustc-link-search=native={}", out_dir);
        println!("cargo:rustc-link-lib=static=thrust_wrapper");
        println!("cargo:rustc-link-search=native={}/lib64", cuda_path);
        println!("cargo:rustc-link-search=native={}/lib64/stubs", cuda_path);
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cuda");
        println!("cargo:rustc-link-lib=cudadevrt");
        println!("cargo:rustc-link-lib=stdc++");

        println!("cargo:warning=visionclaw-gpu: Native CUDA linking complete");
    } else {
        // PTX-only mode: stub out FFI symbols so the linker is satisfied.
        // The stub lives in the webxr monolith at src/utils/cuda_ffi_stubs.c —
        // reference it from here via an absolute-ish relative path. If this path
        // is wrong the linker will emit an error pointing here.
        let stub_candidates = [
            // Relative to this crate (when building from workspace)
            "../../src/utils/cuda_ffi_stubs.c",
            // Absolute Docker image path
            "/app/src/utils/cuda_ffi_stubs.c",
        ];
        let stub_src = stub_candidates
            .iter()
            .find(|p| Path::new(p).exists())
            .map(Path::new)
            .expect("cuda_ffi_stubs.c not found — cannot provide FFI symbols in PTX-only mode");

        let stub_obj = PathBuf::from(&out_dir).join("cuda_ffi_stubs.o");
        let stub_lib = PathBuf::from(&out_dir).join("libthrust_wrapper.a");
        let cc = cuda_host_compiler.as_deref().unwrap_or("gcc");

        let cc_status = Command::new(cc)
            .args(["-c", "-fPIC", "-o"])
            .arg(&stub_obj)
            .arg(stub_src)
            .status()
            .expect("Failed to compile cuda_ffi_stubs.c");
        if !cc_status.success() {
            panic!("cuda_ffi_stubs.c compilation failed");
        }

        let ar_status = Command::new("ar")
            .args(["rcs"])
            .arg(&stub_lib)
            .arg(&stub_obj)
            .status()
            .expect("ar failed for stub library");
        if !ar_status.success() {
            panic!("Failed to create libthrust_wrapper.a from stubs");
        }

        println!("cargo:rustc-link-search=native={}", out_dir);
        println!("cargo:rustc-link-lib=static=thrust_wrapper");
        println!("cargo:rustc-link-search=native={}/lib64", cuda_path);
        println!("cargo:rustc-link-search=native={}/lib64/stubs", cuda_path);
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cuda");
        println!("cargo:rustc-link-lib=stdc++");

        println!("cargo:warning=visionclaw-gpu: PTX-only mode with FFI stubs");
    }
}
