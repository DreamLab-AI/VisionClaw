//! ABI and provenance checks for the `visionclaw_unified` PTX.
//!
//! VisionClaw owns the kernels, and the CUDA engine is a guest that launches
//! them. Two failure modes here are silent: a parameter-list change shifts
//! every pointer after it, and a stale `SimParams` layout reads the wrong
//! constants. Before launching anything, the engine therefore checks three
//! things:
//!
//! * the size of the `.const c_params` block (must be [`SIM_PARAMS_BYTES`]);
//! * the parameter count of every kernel it launches ([`EXPECTED_KERNELS`]);
//! * the build manifest line for the module, which must say `provenance=compiled`.
//!
//! The gitignored local fallback PTX in `visionclaw-gpu/src/ptx/` predates
//! the 212-byte layout (`c_params[172]`, `force_pass_kernel` with 28
//! parameters). It is what the build copies in when `nvcc` fails, so it fails
//! all three checks.
//!
//! Everything here is plain text parsing and builds without the `cuda` feature.
//!
//! ```
//! use codescape_layout::abi::{const_bytes, entry_arity};
//!
//! let ptx = ".const .align 4 .b8 c_params[212];\n\
//!            .visible .entry k(\n\t.param .u64 k_param_0,\n\t.param .u32 k_param_1\n)\n{ ret; }";
//! assert_eq!(const_bytes(ptx, "c_params"), Some(212));
//! assert_eq!(entry_arity(ptx, "k"), Some(2));
//! ```

use crate::LayoutError;

/// Size in bytes of the device `SimParams` (`static_assert` in `visionclaw_unified.cu`).
pub const SIM_PARAMS_BYTES: usize = 212;

/// Every kernel the CUDA engine launches, with the parameter count its launch
/// code was written against (counted from a fresh nvcc 12.9 compile). The
/// spatial grid is built on the host, so `build_grid_kernel` and
/// `compute_cell_bounds_kernel` are not launched and not required.
pub const EXPECTED_KERNELS: [(&str, usize); 3] = [
    ("force_pass_kernel", 30),
    ("cluster_cohesion_kernel", 13),
    ("integrate_pass_kernel", 24),
];

/// The module name in the build manifest.
pub const UNIFIED_MODULE: &str = "visionclaw_unified";

/// Size in bytes of the `.const` variable `symbol`, from its declaration
/// (`.const .align 4 .b8 c_params[212];`). `None` if it is not declared.
pub fn const_bytes(ptx: &str, symbol: &str) -> Option<usize> {
    for line in ptx.lines() {
        let line = line.trim();
        if !line.starts_with(".const") && !line.contains(" .const ") {
            continue;
        }
        let tokens: Vec<&str> = line.trim_end_matches(';').split_whitespace().collect();
        let Some(last) = tokens.last() else { continue };
        let (name, count) = match last.find('[') {
            Some(b) => {
                let n: usize = last[b + 1..].trim_end_matches(']').parse().ok()?;
                (&last[..b], n)
            }
            None => (*last, 1),
        };
        if name != symbol {
            continue;
        }
        let elem = tokens.iter().find_map(|t| type_bytes(t))?;
        return Some(elem * count);
    }
    None
}

fn type_bytes(token: &str) -> Option<usize> {
    match token {
        ".b8" | ".u8" | ".s8" => Some(1),
        ".b16" | ".u16" | ".s16" | ".f16" => Some(2),
        ".b32" | ".u32" | ".s32" | ".f32" => Some(4),
        ".b64" | ".u64" | ".s64" | ".f64" => Some(8),
        _ => None,
    }
}

/// Number of `.param` entries in the parameter list of `.entry name(…)`.
/// `None` if the entry is absent or its list is unterminated.
pub fn entry_arity(ptx: &str, name: &str) -> Option<usize> {
    let needle = format!(".entry {name}");
    let mut from = 0;
    while let Some(off) = ptx[from..].find(&needle) {
        let after = from + off + needle.len();
        let rest = ptx[after..].trim_start();
        if let Some(list) = rest.strip_prefix('(') {
            let close = list.find(')')?;
            return Some(list[..close].matches(".param").count());
        }
        from = after; // a longer name that merely starts with `name`
    }
    None
}

/// Check `ptx` against [`SIM_PARAMS_BYTES`] and [`EXPECTED_KERNELS`].
pub fn check_ptx_abi(ptx: &str) -> Result<(), LayoutError> {
    match const_bytes(ptx, "c_params") {
        Some(SIM_PARAMS_BYTES) => {}
        Some(n) => {
            return Err(LayoutError::AbiMismatch(format!(
                "c_params is {n} bytes, expected {SIM_PARAMS_BYTES}: this is not the current visionclaw_unified build (the gitignored local fallback PTX is 172)"
            )))
        }
        None => return Err(LayoutError::AbiMismatch("no `.const c_params` declaration".into())),
    }
    for (kernel, want) in EXPECTED_KERNELS {
        match entry_arity(ptx, kernel) {
            Some(got) if got == want => {}
            Some(got) => {
                return Err(LayoutError::AbiMismatch(format!(
                    "{kernel} takes {got} parameters, this engine was written against {want}"
                )))
            }
            None => {
                return Err(LayoutError::AbiMismatch(format!(
                    "kernel {kernel} is missing"
                )))
            }
        }
    }
    Ok(())
}

