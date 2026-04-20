//! Integration tests for Sprint 4 / F1 + F2 security primitives.
//!
//! Env-var based tests share process-global state, so they run under a
//! module-scoped `Mutex` guard. Each env-sensitive test takes the guard
//! then clears the variables it owns on both entry and exit.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Mutex;

use solid_pod_rs::security::dotfile::{DotfileAllowlist, ENV_DOTFILE_ALLOWLIST};
use solid_pod_rs::security::ssrf::{
    IpClass, SsrfError, SsrfPolicy, ENV_SSRF_ALLOWLIST, ENV_SSRF_ALLOW_LINK_LOCAL,
    ENV_SSRF_ALLOW_LOOPBACK, ENV_SSRF_ALLOW_PRIVATE, ENV_SSRF_DENYLIST,
};
use url::Url;

// Serialise env-var tests. `parking_lot` would be cleaner but we stay
// on std-only deps to keep the crate surface minimal.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn clear_ssrf_env() {
    for key in [
        ENV_SSRF_ALLOW_PRIVATE,
        ENV_SSRF_ALLOW_LOOPBACK,
        ENV_SSRF_ALLOW_LINK_LOCAL,
        ENV_SSRF_ALLOWLIST,
        ENV_SSRF_DENYLIST,
    ] {
        std::env::remove_var(key);
    }
}

fn clear_dotfile_env() {
    std::env::remove_var(ENV_DOTFILE_ALLOWLIST);
}

// --- F1 classification ---------------------------------------------------

#[test]
fn f1a_classify_rfc1918_private() {
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
        IpClass::Private
    );
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(172, 20, 0, 5))),
        IpClass::Private
    );
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(192, 168, 10, 20))),
        IpClass::Private
    );
}

#[test]
fn f1a_classify_loopback_and_public() {
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
        IpClass::Loopback
    );
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
        IpClass::Public
    );
}

#[test]
fn f1a_classify_cloud_metadata_is_reserved() {
    // 169.254.169.254 MUST be Reserved, not LinkLocal — invariant 4
    // (no toggle unlocks metadata endpoints).
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
        IpClass::Reserved
    );
}

#[test]
fn f1b_classify_ipv6_link_local() {
    let ip: IpAddr = "fe80::1".parse::<Ipv6Addr>().unwrap().into();
    assert_eq!(SsrfPolicy::classify(ip), IpClass::LinkLocal);
}

#[test]
fn f1b_classify_ipv6_ula_private() {
    let ip: IpAddr = "fc00::1".parse::<Ipv6Addr>().unwrap().into();
    assert_eq!(SsrfPolicy::classify(ip), IpClass::Private);
    let ip2: IpAddr = "fd12:3456::1".parse::<Ipv6Addr>().unwrap().into();
    assert_eq!(SsrfPolicy::classify(ip2), IpClass::Private);
}

#[test]
fn f1b_classify_ipv6_loopback() {
    assert_eq!(
        SsrfPolicy::classify(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        IpClass::Loopback
    );
}

#[test]
fn f1b_classify_ipv6_public() {
    let ip: IpAddr = "2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap().into();
    assert_eq!(SsrfPolicy::classify(ip), IpClass::Public);
}

// --- F1 policy + env -----------------------------------------------------

#[tokio::test]
async fn f1c_allowlist_permits_loopback() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_ssrf_env();
    std::env::set_var(ENV_SSRF_ALLOWLIST, "localhost");

    let policy = SsrfPolicy::from_env();
    let url = Url::parse("http://localhost/").unwrap();
    // DNS resolution of `localhost` is expected to succeed on every
    // conformant host; if it does not, treat as environment-not-ready
    // rather than a test failure (we validate policy semantics, not
    // the test host's /etc/hosts).
    match policy.resolve_and_check(&url).await {
        Ok(ip) => {
            assert!(
                ip.is_loopback(),
                "localhost must resolve to a loopback address; got {ip}"
            );
        }
        Err(SsrfError::DnsFailure { .. }) | Err(SsrfError::NoAddresses { .. }) => {
            eprintln!("f1c skipped: DNS resolution for 'localhost' unavailable");
        }
        Err(e) => panic!("allowlist should have permitted localhost: {e}"),
    }

    clear_ssrf_env();
}

#[tokio::test]
async fn f1c_no_allowlist_blocks_loopback() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_ssrf_env();

    let policy = SsrfPolicy::from_env();
    let url = Url::parse("http://127.0.0.1/").unwrap();
    let result = policy.resolve_and_check(&url).await;
    match result {
        Err(SsrfError::BlockedClass {
            class: IpClass::Loopback,
            ..
        }) => {}
        other => panic!("expected BlockedClass(Loopback), got {other:?}"),
    }
}

