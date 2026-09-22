//! `<out>/publish/` — the markdown that is actually published.
//!
//! Every other artefact in the bundle is a *projection* of the knowledge
//! vault: OWL, the scaffold and prose indexes, the search index and the graph
//! tiers are all derived from `knowledge/`'s ontology types and nothing else.
//! The publish staging is different. It is the union of
//!
//! * every `knowledge/` page with `public: true`, and
//! * every `working/` page with `public: true`,
//!
//! because publication is a per-page decision the author makes with the
//! `public` flag, and a curator who marks a `working/` note public means it.
//! Keeping the union out of the ontology projections is deliberate: a public
//! `working/` note is publishable prose, not an OWL class, and letting one into
//! `ontology.ttl` would put an ungoverned page into the reasoner.
//!
//! # The credential gate
//!
//! A page selected for publication is scanned for credentials, and **one match
//! refuses the whole build**: exit 2, nothing written, the previous bundle
//! intact. This is the last gate before bytes leave the machine, so it is a
//! refusal rather than a warning — a key that reaches the published site is
//! compromised, and no later check can un-publish it.

use std::fmt;
use std::path::Path;

use serde::Serialize;
use vault_core::page::{is_unpublished, Page, Vault};
use vault_core::secrets::{self, CredentialKind};

/// One credential found in a page selected for publication.
///
/// Carries the file, the line and the kind — never the value. The build's
/// refusal message is printed to a terminal and to CI logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretFinding {
    /// The vault the page belongs to.
    pub vault: String,
    /// The page's vault-relative path.
    pub path: String,
    /// 1-based line in the file.
    pub line: usize,
    /// What kind of credential it is.
    pub kind: CredentialKind,
}

impl fmt::Display for SecretFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}:{} ({})",
            self.vault, self.path, self.line, self.kind
        )
    }
}

/// The build refused because a page selected for publication carries a
/// credential.
///
/// A distinct error type so the CLI can exit **2** — "a migration or
/// publication the corpus does not permit" — rather than the generic 1.
#[derive(Debug, Clone)]
pub struct SecretsFound {
    /// Every finding, so one run names every page to fix.
    pub findings: Vec<SecretFinding>,
}

impl fmt::Display for SecretsFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} page(s) selected for publication carry a credential; \
             nothing was written. The values are deliberately not reported. ",
            self.findings.len()
        )?;
        for finding in self.findings.iter().take(10) {
            write!(f, "{finding}; ")?;
        }
        if self.findings.len() > 10 {
            write!(f, "… {} more", self.findings.len() - 10)?;
        }
        Ok(())
    }
}

impl std::error::Error for SecretsFound {}

/// What one vault contributed to the publish staging.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VaultPublished {
    /// Pages considered — loaded, in the publish scope.
    pub considered: usize,
    /// Pages with `public: true`, and therefore staged.
    pub published: usize,
}

/// A file staged under `publish/`.
#[derive(Debug, Clone)]
pub struct StagedPage {
    /// Path within the bundle, e.g. `publish/knowledge/Knowledge Graph.md`.
    pub path: String,
    /// The file's bytes.
    pub content: String,
}

/// The publish staging, plus the per-vault counts `--stats` reports.
#[derive(Debug, Clone, Default)]
pub struct Staging {
    /// Files to add to the bundle.
    pub files: Vec<StagedPage>,
    /// Per-vault counts, keyed by vault name.
    pub per_vault: std::collections::BTreeMap<String, VaultPublished>,
}

/// `true` when the page is eligible for the publish staging.
///
/// Two conditions, and the order matters for the count: the page must be inside
/// the publish scope at all (an `_misc/` page never publishes, whatever its
/// flag says), and it must declare `public: true`.
fn eligible(page: &Page) -> bool {
    !is_unpublished(
        page.rel_path
            .strip_prefix("pages")
            .unwrap_or(&page.rel_path),
    ) && page.is_public()
}

/// Render a page for publication.
///
/// The frontmatter is re-rendered rather than copied so a malformed or
/// non-canonical source block cannot reach the published bytes, and the body
/// follows it verbatim — it is the prose the author wrote.
fn render(page: &Page) -> String {
    let front = page.frontmatter.render().unwrap_or_default();
    let body = page.body.trim_end();
    if body.is_empty() {
        front
    } else {
        format!("{front}\n{body}\n")
    }
}

