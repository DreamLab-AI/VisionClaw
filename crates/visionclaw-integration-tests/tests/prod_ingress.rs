//! ADR-2119 acceptance: production ingress is declared, `lan` or `tunnel`.
//!
//! `scripts/launch.sh up prod` used to demand `CLOUDFLARE_TUNNEL_TOKEN` and
//! always start `cloudflared`, so a LAN-only Trust host (owner decision
//! 2026-10-02, R2) could not run the prod profile without a placeholder token
//! and a crash-looping tunnel container (TODO-unified CY-C-D1).
//!
//! These tests run the real launcher against a scratch project root with a
//! fake `docker` on `PATH` that records every invocation, so each clause of the
//! ingress contract is an executable assertion and nothing is ever launched:
//!
//! | `.env.prod` declares        | tunnel token | outcome                                  |
//! |-----------------------------|--------------|------------------------------------------|
//! | `VISIONCLAW_INGRESS=lan`    | absent       | starts `prod` profile, never `cloudflared` |
//! | `VISIONCLAW_INGRESS=tunnel` | absent       | refuses before touching docker           |
//! | nothing (legacy file)       | absent       | refuses: undeclared means tunnel         |
//! | `VISIONCLAW_INGRESS=tunnel` | present      | starts `prod` and `tunnel` profiles      |
//!
//! LAN mode keeps every other prod guard: the four dev flags stay forbidden and
//! the three secrets stay required, plus an explicit `CORS_ALLOWED_ORIGINS`,
//! because the compose default names the owner's public domain.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate> has a repository root")
        .to_path_buf()
}

const SECRETS: &str = "MANAGEMENT_API_KEY=test-management-key\n\
VISIONCLAW_AGENT_KEY=test-agent-key\n\
SOLID_PROXY_SECRET_KEY=test-solid-key\n";

const LAN_CORS: &str = "CORS_ALLOWED_ORIGINS=http://trust-box.lan:3001\n";

const DEV_FLAGS: [&str; 4] = [
    "SETTINGS_AUTH_BYPASS",
    "ALLOW_INSECURE_DEFAULTS",
    "VISIONCLAW_DEV_MODE",
    "DEV_AUTH_LOOPBACK",
];

/// Fake `docker`: records argv plus the profile environment, answers the
/// version probes, and reports no containers or images.
const FAKE_DOCKER: &str = r#"#!/bin/bash
printf '%s | COMPOSE_PROFILES=%s\n' "$*" "${COMPOSE_PROFILES-}" >> "$DOCKER_CALLS"
case "$1" in
  --version) echo "Docker version 0.0.0-fake" ;;
  compose) [[ "$2" == version ]] && echo "Docker Compose version v0.0.0-fake" ;;
  inspect) exit 1 ;;
esac
exit 0
"#;

struct Scratch(PathBuf);

