//! The spoken and typed command grammar.
//!
//! Voice transcripts and the typed command palette both go through
//! [`parse_command`]. Parsing is deterministic and has two steps:
//!
//! 1. [`normalise_spoken`] lowercases the text and drops fillers.
//!    Spoken punctuation becomes characters (`"graph underscore service dot rs"`
//!    becomes `graph_service.rs`) and number words become digits.
//! 2. An ordered list of phrase rules turns the result into a [`Command`]. The
//!    first rule that matches wins, so specific phrases ("what is X doing",
//!    "what does X affect") are tried before general ones ("what is X").
//!
//! Every command the grammar produces passes [`Command::validate`]. When no
//! rule matches, the result is [`ParseError::NoMatch`], which is the server's
//! cue to try the language-model fallback ([`crate::llm`]).

use codescape_model::Lang;
use serde::{Deserialize, Serialize};

/// Depth for `callers of` and `callees of` when none is spoken.
pub const DEFAULT_RELATION_DEPTH: u32 = 1;
/// Depth for `what does X affect` when none is spoken.
pub const DEFAULT_AFFECTED_DEPTH: u32 = 3;
/// Largest traversal depth. The grammar clamps to it; the language-model
/// path rejects anything above it.
pub const MAX_DEPTH: u32 = 10;
/// `rewind` without a duration rewinds this far.
pub const DEFAULT_REWIND_SECONDS: u64 = 60;
/// Longest rewind: 7 days.
pub const MAX_REWIND_SECONDS: u64 = 7 * 24 * 3600;
/// Largest disambiguation candidate number.
pub const MAX_CANDIDATE: u32 = 100;
/// Longest symbol or actor name, in characters.
pub const MAX_NAME_LEN: usize = 256;
/// Longest delegated task, in characters.
pub const MAX_TASK_LEN: usize = 2048;

/// What a command points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// A spoken symbol, file or directory name, resolved by fuzzy search.
    Name(String),
    /// A numbered candidate from the last disambiguation list ("number two",
    /// "the second one"), counting from 1.
    Candidate(u32),
    /// The speaker's current selection or focus ("this", "it", "here").
    This,
}

/// A typed instruction to the Codescape view. Parsing a command never executes
/// it; the server does that.
///
/// The serde representation is externally tagged and snake_case
/// (`"zoom_in"`, `{"focus":"this"}`, `{"callers":{"target":{"name":"main"},"depth":1}}`).
/// [`crate::llm::command_schema`] describes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Fly to and select a target.
    Focus(Target),
    /// Retrieve and summarise a target.
    Explain(Target),
    /// Who calls the target, up to `depth` hops.
    Callers {
        /// What to start from.
        target: Target,
        /// Hops, `1..=MAX_DEPTH`.
        depth: u32,
    },
    /// What the target calls, up to `depth` hops.
    Callees {
        /// What to start from.
        target: Target,
        /// Hops, `1..=MAX_DEPTH`.
        depth: u32,
    },
    /// Everything that depends on the target (blast radius), up to `depth` hops.
    Affected {
        /// What to start from.
        target: Target,
        /// Hops, `1..=MAX_DEPTH`.
        depth: u32,
    },
    /// Shortest dependency path between two targets.
    Path {
        /// Start.
        from: Target,
        /// End.
        to: Target,
    },
    /// Filter by language and/or community. At least one must be set.
    ShowOnly {
        /// Keep only this language.
        lang: Option<Lang>,
        /// Keep only this community.
        community: Option<u32>,
    },
    /// Clear every filter.
    ShowAll,
    /// Hide test files and test symbols.
    HideTests,
    /// Dim everything outside a community: `None` means the focused node's community.
    IsolateCommunity(Option<u32>),
    /// Move the camera closer.
    ZoomIn,
    /// Move the camera away.
    ZoomOut,
    /// Frame the whole graph.
    Overview,
    /// Pop the focus history.
    Back,
    /// Scrub the timeline back by `seconds`.
    Rewind {
        /// How far back, `1..=MAX_REWIND_SECONDS`.
        seconds: u64,
    },
    /// Replay the recorded timeline.
    Replay,
    /// Return to the live view.
    Live,
    /// Report what an actor is doing; `None` means everyone.
    WhatIsDoing {
        /// Actor name as spoken.
        actor: Option<String>,
    },
    /// Keep the camera on an actor's focus.
    Follow {
        /// Actor name as spoken.
        actor: String,
    },
    /// Stop following.
    StopFollowing,
    /// Delegate a task to an agent (dispatched only through a signed intent).
    Ask {
        /// Agent name as spoken.
        actor: String,
        /// The task, verbatim after normalisation.
        task: String,
    },
    /// List what can be said.
    Help,
}

