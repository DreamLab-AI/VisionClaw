//! Verify the shipped deployment actually resolves every opt-in item.
use visionclaw_jev::config::{self, Item};

#[test]
fn compose_defaults_enable_every_item_locally_without_auto_acceptance() {
    let compose = include_str!("../../../docker-compose.unified.yml");
    let mut defaults = std::collections::HashMap::new();
    for line in compose.lines() {
        let Some((name, value)) = line.trim().split_once(": ") else { continue };
        if !name.starts_with("VISIONCLAW_JEV") { continue; }
        let value = value.trim_matches('"');
        let Some((_, default)) = value.strip_prefix("${").and_then(|s| s.strip_suffix('}')).and_then(|s| s.split_once(":-")) else { continue };
        defaults.insert(name.to_string(), default.to_string());
    }
    let lookup = |key: &str| defaults.get(key).cloned();
    for item in Item::ALL {
        let endpoint = config::resolve(item, &lookup).expect(item.name());
        assert_eq!(endpoint.url, "http://systemone:8097/v1/systemone");
        assert_eq!(endpoint.model, "laya-typed-decisions");
        assert!(endpoint.bearer.is_none());
        assert_eq!(endpoint.timeout.as_millis(), 4000);
    }
    assert_eq!(lookup("VISIONCLAW_JEV_LOCAL").as_deref(), Some("1"));
    assert_eq!(config::auto_min_confidence(&lookup), None);
}
