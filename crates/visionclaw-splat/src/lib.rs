//! Gaussian splat asset contract (ADR-2170).
//!
//! A splat scene is its own spatial volume, served as SPZ variants produced
//! offline. This crate is the one definition of everything both ends must
//! agree on:
//!
//! - [`manifest`]: the committed splat manifest (sources, licence, variants
//!   and their SHA-256 values) and the listing the server publishes from it.
//! - [`licence`]: the admission rule. Only licences that allow redistribution
//!   *and* adaptation enter a public cache (decimation and SH stripping are
//!   adaptations).
//! - [`address`]: SHA-256 and the ADR-2023 `sha256-12-<hex>` content address.
//! - [`store`]: the content-addressed cache directory. A hash mismatch is a
//!   refusal, never a retry.
//! - [`spz`]: the SPZ v3 codec. v3 is pinned as the served format because it
//!   is a single gzip stream, which the headset decodes with pure Rust; v4's
//!   per-stream zstd is refused at admission.
//!
//! The server (`src/services/splat_store.rs`) and the Quest client
//! (`xr-client/rust`) both link this crate; the desktop client mirrors the
//! listing shape in `client/src/features/splats/types.ts`.

pub mod address;
pub mod licence;
pub mod manifest;
pub mod spz;
pub mod store;

pub use address::{content_address, is_content_address, sha256_hex};
pub use licence::{admit_licence, CacheMode, LicenceRefusal};
pub use manifest::{
    ListedAsset, ListedVariant, Manifest, SplatAsset, SplatListing, Variant, VariantName,
};
pub use store::{SplatStore, StoreError};
