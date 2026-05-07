// tests/upstream_vectors/nip44.rs
//! L1 reference vectors: NIP-44 v2 (ChaCha20-Poly1305 DM).
//!
//! C1 regression guard. The fixture has nested vectors: `valid.get_conversation_key`,
//! `valid.get_message_keys`, `valid.calc_padded_len`, `valid.encrypt_decrypt`,
//! plus `invalid.*`.

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn nip44v2_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip44-v2.json");
    assert_meta_block(&f, "NIP-44");
    // Wrapped shape: vectors.valid.<bucket>[]
    let valid = f["vectors"]["valid"]
        .as_object()
        .expect("nip44-v2 has nested vectors.valid object");
    assert!(
        valid.contains_key("get_conversation_key"),
        "must have get_conversation_key bucket"
    );
    let conv_keys = valid["get_conversation_key"].as_array().unwrap();
    assert!(
        conv_keys.len() >= 30,
        "expected >= 30 get_conversation_key vectors, got {}",
        conv_keys.len()
    );
}

#[test]
#[ignore = "wires into substrate's NIP-44 v2 implementation (`nostr-core` after PRD-009 F26 absorption); fixture validated end-to-end Phase 0 by paulmillr@671a1f0 reference; this is the primary C1 regression guard"]
fn nip44v2_get_conversation_key_matches_reference() {
    let _ = load_fixture("nip44-v2.json");
    // Phase 2: when forum kit is absorbed:
    //   for v in valid.get_conversation_key:
    //       let computed = nostr_core::nip44::v2::get_conversation_key(sec1, pub2);
    //       assert_eq!(hex(computed), v.conversation_key);
}