/// Stage the publish tree for every vault given, scanning each staged page for
/// credentials.
///
/// # Errors
/// [`SecretsFound`] when any page selected for publication carries a
/// credential. Nothing is staged in that case: the caller must not write a
/// partial bundle.
pub fn stage(vaults: &[&Vault]) -> Result<Staging, SecretsFound> {
    let mut staging = Staging::default();
    let mut findings: Vec<SecretFinding> = Vec::new();

    for vault in vaults {
        let name = vault.kind.dir_name().to_owned();
        let counts = staging.per_vault.entry(name.clone()).or_default();
        // `journals/` is not published: a daily note is working material, and
        // nothing in the estate consumes it. It is still scanned by
        // `vault validate`.
        for page in &vault.pages {
            let rel = page
                .rel_path
                .strip_prefix("pages")
                .unwrap_or(&page.rel_path);
            if is_unpublished(rel) {
                continue;
            }
            counts.considered += 1;
            if !eligible(page) {
                continue;
            }
            counts.published += 1;
            for finding in secrets::scan(&page.body) {
                findings.push(SecretFinding {
                    vault: name.clone(),
                    path: page.rel_path.display().to_string(),
                    line: page.body_line + finding.line - 1,
                    kind: finding.kind,
                });
            }
            staging.files.push(StagedPage {
                path: format!("publish/{name}/{}.md", page.id),
                content: render(page),
            });
        }
    }

    if !findings.is_empty() {
        return Err(SecretsFound { findings });
    }
    staging.files.push(StagedPage {
        path: "publish/knowledge/index.md".to_owned(),
        content: index_page(&staging),
    });
    Ok(staging)
}

/// The OKF §8 index of the publish staging.
///
/// §8 asks a published corpus to carry a machine-readable entry point stating
/// its own extent. The count per vault is the extent; without it a consumer
/// cannot tell an empty publication from a missing one.
fn index_page(staging: &Staging) -> String {
    use std::fmt::Write;
    let total: usize = staging.per_vault.values().map(|c| c.published).sum();
    let mut out = String::new();
    let _ = writeln!(out, "---");
    let _ = writeln!(out, "okf_version: \"0.2\"");
    let _ = writeln!(out, "type: Index");
    let _ = writeln!(out, "title: Published Corpus");
    let _ = writeln!(out, "published_count: {total}");
    let _ = writeln!(out, "by_vault:");
    for (vault, counts) in &staging.per_vault {
        let _ = writeln!(out, "  {vault}:");
        let _ = writeln!(out, "    considered: {}", counts.considered);
        let _ = writeln!(out, "    published: {}", counts.published);
    }
    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# Published Corpus");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{total} page(s) carry `public: true` and are published here."
    );
    let _ = writeln!(out);
    for (vault, counts) in &staging.per_vault {
        let _ = writeln!(
            out,
            "- `{vault}/`: {} of {} page(s)",
            counts.published, counts.considered
        );
    }
    out
}

/// Write the staging to `dir`, replacing whatever was there.
///
/// Used by `--publish-out`, which promotes the markdown separately from the C3
/// bundle. The tree is rebuilt rather than merged: a stale page left behind
/// from a previous generation is a page that is still published after being
/// un-published, which is the same failure the atomic bundle promotion exists
/// to prevent.
///
/// # Errors
/// Any I/O failure while clearing or writing the tree.
pub fn write_to(staging: &Staging, dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    for file in &staging.files {
        // Paths are `publish/<vault>/<id>.md`; `dir` replaces the `publish/`.
        let rel = file.path.strip_prefix("publish/").unwrap_or(&file.path);
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &file.content)?;
    }
    Ok(())
}

