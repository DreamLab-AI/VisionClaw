//! End to end: the `temporal-refit` binary against an on-disk RocksDB store in
//! the v1 form. Dry run by default (read-only open, nothing written), a refusal
//! on a wrong expected count, `--apply`, then a second run that finds nothing.

use std::process::{Command, Output};

use chrono::{DateTime, TimeZone, Utc};
use oxigraph::model::vocab::xsd;
use oxigraph::model::{Literal, NamedNode, Quad};
use oxigraph::store::Store;

use visionclaw_adapters::corpus_history::{
    corpus_sync_activity_urn, CORPUS_SYNC_WRITER, CORPUS_SYNC_WRITER_PREDICATE,
};
use visionclaw_adapters::provenance_emitter::{build_activity_quads, ActivityRecord};
use visionclaw_adapters::provenance_writer::{DL_NS, GRAPH_PROVENANCE};

const AGENT: &str = "did:nostr:11ed64225dd5e2c5e18f61ad43d5ad9272d08739d3a20dd25886197b0738663c";

fn stamp(at: &DateTime<Utc>) -> Literal {
    Literal::new_typed_literal(
        at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        xsd::DATE_TIME,
    )
}

/// A v1 store: `n` versions opened at the first sync, the first `closed` closed
/// by a bare `dl:validTo` at the second.
fn write_v1(store: &Store, n: u32, closed: u32) {
    let t1 = Utc.with_ymd_and_hms(2026, 11, 19, 13, 26, 13).unwrap();
    let t2 = Utc.with_ymd_and_hms(2026, 11, 20, 8, 55, 6).unwrap();
    let g = NamedNode::new_unchecked(GRAPH_PROVENANCE);
    let dl = |t: &str| NamedNode::new_unchecked(format!("{DL_NS}{t}"));
    let prov = |t: &str| NamedNode::new_unchecked(format!("http://www.w3.org/ns/prov#{t}"));
    let rdf = |t: &str| {
        NamedNode::new_unchecked(format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#{t}"))
    };
    for at in [t1, t2] {
        let record = ActivityRecord {
            activity_urn: corpus_sync_activity_urn(&at),
            agent_did: AGENT.to_string(),
            timestamp: at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            action: "ingest".to_string(),
            derivation: "asserted".to_string(),
            used: None,
            generated: None,
            informed_by: None,
        };
        store
            .extend(build_activity_quads(&record).unwrap())
            .unwrap();
        store
            .insert(&Quad::new(
                NamedNode::new_unchecked(record.activity_urn),
                NamedNode::new_unchecked(CORPUS_SYNC_WRITER_PREDICATE),
                Literal::new_simple_literal(CORPUS_SYNC_WRITER),
                g.clone(),
            ))
            .unwrap();
    }
    let activity = NamedNode::new_unchecked(corpus_sync_activity_urn(&t1));
    for i in 0..n {
        let e = NamedNode::new_unchecked(format!(
            "urn:agentbox:event:{}:sha256-12-{i:012x}",
            &AGENT[10..]
        ));
        let put = |p: NamedNode, o: oxigraph::model::Term| {
            store
                .insert(&Quad::new(e.clone(), p, o, g.clone()))
                .unwrap();
        };
        put(rdf("type"), prov("Entity").into());
        put(
            rdf("subject"),
            NamedNode::new_unchecked(format!("urn:ngm:class:s{i}")).into(),
        );
        put(
            rdf("predicate"),
            NamedNode::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subClassOf").into(),
        );
        put(
            rdf("object"),
            NamedNode::new_unchecked("urn:ngm:class:o").into(),
        );
        put(dl("validFrom"), stamp(&t1).into());
        put(prov("generatedAtTime"), stamp(&t1).into());
        put(prov("wasGeneratedBy"), activity.clone().into());
        put(
            prov("wasAttributedTo"),
            NamedNode::new_unchecked(AGENT).into(),
        );
        if i < closed {
            put(dl("validTo"), stamp(&t2).into());
        }
    }
}

fn refit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_temporal-refit"))
        .args(args)
        .output()
        .expect("run temporal-refit")
}

