//! `vault edit` — guarded mutation.
//!
//! The rule, borrowed from IWE's blast-radius guards: **an edit that has not
//! declared its blast radius is refused.** `--expect docs=1,blocks=2` says "I
//! believe this touches one document and two frontmatter keys"; if the edit
//! would touch a different number, nothing is written and the caller is told
//! which guard was wrong. That turns a mistaken selector from a silent
//! 8,000-page rewrite into an error message.
//!
//! Three assignment forms:
//!
//! | form | meaning |
//! |---|---|
//! | `--set key=value` | set (or replace) a scalar |
//! | `--set 'key+=value'` | append to a list, creating it if absent |
//! | `--unset key` | remove the key |
//!
//! A value that parses as YAML is stored as YAML (so `quality=0.35` is a
//! number and `verified+={by: human:npub1, at: …}` is a mapping); anything
//! else is stored as a string.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_yaml::Value as Yaml;
use vault_core::page::Page;

/// One requested change.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Replace the value under `key`.
    Set {
        /// The frontmatter key.
        key: String,
        /// The new value.
        value: Yaml,
    },
    /// Append to the list under `key`.
    Append {
        /// The frontmatter key.
        key: String,
        /// The value to append.
        value: Yaml,
    },
    /// Remove `key`.
    Unset {
        /// The frontmatter key.
        key: String,
    },
}

impl Change {
    /// The key this change targets.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Set { key, .. } | Self::Append { key, .. } | Self::Unset { key } => key,
        }
    }

    /// Parse a `--set` argument: `key=value` or `key+=value`.
    ///
    /// # Errors
    /// When there is no `=`.
    pub fn parse_set(arg: &str) -> Result<Self, String> {
        let (raw_key, value) = arg
            .split_once('=')
            .ok_or_else(|| format!("`{arg}` is not `key=value` or `key+=value`"))?;
        if let Some(key) = raw_key.strip_suffix('+') {
            Ok(Self::Append {
                key: key.trim().to_owned(),
                value: parse_value(value),
            })
        } else {
            Ok(Self::Set {
                key: raw_key.trim().to_owned(),
                value: parse_value(value),
            })
        }
    }
}

/// Parse a CLI value as YAML, falling back to a plain string.
#[must_use]
pub fn parse_value(raw: &str) -> Yaml {
    serde_yaml::from_str::<Yaml>(raw).map_or_else(
        |_| Yaml::String(raw.to_owned()),
        |v| {
            if matches!(v, Yaml::Null) && !raw.trim().is_empty() {
                Yaml::String(raw.to_owned())
            } else {
                v
            }
        },
    )
}

/// The declared blast radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Expectation {
    /// Documents the edit may touch.
    pub docs: Option<usize>,
    /// Frontmatter keys the edit may touch.
    pub blocks: Option<usize>,
}

impl Expectation {
    /// Parse `docs=1,blocks=2`.
    ///
    /// # Errors
    /// On an unknown guard name or a non-numeric value.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut out = Self::default();
        for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (name, value) = part
                .split_once('=')
                .ok_or_else(|| format!("`{part}` is not `name=count`"))?;
            let count: usize = value
                .trim()
                .parse()
                .map_err(|_| format!("`{value}` is not a count"))?;
            match name.trim() {
                "docs" => out.docs = Some(count),
                "blocks" => out.blocks = Some(count),
                other => {
                    return Err(format!(
                        "unknown guard `{other}`; expected `docs` or `blocks`"
                    ))
                }
            }
        }
        Ok(out)
    }

    /// Guard names that were not declared.
    #[must_use]
    pub fn missing(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.docs.is_none() {
            out.push("docs");
        }
        if self.blocks.is_none() {
            out.push("blocks");
        }
        out
    }
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// `--expect` was absent or incomplete.
    #[error("refused: --expect must declare {}; e.g. --expect docs=1,blocks=1", .0.join(" and "))]
    UndeclaredBlastRadius(Vec<&'static str>),
    /// The edit would touch a different number of things than declared.
    #[error("refused: --expect {guard}={declared}, but the edit touches {actual}")]
    GuardViolated {
        /// `docs` or `blocks`.
        guard: &'static str,
        /// What the caller declared.
        declared: usize,
        /// What the edit would actually do.
        actual: usize,
    },
    /// No page with that id.
    #[error("no page with id `{0}`")]
    NoSuchPage(String),
}

/// What an edit did (or would do).
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    /// The page's id.
    pub page: String,
    /// Documents touched.
    pub docs: usize,
    /// Frontmatter keys touched.
    pub blocks: usize,
    /// Key to `(before, after)`, rendered as YAML, for the report.
    pub changes: BTreeMap<String, [Option<String>; 2]>,
    /// The page's new text.
    #[serde(skip)]
    pub rendered: String,
}

