# P15: reasoning extractor token measurement (collections 200-399)

`scripts/p14_extract.py ... 8bthink` on the P12b rendering, collections 200-399 (1,694 paragraphs): Qwen3-8B, thinking
on, temperature 0.6, top-p 0.95, top-k 20, max 6,000 new tokens. Measured: 292,323 prompt and 1,846,739 generated
tokens (172.6 and 1,090.2 per paragraph). Scored by `p12d_score.py`: precision 95.1, recall 89.0; true claims 32/41
single-step and 52/52 two-step; every false claim rejected (a second sampled run of P12d: 95.6 / 88.7).
The planned admission-gate pilot over these paragraphs was not run: on this text the small extractor is near
perfect, so re-reading has nothing to fix; the gate is tested on harder text in the funded work.
