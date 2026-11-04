//! The licence admission rule (ADR-2170 decision 10).
//!
//! Serving a decimated, SH-stripped variant is redistribution of an
//! adaptation, so a public cache admits only licences that permit both: CC0,
//! CC BY and CC BY-SA, or an explicit grant from the rights holder. `-ND`,
//! `-NC` and assets with no stated licence are refused outside a private dev
//! cache.

use serde::{Deserialize, Serialize};

/// Where admitted assets will be served from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    /// Served to anyone who can reach the server. The default.
    #[default]
    Public,
    /// A developer's own cache, never served beyond them.
    PrivateDev,
}

impl CacheMode {
    /// `SPLAT_CACHE_MODE=private-dev` selects the dev cache; anything else,
    /// including unset, is public.
    pub fn from_env_value(v: Option<&str>) -> Self {
        match v.map(str::trim) {
            Some("private-dev") => Self::PrivateDev,
            _ => Self::Public,
        }
    }
}

/// Why an asset's licence keeps it out of the cache.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LicenceRefusal {
    #[error("no licence is stated")]
    NoLicence,
    #[error("licence {0} forbids adaptations (-ND)")]
    NoDerivatives(String),
    #[error("licence {0} is non-commercial (-NC)")]
    NonCommercial(String),
    #[error("licence {0} is not one that permits redistribution and adaptation")]
    NotPermitted(String),
}

/// SPDX ids admitted to a public cache without a grant.
const PERMITTED: &[&str] = &[
    "CC0-1.0",
    "CC-BY-1.0",
    "CC-BY-2.0",
    "CC-BY-2.5",
    "CC-BY-3.0",
    "CC-BY-4.0",
    "CC-BY-SA-1.0",
    "CC-BY-SA-2.0",
    "CC-BY-SA-2.5",
    "CC-BY-SA-3.0",
    "CC-BY-SA-4.0",
];

/// Decide whether an asset with SPDX id `spdx` (empty for none) and an
/// optional explicit `grant` may enter a cache in `mode`.
///
/// A non-empty grant admits any licence: it records the rights holder's
/// permission to redistribute adaptations. A private dev cache admits
/// everything, because nothing in it is served onward.
pub fn admit_licence(
    spdx: &str,
    grant: Option<&str>,
    mode: CacheMode,
) -> Result<(), LicenceRefusal> {
    if mode == CacheMode::PrivateDev || grant.is_some_and(|g| !g.trim().is_empty()) {
        return Ok(());
    }
    let id = spdx.trim().to_ascii_uppercase();
    if id.is_empty() || id == "NOASSERTION" || id == "NONE" {
        return Err(LicenceRefusal::NoLicence);
    }
    let parts: Vec<&str> = id.split('-').collect();
    if parts.contains(&"ND") {
        return Err(LicenceRefusal::NoDerivatives(spdx.trim().to_string()));
    }
    if parts.contains(&"NC") {
        return Err(LicenceRefusal::NonCommercial(spdx.trim().to_string()));
    }
    // Compare case-insensitively against the canonical SPDX spellings.
    if PERMITTED.iter().any(|p| p.eq_ignore_ascii_case(&id)) {
        Ok(())
    } else {
        Err(LicenceRefusal::NotPermitted(spdx.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CacheMode::*;

    #[test]
    fn cc0_cc_by_and_cc_by_sa_are_admitted() {
        for id in ["CC0-1.0", "CC-BY-4.0", "cc-by-sa-4.0", "CC-BY-3.0"] {
            assert_eq!(admit_licence(id, None, Public), Ok(()), "{id}");
        }
    }

    #[test]
    fn nd_nc_and_missing_licences_are_refused_in_public() {
        assert_eq!(
            admit_licence("CC-BY-ND-4.0", None, Public),
            Err(LicenceRefusal::NoDerivatives("CC-BY-ND-4.0".into()))
        );
        assert_eq!(
            admit_licence("CC-BY-NC-SA-4.0", None, Public),
            Err(LicenceRefusal::NonCommercial("CC-BY-NC-SA-4.0".into()))
        );
        // -NC-ND is refused for the adaptation, the stricter reason.
        assert!(matches!(
            admit_licence("CC-BY-NC-ND-4.0", None, Public),
            Err(LicenceRefusal::NoDerivatives(_))
        ));
        assert_eq!(admit_licence("", None, Public), Err(LicenceRefusal::NoLicence));
        assert_eq!(
            admit_licence("NOASSERTION", None, Public),
            Err(LicenceRefusal::NoLicence)
        );
    }

    #[test]
    fn unknown_licences_are_refused_in_public() {
        assert!(matches!(
            admit_licence("LicenseRef-proprietary", None, Public),
            Err(LicenceRefusal::NotPermitted(_))
        ));
    }

    #[test]
    fn an_explicit_grant_admits_any_licence() {
        assert_eq!(
            admit_licence("CC-BY-ND-4.0", Some("Written permission, 2026-11-02"), Public),
            Ok(())
        );
        // A blank grant is no grant.
        assert!(admit_licence("CC-BY-ND-4.0", Some("  "), Public).is_err());
    }

    #[test]
    fn a_private_dev_cache_admits_everything() {
        for id in ["CC-BY-NC-4.0", "CC-BY-ND-4.0", ""] {
            assert_eq!(admit_licence(id, None, PrivateDev), Ok(()), "{id}");
        }
    }

    #[test]
    fn cache_mode_defaults_to_public() {
        assert_eq!(CacheMode::from_env_value(None), Public);
        assert_eq!(CacheMode::from_env_value(Some("dev")), Public);
        assert_eq!(CacheMode::from_env_value(Some("private-dev")), PrivateDev);
    }
}
