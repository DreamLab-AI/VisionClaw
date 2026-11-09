//! Train and evaluate the preregistered P1 arms.
//!
//! Paths come from the environment: `HOOKE_DATA` (Memorilla-Data root), `HOOKE_QWEN` (Qwen3-8B
//! directory), `HOOKE_STAGE3` (converted Stage 3 safetensors).
//!
//! ```text
//! run train <arm> <memory lr> <seed> <out dir> [max rows]
//! run eval  <arm> <weights.safetensors> <validation|test> <out.jsonl>
//! ```

use anyhow::{bail, ensure, Context, Result};
use candle_core::{DType, Device, Tensor, Var};
use candle_nn::{AdamW, Optimizer, ParamsAdamW, VarBuilder, VarMap};
use hooke_memory::data::{self, Row};
use hooke_memory::decoder::Decoder;
use hooke_memory::field::{Arm, Field, Graph, Relations};
use hooke_memory::memory::{MemoryConfig, MemoryModule};
use hooke_memory::prompt::{self, PromptTokenizer};
use hooke_memory::store::EmbeddingStore;
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicU64, Ordering};
use rand::{rngs::StdRng, SeedableRng};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Dataset config under `data/` and `embeddings/<encoder>/` (`HOOKE_CONFIG`, default `factkg`).
static CONFIG: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| std::env::var("HOOKE_CONFIG").unwrap_or_else(|_| "factkg".to_string()));

const ENCODER: &str = "qwen3-embedding-4b";
const BATCH: usize = 8;
/// Rows per micro-batch (`HOOKE_MICRO`, default 4); the effective batch stays `BATCH`.
fn micro() -> usize {
    std::env::var("HOOKE_MICRO").ok().and_then(|v| v.parse().ok()).unwrap_or(4)
}
const EVAL_BATCH: usize = 16;
/// Leave-one-out scorer: documents masked out per decoder batch (P5 targets).
const LOO_BATCH: usize = 16;
const LOO_TRAIN_SEED: u64 = 23;
const WARMUP: usize = 100;
const GEOMETRY_LR: f64 = 1e-3;
/// Embedding shards drawn from. The shards are sorted by label (0-3 are 93% False, 7-10 all
/// True), so the training rows are a label-stratified sample across them (deviation D1).
const TRAIN_SHARDS: &[i64] = &[0, 1, 2, 3, 7, 8];
/// Training rows: half True, half False; the same rows for every arm and seed.
const TRAIN_ROWS: usize = 25_438;
const VALIDATION_ROWS: usize = 2000;
const VALIDATION_SEED: u64 = 7;
const SCRAMBLE_EVAL_SEED: u64 = 11;
const IM_END: u32 = 151645;

struct Env {
    data: PathBuf,
    qwen: PathBuf,
    stage3: PathBuf,
}

fn env() -> Result<Env> {
    let get = |k: &str| std::env::var(k).map(PathBuf::from).with_context(|| format!("{k} not set"));
    Ok(Env { data: get("HOOKE_DATA")?, qwen: get("HOOKE_QWEN")?, stage3: get("HOOKE_STAGE3")? })
}

/// Memory module and (for A1–A4) the field, on one `VarMap`.
struct Model {
    varmap: VarMap,
    memory: MemoryModule,
    field: Option<Field>,
    /// P2 learned privacy bottleneck (`HOOKE_DP_K`): down-projection, clip and noise at width k,
    /// up-projection back to the embedding width. Trained on public rows only.
    dp: Option<(candle_nn::Linear, candle_nn::Linear)>,
}

fn build(arm: Arm, relations: usize, dev: &Device) -> Result<Model> {
    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, dev);
    let cfg = MemoryConfig::default();
    let memory = MemoryModule::new(cfg, vb.clone())?;
    let field = (arm != Arm::A0).then(|| Field::new(arm, relations, cfg.embedding_dim, vb.pp("field"))).transpose()?;
    let dp = match dp_k() {
        None => None,
        Some(k) => Some((
            candle_nn::linear_no_bias(cfg.embedding_dim, k, vb.pp("dp.down"))?,
            candle_nn::linear_no_bias(k, cfg.embedding_dim, vb.pp("dp.up"))?,
        )),
    };
    Ok(Model { varmap, memory, field, dp })
}

/// Overwrite every variable present in `path`; return how many were set.
fn load_into(varmap: &VarMap, path: &Path, dev: &Device) -> Result<usize> {
    let tensors = candle_core::safetensors::load(path, dev)?;
    let data = varmap.data().lock().unwrap();
    let mut n = 0;
    for (name, var) in data.iter() {
        if let Some(t) = tensors.get(name) {
            var.set(&t.to_dtype(DType::F32)?)?;
            n += 1;
        }
    }
    Ok(n)
}

/// GPU memory in use (GiB), logged per phase when `HOOKE_MEMLOG` is set.
fn memlog(dev: &Device, tag: &str) {
    if std::env::var_os("HOOKE_MEMLOG").is_none() {
        return;
    }
    if let Device::Cuda(d) = dev {
        let _ = dev.synchronize();
        if let Ok((free, total)) = d.cuda_stream().context().mem_get_info() {
            eprintln!("mem {tag}: {:.2} GiB used", (total - free) as f64 / (1u64 << 30) as f64);
        }
    }
}

