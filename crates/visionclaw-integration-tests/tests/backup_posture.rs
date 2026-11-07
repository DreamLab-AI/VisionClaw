//! ADR-2017 acceptance: write-master backup posture.
//!
//! The estate-review probe reproduced three defects in
//! `scripts/backup-sqlite.sh`:
//!
//!   1. it exited zero when one of two *requested* databases was missing,
//!   2. its default destination sat inside the source data directory, and
//!   3. it proved page integrity (`PRAGMA integrity_check`) but never proved
//!      the snapshot was restorable at the application level.
//!
//! These tests drive the real script in `MODE=host` against throwaway SQLite
//! fixtures. They need `bash` and `sqlite3` on `PATH` and touch nothing outside
//! a per-test temporary directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate> has a repository root")
        .to_path_buf()
}

fn script() -> PathBuf {
    repo_root().join("scripts/backup-sqlite.sh")
}

/// A disposable working directory; removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("adr2017-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn have_tool(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a SQLite database carrying the tables the application expects, so
/// the script's restore check has something real to probe.
fn make_db(path: &Path, tables: &[&str]) {
    let ddl: String = tables
        .iter()
        .map(|t| format!("CREATE TABLE IF NOT EXISTS \"{t}\" (id INTEGER PRIMARY KEY);"))
        .collect();
    let out = Command::new("sqlite3")
        .arg(path)
        .arg(format!("PRAGMA journal_mode=WAL; {ddl}"))
        .output()
        .expect("run sqlite3");
    assert!(
        out.status.success(),
        "sqlite3 fixture creation failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Populate a scratch data directory with the five declared databases,
/// omitting any named in `omit`.
fn seed_data_dir(data_dir: &Path, omit: &[&str]) {
    std::fs::create_dir_all(data_dir).expect("create data dir");
    let schemas: [(&str, &[&str]); 5] = [
        ("settings.sqlite3", &["settings", "schema_migrations"]),
        (
            "enrichment.sqlite3",
            &[
                "enrichment_proposals",
                "enrichment_decisions",
                "schema_migrations",
            ],
        ),
        (
            "kpi.sqlite3",
            &["kpi_snapshots", "kpi_lineage", "schema_migrations"],
        ),
        // The share store (src/adapters/sqlite_share_repository.rs), required
        // since 238f5960d.
        ("shares.sqlite3", &["shared_graphs", "schema_migrations"]),
        (
            "liveness.sqlite3",
            &["liveness_canaries", "canary_fires", "schema_migrations"],
        ),
    ];
    for (name, tables) in schemas {
        if omit.contains(&name) {
            continue;
        }
        make_db(&data_dir.join(name), tables);
    }
}

/// Run the backup script with `MODE=host` and the given extra environment.
fn run_backup(data_dir: &Path, backup_root: &Path, extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(script())
        .current_dir(repo_root())
        .env("MODE", "host")
        .env("DATA_DIR", data_dir)
        .env("BACKUP_ROOT", backup_root);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.output().expect("run backup-sqlite.sh")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// The one timestamped directory a successful run creates.
fn only_backup_dir(backup_root: &Path) -> PathBuf {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(backup_root)
        .expect("backup root exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(
        dirs.len(),
        1,
        "expected exactly one backup dir in {backup_root:?}"
    );
    dirs.pop().expect("one dir")
}

macro_rules! require_tools {
    () => {
        if !have_tool("sqlite3") || !have_tool("bash") {
            eprintln!("skipping: bash and sqlite3 are required for ADR-2017 tests");
            return;
        }
    };
}

/// Baseline: a complete source directory backs up cleanly, every declared
/// database is captured and the manifest records the membership contract.
#[test]
fn complete_source_backs_up_every_declared_database() {
    require_tools!();
    let scratch = Scratch::new("complete");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &[]);

    let out = run_backup(&data, &backups, &[]);
    assert!(out.status.success(), "run failed:\n{}", stderr(&out));

    let dest = only_backup_dir(&backups);
    for db in [
        "settings.sqlite3",
        "enrichment.sqlite3",
        "kpi.sqlite3",
        "shares.sqlite3",
        "liveness.sqlite3",
    ] {
        assert!(dest.join(db).is_file(), "{db} was not captured");
    }
    let manifest = std::fs::read_to_string(dest.join("MANIFEST.txt")).expect("manifest");
    assert!(manifest.contains("databases=5"), "manifest: {manifest}");
    assert!(
        manifest.contains(
            "required_dbs=settings.sqlite3 enrichment.sqlite3 kpi.sqlite3 shares.sqlite3\n"
        ),
        "manifest must record the required membership: {manifest}"
    );
    assert!(
        manifest.contains("restore_check=application-level"),
        "manifest must record the restore check: {manifest}"
    );
}

/// ADR-2017 defect 1 — the reproduced case. A missing REQUIRED member must
/// fail the run; the old script exited zero.
#[test]
fn missing_required_database_fails_the_run() {
    require_tools!();
    let scratch = Scratch::new("missing-required");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &["settings.sqlite3"]);

    let out = run_backup(&data, &backups, &[]);
    assert!(
        !out.status.success(),
        "a missing required database must fail the run; stderr:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("required database(s) missing"),
        "expected the membership failure message, got:\n{err}"
    );
    assert!(
        err.contains("settings.sqlite3"),
        "the failure must name the missing member:\n{err}"
    );

    // A failed run must not leave a partial backup set that looks restorable.
    let leftovers: Vec<PathBuf> = std::fs::read_dir(&backups)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "a failed run must publish no backup dir, found {leftovers:?}"
    );
}

