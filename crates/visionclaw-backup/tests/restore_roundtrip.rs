//! The restore test (ADR-2103 acceptance, G-7): back up synthetic stores, restore
//! them into an empty directory and prove the contents are equal. It runs without
//! the live system and touches nothing outside its temporary directories.

use oxigraph::io::RdfFormat;
use oxigraph::model::{BlankNode, GraphName, Literal, NamedNode, Quad};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use visionclaw_backup::{
    backup, restore, sqlite_content_digest, verify, BackupRequest, MANIFEST_FILE, OPTIONAL_SQLITE,
    REQUIRED_SQLITE,
};

const PROVENANCE: &str = "urn:graph:provenance";
const PROV_USED: &str = "http://www.w3.org/ns/prov#used";

fn iri(s: &str) -> NamedNode {
    NamedNode::new(s).expect("valid IRI")
}

/// Writes an Oxigraph store with a default-graph triple, three provenance quads
/// (one literal with a newline and a quote) and then closes it.
fn make_oxigraph(dir: &Path) {
    let store = Store::open(dir).expect("open store");
    store
        .insert(&Quad::new(
            iri("urn:node:a"),
            iri("http://www.w3.org/2000/01/rdf-schema#label"),
            Literal::new_simple_literal("alpha"),
            GraphName::DefaultGraph,
        ))
        .expect("insert");
    for n in 1..=3 {
        store
            .insert(&Quad::new(
                iri(&format!("urn:activity:{n}")),
                iri(PROV_USED),
                iri(&format!("urn:entity:{n}")),
                GraphName::NamedNode(iri(PROVENANCE)),
            ))
            .expect("insert");
    }
    // OWL restrictions and rdf:List cells are blank nodes; they must survive.
    let blank = BlankNode::new("restriction1").expect("blank node");
    for (predicate, object) in [
        ("http://www.w3.org/2002/07/owl#onProperty", iri(PROV_USED)),
        (
            "http://www.w3.org/2002/07/owl#someValuesFrom",
            iri("urn:entity:1"),
        ),
    ] {
        store
            .insert(&Quad::new(
                blank.clone(),
                iri(predicate),
                object,
                GraphName::NamedNode(iri(PROVENANCE)),
            ))
            .expect("insert");
    }
    store
        .insert(&Quad::new(
            iri("urn:activity:1"),
            iri("urn:p:note"),
            Literal::new_simple_literal("two\nlines and a \"quote\" and é"),
            GraphName::NamedNode(iri(PROVENANCE)),
        ))
        .expect("insert");
}

/// Creates a WAL-mode database with `rows` rows and returns the open connection,
/// so the caller can keep it (and its WAL) alive during the backup.
fn make_sqlite(path: &Path, rows: i64) -> Connection {
    let conn = Connection::open(path).expect("open sqlite");
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("wal");
    conn.execute_batch(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT, weight REAL, blob BLOB);
         CREATE TABLE empty_table (x INTEGER);",
    )
    .expect("schema");
    for i in 0..rows {
        conn.execute(
            "INSERT INTO items (label, weight, blob) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                format!("{} item {i}", path.display()),
                i as f64 * 1.5,
                vec![i as u8; 4]
            ],
        )
        .expect("insert");
    }
    conn
}

struct Fixture {
    _root: tempfile::TempDir,
    data: std::path::PathBuf,
    dest: std::path::PathBuf,
    /// Open connections whose committed rows may still sit in the WAL.
    _writers: Vec<Connection>,
}

fn fixture(include_optional: bool, skip_required: Option<&str>) -> Fixture {
    let root = tempfile::tempdir().expect("tempdir");
    let data = root.path().join("data");
    let dest = root.path().join("nas");
    fs::create_dir_all(&data).expect("mkdir");
    make_oxigraph(&data.join("oxigraph"));
    let mut writers = Vec::new();
    for (i, name) in REQUIRED_SQLITE.iter().enumerate() {
        if Some(*name) != skip_required {
            writers.push(make_sqlite(&data.join(name), 20 + i as i64));
        }
    }
    if include_optional {
        for name in OPTIONAL_SQLITE {
            writers.push(make_sqlite(&data.join(name), 5));
        }
    }
    Fixture {
        _root: root,
        data,
        dest,
        _writers: writers,
    }
}

