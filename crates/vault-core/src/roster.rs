//! The principal roster: who a stamp's `by` names (ADR-VG-011 §2-§3, ruling B8).
//!
//! `ontology/attribution.yaml` lists the keys the estate answers for. Two
//! shapes live in it, and this reader takes both:
//!
//! * `principals:` — the registry. Each entry has a canonical `did`, a `label`,
//!   a `kind` (`human` or `organisation`) and the older keys it answers to
//!   under `also`. Vocabulary 3 stamps carry the canonical DID.
//! * `generators:` — the vocabulary 2 list of generating actors, each with a
//!   `did`, an optional `npub`, a `kind` (`human` or `process`) and an optional
//!   `principal` naming the registry entry it is filed under.
//!
//! The roster decides two things. **Who a stamp is**: `human:<npub>` and
//! `did:nostr:<hex>` spell the same key, and an `also` key resolves to its
//! principal's canonical DID, so a page reads as the same principal before and
//! after migration. **Whether a stamp is human**: a `human:` actor always is; a
//! `did:nostr:` actor is human only when the roster resolves it to a principal
//! of kind human. An organisation, an unknown key and an agent are not, so
//! `verified` (which certifies human review) stays human-only (B8).
//!
//! The reading mirrors ontomark's (`Roster::resolve_principal`,
//! `Roster::is_human_actor`); `tests/ontomark_v3_parity.rs` pins the two together.
//!
//! ```
//! use vault_core::roster::{PrincipalKind, Roster};
//!
//! let roster = Roster::from_yaml_str(
//!     "principals:\n  - did: did:nostr:b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a\n    label: jjohare\n    kind: human\n    also: [did:nostr:d598abc6edefc37e10523aec4c00f8d9674a93791aa34a1db03bfa9b60b9f246]\n",
//! );
//! let old: vault_core::Actor = "human:npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj".parse().unwrap();
//! let new: vault_core::Actor = "did:nostr:b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a".parse().unwrap();
//! assert_eq!(roster.principal_of(&old), roster.principal_of(&new));
//! assert_eq!(roster.resolve(&new).unwrap().kind, PrincipalKind::Human);
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::okf::Actor;
use crate::principal::DID_NOSTR;

/// What a principal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrincipalKind {
    /// A person. The only kind whose `verified` stamp certifies human review.
    Human,
    /// An organisation: it can be answerable, but it is not a reviewer.
    Organisation,
    /// Anything else (a process, or a kind the roster does not state).
    Other,
}

impl PrincipalKind {
    fn parse(s: &str) -> Self {
        match s.trim() {
            "human" => Self::Human,
            "organisation" | "organization" => Self::Organisation,
            _ => Self::Other,
        }
    }
}

/// One `principals:` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// The canonical `did:nostr:<hex>`.
    pub did: String,
    /// Short name, e.g. `jjohare`.
    pub label: String,
    /// What the principal is.
    pub kind: PrincipalKind,
    /// Older keys that answer to the same principal.
    pub also: Vec<String>,
}

