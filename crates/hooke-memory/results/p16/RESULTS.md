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
| Qwen3-8B prompted, no reasoning | [8B] |

Reading: the P14 extractor learned the generator's structure, not reading. Acquisition from text whose structure does
not give the answer away is open for small models; this is the funded H1.
