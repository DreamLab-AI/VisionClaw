//! Credential detection for the corpus.
//!
//! A sovereign corpus is published to the open web. A page that carries a live
//! API key publishes it, and `git` keeps it afterwards. This module finds the
//! credential forms the estate actually uses and reports **where** and **what
//! kind** — never the value.
//!
//! That restriction is the whole design. A validation report is written to
//! `api/validation-report.json`, printed in CI logs and pasted into issues; a
//! detector that echoed the secret would publish it through the very channel
//! meant to prevent publication.
//!
//! ```
//! use vault_core::secrets::{scan, CredentialKind};
//!
//! let findings = scan("notes\nkey: sk-abcdefghijklmnopqrstuvwxyz012345\n");
//! assert_eq!(findings.len(), 1);
//! assert_eq!(findings[0].line, 2);
//! assert_eq!(findings[0].kind, CredentialKind::OpenAiKey);
//!
//! // One line can match two patterns, and both are reported: a bearer header
//! // whose value is itself a recognisable key is two facts, not one.
//! let both = scan("Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz012345");
//! assert_eq!(both.len(), 2);
//!
//! // An environment-variable reference is not a secret.
//! assert!(scan("api_key=$OPENAI_API_KEY").is_empty());
//! ```

use std::fmt;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The kind of credential a match names.
///
/// This is the *only* thing reported about the value. Adding a variant that
/// carries the matched text would defeat the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// An OpenAI-style key, `sk-…`.
    #[serde(rename = "openai_key")]
    OpenAiKey,
    /// A Perplexity key, `pplx-…`.
    PerplexityKey,
    /// A Hugging Face token, `hf_…`.
    HuggingFaceToken,
    /// A GitHub token, `ghp_…` or `github_pat_…`.
    GitHubToken,
    /// An AWS access key id, `AKIA…`.
    AwsAccessKeyId,
    /// A Slack token, `xoxb-…` or `xoxp-…`.
    SlackToken,
    /// An HTTP `Authorization: Bearer …` value.
    BearerToken,
    /// A PEM private-key block.
    PrivateKeyBlock,
    /// A Nostr secret key, `nsec1…`.
    NostrSecretKey,
    /// A literal `password=…` assignment.
    PasswordLiteral,
    /// A literal `api_key=…` assignment.
    ApiKeyLiteral,
}

impl CredentialKind {
    /// A short, stable name for reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiKey => "openai_key",
            Self::PerplexityKey => "perplexity_key",
            Self::HuggingFaceToken => "huggingface_token",
            Self::GitHubToken => "github_token",
            Self::AwsAccessKeyId => "aws_access_key_id",
            Self::SlackToken => "slack_token",
            Self::BearerToken => "bearer_token",
            Self::PrivateKeyBlock => "private_key_block",
            Self::NostrSecretKey => "nostr_secret_key",
            Self::PasswordLiteral => "password_literal",
            Self::ApiKeyLiteral => "api_key_literal",
        }
    }
}

impl fmt::Display for CredentialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One detection: a line number and a credential kind.
///
/// Deliberately **no value field**. The line is 1-based, so the report reads
/// `path:line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// 1-based line number within the scanned text.
    pub line: usize,
    /// What kind of credential was found.
    pub kind: CredentialKind,
}

/// The pattern set, compiled once.
///
/// Every token pattern requires a run of credential-shaped characters after
/// its prefix, and none of those character classes contains `$`. That single
/// property is what makes `api_key=$OPENAI_API_KEY`, `Bearer $TOKEN` and
/// `password=${DB_PASS}` non-matches: the documented, correct way to write a
/// secret in a page is a reference to one, and a detector that flagged those
/// would train people to ignore it.
fn patterns() -> &'static [(CredentialKind, Regex)] {
    static P: OnceLock<Vec<(CredentialKind, Regex)>> = OnceLock::new();
    P.get_or_init(|| {
        let compile = |kind, re: &str| (kind, Regex::new(re).expect("static credential regex"));
        vec![
            compile(CredentialKind::OpenAiKey, r"\bsk-[A-Za-z0-9_-]{20,}"),
            compile(CredentialKind::PerplexityKey, r"\bpplx-[A-Za-z0-9]{20,}"),
            compile(CredentialKind::HuggingFaceToken, r"\bhf_[A-Za-z0-9]{20,}"),
            compile(
                CredentialKind::GitHubToken,
                r"\b(?:ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})",
            ),
            compile(CredentialKind::AwsAccessKeyId, r"\bAKIA[0-9A-Z]{16}\b"),
            compile(CredentialKind::SlackToken, r"\bxox[bp]-[A-Za-z0-9-]{10,}"),
            // The scheme name is fixed, so the value class can be permissive
            // without becoming a prose matcher.
            compile(
                CredentialKind::BearerToken,
                r"\bBearer[ \t]+[A-Za-z0-9._~+/=-]{20,}",
            ),
            compile(
                CredentialKind::PrivateKeyBlock,
                r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----",
            ),
            // bech32 excludes `1`, `b`, `i` and `o` from the data part.
            compile(
                CredentialKind::NostrSecretKey,
                r"\bnsec1[02-9ac-hj-np-z]{50,}",
            ),
            // An assignment to a literal. The value must be a run of
            // credential characters — which excludes `$`, the
            // environment-reference escape hatch — and long enough not to be
            // the next English word. "The password= form is the one to avoid"
            // is prose about the hazard, not the hazard; `form` is four
            // characters and does not reach the floor.
            compile(
                CredentialKind::PasswordLiteral,
                r#"(?i)\bpassw(?:or)?d\s*=\s*["']?[A-Za-z0-9._~+/=-]{8,}"#,
            ),
            compile(
                CredentialKind::ApiKeyLiteral,
                r#"(?i)\bapi[-_]?key\s*=\s*["']?[A-Za-z0-9._~+/=-]{12,}"#,
            ),
        ]
    })
}

