//! Structured output for the language-model fallback.
//!
//! When [`parse_command`](crate::parse_command) returns
//! [`NoMatch`](crate::ParseError::NoMatch), the server can ask a language
//! model to interpret the utterance. It constrains the model with
//! [`command_schema`], so the reply looks like
//! `{"command": <Command or null>, "answer": "…"}`, and accepts the reply only
//! through [`command_from_llm`]. That function deserialises strictly and runs
//! [`Command::validate`]. A reply that fails is an error. The caller may speak
//! the answer text, but must never execute it.

use codescape_model::Lang;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::grammar::{
    Command, ParseError, MAX_CANDIDATE, MAX_DEPTH, MAX_NAME_LEN, MAX_REWIND_SECONDS, MAX_TASK_LEN,
};

/// Every [`Lang`] variant, in declaration order. A test with an exhaustive
/// match keeps this list complete.
pub(crate) const ALL_LANGS: [Lang; 12] = [
    Lang::Rust,
    Lang::TypeScript,
    Lang::JavaScript,
    Lang::Python,
    Lang::Go,
    Lang::GdScript,
    Lang::Cpp,
    Lang::Cuda,
    Lang::Shell,
    Lang::Nix,
    Lang::Markdown,
    Lang::Other,
];

/// Tags of the unit variants (kept in step with serde by `schema_tags_match_serde`) of [`Command`], which serialise as bare strings.
pub(crate) const UNIT_TAGS: [&str; 10] = [
    "show_all",
    "hide_tests",
    "zoom_in",
    "zoom_out",
    "overview",
    "back",
    "replay",
    "live",
    "stop_following",
    "help",
];

fn object(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

/// One externally tagged variant: `{"<tag>": <inner>}`.
fn tagged(tag: &str, inner: Value) -> Value {
    let mut props = serde_json::Map::new();
    props.insert(tag.to_owned(), inner);
    object(Value::Object(props), &[tag])
}

fn nullable(v: Value) -> Value {
    json!({ "anyOf": [v, { "type": "null" }] })
}

fn name_schema() -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": MAX_NAME_LEN })
}

fn target_schema() -> Value {
    json!({
        "description": "\"this\" = the speaker's selection; {\"candidate\":n} = numbered disambiguation choice; {\"name\":s} = a symbol, file or directory name",
        "anyOf": [
            { "type": "string", "enum": ["this"] },
            tagged("name", name_schema()),
            tagged("candidate", json!({ "type": "integer", "minimum": 1, "maximum": MAX_CANDIDATE })),
        ]
    })
}

fn depth_schema() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": MAX_DEPTH })
}

fn lang_names() -> Vec<Value> {
    ALL_LANGS
        .iter()
        .map(|l| serde_json::to_value(l).expect("Lang serialises to a string"))
        .collect()
}

/// The JSON Schema (draft 2020-12) for a fallback reply:
/// `{"command": <Command, externally tagged> | null, "answer": string}`.
///
/// The schema inlines everything (no `$ref`) and lists every property as
/// required, so it works with strict structured-output decoders. Constraints
/// that span fields, such as `ShowOnly` needing at least one filter, are left
/// to [`Command::validate`].
///
/// ```
/// let s = codescape_collab::command_schema();
/// assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
/// assert_eq!(s["required"], serde_json::json!(["command", "answer"]));
/// ```
pub fn command_schema() -> Value {
    let rel = |tag: &str| {
        tagged(
            tag,
            object(
                json!({ "target": target_schema(), "depth": depth_schema() }),
                &["target", "depth"],
            ),
        )
    };
    let community = json!({ "type": "integer", "minimum": 0 });
    let mut variants = vec![
        json!({ "type": "string", "enum": UNIT_TAGS }),
        tagged("focus", target_schema()),
        tagged("explain", target_schema()),
        rel("callers"),
        rel("callees"),
        rel("affected"),
        tagged(
            "path",
            object(
                json!({ "from": target_schema(), "to": target_schema() }),
                &["from", "to"],
            ),
        ),
        tagged(
            "show_only",
            object(
                json!({
                    "lang": nullable(json!({ "type": "string", "enum": lang_names() })),
                    "community": nullable(community.clone()),
                }),
                &["lang", "community"],
            ),
        ),
        tagged("isolate_community", nullable(community)),
        tagged(
            "rewind",
            object(
                json!({ "seconds": { "type": "integer", "minimum": 1, "maximum": MAX_REWIND_SECONDS } }),
                &["seconds"],
            ),
        ),
        tagged(
            "what_is_doing",
            object(json!({ "actor": nullable(name_schema()) }), &["actor"]),
        ),
        tagged(
            "follow",
            object(json!({ "actor": name_schema() }), &["actor"]),
        ),
        tagged(
            "ask",
            object(
                json!({
                    "actor": name_schema(),
                    "task": { "type": "string", "minLength": 1, "maxLength": MAX_TASK_LEN },
                }),
                &["actor", "task"],
            ),
        ),
    ];
    variants.push(json!({ "type": "null" }));
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "CodescapeCommandReply",
        "description": "Interpret a spoken request about a code graph. Set command to null when the request is not a view command; always put a short spoken reply in answer.",
        "type": "object",
        "properties": {
            "command": { "anyOf": variants },
            "answer": { "type": "string" },
        },
        "required": ["command", "answer"],
        "additionalProperties": false,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    #[serde(default)]
    command: Option<Command>,
    answer: String,
}