/// A missing OPTIONAL member is a skip, not a failure, and the manifest says so.
#[test]
fn missing_optional_database_still_succeeds() {
    require_tools!();
    let scratch = Scratch::new("missing-optional");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &["liveness.sqlite3"]);

    let out = run_backup(&data, &backups, &[]);
    assert!(
        out.status.success(),
        "a missing optional database must not fail the run:\n{}",
        stderr(&out)
    );
    let dest = only_backup_dir(&backups);
    let manifest = std::fs::read_to_string(dest.join("MANIFEST.txt")).expect("manifest");
    assert!(manifest.contains("databases=4"), "manifest: {manifest}");
    assert!(
        manifest.contains("missing_optional=liveness.sqlite3"),
        "manifest must record the absent optional member: {manifest}"
    );
}

/// Reclassifying a member moves it across the boundary: with liveness declared
/// required, its absence now fails.
#[test]
fn membership_declaration_is_authoritative() {
    require_tools!();
    let scratch = Scratch::new("reclassify");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &["liveness.sqlite3"]);

    let out = run_backup(
        &data,
        &backups,
        &[
            ("REQUIRED_DBS", "settings.sqlite3 liveness.sqlite3"),
            ("OPTIONAL_DBS", "enrichment.sqlite3 kpi.sqlite3"),
        ],
    );
    assert!(
        !out.status.success(),
        "liveness declared required must fail when absent:\n{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("liveness.sqlite3"));
}

/// ADR-2017 defect 2 — the default destination must not sit inside the source
/// data directory, and an explicit inside-source destination is refused.
#[test]
fn destination_inside_the_source_directory_is_refused() {
    require_tools!();
    let scratch = Scratch::new("dest-guard");
    let data = scratch.path().join("data");
    let backups = data.join("backups"); // deliberately inside the source
    seed_data_dir(&data, &[]);

    let out = run_backup(&data, &backups, &[]);
    assert!(
        !out.status.success(),
        "a destination inside the source directory must be refused:\n{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("inside the source data directory"),
        "expected the failure-domain message, got:\n{}",
        stderr(&out)
    );
}

/// The guard is an explicit-override, not a hard block: operators who really
/// want an inside-source destination can still have one, loudly.
#[test]
fn inside_source_destination_can_be_overridden_explicitly() {
    require_tools!();
    let scratch = Scratch::new("dest-override");
    let data = scratch.path().join("data");
    let backups = data.join("backups");
    seed_data_dir(&data, &[]);

    let out = run_backup(&data, &backups, &[("ALLOW_BACKUP_INSIDE_DATA", "1")]);
    assert!(out.status.success(), "override failed:\n{}", stderr(&out));
    assert!(
        stderr(&out).contains("share a failure domain"),
        "the override must still warn:\n{}",
        stderr(&out)
    );
}

