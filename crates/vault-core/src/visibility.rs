//! Page visibility: which key decides, and when (ADR-VG-012 §5, ruling B9).
//!
//! Two spellings state the same fact. Vocabulary 2 publishes a page with
//! `public: true`. Vocabulary 3 states it as `visibility: public` (`private`,
//! `shared` or `public`, default `shared`), **on working pages only**. The
//! publisher and the gate (ontomark `is_public_under`) decide by the vault and
//! the vocabulary version, so this reader must too, or `VisionClaw` would serve
//! on the anonymous wire a page ontomark keeps private. [`Rules`] carries that
//! choice and every caller passes one; there is no context-free reading.
//!
//! | rules | the page is public when |
//! |---|---|
//! | [`Rules::PublicFlag`] (every knowledge page, any vocabulary; every page under vocabulary 2; a source with no vocabulary) | `public` is the YAML boolean `true`. `visibility` is **ignored**, whatever it says. |
//! | [`Rules::Visibility`] (a working page under vocabulary 3 or later) | `visibility: public`, and `public`, if also present, does not say `false` |
//!
//! Under [`Rules::Visibility`] the answer fails closed: `public: false` with
//! `visibility: public` is **not public** and is reported as
//! [`Problem::Conflict`] (ontomark reads such a page as public; `VisionClaw`
//! deliberately does not), `public: true` with `visibility: shared` or
//! `private` is not public and is a conflict too, an unknown `visibility` is
//! not public, and `public: true` with no `visibility` is not public (the page
//! has not been migrated; ontomark flags the missing key). Under
//! [`Rules::PublicFlag`] a `public` that is not a YAML boolean is not public,
//! with a [`Problem`].
//!
//! ```
//! use vault_core::visibility::{is_public, Rules};
//!
//! let v2: serde_yaml::Mapping = serde_yaml::from_str("public: true").unwrap();
//! let v3: serde_yaml::Mapping = serde_yaml::from_str("visibility: public").unwrap();
//! assert!(is_public(&v2, Rules::PublicFlag));
//! assert!(is_public(&v3, Rules::Visibility));
//! // The same `visibility: public` is ignored under vocabulary 2.
//! assert!(!is_public(&v3, Rules::PublicFlag));
//! ```

use serde_yaml::{Mapping, Value};

use crate::page::VaultKind;

/// The first vocabulary version that reads `visibility` on working pages.
pub const VISIBILITY_FROM_VERSION: u32 = 3;

/// Which key decides a page's publication (see the module table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rules {
    /// `public: true` alone decides; `visibility` is ignored.
    PublicFlag,
    /// `visibility` decides; `public` can only veto (a conflict fails closed).
    Visibility,
}

impl Rules {
    /// The rules for a page of `kind` under a vocabulary of `version`, as
    /// ontomark's `is_public_in` decides them: `visibility` only for a working
    /// page under version 3 or later. `version` is `None` when the source
    /// supplies no vocabulary, which reads as version 2.
    #[must_use]
    pub fn for_page(kind: VaultKind, version: Option<u32>) -> Self {
        match (kind, version) {
            (VaultKind::Working, Some(v)) if v >= VISIBILITY_FROM_VERSION => Self::Visibility,
            _ => Self::PublicFlag,
        }
    }

    /// The rules for a page identified by its vault-relative or repo path:
    /// only a page under `working/` is a working page.
    #[must_use]
    pub fn for_path(path: &str, version: Option<u32>) -> Self {
        let path = path.trim_start_matches("./").trim_start_matches('/');
        let kind = if path.starts_with("working/") {
            VaultKind::Working
        } else {
            VaultKind::Knowledge
        };
        Self::for_page(kind, version)
    }
}

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

/// Read the visibility of a page from its frontmatter mapping under `rules`.
#[must_use]
pub fn read(map: &Mapping, rules: Rules) -> Reading {
    let get = |k: &str| map.get(Value::String(k.to_owned()));
    let (vis, legacy) = (get("visibility"), get("public"));

    // `public`: a real YAML boolean, never the string "true" (§V2).
    let (legacy_bool, legacy_problem) = match legacy {
        Some(Value::Bool(b)) => (Some(*b), None),
        Some(Value::Null) | None => (None, None),
        Some(other) => (None, Some(Problem::InvalidPublic(text_of(other)))),
    };

    if rules == Rules::PublicFlag {
        // `visibility` is not a key of this vault or vocabulary: it neither
        // publishes the page nor makes a problem.
        let public = legacy_bool == Some(true);
        return Reading {
            visibility: if public {
                Visibility::Public
            } else {
                Visibility::Shared
            },
            public,
            form: if legacy_bool.is_some() {
                Form::LegacyPublic
            } else {
                Form::Default
            },
            declared_not_public: legacy_bool == Some(false),
            problem: legacy_problem,
        };
    }

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

    // Fails closed: public only for `visibility: public` that nothing
    // contradicts. A conflict, an unreadable value and an absent key are not.
    let public = vis_parsed == Some(Visibility::Public) && !conflict;
    let declared_not_public = legacy_bool == Some(false)
        || matches!(vis_parsed, Some(Visibility::Shared | Visibility::Private));
    Reading {
        visibility: if public {
            Visibility::Public
        } else {
            vis_parsed
                .filter(|v| *v != Visibility::Public)
                .unwrap_or(Visibility::Shared)
        },
        public,
        form: if vis.is_some_and(|v| !v.is_null()) {
            Form::Visibility
        } else if legacy_bool.is_some() {
            Form::LegacyPublic
        } else {
            Form::Default
        },
        declared_not_public,
        problem,
    }
}

