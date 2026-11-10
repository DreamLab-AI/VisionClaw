# P11 (exploratory, not preregistered): decompose, look up, compose

Corpus: the P10 validation set (seed 2027, 4,000 claims, count shortcut removed), P10 embeddings (Qwen3-Embedding-4B).
Each claim is decomposed into single-fact lookups over the owner's own embeddings; retrieved triples are read by exact
string parsing (clean triples only). Script: `scripts/p11_compose.py`.

| k per lookup | single-step | two-step | gold supplier found at hop 1 |
|---|---|---|---|
| 1 | 61.1 | 72.4 | 49.9 |
| 4 | 99.6 | 100.0 | 100.0 |

Reading: once a claim is decomposed into targeted lookups, composition on clean triples is solved, against 51.3%
(learned verifier) and 52.6% (Qwen3-8B with the collection in the prompt) end to end in P10. This is an upper bound:
the reader is a string parser. On free text the reader must be learned, its errors compound across hops, and which
extracted facts to admit becomes the learning problem. That is the open question, not a result.
