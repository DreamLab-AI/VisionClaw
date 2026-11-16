//! SQLite online-backup snapshots and content digests.

use anyhow::{bail, Context, Result};
use rusqlite::backup::Backup;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// Pages copied per step of the online backup.
const PAGES_PER_STEP: i32 = 256;
/// Pause between steps, so a busy writer is not starved.
const STEP_PAUSE: Duration = Duration::from_millis(20);

/// Facts about a finished snapshot.
pub(crate) struct SnapshotFacts {
    pub tables: BTreeMap<String, u64>,
    pub content_sha256: String,
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn user_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let names = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(names.collect::<Result<Vec<_>, _>>()?)
}

fn integrity_ok(conn: &Connection, path: &Path) -> Result<()> {
    let verdict: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if verdict != "ok" {
        bail!("integrity_check on {} returned: {verdict}", path.display());
    }
    Ok(())
}

/// Row count per user table.
fn row_counts(conn: &Connection) -> Result<BTreeMap<String, u64>> {
    let mut counts = BTreeMap::new();
    for table in user_tables(conn)? {
        let n: i64 = conn.query_row(
            &format!("SELECT count(*) FROM {}", quote(&table)),
            [],
            |row| row.get(0),
        )?;
        counts.insert(table, n as u64);
    }
    Ok(counts)
}

/// Digest over the schema and every row of every user table, independent of
/// file layout: tables in name order, rows ordered by all columns, each value
/// tagged with its type and length.
///
/// # Errors
///
/// Fails when the database cannot be opened or read.
pub fn sqlite_content_digest(path: &Path) -> Result<String> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    digest_connection(&conn)
}

fn digest_connection(conn: &Connection) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut field = |tag: u8, bytes: &[u8]| {
        hasher.update([tag]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    for table in user_tables(conn)? {
        let sql: String = conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [&table],
            |row| row.get(0),
        )?;
        field(b'T', table.as_bytes());
        field(b'S', sql.as_bytes());
        let columns = conn.prepare(&format!("SELECT * FROM {} LIMIT 0", quote(&table)))?;
        let width = columns.column_count();
        if width == 0 {
            continue;
        }
        let order: Vec<String> = (1..=width).map(|i| i.to_string()).collect();
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM {} ORDER BY {}",
            quote(&table),
            order.join(", ")
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            for i in 0..width {
                match row.get_ref(i)? {
                    ValueRef::Null => field(b'N', &[]),
                    ValueRef::Integer(v) => field(b'I', &v.to_le_bytes()),
                    ValueRef::Real(v) => field(b'R', &v.to_bits().to_le_bytes()),
                    ValueRef::Text(v) => field(b'X', v),
                    ValueRef::Blob(v) => field(b'B', v),
                }
            }
            field(b'E', &[]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Snapshots `src` into `dst` with SQLite's online backup API, then checks the
/// snapshot and records its row counts and content digest.
///
/// The source is opened without the create flag, so a missing file is an error
/// rather than a new empty database.
pub(crate) fn snapshot(src: &Path, dst: &Path) -> Result<SnapshotFacts> {
    let source = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", src.display()))?;
    {
        let mut target =
            Connection::open(dst).with_context(|| format!("creating {}", dst.display()))?;
        let backup = Backup::new(&source, &mut target)?;
        backup
            .run_to_completion(PAGES_PER_STEP, STEP_PAUSE, None)
            .with_context(|| format!("online backup of {}", src.display()))?;
        drop(backup);
        // One self-contained file: no -wal or -shm sidecar to lose or mis-hash.
        let mode: String = target.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
        if mode != "delete" {
            bail!("could not leave {} in rollback-journal mode", dst.display());
        }
    }
    let check = Connection::open_with_flags(dst, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    integrity_ok(&check, dst)?;
    Ok(SnapshotFacts {
        tables: row_counts(&check)?,
        content_sha256: digest_connection(&check)?,
    })
}

/// Checks a restored database against the manifest's facts.
pub(crate) fn check_restored(
    path: &Path,
    tables: &BTreeMap<String, u64>,
    digest: &str,
) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    integrity_ok(&conn, path)?;
    let counts = row_counts(&conn)?;
    if &counts != tables {
        bail!(
            "row counts of {} differ from the manifest: {counts:?} vs {tables:?}",
            path.display()
        );
    }
    let actual = digest_connection(&conn)?;
    if actual != digest {
        bail!(
            "content digest of {} differs from the manifest",
            path.display()
        );
    }
    Ok(())
}
