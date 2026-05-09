#!/usr/bin/env bash
# Decrypt secrets.enc.yaml and export as environment variables.
# Usage: source scripts/sops-env.sh  (then run your command)
#    or: scripts/sops-env.sh exec -- ./scripts/launch.sh up dev
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
SOPS_BIN="${SCRIPT_DIR}/sops"
SECRETS_FILE="${PROJECT_DIR}/secrets.enc.yaml"

export SOPS_AGE_KEY_FILE="${SOPS_AGE_KEY_FILE:-${HOME}/.config/sops/age/keys.txt}"

if [[ ! -f "$SECRETS_FILE" ]]; then
    echo "ERROR: $SECRETS_FILE not found. Run: sops -e --input-type dotenv --output-type yaml secrets.env > secrets.enc.yaml" >&2
    exit 1
fi

if [[ ! -f "$SOPS_AGE_KEY_FILE" ]]; then
    echo "ERROR: age key not found at $SOPS_AGE_KEY_FILE" >&2
    echo "Generate with: age-keygen -o $SOPS_AGE_KEY_FILE" >&2
    exit 1
fi

if [[ "${1:-}" == "exec" ]]; then
    shift
    [[ "${1:-}" == "--" ]] && shift
    exec "$SOPS_BIN" exec-env "$SECRETS_FILE" "$*"
fi

# Source mode: export each key=value
while IFS=': ' read -r key value; do
    [[ -z "$key" || "$key" == \#* || "$key" == sops* ]] && continue
    value="${value%\"}"
    value="${value#\"}"
    [[ -n "$value" ]] && export "$key=$value"
done < <("$SOPS_BIN" -d "$SECRETS_FILE" 2>/dev/null)

echo "Loaded secrets from $SECRETS_FILE ($(grep -c "ENC\[" "$SECRETS_FILE") encrypted values)"
