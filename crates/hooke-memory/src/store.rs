//! Read Memorilla's precomputed embeddings (`embeddings/<encoder>/<config>/<split>/`).
//!
//! `index.safetensors` maps each collection to a shard, start row and length;
//! `documents-NNNNN.safetensors` and `questions.safetensors` hold float16 rows under the
//! tensor name `embeddings`. Shards are memory-mapped and sliced per collection, as in
//! Memorilla's `EmbeddingStore`.

use anyhow::{bail, Context, Result};
use half::f16;
use memmap2::Mmap;
use safetensors::{Dtype, SafeTensors};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

const TENSOR: &str = "embeddings";

fn map(path: &Path) -> Result<Mmap> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    // SAFETY: the embedding files are read-only inputs and are not modified while mapped.
    Ok(unsafe { Mmap::map(&file)? })
}

fn ints(st: &SafeTensors, name: &str) -> Result<Vec<i64>> {
    let t = st.tensor(name)?;
    let b = t.data();
    Ok(match t.dtype() {
        Dtype::I64 => b.chunks_exact(8).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect(),
        Dtype::I32 => b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect(),
        other => bail!("index tensor {name} has unsupported dtype {other:?}"),
    })
}

/// Row-major float16 rows decoded to f32.
fn rows(mmap: &Mmap, start: usize, count: usize) -> Result<(Vec<f32>, usize)> {
    let st = SafeTensors::deserialize(mmap)?;
    let t = st.tensor(TENSOR)?;
    if t.dtype() != Dtype::F16 {
        bail!("expected float16 embeddings, found {:?}", t.dtype());
    }
    let width = t.shape()[1];
    let bytes = &t.data()[start * width * 2..(start + count) * width * 2];
    let v = bytes.chunks_exact(2).map(|c| f16::from_le_bytes([c[0], c[1]]).to_f32()).collect();
    Ok((v, width))
}

/// One split of one benchmark config.
pub struct EmbeddingStore {
    dir: PathBuf,
    position: HashMap<i64, usize>,
    shard: Vec<i64>,
    start: Vec<i64>,
    length: Vec<i64>,
    shards: HashMap<i64, Mmap>,
    questions: Mmap,
}

impl EmbeddingStore {
    /// Open `<root>/embeddings/<encoder>/<config>/<split>`.
    pub fn open(root: &Path, encoder: &str, config: &str, split: &str) -> Result<Self> {
        let dir = root.join("embeddings").join(encoder).join(config).join(split);
        let index_map = map(&dir.join("index.safetensors"))?;
        let index = SafeTensors::deserialize(&index_map)?;
        let ids = ints(&index, "collection_ids")?;
        let position = ids.iter().enumerate().map(|(i, &c)| (c, i)).collect();
        Ok(Self {
            shard: ints(&index, "shard")?,
            start: ints(&index, "start")?,
            length: ints(&index, "length")?,
            position,
            shards: HashMap::new(),
            questions: map(&dir.join("questions.safetensors"))?,
            dir,
        })
    }

    /// Document embeddings of a collection, `(row-major values, num_docs, width)`.
    pub fn documents(&mut self, collection_id: i64) -> Result<(Vec<f32>, usize, usize)> {
        let p = *self.position.get(&collection_id).with_context(|| format!("collection {collection_id} not indexed"))?;
        let shard = self.shard[p];
        if !self.shards.contains_key(&shard) {
            let m = map(&self.dir.join(format!("documents-{shard:05}.safetensors")))?;
            self.shards.insert(shard, m);
        }
        let n = self.length[p] as usize;
        let (v, w) = rows(&self.shards[&shard], self.start[p] as usize, n)?;
        Ok((v, n, w))
    }

    /// The embedding shard holding a collection, if it is indexed.
    pub fn shard(&self, collection_id: i64) -> Option<i64> {
        self.position.get(&collection_id).map(|&p| self.shard[p])
    }

    /// The question embedding of one row of the split.
    pub fn question(&self, row: usize) -> Result<Vec<f32>> {
        Ok(rows(&self.questions, row, 1)?.0)
    }
}
