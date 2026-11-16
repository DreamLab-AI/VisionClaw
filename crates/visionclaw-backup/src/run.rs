//! The backup, verify and restore operations.

use crate::manifest::{Artefact, ArtefactKind, Manifest, MANIFEST_FILE};
use crate::{oxigraph_dump, sqlite_snapshot};
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

/// SQLite databases whose absence fails a run: authored configuration, the
/// enrichment decision ledger, the KPI series and shared graphs. None can be
/// regenerated. Mirrors `REQUIRED_DBS` in `scripts/backup-sqlite.sh`.
pub const REQUIRED_SQLITE: [&str; 4] = [
    "settings.sqlite3",
    "enrichment.sqlite3",
    "kpi.sqlite3",
    "shares.sqlite3",
];

/// SQLite databases the server recreates on boot; absence is recorded, not fatal.
pub const OPTIONAL_SQLITE: [&str; 1] = ["liveness.sqlite3"];

/// Name of the Oxigraph dump inside a run folder.
const OXIGRAPH_ARTEFACT: &str = "oxigraph.nq";
/// Directory name the Oxigraph store takes inside a restore target.
const OXIGRAPH_RESTORE_DIR: &str = "oxigraph";
/// Run folder names are `YYYYMMDDTHHMMSSZ`.
const STAMP_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const STAMP_LEN: usize = 16;

/// What to back up and where to put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRequest {
    /// The server's data directory, holding the SQLite files and `oxigraph/`.
    pub data_dir: PathBuf,
    /// Oxigraph store or checkpoint directory; defaults to `<data_dir>/oxigraph`.
    pub oxigraph_dir: Option<PathBuf>,
    /// Folder that receives one dated sub-folder per run.
    pub dest_root: PathBuf,
    /// Number of complete runs to keep; at least one.
    pub keep: usize,
}

/// A finished run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOutcome {
    /// The dated folder holding the run.
    pub run_dir: PathBuf,
    /// The manifest written into it.
    pub manifest: Manifest,
    /// Older runs removed by retention.
    pub pruned: Vec<PathBuf>,
}

fn sha256_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 1 << 16];
    let mut total = 0_u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("syncing {}", dir.display()))
}

/// Takes one backup run.
///
/// Every store is attempted and every failure reported together. If any
/// required store fails, the partial run is deleted and nothing is published.
/// On success the run is renamed into place, then older runs beyond
/// `request.keep` are removed.
///
/// # Errors
///
/// Fails when `keep` is zero, the destination lies inside the data directory,
/// a required store is missing or cannot be backed up, or any file operation
/// fails.
pub fn backup(request: &BackupRequest) -> Result<BackupOutcome> {
    if request.keep == 0 {
        bail!("keep must be at least 1");
    }
    let data_dir = request
        .data_dir
        .canonicalize()
        .with_context(|| format!("data directory {}", request.data_dir.display()))?;
    let oxigraph_dir = request
        .oxigraph_dir
        .clone()
        .unwrap_or_else(|| data_dir.join("oxigraph"));
    fs::create_dir_all(&request.dest_root)
        .with_context(|| format!("creating {}", request.dest_root.display()))?;
    let dest_root = request.dest_root.canonicalize()?;
    for source in [&data_dir, &oxigraph_dir] {
        if let Ok(source) = source.canonicalize() {
            if dest_root.starts_with(&source) {
                bail!(
                    "destination {} is inside {}: a backup there shares a failure domain with its source",
                    dest_root.display(),
                    source.display()
                );
            }
        }
    }

    let started = chrono::Utc::now();
    let stamp = started.format(STAMP_FORMAT).to_string();
    let run_dir = dest_root.join(&stamp);
    let partial = dest_root.join(format!(".{stamp}.partial"));
    if run_dir.exists() || partial.exists() {
        bail!("a run named {stamp} already exists; wait a second and retry");
    }
    fs::create_dir(&partial)?;

    let result = write_run(&partial, &data_dir, &oxigraph_dir, started);
    let manifest = match result {
        Ok(manifest) => manifest,
        Err(err) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(err);
        }
    };
    fs::rename(&partial, &run_dir).with_context(|| format!("publishing {}", run_dir.display()))?;
    sync_dir(&dest_root)?;
    let pruned = prune(&dest_root, request.keep)?;
    Ok(BackupOutcome {
        run_dir,
        manifest,
        pruned,
    })
}

