//! Service-credential check for the `X-Agent-Key` header (ADR-2093).
//!
//! The comparison uses `subtle::ConstantTimeEq`, so a timing side channel
//! cannot recover the configured key one byte at a time. As with `subtle`'s
//! slice impl, a length mismatch rejects immediately: the credential's length
//! is not secret, its bytes are.

use subtle::ConstantTimeEq;

/// Authorises **only** when a non-empty expected key is configured and the
/// request presents exactly that key. An unset or empty configured key, a
/// missing header, or any mismatch fails closed.
pub fn check_agent_key(expected: Option<&str>, provided: Option<&str>) -> bool {
    match (expected.filter(|s| !s.is_empty()), provided) {
        (Some(key), Some(got)) => key.as_bytes().ct_eq(got.as_bytes()).into(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::check_agent_key;

    #[test]
    fn fails_closed_without_a_configured_key() {
        assert!(!check_agent_key(None, Some("anything")));
        assert!(!check_agent_key(None, None));
        assert!(!check_agent_key(Some(""), Some("")));
        assert!(!check_agent_key(Some(""), Some("x")));
    }

    #[test]
    fn fails_closed_without_a_presented_key() {
        assert!(!check_agent_key(Some("real-key"), None));
    }

    #[test]
    fn equal_unequal_and_length_mismatch() {
        assert!(check_agent_key(Some("real-key"), Some("real-key")));
        assert!(!check_agent_key(Some("real-key"), Some("REAL-KEY")));
        assert!(!check_agent_key(Some("real-key"), Some("real-ke")));
        assert!(!check_agent_key(Some("real-key"), Some("real-keyy")));
        assert!(!check_agent_key(Some("real-key"), Some("")));
    }
}
