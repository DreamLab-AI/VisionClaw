//! `defines-as` — a curated EL **defined class** (ADR-2124).
//!
//! Every other class in the corpus is *primitive*: its page says what it is
//! **necessarily** (`is-a`, and the restrictions the vocabulary flags), never
//! what is **sufficient** to be one. A `defines-as` page states a sufficient
//! condition too, so the reasoner can classify classes *into* it that nobody
//! filed there by hand.
//!
//! # The frontmatter shape
//!
//! A YAML list read as a **conjunction**. Each item is one of:
//!
//! * a named class — `"[[Robot]]"`;
//! * an existential — a one-entry map `relation-key: "[[Gripper]]"`, read as
//!   `∃relation.Gripper`, where `relation-key` is a relation the vocabulary
//!   emits as an existential restriction (`restriction: true`, else
//!   `DEFINITION_NOT_EXISTENTIAL`) and whose sub/super-property closure holds
//!   no transitive property (else `DEFINITION_OVER_TRANSITIVE`).
//!
//! ```yaml
//! defines-as:
//!   - "[[Robot]]"
//!   - has-part: "[[Gripper]]"
//! ```
//!
//! is `GrippingRobot ≡ Robot ⊓ ∃hasPart.Gripper`, emitted as
//! `owl:equivalentClass` over an `owl:intersectionOf` list. A lone
//! `"[[Slug]]"` scalar is accepted as a one-item list.
//!
//! That is all of it, deliberately: a conjunction of names and existentials
//! over names is exactly the OWL 2 EL class-expression fragment the corpus
//! needs, and nothing else parses. Universal restrictions, cardinality,
//! `hasValue`, negation, disjunction, inverses, nominals and nested
//! expressions are refused with [`NON_EL_DEFINITION`]; a closed-world
//! requirement belongs in a `vault validate` rule (ADR-2126), not here.
//!
//! ```
//! use vault_core::definition::{parse, Conjunct};
//! use vault_core::vocabulary::Vocabulary;
//!
//! let vocab = Vocabulary::from_yaml_str(r#"
//! version: 1
//! relations:
//!   is-a:       { owl: "rdfs:subClassOf" }
//!   has-part:   { owl: "vc:hasPart", restriction: true }
//!   defines-as: { owl: "owl:equivalentClass", status: provisional }
//! "#).unwrap();
//!
//! let value: serde_yaml::Value =
//!     serde_yaml::from_str("[\"[[Robot]]\", {has-part: \"[[Gripper]]\"}]").unwrap();
//! let conjuncts = parse(&value, &vocab).unwrap();
//! assert!(matches!(&conjuncts[0], Conjunct::Named(w) if w.target == "Robot"));
//! assert!(matches!(&conjuncts[1],
//!     Conjunct::Some { relation, filler } if relation == "has-part" && filler.target == "Gripper"));
//!
//! let only: serde_yaml::Value = serde_yaml::from_str("[{only: \"[[Gripper]]\"}]").unwrap();
//! assert!(parse(&only, &vocab).unwrap_err().contains("outside OWL 2 EL"));
//! ```

use serde_yaml::Value as Yaml;

use crate::frontmatter::Wikilink;
use crate::vocabulary::Vocabulary;

/// The frontmatter key that carries a class definition.
pub const DEFINES_AS_KEY: &str = "defines-as";

/// The OWL IRI a definition is emitted under.
pub const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";

/// Validation code: a `defines-as` value that is not a conjunction of named
/// classes and existentials over emitted object properties, or that names
/// something other than a class page.
pub const NON_EL_DEFINITION: &str = "NON_EL_DEFINITION";

/// Map keys an author might reach for that name a construct outside OWL 2 EL.
/// They get a message saying so, rather than "not a declared relation".
pub const NON_EL_KEYWORDS: &[&str] = &[
    "only",
    "all",
    "all-values-from",
    "min",
    "max",
    "exactly",
    "cardinality",
    "min-cardinality",
    "max-cardinality",
    "value",
    "has-value",
    "not",
    "complement-of",
    "or",
    "union-of",
    "one-of",
    "inverse",
    "inverse-of",
    "self",
];

