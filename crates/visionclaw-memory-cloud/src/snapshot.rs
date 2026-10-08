//! Assembling sampled rows into the wire snapshot, its vectors blob and the
//! id → row map used to place sidecar hits in the cloud.

use std::collections::{BTreeSet, HashMap};

use sha2::{Digest, Sha256};

use crate::config::NamespacePatterns;
use crate::pca::{project_to_3d, PcaError, Projector};
use crate::sampling::Allocation;
use crate::vector::{encode_vectors_blob, l2_normalise, parse_ruvector_literal};
use crate::wire::{MemoryCloudMeta, MemoryCloudSnapshot, MemoryCloudStratum, SNAPSHOT_VERSION};

/// Path of the vectors endpoint, relative to the site root.
pub const VECTORS_PATH: &str = "/api/memory-cloud/vectors";

/// One row as read from the sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct SampledRow {
    /// Row metadata.
    pub meta: MemoryCloudMeta,
    /// `embedding::text` — a ruvector literal.
    pub embedding: String,
}

/// A snapshot ready to serve.
#[derive(Debug, Clone)]
pub struct BuiltSnapshot {
    /// The JSON payload.
    pub snapshot: MemoryCloudSnapshot,
    /// L2-normalised vectors, row-major, `count * dim`.
    pub vectors: Vec<f32>,
    /// `vectors` encoded for the wire.
    pub blob: Vec<u8>,
    /// `memory_entries.id` → row index.
    pub index_of: HashMap<String, usize>,
    /// Rows dropped (unparsable, wrong dimension, zero norm, duplicate id).
    pub skipped: usize,
    /// The PCA map behind `snapshot.positions`, for placing a query vector
    /// in the cloud; `None` for an empty sample.
    pub projector: Option<Projector>,
}

/// Short, stable id for a snapshot: the first 12 hex digits of
/// SHA-256 over `generated_at` and the row ids in order.
///
/// ```
/// use visionclaw_memory_cloud::snapshot::snapshot_id;
/// let a = snapshot_id(1, ["x", "y"]);
/// assert_eq!(a.len(), 12);
/// assert_ne!(a, snapshot_id(1, ["y", "x"]));
/// assert_ne!(a, snapshot_id(2, ["x", "y"]));
/// ```
pub fn snapshot_id<'a>(generated_at: i64, ids: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(generated_at.to_le_bytes());
    for id in ids {
        hasher.update(id.as_bytes());
        hasher.update([0u8]);
    }
    hex::encode(&hasher.finalize()[..6])
}

/// Relative URL of a snapshot's vectors blob.
pub fn vectors_url(snapshot_id: &str) -> String {
    format!("{VECTORS_PATH}?snapshot={snapshot_id}")
}

