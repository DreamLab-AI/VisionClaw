//! A frozen Qwen3 decoder that accepts input embeddings and is differentiable end to end.
//!
//! Vendored from `candle-transformers` 0.11.1 `models/qwen3.rs` (MIT/Apache-2.0). Two changes:
//! the forward pass starts from embeddings, so memory tokens can be spliced in, and the fused
//! softmax, RMSNorm and RoPE kernels are replaced by composed tensor ops so gradients reach the
//! spliced embeddings. SiLU keeps candle's built-in op, whose backward stays finite where a
//! composed `x / (1 + exp(-x))` overflows. There is no KV cache: training and the short-answer evaluation recompute
//! the full sequence.

use candle_core::{DType, Device, Module, Result, Tensor, D};
use candle_nn::{Embedding, Linear, VarBuilder};
use serde::Deserialize;
use std::path::Path;

/// The fields of a Hugging Face Qwen3 `config.json` that the forward pass uses.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub max_position_embeddings: usize,
    pub rope_theta: f64,
    pub rms_norm_eps: f64,
    pub tie_word_embeddings: bool,
}

/// RMSNorm computed in f32 from composed ops, so it has a backward pass.
#[derive(Debug, Clone)]
struct RmsNorm {
    weight: Tensor,
    eps: f64,
}

impl RmsNorm {
    fn new(size: usize, eps: f64, vb: VarBuilder) -> Result<Self> {
        Ok(Self { weight: vb.get(size, "weight")?, eps })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let dtype = x.dtype();
        let x = x.to_dtype(DType::F32)?;
        let scale = (x.sqr()?.mean_keepdim(D::Minus1)? + self.eps)?.sqrt()?.recip()?;
        let w = self.weight.to_dtype(DType::F32)?;
        x.broadcast_mul(&scale)?.broadcast_mul(&w)?.to_dtype(dtype)
    }
}

/// Rotary tables indexed by absolute position.
#[derive(Debug, Clone)]
struct Rotary {
    cos: Tensor,
    sin: Tensor,
}

impl Rotary {
    fn new(cfg: &Config, dev: &Device) -> Result<Self> {
        let half = cfg.head_dim / 2;
        let inv: Vec<f32> = (0..half)
            .map(|i| 1.0 / cfg.rope_theta.powf((2 * i) as f64 / cfg.head_dim as f64) as f32)
            .collect();
        let inv = Tensor::from_vec(inv, (1, half), dev)?;
        let pos = Tensor::arange(0u32, cfg.max_position_embeddings as u32, dev)?
            .to_dtype(DType::F32)?
            .reshape((cfg.max_position_embeddings, 1))?;
        let freqs = pos.broadcast_mul(&inv)?;
        let freqs = Tensor::cat(&[&freqs, &freqs], D::Minus1)?;
        Ok(Self { cos: freqs.cos()?, sin: freqs.sin()? })
    }

    /// cos and sin for `positions` `[batch, seq]`, shaped `[batch, 1, seq, head_dim]`.
    fn tables(&self, positions: &Tensor) -> Result<(Tensor, Tensor)> {
        let (b, s) = positions.dims2()?;
        let flat = positions.flatten_all()?;
        let pick = |t: &Tensor| -> Result<Tensor> {
            let d = t.dim(1)?;
            t.index_select(&flat, 0)?.reshape((b, 1, s, d))
        };
        Ok((pick(&self.cos)?, pick(&self.sin)?))
    }
}

fn rotate(x: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
    let dtype = x.dtype();
    let x = x.to_dtype(DType::F32)?;
    let half = x.dim(D::Minus1)? / 2;
    let x1 = x.narrow(D::Minus1, 0, half)?;
    let x2 = x.narrow(D::Minus1, half, half)?;
    let rotated = Tensor::cat(&[&x2.neg()?, &x1], D::Minus1)?;
    (x.broadcast_mul(cos)? + rotated.broadcast_mul(sin)?)?.to_dtype(dtype)
}

#[derive(Debug, Clone)]
struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
}

impl Attention {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let (h, kv, hd) = (cfg.num_attention_heads, cfg.num_key_value_heads, cfg.head_dim);
        let lin = |i, o, name| -> Result<Linear> { Ok(Linear::new(vb.get((o, i), &format!("{name}.weight"))?, None)) };
        Ok(Self {
            q_proj: lin(cfg.hidden_size, h * hd, "q_proj")?,
            k_proj: lin(cfg.hidden_size, kv * hd, "k_proj")?,
            v_proj: lin(cfg.hidden_size, kv * hd, "v_proj")?,
            o_proj: lin(h * hd, cfg.hidden_size, "o_proj")?,
            q_norm: RmsNorm::new(hd, cfg.rms_norm_eps, vb.pp("q_norm"))?,
            k_norm: RmsNorm::new(hd, cfg.rms_norm_eps, vb.pp("k_norm"))?,
            heads: h,
            kv_heads: kv,
            head_dim: hd,
        })
    }

    fn forward(&self, x: &Tensor, cos: &Tensor, sin: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (b, s, _) = x.dims3()?;
        let split = |t: Tensor, n: usize| t.reshape((b, s, n, self.head_dim));
        let q = self.q_norm.forward(&split(self.q_proj.forward(x)?, self.heads)?)?.transpose(1, 2)?;
        let k = self.k_norm.forward(&split(self.k_proj.forward(x)?, self.kv_heads)?)?.transpose(1, 2)?;
        let v = split(self.v_proj.forward(x)?, self.kv_heads)?.transpose(1, 2)?;
        let q = rotate(&q.contiguous()?, cos, sin)?;
        let k = rotate(&k.contiguous()?, cos, sin)?;
        let groups = self.heads / self.kv_heads;
        let expand = |t: Tensor| -> Result<Tensor> {
            t.unsqueeze(2)?
                .broadcast_as((b, self.kv_heads, groups, s, self.head_dim))?
                .reshape((b, self.heads, s, self.head_dim))
        };
        let (k, v) = (expand(k)?, expand(v)?);
        let scale = 1.0 / (self.head_dim as f64).sqrt();
        let scores = (q.matmul(&k.t()?)?.to_dtype(DType::F32)? * scale)?.broadcast_add(mask)?;
        let probs = candle_nn::ops::softmax(&scores, D::Minus1)?.to_dtype(v.dtype())?;
        let out = probs.matmul(&v)?.transpose(1, 2)?.reshape((b, s, self.heads * self.head_dim))?;
        self.o_proj.forward(&out)
    }
}

