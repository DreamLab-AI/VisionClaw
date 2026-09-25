//! `vault repair bodies` — rewrite Logseq outliner bodies as Obsidian markdown.
//!
//! The fence-to-properties migration moved every page's metadata into YAML
//! frontmatter and left the body alone. The body is still a Logseq outline:
//! headings are bullets (`- ### Overview`), every paragraph is an indented
//! bullet, images carry Logseq's `{:height 841, :width 800}` sizing, and the
//! blocks the migration inlined in place of `{{embed}}` repeat their first
//! child. Obsidian renders all of that, but as a nested list with literal
//! attribute text, not as the document it is.
//!
//! This pass rewrites the body only. The frontmatter block is carried through
//! byte for byte, and so is everything inside a code fence apart from the
//! outline indentation around it.
//!
//! # The rules
//!
//! The body is read as a sequence of outline **blocks** (a `- ` bullet and its
//! continuation lines) and **free lines** (anything at column 0 that is not a
//! bullet: the leading definition paragraph, an inlined blockquote). Each
//! block is emitted as one of:
//!
//! * **Heading** — a bullet whose content is `#`…`######`. Emitted flush, with
//!   a blank line either side; any continuation lines follow as a paragraph.
//!   A heading closes the list around it, so its children start a new list.
//! * **Break** — a bullet whose content is `---`. Emitted as a thematic break.
//! * **Standalone** — a fence, a table, a blockquote, raw HTML or an
//!   image-only line. Emitted as its own markdown block, indented under the
//!   enclosing list item when it has one, so the list is not broken.
//! * **Empty** — a bare `-`. Dropped; its children are emitted as if they were
//!   its parent's.
//! * **List item** — everything else. Emitted as `- ` at two spaces per level,
//!   where the level counts only the list-item ancestors since the nearest
//!   heading.
//!
//! Outside code, `![alt](src){:height H, :width W}` becomes Obsidian's
//! `![alt|W](src)`, and an immediately repeated blockquote line is dropped.
//! Logseq macros with an Obsidian equivalent are rewritten: `{{video URL}}`
//! becomes an embed, `{{twitter}}` / `{{tweet}}` a link, and the diagram
//! renderer and `{{evalparent}}` controls are dropped.
//! A list item's Logseq task marker becomes a task: `TODO`, `DOING`, `NOW`
//! and `LATER` become `[ ]`, `DONE` becomes `[x]`.
//!
//! The conversion is idempotent: a converted body is its own conversion, so
//! `--check` on a converted corpus reports nothing.
//!
//! ```
//! use vault::bodies::convert_body;
//!
//! let logseq = "- ### Overview\n  - First point.\n    - Detail.\n  - Second point.\n";
//! assert_eq!(
//!     convert_body(logseq),
//!     "### Overview\n\n- First point.\n  - Detail.\n- Second point.\n"
//! );
//! // Converting again changes nothing.
//! assert_eq!(convert_body(&convert_body(logseq)), convert_body(logseq));
//! ```

use std::path::PathBuf;

use serde::Serialize;

use crate::repair::Scope;

/// Columns a tab is worth when measuring outline indentation.
///
/// Logseq writes a block's continuation lines as the bullet's own indentation
/// plus two spaces, so a tab counted as two columns keeps a tab-indented
/// bullet and its space-indented continuation lines on one scale.
const TAB_WIDTH: usize = 2;

/// Rewrite a whole page: the frontmatter block verbatim, the body converted.
///
/// ```
/// use vault::bodies::convert;
///
/// let page = "---\ntype: Note\n---\n- # Title\n\t- Point\n";
/// assert_eq!(convert(page), "---\ntype: Note\n---\n# Title\n\n- Point\n");
/// ```
#[must_use]
pub fn convert(text: &str) -> String {
    let split = vault_core::frontmatter::split(text);
    let head_len = text.len() - split.body.len();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..head_len]);
    out.push_str(&stable_body(split.body));
    out
}