/// Why an utterance or a language-model reply did not become a [`Command`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// No grammar rule matched. `normalised` is the text after
    /// [`normalise_spoken`], ready to hand to the language-model fallback.
    #[error("no command matches {normalised:?}")]
    NoMatch {
        /// The normalised utterance.
        normalised: String,
    },
    /// Nothing was left after normalisation.
    #[error("empty utterance")]
    Empty,
    /// A structured reply was malformed or failed [`Command::validate`].
    #[error("invalid command: {reason}")]
    Invalid {
        /// What was wrong.
        reason: String,
    },
}

fn check_text(what: &str, s: &str, max: usize) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err(format!("{what} is empty"));
    }
    if s.chars().count() > max {
        return Err(format!("{what} is longer than {max} characters"));
    }
    if s.chars().any(char::is_control) {
        return Err(format!("{what} contains control characters"));
    }
    Ok(())
}

impl Target {
    fn check(&self) -> Result<(), String> {
        match self {
            Self::Name(n) => check_text("target name", n, MAX_NAME_LEN),
            Self::Candidate(n) if (1..=MAX_CANDIDATE).contains(n) => Ok(()),
            Self::Candidate(n) => Err(format!("candidate {n} is outside 1..={MAX_CANDIDATE}")),
            Self::This => Ok(()),
        }
    }
}

fn check_depth(depth: u32) -> Result<(), String> {
    if (1..=MAX_DEPTH).contains(&depth) {
        Ok(())
    } else {
        Err(format!("depth {depth} is outside 1..={MAX_DEPTH}"))
    }
}

impl Command {
    /// Check the semantic constraints that the type system does not enforce:
    /// non-empty names and tasks, length caps, depth and candidate ranges,
    /// rewind bounds, and at least one filter on `ShowOnly`.
    ///
    /// ```
    /// use codescape_collab::{Command, Target};
    /// assert!(Command::Focus(Target::Name("main".into())).validate().is_ok());
    /// assert!(Command::Callers { target: Target::This, depth: 99 }.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<(), ParseError> {
        let r = match self {
            Self::Focus(t) | Self::Explain(t) => t.check(),
            Self::Callers { target, depth }
            | Self::Callees { target, depth }
            | Self::Affected { target, depth } => target.check().and_then(|()| check_depth(*depth)),
            Self::Path { from, to } => from.check().and_then(|()| to.check()),
            Self::ShowOnly {
                lang: None,
                community: None,
            } => Err("show_only needs a language or a community".to_owned()),
            Self::Rewind { seconds } if !(1..=MAX_REWIND_SECONDS).contains(seconds) => Err(
                format!("rewind of {seconds} s is outside 1..={MAX_REWIND_SECONDS}"),
            ),
            Self::WhatIsDoing { actor: Some(a) } | Self::Follow { actor: a } => {
                check_text("actor", a, MAX_NAME_LEN)
            }
            Self::Ask { actor, task } => check_text("actor", actor, MAX_NAME_LEN)
                .and_then(|()| check_text("task", task, MAX_TASK_LEN)),
            _ => Ok(()),
        };
        r.map_err(|reason| ParseError::Invalid { reason })
    }
}

// ---------------------------------------------------------------------------
// Normalisation
// ---------------------------------------------------------------------------

const NUMBER_WORDS: [&str; 21] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

const ORDINALS: [(&str, u32); 20] = [
    ("first", 1),
    ("second", 2),
    ("third", 3),
    ("fourth", 4),
    ("fifth", 5),
    ("sixth", 6),
    ("seventh", 7),
    ("eighth", 8),
    ("ninth", 9),
    ("tenth", 10),
    ("1st", 1),
    ("2nd", 2),
    ("3rd", 3),
    ("4th", 4),
    ("5th", 5),
    ("6th", 6),
    ("7th", 7),
    ("8th", 8),
    ("9th", 9),
    ("10th", 10),
];

/// Words after which "one" is a pronoun ("this one", "the second one") and
/// stays a word.
const ONE_STAYS_AFTER: [&str; 15] = [
    "this", "that", "the", "which", "any", "each", "every", "no", "last", "next", "other", "same",
    "another", "some", "only",
];

const FILLERS: [&str; 13] = [
    "um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm", "please", "hey", "ok", "okay", "pls",
];

const FILLER_PAIRS: [(&str, &str); 3] = [("can", "you"), ("could", "you"), ("would", "you")];

fn expand_contraction(t: &str) -> Option<&'static [&'static str]> {
    Some(match t {
        "what's" | "whats" => &["what", "is"],
        "who's" => &["who", "is"],
        "it's" => &["it", "is"],
        "that's" => &["that", "is"],
        "where's" => &["where", "is"],
        "how's" => &["how", "is"],
        "what're" => &["what", "are"],
        "let's" => &["let", "us"],
        "i'm" => &["i", "am"],
        _ => return None,
    })
}

fn is_ordinal(t: &str) -> bool {
    ORDINALS.iter().any(|(w, _)| *w == t)
}