/// Load the `working/` vault beside a knowledge vault root, when it exists.
///
/// # Errors
/// Any read or parse failure in the working vault.
pub fn sibling_working(vault_root: &Path) -> Result<Option<Vault>, vault_core::error::VaultError> {
    let Some(repo) = vault_root.parent() else {
        return Ok(None);
    };
    let working = repo.join("working");
    if !working.join("pages").is_dir() {
        return Ok(None);
    }
    Vault::load(&working, vault_core::page::VaultKind::Working).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::page::VaultKind;

    fn vault(kind: VaultKind, pages: Vec<(&str, &str)>) -> Vault {
        Vault {
            root: std::path::PathBuf::from("/v"),
            kind,
            pages: pages
                .into_iter()
                .map(|(id, text)| {
                    Page::parse(
                        format!("/v/pages/{id}.md"),
                        format!("pages/{id}.md"),
                        id,
                        text,
                    )
                    .unwrap()
                })
                .collect(),
            journals: Vec::new(),
            skipped: Vec::new(),
        }
    }

    #[test]
    fn the_staging_is_the_union_of_both_vaults_public_pages() {
        let knowledge = vault(
            VaultKind::Knowledge,
            vec![
                (
                    "Public Class",
                    "---\ntype: Class\npublic: true\n---\nProse.\n",
                ),
                (
                    "Held Class",
                    "---\ntype: Class\npublic: false\n---\nProse.\n",
                ),
            ],
        );
        let working = vault(
            VaultKind::Working,
            vec![
                (
                    "Public Note",
                    "---\ntype: Note\npublic: true\n---\nProse.\n",
                ),
                ("Private Note", "---\ntype: Note\n---\nProse.\n"),
            ],
        );
        let staging = stage(&[&knowledge, &working]).unwrap();
        let paths: Vec<&str> = staging.files.iter().map(|f| f.path.as_str()).collect();
        assert!(
            paths.contains(&"publish/knowledge/Public Class.md"),
            "{paths:?}"
        );
        assert!(
            paths.contains(&"publish/working/Public Note.md"),
            "{paths:?}"
        );
        assert!(
            !paths.contains(&"publish/knowledge/Held Class.md"),
            "{paths:?}"
        );
        assert!(
            !paths.contains(&"publish/working/Private Note.md"),
            "{paths:?}"
        );

        // Per-vault counts, which is what `--stats` prints.
        assert_eq!(
            staging.per_vault["knowledge"],
            VaultPublished {
                considered: 2,
                published: 1
            }
        );
        assert_eq!(
            staging.per_vault["working"],
            VaultPublished {
                considered: 2,
                published: 1
            }
        );
    }

    #[test]
    fn an_unpublished_dir_never_publishes_however_it_is_flagged() {
        let mut knowledge = vault(
            VaultKind::Knowledge,
            vec![("Held", "---\ntype: Class\npublic: true\n---\nProse.\n")],
        );
        knowledge.pages[0].rel_path = std::path::PathBuf::from("pages/_misc/Held.md");
        let staging = stage(&[&knowledge]).unwrap();
        assert!(
            staging.files.iter().all(|f| !f.path.contains("Held")),
            "{:?}",
            staging.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        // Not published, and not counted as considered either: it is outside
        // the scope, not a page that chose to stay private.
        assert_eq!(staging.per_vault["knowledge"].considered, 0);
    }

    #[test]
    fn one_credential_on_one_published_page_refuses_the_whole_staging() {
        let knowledge = vault(
            VaultKind::Knowledge,
            vec![(
                "Leaky",
                "---\ntype: Class\npublic: true\n---\nkey: sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            )],
        );
        let err = stage(&[&knowledge]).unwrap_err();
        assert_eq!(err.findings.len(), 1);
        assert_eq!(err.findings[0].kind, CredentialKind::OpenAiKey);
        // `---`, `type`, `public`, `---` then the body: file line 5.
        assert_eq!(err.findings[0].line, 5);
        // The message names the page and the kind and never the value.
        let message = err.to_string();
        assert!(message.contains("Leaky"), "{message}");
        assert!(message.contains("openai_key"), "{message}");
        assert!(!message.contains("AAAA"), "{message}");
    }

    #[test]
    fn a_credential_on_a_private_page_does_not_refuse_the_build() {
        // The gate is about what is *published*. A private page is a problem
        // worth reporting — `vault validate` does report it — but it is not a
        // publication, and refusing the build would make the corpus
        // unbuildable over a page nobody can read.
        let knowledge = vault(
            VaultKind::Knowledge,
            vec![(
                "Held",
                "---\ntype: Class\npublic: false\n---\nkey: sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            )],
        );
        assert!(stage(&[&knowledge]).is_ok());
    }

    #[test]
    fn the_index_states_the_extent_per_vault() {
        let knowledge = vault(
            VaultKind::Knowledge,
            vec![("A", "---\ntype: Class\npublic: true\n---\nProse.\n")],
        );
        let staging = stage(&[&knowledge]).unwrap();
        let index = staging
            .files
            .iter()
            .find(|f| f.path == "publish/knowledge/index.md")
            .expect("an index is always written");
        assert!(
            index.content.contains("published_count: 1"),
            "{}",
            index.content
        );
        assert!(index.content.contains("considered: 1"), "{}", index.content);
    }
}
