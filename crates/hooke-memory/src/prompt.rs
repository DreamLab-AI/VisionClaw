//! Prompts and greedy answers, matching Memorilla's `build_messages` + Qwen3 chat template
//! (`enable_thinking=False`) and its vLLM generation.

use crate::decoder::Decoder;
use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use std::path::Path;
use tokenizers::{AddedToken, Tokenizer};

pub const MEMORY_TOKEN: &str = "<|memory|>";
pub const PERSONA_PROMPT: &str =
    "You are a personalized AI assistant. Answer the question about the user based on your understanding of the user.";
const IM_END: u32 = 151645;
const END_OF_TEXT: u32 = 151643;

/// The rendered generation prompt for one question.
pub fn render(system: &str, question: &str, num_memories: usize) -> String {
    format!(
        "<|im_start|>system\n{system}\nMemory{}Memory<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
        MEMORY_TOKEN.repeat(num_memories),
        question.trim()
    )
}

/// The decoder's tokenizer with the memory placeholder registered as a special token.
pub struct PromptTokenizer {
    tok: Tokenizer,
    pub memory_id: u32,
}

impl PromptTokenizer {
    pub fn load(decoder_dir: &Path) -> Result<Self> {
        let mut tok = Tokenizer::from_file(decoder_dir.join("tokenizer.json")).map_err(|e| anyhow!(e))?;
        tok.add_special_tokens(&[AddedToken::from(MEMORY_TOKEN, true)]);
        let memory_id = tok.token_to_id(MEMORY_TOKEN).ok_or_else(|| anyhow!("memory token not registered"))?;
        Ok(Self { tok, memory_id })
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self.tok.encode(text, false).map_err(|e| anyhow!(e))?.get_ids().to_vec())
    }

    pub fn decode(&self, ids: &[u32]) -> Result<String> {
        self.tok.decode(ids, true).map_err(|e| anyhow!(e))
    }
}

/// Input embeddings with each row's memory tokens spliced in at the placeholder positions.
///
/// `ids` are left-padded rows; `memory` is `[b, m, hidden]` in the decoder dtype.
pub fn splice(dec: &Decoder, ids: &[Vec<u32>], memory: &Tensor, memory_id: u32, dev: &Device) -> Result<(Tensor, Tensor)> {
    let len = ids.iter().map(Vec::len).max().unwrap_or(0);
    let mut rows = Vec::with_capacity(ids.len());
    let mut mask = Vec::with_capacity(ids.len() * len);
    for (r, row) in ids.iter().enumerate() {
        let pad = len - row.len();
        let padded: Vec<u32> = std::iter::repeat_n(END_OF_TEXT, pad).chain(row.iter().copied()).collect();
        mask.extend(std::iter::repeat_n(0f32, pad).chain(std::iter::repeat_n(1f32, row.len())));
        let embeds = dec.embed(&Tensor::new(padded.as_slice(), dev)?.unsqueeze(0)?)?.squeeze(0)?;
        // Contiguous runs of placeholders take the memory tokens in order.
        let slots: Vec<usize> = (0..len).filter(|&i| i >= pad && row[i - pad] == memory_id).collect();
        let mem = memory.get(r)?;
        if slots.len() != mem.dim(0)? {
            return Err(anyhow!("row {r}: {} placeholders for {} memory tokens", slots.len(), mem.dim(0)?));
        }
        let mut pieces = Vec::new();
        let mut cursor = 0;
        let (first, last) = (slots[0], slots[slots.len() - 1]);
        if slots.iter().enumerate().any(|(k, &s)| s != first + k) {
            return Err(anyhow!("row {r}: memory placeholders are not contiguous"));
        }
        if first > cursor {
            pieces.push(embeds.narrow(0, cursor, first - cursor)?);
        }
        pieces.push(mem.to_dtype(embeds.dtype())?);
        cursor = last + 1;
        if cursor < len {
            pieces.push(embeds.narrow(0, cursor, len - cursor)?);
        }
        rows.push(Tensor::cat(&pieces, 0)?);
    }
    Ok((Tensor::stack(&rows, 0)?, Tensor::from_vec(mask, (ids.len(), len), dev)?))
}

/// Greedy answers (no sampling), stopping at `<|im_end|>` or after `max_new` tokens.
pub fn generate(
    dec: &Decoder,
    tok: &PromptTokenizer,
    ids: &[Vec<u32>],
    memory: &Tensor,
    max_new: usize,
    dev: &Device,
) -> Result<Vec<String>> {
    let (mut embeds, mut mask) = splice(dec, ids, memory, tok.memory_id, dev)?;
    let b = ids.len();
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); b];
    let mut done = vec![false; b];
    for _ in 0..max_new {
        let h = dec.forward_embeds(&embeds, &mask)?;
        let last = h.narrow(1, h.dim(1)? - 1, 1)?.squeeze(1)?.contiguous()?;
        let next: Vec<u32> = dec.logits(&last)?.argmax(1)?.to_vec1()?;
        for (r, &t) in next.iter().enumerate() {
            if !done[r] {
                if t == IM_END || t == END_OF_TEXT {
                    done[r] = true;
                } else {
                    out[r].push(t);
                }
            }
        }
        if done.iter().all(|d| *d) {
            break;
        }
        let step = dec.embed(&Tensor::new(next.as_slice(), dev)?.unsqueeze(1)?)?;
        embeds = Tensor::cat(&[&embeds, &step], 1)?;
        mask = Tensor::cat(&[&mask, &Tensor::ones((b, 1), DType::F32, dev)?], 1)?;
    }
    out.iter().map(|ids| Ok(tok.decode(ids)?.trim().to_string())).collect()
}