enum Piece {
    Word(String),
    Glue(&'static str),
}

/// Normalise a transcript or typed command for [`parse_command`].
///
/// The steps, applied to whole tokens only:
/// * lowercase; drop `? ! , ;` and quotes; drop full stops at the end of a
///   token (typed `main.rs` keeps its dot);
/// * expand common contractions (`what's` becomes `what is`);
/// * remove fillers: `um`, `uh`, `please`, `hey`, `ok`, `can you`,
///   `could you`, and similar;
/// * turn number words `zero`…`twenty` into digits, keeping "one" when it is
///   a pronoun ("this one", "the second one");
/// * turn spoken punctuation into characters joined to the neighbouring words:
///   `dot` becomes `.`, `underscore` becomes `_`, `slash` becomes `/`, and
///   `colon colon` or `double colon` becomes `::`;
/// * collapse whitespace.
///
/// ```
/// use codescape_collab::normalise_spoken;
/// assert_eq!(normalise_spoken("Um, go to graph underscore service dot RS please"), "go to graph_service.rs");
/// assert_eq!(normalise_spoken("show me number two"), "show me number 2");
/// assert_eq!(normalise_spoken("the second one"), "the second one");
/// assert_eq!(normalise_spoken("explain foo colon colon bar"), "explain foo::bar");
/// ```
pub fn normalise_spoken(text: &str) -> String {
    // One pass is not always a fixpoint ("can can you you", a trailing
    // "dot"). Each later pass only removes tokens or characters, so the loop
    // ends; the cap is a belt-and-braces bound.
    let mut out = normalise_once(text);
    for _ in 0..16 {
        let next = normalise_once(&out);
        if next == out {
            break;
        }
        out = next;
    }
    out
}

fn normalise_once(text: &str) -> String {
    let lowered = text.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| match c {
            '?' | '!' | ',' | ';' | '"' | '\u{201c}' | '\u{201d}' | '\u{00bf}' | '\u{00a1}' => ' ',
            '\u{2018}' | '\u{2019}' => '\'',
            c if c.is_whitespace() || c.is_control() => ' ',
            c => c,
        })
        .collect();

    // Tokens, contractions expanded.
    let mut toks: Vec<String> = Vec::new();
    for raw in cleaned.split_whitespace() {
        let t = raw.trim_end_matches('.');
        if t.is_empty() {
            continue;
        }
        match expand_contraction(t) {
            Some(parts) => toks.extend(parts.iter().map(|p| (*p).to_owned())),
            None => toks.push(t.to_owned()),
        }
    }

