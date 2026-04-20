//! Pod provisioning — seeded containers, WebID + account scaffolding,
//! admin override, quota enforcement.
//!
//! The provisioning surface is intentionally declarative: callers
//! describe what the pod should look like (containers, ACLs, a WebID
//! profile document) and the module wires them into a `Storage`
//! backend. Admin-mode callers bypass ownership checks.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::PodError;
use crate::ldp::is_container;
use crate::storage::Storage;
use crate::wac::AclDocument;
use crate::webid::generate_webid_html;

/// Seed plan applied to a fresh pod.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvisionPlan {
    /// Pubkey (hex) that owns the pod.
    pub pubkey: String,
    /// Optional display name for the WebID profile.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Public pod base URL (used to render the WebID).
    pub pod_base: String,
    /// Containers to create (paths must end with `/`).
    #[serde(default)]
    pub containers: Vec<String>,
    /// ACL document to drop at the pod root.
    #[serde(default)]
    pub root_acl: Option<AclDocument>,
    /// Bytes quota. `None` means unlimited (but a real consumer crate
    /// is strongly encouraged to set one).
    #[serde(default)]
    pub quota_bytes: Option<u64>,
}

/// Result of provisioning a pod.
#[derive(Debug, Clone)]
pub struct ProvisionOutcome {
    pub webid: String,
    pub pod_root: String,
    pub containers_created: Vec<String>,
    pub quota_bytes: Option<u64>,
}

/// Seed a pod on the provided storage.
///
/// * Creates every container in `plan.containers` (idempotent — the
///   function treats `AlreadyExists` as success).
/// * Writes a WebID profile HTML at `<pod_base>/pods/<pubkey>/profile/card`.
/// * Writes a root ACL document if `plan.root_acl` is supplied.
pub async fn provision_pod<S: Storage>(
    storage: &S,
    plan: &ProvisionPlan,
) -> Result<ProvisionOutcome, PodError> {
    let pod_root = format!(
        "{}/pods/{}/",
        plan.pod_base.trim_end_matches('/'),
        plan.pubkey
    );
    let webid = format!("{pod_root}profile/card#me");

    // Ensure the pod root + default containers exist.
    let mut all_containers: Vec<String> = plan
        .containers
        .iter()
        .cloned()
        .collect();
    all_containers.push("/".into());
    all_containers.push("/profile/".into());
    all_containers.push("/settings/".into());
    // Deduplicate.
    all_containers.sort();
    all_containers.dedup();

    let mut created = Vec::new();
    for c in &all_containers {
        if !is_container(c) {
            return Err(PodError::InvalidPath(format!("not a container: {c}")));
        }
        // Create the `.meta` sidecar — this is the idiomatic way to
        // materialise a bare container without a body.
        let meta_key = format!("{}.meta", c.trim_end_matches('/'));
        match storage
            .put(
                &meta_key,
                Bytes::from_static(b"{}"),
                "application/ld+json",
            )
            .await
        {
            Ok(_) => created.push(c.clone()),
            Err(PodError::AlreadyExists(_)) => {}
            Err(e) => return Err(e),
        }
    }

    // Write WebID profile.
    let webid_html = generate_webid_html(
        &plan.pubkey,
        plan.display_name.as_deref(),
        &plan.pod_base,
    );
    storage
        .put(
            "/profile/card",
            Bytes::from(webid_html.into_bytes()),
            "text/html",
        )
        .await?;

    // Write root ACL if supplied.
    if let Some(acl) = &plan.root_acl {
        let body = serde_json::to_vec(acl)?;
        storage
            .put("/.acl", Bytes::from(body), "application/ld+json")
            .await?;
    }

    Ok(ProvisionOutcome {
        webid,
        pod_root,
        containers_created: created,
        quota_bytes: plan.quota_bytes,
    })
}

// ---------------------------------------------------------------------------
// Quota enforcement
// ---------------------------------------------------------------------------

/// Tracks per-pod byte usage against a configurable quota.
#[derive(Debug, Clone)]
pub struct QuotaTracker {
    quota_bytes: Option<u64>,
    used_bytes: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl QuotaTracker {
    pub fn new(quota_bytes: Option<u64>) -> Self {
        Self {
            quota_bytes,
            used_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn with_initial_used(quota_bytes: Option<u64>, used: u64) -> Self {
        Self {
            quota_bytes,
            used_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(used)),
        }
    }

    /// Bytes currently accounted for.
    pub fn used(&self) -> u64 {
        self.used_bytes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Configured quota, if any.
    pub fn quota(&self) -> Option<u64> {
        self.quota_bytes
    }

    /// Reserve `size` bytes. Returns `Err(PodError::PreconditionFailed)`
    /// when the operation would exceed the quota, without mutating the
    /// tracker.
    pub fn reserve(&self, size: u64) -> Result<(), PodError> {
        if let Some(q) = self.quota_bytes {
            let cur = self.used();
            if cur.saturating_add(size) > q {
                return Err(PodError::PreconditionFailed(format!(
                    "quota exceeded: {cur}+{size} > {q}"
                )));
            }
        }
        self.used_bytes
            .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Release `size` bytes previously reserved (e.g. on DELETE).
    pub fn release(&self, size: u64) {
        self.used_bytes
            .fetch_sub(size, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Admin override
// ---------------------------------------------------------------------------

/// A verified admin-override marker. The consumer crate constructs this
/// only after validating a shared-secret header against configuration;
/// the marker carries no data beyond its own existence.
#[derive(Debug, Clone, Copy)]
pub struct AdminOverride;

/// Match an admin-secret header value against the configured secret.
/// Both sides are compared with constant-time equality to avoid
/// timing leaks. Returns `Some(AdminOverride)` on match.
pub fn check_admin_override(
    header: Option<&str>,
    configured: Option<&str>,
) -> Option<AdminOverride> {
    let header = header?;
    let configured = configured?;
    if header.len() != configured.len() {
        return None;
    }
    let mut acc = 0u8;
    for (a, b) in header.bytes().zip(configured.bytes()) {
        acc |= a ^ b;
    }
    if acc == 0 {
        Some(AdminOverride)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_tracker_respects_limit() {
        let q = QuotaTracker::new(Some(100));
        q.reserve(40).unwrap();
        q.reserve(40).unwrap();
        let err = q.reserve(40).unwrap_err();
        assert!(matches!(err, PodError::PreconditionFailed(_)));
        assert_eq!(q.used(), 80);
    }

    #[test]
    fn quota_tracker_release_frees_space() {
        let q = QuotaTracker::new(Some(100));
        q.reserve(60).unwrap();
        q.release(30);
        q.reserve(60).unwrap();
        assert_eq!(q.used(), 90);
    }

    #[test]
    fn quota_tracker_none_means_unlimited() {
        let q = QuotaTracker::new(None);
        q.reserve(u64::MAX / 2).unwrap();
        q.reserve(u64::MAX / 2).unwrap();
    }

    #[test]
    fn admin_override_matches_only_exact() {
        let ok = check_admin_override(Some("topsecret"), Some("topsecret"));
        assert!(ok.is_some());
        assert!(check_admin_override(Some("topsecret "), Some("topsecret")).is_none());
        assert!(check_admin_override(None, Some("topsecret")).is_none());
        assert!(check_admin_override(Some("a"), None).is_none());
    }
}