#[derive(Debug, Clone)]
struct Layer {
    attn: Attention,
    gate: Linear,
    up: Linear,
    down: Linear,
    ln1: RmsNorm,
    ln2: RmsNorm,
}

impl Layer {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let mlp = vb.pp("mlp");
        let lin = |i, o, name| -> Result<Linear> { Ok(Linear::new(mlp.get((o, i), &format!("{name}.weight"))?, None)) };
        Ok(Self {
            attn: Attention::new(cfg, vb.pp("self_attn"))?,
            gate: lin(cfg.hidden_size, cfg.intermediate_size, "gate_proj")?,
            up: lin(cfg.hidden_size, cfg.intermediate_size, "up_proj")?,
            down: lin(cfg.intermediate_size, cfg.hidden_size, "down_proj")?,
            ln1: RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))?,
            ln2: RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("post_attention_layernorm"))?,
        })
    }

    fn forward(&self, x: &Tensor, cos: &Tensor, sin: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let x = (x + self.attn.forward(&self.ln1.forward(x)?, cos, sin, mask)?)?;
        let h = self.ln2.forward(&x)?;
        &x + self.down.forward(&(self.gate.forward(&h)?.silu()? * self.up.forward(&h)?)?)?
    }
}

/// The frozen decoder. Its weights are plain tensors, never `Var`s, so a backward pass only
/// produces gradients for whatever trainable tensors were spliced into the input.
#[derive(Debug, Clone)]
pub struct Decoder {
    embed: Embedding,
    layers: Vec<Layer>,
    norm: RmsNorm,
    lm_head: Linear,
    rotary: Rotary,
    pub config: Config,
}

impl Decoder {
    /// Load a Hugging Face Qwen3 checkpoint directory (`config.json` plus `*.safetensors`).
    pub fn load(dir: &Path, dtype: DType, dev: &Device) -> anyhow::Result<Self> {
        let config: Config = serde_json::from_slice(&std::fs::read(dir.join("config.json"))?)?;
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "safetensors"))
            .collect();
        files.sort();
        // SAFETY: the checkpoint files are not modified while the process maps them.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&files, dtype, dev)? };
        let m = vb.pp("model");
        let embed_w = m.get((config.vocab_size, config.hidden_size), "embed_tokens.weight")?;
        let layers = (0..config.num_hidden_layers)
            .map(|i| Layer::new(&config, m.pp(format!("layers.{i}"))))
            .collect::<Result<Vec<_>>>()?;
        let lm_w = if config.tie_word_embeddings {
            embed_w.clone()
        } else {
            vb.get((config.vocab_size, config.hidden_size), "lm_head.weight")?
        };
        Ok(Self {
            embed: Embedding::new(embed_w, config.hidden_size),
            layers,
            norm: RmsNorm::new(config.hidden_size, config.rms_norm_eps, m.pp("norm"))?,
            lm_head: Linear::new(lm_w, None),
            rotary: Rotary::new(&config, dev)?,
            config,
        })
    }

    /// Token embeddings `[batch, seq, hidden]` for ids `[batch, seq]`.
    pub fn embed(&self, ids: &Tensor) -> Result<Tensor> {
        self.embed.forward(ids)
    }

    /// Final hidden states `[batch, seq, hidden]` for input embeddings `[batch, seq, hidden]`.
    ///
    /// `attention_mask` is `[batch, seq]` (1 for real tokens, 0 for left padding). Positions
    /// count real tokens only, as in Hugging Face generation and vLLM.
    pub fn forward_embeds(&self, embeds: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        let (b, s, _) = embeds.dims3()?;
        let dev = embeds.device();
        let real = attention_mask.to_dtype(DType::F32)?;
        let positions = (real.cumsum(1)? - 1.0)?.relu()?.to_dtype(DType::U32)?;
        let (cos, sin) = self.rotary.tables(&positions)?;
        let causal: Vec<f32> = (0..s)
            .flat_map(|i| (0..s).map(move |j| if j <= i { 0.0 } else { -1e9 }))
            .collect();
        let causal = Tensor::from_vec(causal, (1, 1, s, s), dev)?;
        let padding = ((real - 1.0)? * 1e9)?.reshape((b, 1, 1, s))?;
        let mask = causal.broadcast_add(&padding)?;
        let mut x = embeds.clone();
        for layer in &self.layers {
            x = layer.forward(&x, &cos, &sin, &mask)?;
        }
        self.norm.forward(&x)
    }

    /// Vocabulary logits (f32) for selected hidden states `[n, hidden]`.
    pub fn logits(&self, hidden: &Tensor) -> Result<Tensor> {
        self.lm_head.forward(hidden)?.to_dtype(DType::F32)
    }
}
