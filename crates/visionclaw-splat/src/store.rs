//! The content-addressed splat cache.
//!
//! Variants live flat in one directory as `<sha256-12-hex>.spz`. Opening the
//! store re-hashes every produced variant against the manifest; only files
//! that match and whose asset passes the licence rule are served. A mismatch
//! is a refusal, never a retry.

use crate::address::{address_of_hex, is_content_address, sha256_hex, sha256_hex_reader};
use crate::licence::{CacheMode, LicenceRefusal};
use crate::manifest::{ListedAsset, Manifest, SourceFile, SplatAsset, SplatListing, Variant};
use crate::spz;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Why a file was kept out of the cache.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("asset {asset}: {refusal}")]
    Licence {
        asset: String,
        refusal: LicenceRefusal,
    },
    #[error("{what}: SHA-256 {got} does not match the manifest's {want}")]
    HashMismatch {
        what: String,
        want: String,
        got: String,
    },
    #[error("asset {asset} variant {variant}: the manifest has no SHA-256 for it yet")]
    Unproduced { asset: String, variant: String },
    #[error("asset {asset} variant {variant}: {field} is {got}, the manifest says {want}")]
    Header {
        asset: String,
        variant: String,
        field: &'static str,
        want: u32,
        got: u32,
    },
    #[error("asset {asset} variant {variant}: {source}")]
    Spz {
        asset: String,
        variant: String,
        source: spz::SpzError,
    },
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

fn io_err(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// A served variant.
#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    bytes: u64,
}

/// The verified cache and the listing built from it.
#[derive(Debug)]
pub struct SplatStore {
    manifest: Manifest,
    dir: PathBuf,
    entries: BTreeMap<String, Entry>,
    listing: SplatListing,
}

impl SplatStore {
    /// Verify `dir` against `manifest` in `mode`. Returns the store and every
    /// refusal met on the way; a refused variant is simply not served. A
    /// missing variant file is not a refusal (it has not been admitted yet).
    pub fn open(manifest: Manifest, dir: impl Into<PathBuf>, mode: CacheMode) -> (Self, Vec<StoreError>) {
        let dir = dir.into();
        let mut refusals = Vec::new();
        let mut entries = BTreeMap::new();
        let mut assets = Vec::new();
        for asset in &manifest.assets {
            if let Err(refusal) = asset.admit(mode) {
                refusals.push(StoreError::Licence {
                    asset: asset.id.clone(),
                    refusal,
                });
                continue;
            }
            for v in &asset.variants {
                let Some(want) = &v.sha256 else { continue };
                let address = address_of_hex(want);
                let path = dir.join(format!("{address}.spz"));
                let file = match fs::File::open(&path) {
                    Ok(f) => f,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        refusals.push(io_err(&path, e));
                        continue;
                    }
                };
                let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
                match sha256_hex_reader(std::io::BufReader::new(file)) {
                    Ok(got) if &got == want => {
                        entries.insert(address, Entry { path, bytes });
                    }
                    Ok(got) => refusals.push(StoreError::HashMismatch {
                        what: format!("{} {}", asset.id, v.name.as_str()),
                        want: want.clone(),
                        got,
                    }),
                    Err(e) => refusals.push(io_err(&path, e)),
                }
            }
            let listed = ListedAsset::from_asset(asset, |v| {
                v.sha256
                    .as_deref()
                    .and_then(|h| entries.get(&address_of_hex(h)))
                    .map(|e| e.bytes)
            });
            if !listed.variants.is_empty() {
                assets.push(listed);
            }
        }
        let store = Self {
            manifest,
            dir,
            entries,
            listing: SplatListing { assets },
        };
        (store, refusals)
    }

    /// The listing `GET /api/splats` returns.
    pub fn listing(&self) -> &SplatListing {
        &self.listing
    }

    /// The manifest the store was opened with.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The cache directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file for a served address. Malformed or unserved addresses get
    /// `None`, so no request input ever reaches a path join unchecked.
    pub fn blob_path(&self, address: &str) -> Option<(&Path, u64)> {
        if !is_content_address(address) {
            return None;
        }
        self.entries.get(address).map(|e| (e.path.as_path(), e.bytes))
    }
}

