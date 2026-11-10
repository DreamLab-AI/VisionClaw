//! Gate 0: can a backward pass through the frozen 8B decoder reach spliced-in memory tokens?
//!
//! Usage: `spike <qwen3 checkpoint dir> [steps]`. Prints the gradient norm at the memory
//! positions, whether it is finite and non-zero, and seconds per forward+backward step.

use anyhow::{ensure, Context, Result};
use candle_core::{DType, Device, Tensor, Var, D};
use hooke_memory::decoder::Decoder;
use std::time::Instant;

const MEMORIES: usize = 16;
const PROMPT: usize = 96;
const ANSWER: usize = 8;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().context("usage: spike <qwen3 dir> [steps]")?;
    let steps: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(3);
    let dev = Device::new_cuda(0)?;
    let t = Instant::now();
    let dec = Decoder::load(dir.as_ref(), DType::BF16, &dev)?;
    println!("loaded in {:.1}s", t.elapsed().as_secs_f64());
    let hidden = dec.config.hidden_size;

    let ids: Vec<u32> = (0..PROMPT as u32).map(|i| 1000 + (i * 7919) % 50000).collect();
    let ids = Tensor::from_vec(ids, (1, PROMPT), &dev)?;
    let text = dec.embed(&ids)?;
    let mem = Var::from_tensor(&(Tensor::randn(0f32, 0.02, (1, MEMORIES, hidden), &dev)?))?;
    let targets = ids.narrow(1, PROMPT - ANSWER, ANSWER)?.squeeze(0)?;

    for step in 0..steps {
        let t = Instant::now();
        let embeds = Tensor::cat(
            &[&text.narrow(1, 0, 8)?, &mem.as_tensor().to_dtype(DType::BF16)?, &text.narrow(1, 8, PROMPT - 8)?],
            1,
        )?;
        let len = embeds.dim(1)?;
        let mask = Tensor::ones((1, len), DType::F32, &dev)?;
        let h = dec.forward_embeds(&embeds, &mask)?;
        // Predict the last ANSWER tokens from the positions just before them.
        let h = h.narrow(1, len - ANSWER - 1, ANSWER)?.squeeze(0)?;
        let loss = candle_nn::loss::cross_entropy(&dec.logits(&h)?, &targets)?;
        let grads = loss.backward()?;
        let g = grads.get(mem.as_tensor()).context("no gradient reached the memory tokens")?;
        let norm = g.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
        let per_slot = g.sqr()?.sum(D::Minus1)?.sqrt()?.squeeze(0)?.to_vec1::<f32>()?;
        dev.synchronize()?;
        println!(
            "step {step}: loss {:.4} grad_norm {norm:.3e} finite {} nonzero_slots {}/{MEMORIES} {:.2}s",
            loss.to_scalar::<f32>()?,
            norm.is_finite(),
            per_slot.iter().filter(|v| **v > 0.0).count(),
            t.elapsed().as_secs_f64()
        );
        ensure!(norm.is_finite() && norm > 0.0, "gradient is zero or not finite");
    }
    Ok(())
}