#[tokio::test]
async fn f1d_denylist_overrides_public() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_ssrf_env();
    // 1.0.0.1 is a stable public IPv4 address (Cloudflare); resolving
    // it directly (no DNS hop) keeps the test hermetic.
    std::env::set_var(ENV_SSRF_DENYLIST, "1.0.0.1");

    let policy = SsrfPolicy::from_env();
    let url = Url::parse("http://1.0.0.1/").unwrap();
    let result = policy.resolve_and_check(&url).await;
    match result {
        Err(SsrfError::Denylisted { .. }) => {}
        other => panic!("expected Denylisted, got {other:?}"),
    }

    clear_ssrf_env();
}

#[tokio::test]
async fn f1e_loopback_url_rejected_when_default() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_ssrf_env();

    let policy = SsrfPolicy::from_env();
    // `.localhost` TLD — RFC 6761 requires it resolve to loopback.
    let url = Url::parse("http://foo.localhost/").unwrap();
    let result = policy.resolve_and_check(&url).await;
    match result {
        Err(SsrfError::BlockedClass {
            class: IpClass::Loopback,
            ..
        }) => {}
        // Some resolvers do not honour RFC 6761 and NXDOMAIN the
        // `.localhost` TLD; accept that as an equally-safe outcome
        // (the request still cannot proceed).
        Err(SsrfError::DnsFailure { .. }) | Err(SsrfError::NoAddresses { .. }) => {
            eprintln!("f1e: resolver does not honour RFC 6761 .localhost; DNS failure observed");
        }
        other => panic!("expected BlockedClass(Loopback) or DnsFailure, got {other:?}"),
    }
}

#[tokio::test]
async fn f1_missing_host_is_rejected() {
    let policy = SsrfPolicy::new();
    // `data:` URLs have no host component.
    let url = Url::parse("data:text/plain,hello").unwrap();
    match policy.resolve_and_check(&url).await {
        Err(SsrfError::MissingHost(_)) => {}
        other => panic!("expected MissingHost, got {other:?}"),
    }
}

// --- F2 dotfile allowlist ------------------------------------------------

#[test]
fn f2a_default_allowlist_permits_acl_and_meta() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_dotfile_env();

    let al = DotfileAllowlist::default();
    assert!(al.is_allowed(&PathBuf::from("/resource/.acl")));
    assert!(al.is_allowed(&PathBuf::from("/resource/.meta")));
    assert!(al.is_allowed(&PathBuf::from("/.acl")));
    assert!(al.is_allowed(&PathBuf::from("/.meta")));
}

#[test]
fn f2b_default_allowlist_blocks_env() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_dotfile_env();

    let al = DotfileAllowlist::default();
    assert!(!al.is_allowed(&PathBuf::from("/.env")));
    assert!(!al.is_allowed(&PathBuf::from("/.git")));
    assert!(!al.is_allowed(&PathBuf::from("/a/b/.secret")));
}

#[test]
fn f2c_env_allowlist_permits_listed_entries() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_dotfile_env();
    std::env::set_var(ENV_DOTFILE_ALLOWLIST, ".env,.config");

    let al = DotfileAllowlist::from_env();
    assert!(al.is_allowed(&PathBuf::from("/.env")));
    assert!(al.is_allowed(&PathBuf::from("/.config")));
    // `.acl` was NOT in the env list, so default fall-through does
    // NOT apply — this is intentional, operators who override MUST
    // include every entry they want permitted.
    assert!(!al.is_allowed(&PathBuf::from("/.acl")));

    clear_dotfile_env();
}

#[test]
fn f2d_nested_dotfile_rejected() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_dotfile_env();

    let al = DotfileAllowlist::default();
    assert!(!al.is_allowed(&PathBuf::from("foo/.secret")));
    assert!(!al.is_allowed(&PathBuf::from("/a/b/c/.oops/d")));
}

#[test]
fn f2_env_without_dot_prefix_is_normalised() {
    let _g = ENV_GUARD.lock().unwrap();
    clear_dotfile_env();
    std::env::set_var(ENV_DOTFILE_ALLOWLIST, "notifications");

    let al = DotfileAllowlist::from_env();
    assert!(al.is_allowed(&PathBuf::from("/.notifications")));

    clear_dotfile_env();
}