/// Turn a structured fallback reply into `(command, answer)`.
///
/// Deserialisation is strict: unknown fields, a missing `answer`, wrong types
/// or a multi-key variant object are all [`ParseError::Invalid`]. So is a
/// command that fails [`Command::validate`]. A `null` or absent `command`
/// means the model only answered.
///
/// ```
/// use codescape_collab::{command_from_llm, Command, Target};
/// let v = serde_json::json!({"command": {"focus": {"name": "main"}}, "answer": "Going to main."});
/// assert_eq!(command_from_llm(&v).unwrap(),
///            (Some(Command::Focus(Target::Name("main".into()))), "Going to main.".to_owned()));
/// assert!(command_from_llm(&serde_json::json!({"command": "rm_rf", "answer": ""})).is_err());
/// ```
pub fn command_from_llm(v: &Value) -> Result<(Option<Command>, String), ParseError> {
    let reply = Reply::deserialize(v).map_err(|e| ParseError::Invalid {
        reason: e.to_string(),
    })?;
    if let Some(cmd) = &reply.command {
        cmd.validate()?;
    }
    Ok((reply.command, reply.answer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Target;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    /// Validate `v` against the subset of JSON Schema that `command_schema`
    /// uses: anyOf, type, enum, properties, required, additionalProperties,
    /// minimum, maximum, minLength, maxLength.
    pub(crate) fn conforms(schema: &Value, v: &Value) -> bool {
        if let Some(alts) = schema.get("anyOf").and_then(Value::as_array) {
            if !alts.iter().any(|a| conforms(a, v)) {
                return false;
            }
        }
        if let Some(t) = schema.get("type").and_then(Value::as_str) {
            let ok = match t {
                "null" => v.is_null(),
                "string" => v.is_string(),
                "integer" => v.is_u64() || v.is_i64(),
                "object" => v.is_object(),
                other => panic!("unsupported type {other}"),
            };
            if !ok {
                return false;
            }
        }
        if let Some(e) = schema.get("enum").and_then(Value::as_array) {
            if !e.contains(v) {
                return false;
            }
        }
        if let Some(n) = v.as_f64() {
            if schema
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|m| n < m)
                || schema
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|m| n > m)
            {
                return false;
            }
        }
        if let Some(s) = v.as_str() {
            let len = s.chars().count() as u64;
            if schema
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|m| len < m)
                || schema
                    .get("maxLength")
                    .and_then(Value::as_u64)
                    .is_some_and(|m| len > m)
            {
                return false;
            }
        }
        if let (Some(obj), Some(props)) = (
            v.as_object(),
            schema.get("properties").and_then(Value::as_object),
        ) {
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if required
                .iter()
                .any(|r| !obj.contains_key(r.as_str().unwrap()))
            {
                return false;
            }
            for (k, val) in obj {
                match props.get(k) {
                    Some(s) if !conforms(s, val) => return false,
                    Some(_) => {}
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                        return false
                    }
                    None => {}
                }
            }
        }
        true
    }

    fn every_variant() -> Vec<Command> {
        let t = || Target::Name("graph_service".into());
        vec![
            Command::Focus(Target::This),
            Command::Explain(Target::Candidate(2)),
            Command::Callers {
                target: t(),
                depth: 1,
            },
            Command::Callees {
                target: t(),
                depth: 2,
            },
            Command::Affected {
                target: t(),
                depth: 3,
            },
            Command::Path {
                from: t(),
                to: Target::This,
            },
            Command::ShowOnly {
                lang: Some(Lang::TypeScript),
                community: None,
            },
            Command::ShowAll,
            Command::HideTests,
            Command::IsolateCommunity(None),
            Command::ZoomIn,
            Command::ZoomOut,
            Command::Overview,
            Command::Back,
            Command::Rewind { seconds: 60 },
            Command::Replay,
            Command::Live,
            Command::WhatIsDoing { actor: None },
            Command::Follow {
                actor: "john".into(),
            },
            Command::StopFollowing,
            Command::Ask {
                actor: "claude".into(),
                task: "fix it".into(),
            },
            Command::Help,
        ]
    }

    /// Fails to compile when `Command` or `Lang` gains a variant, so the
    /// schema and `ALL_LANGS` cannot silently fall behind.
    #[allow(dead_code)]
    fn exhaustive(c: &Command, l: Lang) {
        match c {
            Command::Focus(_)
            | Command::Explain(_)
            | Command::Callers { .. }
            | Command::Callees { .. }
            | Command::Affected { .. }
            | Command::Path { .. }
            | Command::ShowOnly { .. }
            | Command::ShowAll
            | Command::HideTests
            | Command::IsolateCommunity(_)
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::Overview
            | Command::Back
            | Command::Rewind { .. }
            | Command::Replay
            | Command::Live
            | Command::WhatIsDoing { .. }
            | Command::Follow { .. }
            | Command::StopFollowing
            | Command::Ask { .. }
            | Command::Help => {}
        }
        match l {
            Lang::Rust
            | Lang::TypeScript
            | Lang::JavaScript
            | Lang::Python
            | Lang::Go
            | Lang::GdScript
            | Lang::Cpp
            | Lang::Cuda
            | Lang::Shell
            | Lang::Nix
            | Lang::Markdown
            | Lang::Other => {}
        }
    }

    fn tag_of(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(m) => m.keys().next().unwrap().clone(),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn schema_tags_match_serde() {
        let serde_tags: BTreeSet<String> = every_variant()
            .iter()
            .map(|c| tag_of(&serde_json::to_value(c).unwrap()))
            .collect();
        assert_eq!(serde_tags.len(), 22, "one instance per variant");
        let schema = command_schema();
        let mut schema_tags = BTreeSet::new();
        for alt in schema["properties"]["command"]["anyOf"].as_array().unwrap() {
            if let Some(e) = alt.get("enum") {
                schema_tags.extend(
                    e.as_array()
                        .unwrap()
                        .iter()
                        .map(|t| t.as_str().unwrap().to_owned()),
                );
            } else if let Some(p) = alt.get("properties") {
                schema_tags.extend(p.as_object().unwrap().keys().cloned());
            }
        }
        assert_eq!(serde_tags, schema_tags);
        let langs = lang_names();
        assert_eq!(langs.len(), 12);
        assert!(
            langs.contains(&json!("type_script")),
            "serde name, not the spoken word"
        );
        for l in ALL_LANGS {
            let back: Lang = serde_json::from_value(serde_json::to_value(l).unwrap()).unwrap();
            assert_eq!(back, l);
        }
    }

    #[test]
    fn wire_shapes() {
        let cases = [
            (Command::ZoomIn, json!("zoom_in")),
            (Command::Focus(Target::This), json!({"focus": "this"})),
            (
                Command::IsolateCommunity(None),
                json!({"isolate_community": null}),
            ),
            (
                Command::Callers {
                    target: Target::Name("m".into()),
                    depth: 1,
                },
                json!({"callers": {"target": {"name": "m"}, "depth": 1}}),
            ),
            (
                Command::ShowOnly {
                    lang: Some(Lang::GdScript),
                    community: None,
                },
                json!({"show_only": {"lang": "gd_script", "community": null}}),
            ),
        ];
        for (c, want) in cases {
            assert_eq!(serde_json::to_value(&c).unwrap(), want);
        }
    }

    #[test]
    fn every_variant_conforms_and_round_trips() {
        let schema = command_schema();
        for c in every_variant() {
            let reply = json!({"command": c, "answer": "ok"});
            assert!(conforms(&schema, &reply), "{reply}");
            assert_eq!(
                command_from_llm(&reply).unwrap(),
                (Some(c), "ok".to_owned())
            );
        }
        let none = json!({"command": null, "answer": "Nothing to do."});
        assert!(conforms(&schema, &none));
        assert_eq!(
            command_from_llm(&none).unwrap(),
            (None, "Nothing to do.".to_owned())
        );
        assert_eq!(
            command_from_llm(&json!({"answer": "hi"})).unwrap(),
            (None, "hi".to_owned())
        );
    }

    #[test]
    fn rejects_bad_replies() {
        let schema = command_schema();
        let bad = [
            json!({"command": "zoom_in"}),
            json!({"command": "zoom_in", "answer": "x", "extra": 1}),
            json!({"command": "rm_rf", "answer": ""}),
            json!({"command": {"focus": "this", "zoom_in": null}, "answer": ""}),
            json!({"command": {"callers": {"target": "this", "depth": 1, "x": 2}}, "answer": ""}),
            json!({"command": {"callers": {"target": "this", "depth": 99}}, "answer": ""}),
            json!({"command": {"callers": {"target": "this", "depth": -1}}, "answer": ""}),
            json!({"command": {"focus": {"name": ""}}, "answer": ""}),
            json!({"command": {"rewind": {"seconds": 0}}, "answer": ""}),
            json!({"command": {"show_only": {"lang": "klingon", "community": null}}, "answer": ""}),
            json!({"command": {"ask": {"actor": "claude", "task": ""}}, "answer": ""}),
            json!("focus main"),
            json!(null),
            json!([1, 2]),
        ];
        for b in bad {
            assert!(
                matches!(command_from_llm(&b), Err(ParseError::Invalid { .. })),
                "{b}"
            );
            assert!(!conforms(&schema, &b), "schema should reject {b}");
        }
        // The schema cannot express "at least one filter"; validate does.
        let both_null =
            json!({"command": {"show_only": {"lang": null, "community": null}}, "answer": ""});
        assert!(conforms(&schema, &both_null));
        assert!(command_from_llm(&both_null).is_err());
    }

    fn target() -> impl Strategy<Value = Target> {
        prop_oneof![
            Just(Target::This),
            (1..=MAX_CANDIDATE).prop_map(Target::Candidate),
            "[a-z_][a-zA-Z0-9_.:/ -]{0,40}".prop_map(Target::Name),
        ]
    }

    fn name() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9:_ ]{0,30}"
    }

    pub(crate) fn command() -> impl Strategy<Value = Command> {
        let depth = 1..=MAX_DEPTH;
        let lang = prop::sample::select(ALL_LANGS.to_vec());
        prop_oneof![
            target().prop_map(Command::Focus),
            target().prop_map(Command::Explain),
            (target(), depth.clone())
                .prop_map(|(target, depth)| Command::Callers { target, depth }),
            (target(), depth.clone())
                .prop_map(|(target, depth)| Command::Callees { target, depth }),
            (target(), depth).prop_map(|(target, depth)| Command::Affected { target, depth }),
            (target(), target()).prop_map(|(from, to)| Command::Path { from, to }),
            (prop::option::of(lang), prop::option::of(any::<u32>()))
                .prop_filter("one filter", |(l, c)| l.is_some() || c.is_some())
                .prop_map(|(lang, community)| Command::ShowOnly { lang, community }),
            Just(Command::ShowAll),
            Just(Command::HideTests),
            prop::option::of(any::<u32>()).prop_map(Command::IsolateCommunity),
            Just(Command::ZoomIn),
            Just(Command::ZoomOut),
            Just(Command::Overview),
            Just(Command::Back),
            (1..=MAX_REWIND_SECONDS).prop_map(|seconds| Command::Rewind { seconds }),
            Just(Command::Replay),
            Just(Command::Live),
            prop::option::of(name()).prop_map(|actor| Command::WhatIsDoing { actor }),
            name().prop_map(|actor| Command::Follow { actor }),
            Just(Command::StopFollowing),
            (name(), "[a-z][a-z0-9 ,.]{0,80}")
                .prop_map(|(actor, task)| Command::Ask { actor, task }),
            Just(Command::Help),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn llm_round_trips_every_variant(cmd in command(), answer in ".{0,40}") {
            let v = serde_json::to_value(&cmd).unwrap();
            let reply = json!({"command": v, "answer": answer});
            prop_assert!(conforms(&command_schema(), &reply), "{}", reply);
            prop_assert_eq!(command_from_llm(&reply), Ok((Some(cmd), answer)));
        }

        #[test]
        fn llm_never_panics_on_junk(s in ".{0,64}", n in any::<i64>()) {
            for v in [
                json!({"command": s, "answer": s}),
                json!({"command": {s.clone(): n}, "answer": ""}),
                json!({"command": {"callers": {"target": {"name": s}, "depth": n}}, "answer": ""}),
            ] {
                let _ = command_from_llm(&v);
            }
        }
    }
}
