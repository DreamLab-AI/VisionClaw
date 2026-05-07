// tests/upstream_vectors/nip59_gift_wrap.rs
//! L1 reference vectors: NIP-59 gift-wrap layer-shape conformance.

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn nip59_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip59-gift-wrap.json");
    assert_meta_block(&f, "NIP-59");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 6, "nip59-gift-wrap fixture must have >= 6 vectors");
}

#[test]
fn nip59_seal_layer_must_have_empty_tags() {
    let f = load_fixture("nip59-gift-wrap.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["layer"].as_str() != Some("seal") {
            continue;
        }
        let event = &v["event"];
        let tags = event["tags"].as_array().unwrap_or_else(|| {
            panic!(
                "case '{}': seal event must have tags array",
                v["case"].as_str().unwrap_or("")
            )
        });
        if v["valid"].as_bool() == Some(true) {
            assert!(
                tags.is_empty(),
                "valid seal '{}' MUST have empty tags array (NIP-59 mandate)",
                v["case"].as_str().unwrap_or("")
            );
        } else {
            // Negative cases either have non-empty tags (which is the
            // violation) OR violate something else; we only assert that
            // the negative case is documented as such.
            assert!(
                v["valid"].as_bool() == Some(false),
                "expected valid: false on case '{}'",
                v["case"].as_str().unwrap_or("")
            );
        }
    }
}

#[test]
fn nip59_wrap_layer_must_have_p_tag() {
    let f = load_fixture("nip59-gift-wrap.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["layer"].as_str() != Some("wrap") {
            continue;
        }
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let tags = v["event"]["tags"].as_array().unwrap();
        let has_p = tags
            .iter()
            .any(|t| t.as_array().and_then(|a| a.first()).and_then(|s| s.as_str()) == Some("p"));
        assert!(
            has_p,
            "valid wrap '{}' MUST have a [\"p\", recipient] tag for relay routing",
            v["case"].as_str().unwrap_or("")
        );
    }
}
