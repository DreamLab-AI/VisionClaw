//! Live parity check for the flat-vault identity rule (owner decision
//! 2026-10-16): every page the old `knowledge/pages/…` + `working/pages/…`
//! layout yielded must get the SAME §V1 identity from the flat layout, and the
//! Obsidian furniture at the flat roots must yield none.
//!
//! The check runs the real [`page_name_from_repo_path`] over `git ls-files
//! knowledge working` in two checkouts of the visionGraph corpus:
//!
//! * `VISIONGRAPH_OLD` — the pre-flattening layout, resolved with the old base
//!   paths `knowledge/pages,working/pages`;
//! * `VISIONGRAPH_FLAT` — the flattened layout, resolved with `knowledge,working`.
//!
//! It is `#[ignore]`d because it needs both checkouts on disk; run it with
//!
//! ```text
//! VISIONGRAPH_OLD=/path/to/visionGraph VISIONGRAPH_FLAT=/path/to/visionGraph-flat \
//!   cargo test -p visionclaw-domain --test flat_vault_parity -- --ignored --nocapture
//! ```
//!
//! The expected difference between the two identity sets is exactly the pages
//! that sat under `knowledge/pages/.deleted/` in the old layout, which the
//! flattening deleted; the check reports and asserts that.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use visionclaw_domain::vault::{page_name_from_repo_path, VAULT_FURNITURE_DIRS};

fn ls_files(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "-z", "knowledge", "working"])
        .output()
        .expect("git ls-files runs");
    assert!(
        out.status.success(),
        "git ls-files failed in {}",
        repo.display()
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| p.ends_with(".md"))
        .map(str::to_owned)
        .collect()
}

/// The identity rule as it stood before the flattening: strip the longest
/// configured base, strip `.md`, decode the legacy encodings — with NO
/// furniture or dot-folder exclusion (the old bases pointed inside `pages/`,
/// so nothing but pages and the `.deleted/` tombstones were ever listed).
fn old_identity(path: &str, bases: &[String]) -> Option<String> {
    let rel = bases
        .iter()
        .filter_map(|base| path.strip_prefix(&format!("{base}/")))
        .min_by_key(|rest| rest.len())?;
    Some(visionclaw_domain::vault::page_name_from_path(rel))
}

#[test]
#[ignore = "needs VISIONGRAPH_OLD and VISIONGRAPH_FLAT checkouts on disk"]
fn flat_layout_yields_the_same_page_identities_as_the_pages_layout() {
    let (Ok(old), Ok(flat)) = (
        std::env::var("VISIONGRAPH_OLD"),
        std::env::var("VISIONGRAPH_FLAT"),
    ) else {
        eprintln!("VISIONGRAPH_OLD / VISIONGRAPH_FLAT unset; nothing to compare");
        return;
    };
    let old_files = ls_files(Path::new(&old));
    let flat_files = ls_files(Path::new(&flat));

    let old_bases = vec!["knowledge/pages".to_string(), "working/pages".to_string()];
    let flat_bases = vec!["knowledge".to_string(), "working".to_string()];

    // (a) identities: equal, except the `.deleted/` tombstones the flattening
    // removed. The old rule yielded an identity for each tombstone (it had no
    // dot-folder exclusion), so they are the whole of the expected difference.
    let old_ids: BTreeSet<String> = old_files
        .iter()
        .filter_map(|p| old_identity(p, &old_bases))
        .collect();
    let flat_ids: BTreeSet<String> = flat_files
        .iter()
        .filter_map(|p| page_name_from_repo_path(p, &flat_bases))
        .collect();
    let deleted: BTreeSet<String> = old_files
        .iter()
        .filter(|p| p.starts_with("knowledge/pages/.deleted/"))
        .filter_map(|p| old_identity(p, &old_bases))
        .collect();

    let only_old: Vec<&String> = old_ids.difference(&flat_ids).collect();
    let only_flat: Vec<&String> = flat_ids.difference(&old_ids).collect();
    eprintln!(
        "old: {} md files -> {} identities; flat: {} md files -> {} identities; \
         .deleted tombstones in old checkout: {}; only-old: {}; only-flat: {}",
        old_files.len(),
        old_ids.len(),
        flat_files.len(),
        flat_ids.len(),
        deleted.len(),
        only_old.len(),
        only_flat.len()
    );
    for id in &only_old {
        eprintln!("  only in old layout: {id}");
    }
    for id in &only_flat {
        eprintln!("  only in flat layout: {id}");
    }
    assert!(only_flat.is_empty(), "the flat layout mints new identities");
    let only_old: BTreeSet<String> = only_old.into_iter().cloned().collect();
    assert_eq!(
        only_old, deleted,
        "the only pages missing from the flat layout must be the deleted tombstones"
    );

    // (b) furniture: no file under a furniture folder gets a page.
    let mut furniture_files = 0usize;
    for p in &flat_files {
        let rel = p
            .strip_prefix("knowledge/")
            .or_else(|| p.strip_prefix("working/"))
            .unwrap_or(p);
        let first = rel.split('/').next().unwrap_or("");
        if VAULT_FURNITURE_DIRS.contains(&first) || first.starts_with('.') {
            furniture_files += 1;
            assert_eq!(
                page_name_from_repo_path(p, &flat_bases),
                None,
                "{p} is furniture and must get no page"
            );
        }
    }
    eprintln!("furniture markdown files in the flat checkout given no page: {furniture_files}");
    assert!(
        furniture_files > 0,
        "the flat checkout should carry templates/bases"
    );
}
