//! ADR-2024: the VisionClaw adapter uses the shared exact-source review contract.
use ontomark_parse::{certification::content_digest, Document};
use vault_core::{okf::Stamp, page::Page};

#[test]
fn stale_review_keeps_historical_protection_and_stamp_metadata() {
    let source = "---\nvalue: 1.00000000000000001\n---\nbody";
    let keys = vec!["value".to_owned()];
    let digest = content_digest(&Document::parse(source), "verified", Some(&keys)).unwrap();
    let yaml =
        format!("by: human:reviewer\nat: 2026-11-15\nkeys: [value]\ncontent_digest: {digest}\n");
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(Stamp::from_yaml(&value).unwrap().to_yaml(), value);
    let stamped = source.replace("\n---\nbody", &format!("\nverified:\n  - by: human:reviewer\n    at: 2026-11-15\n    keys: [value]\n    content_digest: {digest}\n---\nbody"));
    let page = Page::parse("a.md", "a.md", "a", &stamped).unwrap();
    assert_eq!(page.review_summary().current, 1);
    let changed = Page::parse(
        "a.md",
        "a.md",
        "a",
        &stamped.replace("1.00000000000000001", "1.00000000000000002"),
    )
    .unwrap();
    assert_eq!(changed.review_summary().stale, 1);
    assert!(changed.review_assessment().keys.contains("value"));
}
