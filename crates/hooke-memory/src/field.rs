//! The Hooke field: slot geometry for the memory module (preregistration P1).
//!
//! Entities of a collection relax under learned per-relation springs and pairwise repulsion
//! for a fixed number of unrolled steps; each triple sits at the midpoint of its entities;
//! each memory slot is mapped into the same space; and the squared slot–triple distance
//! biases cross-attention. Every step is a tensor operation, so the dynamics train end to end.

use anyhow::Result;
use candle_core::{DType, Device, Tensor, D};
use candle_nn::{Init, Linear, Module, VarBuilder};
use std::collections::HashMap;

pub const DIM: usize = 8;
pub const STEPS: usize = 8;
pub const STEP_SIZE: f64 = 0.1;
const REPULSION_EPS: f64 = 0.01;
/// Per-entity step norm cap; keeps the unrolled dynamics finite when repulsion spikes.
const MAX_STEP: f64 = 1.0;

/// The five preregistered arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// No geometry: Memorilla fine-tuned.
    A0,
    /// Learned per-relation law.
    A1,
    /// One shared spring law.
    A2,
    /// A1's field with triple positions permuted within each collection.
    A3,
    /// Free geometry: triple positions from their own embeddings.
    A4,
}

impl std::str::FromStr for Arm {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "A0" => Arm::A0,
            "A1" => Arm::A1,
            "A2" => Arm::A2,
            "A3" => Arm::A3,
            "A4" => Arm::A4,
            other => anyhow::bail!("unknown arm {other}"),
        })
    }
}

/// Relation vocabulary: train relation strings in first-seen order, then UNK.
#[derive(Debug, Clone, Default)]
pub struct Relations {
    index: HashMap<String, usize>,
}

impl Relations {
    pub fn from_triples<'a>(triples: impl Iterator<Item = &'a str>) -> Self {
        let mut index = HashMap::new();
        for t in triples {
            if let Some((_, r, _)) = parse(t) {
                let n = index.len();
                index.entry(r).or_insert(n);
            }
        }
        Self { index }
    }

    pub fn len(&self) -> usize {
        self.index.len() + 1
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    fn id(&self, r: &str) -> usize {
        self.index.get(r).copied().unwrap_or(self.index.len())
    }
}

/// `subject → relation → object`; middle parts join as the relation.
pub fn parse(t: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = t.split(" → ").collect();
    (parts.len() >= 3).then(|| (parts[0].to_string(), parts[1..parts.len() - 1].join(" → "), parts[parts.len() - 1].to_string()))
}

/// The typed graph of one collection, as dense tensors on the device.
pub struct Graph {
    /// `[E, n]`: 1/deg weights averaging triple embeddings into entity features.
    pub mean: Tensor,
    /// `[E, m]`: +1 at a spring's subject, −1 at its object.
    pub signed: Tensor,
    /// `[n, E]`: weights placing each triple at its entities' midpoint (or the centroid).
    pub place: Tensor,
    /// `[m]`: relation index of each spring.
    pub relation: Tensor,
    pub entities: usize,
}

impl Graph {
    pub fn build(docs: &[String], rel: &Relations, dev: &Device) -> Result<Self> {
        let mut ent: HashMap<String, usize> = HashMap::new();
        let parsed: Vec<_> = docs.iter().map(|t| parse(t)).collect();
        for (s, _, o) in parsed.iter().flatten() {
            for e in [s, o] {
                let n = ent.len();
                ent.entry(e.clone()).or_insert(n);
            }
        }
        let e = ent.len().max(1);
        let n = docs.len();
        let springs: Vec<_> = parsed.iter().flatten().map(|(s, r, o)| (ent[s], rel.id(r), ent[o])).collect();
        let m = springs.len();
        let mut mean = vec![0f32; e * n];
        let mut deg = vec![0f32; e];
        let mut place = vec![0f32; n * e];
        for (j, p) in parsed.iter().enumerate() {
            match p {
                Some((s, _, o)) => {
                    let (a, b) = (ent[s], ent[o]);
                    for x in [a, b] {
                        mean[x * n + j] += 1.0;
                        deg[x] += 1.0;
                        place[j * e + x] += 0.5;
                    }
                }
                None => place[j * e..(j + 1) * e].iter_mut().for_each(|v| *v = 1.0 / e as f32),
            }
        }
        for x in 0..e {
            if deg[x] > 0.0 {
                mean[x * n..(x + 1) * n].iter_mut().for_each(|v| *v /= deg[x]);
            }
        }
        let mut signed = vec![0f32; e * m.max(1)];
        let mut relation = vec![0u32; m.max(1)];
        for (k, &(s, r, o)) in springs.iter().enumerate() {
            signed[s * m + k] += 1.0;
            signed[o * m + k] -= 1.0;
            relation[k] = r as u32;
        }
        Ok(Self {
            mean: Tensor::from_vec(mean, (e, n), dev)?,
            signed: Tensor::from_vec(signed, (e, m.max(1)), dev)?,
            place: Tensor::from_vec(place, (n, e), dev)?,
            relation: Tensor::from_vec(relation, m.max(1), dev)?,
            entities: e,
        })
    }
}

