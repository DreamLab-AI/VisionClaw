//! `.generation.json` — the identity stamp Loom promotes against (decision Q8).
//!
//! A generation is `visionGraph@<local sha>` plus a **content digest**, and the
//! two are not redundant: the sha says which commit the build came from, the
//! digest says what the build actually read. A dirty working tree produces a
//! new digest under an unchanged sha, which is precisely the case a bare commit
//! hash would hide.
//!
//! Two further rules keep the identity honest:
//!
//! * A build from a working tree with uncommitted changes under the vault
//!   paths is `visionGraph@<sha>+dirty` with `dirty: true`, so an
//!   uncommitted build can never masquerade as the commit.
//! * The bundle carries the stamp twice: `<out>/.generation.json` lists every
//!   artefact relative to `<out>`, and `<out>/data/.generation.json` — the
//!   marker Loom reads beside its served index — lists only the `data/`
//!   artefacts, relative to `data/` ([`Generation::scoped`]).

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
    /// `visionGraph@<sha>`, suffixed `+dirty` for an uncommitted build.
    pub id: String,
    /// The source commit, or `unknown` outside a git checkout. Always the bare
    /// sha, dirty or not.
    pub commit: String,
    /// `true` when the vault paths had uncommitted changes at build time.
    pub dirty: bool,
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

impl Generation {
    /// The same stamp for the sub-bundle under `prefix` (e.g. `data/`): only
    /// the artefacts beneath it, renamed relative to it. Identity, digest and
    /// counts are unchanged — it is the same generation, seen from inside.
    #[must_use]
    pub fn scoped(&self, prefix: &str) -> Self {
        let prefix = format!("{}/", prefix.trim_end_matches('/'));
        Self {
            artifacts: self
                .artifacts
                .iter()
                .filter_map(|a| {
                    a.name.strip_prefix(&prefix).map(|rest| Artifact {
                        name: rest.to_owned(),
                        sha256: a.sha256.clone(),
                        bytes: a.bytes,
                    })
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// The generation id for `commit`: `visionGraph@<sha>`, plus `+dirty` when
/// the build read uncommitted changes.
#[must_use]
pub fn generation_id(commit: &str, dirty: bool) -> String {
    if dirty {
        format!("visionGraph@{commit}+dirty")
    } else {
        format!("visionGraph@{commit}")
    }
}

/// `true` when `git status --porcelain` reports any change (tracked or
/// untracked) under `paths` in `repo`. Outside a git checkout, or when git is
/// unavailable, there is no commit to masquerade as, so the answer is `false`.
#[must_use]
pub fn git_dirty(repo: impl AsRef<Path>, paths: &[&Path]) -> bool {
    // `git -C <repo>` resolves a relative pathspec against the repository, not
    // the caller's directory, so every path is made absolute first: with
    // `--repo visionGraph`, `visionGraph/knowledge` would otherwise name
    // `visionGraph/visionGraph/knowledge` and report a dirty tree clean.
    let absolute = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(absolute(repo.as_ref()))
        .args(["status", "--porcelain", "--"]);
    for path in paths {
        cmd.arg(absolute(path));
    }
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| !o.stdout.iter().all(u8::is_ascii_whitespace))
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
#[allow(clippy::too_many_arguments)] // The fields of contract C3.
pub fn generation(
    commit: String,
    dirty: bool,
    content_digest: String,
    generated_at: String,
    class_count: usize,
    page_count: usize,
    vocabulary_version: u32,
    stale_after: Option<String>,
    artifacts: Vec<Artifact>,
) -> Generation {
    Generation {
        id: generation_id(&commit, dirty),
        commit,
        dirty,
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
            false,
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
    fn a_dirty_build_says_so_in_its_id() {
        let g = generation(
            "abc1234".into(),
            true,
            "d".into(),
            "t".into(),
            0,
            0,
            1,
            None,
            Vec::new(),
        );
        assert_eq!(g.id, "visionGraph@abc1234+dirty");
        assert_eq!(g.commit, "abc1234", "the commit stays the bare sha");
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(v["dirty"], true);
    }

    #[test]
    fn scoping_keeps_identity_and_rebases_the_artefacts() {
        let g = generation(
            "abc".into(),
            false,
            "d".into(),
            "t".into(),
            3,
            4,
            1,
            None,
            vec![
                Artifact::of("api/search-index.json", b"{}"),
                Artifact::of("data/ontology.ttl", b"ttl"),
                Artifact::of("data/graph/full.bin", b"bin"),
                Artifact::of("database/x", b"not data/"),
            ],
        );
        let data = g.scoped("data");
        assert_eq!(data.id, g.id);
        assert_eq!(data.content_digest, g.content_digest);
        assert_eq!(data.class_count, 3);
        let names: Vec<&str> = data.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["ontology.ttl", "graph/full.bin"]);
        assert_eq!(data.artifacts[0].sha256, g.artifacts[1].sha256);
    }

    /// A throwaway repository with one committed page, or `None` when git is
    /// unavailable in the test environment.
    fn committed_repo() -> Option<tempfile::TempDir> {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
        };
        git(&["init", "-q"])?;
        std::fs::create_dir_all(dir.path().join("knowledge/pages")).unwrap();
        std::fs::write(dir.path().join("knowledge/pages/A.md"), "a").unwrap();
        std::fs::write(dir.path().join("elsewhere.txt"), "x").unwrap();
        git(&["add", "-A"])?;
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ])?;
        Some(dir)
    }

    #[test]
    fn dirtiness_is_scoped_to_the_vault_paths() {
        let Some(repo) = committed_repo() else {
            return; // no git here: `git_dirty` is false by definition
        };
        let knowledge = repo.path().join("knowledge");
        assert!(!git_dirty(repo.path(), &[&knowledge]), "clean after commit");

        std::fs::write(repo.path().join("elsewhere.txt"), "changed").unwrap();
        assert!(
            !git_dirty(repo.path(), &[&knowledge]),
            "a change outside the vault paths does not dirty the build"
        );

        std::fs::write(repo.path().join("knowledge/pages/B.md"), "new").unwrap();
        assert!(
            git_dirty(repo.path(), &[&knowledge]),
            "an untracked page dirties it"
        );
        // Relative paths, as `--repo <relative>` produces, resolve against the
        // caller's directory, not the repository's.
        // (Built by walking up from the current directory to `/`, so the
        // process-global cwd is never changed under parallel tests.)
        let cwd = std::env::current_dir().unwrap();
        let mut relative_repo = std::path::PathBuf::new();
        for _ in cwd.components().skip(1) {
            relative_repo.push("..");
        }
        relative_repo.push(repo.path().strip_prefix("/").unwrap());
        let relative_knowledge = relative_repo.join("knowledge");
        assert!(relative_repo.is_relative());
        assert!(
            git_dirty(&relative_repo, &[&relative_knowledge]),
            "relative repo and vault paths still see the change"
        );
    }

    #[test]
    fn nothing_is_dirty_outside_a_checkout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("page.md"), "x").unwrap();
        if git_sha(dir.path()) == "unknown" {
            assert!(!git_dirty(dir.path(), &[dir.path()]));
        }
    }

    #[test]
    fn stale_after_is_omitted_when_absent() {
        let g = generation(
            "abc".into(),
            false,
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
