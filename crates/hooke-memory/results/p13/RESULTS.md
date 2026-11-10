# P13 (exploratory, not preregistered): composition with records split between two owners

P10 validation corpus (seed 2027), P10 embeddings. Owner S holds the supplies records; owner R holds every other
record. Protocol: the requester sends the claim to S; S retrieves top-4 of its own records for "which company supplies
A", reads A's suppliers, and asks R "is B located in L?" for each; R retrieves top-4 of its own records and answers yes
or no. S returns the verdict. R learns each (B, L) asked (2.0 per two-step claim on average); S learns R's answers; the
requester learns only the verdict. Single-step claims are decided by R alone. Script `scripts/p13_split_compose.py`.

| stratum | n | correct | true claims | false claims |
|---|---|---|---|---|
| single-step | 2,000 | 1,994 (99.7%) | 99.4% | 100% |
| two-step | 2,000 | 2,000 (100%) | 100% | 100% |

Reading of records is exact string parsing and decomposition is hand-coded for the two claim templates: an upper
bound for clean records. Supersedes the "48.5% for each owner alone" reading of P12c, which is the base rate of false
claims (both owners answered False to every claim, holding no deciding fact by construction).
