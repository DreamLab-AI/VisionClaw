//! Page visibility, read from either header form (ADR-VG-012 §5, ruling B9).
//!
//! Vocabulary 2 publishes a page with `public: true`. Vocabulary 3 states the
//! same fact as `visibility: public` (`private`, `shared` or `public`, default
//! `shared`). The corpus is migrated from the first to the second after this
//! reader ships, so there is **one** function every consumer calls, and it
//! gives the same answer for a page before and after the migration:
//!
//! | header | answer |
//! |---|---|
//! | `public: true` | public |
//! | `visibility: public` | public |
//! | `public: false`, `visibility: shared`, `visibility: private`, neither key | not public |
//! | an unknown `visibility`, or a `public` that is not a YAML boolean | not public, with a [`Problem`] |
//! | both keys, and they disagree | not public, with [`Problem::Conflict`] |
//!
//! The gate fails closed: anything unreadable is not public. Removing `public`
//! from a page that says `visibility: public` therefore does not unpublish it.
//!
//! ```
//! use vault_core::visibility::{is_public, read, Visibility};
//!
//! let v2: serde_yaml::Mapping = serde_yaml::from_str("public: true").unwrap();
//! let v3: serde_yaml::Mapping = serde_yaml::from_str("visibility: public").unwrap();
//! assert!(is_public(&v2) && is_public(&v3));
//! assert_eq!(read(&v3).visibility, Visibility::Public);
//! ```

use serde_yaml::{Mapping, Value};

/// A page's visibility (ADR-VG-012 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Never leaves the owner's store. Not supported by any store yet.
    Private,
    /// Readable inside the estate, never published. The default.
    Shared,
    /// Published, and part of the public half of the knowledge graph.
    Public,
}

impl Visibility {
    /// The frontmatter spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Public => "public",
        }
    }

    /// Parse the exact frontmatter spelling; `None` for anything else.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "private" => Some(Self::Private),
            "shared" => Some(Self::Shared),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// Which key carried the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `visibility:` (vocabulary 3).
    Visibility,
    /// `public:` (vocabulary 2).
    LegacyPublic,
    /// Neither key; the default applies.
    Default,
}

/// Why a header could not be read cleanly. The page is not public either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// `visibility` is not `private`, `shared` or `public` (the text as written).
    InvalidVisibility(String),
    /// `public` is present and is not a YAML boolean (the text as written).
    InvalidPublic(String),
    /// Both keys are present and disagree.
    Conflict,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidVisibility(v) => {
                write!(f, "`visibility: {v}` is not private, shared or public")
            }
            Self::InvalidPublic(v) => write!(f, "`public: {v}` is not a YAML boolean"),
            Self::Conflict => f.write_str("`public` and `visibility` disagree"),
        }
    }
}

/// The reading of one header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The effective visibility; [`Visibility::Shared`] when nothing says more.
    pub visibility: Visibility,
    /// The one publication answer: `true` only for an affirmative, consistent
    /// `public: true` or `visibility: public`.
    pub public: bool,
    /// The key the answer came from.
    pub form: Form,
    /// `public: false` was written explicitly (as opposed to absent). The
    /// class-marker interaction of ADR-2014 turns on the difference; the
    /// `visibility: shared|private` spelling counts the same way.
    pub declared_not_public: bool,
    /// Why the header is not clean, if it is not.
    pub problem: Option<Problem>,
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => format!("{other:?}"),
    }
}

