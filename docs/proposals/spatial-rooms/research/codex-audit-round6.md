**Heartbeat renewal: OPEN.** Checked all six supplied files.

Closing text exists: ADR-2031 D2/D3/D7 and ADR-2157 D5 allow “an equal `rev` with a strictly greater `created_at` and identical content”. ADR-2031 Verification now correctly applies “a backdated 9001 inside the admission window […] at its admission position”.

Remaining contradictions:

- **ADR-2031 Verification:** still requires “a same-second pair resolves to the lowest id” for 39002, contradicting revision priority and equivocation rejection. Its watermark test also omits `created_at`.
- **BASELINE:** “replace only on a strictly greater `rev`”, “for 3900x orders by `rev` only”, and watermark “`(rev, event id)`” contradict heartbeat renewal.
- **DATA-authority-erasure:** the proposed ownership row says “only on a strictly greater `["rev"]`, never `created_at`”.
- **PLAN §4:** retains “ordered by `(created_at, id)`” and “break same-second ties by lowest id”. Its corrected heartbeat paragraph and E3 do not reconcile these.
- **ADR-2157 D5:** “Only (`rev`, `created_at`, id) is persisted, not the membership list or the signed payload.” Equal-rev content comparison after restart therefore needs an explicit prerequisite to rehydrate the accepted event, or a persisted content digest.
- **ADR-2030:** no additional heartbeat contradiction.

Automatic approval review blocked memory search and storage because approval policy is `never`; this audit was not persisted.

VERDICT: needs-round-7