/// A key resolved through the roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The canonical `did:nostr:<hex>` of the principal.
    pub canonical: String,
    /// What the principal is.
    pub kind: PrincipalKind,
    /// The principal's label, when the roster gives one.
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
struct GeneratorFacts {
    label: String,
    kind: PrincipalKind,
    principal: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawRoster {
    #[serde(default)]
    principals: Vec<RawPrincipal>,
    #[serde(default)]
    generators: Vec<RawGenerator>,
}

#[derive(Debug, Deserialize)]
struct RawPrincipal {
    did: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    also: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawGenerator {
    did: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    principal: Option<String>,
}

/// The loaded roster.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    principals: Vec<Principal>,
    generators: HashMap<String, GeneratorFacts>,
    /// Under vocabulary 3 the `human:` spelling names nobody (ruling 2): it no
    /// longer certifies. Off by default, which is the vocabulary 2 rule.
    human_prefix_refused: bool,
}

impl Roster {
    /// Load `ontology/attribution.yaml` beneath `repo_root`.
    ///
    /// A file that does not exist is an empty roster (a vault with no roster:
    /// no `did:nostr:` key resolves to a principal, so none certifies). A file
    /// that **exists** but cannot be read or parsed is an error, never an empty
    /// roster: a typo must not silently uncertify, or mis-certify, every page.
    ///
    /// # Errors
    /// A message naming the file when it exists and is unreadable or invalid.
    pub fn load(repo_root: &Path) -> Result<Self, String> {
        let path = repo_root.join("ontology").join("attribution.yaml");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text)
                .map_err(|e| format!("{} is not a valid roster: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    /// Parse a roster; text that is not a YAML mapping gives an empty roster.
    /// For test fixtures and literals: a file on disk goes through
    /// [`Roster::load`], which reports the error.
    #[must_use]
    pub fn from_yaml_str(text: &str) -> Self {
        Self::parse(text).unwrap_or_default()
    }

    /// Parse a roster.
    ///
    /// # Errors
    /// The YAML error when `text` is not a roster mapping.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawRoster = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
        Ok(Self {
            principals: raw
                .principals
                .into_iter()
                .map(|p| Principal {
                    did: p.did,
                    label: p.label,
                    kind: PrincipalKind::parse(&p.kind),
                    also: p.also,
                })
                .collect(),
            generators: raw
                .generators
                .into_iter()
                .map(|g| {
                    (
                        g.did,
                        GeneratorFacts {
                            label: g.label,
                            kind: PrincipalKind::parse(&g.kind),
                            principal: g.principal,
                        },
                    )
                })
                .collect(),
            human_prefix_refused: false,
        })
    }

    /// The roster as read under a vocabulary of `version`. From version 3 a
    /// `human:<anything>` actor is not human (ruling 2: it is an error there,
    /// and m0002 rewrites it to a `did:nostr:`); before that, or with no
    /// vocabulary, the vocabulary 2 prefix rule stands.
    #[must_use]
    pub fn under_vocabulary(mut self, version: Option<u32>) -> Self {
        self.human_prefix_refused = version.is_some_and(|v| v >= 3);
        self
    }

    /// The registry principals, in file order.
    #[must_use]
    pub fn principals(&self) -> &[Principal] {
        &self.principals
    }

    /// Whether the roster holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.principals.is_empty() && self.generators.is_empty()
    }

    /// Resolve a `did:nostr:` DID to its canonical principal and kind. A
    /// registry entry (canonical, or listed under `also`) wins; a generator
    /// filed under a registry principal resolves to that principal but keeps its own kind (an
    /// instrument is never human); any other generator is its own principal with
    /// the kind it states. `None` when the
    /// roster knows nothing of the key.
    #[must_use]
    pub fn resolve_did(&self, did: &str) -> Option<Resolved> {
        if let Some(p) = self
            .principals
            .iter()
            .find(|p| p.did == did || p.also.iter().any(|a| a == did))
        {
            return Some(Resolved {
                canonical: p.did.clone(),
                kind: p.kind,
                label: Some(p.label.clone()).filter(|l| !l.is_empty()),
            });
        }
        let g = self.generators.get(did)?;
        let label = Some(g.label.clone()).filter(|l| !l.is_empty());
        match &g.principal {
            Some(principal) => Some(Resolved {
                canonical: principal.clone(),
                // Only a principal's OWN keys (its canonical key and `also`)
                // carry its kind, and those were matched above. A generator
                // filed under the principal is an instrument acting for it:
                // it keeps the kind it states, so a swarm is never human.
                kind: if g.kind == PrincipalKind::Human {
                    PrincipalKind::Other
                } else {
                    g.kind
                },
                label,
            }),
            None => Some(Resolved {
                canonical: did.to_owned(),
                kind: g.kind,
                label,
            }),
        }
    }

