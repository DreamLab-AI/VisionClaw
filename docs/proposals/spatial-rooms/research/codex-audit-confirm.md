**29 CLOSED; 4 PARTIAL; none OPEN. One additional design contradiction.**

This checks the supplied text and governing diffs. The revised `AUDIT-DECISIONS.md` was not supplied and could not be read locally, so its corresponding edits remain unverified. Recorded operator calls are not reopened.

| # | Status | Finding |
|---|---|---|
| 1 | CLOSED | PLAN uses L, S, C and D correctly. |
| 2 | PARTIAL | One sentence still includes voice in the 90-second guarantee. |
| 3 | CLOSED | Snapshot expiry and subsequent session closure distinguished. |
| 4 | CLOSED | SFU key/`iss` validation replaces the audience requirement. |
| 5 | CLOSED | Grace explicitly restricted to voluntary disconnects. |
| 6 | CLOSED | Persisted deadline or conservative removal replaces TTL enforcement. |
| 7 | PARTIAL | Weaker voice model adopted, but some promises contradict cached admission. |
| 8 | CLOSED | State revision and heartbeat ordering distinguished. |
| 9 | CLOSED | Revision-aware storage covers 30312 and the exact catalogue namespace. |
| 10 | CLOSED | Same-kind, same-second snapshot scenario removed. |
| 11 | CLOSED | Clock-skew direction corrected. |
| 12 | CLOSED | Claim namespace includes canonical home key. |
| 13 | CLOSED | Claim removal depends on observed removal or stale authority. |
| 14 | CLOSED | Cohort routing distinguished from direct mutation; 9005 included. |
| 15 | CLOSED | Zone-backed and standalone membership authorities distinguished. |
| 16 | CLOSED | Read-grant inference prohibited; separately granted posting membership preserved. |
| 17 | CLOSED | Endpoint changes under the same key no longer constitute forks. |
| 18 | CLOSED | XR default migration required before activation. |
| 19 | CLOSED | Source table matches the adopted reuse policy. |
| 20 | CLOSED | Exact volume-fit ratio and degenerate-axis exception stated. |
| 21 | CLOSED | Fourth-body trigger and unresolved placement conversion recognised. |
| 22 | CLOSED | Preflight and required backend capability precede volume work. |
| 23 | CLOSED | Feature-specific activation gates replace the universal gate. |
| 24 | CLOSED | E3 uses existing authenticated presence without requiring ADR-2157. |
| 25 | PARTIAL | Publisher prerequisite explicit, but its activation step differs across documents. |
| 26 | CLOSED | Exact registered `space:<nip29-room-urn>` form used. |
| 27 | CLOSED | Descriptive-metadata restriction preserves catalogue contract fields. |
| 28 | CLOSED | Active-entry and withdrawal endpoint validation distinguished. |
| 29 | PARTIAL | Envelope names aligned, but one signer allowlist still excludes announcements. |
| 30 | CLOSED | Primary assertions and disposable received references distinguished. |
| 31 | CLOSED | Disposable session-memory render stores explicitly permitted. |
| 32 | CLOSED | Watermark changelog includes content digest. |
| 33 | CLOSED | Supplied references corrected; PLAN §5a now exists. |

**Remaining text for the four PARTIAL findings**

**#2 — Residual 90-second voice promise.** ADR-2157 D4 still says:

> “The single end-to-end guarantee is D = 90 s: from authoritative removal at the home to closure of every presence session (and, via ADR-2160 and ADR-2161, every voice participant and dataspace stream).”

Its attempted closure immediately says:

> “Voice is weaker, by ruling … D_voice = 2L + S + C = 140 s”

The qualification identifies the intended policy, but the preceding explicit inclusion of voice remains contradictory. Remove voice from that first guarantee.

**#7 — Cached admission cannot support the absolute refresh and reconciliation promises.** ADR-2160 D6’s closing text says:

> “The latest token a removed member can hold was minted before their snapshot went stale”

Yet D3 says:

> “A removed member is never issued a refreshed token.”

D6 also promises:

> “Until then, every rejoin with that token is removed by the reconciler within C”

while the reconciler removes:

> “anyone not currently admitted.”

With a withheld removal and a still-fresh pre-removal snapshot, `RoomAdmission` continues to pass. Refresh can therefore occur, and the reconciler cannot identify that rejoin as unauthorised within C of joining. Qualify these promises by **observed removal or loss of fresh authority**. The accepted weaker voice model itself is not reopened.

**#25 — Conflicting publisher activation steps.** PLAN §5a puts the publisher at step 5:

> “forum ADR-2032 D4 … then VisionClaw ADR-2160.”

ADR-2032 D4 instead says:

> “it goes live with the forum chain at estate step 3”

ADR-2031 D11 repeats step 3. Both establish the voice prerequisite, but disagree about when it activates. Choose one step and align the order.

**#29 — Remaining restrictive signer list.** `IDENTITY-authority-chain`, “Peer node key”, says:

> “it signs catalogues, link notices and retractions through the mesh service's local, uid-gated signing socket, which signs only those payload types.”

The closing contract in ADR-2162 D4 explicitly includes:

> “the three envelope types `dataspace_announce`, `link_notice` and `retraction`.”

The governing document’s “only” list still omits `dataspace_announce`.

**New contradiction introduced by the fixes**

**N1 — The new 140-second voice bound omits permitted clock skew.**

ADR-2031 D4 admits a snapshot while:

> “`now − created_at ≤ L + S` (70 s)”

Its corrected Consequences explicitly permit:

> “a slow mirror clock delays age-based expiry by up to S”

Thus 70 seconds of **measured age** can represent 80 seconds of elapsed time since the last pre-removal snapshot. The new voice calculation nevertheless assumes token issuance ends:

> “at most L + S = 70 s after removal” — ADR-2160 D6.

A permitted counterexample, with the LiveKit API reachable throughout:

- A pre-removal snapshot is signed near real time 0.
- The admission consumer’s clock is 10 seconds behind the home.
- At real time 79 seconds, measured snapshot age is 69 seconds: token minting still passes.
- A 60-second token remains usable until approximately real time 139.
- A rejoin just before expiry can remain connected until the next reconciliation, approximately real time 149.

The stated rules therefore permit voice beyond 140 seconds. With a real 60-second token lifetime, the calculation needs **2L + 2S + C = 150 seconds**, or an enforcement change that makes the promised 140 seconds achievable. This concerns the arithmetic of the accepted model, not the operator’s choice to accept weaker voice revocation.

Shared-memory search and storage were blocked because the tools require approval and this session forbids approvals.

VERDICT: design-contradiction (#7, N1; wording residue: #2, #25, #29).