fn write_run(
    partial: &Path,
    data_dir: &Path,
    oxigraph_dir: &Path,
    started: chrono::DateTime<chrono::Utc>,
) -> Result<Manifest> {
    let mut artefacts = Vec::new();
    let mut absent_optional = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    match oxigraph_dump::dump(oxigraph_dir, &partial.join(OXIGRAPH_ARTEFACT)) {
        Ok(facts) => artefacts.push(Artefact {
            name: OXIGRAPH_ARTEFACT.to_owned(),
            bytes: facts.bytes,
            sha256: facts.sha256,
            required: true,
            kind: ArtefactKind::OxigraphNquads { quads: facts.quads },
        }),
        Err(err) => failures.push(format!("oxigraph: {err:#}")),
    }

    let members = REQUIRED_SQLITE
        .iter()
        .map(|n| (*n, true))
        .chain(OPTIONAL_SQLITE.iter().map(|n| (*n, false)));
    for (name, required) in members {
        let source = data_dir.join(name);
        if !source.is_file() {
            if required {
                failures.push(format!(
                    "{name}: required database not found at {}",
                    source.display()
                ));
            } else {
                absent_optional.push(name.to_owned());
            }
            continue;
        }
        let target = partial.join(name);
        let outcome = sqlite_snapshot::snapshot(&source, &target).and_then(|facts| {
            let (bytes, sha256) = sha256_file(&target)?;
            Ok(Artefact {
                name: name.to_owned(),
                bytes,
                sha256,
                required,
                kind: ArtefactKind::Sqlite {
                    tables: facts.tables,
                    content_sha256: facts.content_sha256,
                },
            })
        });
        match outcome {
            Ok(artefact) => artefacts.push(artefact),
            Err(err) => failures.push(format!("{name}: {err:#}")),
        }
    }

    if !failures.is_empty() {
        bail!(
            "backup failed, nothing published:\n  {}",
            failures.join("\n  ")
        );
    }
    artefacts.sort_by(|a, b| a.name.cmp(&b.name));
    let manifest = Manifest {
        format: Manifest::FORMAT,
        created_utc: started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        artefacts,
        absent_optional,
    };
    let path = partial.join(MANIFEST_FILE);
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    File::open(&path)?.sync_all()?;
    sync_dir(partial)?;
    Ok(manifest)
}

/// Complete runs under `root`, oldest first. `.partial` folders and folders
/// without a manifest are not runs.
fn list_runs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut runs = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_stamp = name.len() == STAMP_LEN
            && chrono::NaiveDateTime::parse_from_str(&name, STAMP_FORMAT).is_ok();
        if is_stamp && entry.path().join(MANIFEST_FILE).is_file() {
            runs.push(entry.path());
        }
    }
    runs.sort();
    Ok(runs)
}

fn prune(root: &Path, keep: usize) -> Result<Vec<PathBuf>> {
    let runs = list_runs(root)?;
    let excess = runs.len().saturating_sub(keep);
    let doomed: Vec<PathBuf> = runs.into_iter().take(excess).collect();
    for run in &doomed {
        fs::remove_dir_all(run).with_context(|| format!("pruning {}", run.display()))?;
    }
    Ok(doomed)
}

/// The newest complete run under `root`.
///
/// # Errors
///
/// Fails when `root` cannot be read or holds no complete run.
pub fn latest_run(root: &Path) -> Result<PathBuf> {
    list_runs(root)?
        .pop()
        .ok_or_else(|| anyhow!("no complete backup run under {}", root.display()))
}

fn resolve_run(path: &Path) -> Result<PathBuf> {
    if path.join(MANIFEST_FILE).is_file() {
        Ok(path.to_path_buf())
    } else {
        latest_run(path)
    }
}