    // Fillers.
    let mut kept: Vec<String> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        if let Some(next) = toks.get(i + 1) {
            if FILLER_PAIRS.iter().any(|(a, b)| *a == t && *b == next) {
                i += 2;
                continue;
            }
        }
        if !FILLERS.contains(&t) {
            kept.push(toks[i].clone());
        }
        i += 1;
    }

    // Number words, then spoken punctuation.
    let mut pieces: Vec<Piece> = Vec::with_capacity(kept.len());
    let mut i = 0;
    while i < kept.len() {
        let t = kept[i].as_str();
        let next = kept.get(i + 1).map(String::as_str);
        let prev = i.checked_sub(1).map(|j| kept[j].as_str());
        match (t, next) {
            ("colon", Some("colon")) | ("double", Some("colon")) => {
                pieces.push(Piece::Glue("::"));
                i += 2;
                continue;
            }
            ("dot", _) => pieces.push(Piece::Glue(".")),
            ("underscore", _) => pieces.push(Piece::Glue("_")),
            ("slash", _) => pieces.push(Piece::Glue("/")),
            ("one", _) if prev.is_some_and(|p| ONE_STAYS_AFTER.contains(&p) || is_ordinal(p)) => {
                pieces.push(Piece::Word(t.to_owned()));
            }
            _ => match NUMBER_WORDS.iter().position(|w| *w == t) {
                Some(n) => pieces.push(Piece::Word(n.to_string())),
                None => pieces.push(Piece::Word(t.to_owned())),
            },
        }
        i += 1;
    }

    let mut out = String::with_capacity(text.len());
    let mut glue_next = false;
    for p in pieces {
        match p {
            Piece::Word(w) => {
                if !out.is_empty() && !glue_next {
                    out.push(' ');
                }
                out.push_str(&w);
                glue_next = false;
            }
            Piece::Glue(g) => {
                out.push_str(g);
                glue_next = true;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse an utterance (spoken or typed) into a [`Command`].
///
/// The text is normalised first with [`normalise_spoken`], so a transcript
/// and its normalised form parse the same way. The phrasings cover:
///
/// | Command | Examples |
/// |---|---|
/// | `Focus` | focus X, focus on X, go to X, show me X, find X, jump to X, where is X |
/// | `Explain` | explain X, what is X, describe X, tell me about X, what does X do |
/// | `Callers` | callers of X, who calls X, what calls X (+ `depth N` / `N levels`) |
/// | `Callees` | callees of X, what does X call, what is called by X |
/// | `Affected` | what does X affect, affected by X, blast radius of X, dependents of X |
/// | `Path` | path from A to B, how does A reach B, how does A get to B |
/// | `ShowOnly` | show only rust, only show python, show community 3 |
/// | `ShowAll` / `HideTests` | show all, reset filter / hide tests |
/// | `IsolateCommunity` | isolate this community, isolate community 4 |
/// | camera | zoom in, zoom out, overview, zoom all the way out, back, go back |
/// | time | rewind 5 minutes, rewind 30 seconds, replay, live, go live |
/// | presence | what is claude doing, what is everyone doing, follow john, stop following |
/// | delegation | ask claude to fix the build, tell codex to add tests |
/// | `Help` | help, what can I say |
///
/// Targets: "this", "it" and "here" are [`Target::This`]. "number N", a bare
/// "N" and "the second one" are [`Target::Candidate`]. Anything else is a
/// [`Target::Name`], with a leading "the" removed.
///
/// ```
/// use codescape_collab::{parse_command, Command, Target, ParseError};
/// assert_eq!(parse_command("what does this affect").unwrap(),
///            Command::Affected { target: Target::This, depth: 3 });
/// assert_eq!(parse_command("show me the second one").unwrap(), Command::Focus(Target::Candidate(2)));
/// assert!(matches!(parse_command("sing a song"), Err(ParseError::NoMatch { .. })));
/// ```
pub fn parse_command(text: &str) -> Result<Command, ParseError> {
    let n = normalise_spoken(text);
    if n.is_empty() {
        return Err(ParseError::Empty);
    }
    match parse_normalised(&n) {
        Some(cmd) if cmd.validate().is_ok() => Ok(cmd),
        _ => Err(ParseError::NoMatch { normalised: n }),
    }
}

/// Strip the leading word or phrase `w` from `s` on a token boundary. The
/// boundary is either a space, which is consumed, or spoken punctuation
/// glued on by normalisation (`. _ / :`), which is kept. So `"focus.env"`
/// strips `focus` to `.env`, but `"focused"` does not match `focus`.
fn strip_word<'a>(s: &'a str, w: &str) -> Option<&'a str> {
    let w = w.trim_end();
    let rest = s.strip_prefix(w)?;
    if let Some(r) = rest.strip_prefix(' ') {
        Some(r)
    } else if rest.starts_with(['.', '_', '/', ':']) {
        Some(rest)
    } else {
        None
    }
}

/// The rest of `s` after the first of `prefixes` it starts with (on a token
/// boundary, see [`strip_word`]), when that rest is non-empty.
fn after<'a>(s: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|p| strip_word(s, p).map(str::trim).filter(|r| !r.is_empty()))
}

fn number(s: &str) -> Option<u64> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Overflow saturates, so the command then fails range validation.
    Some(s.parse().unwrap_or(u64::MAX))
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

const THIS_WORDS: [&str; 22] = [
    "this",
    "it",
    "here",
    "that",
    "this one",
    "that one",
    "this node",
    "this file",
    "this symbol",
    "this function",
    "this module",
    "this class",
    "this struct",
    "this thing",
    "selection",
    "selected",
    "selected node",
    "current node",
    "current file",
    "current selection",
    "focus",
    "focused node",
];

fn parse_target(s: &str) -> Option<Target> {
    let s = s.trim();
    let s = strip_word(s, "the").unwrap_or(s).trim();
    if s.is_empty() {
        return None;
    }
    if THIS_WORDS.contains(&s) {
        return Some(Target::This);
    }
    let toks: Vec<&str> = s.split(' ').collect();
    let ordinal = |o: &str| {
        ORDINALS
            .iter()
            .find(|(w, _)| *w == o)
            .map(|(_, n)| u64::from(*n))
    };
    let cand = match toks.as_slice() {
        [n] => number(n).or_else(|| ordinal(n)),
        ["number" | "option" | "candidate" | "result" | "no" | "match", n] => number(n),
        [o, "one" | "1" | "result" | "option" | "candidate" | "match"] => ordinal(o),
        _ => None,
    };
    Some(match cand {
        Some(n) => Target::Candidate(small(n)),
        None => Target::Name(s.to_owned()),
    })
}

