//! Which byte ranges of a page are *code*, so a page that documents a
//! construct is not mistaken for one that uses it.
//!
//! `Dr O'Hare Writing for LogSeq` teaches Logseq syntax. It contains the text
//! `` `{{embed ((block-uuid))}}` `` inside an inline code span, where
//! `block-uuid` is a literal placeholder and not a uuid. Scanning for residue
//! without this module refuses the whole migration over a sentence explaining
//! the thing being migrated away from.
//!
//! # An unterminated fence is not a fence
//!
//! This is the load-bearing rule. 511 knowledge pages carry an **odd** number
//! of code-fence markers — a stray opener, usually inside an OWL
//! functional-syntax block — and 1,713 real `key:: value` metadata lines fall
//! after the last unmatched one. A scanner that treats "from the last opener to end of file"
//! as code silently swallows them, and the migration loses 1,713 relation and
//! `sources[]` entries without reporting anything.
//!
//! So only **paired** fences produce a code region, and the unmatched opener is
//! reported as [`CodeMap::unterminated_fence`] rather than being allowed to
//! consume the rest of the page.

use std::ops::Range;

/// The code regions of one page, plus the defect the scan found.
#[derive(Debug, Clone, Default)]
pub struct CodeMap {
    /// Byte ranges covered by a **paired** fenced block, ascending.
    fenced: Vec<Range<usize>>,
    /// Byte ranges covered by an inline `` ` `` span, ascending.
    inline: Vec<Range<usize>>,
    /// The 1-based line of an unmatched fence opener, when the page has one.
    ///
    /// A genuine corpus defect: the region is treated as prose (which is what
    /// it is) and the page is reported so the source can be cleaned up.
    pub unterminated_fence: Option<usize>,
}

impl CodeMap {
    /// Scan `text` for fenced blocks and inline code spans.
    #[must_use]
    pub fn scan(text: &str) -> Self {
        let (fenced, unterminated_fence) = fenced_regions(text);
        let inline = inline_regions(text, &fenced);
        Self {
            fenced,
            inline,
            unterminated_fence,
        }
    }

    /// `true` when `offset` falls inside any code region.
    #[must_use]
    pub fn contains(&self, offset: usize) -> bool {
        self.fenced
            .iter()
            .chain(&self.inline)
            .any(|r| r.contains(&offset))
    }

    /// `true` when any part of `range` overlaps any code region.
    #[must_use]
    pub fn overlaps(&self, range: &Range<usize>) -> bool {
        overlaps_any(self.fenced.iter().chain(&self.inline), range)
    }

    /// `true` when any part of `range` overlaps an **inline** span.
    ///
    /// The distinction matters for a residue check on fences themselves: a
    /// surviving ` ```json-ld ` block is residue *because* it is a fenced
    /// block, so excusing fenced regions would hide exactly what the check
    /// exists to find. Only a mention inside backticks is documentation.
    #[must_use]
    pub fn overlaps_inline(&self, range: &Range<usize>) -> bool {
        overlaps_any(self.inline.iter(), range)
    }

    /// The paired fenced regions, ascending.
    #[must_use]
    pub fn fenced(&self) -> &[Range<usize>] {
        &self.fenced
    }

    /// The inline spans, ascending.
    #[must_use]
    pub fn inline(&self) -> &[Range<usize>] {
        &self.inline
    }

    /// `true` when the page carries an unmatched fence opener.
    #[must_use]
    pub fn has_unterminated_fence(&self) -> bool {
        self.unterminated_fence.is_some()
    }
}

fn overlaps_any<'a>(
    mut regions: impl Iterator<Item = &'a Range<usize>>,
    range: &Range<usize>,
) -> bool {
    regions.any(|r| r.start < range.end && range.start < r.end)
}

/// Byte ranges of **paired** fenced blocks, plus the line of an unmatched
/// opener.
fn fenced_regions(text: &str) -> (Vec<Range<usize>>, Option<usize>) {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize)> = None; // (byte offset, line)
    let mut offset = 0usize;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if is_fence {
            match open.take() {
                // A closing fence: the whole block, terminator included.
                Some((start, _)) => out.push(start..offset + line.len()),
                None => open = Some((offset, index + 1)),
            }
        }
        offset += line.len() + 1; // +1 for the newline `lines()` stripped
    }
    (out, open.map(|(_, line)| line))
}

