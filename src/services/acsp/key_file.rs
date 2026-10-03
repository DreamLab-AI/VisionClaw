//! VisionClaw's own governance signing key (K_broker): minting and loading.
//!
//! The ACSP producer (kinds 31400-31405), the decision-projection client and
//! the governed voice loop all sign as one panel identity. That identity must
//! belong to VisionClaw alone — never a copy of another service's key — so it
//! is minted here and read through exactly one loader.
//!
//! - [`mint_key_file`] / [`run_mint_cli`] — `visionclaw-server mint-nostr-key
//!   --out <path>`: generates a key with `nostr_sdk::Keys::generate()`, writes
//!   the hex secret to a *new* file at mode 0600 (`O_CREAT|O_EXCL`; an existing
//!   path is refused) and prints only the x-only public key and its did:nostr.
//! - [`load_panel_secret`] — the shared loader. Precedence, first set wins:
//!   1. `ACSP_PANEL_NOSTR_KEY_FILE` (path to a 0600 hex secret file)
//!   2. `VISIONCLAW_NOSTR_KEY_FILE`
//!   3. `ACSP_PANEL_NOSTR_PRIVKEY`  (hex secret in the environment)
//!   4. `VISIONCLAW_NOSTR_PRIVKEY`
//!
//! A configured file var that cannot be used (missing, not a regular file,
//! readable by group/other, not a valid hex secret) is an error, never a
//! silent fall-through to the environment value: an operator who pointed at a
//! file meant that key, not whatever is still in `.env`.
//!
//! No cryptography lives here: generation, encoding and parsing are
//! `nostr-sdk`'s. Secret material never reaches a log line, an error message or
//! a `Debug` rendering.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use nostr_sdk::prelude::{Keys, SecretKey};

/// Subcommand name dispatched from `main` before any server start-up.
pub const MINT_SUBCOMMAND: &str = "mint-nostr-key";

/// Key-file variables, highest precedence first.
pub const PANEL_KEY_FILE_VARS: [&str; 2] =
    ["ACSP_PANEL_NOSTR_KEY_FILE", "VISIONCLAW_NOSTR_KEY_FILE"];

/// Inline secret variables, consulted only when no key-file variable is set.
pub const PANEL_KEY_ENV_VARS: [&str; 2] = ["ACSP_PANEL_NOSTR_PRIVKEY", "VISIONCLAW_NOSTR_PRIVKEY"];

/// Upper bound on a key file's size; a hex secret plus newline is 65 bytes.
const MAX_KEY_FILE_BYTES: u64 = 4096;

/// Public half of a freshly minted key — the only thing the mint prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedKey {
    /// x-only (BIP-340) public key, 64 lowercase hex chars.
    pub pubkey_hex: String,
    /// `did:nostr:<pubkey_hex>`.
    pub did: String,
}

/// Generate a key and write its hex secret to `path`, which must not exist.
///
/// The file is created with `O_CREAT|O_EXCL` at mode 0600 (re-asserted with
/// `fchmod` so a restrictive umask cannot leave it at 0400/0000 and a
/// permissive one cannot widen it), written, and fsynced. On a write failure
/// the partial file is removed.
pub fn mint_key_file(path: &Path) -> io::Result<MintedKey> {
    let keys = Keys::generate();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let written = (|| {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(keys.secret_key().to_secret_hex().as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    let pubkey_hex = keys.public_key().to_hex();
    Ok(MintedKey {
        did: format!("did:nostr:{pubkey_hex}"),
        pubkey_hex,
    })
}

const MINT_USAGE: &str = "usage: visionclaw-server mint-nostr-key --out <path>\n\
\n\
Mints VisionClaw's own governance signing key. Writes the hex secret to <path>\n\
(new file, mode 0600; an existing path is refused) and prints the x-only\n\
public key and its did:nostr. The secret is never printed. Point\n\
ACSP_PANEL_NOSTR_KEY_FILE at <path> to sign with it.\n";

/// Run the `mint-nostr-key` subcommand. `args` excludes the program name and
/// the subcommand itself. Returns the process exit code: 0 minted, 1 refused
/// or failed, 2 usage error.
///
/// On success stdout carries exactly two lines — the pubkey hex, then the
/// did:nostr — and stderr is empty.
pub fn run_mint_cli(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut out_path: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                let _ = out.write_all(MINT_USAGE.as_bytes());
                return 0;
            }
            "--out" => match it.next() {
                Some(p) if !p.is_empty() => out_path = Some(PathBuf::from(p)),
                _ => {
                    let _ = writeln!(err, "mint-nostr-key: --out needs a path\n\n{MINT_USAGE}");
                    return 2;
                }
            },
            other if other.starts_with("--out=") && other.len() > "--out=".len() => {
                out_path = Some(PathBuf::from(&other["--out=".len()..]));
            }
            other => {
                let _ = writeln!(
                    err,
                    "mint-nostr-key: unexpected argument {other:?}\n\n{MINT_USAGE}"
                );
                return 2;
            }
        }
    }
    let Some(path) = out_path else {
        let _ = write!(
            err,
            "mint-nostr-key: --out <path> is required\n\n{MINT_USAGE}"
        );
        return 2;
    };
    match mint_key_file(&path) {
        Ok(minted) => {
            let _ = writeln!(out, "{}\n{}", minted.pubkey_hex, minted.did);
            0
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = writeln!(
                err,
                "mint-nostr-key: refusing to overwrite existing {} — a governance key is never replaced in place",
                path.display()
            );
            1
        }
        Err(e) => {
            let _ = writeln!(err, "mint-nostr-key: cannot create {}: {e}", path.display());
            1
        }
    }
}

