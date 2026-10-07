//! Flattening a `memory_entries.value` (`jsonb`) to a short plain-text snippet.
//!
//! Values are strings, or objects such as `{"title": …, "content": …}` or
//! `{"date": …, "finding": …}`. Postgres returns `jsonb` object keys sorted
//! by length, so a naive walk would lead with dates and ids; text-bearing
//! keys ([`PRIORITY_KEYS`]) are therefore visited first. Only values are
//! emitted, never keys. Control characters count as whitespace, every
//! whitespace run collapses to one space, and the walk stops as soon as the
//! budget is spent so a multi-megabyte value costs no more than a short one.

use serde_json::Value;

/// Default snippet length in characters (Unicode scalar values).
pub const SNIPPET_MAX_CHARS: usize = 160;

/// Object keys whose values are emitted before all others, in this order.
pub const PRIORITY_KEYS: &[&str] = &[
    "title",
    "content",
    "text",
    "summary",
    "finding",
    "description",
    "message",
    "body",
];

const ELLIPSIS: char = '…';

/// Flatten `value` to at most `max_chars` characters. When the text is
/// truncated, the last character is `…` and the cut lands on a character
/// boundary.
///
/// ```
/// use serde_json::json;
/// use visionclaw_memory_cloud::snippet::snippet;
///
/// let v = json!({"date": "2026-08-19", "finding": "Workflows\n\trepinned.", "n": 3});
/// assert_eq!(snippet(&v, 160), "Workflows repinned. 2026-08-19 3");
/// assert_eq!(snippet(&json!("ab cd ef"), 5), "ab c…");
/// ```
pub fn snippet(value: &Value, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut c = Collector::new(max_chars + 1);
    c.walk(value);
    if c.chars <= max_chars {
        return c.out;
    }
    let mut truncated: String = c.out.chars().take(max_chars - 1).collect();
    while truncated.ends_with(' ') {
        truncated.pop();
    }
    truncated.push(ELLIPSIS);
    truncated
}

struct Collector {
    out: String,
    chars: usize,
    limit: usize,
    pending_space: bool,
}

impl Collector {
    fn new(limit: usize) -> Self {
        Self {
            out: String::new(),
            chars: 0,
            limit,
            pending_space: false,
        }
    }

    fn full(&self) -> bool {
        self.chars >= self.limit
    }

    fn separator(&mut self) {
        if self.chars > 0 {
            self.pending_space = true;
        }
    }

    fn push_text(&mut self, text: &str) {
        for ch in text.chars() {
            if self.full() {
                return;
            }
            if ch.is_whitespace() || ch.is_control() {
                self.separator();
                continue;
            }
            if self.pending_space {
                self.out.push(' ');
                self.chars += 1;
                self.pending_space = false;
                if self.full() {
                    return;
                }
            }
            self.out.push(ch);
            self.chars += 1;
        }
    }

    fn walk(&mut self, value: &Value) {
        if self.full() {
            return;
        }
        match value {
            Value::Null => {}
            Value::Bool(b) => {
                self.separator();
                self.push_text(if *b { "true" } else { "false" });
            }
            Value::Number(n) => {
                self.separator();
                self.push_text(&n.to_string());
            }
            Value::String(s) => {
                self.separator();
                self.push_text(s);
            }
            Value::Array(items) => {
                for item in items {
                    if self.full() {
                        return;
                    }
                    self.walk(item);
                }
            }
            Value::Object(map) => {
                for key in PRIORITY_KEYS {
                    if let Some(v) = map.get(*key) {
                        self.walk(v);
                    }
                }
                for (k, v) in map {
                    if self.full() {
                        return;
                    }
                    if !PRIORITY_KEYS.contains(&k.as_str()) {
                        self.walk(v);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collapses_whitespace_and_strips_controls() {
        let v = json!("  a\u{0007}b \n\n c\u{0000}\td  ");
        assert_eq!(snippet(&v, 160), "a b c d");
    }

    #[test]
    fn priority_keys_lead_and_keys_are_not_emitted() {
        let v = json!({
            "path": "examples/x.rs",
            "repo": "ruvector",
            "text": "Module examples/x.rs does things"
        });
        assert_eq!(
            snippet(&v, 160),
            "Module examples/x.rs does things examples/x.rs ruvector"
        );
    }

    #[test]
    fn nested_arrays_and_scalars_flatten() {
        // One non-priority key per object: map ordering differs with
        // serde_json's `preserve_order` feature, which workspace feature
        // unification may switch on.
        assert_eq!(snippet(&json!({"tags": ["a", ["b", 2]]}), 160), "a b 2");
        assert_eq!(
            snippet(&json!([true, null, 1.5, false]), 160),
            "true 1.5 false"
        );
    }

    #[test]
    fn truncates_on_char_boundary_with_ellipsis() {
        let v = json!("ééééé ñññññ 日本語のテキスト");
        let s = snippet(&v, 8);
        assert_eq!(s.chars().count(), 8);
        assert_eq!(s, "ééééé ñ…");
        let s = snippet(&json!("abcdef ghi"), 7);
        assert_eq!(s, "abcdef…", "no dangling space before the ellipsis");
    }

    #[test]
    fn exact_fit_is_not_truncated() {
        let text = "x".repeat(160);
        assert_eq!(snippet(&json!(text.clone()), 160), text);
        assert!(snippet(&json!("x".repeat(161)), 160).ends_with('…'));
    }

    #[test]
    fn huge_values_are_bounded() {
        let big = "word ".repeat(1_000_000);
        let s = snippet(&json!({ "content": big }), 160);
        assert_eq!(s.chars().count(), 160);
        assert_eq!(snippet(&json!(null), 160), "");
        assert_eq!(snippet(&json!("abc"), 0), "");
    }
}