/// Byte ranges of inline `` ` `` spans, ignoring anything already inside a
/// fenced block.
fn inline_regions(text: &str, fenced: &[Range<usize>]) -> Vec<Range<usize>> {
    let in_fence = |o: usize| fenced.iter().any(|r| r.contains(&o));
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'`' || in_fence(i) {
            i += 1;
            continue;
        }
        // A run of n backticks opens a span closed by the next run of n.
        let open_start = i;
        let mut ticks = 0usize;
        while i < bytes.len() && bytes[i] == b'`' {
            ticks += 1;
            i += 1;
        }
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'\n' {
                break; // an inline span does not cross a line
            }
            if bytes[j] == b'`' {
                let run_start = j;
                let mut run = 0usize;
                while j < bytes.len() && bytes[j] == b'`' {
                    run += 1;
                    j += 1;
                }
                if run == ticks {
                    closed = Some(j);
                    let _ = run_start;
                    break;
                }
                continue;
            }
            j += 1;
        }
        if let Some(end) = closed {
            out.push(open_start..end);
            i = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset_of(text: &str, needle: &str) -> usize {
        text.find(needle).expect("needle present")
    }

    #[test]
    fn an_inline_span_is_code() {
        let text = "Use `{{embed ((block-uuid))}}` for embedding.";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "{{embed")));
        assert!(!map.contains(offset_of(text, "for embedding")));
    }

    #[test]
    fn a_paired_fence_is_code() {
        let text = "before\n```\nkey:: value\n```\nafter\n";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "key:: value")));
        assert!(!map.contains(offset_of(text, "before")));
        assert!(!map.contains(offset_of(text, "after")));
        assert!(!map.has_unterminated_fence());
    }

    #[test]
    fn an_unterminated_fence_is_not_a_fence() {
        // The hazard: 511 pages open a stray fence and 1,713 real `key::`
        // lines fall after it. They must stay visible.
        let text = "intro\n```\nstray opener never closed\nsources:: [[X]]\n";
        let map = CodeMap::scan(text);
        assert!(map.has_unterminated_fence());
        assert_eq!(map.unterminated_fence, Some(2));
        assert!(
            !map.contains(offset_of(text, "sources::")),
            "a line after an unmatched opener is prose, not code"
        );
    }

    #[test]
    fn three_markers_pair_the_first_two_and_report_the_third() {
        let text = "a\n```\ncode\n```\nb\n```\ntail:: value\n";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "code")));
        assert!(!map.contains(offset_of(text, "tail:: value")));
        assert_eq!(map.unterminated_fence, Some(6));
    }

    #[test]
    fn a_backtick_inside_a_fence_does_not_open_a_span() {
        let text = "```\nlet x = `y`;\n```\ntrailing `real span` here\n";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "let x")));
        assert!(map.contains(offset_of(text, "real span")));
        assert!(!map.contains(offset_of(text, "trailing")));
    }

    #[test]
    fn an_unclosed_inline_span_is_not_code() {
        let text = "a stray ` backtick and key:: value\n";
        let map = CodeMap::scan(text);
        assert!(!map.contains(offset_of(text, "key:: value")));
    }

    #[test]
    fn an_inline_span_does_not_cross_a_line() {
        let text = "open ` here\nkey:: value\nclose ` there\n";
        let map = CodeMap::scan(text);
        assert!(!map.contains(offset_of(text, "key:: value")));
    }

    #[test]
    fn double_backticks_pair_with_double_backticks() {
        let text = "``a ` b`` and key:: value\n";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "a ` b")));
        assert!(!map.contains(offset_of(text, "key:: value")));
    }

    #[test]
    fn tilde_fences_are_recognised() {
        let text = "a\n~~~\nkey:: value\n~~~\nb\n";
        let map = CodeMap::scan(text);
        assert!(map.contains(offset_of(text, "key:: value")));
    }

    #[test]
    fn overlaps_reports_partial_intersection() {
        let text = "x `code` y";
        let map = CodeMap::scan(text);
        let span = offset_of(text, "`code`");
        assert!(map.overlaps(&(span..span + 3)));
        assert!(!map.overlaps(&(0..1)));
    }

    #[test]
    fn a_page_with_no_code_scans_clean() {
        let map = CodeMap::scan("just prose\nand more prose\n");
        assert!(map.fenced().is_empty());
        assert!(map.inline().is_empty());
        assert!(!map.has_unterminated_fence());
    }

    #[test]
    fn inline_and_fenced_regions_are_distinguishable() {
        let text = "a `span` b\n```json-ld\n{}\n```\n";
        let map = CodeMap::scan(text);
        let fence = offset_of(text, "```json-ld");
        // The fence itself is a fenced region but not an inline span, so a
        // residue check on fences still sees it.
        assert!(map.overlaps(&(fence..fence + 3)));
        assert!(!map.overlaps_inline(&(fence..fence + 3)));
        let span = offset_of(text, "`span`");
        assert!(map.overlaps_inline(&(span..span + 6)));
    }
}
