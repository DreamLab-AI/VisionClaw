//! Per-page header census through vault-core, read-only.
//!
//! ```text
//! core_census <repo> <out.jsonl> [<roster-repo>]
//! ```
//!
//! Walks `knowledge/` and `working/` of `<repo>` (templates, bases, assets and
//! dot-folders are not pages) and writes one JSON line per page with the
//! answers the readers give: publication, the principal of `generated`, human
//! certification, unreadable stamps and recall. The roster is read from
//! `<roster-repo>` (default `<repo>`), so the same roster can judge a corpus
//! before and after its migration. Used for the parity check of ADR-2220:
//! the output of the previous reader (captured before this change) and of this
//! one must be equal on every page of the real corpus.

use std::io::Write;

use vault_core::frontmatter::{self, Frontmatter};
use vault_core::lifecycle::is_recallable;
use vault_core::okf::OkfBlock;
use vault_core::roster::Roster;
use vault_core::visibility;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let roster = Roster::discover(std::path::Path::new(a.get(3).unwrap_or(&a[1])));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&a[2]).unwrap());
    let mut files = Vec::new();
    for vault in ["knowledge", "working"] {
        for e in walkdir::WalkDir::new(root.join(vault))
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                !(n.starts_with('.')
                    || (e.file_type().is_dir()
                        && matches!(n.as_ref(), "templates" | "bases" | "assets")))
            })
        {
            let e = e.unwrap();
            if e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "md") {
                files.push(e.path().to_owned());
            }
        }
    }
    files.sort();
    let now = 1_794_961_600; // 2026-11-18T00:26:40Z, fixed so runs compare
    for p in files {
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let split = frontmatter::split(&text);
        let fm = split.yaml.and_then(|y| Frontmatter::parse(y).ok());
        let line = match fm {
            None => serde_json::json!({"path": rel, "frontmatter": false}),
            Some(fm) => {
                let okf = OkfBlock::from_frontmatter(&fm);
                let reading = visibility::read(&fm.map);
                let raw_verified = match fm.get("verified") {
                    Some(serde_yaml::Value::Sequence(s)) => s.len(),
                    Some(_) => 1,
                    None => 0,
                };
                serde_json::json!({
                    "path": rel, "frontmatter": true,
                    // the previous reader's rule, for the diff
                    "public_old_rule": fm.boolean("public"),
                    "public": reading.public,
                    "visibility_problem": reading.problem.map(|p| p.to_string()),
                    "generated_present": fm.get("generated").is_some(),
                    "generated_parsed": okf.generated.is_some(),
                    "principal": okf.generated.as_ref().and_then(|g| roster.principal_of(&g.by)),
                    "verified_raw": raw_verified,
                    "verified_parsed": okf.verified.len(),
                    "human_verified": okf.is_human_verified(),
                    "human_verified_roster": okf.is_human_verified_with(&roster),
                    "problems": okf.problems.len(),
                    "type": okf.type_name,
                    "status": okf.status_raw,
                    "recallable": is_recallable(&fm.map, now),
                })
            }
        };
        writeln!(out, "{line}").unwrap();
    }
}
