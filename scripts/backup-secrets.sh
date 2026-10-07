#!/usr/bin/env bash
# backup-secrets.sh
#
# Scans the project tree for *user-authored* env-class files (not vendor
# test fixtures or bundled CA certs) and bundles them into a timestamped
# zip in ~/workspace/secret-backups/.
#
# Output: ~/workspace/secret-backups/secrets-<branch>-<utc-timestamp>.zip
#         + manifest.txt listing files inside the zip
#
# Note: deliberately does NOT sweep *.key / *.pem / *.secret broadly
# because vendor crates ship test fixtures with those extensions which
# are public and not secrets. If you need to back up TLS keys, add them
# explicitly to EXPLICIT_FILES below.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo nogit)"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="$HOME/workspace/secret-backups"
ZIP_PATH="$BACKUP_DIR/secrets-${BRANCH}-${STAMP}.zip"
MANIFEST="$BACKUP_DIR/secrets-${BRANCH}-${STAMP}.manifest.txt"

mkdir -p "$BACKUP_DIR"

# Tight pattern set: names that are unambiguously user secret config.
INCLUDE_NAMES=(
    '.env'
    '.env.local'
    '.env.mad-source'
    '.env.production'
    '.env.staging'
    '.tempenv'
    'cth.env'
    'settings.local.toml'
    'CLAUDE.local.md'
    'credentials.json'
)

# Treat as templates / examples — never include even if name matches.
EXCLUDE_BASENAMES=(
    '.env.example'
    '.env.template'
    '.env.template.common'
    '.env.template.oci'
    '.env_template'
)

# Directories to prune for performance and noise.
PRUNE_DIRS=(
    'node_modules'
    'target'
    '.git'
    'dist'
    'build'
    'out'
    '.cache'
    '.pnpm-store'
    'venv'
    '.venv'
    '__pycache__'
    'multi-agent-docker/comfyui'
    'multi-agent-docker/.venv'
    '.claude/worktrees'
)

PRUNE_ARGS=()
for d in "${PRUNE_DIRS[@]}"; do
    PRUNE_ARGS+=( -path "./$d" -prune -o )
done
# Also prune any **/.cargo and **/.venv that may live deeper
PRUNE_ARGS+=( -path '*/.cargo/registry' -prune -o )
PRUNE_ARGS+=( -path '*/.venv' -prune -o )
PRUNE_ARGS+=( -path '*/node_modules' -prune -o )

NAME_OR=()
first=1
for p in "${INCLUDE_NAMES[@]}"; do
    if [[ $first -eq 1 ]]; then
        NAME_OR+=( -name "$p" ); first=0
    else
        NAME_OR+=( -o -name "$p" )
    fi
done

TMP_LIST="$(mktemp)"
trap 'rm -f "$TMP_LIST"' EXIT

find . "${PRUNE_ARGS[@]}" -type f \( "${NAME_OR[@]}" \) -print 2>/dev/null \
    | while read -r f; do
        base="$(basename "$f")"
        skip=0
        for ex in "${EXCLUDE_BASENAMES[@]}"; do
            if [[ "$base" == "$ex" ]]; then skip=1; break; fi
        done
        [[ $skip -eq 0 ]] && echo "$f"
      done \
    | sed 's|^\./||' \
    | sort -u > "$TMP_LIST"

count="$(wc -l < "$TMP_LIST")"
if [[ "$count" -eq 0 ]]; then
    echo "No env-class files found. Nothing to back up."
    exit 0
fi

echo "Found $count env-class files; building $ZIP_PATH"

zip -@ "$ZIP_PATH" < "$TMP_LIST" >/dev/null

{
    echo "# Secret backup manifest"
    echo "# Repo:   $PROJECT_ROOT"
    echo "# Branch: $BRANCH"
    echo "# UTC:    $STAMP"
    echo "# Files:  $count"
    echo
    cat "$TMP_LIST"
} > "$MANIFEST"

unzip -t "$ZIP_PATH" >/dev/null && echo "zip integrity: OK"
echo
echo "Backup written:  $ZIP_PATH"
echo "Manifest:        $MANIFEST"
echo
echo "Files included:"
cat "$TMP_LIST"