/// If `argv` (program name first) invokes [`MINT_SUBCOMMAND`], run it on the
/// real stdout/stderr and return its exit code; otherwise `None` and the
/// server starts normally.
pub fn dispatch_cli(argv: &[String]) -> Option<i32> {
    if argv.get(1).map(String::as_str) != Some(MINT_SUBCOMMAND) {
        return None;
    }
    let stdout = io::stdout();
    let stderr = io::stderr();
    Some(run_mint_cli(
        &argv[2..],
        &mut stdout.lock(),
        &mut stderr.lock(),
    ))
}

/// Where the panel secret came from (never the secret itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelKeySource {
    /// Read from the file named by `var`.
    File {
        /// The variable that named the file.
        var: &'static str,
        /// The file path.
        path: PathBuf,
    },
    /// Taken inline from the environment variable `var`.
    Env {
        /// The variable that held the secret.
        var: &'static str,
    },
}

impl fmt::Display for PanelKeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { var, path } => write!(f, "{var}={}", path.display()),
            Self::Env { var } => write!(f, "{var} (inline env)"),
        }
    }
}

/// The resolved panel signing secret. `Debug` is redacted.
pub struct PanelSecret {
    secret_hex: String,
    source: PanelKeySource,
}

impl PanelSecret {
    /// The hex secret, for `SecretKey::from_hex` / `AcspClient::connect`.
    pub fn secret_hex(&self) -> &str {
        &self.secret_hex
    }

    /// Where it was loaded from.
    pub fn source(&self) -> &PanelKeySource {
        &self.source
    }

    /// Consume into the hex secret (for actors that own the string).
    pub fn into_secret_hex(self) -> String {
        self.secret_hex
    }
}

impl fmt::Debug for PanelSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PanelSecret")
            .field("secret_hex", &"<redacted>")
            .field("source", &self.source)
            .finish()
    }
}

/// Why a configured key file could not be used. Messages name the variable
/// and path, never the contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelKeyError {
    /// The file could not be opened or read.
    Unreadable {
        /// Variable naming the file.
        var: &'static str,
        /// The path.
        path: PathBuf,
        /// The I/O error, rendered.
        reason: String,
    },
    /// The path is not a regular file.
    NotRegularFile {
        /// Variable naming the file.
        var: &'static str,
        /// The path.
        path: PathBuf,
    },
    /// Group or other has any permission bit on the file.
    TooPermissive {
        /// Variable naming the file.
        var: &'static str,
        /// The path.
        path: PathBuf,
        /// The file's permission bits (`mode & 0o777`).
        mode: u32,
    },
    /// The file is larger than any key file could be.
    TooLarge {
        /// Variable naming the file.
        var: &'static str,
        /// The path.
        path: PathBuf,
    },
    /// The contents are not a 64-hex secp256k1 secret.
    InvalidSecret {
        /// Variable naming the file.
        var: &'static str,
        /// The path.
        path: PathBuf,
    },
}