/// [`convert_body`] repeated until it stops changing, at most four times.
///
/// One pass is its own fixpoint on every well-formed body. A body with fence
/// markers written mid-line can need a second: whether such a fence closes
/// depends on the text below it, and the first pass moves that text. Running
/// to the fixpoint is what lets `--check` promise that a converted corpus
/// reports nothing.
fn stable_body(body: &str) -> String {
    let mut current = convert_body(body);
    for _ in 0..3 {
        let next = convert_body(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// One unit of the parsed outline.
#[derive(Debug)]
enum Item {
    /// A column-0 line that is not a bullet, carried through as written.
    Free(String),
    /// A bullet and its continuation lines, indentation relative to the
    /// bullet's content column.
    Block {
        /// The bullet's column, which decides nesting.
        col: usize,
        /// The column continuation lines are relative to.
        content_col: usize,
        /// The first line's content, then each continuation line.
        lines: Vec<String>,
    },
}

/// How a block is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Heading,
    Break,
    Standalone,
    Empty,
    ListItem,
    /// A free line; never returned by [`classify`].
    Free,
}

/// The fence a line opens, as (marker character, run length).
fn fence_open(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// Whether `line` closes a fence opened with `fence`.
fn fence_closes(line: &str, fence: (char, usize)) -> bool {
    let t = line.trim();
    let n = t.chars().take_while(|&x| x == fence.0).count();
    n >= fence.1 && t.chars().all(|x| x == fence.0)
}

/// Leading indentation in columns, and the byte offset where content starts.
fn indent_of(line: &str) -> (usize, usize) {
    let mut cols = 0;
    for (i, ch) in line.char_indices() {
        match ch {
            ' ' => cols += 1,
            '\t' => cols += TAB_WIDTH,
            _ => return (cols, i),
        }
    }
    (cols, line.len())
}

/// Remove up to `cols` columns of leading whitespace.
fn strip_cols(line: &str, cols: usize) -> &str {
    let mut taken = 0;
    for (i, ch) in line.char_indices() {
        if taken >= cols {
            return &line[i..];
        }
        match ch {
            ' ' => taken += 1,
            '\t' => taken += TAB_WIDTH,
            _ => return &line[i..],
        }
    }
    ""
}

/// Parse a body into free lines and outline blocks.
fn parse(body: &str) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    // Whether the last item is a block still accepting continuation lines.
    let mut open_block = false;
    // Whether the previous line was blank: a markdown block boundary.
    let mut prev_blank = false;
    // For a fence opened in a block and never closed anywhere below: the
    // block's content column. Logseq ends such a fence with its block, so it
    // is closed at the first line that leaves the block.
    let mut bounded: Option<usize> = None;
    let all: Vec<&str> = body.lines().map(|l| l.trim_end_matches('\r')).collect();

    for (i, &line) in all.iter().enumerate() {
        let (cols, at) = indent_of(line);
        let rest = &line[at..];
        let bullet = rest == "-" || rest.starts_with("- ") || rest.starts_with("-\t");

        if let Some(f) = fence {
            let leaves = bounded.is_some_and(|cc| !rest.is_empty() && (bullet || cols < cc));
            if !leaves {
                if fence_closes(line, f) {
                    fence = None;
                    bounded = None;
                }
                push_continuation(&mut items, open_block, line);
                continue;
            }
            close_fence(&mut items, f);
            fence = None;
            bounded = None;
        }

        if bullet {
            let content = rest[1..].trim_start().to_owned();
            fence = fence_open(&content);
            bounded = unclosed_below(&all[i + 1..], fence).then_some(cols + 2);
            items.push(Item::Block {
                col: cols,
                content_col: cols + 2,
                lines: vec![content],
            });
            open_block = true;
            prev_blank = false;
            continue;
        }

        if rest.is_empty() {
            prev_blank = true;
            if open_block {
                push_continuation(&mut items, true, "");
            } else {
                items.push(Item::Free(String::new()));
            }
            continue;
        }

        let was_blank = std::mem::replace(&mut prev_blank, false);

        if cols == 0 || !open_block {
            open_block = false;
            fence = fence_open(line);
            items.push(Item::Free(line.to_owned()));
            continue;
        }

        fence = fence_open(rest);
        if let Some(Item::Block { content_col, .. }) = items.last() {
            bounded = unclosed_below(&all[i + 1..], fence).then_some(*content_col);
        }
        // A fence, table, quote or HTML block after a blank line, indented
        // less than the open block's content: in markdown it belongs to an
        // ANCESTOR item, not to the block above it. This is how a converted
        // body nests such a block under its list item, so reading it back the
        // same way is what makes the conversion idempotent.
        if was_blank && starts_standalone(rest) {
            if let Some(Item::Block { content_col, .. }) = items.last() {
                if cols < *content_col {
                    items.push(Item::Block {
                        col: cols.saturating_sub(1),
                        content_col: cols,
                        lines: vec![rest.to_owned()],
                    });
                    continue;
                }
            }
        }
        push_continuation(&mut items, true, line);
    }
    if let (Some(f), true) = (fence, bounded.is_some()) {
        close_fence(&mut items, f);
    }
    items
}

/// Whether a fence just opened is never closed in `below`.
fn unclosed_below(below: &[&str], fence: Option<(char, usize)>) -> bool {
    fence.is_some_and(|f| !below.iter().any(|l| fence_closes(l, f)))
}

/// Close the open block's fence with a matching marker.
fn close_fence(items: &mut [Item], fence: (char, usize)) {
    if let Some(Item::Block { lines, .. }) = items.last_mut() {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        lines.push(std::iter::repeat_n(fence.0, fence.1).collect());
    }
}

/// Whether a line opens a block that is never a list item's paragraph.
fn starts_standalone(content: &str) -> bool {
    fence_open(content).is_some()
        || content.starts_with('|')
        || content.starts_with('>')
        || content.starts_with('<')
        || image_only(content)
}

/// Append a continuation line to the open block, or a free line if none.
fn push_continuation(items: &mut Vec<Item>, open_block: bool, line: &str) {
    if open_block {
        if let Some(Item::Block {
            content_col, lines, ..
        }) = items.last_mut()
        {
            lines.push(strip_cols(line, *content_col).to_owned());
            return;
        }
    }
    items.push(Item::Free(line.to_owned()));
}

/// Whether a line is nothing but markdown images.
fn image_only(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("![") {
        return false;
    }
    let mut rest = t;
    while let Some(r) = rest.strip_prefix("![") {
        let Some(close) = r.find("](") else {
            return false;
        };
        let Some(end) = r[close + 2..].find(')') else {
            return false;
        };
        rest = r[close + 2 + end + 1..].trim_start();
    }
    rest.is_empty()
}

fn classify(lines: &[String]) -> Kind {
    let first = lines.first().map_or("", |s| s.trim());
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Kind::Empty;
    }
    let hashes = first.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && first[hashes..].starts_with(' ') {
        return Kind::Heading;
    }
    if lines.len() == 1 && (first == "---" || first == "***" || first == "___") {
        return Kind::Break;
    }
    if starts_standalone(first) {
        return Kind::Standalone;
    }
    Kind::ListItem
}

/// The output being assembled, with the blank-line discipline in one place.
struct Out {
    lines: Vec<String>,
    /// What the last non-blank thing emitted was.
    last: Option<Kind>,
}

impl Out {
    fn blank(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
    }
}

/// Convert a Logseq outline body to Obsidian markdown.
///
/// See the module documentation for the rules.
#[must_use]
pub fn convert_body(body: &str) -> String {
    let items = parse(body);
    let mut out = Out {
        lines: Vec::new(),
        last: None,
    };
    // Ancestors of the current block: (bullet column, list level its
    // children take).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for item in items {
        match item {
            Item::Free(line) => {
                stack.clear();
                if let Some(f) = fence {
                    if fence_closes(&line, f) {
                        fence = None;
                    }
                    out.push(line);
                    continue;
                }
                if line.is_empty() {
                    out.blank();
                    continue;
                }
                if out.last.is_some_and(|k| k != Kind::Free) {
                    out.blank();
                }
                fence = fence_open(&line);
                let line = if fence.is_some() {
                    line
                } else {
                    fix_inline(&line)
                };
                out.push(line);
                out.last = Some(Kind::Free);
            }
            Item::Block { col, mut lines, .. } => {
                while stack.last().is_some_and(|&(c, _)| c >= col) {
                    stack.pop();
                }
                let level = stack.last().map_or(0, |&(_, l)| l);
                while lines.last().is_some_and(|l| l.trim().is_empty()) && lines.len() > 1 {
                    lines.pop();
                }
                let emptied = !lines[0].trim().is_empty();
                let mut lines = fix_inline_outside_code(&lines);
                // A macro that was the block's whole first line (the Logseq
                // `{{renderer code_diagram,mermaid}}` above a fence) leaves it
                // empty; the block is what follows. Only then: a bullet that
                // was always empty keeps its shape.
                if emptied && lines[0].is_empty() {
                    while lines.len() > 1 && lines[0].trim().is_empty() {
                        lines.remove(0);
                    }
                }
                let kind = classify(&lines);
                let child_level = match kind {
                    Kind::Heading | Kind::Break => 0,
                    Kind::ListItem => level + 1,
                    Kind::Standalone | Kind::Empty | Kind::Free => level,
                };
                // A heading or a rule closes every open list, so nothing
                // after it may nest under a list item from before it.
                if matches!(kind, Kind::Heading | Kind::Break) {
                    for entry in &mut stack {
                        entry.1 = 0;
                    }
                }
                stack.push((col, child_level));
                emit_block(&mut out, kind, level, &lines);
            }
        }
    }

    let mut lines = collapse_blank_runs(out.lines);
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Collapse every run of blank lines outside code to one, and drop a
/// blockquote line that repeats the line before it.
///
/// Blank runs carry no meaning in markdown prose, and a block's own
/// continuation lines can begin or end with one, which would otherwise stack
/// against the separators the emitter inserts. The repeated quote line is the
/// migration's: inlining an `{{embed}}` wrote the block's first child twice.
fn collapse_blank_runs(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut fence: Option<(char, usize)> = None;
    for line in lines {
        if let Some(f) = fence {
            if fence_closes(&line, f) {
                fence = None;
            }
            out.push(line);
            continue;
        }
        fence = fence_open(&line);
        if line.trim().is_empty() {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let quoted = line.trim_start();
        if quoted.starts_with('>') && quoted.trim() != ">" && out.last().is_some_and(|l| *l == line)
        {
            continue;
        }
        out.push(line);
    }
    out
}

/// Emit one block's lines.
fn emit_block(out: &mut Out, kind: Kind, level: usize, lines: &[String]) {
    match kind {
        Kind::Empty | Kind::Free => {}
        Kind::Heading => {
            out.blank();
            out.push(lines[0].trim().to_owned());
            out.blank();
            let rest = &lines[1..];
            if rest.iter().any(|l| !l.trim().is_empty()) {
                for l in rest {
                    out.push(l.clone());
                }
                out.blank();
            }
            out.last = Some(Kind::Heading);
        }
        Kind::Break => {
            out.blank();
            out.push("---".to_owned());
            out.blank();
            out.last = Some(Kind::Break);
        }
        Kind::Standalone => {
            let pad = "  ".repeat(level);
            out.blank();
            for l in lines {
                out.push(indent(&pad, l));
            }
            out.blank();
            out.last = Some(Kind::Standalone);
        }
        Kind::ListItem => {
            if out.last != Some(Kind::ListItem) {
                out.blank();
            }
            let pad = "  ".repeat(level);
            out.push(format!("{pad}- {}", task_marker(&lines[0])));
            let cont = format!("{pad}  ");
            for l in &lines[1..] {
                out.push(indent(&cont, l));
            }
            // An item holding more than one markdown block (a blank line
            // inside it) is closed off like a standalone, so the next item
            // is separated from it the same way on every run.
            out.last = Some(if lines.iter().any(|l| l.trim().is_empty()) {
                Kind::Standalone
            } else {
                Kind::ListItem
            });
        }
    }
}

/// A Logseq task marker as an Obsidian task.
fn task_marker(content: &str) -> std::borrow::Cow<'_, str> {
    for open in ["TODO ", "DOING ", "NOW ", "LATER "] {
        if let Some(rest) = content.strip_prefix(open) {
            return format!("[ ] {rest}").into();
        }
    }
    match content.strip_prefix("DONE ") {
        Some(rest) => format!("[x] {rest}").into(),
        None => content.into(),
    }
}

fn indent(pad: &str, line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!("{pad}{line}")
    }
}