/// The validation code a [`parse`] refusal is reported under: the specific
/// rule when the message starts with one
/// ([`DEFINITION_OVER_TRANSITIVE`](crate::vocabulary::DEFINITION_OVER_TRANSITIVE),
/// [`NOT_AN_EXISTENTIAL`](crate::vocabulary::NOT_AN_EXISTENTIAL)), else
/// [`NON_EL_DEFINITION`].
///
/// ```
/// use vault_core::definition::{refusal_code, NON_EL_DEFINITION};
/// assert_eq!(refusal_code("DEFINITION_OVER_TRANSITIVE: …"), "DEFINITION_OVER_TRANSITIVE");
/// assert_eq!(refusal_code("`only` is outside OWL 2 EL"), NON_EL_DEFINITION);
/// ```
#[must_use]
pub fn refusal_code(message: &str) -> &'static str {
    use crate::vocabulary::{DEFINITION_OVER_TRANSITIVE, NOT_AN_EXISTENTIAL};
    [DEFINITION_OVER_TRANSITIVE, NOT_AN_EXISTENTIAL]
        .into_iter()
        .find(|code| {
            message
                .strip_prefix(code)
                .is_some_and(|rest| rest.starts_with(": "))
        })
        .unwrap_or(NON_EL_DEFINITION)
}

/// A [`parse`] refusal without the `CODE: ` prefix [`refusal_code`] reads, so
/// a report that carries the code separately does not print it twice.
#[must_use]
pub fn strip_code(message: &str) -> &str {
    let code = refusal_code(message);
    message
        .strip_prefix(code)
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(message)
}

/// One conjunct of a definition, as authored (targets still wikilinks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conjunct {
    /// A named class.
    Named(Wikilink),
    /// `∃relation.filler`.
    Some {
        /// The relation's frontmatter key, e.g. `has-part`.
        relation: String,
        /// The named filler class.
        filler: Wikilink,
    },
}

impl Conjunct {
    /// The wikilink this conjunct names: the class, or the filler.
    #[must_use]
    pub fn target(&self) -> &Wikilink {
        match self {
            Self::Named(w) | Self::Some { filler: w, .. } => w,
        }
    }
}

/// Parse a `defines-as` value into its conjuncts.
///
/// # Errors
/// A one-line, human-actionable message for every shape outside the admitted
/// fragment: see the module docs. A refusal under a specific rule starts with
/// its code (`DEFINITION_OVER_TRANSITIVE: …`, `DEFINITION_NOT_EXISTENTIAL: …`);
/// every other carries none and is a [`NON_EL_DEFINITION`]. [`refusal_code`]
/// and [`strip_code`] split the two.
pub fn parse(value: &Yaml, vocab: &Vocabulary) -> Result<Vec<Conjunct>, String> {
    let items: &[Yaml] = match value {
        Yaml::Sequence(items) => items,
        Yaml::String(_) => std::slice::from_ref(value),
        _ => {
            return Err(format!(
                "`{DEFINES_AS_KEY}` must be a list of conjuncts: `\"[[Slug]]\"` or \
                 `relation: \"[[Slug]]\"`"
            ))
        }
    };
    if items.is_empty() {
        return Err(format!(
            "an empty `{DEFINES_AS_KEY}` defines nothing; remove the key"
        ));
    }
    items.iter().map(|item| conjunct(item, vocab)).collect()
}

/// A `"[[Slug]]"` string naming exactly one class.
fn class_name(raw: &str) -> Option<Wikilink> {
    let link = Wikilink::parse(raw)?;
    let malformed = |s: &str| s.contains("[[") || s.contains("]]");
    (!malformed(&link.target) && !link.alias.as_deref().is_some_and(malformed)).then_some(link)
}

fn conjunct(item: &Yaml, vocab: &Vocabulary) -> Result<Conjunct, String> {
    match item {
        Yaml::String(raw) => class_name(raw).map(Conjunct::Named).ok_or_else(|| {
            format!(
                "`{raw}` is not one named class `[[Slug]]`; a definition is a conjunction of \
                 names and `relation: [[Slug]]` existentials (no `or`, `not`, lists or prose)"
            )
        }),
        Yaml::Mapping(map) => {
            if map.len() != 1 {
                return Err(format!(
                    "a conjunct map holds exactly one `relation: \"[[Slug]]\"` pair, found {}; \
                     write one list item per existential",
                    map.len()
                ));
            }
            let (key, filler) = map.iter().next().expect("one entry");
            let Some(relation) = key.as_str() else {
                return Err("a conjunct map's key must be a relation key".to_owned());
            };
            existential(relation, filler, vocab)
        }
        other => Err(format!(
            "a {} is not a named class `[[Slug]]` or a `relation: [[Slug]]` existential",
            type_name(other)
        )),
    }
}

