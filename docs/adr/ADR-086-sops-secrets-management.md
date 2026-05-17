# ADR-086 — SOPS + age for Ecosystem Secrets Management

| Field | Value |
|-------|-------|
| Status | Accepted (2026-05-09) |
| Drives | QE fleet NEW-S1 CRITICAL (plaintext secrets on disk), ADR-081 (key custody) |
| Companion ADRs | ADR-077 (QE policy), ADR-081 (federation key custody & rotation) |
| Affected repos | `VisionClaw`, `agentbox`, `nostr-rust-forum`, `dreamlab-ai-website`, `solid-pod-rs` |

## Context

The VisionClaw `.env` contains 15 live secret values: LLM API keys (OpenAI, DeepSeek, Perplexity, Gemini, HuggingFace), a GitHub PAT, database passwords, and a Nostr private key (`SERVER_NOSTR_PRIVKEY`). All are stored as plaintext on disk with no encryption at rest, no audit trail for access, and no version-controlled history of changes.

The QE fleet audit (PRD-014 addendum) flagged this as **NEW-S1 CRITICAL**. ADR-081 already catalogues 10+ long-lived cryptographic keys across the federated mesh but defers the secrets-at-rest question to tooling. This ADR fills that gap.

Requirements: encrypt secrets at rest, commit the encrypted form to version control, decrypt only at deploy/runtime, and avoid introducing new infrastructure services.

## Decision

**Adopt Mozilla SOPS v3 with age encryption** (Filippo Valsorda's modern alternative to GPG).

### What changes

1. **Split `.env`** into two files:
   - `secrets.enc.yaml` — encrypted via SOPS, committed to git (AES-256-GCM per value)
   - `.env.example` — non-secret env vars and placeholder keys for documentation
2. **One age keypair per operator**: private key at `~/.config/sops/age/keys.txt`, public key listed in `.sops.yaml` (committed).
3. **Helper script** `scripts/sops-env.sh` decrypts and exports secrets at runtime, wrapping `sops exec-env`.
4. **`.sops.yaml`** at repo root defines creation rules mapping `secrets\.enc\.yaml` to the operator's age public key.

### Alternatives rejected

| Alternative | Reason rejected |
|-------------|-----------------|
| HashiCorp Vault | Too much infrastructure for single-operator deployments |
| Docker secrets | No version control, no multi-operator rotation |
| AWS KMS / GCP KMS | Cloud lock-in; ecosystem runs on bare-metal |
| Kubernetes sealed-secrets | Kubernetes-only; VisionClaw uses Docker Compose |
| GPG-based SOPS | GPG keyring complexity, trust model overhead; age is simpler |

## Consequences

### Positive

- Secrets encrypted at rest with AES-256-GCM; safe to commit to git.
- Audit trail: git history tracks every encrypted change with author and timestamp.
- Operator rotation: re-encrypt with a new age public key, update `.sops.yaml`, commit.
- Zero new infrastructure — SOPS and age are each a single static binary.
- Aligns with ADR-081 Tier-1 custody (filesystem + encryption at rest).

### Negative

- Operators must possess the age private key to decrypt; key loss requires re-creation of all secrets from source (provider dashboards).
- The running container still consumes a plaintext `.env` at runtime — migration is incremental (decrypt-then-export).
- SOPS does not enforce access control beyond possession of the age private key.

## Migration Path

```bash
# 1. Generate age keypair
age-keygen -o ~/.config/sops/age/keys.txt

# 2. Copy public key to .sops.yaml
#    creation_rules:
#      - path_regex: secrets\.enc\.yaml$
#        age: "age1..."

# 3. Extract sensitive values from .env into secrets.env
grep -E '^(OPENAI_|DEEPSEEK_|GITHUB_TOKEN|PERPLEXITY_|GEMINI_|HF_|NEO4J_PASSWORD|SERVER_NOSTR_PRIVKEY)' .env > secrets.env

# 4. Encrypt
sops -e --input-type dotenv --output-type yaml secrets.env > secrets.enc.yaml

# 5. Delete plaintext
rm secrets.env

# 6. At runtime: source decrypted secrets before starting services
source scripts/sops-env.sh

# 7. Rotate: re-encrypt with new public key, update .sops.yaml, commit
```

## Files

| File | Purpose |
|------|---------|
| `.sops.yaml` | Creation rules mapping encrypted files to age public keys |
| `secrets.enc.yaml` | Encrypted secrets (15 values), committed |
| `scripts/sops-env.sh` | Decrypt-and-export helper for runtime |
| `.gitignore` | Updated: track `secrets.enc.yaml`, block `secrets.env` and SOPS/age binaries |