/// Check a fetched source file against the manifest.
pub fn verify_source(source: &SourceFile, bytes: &[u8]) -> Result<(), StoreError> {
    let got = sha256_hex(bytes);
    if got == source.sha256 {
        Ok(())
    } else {
        Err(StoreError::HashMismatch {
            what: format!("source {}", source.name),
            want: source.sha256.clone(),
            got,
        })
    }
}

/// Admit one produced variant file into `dir`.
///
/// The asset must pass the licence rule, the bytes must be SPZ v3 whose
/// header matches the manifest's splat count and SH degree, and they must
/// hash to the manifest's SHA-256. The file is written beside its final name
/// and renamed, so a reader never sees half a blob. Returns the address.
pub fn admit_variant(
    asset: &SplatAsset,
    variant: &Variant,
    bytes: &[u8],
    dir: &Path,
    mode: CacheMode,
) -> Result<String, StoreError> {
    asset.admit(mode).map_err(|refusal| StoreError::Licence {
        asset: asset.id.clone(),
        refusal,
    })?;
    let vname = variant.name.as_str().to_string();
    let header = spz::read_header(bytes).map_err(|source| StoreError::Spz {
        asset: asset.id.clone(),
        variant: vname.clone(),
        source,
    })?;
    check_header(asset, variant, &header)?;
    let want = variant.sha256.as_ref().ok_or_else(|| StoreError::Unproduced {
        asset: asset.id.clone(),
        variant: vname.clone(),
    })?;
    let got = sha256_hex(bytes);
    if &got != want {
        return Err(StoreError::HashMismatch {
            what: format!("{} {vname}", asset.id),
            want: want.clone(),
            got,
        });
    }
    let address = address_of_hex(want);
    fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let final_path = dir.join(format!("{address}.spz"));
    let tmp = dir.join(format!(".{address}.spz.partial"));
    let mut f = fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, &final_path).map_err(|e| io_err(&final_path, e))?;
    Ok(address)
}