/// Every credential in `text`, sorted by line then kind, deduplicated.
///
/// A page that pastes the same key twice reports it once per line, because the
/// line is what the person fixing it needs.
#[must_use]
pub fn scan(text: &str) -> Vec<Finding> {
    // Line starts, so a byte offset becomes a line number in one binary search.
    let mut starts: Vec<usize> = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let line_of = |offset: usize| starts.partition_point(|&s| s <= offset);

    let mut out: Vec<Finding> = Vec::new();
    for (kind, regex) in patterns() {
        for m in regex.find_iter(text) {
            out.push(Finding {
                line: line_of(m.start()),
                kind: *kind,
            });
        }
    }
    out.sort_unstable_by_key(|f| (f.line, f.kind));
    out.dedup();
    out
}

/// `true` when `text` carries at least one credential.
///
/// Cheaper to read at a call site that only gates on presence, and it short
/// circuits.
#[must_use]
pub fn has_secret(text: &str) -> bool {
    patterns().iter().any(|(_, re)| re.is_match(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<CredentialKind> {
        scan(text).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn every_declared_form_is_detected() {
        // Synthetic values only: none of these is, or has ever been, live.
        for (text, expected) in [
            (
                "sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                CredentialKind::OpenAiKey,
            ),
            (
                "pplx-0123456789abcdef0123456789",
                CredentialKind::PerplexityKey,
            ),
            (
                "hf_AAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                CredentialKind::HuggingFaceToken,
            ),
            (
                "ghp_0123456789abcdef0123456789abcdef0123",
                CredentialKind::GitHubToken,
            ),
            (
                "github_pat_0123456789abcdef0123456789",
                CredentialKind::GitHubToken,
            ),
            ("AKIAIOSFODNN7EXAMPLE", CredentialKind::AwsAccessKeyId),
            (
                "xoxb-0000000000-0000000000-abcdefghij",
                CredentialKind::SlackToken,
            ),
            (
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz",
                CredentialKind::BearerToken,
            ),
            (
                "-----BEGIN OPENSSH PRIVATE KEY-----",
                CredentialKind::PrivateKeyBlock,
            ),
            (
                "-----BEGIN PRIVATE KEY-----",
                CredentialKind::PrivateKeyBlock,
            ),
            (
                "nsec1acde22222222222222222222222222222222222222222222222222",
                CredentialKind::NostrSecretKey,
            ),
            ("password=hunter2000", CredentialKind::PasswordLiteral),
            ("api_key=abcdefghijkl", CredentialKind::ApiKeyLiteral),
            ("API-KEY = abcdefghijkl", CredentialKind::ApiKeyLiteral),
        ] {
            assert!(
                kinds(text).contains(&expected),
                "{expected} not found in {text:?}: {:?}",
                kinds(text)
            );
            assert!(has_secret(text), "{text:?}");
        }
    }

    #[test]
    fn an_environment_reference_is_never_a_secret() {
        // The documented way to write a credential in a page. Flagging it
        // would make the check noise, and noise is ignored.
        for text in [
            "api_key=$OPENAI_API_KEY",
            "password=${DB_PASSWORD}",
            "API_KEY = $PPLX_KEY",
            "Authorization: Bearer $TOKEN",
            "export OPENAI_API_KEY=$OPENAI_API_KEY",
        ] {
            assert!(scan(text).is_empty(), "{text:?} -> {:?}", kinds(text));
            assert!(!has_secret(text));
        }
    }

    #[test]
    fn ordinary_prose_does_not_match() {
        for text in [
            "The password= form is the one to avoid.",
            "Set your api_key= from the environment.",
            "sk-",
            "Bearer tokens are short-lived.",
            "We use an AKIA-prefixed identifier.",
            "hf_ is the Hugging Face prefix.",
            "An nsec1 value is a Nostr secret key.",
        ] {
            assert!(scan(text).is_empty(), "{text:?} -> {:?}", kinds(text));
        }
    }

    #[test]
    fn the_line_number_is_one_based_and_the_value_is_never_carried() {
        let text = "line one\nline two\nsk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
        let found = scan(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        // The only public fields are the line and the kind; a serialised
        // finding therefore cannot leak the credential.
        let json = serde_json::to_string(&found[0]).unwrap();
        assert_eq!(json, r#"{"line":3,"kind":"openai_key"}"#);
        assert!(!json.contains("AAAA"));
    }

    #[test]
    fn two_credentials_on_two_lines_are_both_reported() {
        let text = "api_key=abcdefghijkl\npassword=hunter2000\n";
        assert_eq!(
            scan(text),
            vec![
                Finding {
                    line: 1,
                    kind: CredentialKind::ApiKeyLiteral
                },
                Finding {
                    line: 2,
                    kind: CredentialKind::PasswordLiteral
                },
            ]
        );
    }
}
