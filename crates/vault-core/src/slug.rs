//! Slug and IRI arithmetic, ported 1:1 from the Python pipeline so that a
//! Rust-built `scaffold-index.json` is byte-identical to the Python one.
//!
//! Three rules, and they are not interchangeable:
//!
//! * [`slugify`] is `re.sub(r'[^a-z0-9]+', '-', s.lower()).strip('-')`;
//! * [`ref_slug`] is `iri.split(":")[-1]` — the *last colon segment*, with no
//!   re-slugification, matching `pipeline/reason.py::ref_slug`;
//! * [`iri_for`] builds `urn:ngm:class:<slug>` from a title.

use std::sync::OnceLock;

use regex::Regex;

fn non_slug_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^a-z0-9]+").expect("static slug regex"))
}

/// Kebab-case a title: lowercase, collapse every non-`[a-z0-9]` run to `-`,
/// trim leading and trailing `-`.
///
/// ```
/// # use vault_core::slug::slugify;
/// assert_eq!(slugify("Knowledge Graph"), "knowledge-graph");
/// assert_eq!(slugify("ISO/IEC 9075"), "iso-iec-9075");
/// ```
#[must_use]
pub fn slugify(s: &str) -> String {
    let lower = s.to_lowercase();
    let replaced = non_slug_re().replace_all(&lower, "-");
    replaced.trim_matches('-').to_owned()
}

/// The slug of an IRI: everything after the last `:`.
///
/// This deliberately does **not** re-slugify. `urn:ngm:class:iso-iec-9075`
/// yields `iso-iec-9075`; a malformed IRI yields whatever followed its last
/// colon, exactly as `pipeline/reason.py` does.
///
/// ```
/// # use vault_core::slug::ref_slug;
/// assert_eq!(ref_slug("urn:ngm:class:knowledge-graph"), "knowledge-graph");
/// assert_eq!(ref_slug("bare-slug"), "bare-slug");
/// ```
#[must_use]
pub fn ref_slug(iri: &str) -> String {
    iri.rsplit(':').next().unwrap_or(iri).to_owned()
}

/// Map a wikilink target onto a slug: an already-kebab target is taken
/// verbatim, anything else is slugified.
///
/// This is the rule Loom's `loom-scaffold::ref_to_slug` uses, and it is what
/// keeps the *curated* `links` list round-tripping: the migration writes a
/// long-tail link as its IRI tail (`[[3-d-animation|3D Animation]]`), and this
/// function reads that tail back unchanged rather than re-slugifying it into
/// something else.
///
/// ```
/// # use vault_core::slug::target_slug;
/// assert_eq!(target_slug("3-d-animation"), "3-d-animation");
/// assert_eq!(target_slug("3D Animation"), "3d-animation");
/// ```
#[must_use]
pub fn target_slug(target: &str) -> String {
    if !target.is_empty()
        && target
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        target.to_owned()
    } else {
        slugify(target)
    }
}

/// Build the canonical class IRI for a page title under `namespace`.
///
/// ```
/// # use vault_core::slug::iri_for;
/// assert_eq!(
///     iri_for("urn:ngm:class:", "Knowledge Graph"),
///     "urn:ngm:class:knowledge-graph"
/// );
/// ```
#[must_use]
pub fn iri_for(namespace: &str, title: &str) -> String {
    format!("{namespace}{}", slugify(title))
}

/// `owl:Thing`, in any of the spellings the corpus has used, is never a useful
/// ancestor (`pipeline/reason.py::_is_thing`).
#[must_use]
pub fn is_thing(slug: &str) -> bool {
    matches!(
        slug.to_lowercase().as_str(),
        "thing" | "owl-thing" | "owl:thing"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_matches_python_regex() {
        assert_eq!(slugify("  Hello,  World!  "), "hello-world");
        assert_eq!(slugify("A/B Testing"), "a-b-testing");
        assert_eq!(slugify("---"), "");
        assert_eq!(slugify("TCP/IP"), "tcp-ip");
    }

    #[test]
    fn ref_slug_takes_the_last_colon_segment_verbatim() {
        assert_eq!(
            ref_slug("urn:visionflow:linked:ai-reasoning"),
            "ai-reasoning"
        );
        assert_eq!(ref_slug("owl:Thing"), "Thing");
        assert_eq!(ref_slug(""), "");
    }

    #[test]
    fn target_slug_takes_a_kebab_target_verbatim() {
        assert_eq!(target_slug("ai-reasoning"), "ai-reasoning");
        assert_eq!(target_slug("3-d-li-dar"), "3-d-li-dar");
        assert_eq!(target_slug("AI Reasoning"), "ai-reasoning");
        assert_eq!(target_slug("TCP/IP"), "tcp-ip");
        assert_eq!(target_slug(""), "");
    }

    #[test]
    fn thing_detection_is_case_insensitive() {
        assert!(is_thing("Thing"));
        assert!(is_thing("owl-thing"));
        assert!(!is_thing("something"));
    }
}