fn fix_inline_outside_code(lines: &[String]) -> Vec<String> {
    let mut fence: Option<(char, usize)> = None;
    lines
        .iter()
        .map(|l| {
            if let Some(f) = fence {
                if fence_closes(l, f) {
                    fence = None;
                }
                return l.clone();
            }
            fence = fence_open(l);
            if fence.is_some() {
                l.clone()
            } else {
                fix_inline(l)
            }
        })
        .collect()
}

fn image_attr_regex() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        // The alt text may hold one level of `[link](…)`, as figure captions do.
        regex::Regex::new(r"!\[((?:[^\[\]]|\[[^\]]*\])*)\]\(([^)\s]*)\)\{(:[^}]*)\}")
            .expect("static image regex")
    })
}

fn width_regex() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r":width\s+(\d+)").expect("static width regex"))
}

/// Replace Logseq image sizing with Obsidian's `![alt|width](src)`.
///
/// ```
/// use vault::bodies::fix_images;
///
/// assert_eq!(
///     fix_images("![a.png](assets/a.png){:height 841, :width 800}"),
///     "![a.png|800](assets/a.png)"
/// );
/// // Height alone carries no Obsidian equivalent and is dropped.
/// assert_eq!(fix_images("![a](assets/a.png){:height 40}"), "![a](assets/a.png)");
/// // A caption with a link in it.
/// assert_eq!(
///     fix_images("![Fig: [src](https://x.org) ok](assets/f.png){:width 500}"),
///     "![Fig: [src](https://x.org) ok|500](assets/f.png)"
/// );
/// ```
#[must_use]
pub fn fix_images(line: &str) -> String {
    if !line.contains("){:") {
        return line.to_owned();
    }
    image_attr_regex()
        .replace_all(line, |c: &regex::Captures<'_>| {
            let alt = &c[1];
            let src = &c[2];
            match width_regex().captures(&c[3]) {
                Some(w) => format!("![{alt}|{}]({src})", &w[1]),
                None => format!("![{alt}]({src})"),
            }
        })
        .into_owned()
}

