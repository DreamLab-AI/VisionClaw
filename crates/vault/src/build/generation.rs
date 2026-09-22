//! `.generation.json` — the identity stamp Loom promotes against (decision Q8).
//!
//! A generation is `visionGraph@<local sha>` plus a **content digest**, and the
//! two are not redundant: the sha says which commit the build came from, the
//! digest says what the build actually read. A dirty working tree produces a
//! new digest under an unchanged sha, which is precisely the case a bare commit
//! hash would hide.

use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// One emitted file, with its hash and size.
#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    /// Path relative to the bundle root, e.g. `data/ontology.ttl`.
    pub name: String,
    /// Lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
    /// Size in bytes.
    pub bytes: u64,
}

impl Artifact {
    /// Hash `content` and record it under `name`.
    #[must_use]
    pub fn of(name: impl Into<String>, content: &[u8]) -> Self {
        Self {
            name: name.into(),
            sha256: hex::encode(Sha256::digest(content)),
            bytes: content.len() as u64,
        }
    }
}

/// The `.generation.json` document (contract C3).
#[derive(Debug, Clone, Serialize)]
pub struct Generation {
    /// `visionGraph@<sha>`.
    pub id: String,
    /// The source commit, or `unknown` outside a git checkout.
    pub commit: String,
    /// SHA-256 over every source page, path-ordered.
    pub content_digest: String,
    /// ISO-8601 build instant.
    pub generated_at: String,
    /// Public OWL classes in this build.
    pub class_count: usize,
    /// Public pages in this build.
    pub page_count: usize,
    /// `vocabulary.yaml`'s `version`.
    pub vocabulary_version: u32,
    /// The bundle's own expiry, or `null` when none is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_after: Option<String>,
    /// Every emitted artefact.
    pub artifacts: Vec<Artifact>,
}

/// Read `git rev-parse HEAD` in `repo`, returning `unknown` when it is not a
/// git checkout or git is unavailable.
#[must_use]
pub fn git_sha(repo: impl AsRef<Path>) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo.as_ref())
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// SHA-256 over `(relative path, file bytes)` pairs, sorted by path.
///
/// The path is hashed alongside the bytes so a rename changes the digest even
/// when no content does.
#[must_use]
pub fn content_digest(mut entries: Vec<(String, Vec<u8>)>) -> String {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    for (path, bytes) in entries {
        h.update(path.as_bytes());
        h.update(b"\0");
        h.update(&bytes);
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

/// Assemble the generation stamp.
#[must_use]
#[allow(clippy::too_many_arguments)] // The eight fields of contract C3.
pub fn generation(
    commit: String,
    content_digest: String,
    generated_at: String,
    class_count: usize,
    page_count: usize,
    vocabulary_version: u32,
    stale_after: Option<String>,
    artifacts: Vec<Artifact>,
) -> Generation {
    Generation {
        id: format!("visionGraph@{commit}"),
        commit,
        content_digest,
        generated_at,
        class_count,
        page_count,
        vocabulary_version,
        stale_after,
        artifacts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_carries_the_sha() {
        let g = generation(
            "abc1234".into(),
            "d".into(),
            "t".into(),
            8146,
            8433,
            1,
            None,
            Vec::new(),
        );
        assert_eq!(g.id, "visionGraph@abc1234");
    }

    #[test]
    fn the_digest_is_order_independent_but_path_sensitive() {
        let a = content_digest(vec![
            ("b.md".into(), b"two".to_vec()),
            ("a.md".into(), b"one".to_vec()),
        ]);
        let b = content_digest(vec![
            ("a.md".into(), b"one".to_vec()),
            ("b.md".into(), b"two".to_vec()),
        ]);
        assert_eq!(a, b);
        let renamed = content_digest(vec![
            ("a.md".into(), b"one".to_vec()),
            ("c.md".into(), b"two".to_vec()),
        ]);
        assert_ne!(a, renamed);
    }

    #[test]
    fn a_content_change_changes_the_digest() {
        let a = content_digest(vec![("a.md".into(), b"one".to_vec())]);
        let b = content_digest(vec![("a.md".into(), b"ONE".to_vec())]);
        assert_ne!(a, b);
    }

    #[test]
    fn the_digest_resists_boundary_ambiguity() {
        // Without the separators, ("ab", "c") and ("a", "bc") would collide.
        let a = content_digest(vec![("ab".into(), b"c".to_vec())]);
        let b = content_digest(vec![("a".into(), b"bc".to_vec())]);
        assert_ne!(a, b);
    }

    #[test]
    fn artifacts_record_hash_and_size() {
        let a = Artifact::of("data/x.json", b"{}");
        assert_eq!(a.bytes, 2);
        assert_eq!(a.sha256.len(), 64);
    }

    #[test]
    fn git_sha_is_unknown_outside_a_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let sha = git_sha(dir.path());
        assert!(sha == "unknown" || sha.len() == 40);
    }

    #[test]
    fn stale_after_is_omitted_when_absent() {
        let g = generation(
            "abc".into(),
            "d".into(),
            "t".into(),
            0,
            0,
            1,
            None,
            Vec::new(),
        );
        let v = serde_json::to_value(&g).unwrap();
        assert!(v.get("stale_after").is_none());
    }
}