fn request(f: &Fixture, keep: usize) -> BackupRequest {
    BackupRequest {
        data_dir: f.data.clone(),
        oxigraph_dir: None,
        dest_root: f.dest.clone(),
        keep,
    }
}

/// Quads of the store with blank-node labels replaced by a signature of the
/// statements the node takes part in. A reload mints fresh blank-node labels
/// (labels carry no meaning in RDF), so equality here means isomorphic: the same
/// nodes, in the same graphs, joined the same way.
fn quads_of(dir: &Path) -> BTreeSet<String> {
    let lines = raw_quads_of(dir);
    let label = |line: &str| -> Option<String> {
        line.strip_prefix("_:")
            .and_then(|rest| rest.split(' ').next())
            .map(str::to_owned)
    };
    let signature = |blank: &str| -> String {
        let mut own: Vec<String> = lines
            .iter()
            .filter(|l| label(l).as_deref() == Some(blank))
            .map(|l| l.replacen(&format!("_:{blank}"), "_:SELF", 1))
            .collect();
        own.sort();
        own.join("|")
    };
    lines
        .iter()
        .map(|line| match label(line) {
            Some(blank) => {
                format!("[{}] {line}", signature(&blank)).replacen(&format!("_:{blank}"), "_:b", 1)
            }
            None => line.clone(),
        })
        .collect()
}

