//! Principal keys (ADR-VG-011): the `did:nostr:<hex>` form a stamp's `by`
//! carries, and its NIP-19 `npub` spelling.
//!
//! A principal is one x-only secp256k1 public key. Stamps written under
//! vocabulary 3 spell it `did:nostr:` plus 64 lowercase hexadecimal characters;
//! stamps written under vocabulary 2 spell the same 32 bytes as a bech32
//! `npub1…` after `human:`. Converting between the two loses nothing, so a page
//! reads as the same principal before and after migration. The bech32 coding is
//! the maintained `bech32` crate's; nothing here implements it.
//!
//! ```
//! use vault_core::principal::{hex_to_npub, npub_to_hex};
//!
//! let hex = "d598abc6edefc37e10523aec4c00f8d9674a93791aa34a1db03bfa9b60b9f246";
//! let npub = "npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj";
//! assert_eq!(npub_to_hex(npub).unwrap(), hex);
//! assert_eq!(hex_to_npub(hex).unwrap(), npub);
//! ```

use bech32::Hrp;

/// The `did:nostr:` prefix of a principal DID.
pub const DID_NOSTR: &str = "did:nostr:";

/// `true` when `s` is exactly 64 lowercase hexadecimal characters.
#[must_use]
pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The 64-hex body of a `did:nostr:` DID, or `None` when `did` is not one or
/// its body is not 64 lowercase hex characters.
#[must_use]
pub fn did_body(did: &str) -> Option<&str> {
    did.strip_prefix(DID_NOSTR).filter(|body| is_hex64(body))
}

/// Decode a bech32 `npub` into the 64-hex key it spells.
///
/// # Errors
/// A message when `npub` is not valid Bech32 (a Bech32m checksum is refused), does not carry the `npub` prefix
/// or does not hold exactly 32 bytes.
pub fn npub_to_hex(npub: &str) -> Result<String, String> {
    // NIP-19 is Bech32 (BIP-173) only. `bech32::decode` accepts a Bech32m
    // checksum too, so check the checksum variant explicitly.
    let checked = bech32::primitives::decode::CheckedHrpstring::new::<bech32::Bech32>(npub)
        .map_err(|e| format!("{npub:?} is not bech32: {e}"))?;
    let hrp = checked.hrp();
    let data: Vec<u8> = checked.byte_iter().collect();
    if hrp.as_str() != "npub" {
        return Err(format!("{npub:?} has the prefix {hrp}, not npub"));
    }
    if data.len() != 32 {
        return Err(format!(
            "{npub:?} holds {} bytes, a public key holds 32",
            data.len()
        ));
    }
    Ok(hex::encode(data))
}

/// Encode a 64-hex key as a bech32 `npub`.
///
/// # Errors
/// A message when `hex` is not 64 lowercase hex characters.
pub fn hex_to_npub(hex: &str) -> Result<String, String> {
    if !is_hex64(hex) {
        return Err(format!("{hex:?} is not 64 lowercase hex characters"));
    }
    let bytes = hex::decode(hex).map_err(|e| e.to_string())?;
    let hrp = Hrp::parse("npub").map_err(|e| e.to_string())?;
    bech32::encode::<bech32::Bech32>(hrp, &bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The npub / hex pairs published in visionGraph's `ontology/attribution.yaml`.
    const PAIRS: [(&str, &str); 7] = [
        (
            "npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj",
            "d598abc6edefc37e10523aec4c00f8d9674a93791aa34a1db03bfa9b60b9f246",
        ),
        (
            "npub126wkhn3fkmf7x7qshycxy9df5ep9hdp65gw3vp6et8hl3wpkvwvs98q46z",
            "569d6bce29b6d3e37810b9306215a9a6425bb43aa21d16075959eff8b8366399",
        ),
        (
            "npub1j3mn56veyldpa7rehyyy8zgtrlswvudw3xqk8apn0h97t7tfjtqqrfxfyk",
            "94773a699927da1ef879b90843890b1fe0e671ae898163f4337dcbe5f96992c0",
        ),
        (
            "npub19knwlhnw6lu0wevfxj3s4h5kjlz2z5w3uxgfs0tvylmksm3mwgyspe82u7",
            "2da6efde6ed7f8f7658934a30ade9697c4a151d1e190983d6c27f7686e3b7209",
        ),
        (
            "npub1mhaqgsg8faj3tryzswgzh6sw2f8ydgd2y3kfts0u5cj9c5dqrnlsaxjndk",
            "ddfa0441074f65158c8283902bea0e524e46a1aa246c95c1fca6245c51a01cff",
        ),
        (
            "npub1ag7s843236euucmycrws9u3e59h6y22k48e3vfnpnuesxcd7nsqq33zm3n",
            "ea3d03d62a8eb3ce6364c0dd02f239a16fa22956a9f31626619f330361be9c00",
        ),
        (
            "npub1phyfu6ez67r60v3h5gcnv2mrsnrfhdwxmqynwq6h23wk9rft070s2cgacn",
            "0dc89e6b22d787a7b237a231362b6384c69bb5c6d809370357545d628d2b7f9f",
        ),
    ];

    #[test]
    fn the_published_pairs_round_trip() {
        for (npub, hex) in PAIRS {
            assert_eq!(npub_to_hex(npub).unwrap(), hex, "{npub}");
            assert_eq!(hex_to_npub(hex).unwrap(), npub, "{hex}");
        }
    }

    /// The NIP-19 specification's own example.
    #[test]
    fn the_nip19_specification_vector_decodes() {
        assert_eq!(
            npub_to_hex("npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg").unwrap(),
            "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e"
        );
    }

    #[test]
    fn malformed_keys_are_rejected_not_guessed() {
        assert!(npub_to_hex("jjohare").is_err());
        assert!(npub_to_hex("nsec10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8q").is_err());
        assert!(hex_to_npub("ABCD").is_err());
        assert_eq!(did_body("did:nostr:not-hex"), None);
        assert_eq!(
            did_body(&format!("did:nostr:{}", PAIRS[0].1)),
            Some(PAIRS[0].1)
        );
        assert_eq!(
            did_body(&format!("did:nostr:{}", PAIRS[0].1.to_uppercase())),
            None
        );
    }

    #[test]
    fn a_bech32m_checksum_is_refused() {
        // Same hrp and 32 bytes as a real npub, encoded with the Bech32m
        // checksum: the `bech32` crate's plain `decode` would accept it.
        let bytes = hex::decode(PAIRS[0].1).unwrap();
        let hrp = Hrp::parse("npub").unwrap();
        let m = bech32::encode::<bech32::Bech32m>(hrp, &bytes).unwrap();
        assert!(
            bech32::decode(&m).is_ok(),
            "the crate's decode accepts Bech32m"
        );
        let err = npub_to_hex(&m).unwrap_err();
        assert!(err.contains("not bech32"), "{err}");
        assert_eq!(npub_to_hex(PAIRS[0].0).unwrap(), PAIRS[0].1);
    }
}