/// Image sizing and Logseq macros, on a line outside code.
fn fix_inline(line: &str) -> String {
    fix_macros(&fix_images(line))
}

fn macro_regexes() -> &'static [(regex::Regex, &'static str)] {
    static R: std::sync::OnceLock<Vec<(regex::Regex, &'static str)>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let re = |p: &str| regex::Regex::new(p).expect("static macro regex");
        vec![
            // A video: Obsidian and Quartz both embed a YouTube or media URL
            // written as an image.
            (re(r"\{\{video\s+(https?://[^\s{}]+)\s*\}\}"), "![]($1)"),
            // A post on X: nothing downstream embeds one, so a link.
            (
                re(r"\{\{(?:twitter|tweet)\s+(?:tweet\s+)?[\[(]?(https?://[^\s{}\]]+)\]?\s*\}\}"),
                "<$1>",
            ),
            (
                re(r"\{\{renderer\s+:linkpreview,\s*(https?://[^\s{}]+)\s*\}\}"),
                "<$1>",
            ),
            // Logseq plugin controls with no meaning outside Logseq: the
            // diagram renderer (the fence below it renders natively) and the
            // code-evaluation button.
            (re(r"\{\{renderer\s+code_diagram,[A-Za-z-]+\s*\}\}"), ""),
            (re(r"\{\{evalparent\}\}"), ""),
        ]
    })
}

