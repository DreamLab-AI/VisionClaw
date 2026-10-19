// nvcc host-toolchain selection, shared by `build.rs` and the library.
//
// `build.rs` cannot depend on the crate it builds, so this file is `include!`d
// there (like `ptx_policy.rs`) and declared as `pub mod nvcc_toolchain` in
// `lib.rs`, which is what lets the rules below be unit-tested. It is `std`-only.
//
// Two things must be right for `nvcc` to compile a `.cu` file, and both vary by
// host: a host C++ compiler the installed CUDA release supports (CUDA 12.9 stops
// at gcc 14; the CachyOS system gcc 16 and the Nix gcc 15 both fail), and the directory holding
// `cuda_runtime.h` (split CUDA packages, such as the Nix one, put the toolkit's
// headers in a separate output from `nvcc`). The previous build script looked
// for a compiler only at four fixed `/usr` paths and never added an include
// directory, so it worked in the CachyOS images (where the `gcc14` package
// provides `/usr/bin/g++-14` beside the default gcc 16) and failed in the Nix
// agentbox image, and the ADR-2030 bundled-PTX fallback then hid the failure.

/// Where a host compiler is looked for when neither environment variable names one.
pub const HOST_COMPILER_CANDIDATES: [&str; 4] = [
    "/usr/bin/g++-13",
    "/usr/bin/g++-14",
    "/opt/cuda/bin/gcc",
    "/usr/local/bin/g++-13",
];

/// Where a host compiler came from, for the build log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCompilerSource {
    /// `NVCC_CCBIN`.
    NvccCcbin,
    /// `CUDAHOSTCXX`.
    CudaHostCxx,
    /// One of [`HOST_COMPILER_CANDIDATES`].
    Candidate,
}

/// Choose the host compiler to hand `nvcc` as `--compiler-bindir`.
///
/// `NVCC_CCBIN` wins, then `CUDAHOSTCXX`, then the first existing entry of
/// [`HOST_COMPILER_CANDIDATES`]. An environment value is used only if it is
/// non-empty and exists: a stale path falls through to the next source rather
/// than failing every kernel with a confusing nvcc error. `None` leaves the
/// choice to `nvcc` (or to a wrapper, as in the agentbox image).
pub fn select_host_compiler(
    nvcc_ccbin: Option<&str>,
    cudahostcxx: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<(String, HostCompilerSource)> {
    fn usable<'a>(v: Option<&'a str>, exists: &dyn Fn(&str) -> bool) -> Option<&'a str> {
        v.map(str::trim).filter(|p| !p.is_empty() && exists(p))
    }
    if let Some(p) = usable(nvcc_ccbin, exists) {
        return Some((p.to_string(), HostCompilerSource::NvccCcbin));
    }
    if let Some(p) = usable(cudahostcxx, exists) {
        return Some((p.to_string(), HostCompilerSource::CudaHostCxx));
    }
    HOST_COMPILER_CANDIDATES
        .iter()
        .find(|p| exists(p))
        .map(|p| (p.to_string(), HostCompilerSource::Candidate))
}

/// The directory to pass as `-I` so `cuda_runtime.h` resolves, if `cuda_path`
/// has one. Returns `None` when `<cuda_path>/include/cuda_runtime.h` does not
/// exist, because pointing `-I` at a directory without the header fixes nothing.
pub fn cuda_include_dir(cuda_path: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let dir = format!("{}/include", cuda_path.trim_end_matches('/'));
    exists(&format!("{dir}/cuda_runtime.h")).then_some(dir)
}

/// The nvcc arguments that carry the host toolchain choice.
pub fn host_toolchain_args(compiler: Option<&str>, include_dir: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(cc) = compiler {
        args.push("--compiler-bindir".to_string());
        args.push(cc.to_string());
    }
    if let Some(dir) = include_dir {
        args.push(format!("-I{dir}"));
    }
    args
}

#[cfg(test)]
mod nvcc_toolchain_tests {
    use super::*;

    fn only<'a>(present: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |p| present.contains(&p)
    }

    #[test]
    fn nvcc_ccbin_wins_over_everything() {
        let ex = only(&["/nix/gcc14/bin/gcc", "/other/g++", "/usr/bin/g++-13"]);
        let got = select_host_compiler(Some("/nix/gcc14/bin/gcc"), Some("/other/g++"), &ex);
        assert_eq!(
            got,
            Some(("/nix/gcc14/bin/gcc".into(), HostCompilerSource::NvccCcbin))
        );
    }

    #[test]
    fn cudahostcxx_is_used_when_nvcc_ccbin_is_absent() {
        let ex = only(&["/other/g++", "/usr/bin/g++-13"]);
        let got = select_host_compiler(None, Some("/other/g++"), &ex);
        assert_eq!(
            got,
            Some(("/other/g++".into(), HostCompilerSource::CudaHostCxx))
        );
    }

    #[test]
    fn a_stale_or_empty_variable_falls_through_instead_of_poisoning_the_build() {
        let ex = only(&["/usr/bin/g++-14"]);
        let got = select_host_compiler(Some("/gone/gcc"), Some("  "), &ex);
        assert_eq!(
            got,
            Some(("/usr/bin/g++-14".into(), HostCompilerSource::Candidate))
        );
    }

    #[test]
    fn the_fixed_candidates_still_work_on_the_cachyos_images() {
        let ex = only(&["/usr/bin/g++-13"]);
        assert_eq!(
            select_host_compiler(None, None, &ex),
            Some(("/usr/bin/g++-13".into(), HostCompilerSource::Candidate))
        );
    }

    #[test]
    fn no_compiler_anywhere_leaves_the_choice_to_nvcc() {
        assert_eq!(select_host_compiler(None, None, &only(&[])), None);
    }

    #[test]
    fn the_include_dir_is_used_only_when_it_holds_the_header() {
        let ex = only(&["/cuda/include/cuda_runtime.h"]);
        assert_eq!(cuda_include_dir("/cuda", &ex), Some("/cuda/include".into()));
        assert_eq!(
            cuda_include_dir("/cuda/", &ex),
            Some("/cuda/include".into())
        );
        assert_eq!(cuda_include_dir("/split-output", &ex), None);
    }

    #[test]
    fn the_args_carry_exactly_what_was_chosen() {
        assert!(host_toolchain_args(None, None).is_empty());
        assert_eq!(
            host_toolchain_args(Some("/g/gcc"), Some("/c/include")),
            ["--compiler-bindir", "/g/gcc", "-I/c/include"]
        );
        assert_eq!(
            host_toolchain_args(None, Some("/c/include")),
            ["-I/c/include"]
        );
    }
}
