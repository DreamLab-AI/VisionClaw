| Item | Status | Closing text / remaining contradiction |
|---|---|---|
| Heartbeat renewal | **OPEN** | ADR-2031 D3 now accepts “an equal `rev` with a strictly greater `created_at` and identical content”. But its Verification still requires “consecutive snapshots of one kind have strictly increasing `rev`” and “a backdated 9001 is refused”, contradicting D2’s heartbeat and admission-window rules. |
| D = 90 s / partition exception | **CLOSED** | ADR-2157 D4: “during a LiveKit API partition, voice participants stay connected until the API returns”. ADR-2160 activation blockers now qualify D with “while the LiveKit API is reachable” and require removal “within C of the API returning”. |
| Link endpoint ownership | **CLOSED** | ADR-2161 D5: “the subject is under the asserting node's key; the object is under the receiving node's key”. D6 and ADR-2162 D7 enforce those same owners. |
| Durable catalogue withdrawal / restart replay | **CLOSED** | ADR-2161 D2 explicitly accepts withdrawal with “no endpoint” on “signature, pinned key and greater `rev` alone”; its “persisted watermark per (K, space id)” prevents restart replay resurrecting the space. |

Memory search and storage were blocked by automatic approval policy; this audit was not persisted.

VERDICT: needs-round-6