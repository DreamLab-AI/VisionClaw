//! ADR-2147: the certificate `vault` writes is the format the independent
//! checker reads, and the checker refuses a forged line.
//!
//! The checker is `oo-cert` (Open Ontologies, Lean 4, theorem
//! `OOCert.certificate_sound`). It shares no code with this crate, which is
//! the point, so it is not a build dependency: set `OO_CERT_BIN` to the
//! binary to run the two checks, otherwise they are skipped and say so. The
//! ADR's Verification section records a run on the published corpus.

use std::path::Path;
use std::process::Command;

use vault::build::turtle::{Graph, Subject, Term};
use vault::certificate::certify;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
const SUBPROP: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";

fn fixture() -> Graph {
    let mut g = Graph::new();
    let iri = |l: &str| Term::Iri(format!("urn:ex:{l}"));
    g.add_iri("urn:ex:espresso", SUBCLASS, iri("coffee"));
    g.add_iri("urn:ex:coffee", SUBCLASS, iri("drink"));
    g.add_iri("urn:ex:cup", RDF_TYPE, iri("espresso"));
    g.add_iri("urn:ex:serves", DOMAIN, iri("cafe"));
    g.add_iri("urn:ex:pours", SUBPROP, iri("serves"));
    g.add_iri("urn:ex:bar", "urn:ex:pours", iri("cup"));
    g.add_iri(
        "urn:ex:cup",
        "http://www.w3.org/2000/01/rdf-schema#label",
        Term::Literal {
            value: "my cup \"of\" joy\n".into(),
            lang: Some("en".into()),
            datatype: None,
        },
    );
    let r = g.fresh_blank();
    g.add_iri("urn:ex:drink", SUBCLASS, Term::Blank(r.clone()));
    g.add(
        Subject::Blank(r),
        "http://www.w3.org/2002/07/owl#someValuesFrom",
        iri("liquid"),
    );
    g
}

fn checker() -> Option<String> {
    let bin = std::env::var("OO_CERT_BIN").ok()?;
    if Path::new(&bin).is_file() {
        Some(bin)
    } else {
        eprintln!("OO_CERT_BIN={bin} is not a file; skipping");
        None
    }
}

fn run(bin: &str, dir: &Path) -> (bool, String) {
    let out = Command::new(bin)
        .arg(dir.join("asserted.tsv"))
        .arg(dir.join("derivations.tsv"))
        .output()
        .expect("oo-cert runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn the_independent_checker_accepts_what_vault_writes() {
    let Some(bin) = checker() else {
        eprintln!("OO_CERT_BIN unset; skipping the independent check");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let cert = certify(&fixture());
    let summary = cert.write(dir.path(), "test").unwrap();
    assert!(summary.derivations > 0);
    let (ok, stdout) = run(&bin, dir.path());
    assert!(ok, "oo-cert refused a genuine certificate: {stdout}");
    assert!(stdout.contains("\"ok\":true"), "{stdout}");
    assert!(
        stdout.contains(&format!("\"derivations\":{}", summary.derivations)),
        "{stdout}"
    );
}

#[test]
fn the_independent_checker_refuses_a_forged_conclusion() {
    let Some(bin) = checker() else {
        eprintln!("OO_CERT_BIN unset; skipping the independent check");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    certify(&fixture()).write(dir.path(), "test").unwrap();
    let path = dir.path().join("derivations.tsv");
    let text = std::fs::read_to_string(&path).unwrap();
    // Keep the premises, change the first conclusion's object to a class
    // nothing supports.
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut fields: Vec<String> = lines[0].split('\t').map(str::to_owned).collect();
    fields[3] = "<urn:ex:beer>".to_owned();
    lines[0] = fields.join("\t");
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    let (ok, stdout) = run(&bin, dir.path());
    assert!(!ok, "oo-cert accepted a forged line: {stdout}");
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
    assert!(stdout.contains("\"first_rejected\":0"), "{stdout}");
}
