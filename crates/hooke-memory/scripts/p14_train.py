"""P14: supervised fine-tuning of the small extractor (full fine-tune, bf16, loss on target tokens only).

Usage: p14_train.py <base model dir> <train jsonl> <out dir> <seed> [epochs] [lr]
"""
import json, random, sys
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

BASE, DATA, OUT, SEED = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
EPOCHS = float(sys.argv[5]) if len(sys.argv) > 5 else 1.0; LR = float(sys.argv[6]) if len(sys.argv) > 6 else 3e-5
torch.manual_seed(SEED); random.seed(SEED)
tok = AutoTokenizer.from_pretrained(BASE); model = AutoModelForCausalLM.from_pretrained(BASE, torch_dtype=torch.bfloat16).cuda()
model.gradient_checkpointing_enable(); model.config.use_cache = False
ex = [json.loads(l) for l in open(DATA)]; random.shuffle(ex)
def encode(e):
    p = tok(e["prompt"], add_special_tokens=False)["input_ids"]; t = tok(e["target"], add_special_tokens=False)["input_ids"] + [tok.eos_token_id]
    return p + t, [-100] * len(p) + t
data = [encode(e) for e in ex]; BS = 32
steps = int(EPOCHS * len(data) / BS)
opt = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.0)
sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: min(1.0, (s + 1) / 50) * max(0.0, 1 - s / steps))
model.train(); MB = 8
for step in range(steps):
    batch = [data[(step * BS + i) % len(data)] for i in range(BS)]; ntok = sum(sum(1 for t in y if t != -100) for _, y in batch)
    for m in range(0, BS, MB):
        mb = batch[m:m + MB]; L = max(len(x) for x, _ in mb)
        ids = torch.tensor([x + [tok.eos_token_id] * (L - len(x)) for x, _ in mb]).cuda()
        lab = torch.tensor([y + [-100] * (L - len(y)) for _, y in mb]).cuda()
        att = torch.tensor([[1] * len(x) + [0] * (L - len(x)) for x, _ in mb]).cuda()
        logits = model(input_ids=ids, attention_mask=att).logits[:, :-1].float()
        loss = torch.nn.functional.cross_entropy(logits.reshape(-1, logits.shape[-1]), lab[:, 1:].reshape(-1), ignore_index=-100, reduction="sum") / ntok
        loss.backward()
    torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0); opt.step(); sched.step(); opt.zero_grad()
    if step % 100 == 0: print(step, steps, round(loss.item() * BS / MB, 4), flush=True)
model.save_pretrained(OUT, safe_serialization=True); tok.save_pretrained(OUT); print("saved", OUT)
