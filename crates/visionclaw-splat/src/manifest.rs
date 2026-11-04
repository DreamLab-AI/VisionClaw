//! The splat manifest and the listing published from it.
//!
//! Splat assets are never committed. The manifest (`data/splats/manifest.json`)
//! is: it records where each source came from, the SHA-256 of every source
//! file, the licence and attribution, and the derived SPZ variants with their
//! own SHA-256 values. A variant whose `sha256` is still `null` has not been
//! produced yet (the pipeline needs a WebGPU build host) and is never served.

use crate::address::{address_of_hex, is_sha256_hex};
use crate::licence::{admit_licence, CacheMode, LicenceRefusal};
use serde::{Deserialize, Serialize};

/// Manifest schema version this crate reads and writes.
pub const MANIFEST_VERSION: u32 = 1;

/// Route the server serves a cached blob under; the address is appended.
pub const BLOB_ROUTE: &str = "/api/splats/blob/";

/// The committed manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub assets: Vec<SplatAsset>,
}

/// One splat scene and everything needed to serve and credit it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplatAsset {
    /// Stable lowercase id (`[a-z0-9-]+`).
    pub id: String,
    pub title: String,
    pub author: String,
    /// The page the asset was published on.
    pub source_url: String,
    pub licence: Licence,
    /// An explicit grant from the rights holder, admitting a licence the
    /// rule would otherwise refuse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<String>,
    /// Shown on every view, followed by the licence link and "modified".
    pub attribution: String,
    /// Upstream inputs whose own terms were not checked.
    #[serde(default)]
    pub upstream_unchecked: Vec<String>,
    /// The source files as fetched.
    pub sources: Vec<SourceFile>,
    /// Axis-aligned bounds of the scene in its own units, for the volume fit.
    pub bounds: Bounds,
    pub variants: Vec<Variant>,
}

/// SPDX id (empty for none) and the licence's URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Licence {
    pub spdx: String,
    pub url: String,
}

/// One fetched source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
}

/// Scene bounds, `min` and `max` corners.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// The LOD variants ADR-2170 decision 5 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VariantName {
    #[serde(rename = "desktop-hi")]
    DesktopHi,
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "xr-3")]
    Xr3,
    #[serde(rename = "xr-2")]
    Xr2,
    #[serde(rename = "xr-1")]
    Xr1,
}

impl VariantName {
    pub const ALL: [VariantName; 5] = [
        VariantName::DesktopHi,
        VariantName::Desktop,
        VariantName::Xr3,
        VariantName::Xr2,
        VariantName::Xr1,
    ];

    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesktopHi => "desktop-hi",
            Self::Desktop => "desktop",
            Self::Xr3 => "xr-3",
            Self::Xr2 => "xr-2",
            Self::Xr1 => "xr-1",
        }
    }

    /// The ISS testbed's target splat count for the variant.
    pub fn target_splats(self) -> u32 {
        match self {
            Self::DesktopHi => 3_000_000,
            Self::Desktop => 1_500_000,
            Self::Xr3 => 100_000,
            Self::Xr2 => 50_000,
            Self::Xr1 => 25_000,
        }
    }

    /// Headset variants are SH 0: view-dependent colour is dropped there.
    pub fn is_headset(self) -> bool {
        matches!(self, Self::Xr3 | Self::Xr2 | Self::Xr1)
    }

    /// The headset splat tier (1..=3) this variant serves, if any.
    pub fn headset_tier(self) -> Option<u8> {
        match self {
            Self::Xr1 => Some(1),
            Self::Xr2 => Some(2),
            Self::Xr3 => Some(3),
            _ => None,
        }
    }
}

/// One derived SPZ variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub name: VariantName,
    /// Splat count in the produced file (the target until it is produced).
    pub splats: u32,
    pub sh_degree: u8,
    /// `null` until the pipeline has produced it.
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// A manifest that cannot be used.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ManifestError {
    #[error("unsupported manifest version {0} (expected {MANIFEST_VERSION})")]
    Version(u32),
    #[error("asset id {0:?} is not [a-z0-9-]+")]
    BadId(String),
    #[error("asset id {0} appears twice")]
    DuplicateId(String),
    #[error("asset {asset}: {field} hash {value:?} is not 64 lowercase hex")]
    BadHash {
        asset: String,
        field: String,
        value: String,
    },
    #[error("asset {asset}: variant {variant} appears twice")]
    DuplicateVariant { asset: String, variant: &'static str },
    #[error("asset {asset}: headset variant {variant} must be SH 0, not {sh}")]
    HeadsetSh {
        asset: String,
        variant: &'static str,
        sh: u8,
    },
    #[error("asset {asset}: SH degree {sh} is above 3")]
    ShDegree { asset: String, sh: u8 },
    #[error("asset {0}: bounds min is not below max")]
    Bounds(String),
    #[error("manifest JSON: {0}")]
    Json(String),
}

