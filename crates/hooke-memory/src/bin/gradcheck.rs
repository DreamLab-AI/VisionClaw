//! Finite-difference check of the gradient that training relies on.
//!
//! Usage: `gradcheck <qwen3 dir> <stage3.safetensors>` with `HOOKE_DATA` set. On one FactKG
//! validation row, compares the analytic directional derivative of the answer loss with
//! respect to (a) the spliced memory tokens and (b) the memory module's output projection
//! bias, against central differences.

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor, Var};
use candle_nn::{VarBuilder, VarMap};
use hooke_memory::{data, decoder::Decoder, memory::{MemoryConfig, MemoryModule}, prompt, store::EmbeddingStore};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (qwen, stage3) = (std::path::PathBuf::from(&a[0]), std::path::PathBuf::from(&a[1]));
    let root = std::path::PathBuf::from(std::env::var("HOOKE_DATA")?);
    let dev = Device::new_cuda(0)?;
    let dec = Decoder::load(&qwen, DType::BF16, &dev)?;
    let tok = prompt::PromptTokenizer::load(&qwen)?;
    let rows = data::load_split(&root, "factkg", "validation")?;
    let mut store = EmbeddingStore::open(&root, "qwen3-embedding-4b", "factkg", "validation")?;
    let i = 5;
    let (v, n, w) = store.documents(rows[i].collection_id)?;
    let docs = Tensor::from_vec(v, (1, n, w), &dev)?;
    let q = Tensor::from_vec(store.question(i)?, (1, w), &dev)?;
    let mut ids = tok.encode(&prompt::render(prompt::PERSONA_PROMPT, &rows[i].question, 16))?;
    let p_len = ids.len();
    let mut ans = tok.encode(rows[i].answer.trim())?;
    ans.push(151645);
    ids.extend_from_slice(&ans);
    println!("row {i}: answer {:?}, answer tokens {ans:?}", rows[i].answer);

    // Loss as a function of memory tokens [1,16,4096] (f32, cast to bf16 before splicing).
    let loss_of = |mem: &Tensor| -> Result<Tensor> {
        let (embeds, mask) = prompt::splice(&dec, &[ids.clone()], &mem.to_dtype(DType::BF16)?, tok.memory_id, &dev)?;
        let h = dec.forward_embeds(&embeds, &mask)?.squeeze(0)?;
        let h = h.narrow(0, p_len - 1, ans.len())?.contiguous()?;
        Ok(candle_nn::loss::cross_entropy(&dec.logits(&h)?, &Tensor::new(ans.as_slice(), &dev)?)?)
    };

    // (a) Gradient with respect to the memory tokens themselves.
    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, &dev);
    let module = MemoryModule::new(MemoryConfig::default(), vb)?;
    let tensors = candle_core::safetensors::load(&stage3, &dev)?;
    for (name, var) in varmap.data().lock().unwrap().iter() {
        var.set(tensors.get(name).context("missing")?)?;
    }
    let mem0 = module.forward(&docs, None, &q, None, false)?.detach();
    let mem = Var::from_tensor(&mem0)?;
    let l = loss_of(mem.as_tensor())?;
    let g = l.backward()?.get(mem.as_tensor()).context("no grad")?.clone();
    let gnorm = g.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
    let d = (&g / gnorm as f64)?;
    println!("loss {:.5}, |dL/dmem| {gnorm:.4e}", l.to_scalar::<f32>()?);
    for eps in [0.5f64, 2.0, 8.0] {
        let lp = loss_of(&(&mem0 + (&d * eps)?)?)?.to_scalar::<f32>()? as f64;
        let lm = loss_of(&(&mem0 - (&d * eps)?)?)?.to_scalar::<f32>()? as f64;
        println!("  memory tokens eps {eps}: finite difference {:.4e} vs analytic {gnorm:.4e}", (lp - lm) / (2.0 * eps));
    }

    // (b) Gradient with respect to a memory-module parameter (projection bias), end to end.
    let l = loss_of(&module.forward(&docs, None, &q, None, false)?)?;
    let grads = l.backward()?;
    let data = varmap.data().lock().unwrap();
    let bias = data.get("memory_projection.bias").context("bias var")?;
    let gb = grads.get(bias.as_tensor()).context("no bias grad")?.clone();
    let gbn = gb.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
    let db = (&gb / gbn as f64)?;
    let b0 = bias.as_tensor().detach().copy()?;
    for eps in [0.5f64, 2.0] {
        bias.set(&(&b0 + (&db * eps)?)?)?;
        let lp = loss_of(&module.forward(&docs, None, &q, None, false)?)?.to_scalar::<f32>()? as f64;
        bias.set(&(&b0 - (&db * eps)?)?)?;
        let lm = loss_of(&module.forward(&docs, None, &q, None, false)?)?.to_scalar::<f32>()? as f64;
        bias.set(&b0)?;
        println!("  projection bias eps {eps}: finite difference {:.4e} vs analytic {gbn:.4e}", (lp - lm) / (2.0 * eps));
    }
    Ok(())
}
