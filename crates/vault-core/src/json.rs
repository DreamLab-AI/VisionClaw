//! CPython-compatible JSON writers.
//!
//! The build's `scaffold-index.json` must be **byte-identical** to the one the
//! retired Python pipeline emitted, because Loom loads it and the migration is
//! not allowed to change what Loom sees. Three `CPython` behaviours differ from
//! `serde_json`'s defaults and all three are reproduced here:
//!
//! 1. `json.dump(..., ensure_ascii=True)` (the default) escapes every
//!    non-ASCII scalar as lowercase `\uXXXX`, using UTF-16 surrogate pairs
//!    above the BMP. `serde_json` writes raw UTF-8.
//! 2. `separators=(",", ":")` is compact with no spaces — `serde_json`'s
//!    compact writer already matches.
//! 3. `indent=2` implies `separators=(",", ": ")` — `serde_json`'s pretty
//!    writer already matches.
//!
//! Key *order* is the other half of the contract: every map on the way in must
//! be insertion-ordered, which is why this crate enables `serde_json`'s
//! `preserve_order` feature.

use std::io::{self, Write};

use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter};

/// Wraps a [`Formatter`] and re-implements string emission with `CPython`'s
/// `ensure_ascii=True` escaping.
struct EnsureAscii<F>(F);

impl<F: Formatter> Formatter for EnsureAscii<F> {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let mut start = 0usize;
        for (idx, ch) in fragment.char_indices() {
            if ch.is_ascii() {
                continue;
            }
            if start < idx {
                writer.write_all(&fragment.as_bytes()[start..idx])?;
            }
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                write!(writer, "\\u{unit:04x}")?;
            }
            start = idx + ch.len_utf8();
        }
        if start < fragment.len() {
            writer.write_all(&fragment.as_bytes()[start..])?;
        }
        Ok(())
    }

    // Everything else delegates: serde_json's char-escape table (the short
    // escapes plus `\u00XX` for the remaining controls) is already CPython's.
    fn write_null<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.write_null(w)
    }
    fn write_bool<W: ?Sized + Write>(&mut self, w: &mut W, v: bool) -> io::Result<()> {
        self.0.write_bool(w, v)
    }
    fn write_i64<W: ?Sized + Write>(&mut self, w: &mut W, v: i64) -> io::Result<()> {
        self.0.write_i64(w, v)
    }
    fn write_u64<W: ?Sized + Write>(&mut self, w: &mut W, v: u64) -> io::Result<()> {
        self.0.write_u64(w, v)
    }
    fn write_f32<W: ?Sized + Write>(&mut self, w: &mut W, v: f32) -> io::Result<()> {
        self.0.write_f32(w, v)
    }
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, v: f64) -> io::Result<()> {
        self.0.write_f64(w, v)
    }
    fn write_number_str<W: ?Sized + Write>(&mut self, w: &mut W, v: &str) -> io::Result<()> {
        self.0.write_number_str(w, v)
    }
    fn begin_string<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_string(w)
    }
    fn end_string<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_string(w)
    }
    fn write_char_escape<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        e: serde_json::ser::CharEscape,
    ) -> io::Result<()> {
        self.0.write_char_escape(w, e)
    }
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn end_object_key<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_key(w)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_value(w)
    }
}

/// Serialise as `CPython`'s `json.dumps(value, separators=(",", ":"))`.
///
/// ```
/// # use vault_core::json::to_compact;
/// let v = serde_json::json!({ "t": "Caf\u{e9}", "n": [1, 2] });
/// let expected = concat!("{\"t\":\"Caf", "\\u00e9", "\",\"n\":[1,2]}");
/// assert_eq!(to_compact(&v).unwrap(), expected);
/// ```
///
/// # Errors
/// Propagates any [`serde_json::Error`] raised while serialising `value`.
///
/// # Panics
/// Never in practice: `serde_json` only ever emits UTF-8, and the UTF-8
/// validation of its own output is retained as a cheap assertion.
pub fn to_compact<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, EnsureAscii(CompactFormatter));
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(out).expect("serde_json emits utf-8"))
}