/// `true` when the page is public under `rules`. This is the one function the
/// knowledge-graph gate, the corpus history, the page API, the publisher-facing
/// predicates and the publication census all call.
#[must_use]
pub fn is_public(map: &Mapping, rules: Rules) -> bool {
    read(map, rules).public
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(yaml: &str, rules: Rules) -> Reading {
        read(&serde_yaml::from_str::<Mapping>(yaml).unwrap(), rules)
    }
    const V2: Rules = Rules::PublicFlag;
    const V3: Rules = Rules::Visibility;

    #[test]
    fn the_rules_follow_the_vault_and_the_vocabulary_version() {
        use VaultKind::{Knowledge, Working};
        assert_eq!(Rules::for_page(Working, Some(3)), V3);
        assert_eq!(Rules::for_page(Working, Some(4)), V3);
        assert_eq!(Rules::for_page(Working, Some(2)), V2);
        assert_eq!(Rules::for_page(Working, None), V2);
        assert_eq!(
            Rules::for_page(Knowledge, Some(3)),
            V2,
            "knowledge: public alone"
        );
        assert_eq!(Rules::for_path("working/A.md", Some(3)), V3);
        assert_eq!(Rules::for_path("./working/A.md", Some(3)), V3);
        assert_eq!(Rules::for_path("knowledge/A.md", Some(3)), V2);
        assert_eq!(Rules::for_path("A.md", Some(3)), V2);
        assert_eq!(Rules::for_path("working/A.md", Some(2)), V2);
    }

    #[test]
    fn under_vocabulary_2_public_alone_decides_and_visibility_is_ignored() {
        // The two probe pages of the independent review, and the rest of the
        // matrix: none of them may be public unless `public: true`.
        for (y, public) in [
            ("public: false\nvisibility: public", false),
            ("visibility: public", false),
            ("public: true\nvisibility: public", true),
            ("public: true\nvisibility: shared", true),
            ("public: true", true),
            ("public: false", false),
            ("type: Note", false),
            ("visibility: world\npublic: true", true),
        ] {
            let x = r(y, V2);
            assert_eq!(x.public, public, "{y}");
            assert_eq!(
                x.problem, None,
                "{y}: visibility is not a key under version 2"
            );
        }
        let x = r("public: 'true'", V2);
        assert!(!x.public);
        assert!(matches!(x.problem, Some(Problem::InvalidPublic(_))));
    }

    #[test]
    fn under_vocabulary_3_visibility_decides_and_a_conflict_fails_closed() {
        for (y, public, conflict) in [
            ("visibility: public", true, false),
            ("public: true\nvisibility: public", true, false),
            ("visibility: shared", false, false),
            ("visibility: private", false, false),
            ("public: false\nvisibility: shared", false, false),
            // Contradictions are not public, and say so.
            ("public: false\nvisibility: public", false, true),
            ("public: true\nvisibility: shared", false, true),
            ("public: true\nvisibility: private", false, true),
            // An unmigrated page is not public under 3.
            ("public: true", false, false),
            ("type: Note", false, false),
        ] {
            let x = r(y, V3);
            assert_eq!(x.public, public, "{y}");
            assert_eq!(x.problem == Some(Problem::Conflict), conflict, "{y}");
        }
    }

    #[test]
    fn anything_unreadable_is_not_public_and_is_reported() {
        for y in [
            "visibility: Public",
            "visibility: world",
            "visibility: true",
            "visibility: 1",
            "public: true\nvisibility: world",
        ] {
            let x = r(y, V3);
            assert!(!x.public, "{y}");
            assert!(
                matches!(x.problem, Some(Problem::InvalidVisibility(_))),
                "{y}"
            );
        }
        for y in ["public: 'true'", "public: yes please", "public: 1"] {
            let x = r(y, V2);
            assert!(!x.public, "{y}");
            assert!(matches!(x.problem, Some(Problem::InvalidPublic(_))), "{y}");
        }
    }

    #[test]
    fn private_and_shared_are_not_public() {
        assert_eq!(r("visibility: private", V3).visibility, Visibility::Private);
        assert_eq!(r("type: Note", V3).visibility, Visibility::Shared);
        assert_eq!(r("type: Note", V2).visibility, Visibility::Shared);
    }

    #[test]
    fn explicit_false_is_distinct_from_absent() {
        assert!(r("public: false", V2).declared_not_public);
        assert!(!r("type: Note", V2).declared_not_public);
        assert!(!r("public: true", V2).declared_not_public);
        // Under version 2 a `visibility: shared` is not a declaration of the
        // class-marker interaction of ADR-2014: only `public: false` is.
        assert!(!r("visibility: shared", V2).declared_not_public);
        assert!(r("visibility: shared", V3).declared_not_public);
    }

    #[test]
    fn the_form_names_the_key_that_answered() {
        assert_eq!(r("public: true", V2).form, Form::LegacyPublic);
        assert_eq!(r("visibility: public", V3).form, Form::Visibility);
        assert_eq!(r("type: Note", V3).form, Form::Default);
    }
}
