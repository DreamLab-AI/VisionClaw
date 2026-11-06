//! Per-page commit history for the ADR-2184/2185 temporal columns.
//!
//! The change markers a [`super::CorpusSource`] lists (`mtime:size`, a blob SHA) carry no commit
//! time, and a checkout's mtimes are its clone time, so neither may stand in for one. A source
//! that can read real history supplies a [`HistoryMap`]; every other source supplies none, and
//! its nodes carry no temporal keys (the wire contract is "omit when absent").

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use visionclaw_domain::models::metadata::{
    wire_timestamp, CHANGE_COUNT_KEY, LAST_COMMIT_KEY, LAST_MODIFIED_KEY,
};

/// One page's commit history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHistory {
    /// Committer time of the newest commit that touched the page.
    pub last_commit: DateTime<Utc>,
    /// How many commits touched the page. A rename starts a new history (no rename following).
    pub change_count: u32,
}

impl PageHistory {
    /// Write the temporal keys onto a node's metadata map. `last_modified` is the last commit,
    /// the precedent `FileService` set for GitHub-sourced metadata.
    pub fn write_keys(&self, node_metadata: &mut HashMap<String, String>) {
        let at = wire_timestamp(&self.last_commit);
        node_metadata.insert(LAST_MODIFIED_KEY.to_string(), at.clone());
        node_metadata.insert(LAST_COMMIT_KEY.to_string(), at);
        node_metadata.insert(CHANGE_COUNT_KEY.to_string(), self.change_count.to_string());
    }
}

/// Page history keyed by [`super::CorpusPage::path`].
pub type HistoryMap = HashMap<String, PageHistory>;

/// Upper bound on one history read; a slower read is abandoned and the sync goes on without it.
const GIT_LOG_TIMEOUT: Duration = Duration::from_secs(120);

/// Commit-start marker in the `git log` format; never part of a path or a date.
const COMMIT_MARK: char = '\u{1}';

/// Parse `git log --format=%x01%cI --name-only -z` output. Commits arrive newest first, so a
/// path's first appearance is its last commit and its appearance count is its change count.
pub(crate) fn parse_git_log(raw: &[u8]) -> HistoryMap {
    let text = String::from_utf8_lossy(raw);
    let mut out = HistoryMap::new();
    let mut current: Option<DateTime<Utc>> = None;
    for token in text.split('\0') {
        let token = token.trim_start_matches('\n');
        if let Some(date) = token.strip_prefix(COMMIT_MARK) {
            current = DateTime::parse_from_rfc3339(date.trim())
                .ok()
                .map(|d| d.with_timezone(&Utc));
            continue;
        }
        let (Some(at), false) = (current, token.is_empty()) else {
            continue;
        };
        out.entry(token.to_string())
            .and_modify(|h: &mut PageHistory| h.change_count += 1)
            .or_insert(PageHistory {
                last_commit: at,
                change_count: 1,
            });
    }
    out
}

/// Read the history of every page under `base_paths` with one `git log` over the vault at
/// `root`. Paths come back relative to `root` (`--relative`), the form the listing uses. Errors
/// name the cause; the caller treats any error as "no history".
pub(crate) async fn git_history(root: &Path, base_paths: &[String]) -> Result<HistoryMap, String> {
    let mut cmd = tokio::process::Command::new("git");
    // The vault is mounted read-only and owned by another user; trust exactly this root.
    cmd.arg("-c")
        .arg(format!("safe.directory={}", root.display()))
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--format=%x01%cI",
            "--name-only",
            "--no-renames",
            "-z",
            "--relative",
            "--",
        ])
        .args(base_paths)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_LOG_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git log timed out after {}s", GIT_LOG_TIMEOUT.as_secs()))?
        .map_err(|e| format!("git unavailable: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(parse_git_log(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn newest_commit_wins_and_every_commit_counts() {
        let raw = b"\x012026-11-04T14:51:04+01:00\0\nknowledge/A.md\0knowledge/B c.md\0\
\x012026-10-01T08:00:00Z\0\nknowledge/A.md\0\0\x012026-09-01T08:00:00Z\0\nknowledge/A.md\0";
        let h = parse_git_log(raw);
        assert_eq!(
            h["knowledge/A.md"],
            PageHistory {
                last_commit: Utc.with_ymd_and_hms(2026, 11, 4, 13, 51, 4).unwrap(),
                change_count: 3,
            }
        );
        assert_eq!(h["knowledge/B c.md"].change_count, 1);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn empty_output_is_no_history() {
        assert!(parse_git_log(b"").is_empty());
    }

    #[test]
    fn a_page_gets_all_three_keys_with_exact_strings() {
        let mut m = HashMap::new();
        PageHistory {
            last_commit: Utc.with_ymd_and_hms(2026, 11, 4, 13, 51, 4).unwrap(),
            change_count: 7,
        }
        .write_keys(&mut m);
        assert_eq!(m["last_modified"], "2026-11-04T13:51:04Z");
        assert_eq!(m["last_commit"], "2026-11-04T13:51:04Z");
        assert_eq!(m["change_count"], "7");
    }

    #[tokio::test]
    async fn a_directory_that_is_not_a_repository_is_an_error_not_a_guess() {
        let dir = std::env::temp_dir().join(format!("vc-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = git_history(&dir, &["knowledge".to_string()]).await;
        std::fs::remove_dir_all(&dir).ok();
        assert!(r.is_err(), "{r:?}");
    }
}
