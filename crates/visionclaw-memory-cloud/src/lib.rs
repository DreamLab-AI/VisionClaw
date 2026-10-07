//! Pure logic for the VisionClaw live memory cloud (`/api/memory-cloud*`).
//!
//! The server samples the RuVector sidecar's `memory_entries` table, projects
//! the sample to 3-D for the browser, and runs nearest-neighbour queries
//! against the sidecar's own HNSW index. Everything in this crate is free of
//! I/O so it can be unit-tested in milliseconds; the actix handler and the
//! Postgres/embedder clients live in the server crate and stay thin.
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`config`] | environment projection (`MEMORY_CLOUD_*`) and namespace patterns |
//! | [`sampling`] | per-namespace sample allocation (∝ √count, floors, noise down-weighting) |
//! | [`vector`] | ruvector text literals, L2 normalisation, the little-endian f32 blob |
//! | [`pca`] | principal-component projection to 3-D with robust scaling |
//! | [`snippet`] | flattening a `jsonb` value to a short plain-text snippet |
//! | [`validate`] | `POST /api/memory-cloud/query` request validation |
//! | [`recall`] | recall@k of an approximate result list against an exact one |
//! | [`snapshot`] | assembling a sample into the wire snapshot and its blob |
//! | [`wire`] | serde types mirroring `client/src/features/visualisation/memoryCloud/types.ts` |
//!
//! ```
//! use visionclaw_memory_cloud::{pca, vector};
//!
//! let mut rows = vec![3.0_f32, 4.0, 0.0, 6.0, 8.0, 0.0];
//! for row in rows.chunks_mut(3) {
//!     vector::l2_normalise(row).unwrap();
//! }
//! assert!((rows[0] - 0.6).abs() < 1e-6);
//! let projection = pca::project_to_3d(&rows, 3).unwrap();
//! assert_eq!(projection.positions.len(), 6);
//! ```

#![deny(missing_docs)]

pub mod config;
pub mod pca;
pub mod recall;
pub mod sampling;
pub mod snapshot;
pub mod snippet;
pub mod validate;
pub mod vector;
pub mod wire;
