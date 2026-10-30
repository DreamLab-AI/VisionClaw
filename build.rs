//! build.rs for the webxr binary — ADR-090 Phase 3
//!
//! CUDA source files have moved to `crates/visionclaw-gpu/src/cuda_sources/`.
//! Compilation and linking are now handled entirely by that crate's build.rs.
//!
//! This file is kept so Cargo does not complain about a missing build script;
//! it simply emits the GPU feature env-var check so existing conditional
//! compilation in webxr's own source files continues to work, and re-runs if
//! any relevant env vars change.

use std::env;

fn main() {
    let gpu_enabled = env::var("CARGO_FEATURE_GPU").is_ok();
    if !gpu_enabled {
        println!("cargo:warning=webxr build.rs: GPU feature not enabled");
    } else {
        println!("cargo:warning=webxr build.rs: GPU feature enabled — CUDA compilation delegated to visionclaw-gpu crate");
    }

    // RES-a: embed the short git SHA so the LivenessHarness can bind a canary
    // fire to the commit it fired at (staleness rule). Runtime VISIONCLAW_GIT_SHA
    // overrides this build-time default; both fall back to "unknown".
    if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        if out.status.success() {
            let sha = String::from_utf8_lossy(&out.stdout);
            let sha = sha.trim();
            if !sha.is_empty() {
                println!("cargo:rustc-env=VISIONCLAW_GIT_SHA={sha}");
            }
        }
    }
    // A missing rerun-if-changed path counts as always changed, and the dev
    // container does not mount .git, so declare it only where it exists.
    if std::path::Path::new(".git/HEAD").exists() {
        println!("cargo:rerun-if-changed=.git/HEAD");
    }

    // Notify Cargo to re-run if the GPU feature flag or CUDA env vars change.
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GPU");
    println!("cargo:rerun-if-env-changed=CUDA_ARCH");
    println!("cargo:rerun-if-env-changed=DOCKER_ENV");
    println!("cargo:rerun-if-changed=build.rs");
}
