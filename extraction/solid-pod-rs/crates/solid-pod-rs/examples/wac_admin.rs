//! CLI for reading and writing WAC `.acl` sidecars on a pod root.
//!
//! Operates directly on an `FsBackend` — does not require a running
//! server. Useful for bootstrapping a pod with an owner ACL, or for
//! sanity-checking an access-control decision locally.
//!
//! Usage:
//! ```bash
//! # Grant a specific agent full control over the pod root.
//! cargo run --example wac_admin -p solid-pod-rs -- \
//!     /tmp/my-pod grant did:nostr:alice /public/ Read
//!
//! # Print the currently-effective ACL for a resource path.
//! cargo run --example wac_admin -p solid-pod-rs -- \
//!     /tmp/my-pod show /public/file.ttl
//!
//! # Check whether an agent has a mode.
//! cargo run --example wac_admin -p solid-pod-rs -- \
//!     /tmp/my-pod check did:nostr:alice /public/file.ttl Read
//! ```
//!
//! Expected output for the `grant` command above:
//! ```text
//! wrote /.acl
//!   agent=did:nostr:alice
//!   accessTo=/public/
//!   mode=acl:Read
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use solid_pod_rs::{
    storage::{fs::FsBackend, Storage},
    wac::{self, AccessMode, AclDocument, AclResolver, StorageAclResolver},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root: PathBuf = args
        .next()
        .ok_or("usage: wac_admin <pod-root> <cmd> ...")?
        .into();
    let cmd = args.next().ok_or("missing command")?;

    let storage = Arc::new(FsBackend::new(&root).await?);

    match cmd.as_str() {
        "grant" => {
            let agent = args.next().ok_or("missing agent URI")?;
            let target = args.next().ok_or("missing target path")?;
            let mode = args.next().ok_or("missing mode (Read/Write/Append/Control)")?;
            grant(storage.clone(), &agent, &target, &mode).await?;
        }
        "show" => {
            let resource = args.next().ok_or("missing resource path")?;
            show(storage.clone(), &resource).await?;
        }
        "check" => {
            let agent = args.next().ok_or("missing agent URI")?;
            let resource = args.next().ok_or("missing resource path")?;
            let mode = args.next().ok_or("missing mode")?;
            check(storage.clone(), &agent, &resource, &mode).await?;
        }
        other => return Err(format!("unknown command: {other}").into()),
    }
    Ok(())
}

async fn grant(
    storage: Arc<FsBackend>,
    agent: &str,
    target: &str,
    mode: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode_iri = mode_to_iri(mode)?;
    // Default (inherited) for containers, accessTo otherwise.
    let is_container = target.ends_with('/');
    let key = if is_container { "acl:default" } else { "acl:accessTo" };
    let acl_body = serde_json::json!({
        "@context": {
            "acl": "http://www.w3.org/ns/auth/acl#",
            "foaf": "http://xmlns.com/foaf/0.1/"
        },
        "@graph": [{
            "@id": "#rule-1",
            "acl:agent":   { "@id": agent },
            key:            { "@id": target },
            "acl:mode":    { "@id": mode_iri }
        }]
    });
    let acl_path = acl_sidecar_for(target);
    let body = Bytes::from(serde_json::to_vec_pretty(&acl_body)?);
    storage.put(&acl_path, body, "application/ld+json").await?;
    println!("wrote {acl_path}");
    println!("  agent={agent}");
    println!("  {key}={target}");
    println!("  mode={mode_iri}");
    Ok(())
}

async fn show(
    storage: Arc<FsBackend>,
    resource: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = StorageAclResolver::new(storage);
    match resolver.find_effective_acl(resource).await? {
        Some(_doc) => {
            let acl_path = acl_sidecar_for(resource);
            println!("effective ACL path: {acl_path}");
        }
        None => {
            println!("no effective ACL found for {resource} (denied by default)");
        }
    }
    Ok(())
}

async fn check(
    storage: Arc<FsBackend>,
    agent: &str,
    resource: &str,
    mode: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode_enum = match mode {
        "Read" => AccessMode::Read,
        "Write" => AccessMode::Write,
        "Append" => AccessMode::Append,
        "Control" => AccessMode::Control,
        other => return Err(format!("unknown mode: {other}").into()),
    };
    let resolver = StorageAclResolver::new(storage);
    let doc: Option<AclDocument> = resolver.find_effective_acl(resource).await?;
    let allowed = wac::evaluate_access(doc.as_ref(), Some(agent), resource, mode_enum, None);
    println!(
        "{decision}: agent={agent} resource={resource} mode={mode}",
        decision = if allowed { "ALLOWED" } else { "DENIED" }
    );
    Ok(())
}

fn mode_to_iri(mode: &str) -> Result<&'static str, Box<dyn std::error::Error>> {
    Ok(match mode {
        "Read" => "acl:Read",
        "Write" => "acl:Write",
        "Append" => "acl:Append",
        "Control" => "acl:Control",
        other => return Err(format!("unknown mode: {other}").into()),
    })
}

fn acl_sidecar_for(path: &str) -> String {
    if path == "/" {
        "/.acl".to_string()
    } else {
        format!("{}.acl", path.trim_end_matches('/'))
    }
}
