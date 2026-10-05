//! `vault validate` — OKF v0.2 conformance, vocabulary agreement, link
//! integrity and the public gate.
//!
//! Rejects obsolete authoring constructs outside code examples: `json-ld`
//! metadata fences, Logseq `key:: value` properties and block references,
//! and `{{embed}}` macros. The canonical input is Obsidian YAML frontmatter.
//!
//! Severity is load-bearing. An **error** blocks `vault build`; a **warning**
//! is reported and does not; **info** is a fact worth surfacing. `MULTI_PARENT`
//! is deliberately info, not a warning: 957 classes carry more than one parent
//! *by design* (the corpus is a cross-domain lattice), and reporting that as a
//! defect once published "961 validation warnings" when 958 of them were the
//! design.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::Serialize;
use vault_core::code::CodeMap;
use vault_core::okf::{OkfBlock, Status};
use vault_core::page::{Page, Vault, VaultKind};
use vault_core::slug::slugify;
use vault_core::vocabulary::{ScalarDef, ScalarType, Vocabulary};

use crate::model::{ClassRecord, Corpus};

/// The domain vocabulary the Python pipeline accepted, used when
/// `vocabulary.yaml` does not enumerate `domain` itself.
const FALLBACK_DOMAINS: &[&str] = &[
    "artificial-intelligence",
    "spatial-computing",
    "blockchain",
    "infrastructure",
    "distributed-collaboration",
    "robotics",
    "ai",
    "supply-chain",
    "metaverse",
    "data",
    "governance",
    "security",
    "standards",
    "finance",
    "distributed-systems",
    "machine-learning",
];

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocks the build.
    Error,
    /// Reported, does not block.
    Warning,
    /// A fact worth surfacing.
    Info,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One finding.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// The page it was found on.
    pub path: String,
    /// How serious it is.
    pub severity: Severity,
    /// A stable machine code.
    pub code: String,
    /// One line a human can act on.
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} {}: {}",
            self.severity.as_str(),
            self.code,
            self.path,
            self.message
        )
    }
}

/// The whole report.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Pages examined under `pages/`.
    pub total_pages: usize,
    /// Pages examined under `journals/`.
    ///
    /// 1,230 journal pages across the two vaults were invisible to `validate`,
    /// `retrieve` and `build` until the tree was walked. A count of zero now
    /// means there are none, not that nobody looked.
    pub total_journals: usize,
    /// Markdown files the vault walk declined to load, with the reason.
    pub skipped: Vec<vault_core::page::Skip>,
    /// Pages carrying an ontology identity.
    pub pages_with_ontology: usize,
    /// Pages with `public: true`.
    pub public_pages: usize,
    /// Every finding, in page order.
    pub issues: Vec<Issue>,
}

/// The machine-readable summary written to `api/validation-report.json`.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Pages examined under `pages/`.
    pub total_pages: usize,
    /// Pages examined under `journals/`.
    pub total_journals: usize,
    /// Markdown files the walk declined to load, with the reason.
    pub skipped: Vec<vault_core::page::Skip>,
    /// Pages carrying an ontology identity.
    pub pages_with_ontology: usize,
    /// Pages with `public: true`.
    pub public_pages: usize,
    /// Total findings.
    pub total_issues: usize,
    /// Error count.
    pub errors: usize,
    /// Warning count.
    pub warnings: usize,
    /// Info count.
    pub info: usize,
    /// Findings per code.
    pub by_code: BTreeMap<String, usize>,
    /// Detailed authoring diagnostics stay local; the published report carries
    /// an empty list.
    pub issues: Vec<Issue>,
}

impl Report {
    /// Findings that block the build.
    #[must_use]
    pub fn errors(&self) -> Vec<&Issue> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .collect()
    }

    /// Findings that do not block the build.
    #[must_use]
    pub fn warnings(&self) -> Vec<&Issue> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .collect()
    }

    /// The published summary (issue detail withheld).
    #[must_use]
    pub fn summary(&self) -> Summary {
        let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
        for i in &self.issues {
            *by_code.entry(i.code.clone()).or_insert(0) += 1;
        }
        Summary {
            total_pages: self.total_pages,
            total_journals: self.total_journals,
            skipped: self.skipped.clone(),
            pages_with_ontology: self.pages_with_ontology,
            public_pages: self.public_pages,
            total_issues: self.issues.len(),
            errors: self.errors().len(),
            warnings: self.warnings().len(),
            info: self
                .issues
                .iter()
                .filter(|i| i.severity == Severity::Info)
                .count(),
            by_code,
            issues: Vec::new(),
        }
    }

    /// The full report, issue detail included — for the local CLI.
    #[must_use]
    pub fn detailed(&self) -> Summary {
        let mut s = self.summary();
        s.issues.clone_from(&self.issues);
        s
    }

    fn push(&mut self, path: &str, severity: Severity, code: &str, message: impl Into<String>) {
        self.issues.push(Issue {
            path: path.to_owned(),
            severity,
            code: code.to_owned(),
            message: message.into(),
        });
    }
}

/// Obsolete authoring patterns rejected in canonical Obsidian input.
type ResiduePatterns = (regex::Regex, regex::Regex, regex::Regex, regex::Regex);

fn residue() -> &'static ResiduePatterns {
    static R: std::sync::OnceLock<ResiduePatterns> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        (
            regex::Regex::new(r"```json-ld").expect("static fence regex"),
            // A Logseq property line: optional bullet, key, `::`, value.
            regex::Regex::new(r"(?m)^\s*(?:-\s*)?[A-Za-z0-9_.-]+::\s").expect("static prop regex"),
            regex::Regex::new(r"\{\{embed\s").expect("static embed regex"),
            regex::Regex::new(
                r"\(\([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\)\)",
            )
            .expect("static block-ref regex"),
        )
    })
}

