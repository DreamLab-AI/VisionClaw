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
#[cfg(test)]
use visionclaw_domain::models::metadata::wire_timestamp;
use visionclaw_domain::models::metadata::TemporalKeys;

/// One page's commit history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHistory {
    /// Committer time of the newest commit that touched the page.
    pub last_commit: DateTime<Utc>,
    /// How many commits touched the page. A rename starts a new history (no rename following).
    pub change_count: u32,
}

impl PageHistory {
    /// This page's [`TemporalKeys`], written by the one temporal-key writer. `last_modified` is
    /// the last commit, the precedent `FileService` set for GitHub-sourced metadata.
    pub fn temporal_keys(&self) -> TemporalKeys {
        TemporalKeys {
            last_modified: Some(self.last_commit),
            last_commit: Some(self.last_commit),
            change_count: Some(self.change_count),
        }
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

/// Paths per GitHub GraphQL history query. 100 aliases measured at 1 rate-limit point.
pub(crate) const GRAPHQL_BATCH: usize = 100;

/// One GraphQL query asking, for each path, its commit count and newest commit time on
/// `branch`. Aliases are `p0..pN` in `paths` order; every literal is JSON-escaped, which GraphQL
/// string syntax accepts.
pub(crate) fn github_history_query(
    owner: &str,
    repo: &str,
    branch: &str,
    paths: &[String],
) -> String {
    let lit = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    let aliases: Vec<String> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            format!(
                "p{i}: history(first: 1, path: {}) {{ totalCount nodes {{ committedDate }} }}",
                lit(p)
            )
        })
        .collect();
    format!(
        "query {{ repository(owner: {}, name: {}) {{ object(expression: {}) {{ ... on Commit {{ {} }} }} }} }}",
        lit(owner),
        lit(repo),
        lit(branch),
        aliases.join(" ")
    )
}

/// The GraphQL `errors` of a reply as one line (`TYPE: message; …`), or `None` when it has none.
/// GitHub answers a rate-limited or otherwise refused query with status 200 and this array.
pub(crate) fn graphql_errors(body: &serde_json::Value) -> Option<String> {
    let errors = body.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }
    Some(
        errors
            .iter()
            .map(|e| {
                let kind = e.get("type").and_then(|t| t.as_str()).unwrap_or("ERROR");
                let message = e.get("message").and_then(|m| m.as_str()).unwrap_or("");
                format!("{kind}: {message}")
            })
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Read a [`github_history_query`] response. A path with no commits, or an alias missing from
/// a partial response, gives no entry. A reply whose `errors` came with no repository object is
/// an error naming them, so the caller treats it as a failed batch, not as "no history"; a
/// partial reply keeps what it carries (the caller reports its errors).
pub(crate) fn parse_github_history(
    body: &serde_json::Value,
    paths: &[String],
) -> Result<HistoryMap, String> {
    let mut out = HistoryMap::new();
    let Some(object) = body.pointer("/data/repository/object") else {
        return match graphql_errors(body) {
            Some(errors) => Err(format!("GitHub GraphQL errors: {errors}")),
            None => Ok(out),
        };
    };
    for (i, path) in paths.iter().enumerate() {
        let Some(h) = object.get(format!("p{i}")) else {
            continue;
        };
        let count = h.get("totalCount").and_then(|c| c.as_u64()).unwrap_or(0);
        let at = h
            .pointer("/nodes/0/committedDate")
            .and_then(|d| d.as_str())
            .and_then(|d| DateTime::parse_from_rfc3339(d).ok());
        if let (Some(at), true) = (at, count > 0) {
            out.insert(
                path.clone(),
                PageHistory {
                    last_commit: at.with_timezone(&Utc),
                    change_count: u32::try_from(count).unwrap_or(u32::MAX),
                },
            );
        }
    }
    Ok(out)
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
    fn a_200_reply_carrying_only_errors_is_a_failed_batch() {
        let body = serde_json::json!({
            "errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]
        });
        let err = parse_github_history(&body, &["knowledge/A.md".to_string()])
            .expect_err("an errors-only reply must not read as 'no history'");
        assert!(err.contains("RATE_LIMITED"), "{err}");
        assert!(err.contains("API rate limit exceeded"), "{err}");
    }

    #[test]
    fn a_partial_reply_keeps_its_data_and_reports_its_errors() {
        let body = serde_json::json!({
            "data": {"repository": {"object": {
                "p0": {"totalCount": 2, "nodes": [{"committedDate": "2026-11-04T13:51:04Z"}]}
            }}},
            "errors": [{"type": "NOT_FOUND", "message": "p1 path"}]
        });
        let paths = ["knowledge/A.md".to_string(), "knowledge/B.md".to_string()];
        let h = parse_github_history(&body, &paths).expect("partial data is kept");
        assert_eq!(h["knowledge/A.md"].change_count, 2);
        assert_eq!(h.len(), 1);
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
        .temporal_keys()
        .write(&mut m);
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

    #[test]
    fn the_github_query_escapes_every_literal_and_aliases_in_order() {
        let q = github_history_query(
            "o",
            "r",
            "main",
            &["knowledge/A \"b\".md".into(), "working/C.md".into()],
        );
        assert!(
            q.contains(r#"p0: history(first: 1, path: "knowledge/A \"b\".md")"#),
            "{q}"
        );
        assert!(q.contains(r#"p1: history(first: 1, path: "working/C.md")"#));
        assert!(q.contains(r#"repository(owner: "o", name: "r") { object(expression: "main")"#));
    }

    /// The shape of the live response from the 2026-11-06 probe of jjohare/visionGraph.
    #[test]
    fn a_github_response_gives_count_and_newest_commit() {
        let body = serde_json::json!({"data": {"repository": {"object": {
            "p0": {"totalCount": 3, "nodes": [{"committedDate": "2026-10-28T11:02:16Z"}]},
            "p1": {"totalCount": 0, "nodes": []}
        }}}});
        let paths = vec![
            "knowledge/A.md".to_string(),
            "knowledge/New.md".to_string(),
            "x.md".to_string(),
        ];
        let h = parse_github_history(&body, &paths).unwrap();
        assert_eq!(h.len(), 1, "no commits and a missing alias give no entry");
        assert_eq!(h["knowledge/A.md"].change_count, 3);
        assert_eq!(
            wire_timestamp(&h["knowledge/A.md"].last_commit),
            "2026-10-28T11:02:16Z"
        );
        assert!(
            parse_github_history(&serde_json::json!({"errors": []}), &paths)
                .unwrap()
                .is_empty()
        );
    }

    /// Live check of the query builder and parser against GitHub. Needs network and a token:
    /// `GITHUB_TOKEN=… cargo test --lib live_github_history -- --ignored`.
    #[tokio::test]
    #[ignore = "needs network and GITHUB_TOKEN"]
    async fn live_github_history_matches_the_probe() {
        let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN");
        let paths = vec![
            "knowledge/.gitignore".to_string(),
            "working/.gitattributes".to_string(),
        ];
        let query = github_history_query("jjohare", "visionGraph", "main", &paths);
        let body: serde_json::Value = reqwest::Client::new()
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "visionclaw-history-test")
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let h = parse_github_history(&body, &paths).unwrap();
        assert_eq!(h.len(), 2, "{body}");
        assert!(h["knowledge/.gitignore"].change_count >= 3);
    }
}