/// The header must match what the manifest promises.
pub fn check_header(
    asset: &SplatAsset,
    variant: &Variant,
    header: &spz::Header,
) -> Result<(), StoreError> {
    let mismatch = |field, want: u32, got: u32| StoreError::Header {
        asset: asset.id.clone(),
        variant: variant.name.as_str().to_string(),
        field,
        want,
        got,
    };
    if header.num_splats != variant.splats {
        return Err(mismatch("splat count", variant.splats, header.num_splats));
    }
    if header.sh_degree != variant.sh_degree {
        return Err(mismatch(
            "SH degree",
            variant.sh_degree as u32,
            header.sh_degree as u32,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::tests::sample;
    use crate::manifest::VariantName;
    use crate::spz::{encode_v3, Splats};

    fn spz_bytes(n: usize) -> Vec<u8> {
        let mut s = Splats::default();
        for i in 0..n {
            s.positions.extend_from_slice(&[i as f32, 0.0, 0.0]);
            s.scales.extend_from_slice(&[0.1; 3]);
            s.rotations.extend_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            s.colors.extend_from_slice(&[0.5, 0.5, 0.5, 1.0]);
        }
        encode_v3(&s, 12)
    }

    /// A manifest whose xr-3 variant is exactly `bytes`.
    fn manifest_for(bytes: &[u8], n: u32) -> Manifest {
        let mut m = sample();
        let v = &mut m.assets[0].variants[0];
        v.sha256 = Some(sha256_hex(bytes));
        v.splats = n;
        v.bytes = Some(bytes.len() as u64);
        m
    }

    #[test]
    fn an_admitted_variant_is_listed_and_served() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let m = manifest_for(&bytes, 5);
        let addr = admit_variant(
            &m.assets[0],
            &m.assets[0].variants[0],
            &bytes,
            dir.path(),
            CacheMode::Public,
        )
        .unwrap();
        let (store, refusals) = SplatStore::open(m, dir.path(), CacheMode::Public);
        assert!(refusals.is_empty(), "{refusals:?}");
        let listed = &store.listing().assets[0].variants;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, VariantName::Xr3);
        assert_eq!(listed[0].address, addr);
        let (path, len) = store.blob_path(&addr).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(len, bytes.len() as u64);
        // No stray partial file.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn admission_refuses_a_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let mut m = manifest_for(&bytes, 5);
        // A valid SPZ whose header matches, but not the file the manifest names.
        m.assets[0].variants[0].sha256 = Some("0".repeat(64));
        assert!(matches!(
            admit_variant(&m.assets[0], &m.assets[0].variants[0], &bytes, dir.path(), CacheMode::Public),
            Err(StoreError::HashMismatch { .. })
        ));
        // Nothing is written on a refusal.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn admission_refuses_a_header_that_disagrees_with_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let m = manifest_for(&bytes, 6);
        assert!(matches!(
            admit_variant(&m.assets[0], &m.assets[0].variants[0], &bytes, dir.path(), CacheMode::Public),
            Err(StoreError::Header { field: "splat count", .. })
        ));
    }

    #[test]
    fn admission_refuses_a_refused_licence() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let mut m = manifest_for(&bytes, 5);
        m.assets[0].licence.spdx = "CC-BY-NC-4.0".into();
        assert!(matches!(
            admit_variant(&m.assets[0], &m.assets[0].variants[0], &bytes, dir.path(), CacheMode::Public),
            Err(StoreError::Licence { .. })
        ));
        // The private dev cache takes it.
        assert!(admit_variant(
            &m.assets[0],
            &m.assets[0].variants[0],
            &bytes,
            dir.path(),
            CacheMode::PrivateDev
        )
        .is_ok());
    }

    #[test]
    fn a_tampered_cache_file_is_refused_at_open_and_not_served() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let m = manifest_for(&bytes, 5);
        let addr = admit_variant(
            &m.assets[0],
            &m.assets[0].variants[0],
            &bytes,
            dir.path(),
            CacheMode::Public,
        )
        .unwrap();
        fs::write(dir.path().join(format!("{addr}.spz")), b"tampered").unwrap();
        let (store, refusals) = SplatStore::open(m, dir.path(), CacheMode::Public);
        assert!(matches!(refusals[..], [StoreError::HashMismatch { .. }]));
        assert!(store.blob_path(&addr).is_none());
        assert!(store.listing().assets.is_empty());
    }

    #[test]
    fn a_licence_refused_asset_is_not_listed_even_if_cached() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = spz_bytes(5);
        let mut m = manifest_for(&bytes, 5);
        admit_variant(
            &m.assets[0],
            &m.assets[0].variants[0],
            &bytes,
            dir.path(),
            CacheMode::Public,
        )
        .unwrap();
        m.assets[0].licence.spdx = "CC-BY-ND-4.0".into();
        let (store, refusals) = SplatStore::open(m, dir.path(), CacheMode::Public);
        assert!(matches!(refusals[..], [StoreError::Licence { .. }]));
        assert!(store.listing().assets.is_empty());
    }

    #[test]
    fn blob_path_refuses_traversal_and_unserved_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = SplatStore::open(sample(), dir.path(), CacheMode::Public);
        assert!(store.blob_path("../../etc/passwd").is_none());
        assert!(store.blob_path("sha256-12-000000000000").is_none());
    }

    #[test]
    fn a_fetched_source_is_verified() {
        let m = sample();
        let src = &m.assets[0].sources[0];
        assert!(matches!(
            verify_source(src, b"not the file"),
            Err(StoreError::HashMismatch { .. })
        ));
    }
}