impl fmt::Display for PanelKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { var, path, reason } => {
                write!(f, "{var}={}: cannot read key file: {reason}", path.display())
            }
            Self::NotRegularFile { var, path } => {
                write!(f, "{var}={}: not a regular file", path.display())
            }
            Self::TooPermissive { var, path, mode } => write!(
                f,
                "{var}={}: key file mode is {mode:04o}, refusing — it must not be readable by group or other (chmod 600 {})",
                path.display(),
                path.display()
            ),
            Self::TooLarge { var, path } => write!(
                f,
                "{var}={}: key file exceeds {MAX_KEY_FILE_BYTES} bytes; not a key file",
                path.display()
            ),
            Self::InvalidSecret { var, path } => write!(
                f,
                "{var}={}: contents are not a 64-hex Nostr secret key (mint one with `visionclaw-server mint-nostr-key --out <path>`)",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PanelKeyError {}

/// Resolve the panel signing secret from the process environment.
///
/// `Ok(None)` means nothing is configured (the governed paths stay off, as
/// before). See the module docs for precedence.
pub fn load_panel_secret() -> Result<Option<PanelSecret>, PanelKeyError> {
    load_panel_secret_with(|k| std::env::var(k).ok())
}

/// [`load_panel_secret`] over an arbitrary variable lookup (tests inject one
/// rather than mutating the process environment). Empty values count as
/// unset.
pub fn load_panel_secret_with(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<PanelSecret>, PanelKeyError> {
    let get = |k: &str| lookup(k).filter(|v| !v.is_empty());
    for var in PANEL_KEY_FILE_VARS {
        if let Some(path) = get(var) {
            let path = PathBuf::from(path);
            let secret_hex = read_key_file(var, &path)?;
            return Ok(Some(PanelSecret {
                secret_hex,
                source: PanelKeySource::File { var, path },
            }));
        }
    }
    for var in PANEL_KEY_ENV_VARS {
        if let Some(secret_hex) = get(var) {
            return Ok(Some(PanelSecret {
                secret_hex,
                source: PanelKeySource::Env { var },
            }));
        }
    }
    Ok(None)
}

fn read_key_file(var: &'static str, path: &Path) -> Result<String, PanelKeyError> {
    let unreadable = |e: io::Error| PanelKeyError::Unreadable {
        var,
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    // Check the metadata of the handle we read from, not a separate stat of
    // the path, so the checked file is the read file. Symlinks are followed
    // (container secret mounts use them); the target's mode is what counts.
    let mut file = File::open(path).map_err(unreadable)?;
    let meta = file.metadata().map_err(unreadable)?;
    if !meta.is_file() {
        return Err(PanelKeyError::NotRegularFile {
            var,
            path: path.to_path_buf(),
        });
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(PanelKeyError::TooPermissive {
            var,
            path: path.to_path_buf(),
            mode,
        });
    }
    if meta.len() > MAX_KEY_FILE_BYTES {
        return Err(PanelKeyError::TooLarge {
            var,
            path: path.to_path_buf(),
        });
    }
    let mut contents = String::new();
    (&mut file)
        .take(MAX_KEY_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|_| PanelKeyError::InvalidSecret {
            var,
            path: path.to_path_buf(),
        })?;
    let secret_hex = contents.trim();
    if SecretKey::from_hex(secret_hex).is_err() {
        return Err(PanelKeyError::InvalidSecret {
            var,
            path: path.to_path_buf(),
        });
    }
    Ok(secret_hex.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::prelude::ToBech32;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn key_file(dir: &Path, name: &str, mode: u32) -> (PathBuf, Keys) {
        let keys = Keys::generate();
        let path = dir.join(name);
        std::fs::write(&path, format!("{}\n", keys.secret_key().to_secret_hex())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        (path, keys)
    }

    fn run(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_mint_cli(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    // ── mint ────────────────────────────────────────────────────────────

    #[test]
    fn mint_creates_file_0600_not_world_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k_broker.key");
        mint_key_file(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode {mode:04o}");
        assert_eq!(mode & 0o077, 0, "group/other must have no bits");
    }

    #[test]
    fn mint_refuses_to_overwrite_and_leaves_existing_file_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k_broker.key");
        std::fs::write(&path, "existing").unwrap();
        let err = mint_key_file(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");

        let (code, out, stderr) = run(&["--out", path.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert!(out.is_empty());
        assert!(stderr.contains("refusing to overwrite"), "{stderr}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");
    }

    #[test]
    fn mint_refuses_a_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        let link = dir.path().join("k_broker.key");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            mint_key_file(&link).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(!target.exists(), "O_EXCL must not follow the link");
    }

    #[test]
    fn printed_pubkey_matches_the_key_in_the_file_and_secret_is_never_printed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k_broker.key");
        let (code, out, err) = run(&["--out", path.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {err}");
        assert!(err.is_empty(), "stderr must be empty on success: {err}");

        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "exactly pubkey + did: {out:?}");
        let secret_hex = std::fs::read_to_string(&path).unwrap().trim().to_string();
        let derived = Keys::new(SecretKey::from_hex(&secret_hex).unwrap());
        assert_eq!(lines[0], derived.public_key().to_hex());
        assert_eq!(
            lines[1],
            format!("did:nostr:{}", derived.public_key().to_hex())
        );

        let secret = derived.secret_key();
        for rendering in [
            secret_hex.clone(),
            secret_hex.to_uppercase(),
            secret.to_bech32().unwrap(),
        ] {
            assert!(!out.contains(&rendering), "secret leaked to stdout");
            assert!(!err.contains(&rendering), "secret leaked to stderr");
        }
    }

    #[test]
    fn mint_cli_accepts_out_equals_and_rejects_bad_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.key");
        let (code, _, _) = run(&[&format!("--out={}", path.display())]);
        assert_eq!(code, 0);
        assert!(path.exists());

        assert_eq!(run(&[]).0, 2);
        assert_eq!(run(&["--out"]).0, 2);
        assert_eq!(run(&["--force", "--out", "x"]).0, 2);
        let (code, out, _) = run(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("mint-nostr-key --out <path>"));
    }

    #[test]
    fn dispatch_only_claims_the_mint_subcommand() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dispatch_cli(&argv(&["visionclaw-server"])), None);
        assert_eq!(
            dispatch_cli(&argv(&["visionclaw-server", "--allow-skip-auth"])),
            None
        );
    }

    // ── loader precedence ───────────────────────────────────────────────

    #[test]
    fn nothing_configured_is_none() {
        assert!(load_panel_secret_with(env(&[])).unwrap().is_none());
        assert!(
            load_panel_secret_with(env(&[("ACSP_PANEL_NOSTR_PRIVKEY", "")]))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn env_path_unchanged_when_no_file_var_is_set() {
        let s = load_panel_secret_with(env(&[
            ("ACSP_PANEL_NOSTR_PRIVKEY", "aa"),
            ("VISIONCLAW_NOSTR_PRIVKEY", "bb"),
        ]))
        .unwrap()
        .unwrap();
        // Passed through verbatim, as the old `env::var(..).or_else(..)` did.
        assert_eq!(s.secret_hex(), "aa");
        assert_eq!(
            s.source(),
            &PanelKeySource::Env {
                var: "ACSP_PANEL_NOSTR_PRIVKEY"
            }
        );

        let s = load_panel_secret_with(env(&[("VISIONCLAW_NOSTR_PRIVKEY", "bb")]))
            .unwrap()
            .unwrap();
        assert_eq!(s.secret_hex(), "bb");
        assert_eq!(
            s.source(),
            &PanelKeySource::Env {
                var: "VISIONCLAW_NOSTR_PRIVKEY"
            }
        );
    }

    #[test]
    fn a_key_file_beats_both_env_vars() {
        let dir = tempfile::tempdir().unwrap();
        let (path, keys) = key_file(dir.path(), "vc.key", 0o600);
        let s = load_panel_secret_with(env(&[
            ("VISIONCLAW_NOSTR_KEY_FILE", path.to_str().unwrap()),
            ("ACSP_PANEL_NOSTR_PRIVKEY", "copied-house-key"),
            ("VISIONCLAW_NOSTR_PRIVKEY", "copied-house-key"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(s.secret_hex(), keys.secret_key().to_secret_hex());
        assert_eq!(
            s.source(),
            &PanelKeySource::File {
                var: "VISIONCLAW_NOSTR_KEY_FILE",
                path
            }
        );
    }

    #[test]
    fn acsp_key_file_beats_visionclaw_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let (acsp, acsp_keys) = key_file(dir.path(), "acsp.key", 0o600);
        let (vc, _) = key_file(dir.path(), "vc.key", 0o600);
        let s = load_panel_secret_with(env(&[
            ("ACSP_PANEL_NOSTR_KEY_FILE", acsp.to_str().unwrap()),
            ("VISIONCLAW_NOSTR_KEY_FILE", vc.to_str().unwrap()),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(s.secret_hex(), acsp_keys.secret_key().to_secret_hex());
        assert!(matches!(
            s.source(),
            PanelKeySource::File {
                var: "ACSP_PANEL_NOSTR_KEY_FILE",
                ..
            }
        ));
    }

    #[test]
    fn a_0644_key_file_is_refused_with_a_clear_error_and_no_env_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let (path, keys) = key_file(dir.path(), "loose.key", 0o644);
        let err = load_panel_secret_with(env(&[
            ("ACSP_PANEL_NOSTR_KEY_FILE", path.to_str().unwrap()),
            ("ACSP_PANEL_NOSTR_PRIVKEY", "fallback-must-not-be-used"),
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            PanelKeyError::TooPermissive {
                var: "ACSP_PANEL_NOSTR_KEY_FILE",
                path: path.clone(),
                mode: 0o644
            }
        );
        let msg = err.to_string();
        assert!(msg.contains("ACSP_PANEL_NOSTR_KEY_FILE"), "{msg}");
        assert!(msg.contains("0644") && msg.contains("chmod 600"), "{msg}");
        assert!(!msg.contains(&keys.secret_key().to_secret_hex()));
    }

    #[test]
    fn group_readable_and_group_writable_files_are_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        for mode in [0o640, 0o620, 0o604, 0o660] {
            let (path, _) = key_file(dir.path(), &format!("k{mode:o}.key"), mode);
            let err = load_panel_secret_with(env(&[(
                "ACSP_PANEL_NOSTR_KEY_FILE",
                path.to_str().unwrap(),
            )]))
            .unwrap_err();
            assert!(
                matches!(err, PanelKeyError::TooPermissive { .. }),
                "{mode:o}"
            );
        }
        // 0400 (read-only owner) is fine.
        let (path, _) = key_file(dir.path(), "ro.key", 0o400);
        assert!(load_panel_secret_with(env(&[(
            "ACSP_PANEL_NOSTR_KEY_FILE",
            path.to_str().unwrap()
        )]))
        .unwrap()
        .is_some());
    }

    #[test]
    fn missing_directory_and_garbage_files_are_errors_without_contents() {
        let dir = tempfile::tempdir().unwrap();
        let lookup = |p: &Path| env(&[("ACSP_PANEL_NOSTR_KEY_FILE", p.to_str().unwrap())]);

        let missing = dir.path().join("absent.key");
        assert!(matches!(
            load_panel_secret_with(lookup(&missing)).unwrap_err(),
            PanelKeyError::Unreadable { .. }
        ));

        assert!(matches!(
            load_panel_secret_with(lookup(dir.path())).unwrap_err(),
            PanelKeyError::NotRegularFile { .. }
        ));

        let garbage = dir.path().join("garbage.key");
        std::fs::write(&garbage, "not-a-key-SENTINEL\n").unwrap();
        std::fs::set_permissions(&garbage, std::fs::Permissions::from_mode(0o600)).unwrap();
        let err = load_panel_secret_with(lookup(&garbage)).unwrap_err();
        assert!(matches!(err, PanelKeyError::InvalidSecret { .. }));
        assert!(!err.to_string().contains("SENTINEL"));
    }

    #[test]
    fn a_minted_file_loads_and_debug_is_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k_broker.key");
        let minted = mint_key_file(&path).unwrap();
        let s = load_panel_secret_with(env(&[(
            "ACSP_PANEL_NOSTR_KEY_FILE",
            path.to_str().unwrap(),
        )]))
        .unwrap()
        .unwrap();
        let keys = Keys::new(SecretKey::from_hex(s.secret_hex()).unwrap());
        assert_eq!(keys.public_key().to_hex(), minted.pubkey_hex);
        let dbg = format!("{s:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains(s.secret_hex()));
    }
}
