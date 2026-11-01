//! The real CLI consumes Ontomark's root and discovery checks.
#![cfg(feature = "shared-layout")]
use std::{fs, path::Path, process::Command};

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("ontology")).unwrap();
    fs::create_dir_all(root.join("knowledge/nested")).unwrap();
    fs::write(
        root.join("ontology/vocabulary.yaml"),
        "version: 1\nnamespace: 'urn:test:'\ntypes: {}\nrelations: {}\n",
    )
    .unwrap();
    fs::write(
        root.join("knowledge/Note.md"),
        "---\ntype: Class\nstatus: stable\npublic: true\nresource: urn:test:note\n---\nNote\n",
    )
    .unwrap();
}

#[test]
fn relative_and_implicit_discovery_find_the_same_parent() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let run = |relative: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vault"));
        cmd.current_dir(dir.path().join("knowledge/nested"));
        if relative {
            cmd.args(["--repo", "."]);
        }
        cmd.args(["--json", "validate", "--vault", "knowledge"])
            .output()
            .unwrap()
    };
    let implicit = run(false);
    let relative = run(true);
    assert_eq!(relative.status.code(), implicit.status.code());
    assert_eq!(relative.stdout, implicit.stdout);
    assert!(
        !relative.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&relative.stderr)
    );
}

#[cfg(unix)]
#[test]
fn root_alias_is_refused_before_build_promotes_anything() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    std::os::unix::fs::symlink("knowledge", dir.path().join("working")).unwrap();
    let out = dir.path().join("www");
    let result = Command::new(env!("CARGO_BIN_EXE_vault"))
        .arg("--repo")
        .arg(dir.path())
        .args(["build", "--vault", "knowledge", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("overlap"));
    assert!(!out.exists());
}
