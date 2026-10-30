**Heartbeat renewal: CLOSED.**

- **CLOSED — restart watermark:** ADR-2031 Verification now specifies “`rev`, `created_at`, event id and content digest”.
- **CLOSED — same-second restriction:** BASELINE scopes it to “for kinds other than 3900x”; “3900x instead orders by (`rev`, `created_at`)”.
- **CLOSED — timestamp role:** ADR-2157 D5 says “`created_at` orders only equal-`rev` heartbeats”; Consequences forecloses “ordering events of different `rev` by `created_at`”.
- **CLOSED — digest coverage:** Both ADRs specify “SHA-256 over the canonical JSON of the pair (`tags`, `content`)”.

No contradiction introduced by these fixes found in the supplied text.

Automatic approval review blocked memory search and storage because approval policy is `never`; audit not persisted.

VERDICT: proposals-ready