//! Memorilla's memory module, ported from `memorilla/memory.py` (Apache-2.0).
//!
//! Learned slots are conditioned on the question, then refined by pre-norm cross-attention to
//! the documents and a GELU feed-forward block, and projected into the decoder's embedding
//! space. Parameter names match the PyTorch state dict, so a converted `memory.pt` loads
//! directly. The optional `geometry` closure is where slot geometry enters (see `field`);
//! without it the module is Memorilla's.

use candle_core::{DType, Result, Tensor, D};
use candle_nn::{LayerNorm, Linear, Module, VarBuilder};

/// Architecture of the released Stage 3 checkpoint.
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    pub embedding_dim: usize,
    pub output_dim: usize,
    pub num_memories: usize,
    pub num_heads: usize,
    pub num_cross_attn_layers: usize,
    pub dropout: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self { embedding_dim: 2560, output_dim: 4096, num_memories: 16, num_heads: 8, num_cross_attn_layers: 2, dropout: 0.1 }
    }
}

const FFN_EXPANSION: usize = 4;
const LN_EPS: f64 = 1e-5;

fn layer_norm(dim: usize, vb: VarBuilder) -> Result<LayerNorm> {
    Ok(LayerNorm::new(vb.get(dim, "weight")?, vb.get(dim, "bias")?, LN_EPS))
}

fn linear(i: usize, o: usize, vb: VarBuilder) -> Result<Linear> {
    Ok(Linear::new(vb.get((o, i), "weight")?, Some(vb.get(o, "bias")?)))
}

fn dropout(x: &Tensor, p: f32, train: bool) -> Result<Tensor> {
    if train && p > 0.0 { candle_nn::ops::dropout(x, p) } else { Ok(x.clone()) }
}

/// `torch.nn.MultiheadAttention` with `batch_first=True` and a packed input projection.
#[derive(Debug, Clone)]
struct MultiheadAttention {
    in_w: Tensor,
    in_b: Tensor,
    out: Linear,
    heads: usize,
    dim: usize,
}

impl MultiheadAttention {
    fn new(dim: usize, heads: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            in_w: vb.get((3 * dim, dim), "in_proj_weight")?,
            in_b: vb.get(3 * dim, "in_proj_bias")?,
            out: linear(dim, dim, vb.pp("out_proj"))?,
            heads,
            dim,
        })
    }

    fn project(&self, x: &Tensor, part: usize) -> Result<Tensor> {
        let (b, l, _) = x.dims3()?;
        let w = self.in_w.narrow(0, part * self.dim, self.dim)?;
        let bias = self.in_b.narrow(0, part * self.dim, self.dim)?;
        let hd = self.dim / self.heads;
        x.broadcast_matmul(&w.t()?)?
            .broadcast_add(&bias)?
            .reshape((b, l, self.heads, hd))?
            .transpose(1, 2)?
            .contiguous()
    }

    fn probs(&self, query: &Tensor, kv: &Tensor, key_mask: Option<&Tensor>, bias: Option<&Tensor>) -> Result<Tensor> {
        let lk = kv.dim(1)?;
        let b = kv.dim(0)?;
        let (q, k) = (self.project(query, 0)?, self.project(kv, 1)?);
        let mut logits = (q.matmul(&k.t()?)? * (1.0 / ((self.dim / self.heads) as f64).sqrt()))?;
        if let Some(bias) = bias {
            logits = logits.broadcast_add(bias)?;
        }
        if let Some(mask) = key_mask {
            logits = logits.broadcast_add(&((mask.to_dtype(logits.dtype())? - 1.0)? * 1e9)?.reshape((b, 1, 1, lk))?)?;
        }
        candle_nn::ops::softmax(&logits, D::Minus1)
    }

    /// `query` `[b, lq, d]`, `kv` `[b, lk, d]`; `key_mask` `[b, lk]` is 1 for real keys;
    /// `bias` broadcasts to `[b, heads, lq, lk]` and is added to the logits.
    fn forward(
        &self,
        query: &Tensor,
        kv: &Tensor,
        key_mask: Option<&Tensor>,
        bias: Option<&Tensor>,
        p: f32,
        train: bool,
    ) -> Result<Tensor> {
        let (b, lq, _) = query.dims3()?;
        let lk = kv.dim(1)?;
        let (q, k, v) = (self.project(query, 0)?, self.project(kv, 1)?, self.project(kv, 2)?);
        let scale = 1.0 / ((self.dim / self.heads) as f64).sqrt();
        let mut logits = (q.matmul(&k.t()?)? * scale)?;
        if let Some(bias) = bias {
            logits = logits.broadcast_add(bias)?;
        }
        if let Some(mask) = key_mask {
            let neg = ((mask.to_dtype(logits.dtype())? - 1.0)? * 1e9)?.reshape((b, 1, 1, lk))?;
            logits = logits.broadcast_add(&neg)?;
        }
        let probs = dropout(&candle_nn::ops::softmax(&logits, D::Minus1)?, p, train)?;
        let out = probs.matmul(&v)?.transpose(1, 2)?.reshape((b, lq, self.dim))?;
        self.out.forward(&out)
    }
}

#[derive(Debug, Clone)]
struct FeedForward {
    up: Linear,
    down: Linear,
}

