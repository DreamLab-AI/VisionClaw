---
id: ADR-2104
title: Execute the SOPS rollout or formally withdraw ADR-109's acceptance — and document plaintext .env as the interim state either way
date: 2026-09-05
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: an owner decision on branch A or branch B below, a secrets-at-rest incident, or agentbox ADR-2027 landing a custody mechanism this decision would ride on
repo: visionclaw
domain: DATA-authority-erasure
lineage: legacy ADR-109 (SOPS + age, accepted 2026-05-09, never executed); DATA-authority-erasure "Credentials" divergence; agentbox ADR-2027 secret custody/rotation/break-glass (`see` — the adjacent open record, superseded by nothing here)
---

# ADR-2104 — Execute the SOPS rollout or formally withdraw ADR-109's acceptance

## Context

Diagram **VC-22.1** (`22-data-authority-provenance-erasure.md:65`) records that SOPS was
accepted on 2026-05-09 (legacy ADR-109) and never executed: `.env` is plaintext. The
tree at this commit shows a rollout that **started and stopped**: `scripts/sops` is a
43 MB statically-linked SOPS binary dated 2026-05-09 — the acceptance date — and
`.gitignore:236` ignores it. None of ADR-109's four deliverables exist: no `.sops.yaml`,
no `secrets.enc.yaml`, no `scripts/sops-env.sh`, no `.env.example` split. `.env` and
`.env.prod` are plaintext on disk and untracked. An accepted ADR that describes four
artefacts, none of which exist, is worse than no ADR: it makes the estate's own records
unreliable as evidence, which is the failure mode the ledger exists to prevent.

## Decision

**Proposed — this record exists to force a choice, and deliberately does not make it.**
The owner picks branch A or branch B. Both branches are fully specified so either can be
executed without further design; what is **not** permitted is leaving ADR-109 accepted
and unexecuted for a fourth month.

### Branch A — execute the rollout

ADR-109 stays accepted and its four deliverables land, unchanged in substance:
`secrets.enc.yaml` (SOPS/age, committed encrypted), `.sops.yaml` with the operator age
public keys, `scripts/sops-env.sh` wrapping `sops exec-env`, and a `.env.example` holding
only non-secret keys and placeholders. `.env` and `.env.prod` are deleted from every
working tree and deploy host once the encrypted form is in use. `scripts/sops` is either
pinned and vendored deliberately (with a recorded checksum and provenance) or replaced by
a nix-provided `sops`; a 43 MB gitignored binary of unrecorded origin on the secrets path
is not acceptable in either branch. ADR-109's `implementation_status` moves to `complete`
only when the acceptance test below passes.

### Branch B — withdraw the acceptance

ADR-109 is marked **rejected/withdrawn** with a dated note stating it was accepted and
never executed, and that the estate's secrets-at-rest posture is instead governed by
whatever mechanism agentbox ADR-2027 lands. `scripts/sops` is deleted, and its
`.gitignore` entry with it, so no partially-executed rollout remains to be mistaken for a
live one. The plaintext posture becomes an *explicit, dated, owned* decision rather than
a lapse.

### Binding in both branches — the interim state is documented

Until branch A completes, `docs/DATA-authority-erasure.md` states plainly that secrets
are plaintext at rest in `.env`/`.env.prod` with no encryption, no access audit trail and
no rotation history, and names the live secret classes (LLM API keys, GitHub PAT,
database passwords, `SERVER_NOSTR_PRIVKEY`). This sentence is not conditional on which
branch is chosen: under branch B it becomes permanent text rather than interim.

## Consequences

- The estate stops carrying an accepted decision that its own tree contradicts — the
  specific defect this pass was convened to find.
- Branch A costs a key-distribution step for every operator and a deploy-path change, and
  couples to agentbox ADR-2027's custody/rotation design; done alone it encrypts at rest
  without solving rotation or break-glass, which must be stated rather than implied.
- Branch B is cheap and honest but leaves NEW-S1 CRITICAL (the QE fleet finding ADR-109
  was raised against) open by choice, with an owner's name against it.
- Either way, `scripts/sops` leaves the tree in its current form: an unpinned,
  unattributed 43 MB binary sitting beside the secrets it was fetched to encrypt is a
  supply-chain exposure in its own right.
- This ADR supersedes nothing. ADR-109 is amended by whichever branch is taken; agentbox
  ADR-2027 is referenced with `see` and keeps its own scope.

## Verification

`implementation_status: none` — no branch has been chosen. Verified at
`b0bc275f6501aae7751b85a72ce15fe1e730e7e8`:

- `ls scripts/sops` → present, `file` reports a stripped static Go ELF, 43,122,840 bytes,
  mtime 2026-05-09; `git check-ignore -v scripts/sops` → `.gitignore:236`;
  `git ls-files --error-unmatch scripts/sops` → not tracked.
- No `.sops.yaml`, `secrets.enc.yaml`, `scripts/sops-env.sh` or `.env.example` anywhere in
  the tree.
- `.env` and `.env.prod` exist, are plaintext and are untracked; `git ls-files | grep -E '^\.env|sops'`
  matches only `docs/archive/adr/ADR-109-sops-secrets-management.md`.
- `docs/DATA-authority-erasure.md` "Credentials" divergence states the same fact.

**Acceptance test.** This ADR is closed when `decision_status` is no longer `proposed`
and the chosen branch's test passes:

- **Branch A:** a clean checkout with only an operator age key present decrypts and boots
  via `scripts/sops-env.sh` with no `.env` on disk; `sops -d secrets.enc.yaml` succeeds
  for a listed operator and fails for a key not in `.sops.yaml`; `grep -rn` for each live
  secret value across the working tree returns no plaintext hit; ADR-109 reads
  `implementation_status: complete` with a `verified_commit`.
- **Branch B:** ADR-109 reads `decision_status: rejected` with a dated withdrawal note;
  `scripts/sops` and its `.gitignore` line are gone; `docs/DATA-authority-erasure.md`
  carries the plaintext-posture paragraph as a standing statement with an owner.

## Disposition — 2026-10-02

- **Suitability:** fits
- **Priority:** P1 — this cycle (Track C item 10: the fresh-host runbook must say how secrets reach a client's machine)
- **Why:** The record still describes the tree at `95e98ab12`. `scripts/sops` is the same 43,122,840-byte binary dated 9 May. There is no `.sops.yaml`, `secrets.enc.yaml`, `scripts/sops-env.sh` or `.env.example`. `docs/DATA-authority-erasure.md:112-113` still says "NEVER EXECUTED". agentbox ADR-2027 (custody) is still `proposed`/`none`. A fresh host built "by following the runbook literally" will meet this choice on day one, either with an age-encrypted file or by copying a plaintext `.env` onto Trust hardware, so the choice cannot wait for the next cycle.
- **Next:** The owner picks branch A or branch B before the runbook is written. Either way, delete the unattributed `scripts/sops` binary.