fn artefact_path(run: &Path, artefact: &Artefact) -> Result<PathBuf> {
    let name = Path::new(&artefact.name);
    if name.file_name().map(Path::new) != Some(name) {
        bail!(
            "manifest names {:?}, which is not a plain file name",
            artefact.name
        );
    }
    Ok(run.join(name))
}

/// Checks a run against its manifest: every artefact present with the recorded
/// size and SHA-256, and no unlisted file. `run` may be a run folder or a
/// backup root, in which case the newest complete run is checked.
///
/// # Errors
///
/// Fails on a missing or unreadable manifest, a missing, resized, altered or
/// unlisted file, or an unsupported manifest version.
pub fn verify(run: &Path) -> Result<(PathBuf, Manifest)> {
    let run = resolve_run(run)?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(run.join(MANIFEST_FILE))?)
        .with_context(|| format!("parsing manifest in {}", run.display()))?;
    if manifest.format != Manifest::FORMAT {
        bail!("unsupported manifest format {}", manifest.format);
    }
    let mut problems = Vec::new();
    for artefact in &manifest.artefacts {
        let path = artefact_path(&run, artefact)?;
        match sha256_file(&path) {
            Err(_) => problems.push(format!("{}: missing", artefact.name)),
            Ok((bytes, _)) if bytes != artefact.bytes => problems.push(format!(
                "{}: {bytes} bytes, manifest says {}",
                artefact.name, artefact.bytes
            )),
            Ok((_, sha)) if sha != artefact.sha256 => {
                problems.push(format!(
                    "{}: SHA-256 differs from the manifest",
                    artefact.name
                ));
            }
            Ok(_) => {}
        }
    }
    for entry in fs::read_dir(&run)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name != MANIFEST_FILE && !manifest.artefacts.iter().any(|a| a.name == name) {
            problems.push(format!("{name}: present but not in the manifest"));
        }
    }
    if !problems.is_empty() {
        bail!(
            "{} failed verification:\n  {}",
            run.display(),
            problems.join("\n  ")
        );
    }
    Ok((run, manifest))
}

/// Rebuilds the stores of a run into an empty `target` directory: the Oxigraph
/// store as `target/oxigraph` and each SQLite database under its own name.
/// The run is verified first, and each restored store is checked against the
/// manifest (quad count; integrity, row counts and content digest).
///
/// # Errors
///
/// Fails if the run does not verify, `target` exists and is not empty, or a
/// restored store disagrees with the manifest. On failure anything written
/// into `target` is removed again.
pub fn restore(run: &Path, target: &Path) -> Result<(PathBuf, Manifest)> {
    let (run, manifest) = verify(run)?;
    if target.exists() {
        if !target.is_dir() || fs::read_dir(target)?.next().is_some() {
            bail!(
                "restore target {} is not an empty directory",
                target.display()
            );
        }
    } else {
        fs::create_dir_all(target)?;
    }
    if let Err(err) = restore_into(&run, &manifest, target) {
        if let Ok(entries) = fs::read_dir(target) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() {
                    fs::remove_dir_all(path)
                } else {
                    fs::remove_file(path)
                };
            }
        }
        return Err(err);
    }
    Ok((run, manifest))
}

fn restore_into(run: &Path, manifest: &Manifest, target: &Path) -> Result<()> {
    for artefact in &manifest.artefacts {
        let source = artefact_path(run, artefact)?;
        match &artefact.kind {
            ArtefactKind::OxigraphNquads { quads } => {
                let loaded = oxigraph_dump::load(&source, &target.join(OXIGRAPH_RESTORE_DIR))?;
                if loaded != *quads {
                    bail!("restored Oxigraph holds {loaded} quads, manifest says {quads}");
                }
            }
            ArtefactKind::Sqlite {
                tables,
                content_sha256,
            } => {
                let restored = target.join(&artefact.name);
                fs::copy(&source, &restored)?;
                sqlite_snapshot::check_restored(&restored, tables, content_sha256)?;
            }
        }
    }
    Ok(())
}