fn raw_quads_of(dir: &Path) -> Vec<String> {
    let store = Store::open(dir).expect("open store");
    let dump = store
        .dump_to_writer(RdfFormat::NQuads, Vec::new())
        .expect("dump");
    String::from_utf8(dump)
        .expect("utf8")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn provenance_count(dir: &Path) -> usize {
    let store = Store::open(dir).expect("open store");
    let query = format!(
        "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{PROVENANCE}> {{ ?s <{PROV_USED}> ?o }} }}"
    );
    #[allow(deprecated)]
    let QueryResults::Solutions(mut rows) = store.query(&query).expect("query") else {
        panic!("expected solutions");
    };
    let row = rows.next().expect("one row").expect("row ok");
    row.get("n")
        .expect("n")
        .to_string()
        .split('"')
        .nth(1)
        .expect("literal")
        .parse()
        .expect("number")
}

fn complete_runs(dest: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dest)
        .expect("read dest")
        .flatten()
        .filter(|e| e.path().join(MANIFEST_FILE).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn backup_then_restore_reproduces_every_store() {
    let f = fixture(true, None);
    let original_quads = quads_of(&f.data.join("oxigraph"));
    assert_eq!(original_quads.len(), 7, "fixture quad count");
    assert_eq!(provenance_count(&f.data.join("oxigraph")), 3);

    let outcome = backup(&request(&f, 14)).expect("backup");
    assert_eq!(
        outcome.manifest.artefacts.len(),
        1 + REQUIRED_SQLITE.len() + 1
    );
    assert!(outcome.manifest.absent_optional.is_empty());
    verify(&outcome.run_dir).expect("verify");

    let target = f.dest.join("restored");
    let (_, manifest) = restore(&f.dest, &target).expect("restore from latest");
    assert_eq!(manifest, outcome.manifest);

    // Oxigraph: same quads (every graph), same count, same sample query.
    assert_eq!(quads_of(&target.join("oxigraph")), original_quads);
    assert_eq!(provenance_count(&target.join("oxigraph")), 3);

    // SQLite: same content digest, so same rows, and the same row counts.
    for name in REQUIRED_SQLITE.iter().chain(OPTIONAL_SQLITE.iter()) {
        let original = f.data.join(name);
        let restored = target.join(name);
        assert_eq!(
            sqlite_content_digest(&restored).expect("restored digest"),
            sqlite_content_digest(&original).expect("original digest"),
            "{name} content"
        );
        let count = |p: &Path| -> i64 {
            Connection::open(p)
                .and_then(|c| c.query_row("SELECT count(*) FROM items", [], |r| r.get(0)))
                .expect("count")
        };
        assert_eq!(count(&restored), count(&original), "{name} rows");
    }
}

#[test]
fn missing_optional_store_is_recorded_and_does_not_fail() {
    let f = fixture(false, None);
    let outcome = backup(&request(&f, 14)).expect("backup");
    assert_eq!(
        outcome.manifest.absent_optional,
        vec!["liveness.sqlite3".to_owned()]
    );
}

#[test]
fn missing_required_store_fails_and_publishes_nothing() {
    let f = fixture(true, Some("kpi.sqlite3"));
    let err = backup(&request(&f, 14)).expect_err("must fail");
    assert!(format!("{err:#}").contains("kpi.sqlite3"), "{err:#}");
    assert!(complete_runs(&f.dest).is_empty());
    let leftovers: Vec<_> = fs::read_dir(&f.dest).expect("dest").flatten().collect();
    assert!(leftovers.is_empty(), "no partial folder may remain");
}

#[test]
fn oxigraph_held_by_a_running_server_fails_by_name() {
    let f = fixture(true, None);
    let _server = Store::open(f.data.join("oxigraph")).expect("hold the store like a server");
    let err = backup(&request(&f, 14)).expect_err("must fail");
    assert!(format!("{err:#}").contains("oxigraph"), "{err:#}");
    assert!(
        err.chain()
            .last()
            .expect("cause")
            .to_string()
            .to_lowercase()
            .contains("lock"),
        "the cause must be the lock, not corruption: {err:#}"
    );
    assert!(complete_runs(&f.dest).is_empty());
}

#[test]
fn a_checkpoint_directory_can_be_backed_up_while_the_store_stays_open() {
    let f = fixture(true, None);
    let server = Store::open(f.data.join("oxigraph")).expect("hold the store like a server");
    let checkpoint = f.dest.parent().expect("parent").join("checkpoint");
    server.backup(&checkpoint).expect("server-side checkpoint");
    let mut req = request(&f, 14);
    req.oxigraph_dir = Some(checkpoint);
    let outcome = backup(&req).expect("backup of checkpoint beside a live store");
    assert_eq!(
        outcome.manifest.artefacts.len(),
        1 + REQUIRED_SQLITE.len() + 1
    );
}

#[test]
fn verify_detects_an_altered_artefact() {
    let f = fixture(true, None);
    let outcome = backup(&request(&f, 14)).expect("backup");
    let victim = outcome.run_dir.join("oxigraph.nq");
    let mut bytes = fs::read(&victim).expect("read");
    bytes[0] ^= 0x01;
    fs::write(&victim, bytes).expect("tamper");
    let err = verify(&outcome.run_dir).expect_err("must fail");
    assert!(format!("{err:#}").contains("oxigraph.nq"), "{err:#}");
}

#[test]
fn restore_refuses_a_non_empty_target_and_leaves_it_alone() {
    let f = fixture(true, None);
    let outcome = backup(&request(&f, 14)).expect("backup");
    let target = f.dest.join("occupied");
    fs::create_dir(&target).expect("mkdir");
    fs::write(target.join("keep.txt"), b"mine").expect("write");
    restore(&outcome.run_dir, &target).expect_err("must refuse");
    assert_eq!(fs::read(target.join("keep.txt")).expect("read"), b"mine");
}

#[test]
fn destination_inside_the_data_directory_is_refused() {
    let f = fixture(true, None);
    let mut req = request(&f, 14);
    req.dest_root = f.data.join("backups");
    let err = backup(&req).expect_err("must refuse");
    assert!(format!("{err:#}").contains("failure domain"), "{err:#}");
}

#[test]
fn retention_keeps_the_newest_runs_and_ignores_partial_folders() {
    let f = fixture(true, None);
    fs::create_dir_all(f.dest.join(".20200101T000000Z.partial")).expect("stale partial");
    let mut last = String::new();
    for _ in 0..3 {
        last = backup(&request(&f, 2))
            .expect("backup")
            .run_dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        sleep(Duration::from_millis(1100));
    }
    let runs = complete_runs(&f.dest);
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs.last(), Some(&last), "newest run survives");
    assert!(f.dest.join(".20200101T000000Z.partial").exists());
}
