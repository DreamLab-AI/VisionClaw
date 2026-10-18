//! Load the `visionclaw_unified` PTX and its build manifest from a directory.
//!
//! The CUDA engine launches VisionClaw's kernels but does not build them, and
//! it no longer links `visionclaw-gpu` to find them (ADR-2143 A5). The build
//! artefacts are handed over at run time instead: a directory holding
//!
//! * [`PTX_FILE`] (`visionclaw_unified.ptx`), and
//! * [`MANIFEST_FILE`] (`ptx-build-manifest.txt`), one line per module in the
//!   format `visionclaw-gpu`'s build script writes next to the PTX in its
//!   `OUT_DIR`.
//!
//! [`PtxSource::from_env`] reads that directory from [`PTX_DIR_ENV`]
//! (`CODESCAPE_PTX_DIR`). The artefact is accepted only if
//!
//! * the manifest's `visionclaw_unified` line says `provenance=compiled`,
//! * the PTX file's [`content_tag`] equals the manifest's `rewritten` tag, so
//!   the file is the one that build wrote, and
//! * the PTX passes [`check_ptx_abi`]: `c_params` is 212 bytes and the three
//!   launched kernels have the expected arities.
//!
//! A failed check is [`LayoutError::AbiMismatch`]. A directory or file that
//! cannot be read is [`LayoutError::Device`], with a message starting
//! `CODESCAPE_PTX_DIR`. Either way [`crate::select_engine`] falls back to the
//! CPU engine and reports the reason.
//!
//! Nothing here touches CUDA, so it builds and is tested without the `cuda`
//! feature.
//!
//! ```
//! use codescape_layout::ptx_source::{PtxSource, MANIFEST_FILE, PTX_FILE};
//! use codescape_layout::LayoutError;
//!
//! let dir = std::env::temp_dir().join(format!("codescape-ptx-doc-{}", std::process::id()));
//! std::fs::create_dir_all(&dir).unwrap();
//! std::fs::write(dir.join(PTX_FILE), ".version 9.0\n").unwrap();
//! std::fs::write(
//!     dir.join(MANIFEST_FILE),
//!     "visionclaw_unified source=src/ptx/visionclaw_unified.ptx provenance=fallback-compiler-failure isa=9.0 original=1 rewritten=1\n",
//! )
//! .unwrap();
//! let err = PtxSource::from_dir(&dir).unwrap_err();
//! assert!(matches!(err, LayoutError::AbiMismatch(m) if m.contains("fallback")));
//! std::fs::remove_dir_all(&dir).unwrap();
//! ```

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::abi::{check_provenance, check_ptx_abi, ManifestEntry, UNIFIED_MODULE};
use crate::LayoutError;

/// Environment variable naming the directory that holds the PTX and its manifest.
pub const PTX_DIR_ENV: &str = "CODESCAPE_PTX_DIR";

/// File name of the unified PTX inside [`PTX_DIR_ENV`].
pub const PTX_FILE: &str = "visionclaw_unified.ptx";

/// File name of the build manifest inside [`PTX_DIR_ENV`].
pub const MANIFEST_FILE: &str = "ptx-build-manifest.txt";

/// A verified `visionclaw_unified` PTX artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtxSource {
    /// The directory the artefacts were read from.
    pub dir: PathBuf,
    /// The PTX text, exactly as the build wrote it.
    pub ptx: String,
    /// The manifest line for the module: provenance (always `compiled`),
    /// declared ISA and content tags.
    pub entry: ManifestEntry,
}

impl PtxSource {
    /// Read and verify the artefacts in `$CODESCAPE_PTX_DIR`.
    ///
    /// An unset or empty variable is [`LayoutError::Device`].
    pub fn from_env() -> Result<Self, LayoutError> {
        Self::from_env_value(std::env::var_os(PTX_DIR_ENV))
    }

