# P16 (validation, not preregistered): harder free text where structure gives nothing away

Renderer `scripts/p16_hard.py` (render seed 16): sentences from all companies shuffled into paragraphs of 3-5, so a
paragraph's first company is not the subject of its facts; supply written from either side, including object-first
forms ("Y is a customer of X", "Y's supplier X shipped..."); distractors with the same entity types but no gold fact
(competition, past or planned supply, site visits, future offices, lost bids, "no dealings"). Collections 200-1999 of
the P10 validation corpus. Scored by `scripts/p12d_score.py` (NO_FORMS=1). No model was trained on this renderer.

| extractor | precision | recall | true 1-step | true 2-step | false 1-step | false 2-step |
|---|---|---|---|---|---|---|
| no-learning rule (`p16_eval.py rule`) | 39.6 | 49.3 | 113/426 | 302/466 | 446/474 | 142/434 |
| 0.6B trained on P14 data, seed 1 | 51.2 | 50.4 | 173/426 | 123/466 | 461/474 | 328/434 |
| Qwen3-8B prompted, no reasoning | 76.6 | 69.8 | 112/426 | 283/466 | 474/474 | 413/434 |
| Qwen3-8B reasoning on every paragraph (collections 200-399 only) | 90.3 | 74.0 | 23/41 | 38/52 | 59/59 | 48/48 |

On the same 200 collections: rule 49.3 recall (33/52 true two-step), small 50.2 (10/52), prompted 8B 69.7 (32/52) (`*-sub200.json`).

Reading: the P14 extractor learned the generator's structure, not reading. Even the 8B model reasoning over every
paragraph reaches only 74.0% recall on this text. Acquisition from text whose structure does not give the answer away is
open, for small and large models alike; this is the funded H1.
