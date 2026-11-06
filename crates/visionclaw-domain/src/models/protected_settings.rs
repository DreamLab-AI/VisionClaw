use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeys {
    pub perplexity: Option<String>,
    pub openai: Option<String>,
    pub ragflow: Option<String>,
}

/// Custom Debug implementation that masks API key values to prevent
/// accidental secret leakage in logs or error messages.
impl fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn mask(opt: &Option<String>) -> &str {
            match opt {
                Some(s) if s.is_empty() => "<empty>",
                Some(_) => "<redacted>",
                None => "<none>",
            }
        }
        f.debug_struct("ApiKeys")
            .field("perplexity", &mask(&self.perplexity))
            .field("openai", &mask(&self.openai))
            .field("ragflow", &mask(&self.ragflow))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NostrUser {
    pub pubkey: String,
    pub npub: String,
    pub is_power_user: bool,
    pub api_keys: ApiKeys,
    pub last_seen: i64,
    pub session_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ApiKeys ---

    #[test]
    fn api_keys_default_all_none() {
        let k = ApiKeys::default();
        assert!(k.perplexity.is_none());
        assert!(k.openai.is_none());
        assert!(k.ragflow.is_none());
    }

    #[test]
    fn api_keys_debug_masks_values() {
        let k = ApiKeys {
            perplexity: Some("secret-key".to_string()),
            openai: None,
            ragflow: Some(String::new()),
        };
        let debug = format!("{:?}", k);
        assert!(
            !debug.contains("secret-key"),
            "debug must not leak key value"
        );
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("<none>"));
        assert!(debug.contains("<empty>"));
    }

    #[test]
    fn api_keys_serde_roundtrip() {
        let k = ApiKeys {
            perplexity: Some("pk".to_string()),
            openai: None,
            ragflow: Some("rk".to_string()),
        };
        let json = serde_json::to_string(&k).unwrap();
        let back: ApiKeys = serde_json::from_str(&json).unwrap();
        assert_eq!(back.perplexity, Some("pk".to_string()));
        assert!(back.openai.is_none());
        assert_eq!(back.ragflow, Some("rk".to_string()));
    }
}