fn out(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn quad_count(path: &str) -> usize {
    Store::open_read_only(path).unwrap().len().unwrap()
}

#[test]
fn the_binary_dry_runs_refuses_applies_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oxigraph");
    let path = path.to_str().unwrap();
    {
        let store = Store::open(path).unwrap();
        write_v1(&store, 12, 3);
    }
    let before = quad_count(path);

    // Dry run is the default and writes nothing.
    let dry = refit(&[path, "--expect-versions", "12", "--expect-closes", "3"]);
    let text = out(&dry);
    assert!(dry.status.success(), "{text}");
    assert!(
        text.contains("DRY RUN") && text.contains("versions: 12"),
        "{text}"
    );
    assert_eq!(quad_count(path), before, "a dry run must not write");

    // A wrong expected count is refused with exit status 1 and nothing is written.
    let refused = refit(&[
        path,
        "--apply",
        "--expect-versions",
        "13",
        "--expect-closes",
        "3",
        "--expect-other-activities",
        "0",
        "--expect-residue",
        "0",
    ]);
    assert_eq!(refused.status.code(), Some(1), "{}", out(&refused));
    assert!(out(&refused).contains("expected 13 versions, found 12"));
    assert_eq!(quad_count(path), before);

    // A wrong residue count is refused too.
    let residue = refit(&[
        path,
        "--apply",
        "--expect-versions",
        "12",
        "--expect-closes",
        "3",
        "--expect-other-activities",
        "0",
        "--expect-residue",
        "10",
    ]);
    assert_eq!(residue.status.code(), Some(1), "{}", out(&residue));
    assert!(
        out(&residue).contains("expected 10 residue versions, found 0"),
        "{}",
        out(&residue)
    );
    assert_eq!(quad_count(path), before);

    // --apply without every count is a usage error and writes nothing.
    let bare = refit(&[path, "--apply"]);
    assert_eq!(bare.status.code(), Some(2), "{}", out(&bare));
    assert!(out(&bare).contains("--expect-residue"), "{}", out(&bare));
    let partial = refit(&[
        path,
        "--apply",
        "--expect-versions",
        "12",
        "--expect-closes",
        "3",
    ]);
    assert_eq!(partial.status.code(), Some(2), "{}", out(&partial));
    assert_eq!(quad_count(path), before);

    // --apply rewrites the store.
    let applied = refit(&[
        path,
        "--apply",
        "--expect-versions",
        "12",
        "--expect-closes",
        "3",
        "--expect-other-activities",
        "0",
        "--expect-residue",
        "0",
    ]);
    let text = out(&applied);
    assert!(applied.status.success(), "{text}");
    assert!(
        text.contains("REFIT COMPLETE") && !text.contains("[FAIL]"),
        "{text}"
    );

    // A second run, even a read-only dry run, finds nothing to do.
    let again = refit(&[path]);
    assert!(again.status.success());
    assert!(out(&again).contains("nothing to do"), "{}", out(&again));
    assert!(out(&again).contains("v2 versions"), "{}", out(&again));

    // Usage errors exit 2.
    assert_eq!(refit(&[]).status.code(), Some(2));
    assert_eq!(refit(&[path, "--bogus"]).status.code(), Some(2));
    assert_eq!(refit(&["/nonexistent/store"]).status.code(), Some(2));
}

const ALL_COUNTS: [&str; 8] = [
    "--expect-versions",
    "0",
    "--expect-closes",
    "0",
    "--expect-other-activities",
    "0",
    "--expect-residue",
    "0",
];

#[test]
fn a_mistyped_path_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let typo = dir.path().join("oxigraph-typo");
    let typo = typo.to_str().unwrap();
    let mut args = vec![typo, "--apply"];
    args.extend(ALL_COUNTS);
    let applied = refit(&args);
    assert_eq!(applied.status.code(), Some(2), "{}", out(&applied));
    assert!(
        !std::path::Path::new(typo).exists(),
        "the typo path was created"
    );
    // The dry run is no different.
    let dry = refit(&[typo]);
    assert_eq!(dry.status.code(), Some(2), "{}", out(&dry));
    assert!(!std::path::Path::new(typo).exists());
    // An existing directory that is not a store is not turned into one.
    let plain = dir.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let mut args = vec![plain.to_str().unwrap(), "--apply"];
    args.extend(ALL_COUNTS);
    let applied = refit(&args);
    assert_eq!(applied.status.code(), Some(2), "{}", out(&applied));
    assert_eq!(std::fs::read_dir(&plain).unwrap().count(), 0);
}

#[test]
fn an_empty_store_and_a_foreign_store_are_refused_not_nothing_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    let foreign = dir.path().join("foreign");
    Store::open(&empty).unwrap();
    {
        let store = Store::open(&foreign).unwrap();
        store
            .insert(&Quad::new(
                NamedNode::new_unchecked("urn:x:a"),
                NamedNode::new_unchecked("urn:x:b"),
                NamedNode::new_unchecked("urn:x:c"),
                NamedNode::new_unchecked("urn:x:g"),
            ))
            .unwrap();
    }
    for (path, mode) in [(&empty, None), (&foreign, None), (&empty, Some("apply"))] {
        let path = path.to_str().unwrap();
        let before = quad_count(path);
        let mut args = vec![path];
        if mode.is_some() {
            args.push("--apply");
            args.extend(ALL_COUNTS);
        }
        let run = refit(&args);
        assert_eq!(run.status.code(), Some(1), "{path}: {}", out(&run));
        assert!(
            out(&run).contains("wrong or empty store path"),
            "{}",
            out(&run)
        );
        assert!(!out(&run).contains("nothing to do"), "{}", out(&run));
        assert_eq!(quad_count(path), before);
    }
}

#[test]
fn a_v1_entity_the_tool_cannot_read_fails_the_dry_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oxigraph");
    let path = path.to_str().unwrap();
    {
        let store = Store::open(path).unwrap();
        write_v1(&store, 12, 3);
        // An entity with rdf:subject and dl:validFrom but no agent, and a
        // dl:validTo on something that is not a version.
        let g = NamedNode::new_unchecked(GRAPH_PROVENANCE);
        let e = NamedNode::new_unchecked("urn:agentbox:event:odd:sha256-12-eeeeeeeeeeee");
        for (p, o) in [
            (
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#subject",
                NamedNode::new_unchecked("urn:x:s"),
            ),
            (
                &format!("{DL_NS}validFrom") as &str,
                NamedNode::new_unchecked("urn:x:ignored"),
            ),
        ] {
            store
                .insert(&Quad::new(
                    e.clone(),
                    NamedNode::new_unchecked(p),
                    o,
                    g.clone(),
                ))
                .unwrap();
        }
    }
    let before = quad_count(path);
    let dry = refit(&[path, "--expect-versions", "12"]);
    assert_eq!(dry.status.code(), Some(1), "{}", out(&dry));
    assert!(out(&dry).contains("REFUSED"), "{}", out(&dry));
    assert!(
        out(&dry).contains("not complete, readable versions"),
        "{}",
        out(&dry)
    );
    assert_eq!(quad_count(path), before);
}