/// The checks that need only a [`Page`] — residue, code-fence integrity,
/// vocabulary agreement and credential detection.
///
/// Separated from [`validate`]'s main loop because that loop zips `pages` with
/// the projected `corpus.records` positionally, and the `journals/` tree has no
/// records: a journal is not an ontology page. These checks apply to every
/// markdown file in the vault regardless, so they live here and both loops call
/// them.
fn page_level_checks(
    report: &mut Report,
    path: &str,
    page: &Page,
    vocab: &Vocabulary,
    strict_keys: bool,
) {
    // --- obsolete authoring constructs ---------------------------------
    //
    // Matches inside code are documentation, not residue: a page that
    // *teaches* Logseq syntax must not fail for describing a construct it
    // does not use. An unmatched fence opener is a separate defect and is
    // reported as one rather than being allowed to hide the rest of the
    // page (see `vault_core::code`).
    let code = CodeMap::scan(&page.body);
    let (fence, prop, embed, block_ref) = residue();
    // A surviving `json-ld` fence is residue *because* it is a fenced
    // block, so only an inline mention excuses it. The other three are
    // excused by any code region.
    for (regex, inline_only, issue, message) in [
        (
            fence,
            true,
            "FENCE_RESIDUE",
            "obsolete json-ld metadata fence; use YAML frontmatter",
        ),
        (
            prop,
            false,
            "LOGSEQ_PROPERTY",
            "obsolete Logseq `key:: value` property; use YAML frontmatter",
        ),
        (
            embed,
            false,
            "EMBED_RESIDUE",
            "obsolete `{{embed}}` macro; use `![[…]]`",
        ),
        (
            block_ref,
            false,
            "BLOCK_REF_RESIDUE",
            "obsolete Logseq `((block-ref))`; use an Obsidian block link",
        ),
    ] {
        let excused = |m: &regex::Match<'_>| {
            let range = m.start()..m.end();
            if inline_only {
                code.overlaps_inline(&range)
            } else {
                code.overlaps(&range)
            }
        };
        if regex.find_iter(&page.body).any(|m| !excused(&m)) {
            report.push(path, Severity::Error, issue, message);
        }
    }
    if let Some(line) = code.unterminated_fence {
        report.push(
            path,
            Severity::Warning,
            "UNTERMINATED_FENCE",
            format!(
                "an unmatched code-fence opener at line {line}; the region \
                 after it is read as prose, which is what it is"
            ),
        );
    }

    // --- vocabulary agreement -------------------------------------------
    if strict_keys {
        for key in page.frontmatter.keys() {
            if !vocab.is_known_key(key) {
                report.push(
                    path,
                    Severity::Error,
                    "UNKNOWN_KEY",
                    format!("`{key}` is not declared in ontology/vocabulary.yaml"),
                );
            }
        }
    }

    // --- credentials -------------------------------------------------------
    //
    // An ERROR, and therefore a build blocker: the corpus is published to the
    // open web, and a key that reaches a commit is compromised whether or not
    // the page is ever served. The finding names the file, the line and the
    // credential TYPE, and never the value — a validation report is itself a
    // published artefact.
    for finding in vault_core::secrets::scan(&page.body) {
        // `finding.line` is body-relative; the reader needs a FILE line.
        let line = page.body_line + finding.line - 1;
        report.push(
            path,
            Severity::Error,
            "SECRET_DETECTED",
            format!(
                "{}:{line} carries what looks like a {} credential; \
                 the value is deliberately not reported",
                page.rel_path.display(),
                finding.kind
            ),
        );
    }
}