/// Turn sampled rows into a servable snapshot.
///
/// Rows whose literal does not parse, whose dimension differs from the first
/// good row, whose norm is zero, or whose id repeats are dropped and counted
/// in [`BuiltSnapshot::skipped`]. Strata come from `allocations`, with
/// `sampled` counting only rows that survived.
///
/// ```
/// use visionclaw_memory_cloud::config::NamespacePatterns;
/// use visionclaw_memory_cloud::sampling::Allocation;
/// use visionclaw_memory_cloud::snapshot::{build_snapshot, SampledRow};
/// use visionclaw_memory_cloud::wire::MemoryCloudMeta;
///
/// let row = |id: &str, v: &str| SampledRow {
///     meta: MemoryCloudMeta {
///         id: id.into(), key: id.into(), namespace: "n".into(),
///         source_type: "s".into(), updated_at: 0,
///     },
///     embedding: v.into(),
/// };
/// let alloc = [Allocation { namespace: "n".into(), embedded: 9, quota: 2 }];
/// let built = build_snapshot(
///     vec![row("a", "[3,4]"), row("b", "[0,2]")],
///     &alloc,
///     &NamespacePatterns::parse_list("personal-context"),
///     1_700_000_000_000,
/// ).unwrap();
/// assert_eq!(built.snapshot.count, 2);
/// assert_eq!(built.vectors, vec![0.6, 0.8, 0.0, 1.0]);
/// assert_eq!(built.index_of["b"], 1);
/// ```
pub fn build_snapshot(
    rows: Vec<SampledRow>,
    allocations: &[Allocation],
    excluded: &NamespacePatterns,
    generated_at: i64,
) -> Result<BuiltSnapshot, PcaError> {
    let mut dim: Option<usize> = None;
    let mut vectors: Vec<f32> = Vec::new();
    let mut metadata: Vec<MemoryCloudMeta> = Vec::with_capacity(rows.len());
    let mut index_of: HashMap<String, usize> = HashMap::with_capacity(rows.len());
    let mut skipped = 0usize;

    for row in rows {
        if index_of.contains_key(&row.meta.id) {
            skipped += 1;
            continue;
        }
        let Ok(mut v) = parse_ruvector_literal(&row.embedding) else {
            skipped += 1;
            continue;
        };
        if v.is_empty() || dim.is_some_and(|d| d != v.len()) || l2_normalise(&mut v).is_err() {
            skipped += 1;
            continue;
        }
        dim.get_or_insert(v.len());
        index_of.insert(row.meta.id.clone(), metadata.len());
        vectors.extend_from_slice(&v);
        metadata.push(row.meta);
    }

    let dim = dim.unwrap_or(0);
    let (positions, projector) = if dim == 0 {
        (Vec::new(), None)
    } else {
        let p = project_to_3d(&vectors, dim)?;
        let projector = p.projector();
        (p.positions, Some(projector))
    };

    let mut sampled_per_ns: HashMap<&str, u64> = HashMap::new();
    for m in &metadata {
        *sampled_per_ns.entry(m.namespace.as_str()).or_default() += 1;
    }
    let strata = allocations
        .iter()
        .map(|a| MemoryCloudStratum {
            namespace: a.namespace.clone(),
            total: a.embedded,
            sampled: sampled_per_ns
                .get(a.namespace.as_str())
                .copied()
                .unwrap_or(0),
        })
        .collect();
    let namespaces: Vec<String> = metadata
        .iter()
        .map(|m| m.namespace.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let source_types: Vec<String> = metadata
        .iter()
        .map(|m| m.source_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let id = snapshot_id(generated_at, metadata.iter().map(|m| m.id.as_str()));
    let blob = encode_vectors_blob(&vectors);
    let snapshot = MemoryCloudSnapshot {
        version: SNAPSHOT_VERSION,
        vectors_url: vectors_url(&id),
        snapshot_id: id,
        generated_at,
        dim,
        count: metadata.len(),
        positions,
        metadata,
        namespaces,
        source_types,
        strata,
        excluded_namespaces: excluded.as_written(),
    };
    Ok(BuiltSnapshot {
        snapshot,
        vectors,
        blob,
        index_of,
        skipped,
        projector,
    })
}

/// `probes` row indices spread evenly over `count` rows (all rows when
/// `count <= probes`). Because the sample is stratified and sorted by
/// namespace, an even stride touches many namespaces.
///
/// ```
/// use visionclaw_memory_cloud::snapshot::probe_rows;
/// assert_eq!(probe_rows(10, 5), vec![0, 2, 4, 6, 8]);
/// assert_eq!(probe_rows(3, 5), vec![0, 1, 2]);
/// ```
pub fn probe_rows(count: usize, probes: usize) -> Vec<usize> {
    if count <= probes {
        return (0..count).collect();
    }
    (0..probes).map(|i| i * count / probes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::decode_vectors_blob;

    fn row(id: &str, ns: &str, st: &str, v: &str) -> SampledRow {
        SampledRow {
            meta: MemoryCloudMeta {
                id: id.into(),
                key: format!("key-{id}"),
                namespace: ns.into(),
                source_type: st.into(),
                updated_at: 5,
            },
            embedding: v.into(),
        }
    }

    #[test]
    fn bad_rows_are_skipped_and_counted() {
        let rows = vec![
            row("a", "n1", "s1", "[1,0,0]"),
            row("b", "n1", "s2", "[0,1]"),   // wrong dimension
            row("c", "n2", "s1", "[0,0,0]"), // zero norm
            row("d", "n2", "s1", "garbage"), // unparsable
            row("a", "n2", "s1", "[0,0,1]"), // duplicate id
            row("e", "n2", "s3", "[0,2,0]"),
        ];
        let alloc = vec![
            Allocation {
                namespace: "n1".into(),
                embedded: 100,
                quota: 2,
            },
            Allocation {
                namespace: "n2".into(),
                embedded: 50,
                quota: 4,
            },
        ];
        let b = build_snapshot(rows, &alloc, &NamespacePatterns::default(), 42).unwrap();
        assert_eq!(b.skipped, 4);
        assert_eq!(b.snapshot.count, 2);
        assert_eq!(b.snapshot.dim, 3);
        assert_eq!(b.snapshot.positions.len(), 6);
        assert_eq!(b.snapshot.namespaces, vec!["n1", "n2"]);
        assert_eq!(b.snapshot.source_types, vec!["s1", "s3"]);
        assert_eq!(b.snapshot.strata[0].sampled, 1);
        assert_eq!(b.snapshot.strata[1].sampled, 1);
        assert_eq!(b.snapshot.strata[1].total, 50);
        assert_eq!(b.index_of["e"], 1);
        assert_eq!(decode_vectors_blob(&b.blob).unwrap(), b.vectors);
        assert_eq!(b.vectors, vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(
            b.snapshot.vectors_url,
            format!(
                "/api/memory-cloud/vectors?snapshot={}",
                b.snapshot.snapshot_id
            )
        );
    }

    #[test]
    fn empty_sample_is_valid() {
        let b = build_snapshot(Vec::new(), &[], &NamespacePatterns::default(), 1).unwrap();
        assert_eq!(b.snapshot.count, 0);
        assert_eq!(b.snapshot.dim, 0);
        assert!(b.blob.is_empty());
        assert!(b.projector.is_none(), "nothing to project into");
    }

    /// The query point contract (`QueryEcho::position`): a sampled row's own
    /// vector, projected with the snapshot's map, lands on that row's
    /// snapshot position. Rows go in unnormalised and duplicated-direction, as
    /// the sidecar returns them.
    #[test]
    fn projecting_a_sampled_rows_vector_lands_on_its_position() {
        let rows: Vec<SampledRow> = (0..120)
            .map(|i| {
                let v: Vec<f32> = (0..16)
                    .map(|j| ((i * 16 + j) as f32 * 0.61).sin() * (1.0 + (i % 7) as f32))
                    .collect();
                row(
                    &format!("r{i}"),
                    if i % 3 == 0 { "a" } else { "b" },
                    "s",
                    &crate::vector::format_ruvector_literal(&v),
                )
            })
            .collect();
        let b = build_snapshot(rows, &[], &NamespacePatterns::default(), 7).unwrap();
        let proj = b.projector.as_ref().expect("a projector");
        assert_eq!(proj.dim(), 16);
        for (i, v) in b.vectors.as_chunks::<16>().0.iter().enumerate() {
            let at = &b.snapshot.positions[i * 3..i * 3 + 3];
            assert_eq!(proj.project(v).unwrap(), [at[0], at[1], at[2]], "row {i}");
        }
        // an unsampled direction still lands inside the cloud's frame
        let q: Vec<f32> = (0..16).map(|j| (j as f32 * 0.3).cos()).collect();
        let p = proj.project(&q).unwrap();
        assert!(p.iter().all(|c| c.is_finite() && c.abs() < 1000.0), "{p:?}");
        assert!(proj.project(&q[..15]).is_none(), "wrong dimension");
        let mut bad = q.clone();
        bad[3] = f32::NAN;
        assert!(proj.project(&bad).is_none(), "non-finite input");
    }

    #[test]
    fn blob_rows_are_unit_length() {
        let rows: Vec<SampledRow> = (0..50)
            .map(|i| {
                let v: Vec<f32> = (0..8).map(|j| ((i * 8 + j) as f32 * 0.37).cos()).collect();
                row(
                    &format!("r{i}"),
                    "n",
                    "s",
                    &crate::vector::format_ruvector_literal(&v),
                )
            })
            .collect();
        let b = build_snapshot(rows, &[], &NamespacePatterns::default(), 1).unwrap();
        for r in b.vectors.as_chunks::<8>().0 {
            assert!((crate::vector::dot(r, r) - 1.0).abs() < 1e-5);
        }
        assert_eq!(b.blob.len(), 50 * 8 * 4);
    }
}
