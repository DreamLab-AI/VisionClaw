//! Shared patch rendering for reviewable proposals, and its exact inverse.
//!
//! [`unified_diff`] renders the diff a `PatchProposal` carries; [`parse`] and
//! [`FilePatch::apply`] read it back and apply it. The applier is strict on
//! purpose — it is the staleness guard of `vault apply`:
//!
//! * every context and removed line must equal the base line at the position
//!   the hunk header states, byte for byte (line terminators included);
//! * there is no fuzz and no offset search: a hunk that does not match where it
//!   says it does is stale, never relocated;
//! * each hunk's body must agree with its own header's line counts, and its
//!   new-side start must agree with where the old side lands it.
//!
//! Lines are split exactly as `similar` splits them (`\n`, `\r\n` and a lone
//! `\r` each end a line), and the `\ No newline at end of file` marker is
//! honoured, so `apply(before, unified_diff(before, after)) == after` for every
//! pair of texts.

use std::fmt;

/// Render a unified diff with repository-relative file labels.
pub fn unified_diff(name: &str, before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}

/// The marker `similar` writes after a line that has no terminator.
const NO_NEWLINE: &str = "\\ No newline at end of file";

/// Why a diff could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// The diff text is not a unified diff this module can read.
    Malformed(String),
    /// The diff is well formed but does not match the base text.
    Stale(String),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed diff: {m}"),
            Self::Stale(m) => write!(f, "stale diff: {m}"),
        }
    }
}

/// One line of a hunk body.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// Present in both texts.
    Context(String),
    /// Present only in the base.
    Removed(String),
    /// Present only in the result.
    Added(String),
}

/// A `@@ -l,n +l,n @@` range: the 1-based start as written, and the length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    start: usize,
    len: usize,
}

impl Range {
    /// Parse `l` or `l,n`.
    fn parse(raw: &str) -> Option<Self> {
        let (start, len) = match raw.split_once(',') {
            Some((s, n)) => (s.parse().ok()?, n.parse().ok()?),
            None => (raw.parse().ok()?, 1),
        };
        // A non-empty range is 1-based; an empty one names the line before it.
        (len == 0 || start >= 1).then_some(Self { start, len })
    }

    /// The 0-based index of the first line the range covers (or, when empty,
    /// the index it inserts at).
    fn index(self) -> usize {
        if self.len == 0 {
            self.start
        } else {
            self.start - 1
        }
    }
}

/// One hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    old: Range,
    new: Range,
    lines: Vec<Line>,
}

/// One file's part of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// The `---` label with its `a/` prefix removed (`/dev/null` stays as is).
    pub old_name: String,
    /// The `+++` label with its `b/` prefix removed.
    pub new_name: String,
    hunks: Vec<Hunk>,
}