/// Apply `changes` to `page`, enforcing `expect`.
///
/// The page is mutated only after every guard has passed, so a refusal leaves
/// the in-memory page untouched as well as the file.
///
/// # Errors
/// [`EditError`] when the blast radius is undeclared or wrong.
pub fn apply(page: &Page, changes: &[Change], expect: Expectation) -> Result<Outcome, EditError> {
    let missing = expect.missing();
    if !missing.is_empty() {
        return Err(EditError::UndeclaredBlastRadius(missing));
    }

    let mut next = page.clone();
    let mut touched: BTreeMap<String, [Option<String>; 2]> = BTreeMap::new();

    for change in changes {
        let key = change.key().to_owned();
        let before = next.frontmatter.get(&key).map(render_yaml);
        match change {
            Change::Set { value, .. } => next.frontmatter.set(key.clone(), value.clone()),
            Change::Append { value, .. } => {
                let mut list = match next.frontmatter.get(&key) {
                    Some(Yaml::Sequence(items)) => items.clone(),
                    Some(other) => vec![other.clone()],
                    None => Vec::new(),
                };
                list.push(value.clone());
                next.frontmatter.set(key.clone(), Yaml::Sequence(list));
            }
            Change::Unset { .. } => {
                next.frontmatter.remove(&key);
            }
        }
        let after = next.frontmatter.get(&key).map(render_yaml);
        if before != after {
            touched.insert(key, [before, after]);
        }
    }

    let blocks = touched.len();
    let docs = usize::from(blocks > 0);

    if let Some(declared) = expect.docs {
        if declared != docs {
            return Err(EditError::GuardViolated {
                guard: "docs",
                declared,
                actual: docs,
            });
        }
    }
    if let Some(declared) = expect.blocks {
        if declared != blocks {
            return Err(EditError::GuardViolated {
                guard: "blocks",
                declared,
                actual: blocks,
            });
        }
    }

    let rendered = next
        .render()
        .map_err(|_| EditError::NoSuchPage(format!("{} could not be rendered", page.id)))?;

    Ok(Outcome {
        page: page.id.clone(),
        docs,
        blocks,
        changes: touched,
        rendered,
    })
}

fn render_yaml(v: &Yaml) -> String {
    serde_yaml::to_string(v)
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page::parse(
            "/v/pages/A.md",
            "pages/A.md",
            "A",
            "---\ntype: Class\nstatus: draft\nquality: 0.35\n---\nbody\n",
        )
        .unwrap()
    }

    #[test]
    fn an_edit_without_expect_is_refused_and_names_the_guards() {
        let err = apply(
            &page(),
            &[Change::parse_set("status=stable").unwrap()],
            Expectation::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            EditError::UndeclaredBlastRadius(vec!["docs", "blocks"])
        );
        assert!(err.to_string().contains("docs and blocks"));
    }

    #[test]
    fn a_partial_expect_names_only_what_is_missing() {
        let err = apply(
            &page(),
            &[Change::parse_set("status=stable").unwrap()],
            Expectation {
                docs: Some(1),
                blocks: None,
            },
        )
        .unwrap_err();
        assert_eq!(err, EditError::UndeclaredBlastRadius(vec!["blocks"]));
    }

    #[test]
    fn a_correct_expectation_applies_the_edit() {
        let out = apply(
            &page(),
            &[Change::parse_set("status=stable").unwrap()],
            Expectation {
                docs: Some(1),
                blocks: Some(1),
            },
        )
        .unwrap();
        assert_eq!(out.blocks, 1);
        assert!(out.rendered.contains("status: stable"));
        assert!(out.rendered.ends_with("body\n"));
    }

    #[test]
    fn a_wrong_block_count_refuses_and_names_the_guard() {
        let err = apply(
            &page(),
            &[
                Change::parse_set("status=stable").unwrap(),
                Change::parse_set("quality=0.9").unwrap(),
            ],
            Expectation {
                docs: Some(1),
                blocks: Some(1),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            EditError::GuardViolated {
                guard: "blocks",
                declared: 1,
                actual: 2
            }
        );
    }

    #[test]
    fn a_no_op_edit_touches_zero_documents() {
        let out = apply(
            &page(),
            &[Change::parse_set("status=draft").unwrap()],
            Expectation {
                docs: Some(0),
                blocks: Some(0),
            },
        )
        .unwrap();
        assert_eq!(out.docs, 0);
        assert!(out.changes.is_empty());
    }

    #[test]
    fn append_creates_the_list_and_keeps_existing_entries() {
        let out = apply(
            &page(),
            &[Change::parse_set("verified+={by: human:npub1, at: 2026-09-22T00:00:00Z}").unwrap()],
            Expectation {
                docs: Some(1),
                blocks: Some(1),
            },
        )
        .unwrap();
        assert!(out.rendered.contains("verified:"));
        assert!(out.rendered.contains("human:npub1"));
    }

    #[test]
    fn unset_removes_a_key() {
        let out = apply(
            &page(),
            &[Change::Unset {
                key: "quality".into(),
            }],
            Expectation {
                docs: Some(1),
                blocks: Some(1),
            },
        )
        .unwrap();
        assert!(!out.rendered.contains("quality"));
    }

    #[test]
    fn values_are_typed_by_yaml_where_they_can_be() {
        assert_eq!(parse_value("0.35"), Yaml::Number(0.35.into()));
        assert_eq!(parse_value("true"), Yaml::Bool(true));
        assert_eq!(parse_value("stable"), Yaml::String("stable".into()));
        assert!(matches!(parse_value("{by: a, at: b}"), Yaml::Mapping(_)));
    }

    #[test]
    fn expectation_parsing_rejects_unknown_guards() {
        assert!(Expectation::parse("docs=1,blocks=2").is_ok());
        assert!(Expectation::parse("pages=1").is_err());
        assert!(Expectation::parse("docs=many").is_err());
    }

    #[test]
    fn set_parsing_distinguishes_append_from_replace() {
        assert!(matches!(
            Change::parse_set("k+=v").unwrap(),
            Change::Append { .. }
        ));
        assert!(matches!(
            Change::parse_set("k=v").unwrap(),
            Change::Set { .. }
        ));
        assert!(Change::parse_set("novalue").is_err());
    }
}