/// Rewrite the Logseq macros that have an Obsidian equivalent.
///
/// `{{query …}}` and the other Logseq-only macros have none and are left as
/// written.
///
/// ```
/// use vault::bodies::fix_macros;
///
/// assert_eq!(
///     fix_macros("{{video https://www.youtube.com/watch?v=AJWTUvXA0Wc}}"),
///     "![](https://www.youtube.com/watch?v=AJWTUvXA0Wc)"
/// );
/// assert_eq!(
///     fix_macros("See {{twitter https://twitter.com/a/status/1}}"),
///     "See <https://twitter.com/a/status/1>"
/// );
/// assert_eq!(fix_macros("{{renderer code_diagram,mermaid}}"), "");
/// assert_eq!(fix_macros("{{query (and [[X]])}}"), "{{query (and [[X]])}}");
/// ```
#[must_use]
pub fn fix_macros(line: &str) -> String {
    if !line.contains("{{") {
        return line.to_owned();
    }
    let mut out = line.to_owned();
    for (re, with) in macro_regexes() {
        out = re.replace_all(&out, *with).into_owned();
    }
    if out.trim().is_empty() {
        String::new()
    } else if out == line {
        out
    } else {
        out.trim_end().to_owned()
    }
}

/// What to convert and how.
#[derive(Debug, Clone)]
pub struct Options {
    /// Every tree to walk.
    pub scopes: Vec<Scope>,
    /// Compute everything, write nothing.
    pub dry_run: bool,
}

