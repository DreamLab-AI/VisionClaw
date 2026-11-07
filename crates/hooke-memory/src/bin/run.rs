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
use rand::{rngs::StdRng, SeedableRng};
use std::io::Write;
use std::path::{Path, PathBuf};

const ENCODER: &str = "qwen3-embedding-4b";
const BATCH: usize = 8;
const EVAL_BATCH: usize = 16;
const WARMUP: usize = 100;
const GEOMETRY_LR: f64 = 1e-3;
const TRAIN_SHARDS: i64 = 3;
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
}

fn build(arm: Arm, relations: usize, dev: &Device) -> Result<Model> {
    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, dev);
    let cfg = MemoryConfig::default();
    let memory = MemoryModule::new(cfg, vb.clone())?;
    let field = (arm != Arm::A0).then(|| Field::new(arm, relations, cfg.embedding_dim, vb.pp("field"))).transpose()?;
    Ok(Model { varmap, memory, field })
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

fn normalise(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !c.is_ascii_punctuation()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Padded batch inputs for the memory module, plus triple positions when the arm needs them.
struct Batch {
    docs: Tensor,
    mask: Tensor,
    question: Tensor,
    triples: Option<Tensor>,
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
        let (v, n, w) = store.documents(row.collection_id)?;
        ensure!(n == row.documents.len(), "collection {}: {n} embeddings for {} documents", row.collection_id, row.documents.len());
        docs.push(Tensor::from_vec(v, (n, w), dev)?);
        lens.push(n);
        questions.push(Tensor::from_vec(store.question(row_index)?, w, dev)?);
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
    let mask: Vec<f32> = lens.iter().flat_map(|&n| (0..n_max).map(move |i| if i < n { 1.0 } else { 0.0 })).collect();
    Ok(Batch {
        docs: Tensor::stack(&docs.iter().zip(&lens).map(|(d, &n)| pad(d, n)).collect::<Result<Vec<_>>>()?, 0)?,
        mask: Tensor::from_vec(mask, (rows.len(), n_max), dev)?,
        question: Tensor::stack(&questions, 0)?,
        triples,
    })
}

fn memory_tokens(m: &Model, b: &Batch, train: bool) -> Result<Tensor> {
    let geometry = |slots: &Tensor| -> candle_core::Result<Tensor> {
        let f = m.field.as_ref().unwrap();
        f.bias(slots, b.triples.as_ref().unwrap()).map_err(|e| candle_core::Error::Msg(e.to_string()))
    };
    let g: Option<&dyn Fn(&Tensor) -> candle_core::Result<Tensor>> = m.field.as_ref().map(|_| &geometry as _);
    Ok(m.memory.forward(&b.docs, Some(&b.mask), &b.question, g, train)?)
}

fn validation_rows(rows: &[Row]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    idx.shuffle(&mut StdRng::seed_from_u64(VALIDATION_SEED));
    idx.truncate(VALIDATION_ROWS);
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
    let (mut correct, mut kl_sum, mut kl_n) = (0usize, 0f64, 0usize);
    let mut scramble = |row: &Row, n: usize| -> Vec<u32> {
        let mut p: Vec<u32> = (0..n as u32).collect();
        p.shuffle(&mut StdRng::seed_from_u64(SCRAMBLE_EVAL_SEED ^ row.collection_id as u64));
        p
    };
    for chunk in idx.chunks(EVAL_BATCH) {
        let items: Vec<(usize, &Row)> = chunk.iter().map(|&i| (i, &rows[i])).collect();
        let b = batch(&items, store, m.field.as_ref(), rel, &mut scramble, dev)?;
        let mem = memory_tokens(m, &b, false)?.to_dtype(DType::BF16)?;
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let e = env()?;
    let dev = Device::new_cuda(0)?;
    let train_rows = data::load_split(&e.data, "factkg", "train")?;
    let rel = Relations::from_triples(train_rows.iter().flat_map(|r| r.documents.iter().map(String::as_str)));
    eprintln!("relations: {} (incl. UNK)", rel.len());
    let tok = PromptTokenizer::load(&e.qwen)?;
    let dec = Decoder::load(&e.qwen, DType::BF16, &dev)?;
    match args.first().map(String::as_str) {
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
            let mut store = EmbeddingStore::open(&e.data, ENCODER, "factkg", "train")?;
            let mut order: Vec<usize> =
                (0..train_rows.len()).filter(|&i| store.shard(train_rows[i].collection_id).is_some_and(|s| s <= TRAIN_SHARDS)).collect();
            eprintln!("training rows in shards 0-{TRAIN_SHARDS}: {}", order.len());
            order.shuffle(&mut StdRng::seed_from_u64(1000 + seed));
            if let Some(m) = max_rows {
                order.truncate(m);
            }
            let (mut mem_vars, mut geo_vars): (Vec<Var>, Vec<Var>) = (Vec::new(), Vec::new());
            for (name, var) in model.varmap.data().lock().unwrap().iter() {
                if name.starts_with("field.") { geo_vars.push(var.clone()) } else { mem_vars.push(var.clone()) }
            }
            let params = |lr| ParamsAdamW { lr, weight_decay: 0.01, ..Default::default() };
            let mut opt_mem = AdamW::new(mem_vars, params(lr))?;
            let mut opt_geo = (!geo_vars.is_empty()).then(|| AdamW::new(geo_vars, params(GEOMETRY_LR))).transpose()?;
            let mut rng = StdRng::seed_from_u64(seed);
            let mut log = std::fs::File::create(out.join("train.log"))?;
            let started = std::time::Instant::now();
            for (step, chunk) in order.chunks(BATCH).enumerate() {
                let warm = ((step + 1) as f64 / WARMUP as f64).min(1.0);
                opt_mem.set_learning_rate(lr * warm);
                if let Some(o) = opt_geo.as_mut() {
                    o.set_learning_rate(GEOMETRY_LR * warm);
                }
                let items: Vec<(usize, &Row)> = chunk.iter().map(|&i| (i, &train_rows[i])).collect();
                let mut scramble = |_: &Row, n: usize| -> Vec<u32> {
                    let mut p: Vec<u32> = (0..n as u32).collect();
                    p.shuffle(&mut rng);
                    p
                };
                let b = batch(&items, &mut store, model.field.as_ref(), &rel, &mut scramble, &dev)?;
                let mem = memory_tokens(&model, &b, true)?.to_dtype(DType::BF16)?;
                let (mut ids, mut targets, mut spans) = (Vec::new(), Vec::new(), Vec::new());
                for (_, r) in &items {
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
                let hidden = dec.forward_embeds(&embeds, &mask)?;
                let mut pick = Vec::new();
                for (r, (row_ids, &(p_len, a_len))) in ids.iter().zip(&spans).enumerate() {
                    let pad = len - row_ids.len();
                    pick.extend((0..a_len).map(|j| (r * len + pad + p_len - 1 + j) as u32));
                }
                let flat = hidden.reshape((ids.len() * len, hidden.dim(2)?))?;
                let h = flat.index_select(&Tensor::new(pick.as_slice(), &dev)?, 0)?;
                let loss = candle_nn::loss::cross_entropy(&dec.logits(&h)?, &Tensor::new(targets.as_slice(), &dev)?)?;
                let grads = loss.backward()?;
                opt_mem.step(&grads)?;
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
            model.varmap.save(out.join("weights.safetensors"))?;
            let val_rows = data::load_split(&e.data, "factkg", "validation")?;
            let mut vstore = EmbeddingStore::open(&e.data, ENCODER, "factkg", "validation")?;
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
            let rows = data::load_split(&e.data, "factkg", &args[3])?;
            let idx: Vec<usize> = if args[3] == "validation" { validation_rows(&rows) } else { (0..rows.len()).collect() };
            let mut store = EmbeddingStore::open(&e.data, ENCODER, "factkg", &args[3])?;
            let (acc, kl) = evaluate(&model, &dec, &tok, &rel, &rows, &idx, &mut store, Path::new(&args[4]), &dev)?;
            println!("{}", serde_json::json!({"arm": args[1], "split": args[3], "rows": idx.len(), "accuracy": acc, "attention_kl": kl}));
        }
        _ => bail!("usage: run train|eval ..."),
    }
    Ok(())
}