/// Read the visibility of a page from its frontmatter mapping.
#[must_use]
pub fn read(map: &Mapping) -> Reading {
    let get = |k: &str| map.get(Value::String(k.to_owned()));
    let (vis, legacy) = (get("visibility"), get("public"));

    // `public`: a real YAML boolean, never the string "true" (§V2).
    let (legacy_bool, legacy_problem) = match legacy {
        Some(Value::Bool(b)) => (Some(*b), None),
        Some(Value::Null) | None => (None, None),
        Some(other) => (None, Some(Problem::InvalidPublic(text_of(other)))),
    };
    // `visibility`: the exact spelling, or a problem.
    let (vis_parsed, vis_problem) = match vis {
        Some(Value::String(s)) => match Visibility::parse(s) {
            Some(v) => (Some(v), None),
            None => (None, Some(Problem::InvalidVisibility(s.clone()))),
        },
        Some(Value::Null) | None => (None, None),
        Some(other) => (None, Some(Problem::InvalidVisibility(text_of(other)))),
    };

    let conflict = matches!((vis_parsed, legacy_bool),
        (Some(v), Some(b)) if (v == Visibility::Public) != b);
    let problem = vis_problem
        .or(legacy_problem)
        .or(conflict.then_some(Problem::Conflict));

    let (visibility, form, public) = match (vis_parsed, legacy_bool) {
        (Some(v), _) => (v, Form::Visibility, v == Visibility::Public),
        (None, Some(b)) => (
            if b {
                Visibility::Public
            } else {
                Visibility::Shared
            },
            Form::LegacyPublic,
            b,
        ),
        (None, None) => (Visibility::Shared, Form::Default, false),
    };
    // Fail closed: a problem anywhere means not public.
    let public = public && problem.is_none();
    let declared_not_public = legacy_bool == Some(false)
        || matches!(vis_parsed, Some(Visibility::Shared | Visibility::Private));
    Reading {
        visibility: if public {
            Visibility::Public
        } else {
            visibility
        },
        public,
        form,
        declared_not_public,
        problem,
    }
}

/// `true` when the page is public under either header form. This is the
/// function the knowledge-graph gate, the publisher-facing predicates and the
/// publication census all call.
#[must_use]
pub fn is_public(map: &Mapping) -> bool {
    read(map).public
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(yaml: &str) -> Reading {
        read(&serde_yaml::from_str::<Mapping>(yaml).unwrap())
    }

    #[test]
    fn both_forms_give_the_same_answer() {
        for (v2, v3, public) in [
            ("public: true", "visibility: public", true),
            ("public: false", "visibility: shared", false),
            ("type: Note", "type: Note", false),
        ] {
            assert_eq!(r(v2).public, public, "{v2}");
            assert_eq!(r(v3).public, public, "{v3}");
        }
    }

    #[test]
    fn removing_public_from_a_migrated_page_does_not_unpublish_it() {
        assert!(r("type: Note\nvisibility: public").public);
        assert!(r("type: Note\npublic: true\nvisibility: public").public);
    }

    #[test]
    fn private_and_shared_are_not_public() {
        assert!(!r("visibility: private").public);
        assert_eq!(r("visibility: private").visibility, Visibility::Private);
        assert_eq!(r("type: Note").visibility, Visibility::Shared);
    }

    #[test]
    fn anything_unreadable_is_not_public_and_is_reported() {
        for y in [
            "visibility: Public",
            "visibility: world",
            "visibility: true",
            "visibility: 1",
        ] {
            let x = r(y);
            assert!(!x.public, "{y}");
            assert!(
                matches!(x.problem, Some(Problem::InvalidVisibility(_))),
                "{y}"
            );
        }
        for y in ["public: 'true'", "public: yes please", "public: 1"] {
            let x = r(y);
            assert!(!x.public, "{y}");
            assert!(matches!(x.problem, Some(Problem::InvalidPublic(_))), "{y}");
        }
    }

    #[test]
    fn disagreeing_keys_fail_closed() {
        for y in [
            "public: true\nvisibility: shared",
            "public: false\nvisibility: public",
            "public: true\nvisibility: private",
        ] {
            let x = r(y);
            assert!(!x.public, "{y}");
            assert_eq!(x.problem, Some(Problem::Conflict), "{y}");
        }
        assert_eq!(r("public: false\nvisibility: shared").problem, None);
    }

    #[test]
    fn explicit_false_is_distinct_from_absent() {
        assert!(r("public: false").declared_not_public);
        assert!(r("visibility: shared").declared_not_public);
        assert!(!r("type: Note").declared_not_public);
        assert!(!r("public: true").declared_not_public);
    }

    #[test]
    fn the_form_names_the_key_that_answered() {
        assert_eq!(r("public: true").form, Form::LegacyPublic);
        assert_eq!(r("visibility: public").form, Form::Visibility);
        assert_eq!(r("type: Note").form, Form::Default);
    }
}
