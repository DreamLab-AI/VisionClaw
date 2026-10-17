//! Golden table: utterance → expected `Command`, through the public API.

use codescape_collab::grammar::{
    DEFAULT_AFFECTED_DEPTH as AD, DEFAULT_RELATION_DEPTH as RD, MAX_DEPTH,
};
use codescape_collab::{command_from_llm, parse_command, Command, ParseError, Target};
use codescape_model::Lang;

fn name(s: &str) -> Target {
    Target::Name(s.to_owned())
}

type Case = (&'static str, fn() -> Command);

const GOLDEN: &[Case] = &[
    // Focus
    ("focus main", || Command::Focus(name("main"))),
    ("Focus on the graph service", || {
        Command::Focus(name("graph service"))
    }),
    ("go to graph underscore service dot rs", || {
        Command::Focus(name("graph_service.rs"))
    }),
    ("Show me src slash main dot rs", || {
        Command::Focus(name("src/main.rs"))
    }),
    ("find GraphServiceActor", || {
        Command::Focus(name("graphserviceactor"))
    }),
    ("jump to foo colon colon bar", || {
        Command::Focus(name("foo::bar"))
    }),
    ("where is the parser", || Command::Focus(name("parser"))),
    ("go to number two", || Command::Focus(Target::Candidate(2))),
    ("show me the second one", || {
        Command::Focus(Target::Candidate(2))
    }),
    ("select the third", || Command::Focus(Target::Candidate(3))),
    ("focus 1", || Command::Focus(Target::Candidate(1))),
    ("Um, can you focus this please?", || {
        Command::Focus(Target::This)
    }),
    // Spoken punctuation right after the command word
    ("focus dot env", || Command::Focus(name(".env"))),
    (
        "go to underscore underscore init underscore underscore dot py",
        || Command::Focus(name("__init__.py")),
    ),
    ("explain slash etc slash hosts", || {
        Command::Explain(name("/etc/hosts"))
    }),
    ("focus on the dot env file", || {
        Command::Focus(name(".env file"))
    }),
    ("go to to underscore string", || {
        Command::Focus(name("to_string"))
    }),
    ("callers of colon colon new", || Command::Callers {
        target: name("::new"),
        depth: RD,
    }),
    ("what does dot env affect", || Command::Affected {
        target: name(".env"),
        depth: AD,
    }),
    ("go back by five minutes", || Command::Rewind {
        seconds: 300,
    }),
    // Explain
    ("explain this", || Command::Explain(Target::This)),
    ("What is the force compute actor?", || {
        Command::Explain(name("force compute actor"))
    }),
    ("what's it", || Command::Explain(Target::This)),
    ("describe number four", || {
        Command::Explain(Target::Candidate(4))
    }),
    ("tell me about the binary protocol", || {
        Command::Explain(name("binary protocol"))
    }),
    ("what does apply changed do", || {
        Command::Explain(name("apply changed"))
    }),
    // Callers
    ("callers of main", || Command::Callers {
        target: name("main"),
        depth: RD,
    }),
    ("who calls this", || Command::Callers {
        target: Target::This,
        depth: RD,
    }),
    ("what calls parse command depth two", || Command::Callers {
        target: name("parse command"),
        depth: 2,
    }),
    ("show me the callers of it three levels deep", || {
        Command::Callers {
            target: Target::This,
            depth: 3,
        }
    }),
    ("find callers of handler with depth of 4", || {
        Command::Callers {
            target: name("handler"),
            depth: 4,
        }
    }),
    ("who calls x to depth 50", || Command::Callers {
        target: name("x"),
        depth: MAX_DEPTH,
    }),
    // Callees
    ("callees of main", || Command::Callees {
        target: name("main"),
        depth: RD,
    }),
    ("what does this call", || Command::Callees {
        target: Target::This,
        depth: RD,
    }),
    ("what does the server call 2 levels", || Command::Callees {
        target: name("server"),
        depth: 2,
    }),
    ("what is called by number one", || Command::Callees {
        target: Target::Candidate(1),
        depth: RD,
    }),
    // Affected
    ("what does this affect", || Command::Affected {
        target: Target::This,
        depth: AD,
    }),
    ("affected by graph dot rs", || Command::Affected {
        target: name("graph.rs"),
        depth: AD,
    }),
    ("what is affected by the interner", || Command::Affected {
        target: name("interner"),
        depth: AD,
    }),
    ("blast radius of it", || Command::Affected {
        target: Target::This,
        depth: AD,
    }),
    ("dependents of the model crate 2 levels", || {
        Command::Affected {
            target: name("model crate"),
            depth: 2,
        }
    }),
    ("what is the blast radius of id interner", || {
        Command::Affected {
            target: name("id interner"),
            depth: AD,
        }
    }),
    ("what does lod affect depth one", || Command::Affected {
        target: name("lod"),
        depth: 1,
    }),
    // Path
    ("path from main to the renderer", || Command::Path {
        from: name("main"),
        to: name("renderer"),
    }),
    ("show me the shortest path from this to number two", || {
        Command::Path {
            from: Target::This,
            to: Target::Candidate(2),
        }
    }),
    ("how does the client reach the gpu kernel", || {
        Command::Path {
            from: name("client"),
            to: name("gpu kernel"),
        }
    }),
    ("how does ingest get to layout", || Command::Path {
        from: name("ingest"),
        to: name("layout"),
    }),
    ("path between a and b", || Command::Path {
        from: name("a"),
        to: name("b"),
    }),
    // Filters
    ("show only rust", || Command::ShowOnly {
        lang: Some(Lang::Rust),
        community: None,
    }),
    ("Only show TypeScript files", || Command::ShowOnly {
        lang: Some(Lang::TypeScript),
        community: None,
    }),
    ("show python only", || Command::ShowOnly {
        lang: Some(Lang::Python),
        community: None,
    }),
    ("show just go", || Command::ShowOnly {
        lang: Some(Lang::Go),
        community: None,
    }),
    ("show only gd script", || Command::ShowOnly {
        lang: Some(Lang::GdScript),
        community: None,
    }),
    ("filter to javascript", || Command::ShowOnly {
        lang: Some(Lang::JavaScript),
        community: None,
    }),
    ("show community seven", || Command::ShowOnly {
        lang: None,
        community: Some(7),
    }),
    ("show only community number 12", || Command::ShowOnly {
        lang: None,
        community: Some(12),
    }),
    ("show all", || Command::ShowAll),
    ("reset filter", || Command::ShowAll),
    ("clear the filters please", || Command::ShowAll),
    ("hide tests", || Command::HideTests),
    ("hide the tests", || Command::HideTests),
    ("isolate this community", || Command::IsolateCommunity(None)),
    ("isolate community 4", || Command::IsolateCommunity(Some(4))),
    ("isolate community number three", || {
        Command::IsolateCommunity(Some(3))
    }),
    // Camera
    ("zoom in", || Command::ZoomIn),
    ("Zoom out.", || Command::ZoomOut),
    ("overview", || Command::Overview),
    ("zoom all the way out", || Command::Overview),
    ("back", || Command::Back),
    ("go back", || Command::Back),
    // Time
    ("rewind five minutes", || Command::Rewind { seconds: 300 }),
    ("rewind 30 seconds", || Command::Rewind { seconds: 30 }),
    ("rewind a minute", || Command::Rewind { seconds: 60 }),
    ("go back ten minutes", || Command::Rewind { seconds: 600 }),
    ("rewind by two hours", || Command::Rewind { seconds: 7200 }),
    ("rewind", || Command::Rewind { seconds: 60 }),
    ("replay", || Command::Replay),
    ("replay last session", || Command::Replay),
    ("live", || Command::Live),
    ("go live", || Command::Live),
    // Presence
    ("what is claude doing", || Command::WhatIsDoing {
        actor: Some("claude".into()),
    }),
    ("What's everyone doing?", || Command::WhatIsDoing {
        actor: None,
    }),
    ("what are the agents doing", || Command::WhatIsDoing {
        actor: None,
    }),
    ("what is codex up to", || Command::WhatIsDoing {
        actor: Some("codex".into()),
    }),
    ("follow john", || Command::Follow {
        actor: "john".into(),
    }),
    ("follow claude tab zero", || Command::Follow {
        actor: "claude tab 0".into(),
    }),
    ("stop following", || Command::StopFollowing),
    ("unfollow", || Command::StopFollowing),
    // Delegation
    ("ask claude to fix the failing test", || Command::Ask {
        actor: "claude".into(),
        task: "fix the failing test".into(),
    }),
    ("Tell codex to add tests to graph dot rs", || Command::Ask {
        actor: "codex".into(),
        task: "add tests to graph.rs".into(),
    }),
    ("hey can you ask the reviewer to look at this", || {
        Command::Ask {
            actor: "reviewer".into(),
            task: "look at this".into(),
        }
    }),
    // Help
    ("help", || Command::Help),
    ("what can I say?", || Command::Help),
];

#[test]
fn golden_table() {
    assert!(GOLDEN.len() >= 60, "only {} golden cases", GOLDEN.len());
    let mut failures = Vec::new();
    for (utterance, want) in GOLDEN {
        let want = want();
        match parse_command(utterance) {
            Ok(got) if got == want => {}
            other => failures.push(format!("{utterance:?}: want {want:?}, got {other:?}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn golden_commands_pass_the_llm_gate() {
    for (_, want) in GOLDEN {
        let cmd = want();
        let reply = serde_json::json!({ "command": cmd, "answer": "" });
        assert_eq!(command_from_llm(&reply).unwrap().0, Some(cmd));
    }
}

#[test]
fn every_command_variant_is_reachable_by_voice() {
    let tags: std::collections::BTreeSet<String> = GOLDEN
        .iter()
        .map(|(_, c)| match serde_json::to_value(c()).unwrap() {
            serde_json::Value::String(s) => s,
            serde_json::Value::Object(m) => m.keys().next().unwrap().clone(),
            v => panic!("{v}"),
        })
        .collect();
    assert_eq!(tags.len(), 22, "{tags:?}");
}

#[test]
fn non_commands() {
    for s in [
        "sing me a song",
        "rewind the tape",
        "ask claude",
        "show only klingon",
        "path from a",
    ] {
        assert!(
            matches!(parse_command(s), Err(ParseError::NoMatch { .. })),
            "{s}"
        );
    }
    assert_eq!(parse_command(" um, uh "), Err(ParseError::Empty));
}