/// The memory module.
#[derive(Debug, Clone)]
pub struct MemoryModule {
    queries: Tensor,
    query_cond_attn: MultiheadAttention,
    query_cond_norm: LayerNorm,
    doc_norm: LayerNorm,
    cross_norms: Vec<LayerNorm>,
    cross_attn: Vec<MultiheadAttention>,
    ffn_norms: Vec<LayerNorm>,
    ffn: Vec<FeedForward>,
    final_norm: LayerNorm,
    projection: Linear,
    pub config: MemoryConfig,
}

impl MemoryModule {
    /// Build from a `VarBuilder` whose names follow Memorilla's state dict.
    pub fn new(config: MemoryConfig, vb: VarBuilder) -> Result<Self> {
        let d = config.embedding_dim;
        let n = config.num_cross_attn_layers;
        let per = |prefix: &str, f: &dyn Fn(VarBuilder) -> Result<LayerNorm>| -> Result<Vec<LayerNorm>> {
            (0..n).map(|i| f(vb.pp(format!("{prefix}.{i}")))).collect()
        };
        Ok(Self {
            queries: vb.get((1, config.num_memories, d), "memory_queries")?,
            query_cond_attn: MultiheadAttention::new(d, config.num_heads, vb.pp("query_cond_attn"))?,
            query_cond_norm: layer_norm(d, vb.pp("query_cond_norm"))?,
            doc_norm: layer_norm(d, vb.pp("doc_norm"))?,
            cross_norms: per("cross_attn_norms", &|v| layer_norm(d, v))?,
            cross_attn: (0..n)
                .map(|i| MultiheadAttention::new(d, config.num_heads, vb.pp(format!("cross_attn_layers.{i}"))))
                .collect::<Result<_>>()?,
            ffn_norms: per("ffn_norms", &|v| layer_norm(d, v))?,
            ffn: (0..n)
                .map(|i| -> Result<FeedForward> {
                    let f = vb.pp(format!("ffn_layers.{i}"));
                    Ok(FeedForward {
                        up: linear(d, d * FFN_EXPANSION, f.pp("0"))?,
                        down: linear(d * FFN_EXPANSION, d, f.pp("3"))?,
                    })
                })
                .collect::<Result<_>>()?,
            final_norm: layer_norm(d, vb.pp("layer_norm"))?,
            projection: linear(d, config.output_dim, vb.pp("memory_projection"))?,
            config,
        })
    }

    /// Memory tokens `[b, num_memories, output_dim]`.
    ///
    /// `docs` `[b, n, d]`, `doc_mask` `[b, n]` (1 for real documents), `question` `[b, d]`.
    /// `geometry`, given the question-conditioned slots `[b, num_memories, d]`, returns a bias
    /// broadcasting to `[b, heads, num_memories, n]` that is added to every cross-attention
    /// layer's logits.
    pub fn forward(
        &self,
        docs: &Tensor,
        doc_mask: Option<&Tensor>,
        question: &Tensor,
        geometry: Option<&dyn Fn(&Tensor) -> Result<Tensor>>,
        train: bool,
    ) -> Result<Tensor> {
        let b = docs.dim(0)?;
        let p = self.config.dropout;
        let q = question.unsqueeze(1)?;
        let slots = self.queries.broadcast_as((b, self.config.num_memories, self.config.embedding_dim))?.contiguous()?;
        let cond = self.query_cond_attn.forward(&slots, &q, None, None, p, train)?;
        let mut slots = self.query_cond_norm.forward(&(slots + cond)?)?;
        let doc_bias = geometry.map(|g| g(&slots)).transpose()?;
        let doc_bias = doc_bias.as_ref();
        let docs = self.doc_norm.forward(docs)?;
        for i in 0..self.config.num_cross_attn_layers {
            let attended = self.cross_attn[i].forward(&self.cross_norms[i].forward(&slots)?, &docs, doc_mask, doc_bias, p, train)?;
            slots = (slots + dropout(&attended, p, train)?)?;
            let h = self.ffn[i].up.forward(&self.ffn_norms[i].forward(&slots)?)?.gelu_erf()?;
            let h = self.ffn[i].down.forward(&dropout(&h, p, train)?)?;
            slots = (slots + dropout(&h, p, train)?)?;
        }
        self.projection.forward(&self.final_norm.forward(&slots)?)
    }

    /// First cross-attention layer's attention probabilities `[b, heads, slots, n]` in
    /// evaluation mode, with and without geometry: the engagement diagnostic of P1.
    pub fn first_layer_probs(
        &self,
        docs: &Tensor,
        doc_mask: Option<&Tensor>,
        question: &Tensor,
        geometry: &dyn Fn(&Tensor) -> Result<Tensor>,
    ) -> Result<(Tensor, Tensor)> {
        let b = docs.dim(0)?;
        let slots = self.queries.broadcast_as((b, self.config.num_memories, self.config.embedding_dim))?.contiguous()?;
        let cond = self.query_cond_attn.forward(&slots, &question.unsqueeze(1)?, None, None, 0.0, false)?;
        let slots = self.query_cond_norm.forward(&(slots + cond)?)?;
        let bias = geometry(&slots)?;
        let docs = self.doc_norm.forward(docs)?;
        let q = self.cross_norms[0].forward(&slots)?;
        let with = self.cross_attn[0].probs(&q, &docs, doc_mask, Some(&bias))?;
        let without = self.cross_attn[0].probs(&q, &docs, doc_mask, None)?;
        Ok((with, without))
    }

    /// Cast-free helper: the dtype the module's parameters were loaded in.
    pub fn dtype(&self) -> DType {
        self.queries.dtype()
    }
}
