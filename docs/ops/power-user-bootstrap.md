# Power-user Pod bootstrap

`vc-cli bootstrap-power-user` is a one-shot command that seeds a power-user's
Pod with GitHub credentials taken from a local `.env` file. It writes one
JSON document to `<pod>/private/config/github` using a NIP-98 signed HTTP
PUT.

This document explains *when* to use it, *what* it expects, and *how* to run
it safely. See ADR-030 for the larger Pod architecture context.

---

## When to use

Run this exactly once per power-user to initialise their Pod's GitHub
configuration, or again whenever the underlying `GITHUB_TOKEN` rotates.
Running it a second time overwrites the existing config; the CLI prompts
interactively unless you pass `--force`.

Typical triggers:

- First onboarding of a power user onto a fresh Pod.
- GitHub PAT expired or was revoked.
- Repo / branch / base-path selection changed.
- Moving the power user from the old `.env` flow onto the Solid-style Pod
  credential store.

---

## Inputs

The CLI reads five keys from the supplied `.env` file:

| Key                | Example                                               | Notes |
|--------------------|-------------------------------------------------------|-------|
| `GITHUB_OWNER`     | `jjohare`                                             | GitHub login |
| `GITHUB_REPO`      | `logseq`                                              | Repository name |
| `GITHUB_BRANCH`    | `main`                                                | Branch to track |
| `GITHUB_BASE_PATH` | `mainKnowledgeGraph/pages,workingGraph/pages`         | Comma-separated **or** repeated over multiple lines |
| `GITHUB_TOKEN`     | `ghp_xxx...`                                          | Personal access token |

Lines starting with `#` and blank lines are ignored. An optional `export `
prefix is stripped. Surrounding single or double quotes are stripped off
values.

In addition, the CLI needs two pieces of ambient configuration:

| Source                          | Provides           | Resolution order |
|---------------------------------|--------------------|-----------------|
| `--pubkey HEX` or `POWER_USER_PUBKEY` | owner x-only pubkey | flag → env |
| `--pod-url URL` or `POD_BASE_URL`     | Pod root URL        | flag → `${POD_BASE_URL}/{pubkey}` |

And one signing key (exactly one is required unless `--dry-run`):

| Source                 | Format     | Use case |
|------------------------|------------|----------|
| `SERVER_NOSTR_PRIVKEY` | 64-char hex | Normal operations — server signs on behalf of the user |
| `POWER_USER_NSEC`      | `nsec1…`   | One-shot bootstrap — user signs directly |

If both are set, `SERVER_NOSTR_PRIVKEY` wins.

---

## Examples

### Dry run (recommended first step)

```bash
vc-cli bootstrap-power-user \
  --env /secure/path/.env \
  --pubkey 79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798 \
  --pod-url https://pods.visionclaw.org/79be66...81798 \
  --dry-run
```

Prints the target URL and a **redacted** copy of the payload (token shown
as `ghp_****1234`) without making any HTTP request. Use this to verify the
five parsed values are what you expect.

### Live write, server-signed

```bash
SERVER_NOSTR_PRIVKEY=$(cat /etc/visionclaw/server.sk) \
  vc-cli bootstrap-power-user \
    --env ~/.visionclaw/power-user.env \
    --pubkey 79be66...81798 \
    --force
```

`POD_BASE_URL` defaults to the production pod gateway; `--force` skips the
interactive confirmation (required when running from CI or a wrapper
script that does not attach a TTY).

### Live write, user-signed (one-time)

```bash
POWER_USER_NSEC="nsec1..." \
  vc-cli bootstrap-power-user \
    --env ~/.visionclaw/power-user.env \
    --pubkey 79be66...81798 \
    --pod-url https://pods.visionclaw.org/79be66...81798
```

Useful during local bring-up before the server has a configured Nostr
signing key.

---

## Security notes

The GitHub token is treated as high-sensitivity throughout:

- It lives in memory in a `Zeroizing<String>` wrapper; the backing buffer
  is explicitly wiped on drop.
- It is **never** written to stdout, stderr, or log output. The only code
  path that prints token information uses `mask_token()`, which emits
  `first4****last4` (for example `ghp_****1234`). Short tokens (<8 chars)
  are redacted to `****`.
- On any error, the CLI prints the **error message and target URL only**;
  the serialised payload (which would contain the token) is never echoed.
- The token is transmitted in a single TLS-wrapped PUT and is not retried
  on 4xx/5xx failures.
- `--dry-run` short-circuits before any HTTP request, so you can safely
  use it to verify configuration against production URLs.

The CLI treats the NIP-98 signing material with the same care — it is
loaded into `Zeroizing<String>` and never printed.

---

## Rotation

To rotate the GitHub token:

1. Generate a new PAT in GitHub with the required scopes.
2. Update `GITHUB_TOKEN=` in the existing `.env` file.
3. Re-run `vc-cli bootstrap-power-user` with `--force`. The Pod endpoint
   overwrites `/private/config/github` in place.
4. Verify with `--dry-run` that the new masked token trailer matches what
   you just generated.

No other state needs to change; the server will pick up the new token on
its next read.

---

## Exit codes

| Code | Meaning |
|------|---------|
| 0    | PUT succeeded (or dry-run completed) |
| 1    | Any error: missing input, invalid pubkey, signing failure, non-2xx response, aborted by user |

Errors are printed to stderr; stdout is used only for the success marker
(`✓ Wrote <url>`) and the dry-run payload dump.
