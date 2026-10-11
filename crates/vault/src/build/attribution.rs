//! The attribution roster: `ontology/attribution.yaml` maps each generating actor
//! (`human:<npub>`, `agent:<label>/<version>`, `process:<name>/<version>`) to the
//! `did:nostr` it publishes under, so every class can carry `prov:wasAttributedTo`.
//! Public keys only; absent roster means no per-class attribution is emitted.

use std::collections::HashMap;
use std::path::Path;

use vault_core::okf::Actor;

/// One roster entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generator {
    /// `did:nostr:<hex x-only pubkey>`.
    pub did: String,
    /// Short label, e.g. `enrichment-swarm`.
    pub label: String,
    /// bech32 `npub` of the same key, the spelling `human:` actors use.
    pub npub: Option<String>,
}

/// The loaded roster, keyed by every actor spelling that resolves to a generator.
#[derive(Debug, Default, Clone)]
pub struct Roster {
    by_actor: HashMap<String, Generator>,
}

impl Roster {
    /// Load `ontology/attribution.yaml` beneath `repo_root`; an absent file is an empty roster.
    #[must_use]
    pub fn discover(repo_root: &Path) -> Self {
        let path = repo_root.join("ontology").join("attribution.yaml");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_yaml_str(&text),
            Err(_) => Self::default(),
        }
    }

    /// Parse the flat roster. Tolerant of comments and ordering; each `- did:` starts an entry.
    #[must_use]
    pub fn from_yaml_str(text: &str) -> Self {
        let mut gens: Vec<Generator> = Vec::new();
        let mut cur: Option<(String, String, Option<String>, Option<String>)> = None; // did,label,npub,process
        let flush = |cur: &mut Option<(String, String, Option<String>, Option<String>)>, gens: &mut Vec<Generator>, keys: &mut Vec<(String, usize)>| {
            if let Some((did, label, npub, process)) = cur.take() {
                let idx = gens.len();
                gens.push(Generator { did, label: label.clone(), npub: npub.clone() });
                keys.push((format!("agent:{label}/0"), idx));
                if let Some(n) = npub { keys.push((format!("human:{n}"), idx)); }
                if let Some(p) = process { keys.push((p, idx)); }
                keys.push((label, idx));
            }
        };
        let mut keys: Vec<(String, usize)> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end();
            let t = line.trim_start();
            if t.is_empty() || t.starts_with('#') { continue; }
            if let Some(rest) = t.strip_prefix("- did:") {
                flush(&mut cur, &mut gens, &mut keys);
                cur = Some((rest.trim().to_owned(), String::new(), None, None));
                continue;
            }
            let Some((k, v)) = t.split_once(':') else { continue };
            let v = v.trim().trim_matches('"').to_owned();
            if let Some(c) = cur.as_mut() {
                match k.trim() {
                    "label" => c.1 = v,
                    "npub" => c.2 = Some(v),
                    "process" => c.3 = Some(v),
                    _ => {}
                }
            }
        }
        flush(&mut cur, &mut gens, &mut keys);
        let mut by_actor = HashMap::new();
        for (k, idx) in keys { by_actor.insert(k, gens[idx].clone()); }
        Self { by_actor }
    }

    /// The generator an actor resolves to. `agent:<label>/<any-version>` matches on label;
    /// `human:<npub>` on the npub; `process:<name>/<version>` on an explicit `process:` line.
    #[must_use]
    pub fn resolve(&self, actor: &Actor) -> Option<&Generator> {
        match actor {
            Actor::Human(npub) => self.by_actor.get(&format!("human:{npub}")),
            Actor::Agent { producer, .. } => self.by_actor.get(&format!("agent:{producer}/0")).or_else(|| self.by_actor.get(producer)),
            Actor::Process { name, version } => self.by_actor.get(&format!("process:{name}/{version}")),
        }
    }

    /// Whether the roster holds any generator.
    #[must_use]
    pub fn is_empty(&self) -> bool { self.by_actor.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROSTER: &str = "generators:\n  - did: did:nostr:aa\n    label: jjohare\n    npub: npub1x\n    kind: human\n  - did: did:nostr:bb\n    label: enrichment-swarm\n    kind: process\n  - did: did:nostr:cc\n    label: space-corpus-research\n    process: process:space-corpus-research/0.1.0\n";

    #[test]
    fn resolves_each_actor_spelling() {
        let r = Roster::from_yaml_str(ROSTER);
        assert_eq!(r.resolve(&"human:npub1x".parse().unwrap()).unwrap().did, "did:nostr:aa");
        assert_eq!(r.resolve(&"agent:enrichment-swarm/1".parse().unwrap()).unwrap().did, "did:nostr:bb");
        assert_eq!(r.resolve(&"process:space-corpus-research/0.1.0".parse().unwrap()).unwrap().did, "did:nostr:cc");
        assert!(r.resolve(&"process:vault-migrate/1.0".parse().unwrap()).is_none());
    }
}