    /// [`PtxSource::from_env`] with the variable's value passed in, so tests
    /// do not mutate the process environment.
    fn from_env_value(value: Option<OsString>) -> Result<Self, LayoutError> {
        match value {
            Some(dir) if !dir.is_empty() => Self::from_dir(Path::new(&dir)),
            _ => Err(LayoutError::Device(format!(
                "{PTX_DIR_ENV} is not set: point it at the directory holding {PTX_FILE} and {MANIFEST_FILE}"
            ))),
        }
    }

    /// Read and verify [`PTX_FILE`] and [`MANIFEST_FILE`] in `dir`.
    pub fn from_dir(dir: &Path) -> Result<Self, LayoutError> {
        let unreadable = |file: &str, e: std::io::Error| {
            LayoutError::Device(format!("{PTX_DIR_ENV} {}: {file}: {e}", dir.display()))
        };
        if !dir.is_dir() {
            return Err(LayoutError::Device(format!(
                "{PTX_DIR_ENV} {}: not a directory",
                dir.display()
            )));
        }
        let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE))
            .map_err(|e| unreadable(MANIFEST_FILE, e))?;
        let bytes = std::fs::read(dir.join(PTX_FILE)).map_err(|e| unreadable(PTX_FILE, e))?;
        let entry = check_provenance(&manifest, UNIFIED_MODULE)?;
        let tag = content_tag(&bytes);
        if tag != entry.rewritten {
            return Err(LayoutError::AbiMismatch(format!(
                "{} has content tag {tag:016x}, its build manifest says {:016x}",
                dir.join(PTX_FILE).display(),
                entry.rewritten
            )));
        }
        let ptx = String::from_utf8(bytes)
            .map_err(|e| LayoutError::AbiMismatch(format!("{PTX_FILE} is not UTF-8 text: {e}")))?;
        check_ptx_abi(&ptx)?;
        Ok(PtxSource {
            dir: dir.to_path_buf(),
            ptx,
            entry,
        })
    }
}