/// The shipped default `BACKUP_ROOT` resolves outside `./data`.
#[test]
fn default_backup_root_is_outside_the_data_directory() {
    let source = std::fs::read_to_string(script()).expect("read backup script");
    let line = source
        .lines()
        .find(|l| l.trim_start().starts_with("BACKUP_ROOT=\"${BACKUP_ROOT:-"))
        .expect("BACKUP_ROOT default is declared");
    assert!(
        !line.contains("./data/"),
        "the default destination must not live under ./data: {line}"
    );
}

/// ADR-2017 defect 3 — `integrity_check` alone is not a restore check. A
/// page-clean but schema-less database passes integrity_check and must still
/// fail the application-level probe.
#[test]
fn page_clean_but_schemaless_database_fails_the_restore_check() {
    require_tools!();
    let scratch = Scratch::new("restore-check");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &[]);

    // Replace settings.sqlite3 with a valid, empty SQLite file: page-consistent
    // (integrity_check says ok) but carrying none of the application tables.
    let settings = data.join("settings.sqlite3");
    std::fs::remove_file(&settings).expect("remove fixture");
    make_db(&settings, &["unrelated_table"]);

    let out = run_backup(&data, &backups, &[]);
    assert!(
        !out.status.success(),
        "a schema-less snapshot must fail the restore check:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("restore check failed for settings.sqlite3"),
        "expected the restore-check failure, got:\n{err}"
    );
    assert!(
        err.contains("'settings' is not queryable"),
        "the failure must name the missing table:\n{err}"
    );
}

/// The restore check reports live row counts for the tables it probes, which is
/// what makes it an application-level check rather than a file test.
#[test]
fn restore_check_reports_application_row_counts() {
    require_tools!();
    let scratch = Scratch::new("restore-rows");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &[]);

    let insert = Command::new("sqlite3")
        .arg(data.join("kpi.sqlite3"))
        .arg("INSERT INTO kpi_snapshots (id) VALUES (1),(2),(3);")
        .output()
        .expect("seed rows");
    assert!(insert.status.success());

    let out = run_backup(&data, &backups, &[]);
    assert!(out.status.success(), "run failed:\n{}", stderr(&out));
    assert!(
        stderr(&out).contains("restore-check kpi.sqlite3: kpi_snapshots rows=3"),
        "expected the restored row count, got:\n{}",
        stderr(&out)
    );
}

/// Every declared database has a declared restore schema: a backup that copies
/// a file the restore check never opens has not shown it can be restored.
/// shares.sqlite3 joined REQUIRED_DBS in 238f5960d without one and was skipped.
#[test]
fn every_declared_database_is_restore_checked() {
    require_tools!();
    let scratch = Scratch::new("restore-all");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &[]);

    let insert = Command::new("sqlite3")
        .arg(data.join("shares.sqlite3"))
        .arg("INSERT INTO shared_graphs (id) VALUES (1),(2);")
        .output()
        .expect("seed rows");
    assert!(insert.status.success());

    let out = run_backup(&data, &backups, &[]);
    let err = stderr(&out);
    assert!(out.status.success(), "run failed:\n{err}");
    assert!(
        !err.contains("no declared schema, skipped"),
        "a declared database skipped its restore check:\n{err}"
    );
    assert!(
        err.contains("restore-check shares.sqlite3: shared_graphs rows=2"),
        "expected the share store's restored row count, got:\n{err}"
    );
}

/// The restore check is opt-out for the rare case where sqlite3 is unavailable
/// on the backup host; opting out is recorded in the manifest.
#[test]
fn restore_check_can_be_disabled_and_is_recorded() {
    require_tools!();
    let scratch = Scratch::new("no-verify");
    let data = scratch.path().join("data");
    let backups = scratch.path().join("backups");
    seed_data_dir(&data, &[]);

    let out = run_backup(&data, &backups, &[("VERIFY_RESTORE", "0")]);
    assert!(out.status.success(), "run failed:\n{}", stderr(&out));
    let dest = only_backup_dir(&backups);
    let manifest = std::fs::read_to_string(dest.join("MANIFEST.txt")).expect("manifest");
    assert!(
        manifest.contains("restore_check=skipped"),
        "the manifest must record that the check was skipped: {manifest}"
    );
}
