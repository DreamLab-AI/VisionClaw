# P12 (exploratory, not preregistered): reasoning, free text and two owners

All on the P10 validation corpus (seed 2027; count shortcut removed). Qwen3-8B, vLLM 0.33 (eager), bf16.

## P12a: plain text with reasoning on (`scripts/p12_think.py`)
Full collection in the prompt, thinking on (temperature 0.6, top-p 0.95, top-k 20, max 8,192 new tokens), first 400
two-step and 200 single-step claims: **98.0% two-step, 99.0% single-step**; all 600 outputs closed reasoning and
parsed. Median per two-step claim: 827 prompt tokens and 532 generated tokens. With reasoning off (P10): 52.6 / 79.2.
Reading: a language model composes two private facts when it can read all the text in one place.

## P12c: the same model, facts split between two owners (`scripts/p12_split.py`)
Owner "supply" holds only the supplies records; owner "registry" holds every other record (locations, capabilities,
certifications). Each owner's model, thinking on, sees only its own documents: **48.5% two-step for each owner**
(it answers False to every claim it cannot confirm). Pooling both halves (P12a) gives 98.0%, by giving one party all
the text. Composition across the owners, one single-fact lookup per hop: 100.0% on clean facts (P11).

## P12b: free text, extracted at ingest (`scripts/p12_freetext.py`, offline rescoring `scripts/p12_rescore.py`)
400 collections rendered as prose (paragraph per company, varied templates, pronouns, 30% of companies with one
rumour, negation, past location or plan). Qwen3-8B extracts triples per paragraph (thinking off, greedy); the same
model scores each with logit(Yes) - logit(No). Test half (200 collections), canonical names (template suffixes
"services", "capability", "certification" stripped; partnership symmetric):

| admission | fact precision | fact recall | single-step | two-step |
|---|---|---|---|---|
| admit all extracted | 91.3 | 77.0 | 90.0 | 91.0 |
| score threshold tuned on dev | 91.3 | 77.0 | 90.0 | 91.0 (tuned threshold admits all) |
| schema domain/range check | 94.4 | 77.0 | 90.0 | 91.0 |
| gold facts (oracle) | 100 | 100 | 100 | 100 |

Score AUC for correct against wrong candidates: 0.733. Recall by sentence form (replayed rendering): inverted
supplier forms 11.2% ("{o} sources components from {s}"), 13.4% ("Among {o}'s suppliers is {s}"), 19.5% ("{o} buys
parts from {s}"); "{s} provides {c} services" 29.3%; direct forms 95-100%. The gap to the oracle is missed facts in
paraphrases that invert or bury the relation, not wrong admissions; a prompted model's own confidence does not fix it.