// Copied verbatim from `crates/visionclaw-gpu/src/ptx_policy.rs` (`content_tag`),
// so that the tag the build writes and the tag checked here are one function.
// Both crates are AGPL-3.0-only works of the same copyright holder.
/// A non-cryptographic FNV-1a content tag.
///
/// Used only to bind a built artefact to the bytes it came from in the build
/// manifest — never as a security or integrity boundary, which is why a real
/// hash function is deliberately not pulled into the build script for it.
///
/// ```
/// use codescape_layout::ptx_source::content_tag;
///
/// // FNV-1a 64-bit test vectors.
/// assert_eq!(content_tag(b""), 0xcbf2_9ce4_8422_2325);
/// assert_eq!(content_tag(b"a"), 0xaf63_dc4c_8601_ec8c);
/// ```
pub fn content_tag(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRESH: &str = include_str!("../tests/fixtures/unified_fresh_212.ptx");
    const STALE: &str = include_str!("../tests/fixtures/unified_committed_172.ptx");

    fn manifest_line(provenance: &str, tag: u64) -> String {
        format!(
            "gpu_clustering_kernels source=src/cuda_sources/gpu_clustering_kernels.cu provenance=compiled isa=9.0 original=1 rewritten=1\n\
             visionclaw_unified source=src/cuda_sources/visionclaw_unified.cu provenance={provenance} isa=9.0 original={tag:016x} rewritten={tag:016x}\n"
        )
    }

    /// A tempdir holding `ptx` and a manifest line with `provenance` and `tag`.
    fn artefacts(ptx: &str, provenance: &str, tag: u64) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PTX_FILE), ptx).unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            manifest_line(provenance, tag),
        )
        .unwrap();
        dir
    }

    #[test]
    fn content_tag_matches_fnv1a_64_vectors() {
        assert_eq!(content_tag(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_tag(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(content_tag(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_fresh_compiled_artefact_is_accepted() {
        let tag = content_tag(FRESH.as_bytes());
        let dir = artefacts(FRESH, "compiled", tag);
        let src = PtxSource::from_dir(dir.path()).unwrap();
        assert_eq!(src.ptx, FRESH);
        assert_eq!(src.entry.module, UNIFIED_MODULE);
        assert_eq!(src.entry.provenance, "compiled");
        assert_eq!(src.entry.isa, "9.0");
        assert_eq!(src.entry.rewritten, tag);
        assert_eq!(src.dir, dir.path());
    }

    #[test]
    fn the_stale_172_byte_layout_is_refused_even_when_compiled_and_tagged() {
        let dir = artefacts(STALE, "compiled", content_tag(STALE.as_bytes()));
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains("172")),
            "{err}"
        );
    }

    #[test]
    fn a_fallback_provenance_is_refused() {
        let dir = artefacts(
            FRESH,
            "fallback-compiler-failure",
            content_tag(FRESH.as_bytes()),
        );
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains("fallback-compiler-failure")),
            "{err}"
        );
    }

    #[test]
    fn a_tag_mismatch_is_refused() {
        let tag = content_tag(FRESH.as_bytes()) ^ 1;
        let dir = artefacts(FRESH, "compiled", tag);
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains(&format!("{tag:016x}"))),
            "{err}"
        );
    }

    #[test]
    fn a_manifest_without_the_unified_line_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PTX_FILE), FRESH).unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            "pagerank source=x provenance=compiled isa=9.0 original=1 rewritten=1\n",
        )
        .unwrap();
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::AbiMismatch(m) if m.contains("no line for visionclaw_unified")),
            "{err}"
        );
    }

    #[test]
    fn a_missing_dir_is_a_device_error() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("absent");
        let err = PtxSource::from_dir(&gone).unwrap_err();
        assert!(
            matches!(&err, LayoutError::Device(m) if m.starts_with("CODESCAPE_PTX_DIR") && m.contains("not a directory")),
            "{err}"
        );
    }

    #[test]
    fn missing_files_are_device_errors_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::Device(m) if m.starts_with("CODESCAPE_PTX_DIR") && m.contains(MANIFEST_FILE)),
            "{err}"
        );
        std::fs::write(dir.path().join(MANIFEST_FILE), manifest_line("compiled", 1)).unwrap();
        let err = PtxSource::from_dir(dir.path()).unwrap_err();
        assert!(
            matches!(&err, LayoutError::Device(m) if m.starts_with("CODESCAPE_PTX_DIR") && m.contains(PTX_FILE)),
            "{err}"
        );
    }

    #[test]
    fn an_unset_or_empty_variable_is_a_device_error() {
        for value in [None, Some(OsString::new())] {
            let err = PtxSource::from_env_value(value).unwrap_err();
            assert!(
                matches!(&err, LayoutError::Device(m) if m.starts_with("CODESCAPE_PTX_DIR is not set")),
                "{err}"
            );
        }
        let tag = content_tag(FRESH.as_bytes());
        let dir = artefacts(FRESH, "compiled", tag);
        let src = PtxSource::from_env_value(Some(dir.path().as_os_str().to_owned())).unwrap();
        assert_eq!(src.entry.rewritten, tag);
    }

    /// Checks a real `visionclaw-gpu` build: the copied tag function must
    /// reproduce the manifest tag the build script wrote.
    #[test]
    #[ignore = "needs a real build: CODESCAPE_PTX_DIR=<dir> cargo test -p codescape-layout -- --ignored real_build"]
    fn real_build_artefacts_verify() {
        let src = PtxSource::from_env().unwrap();
        assert_eq!(content_tag(src.ptx.as_bytes()), src.entry.rewritten);
        eprintln!(
            "{}: isa {} tag {:016x}",
            src.dir.display(),
            src.entry.isa,
            src.entry.rewritten
        );
    }
}