/// Split `text` into lines exactly as `similar::TextDiff::from_lines` does,
/// keeping each line's terminator.
fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                out.push(&text[start..i + 2]);
                i += 2;
                start = i;
            }
            b'\r' | b'\n' => {
                out.push(&text[start..=i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// A header token without its single `\n` terminator; `None` when it has any
/// other terminator (the renderer only ever writes `\n` after a header).
fn header_text(token: &str) -> Option<&str> {
    let text = token.strip_suffix('\n')?;
    (!text.ends_with('\r')).then_some(text)
}

/// Parse `@@ -l,n +l,n @@`.
fn parse_hunk_header(token: &str) -> Result<(Range, Range), PatchError> {
    let malformed = || PatchError::Malformed(format!("bad hunk header {:?}", token.trim_end()));
    let text = header_text(token).ok_or_else(malformed)?;
    let inner = text
        .strip_prefix("@@ -")
        .and_then(|t| t.strip_suffix(" @@"))
        .ok_or_else(malformed)?;
    let (old, new) = inner.split_once(" +").ok_or_else(malformed)?;
    Ok((
        Range::parse(old).ok_or_else(malformed)?,
        Range::parse(new).ok_or_else(malformed)?,
    ))
}

/// Parse a unified diff of one or more files.
///
/// Hunk bodies are delimited by their headers' line counts, never by line
/// prefixes, so a removed line that itself starts `-- ` is not mistaken for
/// the next file's `---` header.
///
/// # Errors
/// [`PatchError::Malformed`] on anything that is not the shape
/// [`unified_diff`] produces.
pub fn parse(diff: &str) -> Result<Vec<FilePatch>, PatchError> {
    let tokens = split_lines(diff);
    let mut files = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let old_name = header_text(tokens[i])
            .and_then(|t| t.strip_prefix("--- "))
            .ok_or_else(|| {
                PatchError::Malformed(format!(
                    "expected a `--- ` file header, found {:?}",
                    tokens[i].trim_end()
                ))
            })?;
        let new_name = tokens
            .get(i + 1)
            .and_then(|t| header_text(t))
            .and_then(|t| t.strip_prefix("+++ "))
            .ok_or_else(|| {
                PatchError::Malformed(format!("`--- {old_name}` is not followed by `+++ `"))
            })?;
        i += 2;

        let mut hunks = Vec::new();
        while let Some(token) = tokens.get(i).filter(|t| t.starts_with("@@ ")) {
            let (old, new) = parse_hunk_header(token)?;
            i += 1;
            let (mut old_seen, mut new_seen) = (0, 0);
            let mut lines: Vec<Line> = Vec::new();
            while old_seen < old.len || new_seen < new.len {
                let token = tokens.get(i).ok_or_else(|| {
                    PatchError::Malformed(format!(
                        "hunk @@ -{},{} +{},{} @@ ends early",
                        old.start, old.len, new.start, new.len
                    ))
                })?;
                i += 1;
                // Classify by the ASCII tag byte before slicing, so a line
                // opening with a multi-byte character is malformed, not a panic.
                let Some(&tag @ (b' ' | b'-' | b'+')) = token.as_bytes().first() else {
                    return Err(PatchError::Malformed(format!(
                        "unexpected hunk line {:?}",
                        token.trim_end()
                    )));
                };
                let mut value = &token[1..];
                if tokens
                    .get(i)
                    .is_some_and(|t| t.trim_end_matches(['\r', '\n']) == NO_NEWLINE)
                {
                    // `similar` wrote a `\n` after a line that has none.
                    value = value.strip_suffix('\n').ok_or_else(|| {
                        PatchError::Malformed("a no-newline marker after a bare `\\r`".into())
                    })?;
                    i += 1;
                }
                let value = value.to_owned();
                let line = match tag {
                    b' ' => {
                        old_seen += 1;
                        new_seen += 1;
                        Line::Context(value)
                    }
                    b'-' => {
                        old_seen += 1;
                        Line::Removed(value)
                    }
                    _ => {
                        new_seen += 1;
                        Line::Added(value)
                    }
                };
                lines.push(line);
            }
            if old_seen != old.len || new_seen != new.len {
                return Err(PatchError::Malformed(format!(
                    "hunk @@ -{},{} +{},{} @@ holds {old_seen} old and {new_seen} new lines",
                    old.start, old.len, new.start, new.len
                )));
            }
            hunks.push(Hunk { old, new, lines });
        }
        if hunks.is_empty() {
            return Err(PatchError::Malformed(format!(
                "`--- {old_name}` has no hunks"
            )));
        }
        files.push(FilePatch {
            old_name: old_name.strip_prefix("a/").unwrap_or(old_name).to_owned(),
            new_name: new_name.strip_prefix("b/").unwrap_or(new_name).to_owned(),
            hunks,
        });
    }
    Ok(files)
}

impl FilePatch {
    /// Apply this patch to `before`, exactly.
    ///
    /// # Errors
    /// [`PatchError::Stale`] when any context or removed line differs from the
    /// base at the stated position, a hunk starts before the previous one ends
    /// or past the end of the base, or a hunk's new-side start disagrees with
    /// where its old side lands it.
    pub fn apply(&self, before: &str) -> Result<String, PatchError> {
        let old = split_lines(before);
        let mut out: Vec<&str> = Vec::with_capacity(old.len());
        let mut cursor = 0;
        for (n, hunk) in self.hunks.iter().enumerate() {
            let n = n + 1;
            let start = hunk.old.index();
            if start < cursor || start > old.len() {
                return Err(PatchError::Stale(format!(
                    "hunk {n} starts at line {}, outside the {} line(s) still unconsumed",
                    hunk.old.start,
                    old.len().saturating_sub(cursor)
                )));
            }
            out.extend_from_slice(&old[cursor..start]);
            if hunk.new.index() != out.len() {
                return Err(PatchError::Stale(format!(
                    "hunk {n} claims result line {} but lands at line {}",
                    hunk.new.start,
                    out.len() + 1
                )));
            }
            let mut at = start;
            for line in &hunk.lines {
                match line {
                    Line::Context(text) | Line::Removed(text) => {
                        let found = old.get(at).copied();
                        if found != Some(text.as_str()) {
                            return Err(PatchError::Stale(format!(
                                "hunk {n}, line {}: expected {:?}, found {:?}",
                                at + 1,
                                text,
                                found.unwrap_or("<end of file>")
                            )));
                        }
                        if matches!(line, Line::Context(_)) {
                            out.push(found.unwrap_or_default());
                        }
                        at += 1;
                    }
                    Line::Added(text) => out.push(text),
                }
            }
            cursor = at;
        }
        out.extend_from_slice(&old[cursor..]);
        Ok(out.concat())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diff `before` → `after`, parse it, apply it to `before`.
    fn round_trip(before: &str, after: &str) -> String {
        let diff = unified_diff("Page", before, after);
        let files = parse(&diff).unwrap_or_else(|e| panic!("{e}\n{diff}"));
        assert_eq!(files.len(), 1, "{diff}");
        assert_eq!(files[0].old_name, "Page");
        assert_eq!(files[0].new_name, "Page");
        files[0]
            .apply(before)
            .unwrap_or_else(|e| panic!("{e}\n{diff}"))
    }

    fn numbered(n: usize) -> String {
        (1..=n)
            .flat_map(|i| ["line ".to_owned(), i.to_string(), "\n".to_owned()])
            .collect()
    }

    #[test]
    fn a_single_insertion_round_trips() {
        let before = "---\ntype: Class\nis-a:\n- '[[Token]]'\nstatus: stable\n---\nbody\n";
        let after = "---\ntype: Class\nis-a:\n- '[[Token]]'\ndisjoint-with:\n- '[[Non-Fungible Token]]'\nstatus: stable\n---\nbody\n";
        assert_eq!(round_trip(before, after), after);
    }

    #[test]
    fn multiple_hunks_round_trip() {
        let before = numbered(40);
        let after = before
            .replace("line 3\n", "line three\n")
            .replace("line 20\n", "")
            .replace("line 35\n", "line 35\ninserted\n");
        let diff = unified_diff("Page", &before, &after);
        assert_eq!(diff.matches("\n@@ ").count(), 3, "{diff}");
        assert_eq!(round_trip(&before, &after), after);
    }

    #[test]
    fn additions_at_the_end_of_the_file_round_trip() {
        let before = numbered(10);
        let after = format!("{before}line 11\nline 12\n");
        assert_eq!(round_trip(&before, &after), after);
    }

    #[test]
    fn additions_at_the_start_of_the_file_round_trip() {
        let before = numbered(10);
        let after = format!("line 0\n{before}");
        assert_eq!(round_trip(&before, &after), after);
    }

    #[test]
    fn a_missing_trailing_newline_round_trips_both_ways() {
        // Gaining one, losing one, and changing the unterminated last line.
        assert_eq!(round_trip("a\nb\nc", "a\nb\nc\n"), "a\nb\nc\n");
        assert_eq!(round_trip("a\nb\nc\n", "a\nb\nc"), "a\nb\nc");
        assert_eq!(round_trip("a\nb\nc", "a\nB\nc"), "a\nB\nc");
        assert_eq!(round_trip("a\nb", "a\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn crlf_and_lone_cr_line_endings_round_trip() {
        assert_eq!(
            round_trip("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\n"),
            "a\r\nB\r\nc\r\n"
        );
        assert_eq!(round_trip("a\rb\rc\r", "a\rb\rd\r"), "a\rb\rd\r");
    }

    #[test]
    fn a_removed_line_that_looks_like_a_header_is_still_a_body_line() {
        let before = "a\n-- b\n++ c\n@@ d\nz\n";
        let after = "a\nz\n";
        assert_eq!(round_trip(before, after), after);
    }

    #[test]
    fn whole_file_replacement_and_deletion_round_trip() {
        assert_eq!(round_trip("x\ny\n", "p\nq\nr\n"), "p\nq\nr\n");
        assert_eq!(round_trip("x\ny\n", ""), "");
        assert_eq!(round_trip("", "x\n"), "x\n");
    }

    #[test]
    fn a_context_mismatch_is_stale() {
        let before = numbered(10);
        let after = before.replace("line 5\n", "line five\n");
        let diff = unified_diff("Page", &before, &after);
        let patch = &parse(&diff).unwrap()[0];
        // The same line count, but a context line now reads differently.
        let drifted = before.replace("line 4\n", "line four\n");
        assert!(matches!(patch.apply(&drifted), Err(PatchError::Stale(_))));
        // A removed line that changed underneath is stale too.
        let drifted = before.replace("line 5\n", "line 5 edited\n");
        assert!(matches!(patch.apply(&drifted), Err(PatchError::Stale(_))));
        // Whitespace counts: there is no fuzz.
        let drifted = before.replace("line 6\n", "line 6 \n");
        assert!(matches!(patch.apply(&drifted), Err(PatchError::Stale(_))));
    }

    #[test]
    fn a_shifted_base_is_stale_not_relocated() {
        // Every hunk line still exists, one line lower: no offset search.
        let before = numbered(10);
        let after = before.replace("line 5\n", "line five\n");
        let diff = unified_diff("Page", &before, &after);
        let patch = &parse(&diff).unwrap()[0];
        let shifted = format!("line 0\n{before}");
        assert!(matches!(patch.apply(&shifted), Err(PatchError::Stale(_))));
    }

    #[test]
    fn wrong_line_numbers_are_stale() {
        let before = numbered(10);
        let after = before.replace("line 5\n", "line five\n");
        let diff = unified_diff("Page", &before, &after);
        assert!(diff.contains("@@ -2,7 +2,7 @@"), "{diff}");
        for wrong in ["@@ -3,7 +3,7 @@", "@@ -2,7 +3,7 @@", "@@ -9,7 +9,7 @@"] {
            let edited = diff.replace("@@ -2,7 +2,7 @@", wrong);
            let patch = &parse(&edited).unwrap()[0];
            assert!(
                matches!(patch.apply(&before), Err(PatchError::Stale(_))),
                "{wrong}"
            );
        }
        // A hunk past the end of the file.
        let edited = diff.replace("@@ -2,7 +2,7 @@", "@@ -40,7 +40,7 @@");
        let patch = &parse(&edited).unwrap()[0];
        assert!(matches!(patch.apply(&before), Err(PatchError::Stale(_))));
    }

    #[test]
    fn a_hunk_disagreeing_with_its_own_counts_is_malformed() {
        let before = numbered(10);
        let after = before.replace("line 5\n", "line five\n");
        let diff = unified_diff("Page", &before, &after);
        let edited = diff.replace("@@ -2,7 +2,7 @@", "@@ -2,9 +2,9 @@");
        assert!(matches!(parse(&edited), Err(PatchError::Malformed(_))));
    }

    #[test]
    fn garbage_is_malformed() {
        for bad in [
            "not a diff\n",
            "--- a/X\n",
            "--- a/X\n+++ b/X\n",
            "--- a/X\n+++ b/X\n@@ -1 +1 @@\n?x\n",
            "--- a/X\n+++ b/X\n@@ -a +1 @@\n x\n",
            "--- a/X\n+++ b/X\n@@ -1 +1 @@\né\n",
        ] {
            assert!(
                matches!(parse(bad), Err(PatchError::Malformed(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_multi_file_diff_parses_into_one_patch_per_file() {
        let mut diff = unified_diff("A", "a\n", "b\n");
        diff.push_str(&unified_diff("B", "c\n", "d\n"));
        let files = parse(&diff).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.old_name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
        assert_eq!(files[1].apply("c\n").unwrap(), "d\n");
    }

    #[test]
    fn line_splitting_matches_similar() {
        for text in ["", "a", "a\n", "a\r\nb", "a\rb\r\n\r\nc", "\n\n"] {
            let ours = split_lines(text);
            let theirs: Vec<&str> = similar::TextDiff::from_lines(text, "")
                .old_slices()
                .to_vec();
            assert_eq!(ours, theirs, "{text:?}");
        }
    }
}