fn existential(relation: &str, filler: &Yaml, vocab: &Vocabulary) -> Result<Conjunct, String> {
    if NON_EL_KEYWORDS.contains(&relation) {
        return Err(format!(
            "`{relation}` is outside OWL 2 EL; a definition admits only named classes and \
             existentials (`relation: [[Slug]]`). Express a closed-world requirement as a \
             `vault validate` rule (ADR-2126)"
        ));
    }
    if vocab.existential_property(relation).is_none() {
        if let Some(refusal) = vocab.existential_refusal(relation) {
            return Err(refusal);
        }
        return Err(match vocab.relations.get(relation) {
            None => format!("`{relation}` is not a declared relation in ontology/vocabulary.yaml"),
            Some(def) if !def.emitted => format!(
                "relation `{relation}` is not emitted to ontology.ttl, so an existential over \
                 it would define the class over nothing the reasoner can see"
            ),
            Some(def) => format!(
                "relation `{relation}` maps to {}, which is not an object property; \
                 existentials range over the corpus's emitted object properties",
                def.owl
            ),
        });
    }
    if let Some(refusal) = vocab.transitive_refusal(relation) {
        return Err(refusal);
    }
    let filler = match filler {
        Yaml::String(raw) => class_name(raw).ok_or_else(|| {
            format!("`{relation}: {raw}` needs one named filler class `[[Slug]]`")
        })?,
        Yaml::Sequence(_) => {
            return Err(format!(
                "`{relation}` takes one filler per conjunct; write a separate \
                 `{relation}: \"[[Slug]]\"` item for each"
            ))
        }
        Yaml::Mapping(_) => {
            return Err(format!(
                "`{relation}` has a nested class expression; the filler must be a named \
                 `[[Slug]]` (define the inner class on its own page)"
            ))
        }
        other => {
            return Err(format!(
                "`{relation}` has a {} filler; it needs one named `[[Slug]]`",
                type_name(other)
            ))
        }
    };
    Ok(Conjunct::Some {
        relation: relation.to_owned(),
        filler,
    })
}

