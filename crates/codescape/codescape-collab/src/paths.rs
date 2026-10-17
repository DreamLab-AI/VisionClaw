//! Rewrite paths from agent hooks into repository space and resolve them to
//! file ids.
//!
//! An activity hook reports paths as the agent sees them (for example
//! `/home/devuser/workspace/project/src/main.rs`), but the server indexes the
//! repository under a different mount (`/repos/visionclaw`). A [`PathMap`]
//! rewrites the prefix, and [`resolve_paths`] then finds the file id.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

/// A malformed `CODESCAPE_PATH_MAP` entry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathMapError {
    /// The entry has no `=` between source and target.
    #[error("path map entry {entry:?} has no '='")]
    MissingSeparator {
        /// The offending entry.
        entry: String,
    },
    /// The entry's source prefix is empty.
    #[error("path map entry {entry:?} has an empty source prefix")]
    EmptyPrefix {
        /// The offending entry.
        entry: String,
    },
}

/// Prefix-rewrite rules. When several prefixes match, the longest wins.
///
/// ```
/// use codescape_collab::PathMap;
/// let m: PathMap = "/home/u/project=/repos/vc:/home/u/project/agentbox=/repos/ab".parse().unwrap();
/// assert_eq!(m.rewrite("/home/u/project/src/main.rs"), "/repos/vc/src/main.rs");
/// assert_eq!(m.rewrite("/home/u/project/agentbox/flake.nix"), "/repos/ab/flake.nix");
/// assert_eq!(m.rewrite("/home/u/project2/x"), "/home/u/project2/x");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMap {
    /// `(from, to)`, sorted longest `from` first.
    rules: Vec<(String, String)>,
}

/// Remove trailing slashes but keep a bare root `/`.
fn trim_slashes(s: &str) -> &str {
    let t = s.trim_end_matches('/');
    if t.is_empty() && s.starts_with('/') {
        "/"
    } else {
        t
    }
}

impl PathMap {
    /// An empty map, which rewrites nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse the environment form `FROM=TO[:FROM=TO…]`. Empty entries, such as
    /// a trailing `:`, are skipped. Trailing slashes are dropped from both
    /// sides. An empty `TO` makes matching paths relative.
    pub fn parse(spec: &str) -> Result<Self, PathMapError> {
        let mut m = Self::new();
        for entry in spec.split(':').map(str::trim).filter(|e| !e.is_empty()) {
            let (from, to) =
                entry
                    .split_once('=')
                    .ok_or_else(|| PathMapError::MissingSeparator {
                        entry: entry.to_owned(),
                    })?;
            if from.trim().is_empty() {
                return Err(PathMapError::EmptyPrefix {
                    entry: entry.to_owned(),
                });
            }
            m.insert(from, to);
        }
        Ok(m)
    }

    /// Add a rule. A rule with the same normalised `from` is replaced.
    pub fn insert(&mut self, from: &str, to: &str) {
        let from = trim_slashes(from.trim()).to_owned();
        let to = trim_slashes(to.trim()).to_owned();
        self.rules.retain(|(f, _)| *f != from);
        self.rules.push((from, to));
        self.rules.sort_by_key(|r| std::cmp::Reverse(r.0.len()));
    }

    /// Number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// `true` when there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rules as `(from, to)`, longest prefix first.
    pub fn rules(&self) -> impl Iterator<Item = (&str, &str)> {
        self.rules.iter().map(|(f, t)| (f.as_str(), t.as_str()))
    }

    /// Rewrite `path` with the longest matching prefix. A prefix only matches
    /// on a whole path component: `/a/b` matches `/a/b` and `/a/b/c`, never
    /// `/a/bc`. Paths that match no rule come back unchanged.
    pub fn rewrite(&self, path: &str) -> String {
        for (from, to) in &self.rules {
            let rest = if from == "/" {
                path.strip_prefix('/')
            } else {
                path.strip_prefix(from.as_str()).and_then(|r| {
                    if r.is_empty() {
                        Some(r)
                    } else {
                        r.strip_prefix('/')
                    }
                })
            };
            if let Some(rest) = rest {
                return match (to.as_str(), rest) {
                    (_, "") => to.clone(),
                    ("", r) => r.to_owned(),
                    ("/", r) => format!("/{r}"),
                    (t, r) => format!("{t}/{r}"),
                };
            }
        }
        path.to_owned()
    }
}

