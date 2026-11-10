//! Operator switch for the per-relation spring-weight table (ADR-2191).
//!
//! Server-only, environment-driven, read once at first use (restart to
//! change). Two variables:
//!
//! * `VISIONCLAW_RELATION_WEIGHTS`: path to a TOML table. Unset or empty means
//!   the feature is off and the force pass is exactly as before.
//! * `VISIONCLAW_RELATION_WEIGHTS_ALLOW_LEARNED`: `true`/`1`/`yes`/`on` to
//!   accept a table whose provenance is `learned`. Default false (the
//!   ADR-2174 gate).
//!
//! A missing, unreadable, malformed or refused table leaves the feature off
//! and logs why. It never stops the server.

use std::sync::OnceLock;

use log::{error, info, warn};
use visionclaw_domain::models::relation_weights::{LoadOptions, RelationWeightTable};

/// Environment variable naming the table file.
pub const ENV_TABLE: &str = "VISIONCLAW_RELATION_WEIGHTS";
/// Environment variable that admits a learned table.
pub const ENV_ALLOW_LEARNED: &str = "VISIONCLAW_RELATION_WEIGHTS_ALLOW_LEARNED";

static ACTIVE: OnceLock<Option<RelationWeightTable>> = OnceLock::new();

/// The bound table, or `None` when the feature is off. Loaded on first call.
pub fn active() -> Option<&'static RelationWeightTable> {
    ACTIVE
        .get_or_init(|| {
            load(
                std::env::var(ENV_TABLE).ok().as_deref(),
                std::env::var(ENV_ALLOW_LEARNED).ok().as_deref(),
            )
        })
        .as_ref()
}

/// Parse the allow-learned flag. Unrecognised values are false, with a warning.
pub fn parse_allow_learned(raw: Option<&str>) -> bool {
    match raw.map(|s| s.trim().to_ascii_lowercase()) {
        None => false,
        Some(v) if v.is_empty() => false,
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "0" | "false" | "no" | "off") => false,
        Some(v) => {
            warn!("relation weights: {ENV_ALLOW_LEARNED}={v:?} not recognised; treating as false");
            false
        }
    }
}

/// Load a table from `path` (the value of [`ENV_TABLE`]), never panicking.
pub fn load(path: Option<&str>, allow_learned: Option<&str>) -> Option<RelationWeightTable> {
    let path = match path.map(str::trim) {
        None | Some("") => {
            info!("relation weights: off ({ENV_TABLE} unset)");
            return None;
        }
        Some(p) => p,
    };
    let options = LoadOptions {
        allow_learned: parse_allow_learned(allow_learned),
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            warn!("relation weights: off; cannot read {path}: {e}");
            return None;
        }
    };
    match RelationWeightTable::parse(&text, options) {
        Ok(table) => {
            let p = table.provenance();
            info!(
                "relation weights: bound {path} ({} rows, provenance {}{}, registry weight {:?}, allow_learned {})",
                table.len(),
                p.source,
                p.commit.as_deref().map(|c| format!(" @ {c}")).unwrap_or_default(),
                table.registry_weight(),
                options.allow_learned,
            );
            Some(table)
        }
        Err(e) => {
            error!("relation weights: off; {path} refused: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use visionclaw_domain::models::relation_weights::ProvenanceSource;

    fn file(text: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(text.as_bytes()).unwrap();
        f
    }

    #[test]
    fn unset_or_empty_path_is_off() {
        assert!(load(None, None).is_none());
        assert!(load(Some("   "), Some("true")).is_none());
    }

    #[test]
    fn a_missing_file_is_off_not_a_panic() {
        assert!(load(Some("/nonexistent/relation-weights.toml"), None).is_none());
    }

    #[test]
    fn a_malformed_file_is_off_not_a_panic() {
        let f = file(
            "format = \"visionclaw.relation-weights\"\nversion = 1\n[provenance]\nsource = 7\n",
        );
        assert!(load(f.path().to_str(), None).is_none());
        let g = file("\u{0}\u{1} not toml at all [[[");
        assert!(load(g.path().to_str(), None).is_none());
    }

    #[test]
    fn the_shipped_hand_set_example_loads() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/relation-weights/hand-set-two-relations.toml"
        );
        let t = load(Some(path), None).expect("example table loads");
        assert_eq!(t.provenance().source, ProvenanceSource::HandSet);
    }

    const LEARNED: &str = "format = \"visionclaw.relation-weights\"\nversion = 1\n\
        [provenance]\nsource = \"learned\"\nproducer_crate = \"test-producer\"\n\
        commit = \"0123456789abcdef0123456789abcdef01234567\"\n\
        preregistration_result = \"test-fixture#none\"\n\
        [composition]\nregistry_weight = \"ignore\"\n";

    #[test]
    fn a_learned_table_needs_the_explicit_flag() {
        let f = file(LEARNED);
        let p = f.path().to_str();
        assert!(load(p, None).is_none());
        assert!(load(p, Some("false")).is_none());
        assert!(load(p, Some("maybe")).is_none());
        assert!(load(p, Some("TRUE")).is_some());
        assert!(load(p, Some("1")).is_some());
    }

    #[test]
    fn allow_learned_parsing() {
        for v in ["1", "true", "Yes", " on "] {
            assert!(parse_allow_learned(Some(v)), "{v}");
        }
        for v in ["", "0", "false", "off", "no", "2", "enabled"] {
            assert!(!parse_allow_learned(Some(v)), "{v}");
        }
        assert!(!parse_allow_learned(None));
    }
}