/// One line of `ptx-build-manifest.txt` (written by `visionclaw-gpu`'s build script).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Module name, such as `visionclaw_unified`.
    pub module: String,
    /// Where the PTX came from: the `.cu` source, or a pre-compiled file.
    pub source: String,
    /// `compiled`, `fallback-launch-failure` or `fallback-compiler-failure`.
    pub provenance: String,
    /// Declared PTX ISA after the build-time rewrite.
    pub isa: String,
    /// FNV-1a content tag of the PTX before the ISA rewrite.
    pub original: u64,
    /// FNV-1a content tag of the PTX as written to `OUT_DIR`.
    pub rewritten: u64,
}

/// Parse every well-formed line of a build manifest.
///
/// ```
/// use codescape_layout::abi::parse_manifest;
///
/// let m = parse_manifest("visionclaw_unified source=a.cu provenance=compiled isa=8.8 original=00000000000000ff rewritten=00000000000000ff\n");
/// assert_eq!(m[0].provenance, "compiled");
/// assert_eq!(m[0].rewritten, 255);
/// ```
pub fn parse_manifest(text: &str) -> Vec<ManifestEntry> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let module = parts.next()?.to_string();
            let mut e = ManifestEntry {
                module,
                source: String::new(),
                provenance: String::new(),
                isa: String::new(),
                original: 0,
                rewritten: 0,
            };
            for kv in parts {
                let (k, v) = kv.split_once('=')?;
                match k {
                    "source" => e.source = v.to_string(),
                    "provenance" => e.provenance = v.to_string(),
                    "isa" => e.isa = v.to_string(),
                    "original" => e.original = u64::from_str_radix(v, 16).ok()?,
                    "rewritten" => e.rewritten = u64::from_str_radix(v, 16).ok()?,
                    _ => {}
                }
            }
            (!e.provenance.is_empty()).then_some(e)
        })
        .collect()
}

/// The manifest entry for `module`. Fails unless its provenance is `compiled`.
pub fn check_provenance(
    manifest: Option<&str>,
    module: &str,
) -> Result<ManifestEntry, LayoutError> {
    let Some(text) = manifest else {
        return Err(LayoutError::AbiMismatch(
            "no PTX build manifest: provenance cannot be proven".into(),
        ));
    };
    let entry = parse_manifest(text)
        .into_iter()
        .find(|e| e.module == module)
        .ok_or_else(|| {
            LayoutError::AbiMismatch(format!("build manifest has no line for {module}"))
        })?;
    if entry.provenance != "compiled" {
        return Err(LayoutError::AbiMismatch(format!(
            "{module} PTX provenance is `{}` (from {}): refusing a fallback build",
            entry.provenance, entry.source
        )));
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRESH: &str = include_str!("../tests/fixtures/unified_fresh_212.ptx");
    const COMMITTED: &str = include_str!("../tests/fixtures/unified_committed_172.ptx");

    #[test]
    fn fresh_build_passes() {
        assert_eq!(const_bytes(FRESH, "c_params"), Some(212));
        assert_eq!(entry_arity(FRESH, "force_pass_kernel"), Some(30));
        assert_eq!(check_ptx_abi(FRESH), Ok(()));
    }

    #[test]
    fn committed_fallback_is_refused() {
        assert_eq!(const_bytes(COMMITTED, "c_params"), Some(172));
        assert_eq!(entry_arity(COMMITTED, "force_pass_kernel"), Some(28));
        let err = check_ptx_abi(COMMITTED).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains("172")),
            "{err}"
        );
    }

    #[test]
    fn arity_drift_is_refused_even_with_the_right_params_size() {
        // The right c_params, but force_pass_kernel loses its last parameter.
        let drifted = FRESH.replacen(",\n\t.param .u64 force_pass_kernel_param_29", "", 1);
        assert_eq!(entry_arity(&drifted, "force_pass_kernel"), Some(29));
        let err = check_ptx_abi(&drifted).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains("force_pass_kernel takes 29")),
            "{err}"
        );
    }

    #[test]
    fn parser_edge_cases() {
        assert_eq!(entry_arity(".entry k()\n{}", "k"), Some(0));
        assert_eq!(
            entry_arity(
                ".entry kk(.param .u32 a)\n.entry k(.param .u32 a, .param .u32 b)",
                "k"
            ),
            Some(2),
            "prefix names do not match"
        );
        assert_eq!(entry_arity(".entry k(.param .u32 a", "k"), None);
        assert_eq!(entry_arity("", "k"), None);
        assert_eq!(
            const_bytes(".const .align 8 .b64 table[4];", "table"),
            Some(32)
        );
        assert_eq!(const_bytes(".const .f32 scale;", "scale"), Some(4));
        assert_eq!(
            const_bytes(".const .align 4 .b8 c_params_x[212];", "c_params"),
            None
        );
        assert!(check_ptx_abi("no ptx here").is_err());
    }

    #[test]
    fn provenance_must_be_compiled() {
        let good = "visionclaw_unified source=src/cuda_sources/visionclaw_unified.cu provenance=compiled isa=8.8 original=30eb7046455fbc10 rewritten=30eb7046455fbc10\n";
        let entry = check_provenance(Some(good), UNIFIED_MODULE).unwrap();
        assert_eq!(entry.rewritten, 0x30eb_7046_455f_bc10);
        let bad = "visionclaw_unified source=src/ptx/visionclaw_unified.ptx provenance=fallback-compiler-failure isa=8.8 original=1 rewritten=1\n";
        assert!(
            matches!(check_provenance(Some(bad), UNIFIED_MODULE), Err(LayoutError::AbiMismatch(m)) if m.contains("fallback"))
        );
        assert!(check_provenance(None, UNIFIED_MODULE).is_err());
        assert!(check_provenance(
            Some("pagerank source=x provenance=compiled isa=8.8 original=1 rewritten=1"),
            UNIFIED_MODULE
        )
        .is_err());
    }
}