/// Learned geometry parameters. Absent for A0.
pub struct Field {
    pub arm: Arm,
    entity: Linear,
    slot: Linear,
    triple: Linear,
    theta: Tensor,
    phi: Tensor,
    repel: Tensor,
    pub alpha: Tensor,
    pub log_sigma: Tensor,
}

impl Field {
    pub fn new(arm: Arm, relations: usize, embed_dim: usize, vb: VarBuilder) -> Result<Self> {
        let lin = |name: &str| -> Result<Linear> {
            let w = vb.get_with_hints((DIM, embed_dim), &format!("{name}.weight"), Init::Randn { mean: 0.0, stdev: 0.02 })?;
            let b = vb.get_with_hints(DIM, &format!("{name}.bias"), Init::Const(0.0))?;
            Ok(Linear::new(w, Some(b)))
        };
        let laws = if arm == Arm::A2 { 1 } else { relations };
        // softplus(0.5413) = 1.0: unit stiffness and rest length; softplus(-4.6) ≈ 0.01 repulsion.
        Ok(Self {
            arm,
            entity: lin("entity")?,
            slot: lin("slot")?,
            triple: lin("triple")?,
            theta: vb.get_with_hints(laws, "theta", Init::Const(0.5413))?,
            phi: vb.get_with_hints(laws, "phi", Init::Const(0.5413))?,
            repel: vb.get_with_hints(1, "repel", Init::Const(-4.6))?,
            alpha: vb.get_with_hints(1, "alpha", Init::Const(0.1))?,
            log_sigma: vb.get_with_hints(1, "log_sigma", Init::Const(0.0))?,
        })
    }

    fn softplus(x: &Tensor) -> Result<Tensor> {
        Ok((x.exp()? + 1.0)?.log()?)
    }

    /// Relaxed entity positions `[E, DIM]`.
    fn relax(&self, g: &Graph, docs: &Tensor) -> Result<Tensor> {
        let mut x = self.entity.forward(&g.mean.matmul(docs)?)?;
        let rel = if self.arm == Arm::A2 { g.relation.zeros_like()? } else { g.relation.clone() };
        let k = Self::softplus(&self.theta)?.index_select(&rel, 0)?.unsqueeze(1)?;
        let l = Self::softplus(&self.phi)?.index_select(&rel, 0)?.unsqueeze(1)?;
        let c = Self::softplus(&self.repel)?;
        let e = g.entities;
        let off_diag = (Tensor::ones((e, e), DType::F32, x.device())? - Tensor::eye(e, DType::F32, x.device())?)?;
        for _ in 0..STEPS {
            // Springs: d = x_s − x_o; dU/dx_s = 2k(r − ℓ) d / r.
            let d = g.signed.t()?.matmul(&x)?;
            let r = (d.sqr()?.sum_keepdim(1)? + 1e-9)?.sqrt()?;
            let pull = (d.broadcast_div(&r)?.broadcast_mul(&(r.broadcast_sub(&l)?.broadcast_mul(&k)? * 2.0)?))?;
            let spring = g.signed.matmul(&pull)?;
            // Repulsion: dU/dx_i = Σ_j −2c (x_i − x_j) / (‖x_i − x_j‖² + ε)².
            let diff = x.unsqueeze(1)?.broadcast_sub(&x.unsqueeze(0)?)?;
            let d2 = (diff.sqr()?.sum(D::Minus1)? + REPULSION_EPS)?;
            let w = (d2.sqr()?.recip()? * off_diag.clone())?.broadcast_mul(&(c.clone() * -2.0)?)?;
            let repel = diff.broadcast_mul(&w.unsqueeze(2)?)?.sum(1)?;
            let step = ((spring + repel)? * STEP_SIZE)?;
            let norm = (step.sqr()?.sum_keepdim(1)? + 1e-12)?.sqrt()?;
            let scale = (norm.clamp(MAX_STEP, f64::INFINITY)?.recip()? * MAX_STEP)?;
            x = (x - step.broadcast_mul(&scale)?)?;
        }
        Ok(x)
    }

    /// Triple positions `[n, DIM]` for one collection.
    pub fn triples(&self, g: &Graph, docs: &Tensor, permutation: Option<&Tensor>) -> Result<Tensor> {
        let pos = match self.arm {
            Arm::A4 => self.triple.forward(docs)?,
            _ => g.place.matmul(&self.relax(g, docs)?)?,
        };
        Ok(match (self.arm, permutation) {
            (Arm::A3, Some(p)) => pos.index_select(p, 0)?,
            _ => pos,
        })
    }

    /// Cross-attention bias `[b, 1, slots, n]` from conditioned slots `[b, slots, d]` and
    /// padded triple positions `[b, n, DIM]`.
    pub fn bias(&self, slots: &Tensor, triples: &Tensor) -> Result<Tensor> {
        let p = self.slot.forward(slots)?;
        let d2 = p.unsqueeze(2)?.broadcast_sub(&triples.unsqueeze(1)?)?.sqr()?.sum(D::Minus1)?;
        let inv = (self.log_sigma.exp()?.sqr()? * 2.0)?.recip()?;
        let scale = (self.alpha.broadcast_mul(&inv)? * -1.0)?;
        d2.broadcast_mul(&scale)?.unsqueeze(1).map_err(Into::into)
    }
}