/// Validate a loaded vault against its vocabulary.
#[must_use]
#[allow(clippy::too_many_lines)] // One check per rule; splitting hides the list.
pub fn validate(vault: &Vault, corpus: &Corpus, vocab: &Vocabulary) -> Report {
    let mut report = Report {
        total_pages: vault.pages.len(),
        total_journals: vault.journals.len(),
        skipped: vault.skipped.clone(),
        ..Report::default()
    };
    let strict_keys = vault.kind.rejects_unknown_keys();

    // `domain` is free text with a documented `observed:` census and six
    // `roots:`, so the check is "did the census ever see this value?" rather
    // than "is this in a closed enumeration?". The closed reading produced 84
    // `INVALID_DOMAIN` warnings for `economics` — a value the census records
    // with a count, i.e. one the owner knows about and has deliberately not
    // normalised yet. A warning that fires on known-good data is noise, and
    // noise is what gets filtered out along with the real findings.
    let valid_domains: HashSet<&str> = {
        let declared: Vec<&str> = vocab
            .scalars
            .get("domain")
            .map(ScalarDef::known_values)
            .unwrap_or_default();
        if declared.is_empty() {
            FALLBACK_DOMAINS.iter().copied().collect()
        } else {
            declared.into_iter().collect()
        }
    };

    let page_ids: HashSet<&str> = vault.pages.iter().map(|p| p.id.as_str()).collect();
    let alias_targets: HashSet<String> = vault
        .pages
        .iter()
        .flat_map(|p| {
            p.frontmatter
                .strings("aliases")
                .into_iter()
                .map(|a| a.to_lowercase())
        })
        .collect();
    let titles: HashSet<String> = vault
        .pages
        .iter()
        .map(|p| p.title().to_lowercase())
        .collect();
    let private_ids: HashSet<&str> = vault
        .pages
        .iter()
        .filter(|p| !p.is_public())
        .map(|p| p.id.as_str())
        .collect();

    let mut iri_owners: HashMap<&str, Vec<&str>> = HashMap::new();
    // `disjoint-with` targets resolve to records by IRI, for the sibling rule.
    let by_iri: HashMap<&str, &ClassRecord> = corpus
        .records
        .iter()
        .filter(|r| r.has_ontology && !r.iri.is_empty())
        .map(|r| (r.iri.as_str(), r))
        .collect();

    for (page, record) in vault.pages.iter().zip(&corpus.records) {
        let path = page.id.as_str();
        if record.public {
            report.public_pages += 1;
        }

        page_level_checks(&mut report, path, page, vocab, strict_keys);
        for (key, def) in &vocab.scalars {
            let Some(raw) = page.frontmatter.text(key) else {
                continue;
            };
            match def.value_type {
                ScalarType::Number => match page.frontmatter.number(key) {
                    None => report.push(
                        path,
                        Severity::Error,
                        "INVALID_NUMBER",
                        format!("`{key}: {raw}` is not a number"),
                    ),
                    Some(v) => {
                        if def.min.is_some_and(|m| v < m) || def.max.is_some_and(|m| v > m) {
                            report.push(
                                path,
                                Severity::Warning,
                                "OUT_OF_RANGE",
                                format!("`{key}: {v}` is outside the declared bounds"),
                            );
                        }
                    }
                },
                ScalarType::Boolean if page.frontmatter.boolean(key).is_none() => report.push(
                    path,
                    Severity::Error,
                    "INVALID_BOOLEAN",
                    format!("`{key}: {raw}` is not a boolean"),
                ),
                _ => {}
            }
            if !def.r#enum.is_empty() && !def.r#enum.contains(&raw) {
                report.push(
                    path,
                    Severity::Warning,
                    "INVALID_ENUM",
                    format!("`{key}: {raw}` is not one of {:?}", def.r#enum),
                );
            }
        }

        // --- OKF conformance -------------------------------------------------
        let okf = OkfBlock::from_frontmatter(&page.frontmatter);
        validate_okf(&mut report, path, &okf, vault.kind, vocab, record);

        if !record.has_ontology {
            continue;
        }
        report.pages_with_ontology += 1;

        if record.iri.is_empty() {
            report.push(
                path,
                Severity::Error,
                "MISSING_CLASS_IRI",
                "the page declares a type but no resource",
            );
            continue;
        }
        iri_owners.entry(&record.iri).or_default().push(path);
        check_disjointness(&mut report, path, record, &by_iri);
        check_definition(&mut report, path, page, record, vocab, &by_iri);

        if record.label.is_empty() {
            report.push(path, Severity::Error, "MISSING_LABEL", "no label or title");
        }
        if record.domain.is_empty() {
            report.push(path, Severity::Warning, "MISSING_DOMAIN", "no domain");
        } else if !valid_domains.contains(record.domain.as_str()) {
            report.push(
                path,
                Severity::Warning,
                "INVALID_DOMAIN",
                format!("domain `{}` is not in the valid set", record.domain),
            );
        }

        // --- taxonomy and link integrity -------------------------------------
        for parent in &record.sub_class_of {
            if parent.iri == record.iri {
                report.push(
                    path,
                    Severity::Error,
                    "SELF_REFERENCE",
                    format!("self-referential is-a: {}", record.iri),
                );
            }
            let p_slug = vault_core::slug::ref_slug(&parent.iri);
            let l_slug = slugify(&parent.label);
            if !parent.label.is_empty() && p_slug != l_slug && corpus.by_slug(&p_slug).is_none() {
                report.push(
                    path,
                    Severity::Warning,
                    "SLUG_MISMATCH",
                    format!("parent IRI slug `{p_slug}` != label slug `{l_slug}`"),
                );
            }
        }
        if record.sub_class_of.len() > 1 {
            report.push(
                path,
                Severity::Info,
                "MULTI_PARENT",
                format!(
                    "bridging class, {} parents: {}",
                    record.sub_class_of.len(),
                    record
                        .sub_class_of
                        .iter()
                        .map(|r| r.label.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }

        for key in vocab.relations.keys() {
            for link in page.frontmatter.wikilinks(key) {
                let lower = link.target.to_lowercase();
                if !page_ids.contains(link.target.as_str())
                    && !titles.contains(&lower)
                    && !alias_targets.contains(&lower)
                    && !vocab.tail_iris.contains_key(&link.target)
                {
                    report.push(
                        path,
                        Severity::Warning,
                        "DANGLING_LINK",
                        format!("`{key}` points at `{}`, which is not a page", link.target),
                    );
                }
                if record.public && private_ids.contains(link.target.as_str()) {
                    report.push(
                        path,
                        Severity::Warning,
                        "PUBLIC_LINKS_PRIVATE",
                        format!(
                            "public page links to private `{}` under `{key}`; \
                             the build will drop the edge",
                            link.target
                        ),
                    );
                }
            }
        }
    }

    let mut duplicates: Vec<(&&str, &Vec<&str>)> =
        iri_owners.iter().filter(|(_, v)| v.len() > 1).collect();
    duplicates.sort_by_key(|(iri, _)| **iri);
    for (iri, owners) in duplicates {
        report.push(
            owners[0],
            Severity::Error,
            "DUPLICATE_IRI",
            format!(
                "IRI {iri} claimed by {} pages: {}",
                owners.len(),
                owners
                    .iter()
                    .take(5)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    // --- the journals/ tree --------------------------------------------------
    //
    // A journal has no ontology identity, so none of the projection checks
    // apply to it; the page-level ones all do, and the secret scan especially:
    // a daily note is exactly where a pasted API key ends up.
    for journal in &vault.journals {
        page_level_checks(&mut report, &journal.id, journal, vocab, false);
        // The identity of a journal page is its date. A stem that is not one is
        // a page misfiled into `journals/`, which is worth saying out loud.
        if vault_core::page::journal_date(&journal.id).is_none() {
            report.push(
                &journal.id,
                Severity::Warning,
                "JOURNAL_NOT_DATED",
                "a `journals/` page's identity is its date, `YYYY-MM-DD`",
            );
        }
        let declared = journal.frontmatter.text("type");
        if let Some(declared) = declared {
            if declared != vault_core::page::JOURNAL_TYPE
                && !vocab.working_types.contains(&declared)
                && !vocab.types.contains_key(&declared)
            {
                report.push(
                    &journal.id,
                    Severity::Error,
                    "INVALID_TYPE",
                    format!("`{declared}` is not a declared type"),
                );
            }
        }
    }

    report
}

/// ADR-2124: a `defines-as` value is a conjunction of named classes and
/// existentials over emitted object properties ([`vault_core::definition`]),
/// on a class page, naming only class pages. Anything else is an error
/// (`NON_EL_DEFINITION`): the emitter would drop it, and a silently dropped
/// definition is a classification the author believes exists and does not.
fn check_definition(
    report: &mut Report,
    path: &str,
    page: &Page,
    record: &ClassRecord,
    vocab: &Vocabulary,
    by_iri: &HashMap<&str, &ClassRecord>,
) {
    use vault_core::definition::{
        parse, refusal_code, strip_code, DEFINES_AS_KEY, NON_EL_DEFINITION,
    };
    let Some(value) = page.frontmatter.get(DEFINES_AS_KEY) else {
        return;
    };
    if !vocab.is_relation(DEFINES_AS_KEY) {
        return; // `UNKNOWN_KEY` already reports an unregistered key.
    }
    if let Err(message) = parse(value, vocab) {
        // `DEFINITION_OVER_TRANSITIVE` and `DEFINITION_NOT_EXISTENTIAL` are
        // reported under their own code; every other shape is non-EL.
        report.push(
            path,
            Severity::Error,
            refusal_code(&message),
            strip_code(&message).to_owned(),
        );
        return;
    }
    let mut refuse =
        |message: String| report.push(path, Severity::Error, NON_EL_DEFINITION, message);
    if record.entity_type == crate::model::EntityType::Individual {
        refuse("an individual cannot be defined; `defines-as` belongs on a class page".to_owned());
        return;
    }
    let missing: Vec<String> = record
        .defines_as
        .iter()
        .map(crate::model::Conjunct::target)
        .filter(|t| {
            by_iri
                .get(t.iri.as_str())
                .is_none_or(|r| r.entity_type != crate::model::EntityType::Class)
        })
        .map(|t| format!("`{}` ({})", t.label, t.iri))
        .collect();
    if !missing.is_empty() {
        refuse(format!(
            "the definition names {}, which is not a class page; a definition is over \
             existing classes",
            missing.join(", ")
        ));
    }
}

/// ADR-2125: every `disjoint-with` pair is two siblings — a declared class
/// page sharing a direct parent with this one — and neither member is a domain
/// root or taxonomy category ([`vault_core::consistency::check_disjoint_pair`],
/// the rule the elevation actor applies too).
fn check_disjointness(
    report: &mut Report,
    path: &str,
    record: &ClassRecord,
    by_iri: &HashMap<&str, &ClassRecord>,
) {
    use vault_core::consistency::{check_disjoint_pair, DISJOINT_NOT_SIBLINGS};
    let parents =
        |r: &ClassRecord| -> Vec<String> { r.sub_class_of.iter().map(|p| p.iri.clone()).collect() };
    for target in record.relation(crate::model::DISJOINT_WITH_JSON_KEY) {
        let outcome = match by_iri.get(target.iri.as_str()) {
            Some(other) => {
                check_disjoint_pair(&record.iri, &parents(record), &other.iri, &parents(other))
            }
            None if vault_core::consistency::is_taxonomic(&target.iri) => {
                check_disjoint_pair(&record.iri, &[], &target.iri, &[])
            }
            None => Err(vault_core::promotion::Blocker::new(
                DISJOINT_NOT_SIBLINGS,
                format!(
                    "{} disjoint-with {}: the target is not a class page, so it cannot be a sibling",
                    record.iri, target.iri
                ),
            )),
        };
        if let Err(blocker) = outcome {
            report.push(path, Severity::Error, &blocker.code, blocker.detail);
        }
    }
}

fn validate_okf(
    report: &mut Report,
    path: &str,
    okf: &OkfBlock,
    kind: VaultKind,
    vocab: &Vocabulary,
    record: &ClassRecord,
) {
    let knowledge = kind == VaultKind::Knowledge;

    match &okf.type_name {
        None if knowledge => report.push(
            path,
            Severity::Error,
            "MISSING_TYPE",
            "every knowledge page needs a `type`",
        ),
        Some(t) => {
            let permitted = if knowledge {
                vocab.types.contains_key(t)
            } else {
                vocab.working_types.contains(t) || vocab.types.contains_key(t)
            };
            if !permitted && !vocab.types.is_empty() {
                report.push(
                    path,
                    Severity::Error,
                    "INVALID_TYPE",
                    format!("`type: {t}` is not declared for this vault"),
                );
            }
            if let Some(def) = vocab.types.get(t) {
                for required in &def.required {
                    let present = match required.as_str() {
                        "resource" => !record.iri.is_empty() && record.has_ontology,
                        "status" => okf.status_raw.is_some(),
                        other => !record.page_id.is_empty() && okf_has(okf, other),
                    };
                    if !present {
                        report.push(
                            path,
                            Severity::Error,
                            "MISSING_REQUIRED",
                            format!("`type: {t}` requires `{required}`"),
                        );
                    }
                }
            }
        }
        None => {}
    }

    match (&okf.status_raw, okf.status) {
        (Some(raw), None) => report.push(
            path,
            Severity::Error,
            "INVALID_STATUS",
            format!("`status: {raw}` is not draft, stable or deprecated"),
        ),
        (None, _) if knowledge && record.has_ontology => report.push(
            path,
            Severity::Error,
            "MISSING_STATUS",
            "every knowledge page needs a `status`",
        ),
        _ => {}
    }

    if okf.status == Some(Status::Stable) && !okf.is_human_verified() {
        report.push(
            path,
            Severity::Warning,
            "UNVERIFIED_STABLE",
            "`status: stable` with no human entry in `verified`",
        );
    }
    if okf.generated.is_none() && knowledge && record.has_ontology {
        report.push(
            path,
            Severity::Warning,
            "MISSING_GENERATED",
            "no `generated` stamp",
        );
    }
}

fn okf_has(okf: &OkfBlock, key: &str) -> bool {
    match key {
        "type" => okf.type_name.is_some(),
        "generated" => okf.generated.is_some(),
        "verified" => !okf.verified.is_empty(),
        "sources" => !okf.sources.is_empty(),
        "stale_after" => okf.stale_after.is_some(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::page::{Page, VaultKind};

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
working_types: [Note]
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires" }
scalars:
  domain:   { type: text, enum: [infrastructure, blockchain] }
  quality:  { type: number, min: 0, max: 1 }
"#,
        )
        .unwrap()
    }

    fn vault_of(pages: Vec<(&str, &str)>, kind: VaultKind) -> Vault {
        Vault {
            root: std::path::PathBuf::new(),
            kind,
            journals: Vec::new(),
            skipped: Vec::new(),
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
        }
    }

    fn run(pages: Vec<(&str, &str)>, kind: VaultKind) -> Report {
        let vault = vault_of(pages, kind);
        let corpus = Corpus::build(&vault, &vocab());
        validate(&vault, &corpus, &vocab())
    }

    fn codes(report: &Report) -> Vec<String> {
        report.issues.iter().map(|i| i.code.clone()).collect()
    }

    const GOOD: &str = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: stable\ndomain: infrastructure\ngenerated: { by: process:vault/1.0, at: 2026-09-22T00:00:00Z }\nverified: [{ by: human:npub1, at: 2026-09-22T00:00:00Z }]\n---\n";

    #[test]
    fn a_conformant_page_produces_no_errors() {
        let r = run(vec![("A", GOOD)], VaultKind::Knowledge);
        assert!(r.errors().is_empty(), "{:?}", codes(&r));
        assert_eq!(r.public_pages, 1);
        assert_eq!(r.pages_with_ontology, 1);
    }

    #[test]
    fn an_unknown_key_fails_knowledge_but_not_working() {
        let page =
            "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\nowl-class: Thing\n---\n";
        assert!(codes(&run(vec![("A", page)], VaultKind::Knowledge))
            .contains(&"UNKNOWN_KEY".to_owned()));
        let working = "---\ntype: Note\nowl-class: Thing\n---\n";
        assert!(!codes(&run(vec![("A", working)], VaultKind::Working))
            .contains(&"UNKNOWN_KEY".to_owned()));
    }

    #[test]
    fn residue_from_the_old_format_is_an_error() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n```json-ld\n{}\n```\n- public:: true\n{{embed [[X]]}}\n";
        let c = codes(&run(vec![("A", page)], VaultKind::Knowledge));
        assert!(c.contains(&"FENCE_RESIDUE".to_owned()));
        assert!(c.contains(&"LOGSEQ_PROPERTY".to_owned()));
        assert!(c.contains(&"EMBED_RESIDUE".to_owned()));
    }

    #[test]
    fn residue_inside_a_fenced_block_or_an_inline_span_is_not_residue() {
        // A page may legitimately SHOW the old syntax — a migration note, a
        // style guide, an example. Only a live construct is residue.
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n\
                    Fenced example:\n\n```markdown\nrequires:: [[Example]]\n\
                    {{embed [[Other]]}}\n((deadbeef-dead-beef-dead-beefdeadbeef))\n```\n\n\
                    Inline: `key:: value` and `{{embed [[X]]}}`.\n";
        let c = codes(&run(vec![("A", page)], VaultKind::Knowledge));
        assert!(!c.contains(&"LOGSEQ_PROPERTY".to_owned()), "{c:?}");
        assert!(!c.contains(&"EMBED_RESIDUE".to_owned()), "{c:?}");
        assert!(!c.contains(&"BLOCK_REF_RESIDUE".to_owned()), "{c:?}");
    }

    #[test]
    fn a_credential_is_an_error_naming_the_line_and_the_type_but_never_the_value() {
        // Synthetic value; never live.
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n\
                    Notes.\nkey: sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        let found: Vec<&Issue> = r
            .issues
            .iter()
            .filter(|i| i.code == "SECRET_DETECTED")
            .collect();
        assert_eq!(found.len(), 1, "{:?}", r.issues);
        assert_eq!(found[0].severity, Severity::Error);
        // `---` x2 + two keys = 4 lines of frontmatter, `Notes.` line 5, key line 6.
        assert!(found[0].message.contains(":7"), "{}", found[0].message);
        assert!(
            found[0].message.contains("openai_key"),
            "{}",
            found[0].message
        );
        assert!(!found[0].message.contains("AAAA"), "{}", found[0].message);
    }

    #[test]
    fn an_environment_variable_reference_is_not_a_credential() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n\
                    Set `api_key=$OPENAI_API_KEY` in the environment.\n";
        let c = codes(&run(vec![("A", page)], VaultKind::Knowledge));
        assert!(!c.contains(&"SECRET_DETECTED".to_owned()), "{c:?}");
    }

    #[test]
    fn a_page_documenting_logseq_syntax_is_not_residue() {
        // `Dr O'Hare Writing for LogSeq` teaches the syntax; it must not fail
        // for describing the construct being migrated away from.
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n                    - **Block Embeds:** Use `{{embed ((block-uuid))}}` for embedding.\n                    - Properties look like `key:: value` in Logseq.\n                    \n```\nrequires:: [[Example]]\n```\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        let c = codes(&r);
        assert!(!c.contains(&"EMBED_RESIDUE".to_owned()), "{c:?}");
        assert!(!c.contains(&"LOGSEQ_PROPERTY".to_owned()), "{c:?}");
        assert!(!c.contains(&"BLOCK_REF_RESIDUE".to_owned()), "{c:?}");
        assert!(r.errors().is_empty(), "{:?}", r.errors());
    }

    #[test]
    fn an_unterminated_fence_does_not_hide_residue_after_it() {
        // 511 pages open a stray fence; 1,713 real `key::` lines fall after
        // it. Those must still be reported, and the defect named separately.
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n                    intro\n```\nstray opener never closed\nsources:: [[X]]\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        let c = codes(&r);
        assert!(
            c.contains(&"LOGSEQ_PROPERTY".to_owned()),
            "a line after an unmatched opener is prose: {c:?}"
        );
        assert!(c.contains(&"UNTERMINATED_FENCE".to_owned()), "{c:?}");
        // The defect is a warning; the residue it exposes is the error.
        assert!(r.warnings().iter().any(|i| i.code == "UNTERMINATED_FENCE"));
    }

    #[test]
    fn a_block_reference_outside_code_is_residue() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\n---\n                    See ((66f13d66-1c8e-45a9-9f8b-41c4cc68ef9c)) for detail.\n";
        assert!(codes(&run(vec![("A", page)], VaultKind::Knowledge))
            .contains(&"BLOCK_REF_RESIDUE".to_owned()));
    }

    #[test]
    fn a_duplicate_iri_is_an_error() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:same\nstatus: draft\n---\n";
        assert!(
            codes(&run(vec![("A", page), ("B", page)], VaultKind::Knowledge))
                .contains(&"DUPLICATE_IRI".to_owned())
        );
    }

    #[test]
    fn a_self_referential_parent_is_an_error() {
        let page =
            "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\nis-a: [\"[[A]]\"]\n---\n";
        assert!(codes(&run(vec![("A", page)], VaultKind::Knowledge))
            .contains(&"SELF_REFERENCE".to_owned()));
    }

    #[test]
    fn a_missing_status_or_type_is_an_error() {
        let no_status = "---\ntype: Class\nresource: urn:ngm:class:a\n---\n";
        assert!(codes(&run(vec![("A", no_status)], VaultKind::Knowledge))
            .contains(&"MISSING_STATUS".to_owned()));
        let no_type = "---\nresource: urn:ngm:class:a\nstatus: draft\n---\n";
        assert!(codes(&run(vec![("A", no_type)], VaultKind::Knowledge))
            .contains(&"MISSING_TYPE".to_owned()));
    }

    #[test]
    fn an_undeclared_type_is_an_error() {
        let page = "---\ntype: Widget\nresource: urn:ngm:class:a\nstatus: draft\n---\n";
        assert!(codes(&run(vec![("A", page)], VaultKind::Knowledge))
            .contains(&"INVALID_TYPE".to_owned()));
    }

    #[test]
    fn an_out_of_range_scalar_is_a_warning_not_an_error() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\nquality: 4\n---\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        assert!(codes(&r).contains(&"OUT_OF_RANGE".to_owned()));
        assert!(r.errors().is_empty());
    }

    #[test]
    fn a_dangling_link_is_a_warning() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\nrequires: [\"[[Nowhere]]\"]\n---\n";
        assert!(codes(&run(vec![("A", page)], VaultKind::Knowledge))
            .contains(&"DANGLING_LINK".to_owned()));
    }

    #[test]
    fn a_public_page_linking_to_a_private_one_is_flagged() {
        let pub_page = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: draft\nrequires: [\"[[B]]\"]\n---\n";
        let priv_page =
            "---\ntype: Class\npublic: false\nresource: urn:ngm:class:b\nstatus: draft\n---\n";
        assert!(codes(&run(
            vec![("A", pub_page), ("B", priv_page)],
            VaultKind::Knowledge
        ))
        .contains(&"PUBLIC_LINKS_PRIVATE".to_owned()));
    }

    #[test]
    fn multi_parent_is_info_not_a_warning() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: draft\nis-a: [\"[[B]]\", \"[[C]]\"]\n---\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        let issue = r
            .issues
            .iter()
            .find(|i| i.code == "MULTI_PARENT")
            .expect("multi-parent reported");
        assert_eq!(issue.severity, Severity::Info);
    }

    #[test]
    fn stable_without_a_human_signature_is_a_warning() {
        let page = "---\ntype: Class\nresource: urn:ngm:class:a\nstatus: stable\n---\n";
        let r = run(vec![("A", page)], VaultKind::Knowledge);
        assert!(codes(&r).contains(&"UNVERIFIED_STABLE".to_owned()));
        assert!(r.errors().is_empty());
    }

    #[test]
    fn the_published_summary_withholds_issue_detail() {
        let r = run(vec![("A", GOOD)], VaultKind::Knowledge);
        assert!(r.summary().issues.is_empty());
        assert_eq!(r.detailed().issues.len(), r.issues.len());
    }

    // ── ADR-2125: DISJOINT_NOT_SIBLINGS ─────────────────────────────────────

    fn run_disjoint(pages: &[(&str, String)]) -> Report {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a:          { owl: "rdfs:subClassOf" }
  disjoint-with: { owl: "owl:disjointWith", status: provisional }
"#,
        )
        .unwrap();
        let pages: Vec<(&str, &str)> = pages.iter().map(|(id, t)| (*id, t.as_str())).collect();
        let vault = vault_of(pages, VaultKind::Knowledge);
        let corpus = Corpus::build(&vault, &vocab);
        validate(&vault, &corpus, &vocab)
    }

    fn cls(slug: &str, extra: &str) -> String {
        format!("---\ntype: Class\nresource: urn:ngm:class:{slug}\nstatus: draft\n{extra}---\n")
    }

    fn not_siblings(r: &Report) -> Vec<&Issue> {
        r.issues
            .iter()
            .filter(|i| i.code == vault_core::consistency::DISJOINT_NOT_SIBLINGS)
            .collect()
    }

    #[test]
    fn disjoint_siblings_validate_cleanly() {
        let r = run_disjoint(&[
            ("Parent", cls("parent", "")),
            (
                "A",
                cls("a", "is-a: [\"[[Parent]]\"]\ndisjoint-with: [\"[[B]]\"]\n"),
            ),
            ("B", cls("b", "is-a: [\"[[Parent]]\"]\n")),
        ]);
        assert!(not_siblings(&r).is_empty(), "{:?}", r.issues);
        assert!(r.errors().is_empty(), "{:?}", r.errors());
    }

    #[test]
    fn disjointness_without_a_shared_parent_is_an_error_on_the_declaring_page() {
        let r = run_disjoint(&[
            ("X", cls("x", "")),
            ("Y", cls("y", "")),
            (
                "A",
                cls("a", "is-a: [\"[[X]]\"]\ndisjoint-with: [\"[[B]]\"]\n"),
            ),
            ("B", cls("b", "is-a: [\"[[Y]]\"]\n")),
        ]);
        let found = not_siblings(&r);
        assert_eq!(found.len(), 1, "{:?}", r.issues);
        assert_eq!(found[0].path, "A");
        assert_eq!(found[0].severity, Severity::Error);
    }

    #[test]
    fn a_domain_root_or_category_is_never_a_disjointness_member() {
        for root in ["Robotics", "AI Technique"] {
            let slug = vault_core::slug::slugify(root);
            let r = run_disjoint(&[
                ("Parent", cls("parent", "")),
                (root, cls(&slug, "is-a: [\"[[Parent]]\"]\n")),
                (
                    "A",
                    cls(
                        "a",
                        &format!("is-a: [\"[[Parent]]\"]\ndisjoint-with: [\"[[{root}]]\"]\n"),
                    ),
                ),
            ]);
            let found = not_siblings(&r);
            assert_eq!(found.len(), 1, "{root}: {:?}", r.issues);
            assert!(
                found[0]
                    .message
                    .contains("domain root or taxonomy category"),
                "{}",
                found[0].message
            );
        }
    }

    #[test]
    fn a_disjoint_target_that_is_not_a_page_is_an_error() {
        let r = run_disjoint(&[
            ("Parent", cls("parent", "")),
            (
                "A",
                cls(
                    "a",
                    "is-a: [\"[[Parent]]\"]\ndisjoint-with: [\"[[Ghost]]\"]\n",
                ),
            ),
        ]);
        assert_eq!(not_siblings(&r).len(), 1, "{:?}", r.issues);
    }

    // ── ADR-2124: NON_EL_DEFINITION ─────────────────────────────────────────

    fn run_defined(defines_as: &str) -> Report {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
  Individual: { owl: "owl:NamedIndividual", required: [resource, status] }
relations:
  is-a:          { owl: "rdfs:subClassOf" }
  has-part:      { owl: "vc:hasPart", restriction: true }
  requires:      { owl: "vc:requires", characteristics: [transitive], sub_property_of: "vc:dependsOn", restriction: true }
  depends-on:    { owl: "vc:dependsOn", characteristics: [transitive] }
  enables:       { owl: "vc:enables" }
  produces:      { owl: "vc:produces", emitted: false, status: provisional }
  disjoint-with: { owl: "owl:disjointWith", status: provisional }
  same-as:       { owl: "skos:exactMatch", emitted: false }
  defines-as:    { owl: "owl:equivalentClass", status: provisional }
"#,
        )
        .unwrap();
        let defined = cls("defined", defines_as);
        let pages = [
            ("Robot", cls("robot", "")),
            ("Gripper", cls("gripper", "")),
            ("Defined", defined),
        ];
        let pages: Vec<(&str, &str)> = pages.iter().map(|(id, t)| (*id, t.as_str())).collect();
        let vault = vault_of(pages, VaultKind::Knowledge);
        let corpus = Corpus::build(&vault, &vocab);
        validate(&vault, &corpus, &vocab)
    }

    fn non_el(r: &Report) -> Vec<&Issue> {
        r.issues
            .iter()
            .filter(|i| i.code == vault_core::definition::NON_EL_DEFINITION)
            .collect()
    }

    #[test]
    fn a_conjunction_of_named_classes_and_existentials_validates_cleanly() {
        for ok in [
            "defines-as:\n  - \"[[Robot]]\"\n  - has-part: \"[[Gripper]]\"\n",
            "defines-as:\n  - has-part: \"[[Gripper]]\"\n",
            "defines-as: \"[[Robot]]\"\n",
            "defines-as: [\"[[Robot]]\", {has-part: \"[[Gripper]]\"}]\n",
        ] {
            let r = run_defined(ok);
            assert!(non_el(&r).is_empty(), "{ok}: {:?}", r.issues);
            assert!(r.errors().is_empty(), "{ok}: {:?}", r.errors());
        }
    }

    #[test]
    fn every_non_el_definition_is_refused_on_the_declaring_page() {
        for (bad, why) in [
            (
                "defines-as:\n  - only: \"[[Gripper]]\"\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - min: \"[[Gripper]]\"\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - not: \"[[Gripper]]\"\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - or: [\"[[Robot]]\", \"[[Gripper]]\"]\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - inverse: \"[[Gripper]]\"\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - value: \"[[Gripper]]\"\n",
                "outside OWL 2 EL",
            ),
            (
                "defines-as:\n  - \"[[Robot]] or [[Gripper]]\"\n",
                "named class",
            ),
            ("defines-as:\n  - \"not [[Robot]]\"\n", "named class"),
            ("defines-as: []\n", "empty"),
            (
                "defines-as:\n  - has-part: [\"[[Robot]]\", \"[[Gripper]]\"]\n",
                "one filler",
            ),
            (
                "defines-as:\n  - has-part: {has-part: \"[[Gripper]]\"}\n",
                "nested",
            ),
            (
                "defines-as:\n  - {has-part: \"[[Gripper]]\", is-a: \"[[Robot]]\"}\n",
                "exactly one",
            ),
            (
                "defines-as:\n  - produces: \"[[Gripper]]\"\n",
                "not emitted",
            ),
            (
                "defines-as:\n  - is-a: \"[[Gripper]]\"\n",
                "not an object property",
            ),
            (
                "defines-as:\n  - disjoint-with: \"[[Gripper]]\"\n",
                "not an object property",
            ),
            ("defines-as:\n  - same-as: \"[[Gripper]]\"\n", "not emitted"),
            (
                "defines-as:\n  - frobs: \"[[Gripper]]\"\n",
                "not a declared relation",
            ),
            ("defines-as:\n  - 42\n", "named class"),
            ("defines-as:\n  - \"[[Ghost]]\"\n", "not a class page"),
            (
                "defines-as:\n  - has-part: \"[[Ghost]]\"\n",
                "not a class page",
            ),
        ] {
            let r = run_defined(bad);
            let found = non_el(&r);
            assert_eq!(found.len(), 1, "{bad}: {:?}", r.issues);
            assert_eq!(found[0].path, "Defined", "{bad}");
            assert_eq!(found[0].severity, Severity::Error, "{bad}");
            assert!(
                found[0].message.contains(why),
                "{bad}: expected {why:?} in {:?}",
                found[0].message
            );
        }
    }

    /// ADR-2124 defect: an existential over the transitive `requires` (or its
    /// transitive super-property `dependsOn`) validated, and the build then
    /// hung saturating 12,515 existentials. Validation refuses it under its
    /// own code; a plain emitted property is refused as not an existential.
    #[test]
    fn transitive_and_non_restriction_existentials_are_refused_under_their_own_codes() {
        use vault_core::vocabulary::{DEFINITION_OVER_TRANSITIVE, NOT_AN_EXISTENTIAL};
        for (bad, code, why) in [
            (
                "defines-as:\n  - \"[[Robot]]\"\n  - requires: \"[[Gripper]]\"\n",
                DEFINITION_OVER_TRANSITIVE,
                "#dependsOn, https://narrativegoldmine.com/ns/v1#requires",
            ),
            (
                "defines-as:\n  - enables: \"[[Gripper]]\"\n",
                NOT_AN_EXISTENTIAL,
                "restriction: true",
            ),
            (
                "defines-as:\n  - depends-on: \"[[Gripper]]\"\n",
                NOT_AN_EXISTENTIAL,
                "restriction: true",
            ),
        ] {
            let r = run_defined(bad);
            let found: Vec<&Issue> = r.issues.iter().filter(|i| i.code == code).collect();
            assert_eq!(found.len(), 1, "{bad}: {:?}", r.issues);
            assert_eq!(found[0].severity, Severity::Error, "{bad}");
            assert!(
                found[0].message.contains(why),
                "{bad}: {}",
                found[0].message
            );
            assert!(!found[0].message.starts_with(code), "code printed twice");
            assert!(non_el(&r).is_empty(), "{bad}: {:?}", r.issues);
        }
    }

    #[test]
    fn an_individual_cannot_be_defined() {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Individual: { owl: "owl:NamedIndividual", required: [resource, status] }
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a:       { owl: "rdfs:subClassOf" }
  defines-as: { owl: "owl:equivalentClass", status: provisional }
"#,
        )
        .unwrap();
        let pages = [
            ("Robot", cls("robot", "")),
            (
                "R2",
                "---\ntype: Individual\nresource: urn:ngm:class:r2\nstatus: draft\ndefines-as: \"[[Robot]]\"\n---\n".to_owned(),
            ),
        ];
        let pages: Vec<(&str, &str)> = pages.iter().map(|(id, t)| (*id, t.as_str())).collect();
        let vault = vault_of(pages, VaultKind::Knowledge);
        let corpus = Corpus::build(&vault, &vocab);
        let r = validate(&vault, &corpus, &vocab);
        let found = non_el(&r);
        assert_eq!(found.len(), 1, "{:?}", r.issues);
        assert!(
            found[0].message.contains("individual"),
            "{}",
            found[0].message
        );
    }
}