fn parse_actor(s: &str) -> Option<String> {
    let s = s.trim();
    let s = strip_word(s, "the").unwrap_or(s).trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn parse_lang(s: &str) -> Option<Lang> {
    let mut s = s.trim();
    for suffix in [" only", " files", " code", " sources", " source"] {
        s = s.strip_suffix(suffix).unwrap_or(s);
    }
    Some(match s {
        "rust" | "rs" => Lang::Rust,
        "typescript" | "type script" | "ts" | "tsx" => Lang::TypeScript,
        "javascript" | "java script" | "js" | "jsx" => Lang::JavaScript,
        "python" | "py" => Lang::Python,
        "go" | "golang" | "go lang" => Lang::Go,
        "gdscript" | "gd script" | "g d script" | "godot" | "godot script" => Lang::GdScript,
        "c++" | "cpp" | "c" | "c plus plus" => Lang::Cpp,
        "cuda" => Lang::Cuda,
        "shell" | "bash" | "shell script" | "shell scripts" => Lang::Shell,
        "nix" => Lang::Nix,
        "markdown" => Lang::Markdown,
        _ => return None,
    })
}

/// A community number, optionally prefixed with "number".
fn parse_community(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix("number ").unwrap_or(s);
    number(s).map(small)
}

/// Split a trailing depth phrase ("depth 2", "with depth of 3",
/// "2 levels deep", "up to 3 hops") off `s`, clamping the depth to
/// `1..=MAX_DEPTH`.
fn split_depth(s: &str) -> (&str, Option<u32>) {
    let toks: Vec<&str> = s.split(' ').collect();
    let mut end = toks.len();
    let mut depth = None;
    if end >= 2 {
        if let ("depth", Some(n)) = (toks[end - 2], number(toks[end - 1])) {
            depth = Some(n);
            end -= 2;
        } else if end >= 3 && toks[end - 3] == "depth" && toks[end - 2] == "of" {
            if let Some(n) = number(toks[end - 1]) {
                depth = Some(n);
                end -= 3;
            }
        }
        if depth.is_none() {
            let mut e = end;
            if toks[e - 1] == "deep" {
                e -= 1;
            }
            if e >= 2 && matches!(toks[e - 1], "levels" | "level" | "hops" | "hop") {
                if let Some(n) = number(toks[e - 2]) {
                    depth = Some(n);
                    end = e - 2;
                }
            }
        }
    }
    if depth.is_some() {
        while end > 0 && matches!(toks[end - 1], "with" | "to" | "at" | "up" | "of" | "for") {
            end -= 1;
        }
    }
    let base_len = toks[..end].iter().map(|t| t.len()).sum::<usize>() + end.saturating_sub(1);
    let depth = depth.map(|d| small(d).clamp(1, MAX_DEPTH));
    (&s[..base_len.min(s.len())], depth)
}

/// Wrappers allowed in front of relational queries ("show me the callers of…").
const LEADS: [&str; 16] = [
    "show me the ",
    "show me all ",
    "show me ",
    "show the ",
    "show all ",
    "show ",
    "find the ",
    "find all ",
    "find ",
    "list the ",
    "list all ",
    "list ",
    "give me the ",
    "what is the ",
    "what are the ",
    "highlight ",
];

fn parse_normalised(s: &str) -> Option<Command> {
    rule_fixed(s)
        .or_else(|| rule_time(s))
        .or_else(|| rule_filters(s))
        .or_else(|| rule_presence(s))
        .or_else(|| {
            std::iter::once(s)
                .chain(LEADS.iter().filter_map(|l| s.strip_prefix(l)))
                .find_map(rule_relations)
        })
        .or_else(|| rule_explain(s))
        .or_else(|| rule_focus(s))
}

fn rule_fixed(s: &str) -> Option<Command> {
    Some(match s {
        "help" | "help me" | "what can i say" | "what can i do" | "what can i ask"
        | "list commands" | "show commands" | "commands" | "show help" => Command::Help,
        "stop following" | "unfollow" | "stop follow" | "stop tracking" | "stop watching" => {
            Command::StopFollowing
        }
        "live" | "go live" | "back to live" | "resume live" | "return to live" | "jump to live"
        | "go to live" | "live view" | "show live" | "now" => Command::Live,
        "overview"
        | "show overview"
        | "zoom all the way out"
        | "zoom out all the way"
        | "zoom out fully"
        | "zoom to fit"
        | "fit all"
        | "show the whole graph"
        | "whole graph"
        | "bird's eye view"
        | "birds eye view" => Command::Overview,
        "zoom in" | "zoom closer" | "closer" | "zoom in a bit" | "zoom in more" => Command::ZoomIn,
        "zoom out" | "further" | "zoom back" | "zoom out a bit" | "zoom out more" => {
            Command::ZoomOut
        }
        "back" | "go back" | "previous" | "go to previous" | "previous focus" | "take me back" => {
            Command::Back
        }
        "show all"
        | "show everything"
        | "show me everything"
        | "reset filter"
        | "reset filters"
        | "reset the filter"
        | "reset the filters"
        | "clear filter"
        | "clear filters"
        | "clear the filter"
        | "clear the filters"
        | "remove filter"
        | "remove filters"
        | "remove the filter"
        | "show all languages"
        | "show all communities"
        | "unfilter" => Command::ShowAll,
        "hide tests" | "hide test" | "hide the tests" | "hide test files" | "hide all tests"
        | "hide test code" | "no tests" => Command::HideTests,
        "isolate"
        | "isolate this community"
        | "isolate the community"
        | "isolate community"
        | "isolate this cluster"
        | "isolate this group"
        | "isolate it"
        | "isolate this" => Command::IsolateCommunity(None),
        "replay" => Command::Replay,
        "rewind" => Command::Rewind {
            seconds: DEFAULT_REWIND_SECONDS,
        },
        _ => {
            if s.starts_with("stop following ") {
                Command::StopFollowing
            } else if s.starts_with("replay ") {
                Command::Replay
            } else {
                return None;
            }
        }
    })
}

fn rule_time(s: &str) -> Option<Command> {
    let rest = after(
        s,
        &[
            "rewind by ",
            "rewind ",
            "go back by ",
            "go back ",
            "jump back ",
            "scrub back ",
        ],
    )?;
    let mut toks: Vec<&str> = rest.split(' ').collect();
    if toks.last().is_some_and(|t| matches!(*t, "ago" | "back")) {
        toks.pop();
    }
    let (n, unit) = match toks.as_slice() {
        [n] => (*n, "minutes"),
        [n, unit] => (*n, *unit),
        _ => return None,
    };
    let n = match n {
        "a" | "an" => 1,
        n => number(n)?,
    };
    let unit_s: u64 = match unit {
        "second" | "seconds" | "sec" | "secs" | "s" => 1,
        "minute" | "minutes" | "min" | "mins" | "m" => 60,
        "hour" | "hours" | "hr" | "hrs" | "h" => 3600,
        _ => return None,
    };
    Some(Command::Rewind {
        seconds: n.saturating_mul(unit_s),
    })
}

fn rule_filters(s: &str) -> Option<Command> {
    if let Some(rest) = after(s, &["isolate community ", "isolate cluster "]) {
        return parse_community(rest).map(|c| Command::IsolateCommunity(Some(c)));
    }
    if let Some(rest) = after(
        s,
        &[
            "show only community ",
            "only show community ",
            "show just community ",
            "show community ",
            "filter to community ",
            "filter by community ",
            "filter community ",
        ],
    ) {
        return parse_community(rest).map(|c| Command::ShowOnly {
            lang: None,
            community: Some(c),
        });
    }
    let lang_rest = after(
        s,
        &[
            "show me only ",
            "show only ",
            "only show ",
            "show just ",
            "just show ",
            "filter to ",
            "filter by ",
            "filter ",
        ],
    )
    .or_else(|| {
        s.strip_prefix("show ")
            .and_then(|r| r.strip_suffix(" only"))
    });
    lang_rest
        .and_then(parse_lang)
        .map(|lang| Command::ShowOnly {
            lang: Some(lang),
            community: None,
        })
}

const EVERYONE: [&str; 11] = [
    "everyone",
    "everybody",
    "every one",
    "every agent",
    "all agents",
    "all the agents",
    "agents",
    "the agents",
    "the team",
    "people",
    "all",
];

fn rule_presence(s: &str) -> Option<Command> {
    if matches!(
        s,
        "what is happening" | "what is going on" | "who is here" | "who is working"
    ) {
        return Some(Command::WhatIsDoing { actor: None });
    }
    if let Some(rest) = after(s, &["what is ", "what are "]) {
        let who = [
            " doing right now",
            " doing now",
            " doing",
            " up to",
            " working on",
        ]
        .iter()
        .find_map(|suf| rest.strip_suffix(suf))
        .map(str::trim)
        .filter(|w| !w.is_empty());
        if let Some(who) = who {
            return Some(Command::WhatIsDoing {
                actor: if EVERYONE.contains(&who) {
                    None
                } else {
                    parse_actor(who)
                },
            });
        }
    }
    if let Some(rest) = after(s, &["follow ", "track ", "watch "]) {
        return parse_actor(rest).map(|actor| Command::Follow { actor });
    }
    if let Some(rest) = after(s, &["ask ", "tell "]) {
        if !rest.starts_with("me ") {
            let (who, task) = rest.split_once(" to ")?;
            let task = task.trim();
            return Some(Command::Ask {
                actor: parse_actor(who)?,
                task: (!task.is_empty()).then(|| task.to_owned())?,
            });
        }
    }
    None
}

fn targets(a: &str, b: &str) -> Option<(Target, Target)> {
    Some((parse_target(a)?, parse_target(b)?))
}

fn rule_relations(s: &str) -> Option<Command> {
    // Path queries: no depth.
    if let Some(rest) = after(
        s,
        &[
            "shortest path from ",
            "path from ",
            "route from ",
            "a path from ",
            "how do i get from ",
        ],
    ) {
        let (a, b) = rest.split_once(" to ")?;
        let (from, to) = targets(a, b)?;
        return Some(Command::Path { from, to });
    }
    if let Some(rest) = after(s, &["path between ", "shortest path between "]) {
        let (a, b) = rest.split_once(" and ")?;
        let (from, to) = targets(a, b)?;
        return Some(Command::Path { from, to });
    }
    if let Some(rest) = after(s, &["how does "]) {
        let split = [" reach ", " get to ", " lead to ", " connect to "]
            .iter()
            .find_map(|m| rest.split_once(m));
        if let Some((a, b)) = split {
            let (from, to) = targets(a, b)?;
            return Some(Command::Path { from, to });
        }
    }

    let (base, depth) = split_depth(s);
    let rel = |d: u32| depth.unwrap_or(d);

    if let Some(rest) = after(
        base,
        &[
            "callers of ",
            "caller of ",
            "who calls ",
            "what calls ",
            "who is calling ",
            "everything that calls ",
            "anything that calls ",
            "calls to ",
            "call sites of ",
        ],
    ) {
        return Some(Command::Callers {
            target: parse_target(rest)?,
            depth: rel(DEFAULT_RELATION_DEPTH),
        });
    }
    let callees = after(
        base,
        &[
            "callees of ",
            "callee of ",
            "what is called by ",
            "calls made by ",
            "calls from ",
        ],
    )
    .or_else(|| {
        let r = strip_word(base, "what does")?;
        r.strip_suffix(" call into")
            .or_else(|| r.strip_suffix(" call"))
    });
    if let Some(rest) = callees {
        return Some(Command::Callees {
            target: parse_target(rest)?,
            depth: rel(DEFAULT_RELATION_DEPTH),
        });
    }
    let affected = after(
        base,
        &[
            "what is affected by ",
            "what gets affected by ",
            "affected by ",
            "blast radius of ",
            "blast radius for ",
            "dependents of ",
            "impact of ",
            "what depends on ",
            "who depends on ",
            "what breaks if i change ",
            "what would break if i change ",
            "what breaks if i touch ",
        ],
    )
    .or_else(|| {
        let r = strip_word(base, "what does")?;
        [" affect", " impact", " break"]
            .iter()
            .find_map(|suf| r.strip_suffix(suf))
    });
    if let Some(rest) = affected {
        return Some(Command::Affected {
            target: parse_target(rest)?,
            depth: rel(DEFAULT_AFFECTED_DEPTH),
        });
    }
    None
}

fn rule_explain(s: &str) -> Option<Command> {
    if let Some(r) = strip_word(s, "what does").and_then(|r| r.strip_suffix(" do")) {
        return parse_target(r).map(Command::Explain);
    }
    let rest = after(
        s,
        &[
            "explain ",
            "what is ",
            "what are ",
            "describe ",
            "tell me about ",
            "summarise ",
            "summarize ",
        ],
    )?;
    parse_target(rest).map(Command::Explain)
}

fn rule_focus(s: &str) -> Option<Command> {
    let rest = after(
        s,
        &[
            "focus on ",
            "focus ",
            "go to ",
            "goto ",
            "show me ",
            "find ",
            "jump to ",
            "take me to ",
            "select ",
            "open ",
            "navigate to ",
            "zoom to ",
            "zoom into ",
            "zoom in on ",
            "look at ",
            "where is ",
            "fly to ",
        ],
    )?;
    parse_target(rest).map(Command::Focus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn normalisation_cases() {
        let cases = [
            ("", ""),
            ("   ", ""),
            ("Um, uh... OK", ""),
            ("Hey, can you focus main please?", "focus main"),
            ("could you  go   to\tlib dot rs", "go to lib.rs"),
            ("graph underscore service dot rs", "graph_service.rs"),
            ("src slash main dot rs", "src/main.rs"),
            ("foo double colon bar colon colon baz", "foo::bar::baz"),
            ("dot env", ".env"),
            ("dotenv undo", "dotenv undo"),
            ("rewind five minutes", "rewind 5 minutes"),
            ("rewind twenty seconds", "rewind 20 seconds"),
            ("go to number two", "go to number 2"),
            (
                "callers of one shot depth three",
                "callers of 1 shot depth 3",
            ),
            ("this one", "this one"),
            ("the first one", "the first one"),
            ("What's everyone doing?", "what is everyone doing"),
            ("Focus main.", "focus main"),
            ("explain main.rs.", "explain main.rs"),
            ("Show only TypeScript!", "show only typescript"),
            ("İstanbul", "i̇stanbul"),
            ("v dot two", "v.2"),
        ];
        for (input, want) in cases {
            assert_eq!(normalise_spoken(input), want, "input {input:?}");
        }
    }

    #[test]
    fn depth_suffixes() {
        assert_eq!(
            split_depth("callers of x depth 2"),
            ("callers of x", Some(2))
        );
        assert_eq!(
            split_depth("callers of x with depth of 4"),
            ("callers of x", Some(4))
        );
        assert_eq!(
            split_depth("callers of x 3 levels deep"),
            ("callers of x", Some(3))
        );
        assert_eq!(
            split_depth("callers of x up to 2 hops"),
            ("callers of x", Some(2))
        );
        assert_eq!(
            split_depth("callers of x to depth 99"),
            ("callers of x", Some(MAX_DEPTH))
        );
        assert_eq!(
            split_depth("callers of x depth 0"),
            ("callers of x", Some(1))
        );
        assert_eq!(split_depth("callers of x"), ("callers of x", None));
        assert_eq!(split_depth("deep"), ("deep", None));
        assert_eq!(split_depth("2 levels"), ("", Some(2)));
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(matches!(
            parse_command("focus number 0"),
            Err(ParseError::NoMatch { .. })
        ));
        assert!(matches!(
            parse_command("focus number 99999999999999999999999"),
            Err(ParseError::NoMatch { .. })
        ));
        assert!(matches!(
            parse_command("rewind 0 minutes"),
            Err(ParseError::NoMatch { .. })
        ));
        assert!(matches!(
            parse_command("rewind 99999999999999999999 hours"),
            Err(ParseError::NoMatch { .. })
        ));
        assert!(matches!(
            parse_command("show only klingon"),
            Err(ParseError::NoMatch { .. })
        ));
        assert_eq!(parse_command("um please"), Err(ParseError::Empty));
        assert_eq!(
            parse_command("Sing me a song"),
            Err(ParseError::NoMatch {
                normalised: "sing me a song".into()
            })
        );
    }

    #[test]
    fn validate_messages() {
        let bad = [
            Command::Focus(Target::Name("  ".into())),
            Command::Focus(Target::Name("a\u{7}".into())),
            Command::Focus(Target::Name("x".repeat(MAX_NAME_LEN + 1))),
            Command::Explain(Target::Candidate(0)),
            Command::Affected {
                target: Target::This,
                depth: 0,
            },
            Command::Path {
                from: Target::This,
                to: Target::Candidate(MAX_CANDIDATE + 1),
            },
            Command::ShowOnly {
                lang: None,
                community: None,
            },
            Command::Rewind { seconds: 0 },
            Command::Rewind {
                seconds: MAX_REWIND_SECONDS + 1,
            },
            Command::WhatIsDoing {
                actor: Some(String::new()),
            },
            Command::Follow {
                actor: String::new(),
            },
            Command::Ask {
                actor: "c".into(),
                task: String::new(),
            },
        ];
        for c in bad {
            assert!(
                matches!(c.validate(), Err(ParseError::Invalid { .. })),
                "{c:?}"
            );
        }
        assert!(Command::ShowOnly {
            lang: Some(Lang::Go),
            community: Some(1)
        }
        .validate()
        .is_ok());
    }

    fn grammar_word() -> impl Strategy<Value = String> {
        prop::sample::select(vec![
            "focus",
            "on",
            "go",
            "to",
            "show",
            "me",
            "only",
            "community",
            "what",
            "is",
            "does",
            "affect",
            "call",
            "callers",
            "of",
            "who",
            "calls",
            "depth",
            "levels",
            "deep",
            "path",
            "from",
            "how",
            "reach",
            "rewind",
            "minutes",
            "seconds",
            "hours",
            "ask",
            "tell",
            "follow",
            "stop",
            "following",
            "doing",
            "everyone",
            "number",
            "the",
            "second",
            "one",
            "this",
            "it",
            "dot",
            "underscore",
            "slash",
            "colon",
            "double",
            "isolate",
            "zoom",
            "in",
            "out",
            "back",
            "live",
            "replay",
            "help",
            "rust",
            "a",
            "um",
            "please",
            "can",
            "you",
            "explain",
            "describe",
            "hide",
            "tests",
            "2",
            "0",
            "99999999999999",
            "twenty",
            "#3",
            "",
            " ",
            ".",
            "?",
            "İ",
            "\u{0}",
            "what's",
            "x",
            "main.rs",
        ])
        .prop_map(str::to_owned)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn never_panics_on_any_string(s in any::<String>()) {
            let n = normalise_spoken(&s);
            if let Ok(cmd) = parse_command(&n) {
                prop_assert!(cmd.validate().is_ok());
            }
            let _ = parse_command(&s);
        }

        #[test]
        fn never_panics_on_grammar_soup(words in prop::collection::vec(grammar_word(), 0..12)) {
            let s = words.join(" ");
            let n = normalise_spoken(&s);
            match parse_command(&n) {
                Ok(cmd) => {
                    prop_assert!(cmd.validate().is_ok(), "{cmd:?}");
                    // Normalising again changes nothing that matters.
                    prop_assert_eq!(parse_command(&s), Ok(cmd));
                }
                Err(ParseError::Invalid { .. }) => prop_assert!(false, "grammar never yields Invalid"),
                Err(_) => {}
            }
        }
    }
}
