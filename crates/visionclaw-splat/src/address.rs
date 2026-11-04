//! SHA-256 and the ADR-2023 content address.

use sha2::{Digest, Sha256};
use std::io::Read;

/// Prefix of an ADR-2023 content address.
pub const CONTENT_ADDR_PREFIX: &str = "sha256-12-";

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Lowercase hex SHA-256 of everything `reader` yields, streamed in 1 MiB
/// blocks so a 60 MB variant is never held twice.
pub fn sha256_hex_reader(mut reader: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// `sha256-12-<12 lowercase hex>` content address of `bytes`, byte-for-byte
/// the server's `uri::content_address`.
pub fn content_address(bytes: &[u8]) -> String {
    address_of_hex(&sha256_hex(bytes))
}

/// The content address for a full lowercase hex SHA-256.
///
/// # Panics
/// If `sha256` is shorter than 12 characters; callers pass validated hashes.
pub fn address_of_hex(sha256: &str) -> String {
    format!("{CONTENT_ADDR_PREFIX}{}", &sha256[..12])
}

/// Whether `s` is a well-formed content address. Used to refuse path
/// traversal before a cache path is ever built from request input.
pub fn is_content_address(s: &str) -> bool {
    s.strip_prefix(CONTENT_ADDR_PREFIX)
        .is_some_and(|h| h.len() == 12 && is_lower_hex(h))
}

/// Whether `s` is a full lowercase hex SHA-256 (64 characters).
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && is_lower_hex(s)
}

fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex(bytes: &[u8]) -> String {
    const NIBBLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(NIBBLE[(b >> 4) as usize] as char);
        out.push(NIBBLE[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_published_vector() {
        // FIPS 180-2 "abc" vector.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn content_address_is_the_first_twelve_hex() {
        assert_eq!(content_address(b"abc"), "sha256-12-ba7816bf8f01");
        assert!(is_content_address("sha256-12-ba7816bf8f01"));
    }

    #[test]
    fn streamed_hash_equals_one_shot_hash() {
        let data = vec![7u8; (1 << 20) + 13];
        assert_eq!(sha256_hex_reader(&data[..]).unwrap(), sha256_hex(&data));
    }

    #[test]
    fn malformed_addresses_are_refused() {
        for bad in [
            "sha256-12-BA7816BF8F01",
            "sha256-12-ba7816bf8f0",
            "sha256-12-../../etc/pa",
            "ba7816bf8f01",
            "",
        ] {
            assert!(!is_content_address(bad), "{bad}");
        }
    }
}