fn type_name(v: &Yaml) -> &'static str {
    match v {
        Yaml::Null => "null",
        Yaml::Bool(_) => "boolean",
        Yaml::Number(_) => "number",
        Yaml::String(_) => "string",
        Yaml::Sequence(_) => "list",
        Yaml::Mapping(_) => "map",
        Yaml::Tagged(_) => "tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::{DEFINITION_OVER_TRANSITIVE, NOT_AN_EXISTENTIAL};

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
relations:
  is-a:          { owl: "rdfs:subClassOf" }
  has-part:      { owl: "vc:hasPart", restriction: true }
  requires:      { owl: "vc:requires", restriction: true }
  enables:       { owl: "vc:enables" }
  precedes:      { owl: "vc:precedes", characteristics: [transitive], restriction: true }
  just-before:   { owl: "vc:justBefore", sub_property_of: "vc:precedes", restriction: true }
  produces:      { owl: "vc:produces", emitted: false, status: provisional }
  disjoint-with: { owl: "owl:disjointWith", status: provisional }
  same-as:       { owl: "skos:exactMatch", emitted: false }
  defines-as:    { owl: "owl:equivalentClass", status: provisional }
"#,
        )
        .unwrap()
    }

    fn yaml(s: &str) -> Yaml {
        serde_yaml::from_str(s).unwrap()
    }

    fn err(s: &str) -> String {
        parse(&yaml(s), &vocab()).expect_err(s)
    }

    #[test]
    fn names_and_existentials_parse_in_author_order() {
        let c = parse(
            &yaml(
                "[\"[[Robot]]\", {has-part: \"[[Gripper|a gripper]]\"}, {requires: \"[[Power]]\"}]",
            ),
            &vocab(),
        )
        .unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], Conjunct::Named(Wikilink::new("Robot")));
        assert_eq!(
            c[1],
            Conjunct::Some {
                relation: "has-part".into(),
                filler: Wikilink {
                    target: "Gripper".into(),
                    alias: Some("a gripper".into())
                }
            }
        );
        assert_eq!(c[2].target().target, "Power");
    }

    #[test]
    fn a_lone_wikilink_scalar_is_a_one_item_definition() {
        assert_eq!(
            parse(&yaml("\"[[Robot]]\""), &vocab()).unwrap(),
            vec![Conjunct::Named(Wikilink::new("Robot"))]
        );
    }

    #[test]
    fn every_non_el_keyword_is_named_as_such() {
        for kw in NON_EL_KEYWORDS {
            let e = err(&format!("[{{{kw}: \"[[Gripper]]\"}}]"));
            assert!(e.contains("outside OWL 2 EL"), "{kw}: {e}");
        }
    }

    #[test]
    fn disjunction_and_negation_inside_a_string_are_not_names() {
        for s in [
            "[\"[[A]] or [[B]]\"]",
            "[\"not [[A]]\"]",
            "[\"[[A]], [[B]]\"]",
            "[\"Robot\"]",
            "[42]",
            "[null]",
            "[[\"[[A]]\"]]",
        ] {
            assert!(err(s).contains("named class"), "{s}: {}", err(s));
        }
    }

    #[test]
    fn malformed_existentials_are_refused_with_a_reason() {
        assert!(err("[]").contains("empty"));
        assert!(err("{has-part: \"[[A]]\"}").contains("list"));
        assert!(err("[{has-part: [\"[[A]]\", \"[[B]]\"]}]").contains("one filler"));
        assert!(err("[{has-part: {has-part: \"[[A]]\"}}]").contains("nested"));
        assert!(err("[{has-part: \"[[A]]\", requires: \"[[B]]\"}]").contains("exactly one"));
        assert!(err("[{has-part: \"A\"}]").contains("[[Slug]]"));
        assert!(err("[{produces: \"[[A]]\"}]").contains("not emitted"));
        assert!(err("[{same-as: \"[[A]]\"}]").contains("not emitted"));
        assert!(err("[{is-a: \"[[A]]\"}]").contains("not an object property"));
        assert!(err("[{disjoint-with: \"[[A]]\"}]").contains("not an object property"));
        assert!(err("[{defines-as: \"[[A]]\"}]").contains("not an object property"));
        assert!(err("[{frobs: \"[[A]]\"}]").contains("not a declared relation"));
    }

    /// Defect: an emitted object property without `restriction: true` was
    /// reported as "not an object property". It is one; what it lacks is the
    /// `C ⊑ ∃P.D` axioms a definition needs, and the refusal must say so.
    #[test]
    fn a_plain_emitted_property_is_refused_as_not_an_existential() {
        let e = err("[{enables: \"[[A]]\"}]");
        assert!(e.starts_with(NOT_AN_EXISTENTIAL), "{e}");
        assert!(e.contains("restriction: true"), "{e}");
        assert!(!e.contains("not an object property"), "{e}");
        assert_eq!(refusal_code(&e), NOT_AN_EXISTENTIAL);
    }

    /// ADR-2124: an existential over a transitive property, or one with a
    /// transitive property in its sub/super-property closure, is refused
    /// before the reasoner can be asked to saturate it.
    #[test]
    fn an_existential_over_a_transitive_hierarchy_is_refused() {
        for (key, culprit) in [("precedes", "#precedes"), ("just-before", "#precedes")] {
            let e = err(&format!("[\"[[Robot]]\", {{{key}: \"[[A]]\"}}]"));
            assert!(e.starts_with(DEFINITION_OVER_TRANSITIVE), "{key}: {e}");
            assert!(e.contains(culprit), "{key}: {e}");
            assert_eq!(refusal_code(&e), DEFINITION_OVER_TRANSITIVE);
        }
        // has-part and requires (non-transitive here) still parse.
        assert!(parse(&yaml("[{requires: \"[[A]]\"}]"), &vocab()).is_ok());
    }

    #[test]
    fn every_other_refusal_carries_the_generic_code() {
        assert_eq!(refusal_code(&err("[{only: \"[[A]]\"}]")), NON_EL_DEFINITION);
        assert_eq!(
            refusal_code(&err("[{frobs: \"[[A]]\"}]")),
            NON_EL_DEFINITION
        );
        assert!(strip_code(&err("[{enables: \"[[A]]\"}]")).starts_with("relation `enables`"));
    }
}
