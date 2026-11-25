//! Corpus history in the bi-temporal provenance graph (VisionFlow ADR-2022).
//!
//! The writer, planner and convergence digest live in
//! [`visionclaw_adapters::corpus_history`] so they build and test without the
//! GPU toolchain and so the offline refit tool can call the production planner.
//! This module re-exports them and adds the one piece that needs the server
//! crate: resolving the corpus-sync agent from the panel key.

pub use visionclaw_adapters::corpus_history::*;

/// Resolve the corpus-sync agent from the kill switch and the panel key.
pub fn resolve_corpus_agent() -> CorpusAgent {
    use nostr_sdk::prelude::{Keys, SecretKey};
    if !enabled_by_env() {
        return CorpusAgent::Disabled;
    }
    match crate::services::acsp::key_file::load_panel_secret() {
        Ok(None) => CorpusAgent::NoIdentity,
        Err(e) => CorpusAgent::Misconfigured(e.to_string()),
        Ok(Some(secret)) => match SecretKey::from_hex(secret.secret_hex()) {
            Ok(sk) => {
                CorpusAgent::Ready(format!("did:nostr:{}", Keys::new(sk).public_key().to_hex()))
            }
            Err(_) => CorpusAgent::Misconfigured("panel key is not a valid secret".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    /// ADR-2021: the activity id is minted by `uri::execution`, so the server's
    /// strict URN parser accepts it.
    #[test]
    fn the_activity_urn_parses_as_a_canonical_execution_id() {
        let urn = corpus_sync_activity_urn(&Utc.with_ymd_and_hms(2026, 11, 3, 9, 0, 0).unwrap());
        match crate::uri::parse(&urn) {
            Ok(crate::uri::ParsedUri::Execution { address }) => {
                assert!(crate::uri::is_content_address(&address))
            }
            other => panic!("{urn} must parse as an execution URN, got {other:?}"),
        }
    }
}
