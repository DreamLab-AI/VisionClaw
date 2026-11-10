"""Golden reference for the hooke-memory Rust port, produced with Memorilla's own code.

Usage: golden.py <memorilla clone> <data root> <stage3 checkpoint dir> <qwen3-8b dir> <out dir> [rows]

Writes to <out dir>:
  stage3.safetensors  the memory module state dict (f32), converted from memory.pt (weights_only)
  golden.safetensors  per example i: doc_{i} [n,2560] f32, question_{i} [2560] f32,
                      memory_f32_{i} [16,4096], memory_bf16_{i} [16,4096] (as f32)
  golden.json         per example: row, collection_id, prompt text, input_ids, answer, choices
"""

import json
import sys
from pathlib import Path

clone, data_root, ckpt, decoder, out = (Path(a) for a in sys.argv[1:6])
rows = int(sys.argv[6]) if len(sys.argv) > 6 else 64
sys.path.insert(0, str(clone))

import torch  # noqa: E402
from safetensors.torch import save_file  # noqa: E402
from transformers import AutoTokenizer  # noqa: E402

from memorilla.benchmarks import PERSONA_PROMPT  # noqa: E402
from memorilla.data import apply_chat_template, build_messages  # noqa: E402
from memorilla.embeddings import load_split_with_embeddings  # noqa: E402
from memorilla.memory import MemoryModule  # noqa: E402
from memorilla.utils import load_memory_state, prepare_tokenizer  # noqa: E402

out.mkdir(parents=True, exist_ok=True)
state = load_memory_state(ckpt / "memory.pt")
save_file({k: v.float().contiguous() for k, v in state.items()}, str(out / "stage3.safetensors"))

module32 = MemoryModule.from_pretrained(ckpt).float().eval()
module16 = MemoryModule.from_pretrained(ckpt).to(torch.bfloat16).eval()
tok = AutoTokenizer.from_pretrained(decoder)
memory_id = prepare_tokenizer(tok)

dataset, store = load_split_with_embeddings(
    "factkg", "test", data_root, ["collection_id", "question", "answer", "choices"], "Qwen/Qwen3-Embedding-4B"
)
tensors, meta = {}, []
with torch.no_grad():
    for i in range(rows):
        row = dataset[i]
        docs = store.documents(int(row["collection_id"])).float()
        question = store.question(i).float()
        m32 = module32(docs[None], None, question[None])[0]
        m16 = module16(docs[None].bfloat16(), None, question[None].bfloat16())[0].float()
        messages = build_messages(row["question"], PERSONA_PROMPT, num_memories=module32.num_memories)
        prompt = apply_chat_template(tok, messages, add_generation_prompt=True)
        ids = tok(prompt)["input_ids"]
        tensors |= {f"doc_{i}": docs.contiguous(), f"question_{i}": question.contiguous(),
                    f"memory_f32_{i}": m32.contiguous(), f"memory_bf16_{i}": m16.contiguous()}
        meta.append({"row": i, "collection_id": int(row["collection_id"]), "prompt": prompt, "input_ids": ids,
                     "answer": row["answer"], "choices": list(row["choices"])})
save_file(tensors, str(out / "golden.safetensors"))
json.dump({"memory_token_id": memory_id, "examples": meta}, open(out / "golden.json", "w"))
print(f"wrote {rows} examples, memory token id {memory_id}, state keys {len(state)}")
print("first prompt:", repr(meta[0]["prompt"][:400]))
print("state shapes:", {k: tuple(v.shape) for k, v in list(state.items())[:40]})
