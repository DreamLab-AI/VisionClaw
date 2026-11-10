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

## Corrections after independent review (9 November 2026)
* The P12b table above is a post-hoc rescore: name canonicalisation was added after inspecting extraction errors.
  The original run gives precision 88.0, recall 74.3, single-step 88.0, two-step 91.0, score AUC 0.639.
* Test half: 100 single-step and 100 two-step claims. Every false claim was rejected; 31 of 41 true single-step and 43
  of 52 true two-step claims were confirmed (`rescore.json`). Composed accuracy therefore mixes a 100% false-claim
  rate with true-claim recall; report the two separately.
* Recall by sentence form is produced by `scripts/p12_forms.py` (`forms.json`): 23 forms, 6 under 50%, from 11.2%
  to 100%; "X supplies Y" 99.7%. The earlier "95-100% on direct forms" summary was too broad.
* The threshold grid starts at the minimum score, so a tuned threshold that admits everything is not evidence about a
  gate either way.
* P12c's 48.5% is the share of false claims among the first 400 two-step claims: each owner answered False to every
  claim. It is a base rate, not a measured failure. See P13 for composition with the records actually split.
* The 827 figure is the median token count of the records alone (`forms.json`), not of the full prompt.

## P12d: extraction with reasoning on (`scripts/p12d_extract.py`, scored by `scripts/p12d_score.py`)
Same rendering (seed 0) and prompt as P12b, Qwen3-8B with thinking on (temperature 0.6, top-p 0.95, top-k 20, max
6,000 new tokens), test collections 200-399 only, admit all, same canonicalisation (`p12d-score.json`):
precision 95.6, recall 88.7; true claims confirmed 33/41 single-step, 52/52 two-step; false claims rejected 59/59 and
48/48. Generated tokens per paragraph about 1,100 (vLLM throughput x elapsed: 1,377 tok/s x 674 s over 848
paragraphs; 1,528 x 636 s over 846) against about 80 without reasoning (P12b run). Weakest forms: "X has been approved
under C" 27.2%, "X provides C services" 60.3%; inverted supplier forms 84.8-91.7%. P12b scored by the same script on the
same half: `p12b-testhalf-score.json` (matches `rescore.json`).
