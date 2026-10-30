**Heartbeat renewal: OPEN.** Checked all six supplied files.

Closing text:
- **ADR-2031 D2/D3/D7; ADR-2157 D5:** “an equal `rev` with a strictly greater `created_at` and identical content”.
- **ADR-2031 Verification:** now tests that an equal-rev heartbeat “does” replace, and restart “still accepts the next heartbeat and rejects equivocation”.
- **BASELINE, DATA ownership row and PLAN §4/E3:** now explicitly permit equal-rev heartbeats.
- **Restart comparison:** ADR-2031 D3 and ADR-2157 D5 persist “`(rev, created_at, event id, content digest)`”.

Remaining contradictions:
- **ADR-2031 Verification:** still specifies elsewhere “persisted per-kind watermark of `rev` and event id”, omitting `created_at` and digest.
- **BASELINE pending invariants:** still universally prohibits “a same-second higher id” replacing state. A higher-`rev` 3900x must win regardless of id; scope that prohibition to other kinds.
- **ADR-2157 D5/Consequences:** retains “used only for freshness” and “Foreclosed: … ordering by `created_at`”, despite using it to order equal-rev renewals.
- **New digest gap, ADR-2031 D3 / ADR-2157 D5:** “SHA-256 over the canonical JSON of the event's tags” does not cover the event’s `content` field. Require that field to be fixed/empty or include it in the digest to enforce “identical content” after restart.

**ADR-2030:** no additional heartbeat contradiction.

Automatic approval review blocked memory search because approval policy is `never`; this audit was not persisted.

VERDICT: needs-round-8