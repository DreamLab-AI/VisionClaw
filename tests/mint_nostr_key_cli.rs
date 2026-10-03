//! End-to-end check of `visionclaw-server mint-nostr-key` as a real process:
//! what reaches the operator's terminal is the pubkey and did:nostr only.

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use nostr_sdk::prelude::{Keys, SecretKey, ToBech32};

fn mint(out: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_visionclaw-server"))
        .args(["mint-nostr-key", "--out"])
        .arg(out)
        .env_clear()
        .output()
        .expect("spawn visionclaw-server")
}

#[test]
fn mint_subcommand_prints_pubkey_and_did_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k_broker.key");
    let o = mint(&path);
    let stdout = String::from_utf8(o.stdout).unwrap();
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert!(o.status.success(), "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr must be empty: {stderr}");

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let secret_hex = std::fs::read_to_string(&path).unwrap().trim().to_string();
    let keys = Keys::new(SecretKey::from_hex(&secret_hex).unwrap());
    let pk = keys.public_key().to_hex();
    assert_eq!(stdout, format!("{pk}\ndid:nostr:{pk}\n"));
    for leak in [
        secret_hex.clone(),
        secret_hex.to_uppercase(),
        keys.secret_key().to_bech32().unwrap(),
    ] {
        assert!(!stdout.contains(&leak) && !stderr.contains(&leak));
    }
}

#[test]
fn mint_subcommand_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k_broker.key");
    assert!(mint(&path).status.success());
    let before = std::fs::read(&path).unwrap();
    let o = mint(&path);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).contains("refusing to overwrite"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