    /// Resolve an actor: a `did:nostr:` key directly, a `human:<npub>` through
    /// the key it decodes to, and a `human:<name>` through the label of the
    /// human generator or principal of that name. Agents and processes name
    /// instruments, not principals, and resolve to `None`.
    #[must_use]
    pub fn resolve(&self, actor: &Actor) -> Option<Resolved> {
        match actor {
            Actor::Did(_) | Actor::Human(_) => {
                if let Some(did) = actor.principal_did() {
                    return self.resolve_did(&did);
                }
                let Actor::Human(name) = actor else {
                    return None;
                };
                // `human:jjohare`: a name, not a key.
                self.principals
                    .iter()
                    .find(|p| &p.label == name && p.kind == PrincipalKind::Human)
                    .map(|p| (p.did.clone(), p.kind, p.label.clone()))
                    .or_else(|| {
                        self.generators
                            .iter()
                            .filter(|(_, g)| &g.label == name && g.kind == PrincipalKind::Human)
                            .min_by_key(|(did, _)| (*did).clone())
                            .map(|(did, g)| (did.clone(), g.kind, g.label.clone()))
                    })
                    .and_then(|(did, _, _)| self.resolve_did(&did))
            }
            Actor::Process { .. } | Actor::Agent { .. } => None,
        }
    }

    /// The canonical principal DID of an actor, if it names one. Where the
    /// roster knows the key, the registry's canonical DID; where it does not,
    /// the key the actor itself spells (a decodable `human:<npub>` or a
    /// `did:nostr:`). `None` for an instrument or an undecodable `human:` name
    /// the roster does not know.
    #[must_use]
    pub fn principal_of(&self, actor: &Actor) -> Option<String> {
        self.resolve(actor)
            .map(|r| r.canonical)
            .or_else(|| actor.principal_did())
    }

    /// Whether `actor` is a person. A `human:` actor always is (the
    /// vocabulary 2 rule, which needs no roster). A `did:nostr:` actor is only
    /// when the roster resolves it to a principal of kind human.
    #[must_use]
    pub fn is_human(&self, actor: &Actor) -> bool {
        match actor {
            Actor::Human(_) => !self.human_prefix_refused,
            Actor::Did(_) => self
                .resolve(actor)
                .is_some_and(|r| r.kind == PrincipalKind::Human),
            Actor::Process { .. } | Actor::Agent { .. } => false,
        }
    }