fn normalise(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !c.is_ascii_punctuation()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Padded batch inputs for the memory module, plus triple positions when the arm needs them.
struct Batch {
    docs: Tensor,
    /// Keyed per-document noise at the bottleneck width (P2 with `HOOKE_DP_K`).
    noise: Option<Tensor>,
    mask: Tensor,
    question: Tensor,
    triples: Option<Tensor>,
}

/// Training-time salt for the P2 noise keys; 0 (the release key) outside training.
static DP_SALT: AtomicU64 = AtomicU64::new(0);

fn dp_sigma() -> Option<f64> {
    std::env::var("HOOKE_DP_SIGMA").ok().and_then(|s| s.parse().ok())
}
fn dp_k() -> Option<usize> {
    std::env::var("HOOKE_DP_K").ok().and_then(|s| s.parse().ok())
}
fn dp_clip() -> f64 {
    std::env::var("HOOKE_DP_CLIP").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0)
}

/// FNV-1a over byte strings: a fixed, platform-independent key derivation for noise seeds.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for &b in *p {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Gaussian noise for one document, keyed by (release seed, training salt, collection, document
/// text). Keying by content, not position, means deleting a document leaves every other
/// document's noise unchanged: revocation equals never having ingested it (P2 T3).
fn keyed_noise(collection_id: i64, doc: &str, dim: usize, sigma: f64) -> Vec<f32> {
    use rand::{Rng, SeedableRng};
    let seed: u64 = std::env::var("HOOKE_DP_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let salt = DP_SALT.load(Ordering::Relaxed);
    let key = fnv(&[&seed.to_le_bytes(), &salt.to_le_bytes(), &collection_id.to_le_bytes(), doc.as_bytes()]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(key);
    (0..dim).map(|_| (sigma * rng.sample::<f64, _>(rand_distr::StandardNormal)) as f32).collect()
}

/// Input-level Gaussian mechanism at the embedding width (P2 without a bottleneck): clip each
/// document embedding to L2 norm C, add keyed N(0, sigma^2) noise per coordinate. Unset: no-op.
fn privatise(v: &mut [f32], w: usize, row: &Row) {
    let Some(sigma) = dp_sigma() else { return };
    if dp_k().is_some() {
        return;
    }
    let clip = dp_clip();
    for (doc, text) in v.chunks_mut(w).zip(&row.documents) {
        let norm = doc.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>().sqrt();
        let scale = if norm > clip { clip / norm } else { 1.0 };
        let z = keyed_noise(row.collection_id, text, w, sigma);
        for (x, e) in doc.iter_mut().zip(z) {
            *x = ((*x as f64) * scale) as f32 + e;
        }
    }
}

/// Headroom probe (validation only): keep a fraction of each collection's documents, chosen by
/// `HOOKE_KEEP_MODE` = random (seeded by collection), sim (cosine to the question embedding) or
/// lexical (word overlap between claim and document text) or attention (the memory's own
/// first-layer attention mass per document, read from the `HOOKE_ATTN_FILE` dump written by an
/// earlier full-collection pass with `HOOKE_ATTN_DUMP`). Unset `HOOKE_KEEP_FRAC` keeps all.
fn keep_subset(v: Vec<f32>, n: usize, w: usize, row_index: usize, row: &Row, q: &[f32]) -> (Vec<f32>, usize) {
    let Some(frac) = std::env::var("HOOKE_KEEP_FRAC").ok().and_then(|s| s.parse::<f64>().ok()) else {
        return (v, n);
    };
    let k = ((frac * n as f64).ceil() as usize).clamp(1, n);
    if k == n {
        return (v, n);
    }
    let mode = std::env::var("HOOKE_KEEP_MODE").unwrap_or_else(|_| "random".into());
    let mut idx: Vec<usize> = (0..n).collect();
    match mode.as_str() {
        "sim" => {
            let score = |i: usize| -> f32 { v[i * w..(i + 1) * w].iter().zip(q).map(|(a, b)| a * b).sum() };
            idx.sort_by(|&a, &b| score(b).total_cmp(&score(a)));
        }
        "attention" | "file" => {
            static SCORES: std::sync::OnceLock<std::collections::HashMap<usize, Vec<f32>>> = std::sync::OnceLock::new();
            let scores = SCORES.get_or_init(|| {
                let path = std::env::var("HOOKE_ATTN_FILE").expect("HOOKE_KEEP_MODE=attention|file needs HOOKE_ATTN_FILE");
                let text = std::fs::read_to_string(&path).expect("read HOOKE_ATTN_FILE");
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| {
                        let v: serde_json::Value = serde_json::from_str(l).expect("attention dump line");
                        let row = v["row"].as_u64().expect("row") as usize;
                        let sc = v["scores"].as_array().expect("scores").iter().map(|x| x.as_f64().unwrap() as f32).collect();
                        (row, sc)
                    })
                    .collect()
            });
            let sc = scores.get(&row_index).unwrap_or_else(|| panic!("no attention scores for row {row_index}"));
            assert_eq!(sc.len(), n, "row {row_index}: {} attention scores for {n} documents", sc.len());
            idx.sort_by(|&a, &b| sc[b].total_cmp(&sc[a]).then(a.cmp(&b)));
        }
        "lexical" => {
            let words = |t: &str| -> std::collections::HashSet<String> {
                t.split(|c: char| !c.is_alphanumeric()).filter(|x| x.len() > 2).map(|x| x.to_lowercase()).collect()
            };
            let qw = words(&row.question);
            let score: Vec<usize> = row.documents.iter().map(|d| words(d).intersection(&qw).count()).collect();
            idx.sort_by(|&a, &b| score[b].cmp(&score[a]).then(a.cmp(&b)));
        }
        _ => {
            use rand::{seq::SliceRandom, SeedableRng};
            let mut rng = rand::rngs::StdRng::seed_from_u64(row.collection_id as u64 ^ 0x9e37_79b9_7f4a_7c15);
            idx.shuffle(&mut rng);
        }
    }
    let mut keep: Vec<usize> = idx[..k].to_vec();
    keep.sort_unstable();
    let mut out = Vec::with_capacity(k * w);
    for i in keep {
        out.extend_from_slice(&v[i * w..(i + 1) * w]);
    }
    (out, k)
}

fn batch(
    rows: &[(usize, &Row)],
    store: &mut EmbeddingStore,
    field: Option<&Field>,
    rel: &Relations,
    scramble: &mut dyn FnMut(&Row, usize) -> Vec<u32>,
    dev: &Device,
) -> Result<Batch> {
    let mut docs = Vec::new();
    let mut lens = Vec::new();
    let mut questions = Vec::new();
    for &(row_index, row) in rows {
        let (mut v, n, w) = store.documents(row.collection_id)?;
        ensure!(n == row.documents.len(), "collection {}: {n} embeddings for {} documents", row.collection_id, row.documents.len());
        privatise(&mut v, w, row);
        let q = store.question(row_index)?;
        let (v, n) = keep_subset(v, n, w, row_index, row, &q);
        docs.push(Tensor::from_vec(v, (n, w), dev)?);
        lens.push(n);
        questions.push(Tensor::from_vec(q, w, dev)?);
    }
    let n_max = *lens.iter().max().unwrap();
    let pad = |t: &Tensor, n: usize| -> Result<Tensor> {
        if n == n_max { Ok(t.clone()) } else { Ok(t.pad_with_zeros(0, 0, n_max - n)?) }
    };
    let triples = match field {
        None => None,
        Some(f) => {
            let mut out = Vec::new();
            for ((_, row), (d, &n)) in rows.iter().zip(docs.iter().zip(&lens)) {
                let g = Graph::build(&row.documents, rel, dev)?;
                let perm = (f.arm == Arm::A3).then(|| Tensor::new(scramble(row, n).as_slice(), dev)).transpose()?;
                out.push(pad(&f.triples(&g, d, perm.as_ref())?, n)?);
            }
            Some(Tensor::stack(&out, 0)?)
        }
    };
    let noise = match (dp_sigma(), dp_k()) {
        (Some(sigma), Some(k)) => {
            let mut out = Vec::new();
            for ((_, row), &n) in rows.iter().zip(&lens) {
                let z: Vec<f32> = row.documents.iter().flat_map(|t| keyed_noise(row.collection_id, t, k, sigma)).collect();
                out.push(pad(&Tensor::from_vec(z, (n, k), dev)?, n)?);
            }
            Some(Tensor::stack(&out, 0)?)
        }
        _ => None,
    };
    let mask: Vec<f32> = lens.iter().flat_map(|&n| (0..n_max).map(move |i| if i < n { 1.0 } else { 0.0 })).collect();
    Ok(Batch {
        docs: Tensor::stack(&docs.iter().zip(&lens).map(|(d, &n)| pad(d, n)).collect::<Result<Vec<_>>>()?, 0)?,
        mask: Tensor::from_vec(mask, (rows.len(), n_max), dev)?,
        question: Tensor::stack(&questions, 0)?,
        triples,
        noise,
    })
}

fn memory_tokens(m: &Model, b: &Batch, train: bool) -> Result<Tensor> {
    let geometry = |slots: &Tensor| -> candle_core::Result<Tensor> {
        let f = m.field.as_ref().unwrap();
        f.bias(slots, b.triples.as_ref().unwrap()).map_err(|e| candle_core::Error::Msg(e.to_string()))
    };
    let g: Option<&dyn Fn(&Tensor) -> candle_core::Result<Tensor>> = m.field.as_ref().map(|_| &geometry as _);
    let docs = match &m.dp {
        None => b.docs.clone(),
        Some((down, up)) => {
            use candle_core::Module;
            // Down-project, clip each document to L2 norm C, add the keyed noise, up-project.
            let z = down.forward(&b.docs)?;
            let norm = (z.sqr()?.sum_keepdim(2)? + 1e-12)?.sqrt()?;
            let c = dp_clip();
            let z = z.broadcast_mul(&norm.clamp(c, f64::MAX)?.recip()?.affine(c, 0.0)?)?;
            let z = match &b.noise {
                Some(e) => (z + e)?,
                None => z,
            };
            up.forward(&z)?
        }
    };
    Ok(m.memory.forward(&docs, Some(&b.mask), &b.question, g, train)?)
}

fn validation_rows(rows: &[Row]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    idx.shuffle(&mut StdRng::seed_from_u64(VALIDATION_SEED));
    idx.truncate(std::env::var("HOOKE_VAL_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(VALIDATION_ROWS));
    idx
}

/// Score rows; write per-claim outcomes; return accuracy (and the mean attention KL when
/// geometry is present).
#[allow(clippy::too_many_arguments)]
fn evaluate(
    m: &Model,
    dec: &Decoder,
    tok: &PromptTokenizer,
    rel: &Relations,
    rows: &[Row],
    idx: &[usize],
    store: &mut EmbeddingStore,
    out: &Path,
    dev: &Device,
) -> Result<(f64, Option<f64>)> {
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    // P3 probe: per-document attention mass on the full collection, for the `attention` keep mode.
    let mut attn_dump = std::env::var("HOOKE_ATTN_DUMP").ok().map(|p| std::io::BufWriter::new(std::fs::File::create(p).expect("create HOOKE_ATTN_DUMP"))).map(|f| {
        assert!(std::env::var("HOOKE_KEEP_FRAC").is_err(), "HOOKE_ATTN_DUMP must run on the full collection");
        f
    });
    let (mut correct, mut kl_sum, mut kl_n) = (0usize, 0f64, 0usize);
    let mut scramble = |row: &Row, n: usize| -> Vec<u32> {
        let mut p: Vec<u32> = (0..n as u32).collect();
        p.shuffle(&mut StdRng::seed_from_u64(SCRAMBLE_EVAL_SEED ^ row.collection_id as u64));
        p
    };
    for chunk in idx.chunks(EVAL_BATCH) {
        let items: Vec<(usize, &Row)> = chunk.iter().map(|&i| (i, &rows[i])).collect();
        let b = batch(&items, store, m.field.as_ref(), rel, &mut scramble, dev)?;
        // Detached: evaluation must not keep the autograd graph alive through decoding.
        let mem = memory_tokens(m, &b, false)?.detach().to_dtype(DType::BF16)?;
        if let Some(f) = attn_dump.as_mut() {
            let a = m.memory.doc_attention(&b.docs, Some(&b.mask), &b.question)?.to_dtype(DType::F32)?.to_vec2::<f32>()?;
            for (j, (i, r)) in items.iter().enumerate() {
                writeln!(f, "{}", serde_json::json!({"row": i, "scores": &a[j][..r.documents.len()]}))?;
            }
        }
        if let (Some(f), Some(t)) = (m.field.as_ref(), b.triples.as_ref()) {
            let geo = |s: &Tensor| f.bias(s, t).map_err(|e| candle_core::Error::Msg(e.to_string()));
            let (with, without) = m.memory.first_layer_probs(&b.docs, Some(&b.mask), &b.question, &geo)?;
            let kl = (&with * ((&with + 1e-12)?.log()? - (&without + 1e-12)?.log()?)?)?.sum(candle_core::D::Minus1)?;
            kl_sum += kl.mean_all()?.to_scalar::<f32>()? as f64 * items.len() as f64;
            kl_n += items.len();
        }
        let ids: Vec<Vec<u32>> = items
            .iter()
            .map(|(_, r)| tok.encode(&prompt::render(prompt::PERSONA_PROMPT, &r.question, 16)))
            .collect::<Result<_>>()?;
        let answers = prompt::generate(dec, tok, &ids, &mem, 8, dev)?;
        for ((i, r), a) in items.iter().zip(&answers) {
            let chosen = r.choices.iter().find(|c| normalise(c) == normalise(a));
            let ok = chosen.is_some_and(|c| c.trim() == r.answer.trim());
            correct += usize::from(ok);
            writeln!(file, "{}", serde_json::json!({"row": i, "collection_id": r.collection_id, "prediction": a, "correct": ok}))?;
        }
    }
    Ok((correct as f64 / idx.len() as f64, (kl_n > 0).then(|| kl_sum / kl_n as f64)))
}

/// P2 T3: is revoking a document the same as never having ingested it?
///
/// For validation collections with at least two documents, one document is chosen at random.
/// Two ingestion schemes are compared at the embedding width: keyed noise (this design, noise per
/// document from its content) and positional noise (one stream per collection, drawn in ingest
/// order). For each, the memory tokens after "privatise everything at ingest, then delete the
/// stored row" are compared with those after "ingest only the remaining documents". Runs on the CPU
/// without the decoder; identical memory tokens give identical answers.
fn revoke(e: &Env, args: &[String]) -> Result<()> {
    use rand::Rng;
    ensure!(args.len() >= 3, "run revoke <weights> <out.json> [rows]");
    let sigma = dp_sigma().context("run revoke needs HOOKE_DP_SIGMA")?;
    ensure!(dp_k().is_none(), "run revoke checks the embedding-width mechanism; unset HOOKE_DP_K");
    let want: usize = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(200);
    let dev = Device::Cpu;
    let model = build(Arm::A0, 1, &dev)?;
    let set = load_into(&model.varmap, Path::new(&args[1]), &dev)?;
    ensure!(set == model.varmap.all_vars().len(), "weights cover {set} of {} variables", model.varmap.all_vars().len());
    let rows = data::load_split(&e.data, CONFIG.as_str(), "validation")?;
    let mut store = EmbeddingStore::open(&e.data, ENCODER, CONFIG.as_str(), "validation")?;
    let clip = dp_clip();
    let clip_add = |x: &[f32], z: Vec<f32>| -> Vec<f32> {
        let norm = x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>().sqrt();
        let scale = if norm > clip { clip / norm } else { 1.0 };
        x.iter().zip(z).map(|(&v, e)| ((v as f64) * scale) as f32 + e).collect()
    };
    let tokens = |v: Vec<f32>, n: usize, w: usize, q: &[f32]| -> Result<Tensor> {
        let b = Batch {
            docs: Tensor::from_vec(v, (1, n, w), &dev)?,
            noise: None,
            mask: Tensor::ones((1, n), DType::F32, &dev)?,
            question: Tensor::from_vec(q.to_vec(), (1, w), &dev)?,
            triples: None,
        };
        memory_tokens(&model, &b, false)
    };
    let max_diff = |a: &Tensor, b: &Tensor| -> Result<f32> { Ok((a - b)?.abs()?.flatten_all()?.max(0)?.to_scalar::<f32>()?) };
    let mut pick = StdRng::seed_from_u64(SCRAMBLE_EVAL_SEED);
    let (mut checked, mut keyed_max, mut positional_max, mut positional_differ) = (0usize, 0f32, 0f32, 0usize);
    for ri in validation_rows(&rows) {
        if checked == want {
            break;
        }
        let row = &rows[ri];
        let (raw, n, w) = store.documents(row.collection_id)?;
        if n < 2 {
            continue;
        }
        let j = pick.gen_range(0..n);
        let q = store.question(ri)?;
        let keyed = |idx: &[usize]| -> Vec<f32> {
            idx.iter().flat_map(|&i| clip_add(&raw[i * w..(i + 1) * w], keyed_noise(row.collection_id, &row.documents[i], w, sigma))).collect()
        };
        let positional = |idx: &[usize]| -> Vec<f32> {
            let mut r = StdRng::seed_from_u64(fnv(&[&row.collection_id.to_le_bytes()]));
            idx.iter()
                .flat_map(|&i| {
                    let z: Vec<f32> = (0..w).map(|_| (sigma * r.sample::<f64, _>(rand_distr::StandardNormal)) as f32).collect();
                    clip_add(&raw[i * w..(i + 1) * w], z)
                })
                .collect()
        };
        let all: Vec<usize> = (0..n).collect();
        let kept: Vec<usize> = (0..n).filter(|&i| i != j).collect();
        let delete = |full: Vec<f32>| -> Vec<f32> { full.chunks(w).enumerate().filter(|(i, _)| *i != j).flat_map(|(_, c)| c.to_vec()).collect() };
        let kd = max_diff(&tokens(delete(keyed(&all)), n - 1, w, &q)?, &tokens(keyed(&kept), n - 1, w, &q)?)?;
        let pd = max_diff(&tokens(delete(positional(&all)), n - 1, w, &q)?, &tokens(positional(&kept), n - 1, w, &q)?)?;
        keyed_max = keyed_max.max(kd);
        positional_max = positional_max.max(pd);
        positional_differ += usize::from(pd > 0.0);
        checked += 1;
    }
    let out = serde_json::json!({"rows": checked, "sigma": sigma, "clip": clip,
        "keyed_max_abs_diff": keyed_max, "keyed_identical": keyed_max == 0.0,
        "positional_max_abs_diff": positional_max, "positional_rows_differing": positional_differ});
    std::fs::write(&args[2], serde_json::to_string_pretty(&out)?)?;
    println!("{out}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let e = env()?;
    if args.first().map(String::as_str) == Some("revoke") {
        return revoke(&e, &args);
    }
    let dev = Device::new_cuda(0)?;
    let train_rows = data::load_split(&e.data, CONFIG.as_str(), "train")?;
    let rel = Relations::from_triples(train_rows.iter().flat_map(|r| r.documents.iter().map(String::as_str)));
    eprintln!("relations: {} (incl. UNK)", rel.len());
    let tok = PromptTokenizer::load(&e.qwen)?;
    let dec = Decoder::load(&e.qwen, DType::BF16, &dev)?;
    match args.first().map(String::as_str) {
        Some("loo") => {
            // P5 targets: signed gold-answer logit margin with the full collection and with each
            // document masked out in turn. Plain full collections only: no noise, no subset.
            ensure!(args.len() >= 4, "run loo <split> <out.jsonl> <rows> [start]");
            ensure!(dp_sigma().is_none() && std::env::var("HOOKE_KEEP_FRAC").is_err(), "run loo runs on the plain full collection");
            let split = args[1].as_str();
            let out = PathBuf::from(&args[2]);
            let want: usize = args[3].parse()?;
            let start: usize = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(0);
            let model = build(Arm::A0, rel.len(), &dev)?;
            let set = load_into(&model.varmap, &e.stage3, &dev)?;
            ensure!(set == 37, "loaded {set} Stage 3 tensors, expected 37");
            let owned;
            let rows: &[Row] = if split == "train" { &train_rows } else { owned = data::load_split(&e.data, CONFIG.as_str(), split)?; &owned };
            let order: Vec<usize> = if split == "validation" {
                validation_rows(rows)
            } else {
                let mut i: Vec<usize> = (0..rows.len()).collect();
                i.shuffle(&mut StdRng::seed_from_u64(LOO_TRAIN_SEED));
                i
            };
            let mut store = EmbeddingStore::open(&e.data, ENCODER, CONFIG.as_str(), split)?;
            let (t_ids, f_ids) = (tok.encode("True")?, tok.encode("False")?);
            ensure!(t_ids.len() == 1 && f_ids.len() == 1, "answer tokens must be single tokens");
            let (t_id, f_id) = (t_ids[0] as usize, f_ids[0] as usize);
            let margin = |mem: &Tensor, ids: &[Vec<u32>]| -> Result<Vec<f32>> {
                let (embeds, mask) = prompt::splice(&dec, ids, mem, tok.memory_id, &dev)?;
                let h = dec.forward_embeds(&embeds, &mask)?;
                let last = h.narrow(1, h.dim(1)? - 1, 1)?.squeeze(1)?.contiguous()?;
                let logits = dec.logits(&last)?.to_dtype(DType::F32)?;
                let t = logits.narrow(1, t_id, 1)?.squeeze(1)?;
                let f = logits.narrow(1, f_id, 1)?.squeeze(1)?;
                Ok((t - f)?.to_vec1::<f32>()?)
            };
            let mut file = std::io::BufWriter::new(std::fs::File::create(&out)?);
            let (mut done, mut skipped, mut checked) = (0usize, 0usize, false);
            for &ri in order.iter().skip(start) {
                if done == want {
                    break;
                }
                let row = &rows[ri];
                let Ok((v, n, w)) = store.documents(row.collection_id) else { skipped += 1; continue };
                if n != row.documents.len() || n < 2 {
                    skipped += 1;
                    continue;
                }
                let sign = match row.answer.trim() { "True" => 1f32, "False" => -1f32, _ => { skipped += 1; continue } };
                let q = store.question(ri)?;
                let docs = Tensor::from_vec(v.clone(), (1, n, w), &dev)?;
                let question = Tensor::from_vec(q.clone(), (1, w), &dev)?;
                let full = Batch { docs: docs.clone(), noise: None, mask: Tensor::ones((1, n), DType::F32, &dev)?, question: question.clone(), triples: None };
                let mem_full = memory_tokens(&model, &full, false)?.detach().to_dtype(DType::BF16)?;
                let attention = model.memory.doc_attention(&full.docs, Some(&full.mask), &full.question)?.to_dtype(DType::F32)?.to_vec2::<f32>()?[0].clone();
                let ids = vec![tok.encode(&prompt::render(prompt::PERSONA_PROMPT, &row.question, 16))?];
                let s_full = sign * margin(&mem_full, &ids)?[0];
                let mut s_minus = Vec::with_capacity(n);
                for chunk in (0..n).step_by(LOO_BATCH) {
                    let b = (n - chunk).min(LOO_BATCH);
                    let mut mask = vec![1f32; b * n];
                    for r in 0..b {
                        mask[r * n + chunk + r] = 0.0;
                    }
                    let bb = Batch {
                        docs: docs.broadcast_as((b, n, w))?.contiguous()?,
                        noise: None,
                        mask: Tensor::from_vec(mask, (b, n), &dev)?,
                        question: question.broadcast_as((b, w))?.contiguous()?,
                        triples: None,
                    };
                    let mem_f32 = memory_tokens(&model, &bb, false)?.detach().to_dtype(DType::F32)?;
                    if !checked {
                        // Masking a document must equal removing it: check once, on document 0, with
                        // both sides in f32 (the earlier check compared a bf16-rounded side with f32).
                        let rb = Batch { docs: Tensor::from_vec(v[w..].to_vec(), (1, n - 1, w), &dev)?, noise: None, mask: Tensor::ones((1, n - 1), DType::F32, &dev)?, question: question.clone(), triples: None };
                        let mem_r = memory_tokens(&model, &rb, false)?.detach().to_dtype(DType::F32)?;
                        let d = (mem_f32.get(0)? - mem_r.get(0)?)?.abs()?.flatten_all()?.max(0)?.to_scalar::<f32>()?;
                        eprintln!("mask-vs-remove max abs diff (first row, doc 0): {d:.3e}");
                        checked = true;
                    }
                    let mem = mem_f32.to_dtype(DType::BF16)?;
                    let ids_b: Vec<Vec<u32>> = vec![ids[0].clone(); b];
                    s_minus.extend(margin(&mem, &ids_b)?.into_iter().map(|m| sign * m));
                }
                let sims: Vec<f32> = (0..n).map(|i| v[i * w..(i + 1) * w].iter().zip(&q).map(|(a, b)| a * b).sum()).collect();
                let deltas: Vec<f32> = s_minus.iter().map(|s| s_full - s).collect();
                writeln!(file, "{}", serde_json::json!({"row": ri, "collection_id": row.collection_id, "n": n, "answer": row.answer.trim(), "margin_full": s_full, "deltas": deltas, "attention": attention, "sims": sims}))?;
                done += 1;
                if done % 50 == 0 {
                    eprintln!("loo {done}/{want} rows (skipped {skipped})");
                }
            }
            eprintln!("loo done: {done} rows, skipped {skipped}");
        }
        Some("train") => {
            ensure!(args.len() >= 5, "run train <arm> <memory lr> <seed> <out dir> [max rows]");
            let arm: Arm = args[1].parse()?;
            let lr: f64 = args[2].parse()?;
            let seed: u64 = args[3].parse()?;
            let out = PathBuf::from(&args[4]);
            let max_rows: Option<usize> = args.get(5).map(|s| s.parse()).transpose()?;
            std::fs::create_dir_all(&out)?;
            let model = build(arm, rel.len(), &dev)?;
            let set = load_into(&model.varmap, &e.stage3, &dev)?;
            ensure!(set == 37, "loaded {set} Stage 3 tensors, expected 37");
            let mut store = EmbeddingStore::open(&e.data, ENCODER, CONFIG.as_str(), "train")?;
            let (mut yes, mut no): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
            for i in 0..train_rows.len() {
                if store.shard(train_rows[i].collection_id).is_some_and(|s| TRAIN_SHARDS.contains(&s)) {
                    match train_rows[i].answer.trim() {
                        "True" => yes.push(i),
                        "False" => no.push(i),
                        other => bail!("unexpected FactKG answer {other:?}"),
                    }
                }
            }
            let half = TRAIN_ROWS / 2;
            ensure!(yes.len() >= half && no.len() >= half, "shards {TRAIN_SHARDS:?} hold {} True and {} False rows, need {half} each", yes.len(), no.len());
            let mut pick = StdRng::seed_from_u64(1000);
            yes.shuffle(&mut pick);
            no.shuffle(&mut pick);
            let mut order: Vec<usize> = yes[..half].iter().chain(&no[..half]).copied().collect();
            order.sort_unstable();
            eprintln!("training rows: {} stratified from shards {TRAIN_SHARDS:?}", order.len());
            let mut balance = std::collections::BTreeMap::new();
            for &i in &order { *balance.entry(train_rows[i].answer.trim().to_string()).or_insert(0usize) += 1; }
            eprintln!("training answers: {balance:?}");
            order.shuffle(&mut StdRng::seed_from_u64(1000 + seed));
            if let Some(m) = max_rows {
                order.truncate(m);
            }
            let (mut mem_vars, mut geo_vars): (Vec<Var>, Vec<Var>) = (Vec::new(), Vec::new());
            for (name, var) in model.varmap.data().lock().unwrap().iter() {
                if name.starts_with("field.") || name.starts_with("dp.") { geo_vars.push(var.clone()) } else { mem_vars.push(var.clone()) }
            }
            // P1b (`HOOKE_P1B=1`, PREREGISTRATION-P1b.md): the Stage 3 memory module is frozen and
            // only geometry trains; `lr` is the geometry's peak rate, under Memorilla's schedule
            // (10% linear warm-up, cosine decay to 10% of peak). Unset, P1 behaves as frozen in P1.
            let p1b = std::env::var("HOOKE_P1B").is_ok_and(|v| v == "1");
            // P2 (`HOOKE_P2=1`, PREREGISTRATION-P2.md): the whole memory module (and any privacy
            // bottleneck) trains under Memorilla's schedule on noise-perturbed public rows.
            let cosine = p1b || std::env::var("HOOKE_P2").is_ok_and(|v| v == "1");
            ensure!(!p1b || !geo_vars.is_empty(), "P1b trains geometry only; arm {} has none", args[1]);
            let geo_peak = if p1b { lr } else { GEOMETRY_LR };
            let total_steps = order.len().div_ceil(BATCH);
            let warm_steps = if cosine { (total_steps / 10).max(1) } else { WARMUP };
            let schedule = |step: usize, peak: f64| -> f64 {
                if step < warm_steps {
                    peak * (step + 1) as f64 / warm_steps as f64
                } else if cosine {
                    let t = (step - warm_steps) as f64 / (total_steps - warm_steps).max(1) as f64;
                    peak * (0.1 + 0.9 * 0.5 * (1.0 + (std::f64::consts::PI * t).cos()))
                } else {
                    peak
                }
            };
            let params = |lr| ParamsAdamW { lr, weight_decay: 0.01, ..Default::default() };
            let mut opt_mem = (!p1b).then(|| AdamW::new(mem_vars, params(lr))).transpose()?;
            let mut opt_geo = (!geo_vars.is_empty()).then(|| AdamW::new(geo_vars, params(geo_peak))).transpose()?;
            let mut rng = StdRng::seed_from_u64(seed);
            let mut log = std::fs::File::create(out.join("train.log"))?;
            DP_SALT.store(seed.wrapping_add(1), Ordering::Relaxed);
            let started = std::time::Instant::now();
            for (step, chunk) in order.chunks(BATCH).enumerate() {
                if let Some(o) = opt_mem.as_mut() {
                    o.set_learning_rate(schedule(step, lr));
                }
                if let Some(o) = opt_geo.as_mut() {
                    o.set_learning_rate(schedule(step, geo_peak));
                }
                let items: Vec<(usize, &Row)> = chunk.iter().map(|&i| (i, &train_rows[i])).collect();
                let mut scramble = |_: &Row, n: usize| -> Vec<u32> {
                    let mut p: Vec<u32> = (0..n as u32).collect();
                    p.shuffle(&mut rng);
                    p
                };
                // Gradients of the token-mean loss over the whole batch, accumulated over
                // micro-batches so the decoder's activations fit in memory.
                let mut grads: Option<candle_core::backprop::GradStore> = None;
                let total_tokens: usize = items.iter().map(|(_, r)| tok.encode(r.answer.trim()).map(|a| a.len() + 1)).sum::<Result<usize>>()?;
                let mut loss_sum = 0f64;
                for micro in items.chunks(micro()) {
                    memlog(&dev, &format!("step {step} micro start"));
                    let b = batch(micro, &mut store, model.field.as_ref(), &rel, &mut scramble, &dev)?;
                    let mem = memory_tokens(&model, &b, true)?.to_dtype(DType::BF16)?;
                    let (mut ids, mut targets, mut spans) = (Vec::new(), Vec::new(), Vec::new());
                    for (_, r) in micro {
                        let mut p = tok.encode(&prompt::render(prompt::PERSONA_PROMPT, &r.question, 16))?;
                        let mut a = tok.encode(r.answer.trim())?;
                        a.push(IM_END);
                        spans.push((p.len(), a.len()));
                        targets.extend_from_slice(&a);
                        p.extend_from_slice(&a);
                        ids.push(p);
                    }
                    let (embeds, mask) = prompt::splice(&dec, &ids, &mem, tok.memory_id, &dev)?;
                    let len = embeds.dim(1)?;
                    memlog(&dev, "memory tokens + splice");
                    let hidden = dec.forward_embeds(&embeds, &mask)?;
                    memlog(&dev, "decoder forward");
                    let mut pick = Vec::new();
                    for (r, (row_ids, &(p_len, a_len))) in ids.iter().zip(&spans).enumerate() {
                        let pad = len - row_ids.len();
                        pick.extend((0..a_len).map(|j| (r * len + pad + p_len - 1 + j) as u32));
                    }
                    let flat = hidden.reshape((ids.len() * len, hidden.dim(2)?))?;
                    let h = flat.index_select(&Tensor::new(pick.as_slice(), &dev)?, 0)?;
                    let logp = candle_nn::ops::log_softmax(&dec.logits(&h)?, candle_core::D::Minus1)?;
                    let nll = logp.gather(&Tensor::new(targets.as_slice(), &dev)?.unsqueeze(1)?, 1)?.sum_all()?.neg()?;
                    let micro_loss = (nll / total_tokens as f64)?;
                    loss_sum += micro_loss.to_scalar::<f32>()? as f64;
                    let g = micro_loss.backward()?;
                    if std::env::var_os("HOOKE_GRADDIAG").is_some() {
                        for (name, var) in model.varmap.data().lock().unwrap().iter() {
                            match g.get(var.as_tensor()) {
                                Some(t) => eprintln!("grad {name}: {:.3e}", t.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?),
                                None => eprintln!("grad {name}: NONE"),
                            }
                        }
                        std::process::exit(0);
                    }
                    memlog(&dev, "backward");
                    grads = Some(match grads {
                        None => g,
                        Some(mut acc) => {
                            for var in model.varmap.all_vars() {
                                if let Some(add) = g.get(var.as_tensor()) {
                                    let sum = match acc.get(var.as_tensor()) {
                                        Some(prev) => (prev + add)?,
                                        None => add.clone(),
                                    };
                                    acc.insert(var.as_tensor(), sum);
                                }
                            }
                            acc
                        }
                    });
                }
                let grads = grads.context("empty batch")?;
                memlog(&dev, "accumulated");
                let loss = Tensor::new(loss_sum as f32, &dev)?;
                if let Some(o) = opt_mem.as_mut() {
                    o.step(&grads)?;
                }
                if let Some(o) = opt_geo.as_mut() {
                    o.step(&grads)?;
                }
                if step % 50 == 0 {
                    let mut line = format!("step {step} loss {:.4} elapsed {:.0}s", loss.to_scalar::<f32>()?, started.elapsed().as_secs_f64());
                    if let Some(f) = model.field.as_ref() {
                        line += &format!(" alpha {:.4} sigma {:.4}", f.alpha.to_vec1::<f32>()?[0], f.log_sigma.exp()?.to_vec1::<f32>()?[0]);
                    }
                    writeln!(log, "{line}")?;
                    eprintln!("{line}");
                    if !loss.to_scalar::<f32>()?.is_finite() {
                        bail!("loss is not finite at step {step}");
                    }
                }
            }
            DP_SALT.store(0, Ordering::Relaxed);
            model.varmap.save(out.join("weights.safetensors"))?;
            let val_rows = data::load_split(&e.data, CONFIG.as_str(), "validation")?;
            let mut vstore = EmbeddingStore::open(&e.data, ENCODER, CONFIG.as_str(), "validation")?;
            let idx = validation_rows(&val_rows);
            let (acc, kl) = evaluate(&model, &dec, &tok, &rel, &val_rows, &idx, &mut vstore, &out.join("validation.jsonl"), &dev)?;
            let summary = serde_json::json!({"arm": args[1], "lr": lr, "seed": seed, "train_rows": order.len(),
                "validation_rows": idx.len(), "validation_accuracy": acc, "attention_kl": kl,
                "train_seconds": started.elapsed().as_secs_f64()});
            std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary)?)?;
            println!("{summary}");
        }
        Some("eval") => {
            ensure!(args.len() == 5, "run eval <arm> <weights> <validation|test> <out.jsonl>");
            let arm: Arm = args[1].parse()?;
            let model = build(arm, rel.len(), &dev)?;
            let set = load_into(&model.varmap, Path::new(&args[2]), &dev)?;
            ensure!(set == model.varmap.all_vars().len(), "weights cover {set} of {} variables", model.varmap.all_vars().len());
            let rows = data::load_split(&e.data, CONFIG.as_str(), &args[3])?;
            let idx: Vec<usize> = if args[3] == "validation" { validation_rows(&rows) } else { (0..rows.len()).collect() };
            let mut store = EmbeddingStore::open(&e.data, ENCODER, CONFIG.as_str(), &args[3])?;
            let (acc, kl) = evaluate(&model, &dec, &tok, &rel, &rows, &idx, &mut store, Path::new(&args[4]), &dev)?;
            println!("{}", serde_json::json!({"arm": args[1], "split": args[3], "rows": idx.len(), "accuracy": acc, "attention_kl": kl}));
        }
        _ => bail!("usage: run train|eval|revoke ..."),
    }
    Ok(())
}