/// One converted page.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// The page id.
    pub id: String,
    /// The vault the page belongs to.
    pub vault: String,
    /// Absolute path.
    #[serde(skip)]
    pub path: PathBuf,
    /// The converted file.
    #[serde(skip)]
    pub after: String,
}

/// Aggregate counts and the page list.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Markdown files examined.
    pub pages_examined: usize,
    /// Pages whose body changed.
    pub pages_converted: usize,
    /// Every converted page.
    pub changes: Vec<Change>,
}

/// Convert every page under `options.scopes`.
///
/// # Errors
/// Any read or write failure.
pub fn run(options: &Options) -> anyhow::Result<Report> {
    let mut report = Report::default();
    for scope in &options.scopes {
        if !scope.dir.is_dir() {
            continue;
        }
        for path in vault_core::page::walk_pages(&scope.dir)?.files {
            report.pages_examined += 1;
            let before = std::fs::read_to_string(&path)?;
            let after = convert(&before);
            if after == before {
                continue;
            }
            let rel = path.strip_prefix(&scope.dir).unwrap_or(&path);
            let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
            report.pages_converted += 1;
            report.changes.push(Change {
                id,
                vault: scope.vault.clone(),
                path: path.clone(),
                after,
            });
        }
    }
    if !options.dry_run {
        for change in &report.changes {
            std::fs::write(&change.path, &change.after)?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idempotent(body: &str) -> String {
        let once = convert_body(body);
        assert_eq!(convert_body(&once), once, "not idempotent:\n{once}");
        once
    }

    #[test]
    fn a_leading_definition_paragraph_is_untouched() {
        let body = "A graph of entities.\n\n- ### Overview\n  - Point.\n";
        assert_eq!(
            idempotent(body),
            "A graph of entities.\n\n### Overview\n\n- Point.\n"
        );
    }

    #[test]
    fn tab_indented_headings_flatten_and_lists_restart_under_them() {
        let body = "- # Stack\n\t- Intro.\n\t\t- ## Engine\n\t\t\t- Docker.\n\t\t\t\t- Detail.\n";
        assert_eq!(
            idempotent(body),
            "# Stack\n\n- Intro.\n\n## Engine\n\n- Docker.\n  - Detail.\n"
        );
    }

    #[test]
    fn a_fence_in_a_bullet_keeps_its_content_and_sits_under_its_item() {
        let body = "- Diagram:\n\t- ```mermaid\n\t  graph TD\n\t    A --> B\n\t  ```\n";
        assert_eq!(
            idempotent(body),
            "- Diagram:\n\n  ```mermaid\n  graph TD\n    A --> B\n  ```\n"
        );
    }

    #[test]
    fn a_top_level_fence_is_flush() {
        let body = "- ```\n  x = 1\n\n  y = 2\n  ```\n- After.\n";
        assert_eq!(idempotent(body), "```\nx = 1\n\ny = 2\n```\n\n- After.\n");
    }

    #[test]
    fn continuation_lines_follow_their_item() {
        let body = "- **Time:** 3 minutes\n  **Goal:** explain.\n";
        assert_eq!(
            idempotent(body),
            "- **Time:** 3 minutes\n  **Goal:** explain.\n"
        );
    }

    #[test]
    fn empty_bullets_vanish_and_their_children_move_up() {
        let body = "-\n- Point.\n\t-\n\t\t- Child.\n";
        assert_eq!(idempotent(body), "- Point.\n  - Child.\n");
    }

    #[test]
    fn a_dash_rule_bullet_is_a_thematic_break() {
        let body = "- One.\n- ---\n- # Two\n";
        assert_eq!(idempotent(body), "- One.\n\n---\n\n# Two\n");
    }

    #[test]
    fn a_heading_bullet_with_text_below_becomes_heading_and_paragraph() {
        let body = "- ## Slide 1\n  **Time:** 3 minutes\n  - Point.\n";
        assert_eq!(
            idempotent(body),
            "## Slide 1\n\n**Time:** 3 minutes\n\n- Point.\n"
        );
    }

    #[test]
    fn inlined_blockquotes_lose_their_repeated_line() {
        let body = "- <!-- vault-migrate: inlined -->\n> # Terms\n> ## Definition\n> ## Definition\n> Text.\n- Next.\n";
        assert_eq!(
            idempotent(body),
            "<!-- vault-migrate: inlined -->\n\n> # Terms\n> ## Definition\n> Text.\n\n- Next.\n"
        );
    }

    #[test]
    fn image_only_bullets_become_paragraphs_with_obsidian_sizing() {
        let body = "- # Title\n\t- ![p.jpg](assets/p.jpg){:height 378, :width 872}\n\t- Caption.\n";
        assert_eq!(
            idempotent(body),
            "# Title\n\n![p.jpg|872](assets/p.jpg)\n\n- Caption.\n"
        );
    }

    #[test]
    fn a_table_under_a_list_item_is_indented_under_it() {
        let body = "- Users:\n\t- | a | b |\n\t  | - | - |\n\t  | 1 | 2 |\n\t- After.\n";
        assert_eq!(
            idempotent(body),
            "- Users:\n\n  | a | b |\n  | - | - |\n  | 1 | 2 |\n\n  - After.\n"
        );
    }

    #[test]
    fn a_heading_bullet_whose_text_starts_after_a_blank_line_gets_one_blank() {
        let body = "- ### Content\n\n  ## Definition\n\n  Text.\n";
        assert_eq!(idempotent(body), "### Content\n\n## Definition\n\nText.\n");
    }

    #[test]
    fn blank_runs_inside_code_survive() {
        let body = "- ```\n  a\n\n\n  b\n  ```\n";
        assert_eq!(idempotent(body), "```\na\n\n\nb\n```\n");
    }

    #[test]
    fn a_quote_under_a_parent_item_stays_with_the_parent() {
        let body = "- Parent:\n  - One.\n  - >14 months of data\n  - Two.\n";
        assert_eq!(
            idempotent(body),
            "- Parent:\n  - One.\n\n  >14 months of data\n\n  - Two.\n"
        );
    }

    #[test]
    fn items_after_a_heading_never_nest_under_items_before_it() {
        let body = "- Intro.\n\t- Point.\n\t\t- ### Heading\n\t - Under.\n";
        assert_eq!(
            idempotent(body),
            "- Intro.\n  - Point.\n\n### Heading\n\n- Under.\n"
        );
    }

    #[test]
    fn a_repeated_quote_line_inside_a_list_item_is_dropped() {
        let body = "- Item\n\t- > ## Gap\n\t  > Text.\n\t  > Text.\n";
        assert_eq!(idempotent(body), "- Item\n\n  > ## Gap\n  > Text.\n");
    }

    #[test]
    fn an_unclosed_fence_in_a_bullet_ends_with_its_block() {
        let body =
            "- Diagram\n\t- ```mermaid\n\t  graph LR\n\t  A --> B\n  Prose after.\n- Next.\n";
        assert_eq!(
            idempotent(body),
            "- Diagram\n\n  ```mermaid\n  graph LR\n  A --> B\n  ```\n  Prose after.\n\n- Next.\n"
        );
    }

    #[test]
    fn a_closed_fence_keeps_code_that_is_less_indented_than_its_bullet() {
        let body = "- ```\ncode at column zero\n```\n- Next.\n";
        assert_eq!(
            idempotent(body),
            "```\ncode at column zero\n```\n\n- Next.\n"
        );
    }

    #[test]
    fn logseq_task_markers_become_tasks() {
        let body = "- TODO Write it\n- DONE Ship it\n- NOW is the time\n";
        assert_eq!(
            idempotent(body),
            "- [ ] Write it\n- [x] Ship it\n- [ ] is the time\n"
        );
    }

    #[test]
    fn a_video_bullet_becomes_an_embed_and_a_renderer_line_vanishes() {
        let body = "- Watch:\n\t- {{video https://youtu.be/abc}}\n- {{renderer code_diagram,mermaid}}\n\t- ```mermaid\n\t  graph TD\n\t  ```\n";
        assert_eq!(
            idempotent(body),
            "- Watch:\n\n  ![](https://youtu.be/abc)\n\n```mermaid\ngraph TD\n```\n"
        );
    }

    #[test]
    fn a_renderer_as_the_first_line_of_a_fenced_block_leaves_the_fence() {
        let body = "- {{renderer code_diagram,mermaid}}\n  ```mermaid\n  graph TD\n  ```\n";
        assert_eq!(idempotent(body), "```mermaid\ngraph TD\n```\n");
    }

    #[test]
    fn macros_inside_code_are_left_alone() {
        let body = "```\n{{video https://youtu.be/abc}}\n```\n";
        assert_eq!(idempotent(body), body);
    }

    #[test]
    fn frontmatter_is_carried_verbatim() {
        let page = "---\ntitle: x\nlinks:\n- '[[A]]'\n---\n- ### H\n  - p\n";
        assert_eq!(
            convert(page),
            "---\ntitle: x\nlinks:\n- '[[A]]'\n---\n### H\n\n- p\n"
        );
    }

    #[test]
    fn a_plain_markdown_page_is_left_alone() {
        let body = "Intro paragraph\nover two lines.\n\n## Heading\n\n- a\n  - b\n\n```\ncode\n\n\ncode\n```\n";
        assert_eq!(convert_body(body), body);
    }
}