/// Serialise as `CPython`'s `json.dumps(value, indent=2)`.
///
/// ```
/// # use vault_core::json::to_indented;
/// let v = serde_json::json!({ "a": 1 });
/// assert_eq!(to_indented(&v).unwrap(), "{\n  \"a\": 1\n}");
/// ```
///
/// # Errors
/// Propagates any [`serde_json::Error`] raised while serialising `value`.
///
/// # Panics
/// Never in practice; see [`to_compact`].
pub fn to_indented<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut out,
        EnsureAscii(PrettyFormatter::with_indent(b"  ")),
    );
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(out).expect("serde_json emits utf-8"))
}

/// A formatter with `CPython`'s **default** separators, `(", ", ": ")`.
///
/// `json.dumps(value)` with neither `indent` nor `separators` is not compact:
/// it puts a space after every comma and colon. `prose_index.py` relies on
/// that default, so reproducing it is part of the artefact contract.
struct PythonDefault;

impl Formatter for PythonDefault {
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
}

/// Serialise as `CPython`'s bare `json.dumps(value)`: default separators
/// **and** `ensure_ascii=True`. This is what `build.py` writes for
/// `ontology.json` and `overview.json`.
///
/// ```
/// # use vault_core::json::to_default_ascii;
/// let v = serde_json::json!({ "a": 1, "b": "Caf\u{e9}" });
/// let expected = concat!("{\"a\": 1, \"b\": \"Caf", "\\u00e9", "\"}");
/// assert_eq!(to_default_ascii(&v).unwrap(), expected);
/// ```
///
/// # Errors
/// Propagates any [`serde_json::Error`] raised while serialising `value`.
///
/// # Panics
/// Never in practice; see [`to_compact`].
pub fn to_default_ascii<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, EnsureAscii(PythonDefault));
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(out).expect("serde_json emits utf-8"))
}

/// Serialise as `CPython`'s `json.dumps(value, ensure_ascii=False)` — default
/// separators, raw UTF-8. This is the setting `prose_index.py` uses.
///
/// ```
/// # use vault_core::json::to_default_utf8;
/// let v: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":[1,2]}"#).unwrap();
/// assert_eq!(to_default_utf8(&v).unwrap(), r#"{"a": 1, "b": [1, 2]}"#);
/// ```
///
/// # Errors
/// Propagates any [`serde_json::Error`] raised while serialising `value`.
///
/// # Panics
/// Never in practice; see [`to_compact`].
pub fn to_default_utf8<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PythonDefault);
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(out).expect("serde_json emits utf-8"))
}

/// Serialise compactly and **without** `ensure_ascii`.
///
/// # Errors
/// Propagates any [`serde_json::Error`] raised while serialising `value`.
pub fn to_compact_utf8<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    serde_json::to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_non_ascii_as_lowercase_utf16() {
        let v = serde_json::json!("na\u{ef}ve \u{2014} \u{1f702}");
        let expected =
            concat!("\"na", "\\u00ef", "ve ", "\\u2014", " ", "\\ud83d", "\\udf02", "\"");
        assert_eq!(to_compact(&v).unwrap(), expected);
    }

    #[test]
    fn control_characters_use_the_python_table() {
        let v = serde_json::json!("a\tb\nc\u{1}");
        let expected = concat!("\"a", "\\t", "b", "\\n", "c", "\\u0001", "\"");
        assert_eq!(to_compact(&v).unwrap(), expected);
    }

    #[test]
    fn compact_has_no_spaces_and_preserves_insertion_order() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"z":1,"a":{"b":[true,null]}}"#).unwrap();
        assert_eq!(to_compact(&v).unwrap(), r#"{"z":1,"a":{"b":[true,null]}}"#);
    }

    #[test]
    fn default_separators_put_a_space_after_comma_and_colon() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"z":1,"a":{"b":[true,null]},"c":[]}"#).unwrap();
        assert_eq!(
            to_default_utf8(&v).unwrap(),
            r#"{"z": 1, "a": {"b": [true, null]}, "c": []}"#
        );
    }

    #[test]
    fn default_separators_keep_non_ascii_raw() {
        let v = serde_json::json!("na\u{ef}ve");
        assert_eq!(to_default_utf8(&v).unwrap(), "\"na\u{ef}ve\"");
    }

    #[test]
    fn indented_matches_python_indent_two() {
        let v: serde_json::Value = serde_json::from_str(r#"{"a":[1,2],"b":{}}"#).unwrap();
        assert_eq!(
            to_indented(&v).unwrap(),
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {}\n}"
        );
    }
}
