//! N-Quads dump and reload of an Oxigraph store.

use anyhow::{bail, Context, Result};
use oxigraph::io::RdfFormat;
use oxigraph::store::Store;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

/// Writer that hashes and counts what passes through it.
struct Tally<W: Write> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
    lines: u64,
}

impl<W: Write> Write for Tally<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        let chunk = &buf[..written];
        self.hasher.update(chunk);
        self.bytes += written as u64;
        // N-Quads is one statement per line and escapes newlines inside literals.
        self.lines += chunk.iter().filter(|b| **b == b'\n').count() as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Facts about a finished dump.
pub(crate) struct DumpFacts {
    pub bytes: u64,
    pub sha256: String,
    pub quads: u64,
}

/// How the source store is opened.
#[derive(Clone, Copy)]
pub(crate) enum Access {
    /// A live or stopped store: opened read-write because Oxigraph forbids a
    /// read-only reader beside a live writer, so a store locked by a running
    /// server fails by name instead of being read unsafely.
    ReadWrite,
    /// A server checkpoint: nobody writes to it, and a read-only open leaves the
    /// artefact being copied untouched.
    ReadOnly,
}

/// Dumps every graph of the store at `store_dir` to `out` as N-Quads.
pub(crate) fn dump(store_dir: &Path, out: &Path, access: Access) -> Result<DumpFacts> {
    if !store_dir.join("CURRENT").is_file() {
        bail!(
            "{} is not an Oxigraph store (no RocksDB CURRENT file)",
            store_dir.display()
        );
    }
    let opened = match access {
        Access::ReadWrite => Store::open(store_dir),
        Access::ReadOnly => Store::open_read_only(store_dir),
    };
    let store = opened.with_context(|| {
        format!(
            "cannot open Oxigraph store {}: if the server is running it holds the lock; \
             stop it or back up the server's checkpoint directory (--checkpoint-dir) instead",
            store_dir.display()
        )
    })?;
    let file = File::create(out).with_context(|| format!("creating {}", out.display()))?;
    let tally = Tally {
        inner: BufWriter::new(file),
        hasher: Sha256::new(),
        bytes: 0,
        lines: 0,
    };
    // `dump_to_writer` iterates one read snapshot, so the dump is consistent.
    let mut tally = store
        .dump_to_writer(RdfFormat::NQuads, tally)
        .context("dumping Oxigraph as N-Quads")?;
    tally.flush()?;
    let file = tally
        .inner
        .into_inner()
        .map_err(|e| anyhow::anyhow!("flushing dump: {e}"))?;
    file.sync_all()?;
    let expected = store.len().context("counting quads")? as u64;
    if expected != tally.lines {
        bail!(
            "dump holds {} quads but the store reports {expected}; \
             the store changed during the dump",
            tally.lines
        );
    }
    Ok(DumpFacts {
        bytes: tally.bytes,
        sha256: format!("{:x}", tally.hasher.finalize()),
        quads: tally.lines,
    })
}

/// Loads an N-Quads dump into a new store at `store_dir`; returns the quad count.
pub(crate) fn load(dump: &Path, store_dir: &Path) -> Result<u64> {
    let store = Store::open(store_dir)
        .with_context(|| format!("creating Oxigraph store {}", store_dir.display()))?;
    let reader = BufReader::new(File::open(dump)?);
    store
        .bulk_loader()
        .load_from_reader(RdfFormat::NQuads, reader)
        .context("loading N-Quads")?;
    Ok(store.len()? as u64)
}
