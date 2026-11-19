//! ADR-2204 S2 — `crates/visionclaw-domain/data/relations.yaml` against the recorded pre-schema
//! behaviour and against its generated JSON projection.
//!
//! * The golden table `tests/fixtures/predicate_table.golden` (repo root) was
//!   recorded from the sync BEFORE the schema existed: predicate → label →
//!   weight bits. The schema must reproduce every row.
//! * The desktop client's colour table is generated from the schema into
//!   `client/src/features/graph/relations.generated.json`; this pins the two.
//!   Regenerate with
//!   `cargo test -p visionclaw-domain --test relations_schema -- --ignored regenerate`.

use std::path::{Path, PathBuf};

use visionclaw_domain::relations::{PredicateClass, Producer, RelationSchema};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn schema_reproduces_the_pre_schema_predicate_table() {
    let schema = RelationSchema::builtin();
    let golden = std::fs::read_to_string(repo_root().join("tests/fixtures/predicate_table.golden"))
        .expect("golden fixture");
    let mut predicate_rows = 0;
    for row in golden.lines() {
        let cols: Vec<&str> = row.split(" | ").map(str::trim).collect();
        match cols.as_slice() {
            [iri, label, weight] if iri.starts_with("http") => {
                predicate_rows += 1;
                match schema.classify(iri) {
                    PredicateClass::Relation(r) => {
                        assert_eq!(&r.label, label, "{iri}");
                        let want = u32::from_str_radix(weight, 16).expect("hex bits");
                        assert_eq!(
                            r.legacy_weight.expect("weight").to_bits(),
                            want,
                            "{iri}: weight bits"
                        );
                    }
                    PredicateClass::Ignored | PredicateClass::Unknown => {
                        assert_eq!(*label, "", "{iri} produced no edge before the schema");
                    }
                }
            }
            ["domain_member spoke", w] => assert_eq!(bits(schema, Producer::DomainSpoke), *w),
            ["inferred (hierarchical)", w] => assert_eq!(bits(schema, Producer::Inferred), *w),
            ["wikilink", w] => assert_eq!(bits(schema, Producer::Wikilink), *w),
            ["elevated_from", w] => assert_eq!(bits(schema, Producer::ElevatedFrom), *w),
            ["cocite per target", w] => assert_eq!(bits(schema, Producer::CoCitation), *w),
            other => panic!("unrecognised golden row {other:?}"),
        }
    }
    assert!(
        predicate_rows >= 40,
        "the golden covers every sync predicate"
    );
}

fn bits(schema: &RelationSchema, p: Producer) -> String {
    format!("{:08x}", schema.producer_weight(p).to_bits())
}

#[test]
fn rdf_type_is_a_known_non_edge_and_unknown_is_unknown() {
    let schema = RelationSchema::builtin();
    let golden =
        std::fs::read_to_string(repo_root().join("tests/fixtures/predicate_table.golden")).unwrap();
    assert!(golden.contains("22-rdf-syntax-ns#type |  | -"));
    assert!(matches!(
        schema.classify("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
        PredicateClass::Ignored
    ));
    assert!(matches!(
        schema.classify("https://example.org/not-a-known-predicate#x"),
        PredicateClass::Unknown
    ));
}

const PROJECTION_PATH: &str = "client/src/features/graph/relations.generated.json";

fn projection_text() -> String {
    serde_json::to_string_pretty(&RelationSchema::builtin().projection()).unwrap() + "\n"
}

#[test]
fn committed_projection_matches_the_yaml() {
    let committed = std::fs::read_to_string(repo_root().join(PROJECTION_PATH))
        .expect("projection is committed; regenerate it");
    assert_eq!(
        committed,
        projection_text(),
        "{PROJECTION_PATH} is stale: regenerate from crates/visionclaw-domain/data/relations.yaml"
    );
}

#[test]
#[ignore = "writes the committed projection"]
fn regenerate() {
    std::fs::write(repo_root().join(PROJECTION_PATH), projection_text()).unwrap();
}

/// The colours the desktop client held in `EDGE_TYPE_COLORS` before the table
/// was generated (ADR-2204 S2). Every key and hex value must survive.
#[test]
fn projection_carries_every_legacy_client_colour() {
    let legacy: &[(&str, &str)] = &[
        ("hierarchical", "#FFD700"),
        ("subclass", "#FFD700"),
        ("structural", "#4FC3F7"),
        ("has_part", "#4FC3F7"),
        ("is_part_of", "#4FC3F7"),
        ("dependency", "#81C784"),
        ("requires", "#81C784"),
        ("depends_on", "#81C784"),
        ("enables", "#81C784"),
        ("associative", "#CE93D8"),
        ("relates_to", "#CE93D8"),
        ("bridge", "#FF7043"),
        ("bridges_to", "#FF7043"),
        ("bridges_from", "#FF7043"),
        ("provenance", "#FFB300"),
        ("utilisation", "#26C6DA"),
        ("co_citation", "#5C6BC0"),
        ("standardisation", "#7E8CE0"),
        ("implements", "#EC407A"),
        ("explicit_link", "#5A6470"),
        ("namespace", "#78909C"),
        ("inferred", "#B0BEC5"),
        ("domain_member", "#A1887F"),
        ("chain_payment", "#E040FB"),
    ];
    let p = RelationSchema::builtin().projection();
    let colours = p["colours"].as_object().unwrap();
    assert_eq!(colours.len(), legacy.len(), "no key added or lost");
    for (k, hex) in legacy {
        assert_eq!(colours[*k], *hex, "{k}");
    }
    assert_eq!(p["defaultColour"], "#AAAAAA");
}