impl Scratch {
    /// A project root holding the real launcher and compose file, a fake
    /// `docker`, and the given `.env.prod`.
    fn new(tag: &str, env_prod: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("adr2119-{tag}-{nanos}"));
        let root = repo_root();
        for sub in ["scripts", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).expect("create scratch dir");
        }
        std::fs::copy(
            root.join("scripts/launch.sh"),
            dir.join("scripts/launch.sh"),
        )
        .expect("copy launcher");
        std::fs::copy(
            root.join("docker-compose.unified.yml"),
            dir.join("docker-compose.unified.yml"),
        )
        .expect("copy compose file");
        std::fs::write(dir.join(".env.prod"), env_prod).expect("write .env.prod");
        for (name, body) in [("docker", FAKE_DOCKER), ("sleep", "#!/bin/sh\nexit 0\n")] {
            let path = dir.join("bin").join(name);
            std::fs::write(&path, body).expect("write fake tool");
            let chmod = Command::new("chmod")
                .arg("755")
                .arg(&path)
                .status()
                .expect("run chmod");
            assert!(chmod.success(), "chmod {name}");
        }
        Scratch(dir)
    }

    /// Run `launch.sh <args>` with a clean environment, so no ingress, token or
    /// profile variable leaks in from the caller.
    fn launch(&self, args: &[&str]) -> Output {
        // The fake tools shadow the real ones; the caller's PATH supplies
        // coreutils wherever this host keeps them (NixOS has no /usr/bin).
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new("bash")
            .arg(self.0.join("scripts/launch.sh"))
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.0)
            .env("DOCKER_CALLS", self.0.join("docker.calls"))
            .output()
            .expect("run launch.sh")
    }

    fn docker_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.0.join("docker.calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The single `compose … up -d --remove-orphans` call against the unified
    /// compose file: the one that starts the prod stack.
    fn stack_up_call(&self) -> String {
        let compose = format!("-f {}", self.0.join("docker-compose.unified.yml").display());
        let ups: Vec<String> = self
            .docker_calls()
            .into_iter()
            .filter(|c| c.contains(&compose) && c.contains(" up -d --remove-orphans"))
            .collect();
        assert_eq!(ups.len(), 1, "expected one stack `up`, got {ups:#?}");
        ups.into_iter().next().expect("one up call")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn transcript(out: &Output) -> String {
    format!(
        "status {:?}\n--- stdout\n{}\n--- stderr\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn lan_prod_without_tunnel_token_plans_a_start_without_cloudflared() {
    let s = Scratch::new(
        "lan-up",
        &format!("VISIONCLAW_INGRESS=lan\n{SECRETS}{LAN_CORS}"),
    );
    let out = s.launch(&["up", "prod"]);
    assert!(out.status.success(), "{}", transcript(&out));

    let up = s.stack_up_call();
    assert!(
        up.contains("--profile prod"),
        "prod profile not active: {up}"
    );
    assert!(
        !up.contains("tunnel"),
        "LAN up activated the tunnel profile: {up}"
    );
    for call in s.docker_calls() {
        if call.contains("cloudflared") {
            assert!(
                call.contains(" rm --stop --force cloudflared"),
                "LAN mode touched cloudflared other than to remove a stale one: {call}"
            );
        }
    }
}

#[test]
fn tunnel_prod_without_token_refuses_before_touching_docker() {
    let s = Scratch::new(
        "tunnel-no-token",
        &format!("VISIONCLAW_INGRESS=tunnel\n{SECRETS}"),
    );
    let out = s.launch(&["up", "prod"]);
    assert!(!out.status.success(), "{}", transcript(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("CLOUDFLARE_TUNNEL_TOKEN"),
        "{}",
        transcript(&out)
    );
    assert!(s.docker_calls().is_empty(), "{:#?}", s.docker_calls());
}

#[test]
fn undeclared_ingress_keeps_the_tunnel_contract() {
    // A pre-ADR-2119 `.env.prod`: an empty or forgotten token must still fail
    // loudly, never silently mean "no tunnel".
    let s = Scratch::new("legacy-no-token", SECRETS);
    let out = s.launch(&["up", "prod"]);
    assert!(!out.status.success(), "{}", transcript(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("CLOUDFLARE_TUNNEL_TOKEN"),
        "{}",
        transcript(&out)
    );
    assert!(s.docker_calls().is_empty(), "{:#?}", s.docker_calls());
}

#[test]
fn tunnel_prod_with_token_starts_the_tunnel_profile() {
    let s = Scratch::new(
        "tunnel-up",
        &format!("VISIONCLAW_INGRESS=tunnel\nCLOUDFLARE_TUNNEL_TOKEN=test-token\n{SECRETS}"),
    );
    let out = s.launch(&["up", "prod"]);
    assert!(out.status.success(), "{}", transcript(&out));
    let up = s.stack_up_call();
    assert!(up.contains("--profile prod"), "{up}");
    assert!(
        up.contains("--profile tunnel"),
        "tunnel profile not active: {up}"
    );
}

#[test]
fn lan_prod_still_refuses_every_dev_flag() {
    for flag in DEV_FLAGS {
        let s = Scratch::new(
            "lan-dev-flag",
            &format!("VISIONCLAW_INGRESS=lan\n{SECRETS}{LAN_CORS}{flag}=0\n"),
        );
        let out = s.launch(&["up", "prod"]);
        assert!(
            !out.status.success(),
            "{flag} accepted:\n{}",
            transcript(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(flag),
            "{}",
            transcript(&out)
        );
        assert!(
            s.docker_calls().is_empty(),
            "{flag}: {:#?}",
            s.docker_calls()
        );
    }
}

#[test]
fn lan_prod_still_demands_the_secrets_and_explicit_cors() {
    let full = format!("VISIONCLAW_INGRESS=lan\n{SECRETS}{LAN_CORS}");
    for required in [
        "MANAGEMENT_API_KEY",
        "VISIONCLAW_AGENT_KEY",
        "SOLID_PROXY_SECRET_KEY",
        "CORS_ALLOWED_ORIGINS",
    ] {
        let env_prod: String = full
            .lines()
            .filter(|l| !l.starts_with(&format!("{required}=")))
            .map(|l| format!("{l}\n"))
            .collect();
        let s = Scratch::new("lan-missing", &env_prod);
        let out = s.launch(&["up", "prod"]);
        assert!(
            !out.status.success(),
            "missing {required} accepted:\n{}",
            transcript(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(required),
            "{}",
            transcript(&out)
        );
        assert!(
            s.docker_calls().is_empty(),
            "{required}: {:#?}",
            s.docker_calls()
        );
    }
}

#[test]
fn unknown_ingress_value_is_refused() {
    let s = Scratch::new(
        "bad-ingress",
        &format!("VISIONCLAW_INGRESS=public\nCLOUDFLARE_TUNNEL_TOKEN=t\n{SECRETS}{LAN_CORS}"),
    );
    let out = s.launch(&["up", "prod"]);
    assert!(!out.status.success(), "{}", transcript(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("VISIONCLAW_INGRESS"),
        "{}",
        transcript(&out)
    );
    assert!(s.docker_calls().is_empty(), "{:#?}", s.docker_calls());
}

#[test]
fn prod_down_reaches_a_leftover_tunnel() {
    // Switching a host from tunnel to LAN must not strand a running tunnel.
    let s = Scratch::new(
        "lan-down",
        &format!("VISIONCLAW_INGRESS=lan\n{SECRETS}{LAN_CORS}"),
    );
    let out = s.launch(&["down", "prod"]);
    assert!(out.status.success(), "{}", transcript(&out));
    let downs: Vec<String> = s
        .docker_calls()
        .into_iter()
        .filter(|c| c.contains(" down --remove-orphans"))
        .collect();
    assert_eq!(downs.len(), 1, "{downs:#?}");
    assert!(downs[0].contains("--profile tunnel"), "{}", downs[0]);
}

/// The `profiles:` list of one top-level service in the compose file.
fn service_profiles(compose: &str, service: &str) -> Vec<String> {
    let header = format!("  {service}:");
    let mut lines = compose
        .lines()
        .skip_while(|l| l.trim_end() != header)
        .skip(1);
    let mut profiles = Vec::new();
    let mut in_profiles = false;
    for line in lines.by_ref() {
        let indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && indent <= 2 && !line.trim_start().starts_with('#') {
            break; // next service or top-level key
        }
        let trimmed = line.trim();
        if indent == 4 {
            in_profiles = trimmed == "profiles:";
            continue;
        }
        if in_profiles {
            if let Some(p) = trimmed.strip_prefix("- ") {
                profiles.push(p.trim().to_owned());
            }
        }
    }
    profiles
}

#[test]
fn cloudflared_runs_only_under_the_tunnel_profile() {
    let compose = std::fs::read_to_string(repo_root().join("docker-compose.unified.yml"))
        .expect("read compose file");
    assert_eq!(service_profiles(&compose, "cloudflared"), vec!["tunnel"]);
    let prod = service_profiles(&compose, "visionclaw-production");
    assert!(prod.contains(&"prod".to_owned()), "{prod:?}");
    assert!(!prod.contains(&"tunnel".to_owned()), "{prod:?}");
}
