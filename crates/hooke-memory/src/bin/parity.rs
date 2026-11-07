//! Gate 1: does the Rust port reproduce Memorilla?
//!
//! Usage: `parity <golden dir> <data root> <qwen3 dir> <python predictions.jsonl> <rows>`
//!
//! 1. Memory tokens: Rust module vs Python on the golden examples (f32 and bf16).
//! 2. Prompts: rendered text and token ids vs Python.
//! 3. End to end: greedy answers on the first `rows` FactKG test rows vs Python's predictions,
//!    with accuracy against the reference answers.

use anyhow::{ensure, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use hooke_memory::{data, decoder::Decoder, memory::{MemoryConfig, MemoryModule}, prompt, store::EmbeddingStore};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Golden {
    memory_token_id: u32,
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    prompt: String,
    input_ids: Vec<u32>,
    collection_id: i64,
}

#[derive(Deserialize)]
struct Prediction {
    question: String,
    prediction: String,
}

fn module(path: &std::path::Path, dtype: DType, dev: &Device) -> Result<MemoryModule> {
    // SAFETY: the converted checkpoint is not modified while mapped.
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[path], dtype, dev)? };
    Ok(MemoryModule::new(MemoryConfig::default(), vb)?)
}

fn max_abs(a: &Tensor, b: &Tensor) -> Result<f32> {
    Ok((a - b)?.abs()?.max_all()?.to_scalar()?)
}

fn cosine(a: &Tensor, b: &Tensor) -> Result<f32> {
    let dot = (a * b)?.sum_all()?.to_scalar::<f32>()?;
    let na = a.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
    let nb = b.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
    Ok(dot / (na * nb))
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    ensure!(a.len() == 5, "usage: parity <golden dir> <data root> <qwen3 dir> <predictions.jsonl> <rows>");
    let (golden_dir, root, qwen, preds) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]), PathBuf::from(&a[2]), PathBuf::from(&a[3]));
    let rows: usize = a[4].parse()?;
    let dev = Device::new_cuda(0)?;
    let golden: Golden = serde_json::from_slice(&std::fs::read(golden_dir.join("golden.json"))?)?;
    let tensors = candle_core::safetensors::load(golden_dir.join("golden.safetensors"), &dev)?;
    let ckpt = golden_dir.join("stage3.safetensors");

    // 1. Memory tokens.
    for (dtype, key) in [(DType::F32, "memory_f32"), (DType::BF16, "memory_bf16")] {
        let m = module(&ckpt, dtype, &dev)?;
        let (mut worst, mut worst_cos) = (0f32, 1f32);
        for i in 0..golden.examples.len() {
            let docs = tensors[&format!("doc_{i}")].to_dtype(dtype)?.unsqueeze(0)?;
            let q = tensors[&format!("question_{i}")].to_dtype(dtype)?.unsqueeze(0)?;
            let ours = m.forward(&docs, None, &q, None, false)?.squeeze(0)?.to_dtype(DType::F32)?;
            let theirs = &tensors[&format!("{key}_{i}")];
            worst = worst.max(max_abs(&ours, theirs)?);
            worst_cos = worst_cos.min(cosine(&ours, theirs)?);
        }
        println!("memory {key}: max |diff| {worst:.3e}, min cosine {worst_cos:.6} over {} examples", golden.examples.len());
    }

    // 2. Prompts.
    let tok = prompt::PromptTokenizer::load(&qwen)?;
    ensure!(tok.memory_id == golden.memory_token_id, "memory token id {} != {}", tok.memory_id, golden.memory_token_id);
    let test = data::load_split(&root, "factkg", "test")?;
    let (mut text_ok, mut ids_ok) = (0, 0);
    for (i, e) in golden.examples.iter().enumerate() {
        ensure!(test[i].collection_id == e.collection_id, "row {i}: collection order differs from Python");
        let p = prompt::render(prompt::PERSONA_PROMPT, &test[i].question, 16);
        text_ok += usize::from(p == e.prompt);
        ids_ok += usize::from(tok.encode(&p)? == e.input_ids);
    }
    println!("prompts: text {text_ok}/{n}, token ids {ids_ok}/{n}", n = golden.examples.len());

    // 3. End to end against Python's predictions.
    let python: Vec<Prediction> = std::fs::read_to_string(&preds)?
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let dec = Decoder::load(&qwen, DType::BF16, &dev)?;
    let m = module(&ckpt, DType::BF16, &dev)?;
    let mut store = EmbeddingStore::open(&root, "qwen3-embedding-4b", "factkg", "test")?;
    let (mut agree, mut correct, mut python_correct, mut docs_match) = (0, 0, 0, 0);
    let batch = 16;
    for start in (0..rows).step_by(batch) {
        let idx: Vec<usize> = (start..(start + batch).min(rows)).collect();
        let mut mems = Vec::new();
        let mut ids = Vec::new();
        for &i in &idx {
            let (v, n, w) = store.documents(test[i].collection_id)?;
            docs_match += usize::from(n == test[i].documents.len());
            let docs = Tensor::from_vec(v, (1, n, w), &dev)?.to_dtype(DType::BF16)?;
            let q = Tensor::from_vec(store.question(i)?, (1, w), &dev)?.to_dtype(DType::BF16)?;
            mems.push(m.forward(&docs, None, &q, None, false)?.squeeze(0)?);
            ids.push(tok.encode(&prompt::render(prompt::PERSONA_PROMPT, &test[i].question, 16))?);
        }
        let memory = Tensor::stack(&mems, 0)?;
        let answers = prompt::generate(&dec, &tok, &ids, &memory, 8, &dev)?;
        for (k, &i) in idx.iter().enumerate() {
            let p = &python[i];
            ensure!(p.question == test[i].question, "row {i}: prediction order differs");
            agree += usize::from(answers[k] == p.prediction.trim());
            correct += usize::from(answers[k] == test[i].answer.trim());
            python_correct += usize::from(p.prediction.trim() == test[i].answer.trim());
        }
        if start % 160 == 0 {
            println!("  {}/{rows}: agree {agree}, rust correct {correct}, python correct {python_correct}", start + idx.len());
        }
    }
    println!(
        "end to end on {rows} rows: answers agree {agree} ({:.2}%), Rust accuracy {:.2}%, Python accuracy {:.2}% (exact match), doc counts match {docs_match}/{rows}",
        100.0 * agree as f64 / rows as f64,
        100.0 * correct as f64 / rows as f64,
        100.0 * python_correct as f64 / rows as f64
    );
    Ok(())
}