    /// `did:nostr:` plus `hex`, the form the roster is keyed by.
    #[must_use]
    pub fn did_of_hex(hex: &str) -> String {
        format!("{DID_NOSTR}{hex}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B41: &str = "did:nostr:b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a";
    const D59: &str = "did:nostr:d598abc6edefc37e10523aec4c00f8d9674a93791aa34a1db03bfa9b60b9f246";
    const EA3: &str = "did:nostr:ea3d03d62a8eb3ce6364c0dd02f239a16fa22956a9f31626619f330361be9c00";
    const SWARM: &str =
        "did:nostr:94773a699927da1ef879b90843890b1fe0e671ae898163f4337dcbe5f96992c0";

    fn registry() -> Roster {
        Roster::from_yaml_str(&format!(
            "principals:\n  - did: {B41}\n    label: jjohare\n    kind: human\n    also: [{D59}]\n  - did: {EA3}\n    label: acme\n    kind: organisation\ngenerators:\n  - did: {SWARM}\n    label: enrichment-swarm\n    kind: process\n    principal: {B41}\n"
        ))
    }

    fn actor(s: &str) -> Actor {
        s.parse().unwrap()
    }

    #[test]
    fn a_missing_roster_is_empty_and_a_broken_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("roster-load-{}", std::process::id()));
        let onto = dir.join("ontology");
        std::fs::create_dir_all(&onto).unwrap();
        assert!(Roster::load(&dir).unwrap().is_empty(), "absent file");
        std::fs::write(onto.join("attribution.yaml"), "principals: [unclosed\n").unwrap();
        let err = Roster::load(&dir).unwrap_err();
        assert!(err.contains("attribution.yaml"), "{err}");
        std::fs::write(
            onto.join("attribution.yaml"),
            format!("principals:\n  - did: {B41}\n    kind: human\n"),
        )
        .unwrap();
        assert!(Roster::load(&dir).unwrap().is_human(&actor(B41)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_human_prefix_certifies_under_vocabulary_2_and_not_under_3() {
        let h = actor("human:npub1kst9anything");
        let r = registry();
        assert!(r.clone().under_vocabulary(Some(2)).is_human(&h));
        assert!(r.clone().under_vocabulary(None).is_human(&h));
        assert!(!r.clone().under_vocabulary(Some(3)).is_human(&h));
        // did:nostr keys are unaffected.
        assert!(r.under_vocabulary(Some(3)).is_human(&actor(B41)));
    }

    #[test]
    fn also_keys_resolve_to_the_canonical_principal() {
        let r = registry();
        let via_also = r.resolve(&actor(D59)).unwrap();
        assert_eq!(
            (via_also.canonical.as_str(), via_also.kind),
            (B41, PrincipalKind::Human)
        );
    }

    #[test]
    fn the_old_and_new_spellings_are_one_principal() {
        let r = registry();
        let npub_old = "human:npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj";
        assert_eq!(r.principal_of(&actor(npub_old)).as_deref(), Some(B41));
        assert_eq!(r.principal_of(&actor(D59)).as_deref(), Some(B41));
        assert_eq!(r.principal_of(&actor(B41)).as_deref(), Some(B41));
    }

    #[test]
    fn a_roster_less_reader_still_agrees_on_the_key() {
        let r = Roster::default();
        let npub = actor("human:npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj");
        assert_eq!(r.principal_of(&npub).as_deref(), Some(D59));
        assert_eq!(r.principal_of(&actor(D59)).as_deref(), Some(D59));
    }

    #[test]
    fn human_certification_follows_the_roster_kind() {
        let r = registry();
        assert!(r.is_human(&actor(B41)));
        assert!(
            r.is_human(&actor(D59)),
            "an `also` key of a human principal"
        );
        assert!(
            !r.is_human(&actor(EA3)),
            "an organisation is not a reviewer"
        );
        let unknown = format!("{DID_NOSTR}{}", "0".repeat(63) + "1");
        assert!(!r.is_human(&actor(&unknown)));
        assert!(
            !Roster::default().is_human(&actor(B41)),
            "no roster, no human did"
        );
        assert!(
            r.is_human(&actor("human:npub1kst9anything")),
            "the prefix is enough"
        );
        assert!(!r.is_human(&actor("agent:swarm/1")));
    }

    #[test]
    fn a_generator_filed_under_a_principal_acts_for_it_but_is_never_human() {
        let r = registry();
        let g = r.resolve(&actor(SWARM)).unwrap();
        assert_eq!((g.canonical.as_str(), g.kind), (B41, PrincipalKind::Other));
        assert!(!r.is_human(&actor(SWARM)));
        // The principal's own keys still are.
        assert!(r.is_human(&actor(B41)) && r.is_human(&actor(D59)));
    }

    #[test]
    fn a_generator_with_no_stated_kind_is_not_human_either() {
        let r = Roster::from_yaml_str(&format!(
            "principals:\n  - did: {B41}\n    label: j\n    kind: human\ngenerators:\n  - did: {SWARM}\n    label: s\n    principal: {B41}\n"
        ));
        assert!(!r.is_human(&actor(SWARM)));
    }

    #[test]
    fn a_vocabulary_2_roster_resolves_generators_by_their_stated_kind() {
        let r = Roster::from_yaml_str(&format!(
            "generators:\n  - did: {D59}\n    label: jjohare\n    npub: npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj\n    kind: human\n  - did: {SWARM}\n    label: swarm\n    kind: process\n"
        ));
        assert!(r.is_human(&actor(D59)));
        assert!(!r.is_human(&actor(SWARM)));
        let by_npub =
            actor("human:npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj");
        assert_eq!(r.principal_of(&by_npub).as_deref(), Some(D59));
    }

    #[test]
    fn a_human_name_resolves_through_the_label() {
        let r = registry();
        assert_eq!(
            r.principal_of(&actor("human:jjohare")).as_deref(),
            Some(B41)
        );
        assert_eq!(r.principal_of(&actor("human:nobody")), None);
        assert!(r.is_human(&actor("human:jjohare")));
    }

    #[test]
    fn a_roster_that_is_not_yaml_is_an_error_not_a_silent_empty() {
        assert!(Roster::parse("principals: [unclosed").is_err());
        assert!(Roster::from_yaml_str("principals: [unclosed").is_empty());
    }
}