impl Manifest {
    /// Parse and validate.
    pub fn from_json(text: &str) -> Result<Self, ManifestError> {
        let m: Manifest =
            serde_json::from_str(text).map_err(|e| ManifestError::Json(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    /// Pretty JSON with a trailing newline, the committed form.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("manifest serialises");
        s.push('\n');
        s
    }

    /// Structural checks that do not need the files.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != MANIFEST_VERSION {
            return Err(ManifestError::Version(self.version));
        }
        let mut ids = std::collections::BTreeSet::new();
        for a in &self.assets {
            if a.id.is_empty()
                || !a
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                return Err(ManifestError::BadId(a.id.clone()));
            }
            if !ids.insert(a.id.as_str()) {
                return Err(ManifestError::DuplicateId(a.id.clone()));
            }
            if (0..3).any(|i| a.bounds.min[i] >= a.bounds.max[i]) {
                return Err(ManifestError::Bounds(a.id.clone()));
            }
            for s in &a.sources {
                if !is_sha256_hex(&s.sha256) {
                    return Err(ManifestError::BadHash {
                        asset: a.id.clone(),
                        field: format!("source {}", s.name),
                        value: s.sha256.clone(),
                    });
                }
            }
            let mut names = std::collections::BTreeSet::new();
            for v in &a.variants {
                if !names.insert(v.name) {
                    return Err(ManifestError::DuplicateVariant {
                        asset: a.id.clone(),
                        variant: v.name.as_str(),
                    });
                }
                if v.sh_degree > 3 {
                    return Err(ManifestError::ShDegree {
                        asset: a.id.clone(),
                        sh: v.sh_degree,
                    });
                }
                if v.name.is_headset() && v.sh_degree != 0 {
                    return Err(ManifestError::HeadsetSh {
                        asset: a.id.clone(),
                        variant: v.name.as_str(),
                        sh: v.sh_degree,
                    });
                }
                if let Some(h) = &v.sha256 {
                    if !is_sha256_hex(h) {
                        return Err(ManifestError::BadHash {
                            asset: a.id.clone(),
                            field: format!("variant {}", v.name.as_str()),
                            value: h.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The asset with `id`.
    pub fn asset(&self, id: &str) -> Option<&SplatAsset> {
        self.assets.iter().find(|a| a.id == id)
    }
}

impl SplatAsset {
    /// The licence rule applied to this asset.
    pub fn admit(&self, mode: CacheMode) -> Result<(), LicenceRefusal> {
        admit_licence(&self.licence.spdx, self.grant.as_deref(), mode)
    }
}

/// What `GET /api/splats` returns: only admitted assets, and only the
/// variants that are produced, cached and verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SplatListing {
    pub assets: Vec<ListedAsset>,
}

/// One servable asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedAsset {
    pub id: String,
    pub title: String,
    pub author: String,
    pub source_url: String,
    pub licence: Licence,
    pub attribution: String,
    /// Always true: every served variant is decimated or SH-stripped, so
    /// every view says "modified (decimated)".
    pub modified: bool,
    pub bounds: Bounds,
    pub variants: Vec<ListedVariant>,
}

/// One servable variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedVariant {
    pub name: VariantName,
    pub splats: u32,
    pub sh_degree: u8,
    /// Full SHA-256; clients verify the bytes against it and refuse a
    /// mismatch.
    pub sha256: String,
    /// `sha256-12-<hex>`, the cache key on every tier.
    pub address: String,
    pub bytes: u64,
    /// `/api/splats/blob/<address>`.
    pub url: String,
}

impl ListedAsset {
    /// The listed entry for `asset`, keeping only variants `present` says
    /// are cached and verified.
    pub fn from_asset(asset: &SplatAsset, present: impl Fn(&Variant) -> Option<u64>) -> Self {
        let variants = asset
            .variants
            .iter()
            .filter_map(|v| {
                let sha = v.sha256.as_ref()?;
                let bytes = present(v)?;
                let address = address_of_hex(sha);
                Some(ListedVariant {
                    name: v.name,
                    splats: v.splats,
                    sh_degree: v.sh_degree,
                    sha256: sha.clone(),
                    url: format!("{BLOB_ROUTE}{address}"),
                    address,
                    bytes,
                })
            })
            .collect();
        Self {
            id: asset.id.clone(),
            title: asset.title.clone(),
            author: asset.author.clone(),
            source_url: asset.source_url.clone(),
            licence: asset.licence.clone(),
            attribution: asset.attribution.clone(),
            modified: true,
            bounds: asset.bounds,
            variants,
        }
    }

    /// The credits line every view shows.
    pub fn credits(&self) -> String {
        format!(
            "{} ({} {}), modified (decimated)",
            self.attribution, self.licence.spdx, self.licence.url
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const H1: &str = "6e4a3d5775b6cf9de4941fa6b90030705ed169ae775b3a29301d866f7d6f7797";

    pub(crate) fn sample() -> Manifest {
        Manifest {
            version: 1,
            assets: vec![SplatAsset {
                id: "iss".into(),
                title: "ISS".into(),
                author: "Tamás Máté".into(),
                source_url: "https://superspl.at/scene/cc026f6a".into(),
                licence: Licence {
                    spdx: "CC-BY-4.0".into(),
                    url: "https://creativecommons.org/licenses/by/4.0/".into(),
                },
                grant: None,
                attribution: "ISS by Tamás Máté".into(),
                upstream_unchecked: vec![],
                sources: vec![SourceFile {
                    name: "meta.json".into(),
                    url: "https://example.invalid/meta.json".into(),
                    sha256: H1.into(),
                }],
                bounds: Bounds {
                    min: [-1.0; 3],
                    max: [1.0; 3],
                },
                variants: vec![
                    Variant {
                        name: VariantName::Xr3,
                        splats: 100_000,
                        sh_degree: 0,
                        sha256: Some(H1.into()),
                        bytes: Some(10),
                    },
                    Variant {
                        name: VariantName::Desktop,
                        splats: 1_500_000,
                        sh_degree: 3,
                        sha256: None,
                        bytes: None,
                    },
                ],
            }],
        }
    }

    #[test]
    fn the_sample_round_trips_through_json() {
        let m = sample();
        assert_eq!(Manifest::from_json(&m.to_json()).unwrap(), m);
        assert!(m.to_json().contains("\"xr-3\""));
        assert!(m.to_json().contains("\"shDegree\""));
    }

    #[test]
    fn a_headset_variant_with_sh_is_refused() {
        let mut m = sample();
        m.assets[0].variants[0].sh_degree = 3;
        assert!(matches!(m.validate(), Err(ManifestError::HeadsetSh { .. })));
    }

    #[test]
    fn a_bad_hash_is_refused() {
        let mut m = sample();
        m.assets[0].variants[0].sha256 = Some("ABC".into());
        assert!(matches!(m.validate(), Err(ManifestError::BadHash { .. })));
    }

    #[test]
    fn duplicate_and_malformed_ids_are_refused() {
        let mut m = sample();
        m.assets.push(m.assets[0].clone());
        assert!(matches!(m.validate(), Err(ManifestError::DuplicateId(_))));
        let mut m = sample();
        m.assets[0].id = "../iss".into();
        assert!(matches!(m.validate(), Err(ManifestError::BadId(_))));
    }

    #[test]
    fn listing_keeps_only_produced_present_variants() {
        let m = sample();
        let listed = ListedAsset::from_asset(&m.assets[0], |v| v.bytes);
        assert_eq!(listed.variants.len(), 1);
        let v = &listed.variants[0];
        assert_eq!(v.address, "sha256-12-6e4a3d5775b6");
        assert_eq!(v.url, "/api/splats/blob/sha256-12-6e4a3d5775b6");
        assert!(listed.modified);
        assert!(listed.credits().ends_with("modified (decimated)"));
        // Not present in the cache: not listed.
        assert!(ListedAsset::from_asset(&m.assets[0], |_| None)
            .variants
            .is_empty());
    }
}