impl FromStr for PathMap {
    type Err = PathMapError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Map paths to file ids, in input order, without duplicates.
///
/// Leading `./` is ignored. A path that is not a key itself but is absolute is
/// matched by its longest suffix that is a key, so
/// `/repos/vc/src/main.rs` resolves to `src/main.rs` rather than to a
/// root-level `main.rs`. Unresolvable paths are skipped.
///
/// ```
/// use std::{collections::HashMap, sync::Arc};
/// use codescape_collab::resolve_paths;
/// let files: HashMap<Arc<str>, u32> = [("src/main.rs".into(), 1), ("main.rs".into(), 2)].into();
/// let ids = resolve_paths(&files, &["/repos/vc/src/main.rs".into(), "./main.rs".into(), "nope".into()]);
/// assert_eq!(ids, vec![1, 2]);
/// ```
pub fn resolve_paths(files: &HashMap<Arc<str>, u32>, paths: &[String]) -> Vec<u32> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        let mut p = p.trim();
        while let Some(r) = p.strip_prefix("./") {
            p = r;
        }
        let hit = files.get(p).copied().or_else(|| {
            if !p.starts_with('/') {
                return None;
            }
            p.match_indices('/')
                .map(|(i, _)| &p[i + 1..])
                .filter(|s| !s.is_empty())
                .find_map(|s| files.get(s).copied())
        });
        if let Some(id) = hit {
            if seen.insert(id) {
                out.push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_rewrite_cases() {
        let m = PathMap::parse("/home/devuser/workspace/project/=/repos/visionclaw/").unwrap();
        let cases = [
            (
                "/home/devuser/workspace/project/src/a.rs",
                "/repos/visionclaw/src/a.rs",
            ),
            ("/home/devuser/workspace/project", "/repos/visionclaw"),
            ("/home/devuser/workspace/project/", "/repos/visionclaw"),
            (
                "/home/devuser/workspace/project2/a.rs",
                "/home/devuser/workspace/project2/a.rs",
            ),
            ("relative/a.rs", "relative/a.rs"),
        ];
        for (input, want) in cases {
            assert_eq!(m.rewrite(input), want, "{input}");
        }
    }

    #[test]
    fn longest_prefix_wins_regardless_of_order() {
        let m: PathMap = "/a=/x:/a/b=/y:".parse().unwrap();
        assert_eq!(m.rewrite("/a/b/c"), "/y/c");
        assert_eq!(m.rewrite("/a/c"), "/x/c");
        assert_eq!(m.len(), 2);
        let m: PathMap = "/a/b=/y:/a=/x".parse().unwrap();
        assert_eq!(m.rewrite("/a/b/c"), "/y/c");
    }

    #[test]
    fn empty_target_relativises_and_root_rules_work() {
        let m: PathMap = "/w/p=".parse().unwrap();
        assert_eq!(m.rewrite("/w/p/src/x.rs"), "src/x.rs");
        let m: PathMap = "/=/mnt".parse().unwrap();
        assert_eq!(m.rewrite("/etc/x"), "/mnt/etc/x");
        let m: PathMap = "/w=/".parse().unwrap();
        assert_eq!(m.rewrite("/w/x"), "/x");
    }

    #[test]
    fn parse_errors_and_replacement() {
        assert!(PathMap::parse("").unwrap().is_empty());
        assert_eq!(
            PathMap::parse("/a"),
            Err(PathMapError::MissingSeparator { entry: "/a".into() })
        );
        assert_eq!(
            PathMap::parse("=/b"),
            Err(PathMapError::EmptyPrefix {
                entry: "=/b".into()
            })
        );
        let mut m = PathMap::parse("/a=/x").unwrap();
        m.insert("/a/", "/z");
        assert_eq!(m.rules().collect::<Vec<_>>(), vec![("/a", "/z")]);
    }

    #[test]
    fn resolve_prefers_longest_suffix_and_dedupes() {
        let files: HashMap<Arc<str>, u32> = [
            ("src/lib/mod.rs".into(), 1),
            ("mod.rs".into(), 2),
            ("lib/mod.rs".into(), 3),
        ]
        .into();
        let ids = resolve_paths(
            &files,
            &[
                "/r/src/lib/mod.rs".into(),
                "src/lib/mod.rs".into(),
                "/elsewhere/lib/mod.rs".into(),
                "other/mod.rs".into(), // relative and not a key: no suffix matching
                "/".into(),
            ],
        );
        assert_eq!(ids, vec![1, 3]);
    }
}
