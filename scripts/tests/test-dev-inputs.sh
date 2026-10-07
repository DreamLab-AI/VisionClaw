#!/usr/bin/env bash
# test-dev-inputs.sh — the dev container always sees the host's current Cargo
# manifests and Vite root files, and never sees a secret (ADR-2008, 2026-10-07).
#
# A single-file bind mount pins the host inode at container start; git replaces
# files with a new inode, so after a merge the container kept the old Cargo.toml
# while src/ showed the new code (`unresolved import subtle`). The fix publishes
# the seven root files into the gitignored directory .dev-inputs/ (a directory
# mount sees renames), stamped with the source commit, and the container copies
# them into /app only when the stamp's digest matches.
#
# Sections (no Docker needed; LIVE=1 adds read-only checks on the running
# container via `docker exec`/`docker inspect`):
#   1. compose: no single-file bind, no checkout mount, .dev-inputs read-only,
#      compose-hash label on both app services;
#   2. secrets: no .env*, GH or cth.env under any host path the dev service
#      mounts (SECRETS_ROOT=<checkout> checks a real checkout's untracked files);
#   3. publish (host side) and the git hooks that republish after merge/checkout;
#   4. verify + install (container side): a half-published set is refused;
#   5. the wrapper refuses to build without a valid stamp, logs the stamp, builds
#      --locked, and never cargo-cleans on a lock mismatch;
#   6. launch.sh: redeploy picks up a Cargo.toml change; redeploy refuses and
#      `up` recreates when the compose config drifted from the container label;
#      needs_recompile sees manifests and crates;
#   7. wiring: entrypoint, supervisor program, prune list, hot-patch list.
#
#   bash scripts/tests/test-dev-inputs.sh
#   SECRETS_ROOT=/path/to/checkout bash scripts/tests/test-dev-inputs.sh
#   LIVE=1 bash scripts/tests/test-dev-inputs.sh      # after deploying

# Each check is a deferred expression run by eval, so single quotes are the
# point (SC2016) and variables read only inside them look unused (SC2034).
# shellcheck disable=SC2016,SC2034

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE="${COMPOSE:-$ROOT/docker-compose.unified.yml}"
LIB="$ROOT/scripts/lib/dev-inputs.sh"
WRAPPER="$ROOT/scripts/rust-backend-wrapper.sh"
LAUNCH="$ROOT/scripts/launch.sh"
SECRETS_ROOT="${SECRETS_ROOT:-$ROOT}"
INPUTS=(Cargo.toml Cargo.lock build.rs client/index.html client/vite.config.ts client/tsconfig.json client/postcss.config.cjs)
SECRET_NAMES=(-name '.env*' -o -name GH -o -name cth.env)

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1"; }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
same() { [[ "$(sha256sum < "$1")" == "$(sha256sum < "$2")" ]]; }
command -v yq >/dev/null || { echo "yq (mikefarah v4) is required"; exit 2; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Host bind sources of a compose service, one per line, with the project-root
# variable resolved to $2. Short syntax "src:dst[:mode]" and long-syntax binds.
# The project-root variable is substituted BEFORE splitting on ':', because
# "${HOST_PROJECT_ROOT:-.}" contains a colon of its own.
bind_sources() {
    {
        yq -r ".services.\"$1\".volumes[] | select(tag == \"!!str\")" "$COMPOSE" |
            sed "s|\${HOST_PROJECT_ROOT:-.}|$2|" | cut -d: -f1
        yq -r ".services.\"$1\".volumes[] | select(tag == \"!!map\" and .type == \"bind\") | .source" "$COMPOSE" |
            sed "s|\${HOST_PROJECT_ROOT:-.}|$2|"
    } | grep '^/'
}

echo "== 1. compose: directory binds only, no checkout mount"
dev_binds="$(bind_sources visionclaw "$ROOT")"
check "dev service bind sources parsed" '[[ -n "$dev_binds" ]]'
single=()
while read -r src; do
    rel="${src#"$ROOT"/}"
    # A source that is a file in git, or a path whose name is one of the inputs.
    if [[ -f "$src" ]] || printf '%s\n' "${INPUTS[@]}" | grep -qxF "$rel"; then single+=("$rel"); fi
done <<< "$dev_binds"
check "no single-file bind mounts (found: ${single[*]:-none})" '[[ ${#single[@]} -eq 0 ]]'
check "the checkout root itself is not mounted" '! grep -qx "$ROOT" <<< "$dev_binds"'
check "no /app/.host mount" '! grep -q "/app/\.host" "$COMPOSE"'
dev_inputs_mount="$(yq -r '.services.visionclaw.volumes[] | select(tag == "!!map" and .target == "/app/.dev-inputs") |
    [.type, .source, (.read_only|tostring), (.bind.create_host_path|tostring)] | join(" ")' "$COMPOSE")"
check ".dev-inputs is a read-only directory bind that is never auto-created (got: $dev_inputs_mount)" \
    '[[ "$dev_inputs_mount" == "bind \${HOST_PROJECT_ROOT:-.}/.dev-inputs true false" ]]'
check ".dev-inputs/ is gitignored" 'git -C "$ROOT" check-ignore -q .dev-inputs/Cargo.toml'
for svc in visionclaw visionclaw-production; do
    label="$(yq -r ".services.\"$svc\".labels[\"visionclaw.compose-hash\"] // \"\"" "$COMPOSE")"
    check "$svc carries the compose-hash label (got: $label)" '[[ "$label" == "\${VISIONCLAW_COMPOSE_HASH:-}" ]]'
done

echo "== 2. secrets: nothing secret-bearing under any dev bind source ($SECRETS_ROOT)"
while read -r src; do
    [[ -e "$src" ]] || continue
    found="$(find "$src" \( -name node_modules -o -name target -o -name .git \) -prune -o \( "${SECRET_NAMES[@]}" \) -print 2>/dev/null | head -5)"
    rel="${src#"$SECRETS_ROOT"/}"
    check "no secret files under $rel${found:+ (found: $found)}" '[[ -z "$found" ]]'
done < <(bind_sources visionclaw "$SECRETS_ROOT")

echo "== 3. publish (host side)"
proj="$TMP/proj"; mkdir -p "$proj/client" "$proj/scripts/lib"
for f in "${INPUTS[@]}"; do printf 'v1 %s\n' "$f" > "$proj/$f"; done
printf 'SECRET=1\n' > "$proj/.env"; printf 'tok\n' > "$proj/GH"; printf 'x\n' > "$proj/client/.env"
cp "$LIB" "$proj/scripts/lib/dev-inputs.sh"
git -C "$proj" init -q && git -C "$proj" add -A . && git -C "$proj" -c user.email=t@t -c user.name=t commit -qm init
head1="$(git -C "$proj" rev-parse HEAD)"
out="$(bash "$LIB" --publish "$proj" 2>&1)"; rc=$?
check "publish succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
for f in "${INPUTS[@]}"; do
    check "publish copies $f" 'same "$proj/$f" "$proj/.dev-inputs/$f"'
done
listing="$(cd "$proj/.dev-inputs" && find . -type f | sed 's|^\./||' | sort | tr '\n' ' ')"
expected="$(printf '%s\n' "${INPUTS[@]}" .stamp | sort | tr '\n' ' ')"
check "only the seven inputs and .stamp are published (got: $listing)" '[[ "$listing" == "$expected" ]]'
check "no secret is published (ls -a of .dev-inputs and its client/)" \
    '! ls -a "$proj/.dev-inputs" "$proj/.dev-inputs/client" | grep -qE "^(\.env.*|GH|cth\.env)$"'
check "stamp records the source commit" 'grep -qx "source_sha=$head1" "$proj/.dev-inputs/.stamp"'
check "stamp records a clean tree" 'grep -qx "source_dirty=0" "$proj/.dev-inputs/.stamp"'
lock_mtime="$(stat -c %Y "$proj/.dev-inputs/Cargo.lock")"; touch -d '2001-01-01' "$proj/.dev-inputs/Cargo.lock"
lock_mtime="$(stat -c %Y "$proj/.dev-inputs/Cargo.lock")"
printf 'v2 subtle\n' > "$proj/Cargo.toml.tmp" && mv -f "$proj/Cargo.toml.tmp" "$proj/Cargo.toml"
rm "$proj/build.rs"
out="$(bash "$LIB" --publish "$proj" 2>&1)"
check "republish picks up a git-style replaced Cargo.toml" 'grep -q subtle "$proj/.dev-inputs/Cargo.toml"'
check "republish removes an input deleted on the host" '[[ ! -e "$proj/.dev-inputs/build.rs" ]]'
check "republish leaves unchanged inputs untouched (mtime)" '[[ "$(stat -c %Y "$proj/.dev-inputs/Cargo.lock")" == "$lock_mtime" ]]'
check "stamp marks the uncommitted change as dirty" 'grep -qx "source_dirty=1" "$proj/.dev-inputs/.stamp"'
check "republish verifies" 'bash "$LIB" --verify "$proj/.dev-inputs" >/dev/null 2>&1'

echo "== 3b. git hooks republish after checkout/merge, only in a deployed checkout"
for h in post-checkout post-merge post-rewrite; do
    check ".githooks/$h exists and is executable" '[[ -x "$ROOT/.githooks/$h" ]]'
done
hk="$TMP/hk"; mkdir -p "$hk/client" "$hk/scripts/lib"
for f in "${INPUTS[@]}"; do printf 'v1 %s\n' "$f" > "$hk/$f"; done
cp "$LIB" "$hk/scripts/lib/dev-inputs.sh"; cp -r "$ROOT/.githooks" "$hk/.githooks"
git -C "$hk" init -q -b main && git -C "$hk" config core.hooksPath .githooks
git -C "$hk" add -A . && git -C "$hk" -c user.email=t@t -c user.name=t commit -qm init --no-verify
git -C "$hk" checkout -q -b feature 2>/dev/null
check "a checkout that never published gets no .dev-inputs from the hook" '[[ ! -d "$hk/.dev-inputs" ]]'
bash "$LIB" --publish "$hk" >/dev/null 2>&1
printf '[dependencies]\nsubtle = "2"\n' > "$hk/Cargo.toml"
git -C "$hk" -c user.email=t@t -c user.name=t commit -qam "add subtle" --no-verify
check "a commit alone does not republish (no hook for it)" '! grep -q subtle "$hk/.dev-inputs/Cargo.toml"'
git -C "$hk" checkout -q main
git -C "$hk" checkout -q feature
check "post-checkout republishes the checked-out Cargo.toml" 'grep -q subtle "$hk/.dev-inputs/Cargo.toml"'
git -C "$hk" checkout -q main
check "post-checkout back to main republishes main's Cargo.toml" 'same "$hk/Cargo.toml" "$hk/.dev-inputs/Cargo.toml"'
git -C "$hk" -c user.email=t@t -c user.name=t merge -q --no-edit feature >/dev/null 2>&1
check "post-merge republishes the merged Cargo.toml" 'grep -q subtle "$hk/.dev-inputs/Cargo.toml"'
check "post-merge stamp names the merge result" 'grep -qx "source_sha=$(git -C "$hk" rev-parse HEAD)" "$hk/.dev-inputs/.stamp"'

echo "== 4. verify + install (container side)"
app="$TMP/app"; mkdir -p "$app/client"
printf 'stale\n' > "$app/Cargo.toml"
out="$(DEV_INPUTS_DIR="$proj/.dev-inputs" APP_ROOT="$app" bash "$LIB" --once 2>&1)"; rc=$?
check "install succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
check "install replaces the stale app Cargo.toml" 'same "$proj/.dev-inputs/Cargo.toml" "$app/Cargo.toml"'
check "install copies vite.config.ts as a regular file (Vite resolves symlinks)" \
    '[[ -f "$app/client/vite.config.ts" && ! -L "$app/client/vite.config.ts" ]]'
check "install logs the source commit" 'grep -q "source_sha=" <<< "$out"'
printf 'half-published\n' > "$proj/.dev-inputs/Cargo.toml"
before="$(sha256sum < "$app/Cargo.toml")"
out="$(DEV_INPUTS_DIR="$proj/.dev-inputs" APP_ROOT="$app" DEV_INPUTS_VERIFY_RETRIES=1 bash "$LIB" --once 2>&1)"; rc=$?
check "a digest mismatch (half-published set) fails install (rc=$rc)" '[[ $rc -ne 0 ]]'
check "a refused install leaves /app untouched" '[[ "$(sha256sum < "$app/Cargo.toml")" == "$before" ]]'
out="$(DEV_INPUTS_DIR="$TMP/absent" APP_ROOT="$app" DEV_INPUTS_VERIFY_RETRIES=1 bash "$LIB" --once 2>&1)"; rc=$?
check "a missing .dev-inputs fails and names launch.sh (rc=$rc)" '[[ $rc -ne 0 ]] && grep -q "launch.sh" <<< "$out"'
bash "$LIB" --publish "$proj" >/dev/null 2>&1

echo "== 5. wrapper: stamp gate, --locked, no clean on lock mismatch"
w="$TMP/w"; mkdir -p "$w/bin" "$w/app/src" "$w/app/client"
printf 'fn main() {}\n' > "$w/app/src/main.rs"
printf '[package]\nname = "stale"\n' > "$w/app/Cargo.toml"
mkdir -p "$w/src/client"
for f in "${INPUTS[@]}"; do printf 'x\n' > "$w/src/$f"; done
printf '[package]\nname = "fresh"\n' > "$w/src/Cargo.toml"
git -C "$w/src" init -q && git -C "$w/src" add -A . && git -C "$w/src" -c user.email=t@t -c user.name=t commit -qm i
bash "$LIB" --publish "$w/src" >/dev/null 2>&1
printf '#!/bin/sh\necho 8.9\n' > "$w/bin/nvidia-smi"
cat > "$w/bin/cargo" <<'EOF'
#!/bin/bash
echo "$* | manifest=$(tr '\n' ' ' < "$APP_ROOT/Cargo.toml")" >> "$APP_ROOT/cargo.calls"
[[ "$1" == clean ]] && exit 0
if [[ "${LOCK_MISMATCH:-0}" == 1 ]]; then
    echo "error: cannot update the lock file $APP_ROOT/Cargo.lock because --locked was passed to prevent this" >&2
    exit 101
fi
mkdir -p "$APP_ROOT/target/dev-runtime"
printf '#!/bin/sh\necho backend-started\n' > "$APP_ROOT/target/dev-runtime/visionclaw-server"
chmod +x "$APP_ROOT/target/dev-runtime/visionclaw-server"
EOF
chmod +x "$w/bin/"*
run_wrapper() {
    env APP_ROOT="$w/app" DEV_INPUTS_DIR="$w/src/.dev-inputs" PATH="$w/bin:$PATH" DEV_INPUTS_VERIFY_RETRIES=1 \
        SKIP_RUST_REBUILD=false BUILD_FEATURES=gpu,ontology,dev-auth "$@" bash "$WRAPPER" 2>&1
}
out="$(run_wrapper)"; rc=$?
check "wrapper starts the backend (rc=$rc)" '[[ $rc -eq 0 ]] && grep -q backend-started <<< "$out"'
check "cargo built against the published manifest, not the stale copy" 'grep -q "name = \"fresh\"" "$w/app/cargo.calls"'
check "cargo build passes --locked" 'grep -q "^build .*--locked" "$w/app/cargo.calls"'
check "wrapper logs the stamp's source commit" 'grep -q "source_sha=$(git -C "$w/src" rev-parse HEAD)" <<< "$out"'
rm -f "$w/app/cargo.calls" "$w/app/target/dev-runtime/visionclaw-server"
out="$(run_wrapper LOCK_MISMATCH=1)"; rc=$?
check "lockfile mismatch fails the wrapper (rc=$rc)" '[[ $rc -ne 0 ]]'
check "lockfile mismatch does not cargo clean the target cache" '! grep -q "^clean" "$w/app/cargo.calls"'
check "lockfile mismatch names the remedy" 'grep -q "Cargo.lock" <<< "$out" && grep -q "redeploy" <<< "$out"'
rm -f "$w/app/cargo.calls"
rm "$w/src/.dev-inputs/.stamp"
out="$(run_wrapper)"; rc=$?
check "a missing stamp makes the wrapper refuse (rc=$rc)" '[[ $rc -ne 0 ]] && grep -q "refusing" <<< "$out"'
check "a refused build never reaches cargo" '[[ ! -e "$w/app/cargo.calls" ]]'

echo "== 6. launch.sh: redeploy and compose drift (fake docker)"
lp="$TMP/lp"; mkdir -p "$lp/scripts/lib" "$lp/bin" "$lp/src" "$lp/crates" "$lp/client/src" "$lp/agentbox"
cp "$LAUNCH" "$lp/scripts/launch.sh"; cp "$ROOT/scripts/lib/"*.sh "$lp/scripts/lib/"; cp "$COMPOSE" "$lp/"
printf 'X=1\n' > "$lp/.env"
for f in "${INPUTS[@]}"; do printf 'v1 %s\n' "$f" > "$lp/$f"; done
git -C "$lp" init -q && git -C "$lp" add -A . && git -C "$lp" -c user.email=t@t -c user.name=t commit -qm i
cat > "$lp/bin/docker" <<'EOF'
#!/bin/bash
printf '%s | HASH=%s\n' "$*" "${VISIONCLAW_COMPOSE_HASH-unset}" >> "$DOCKER_CALLS"
case "$1" in
  --version) echo "Docker version 0.0.0-fake" ;;
  ps) [[ "${FAKE_RUNNING:-1}" == 1 ]] && echo visionclaw_container ;;
  images) echo "visionclaw-visionclaw latest" ;;
  inspect)
    case "$*" in
      *compose-hash*) echo "${FAKE_LABEL:-}" ;;
      *Health*) echo healthy ;;
      *StartedAt*) echo "${FAKE_STARTED:-2099-01-01T00:00:00Z}" ;;
      *Created*) echo "2099-01-01T00:00:00Z" ;;
      # Real docker: other formats on an existing container print nothing.
      *) [[ "$*" == *visionclaw_container* ]] || exit 1 ;;
    esac ;;
  compose)
    [[ "$2" == version ]] && echo "Docker Compose version v0.0.0-fake"
    [[ "$*" == *" config "* || "$*" == *" config" ]] && printf 'resolved-config %s\n' "${FAKE_CONFIG:-v1}" ;;
esac
exit 0
EOF
printf '#!/bin/sh\nexit 0\n' > "$lp/bin/sleep"; printf '#!/bin/sh\nexit 1\n' > "$lp/bin/nvidia-smi"
chmod +x "$lp/bin/"*
good_hash="$(printf 'resolved-config v1\n' | sha256sum | cut -d' ' -f1)"
: > "$lp/docker.calls"
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="$good_hash" bash "$lp/scripts/launch.sh" redeploy dev 2>&1)"; rc=$?
check "redeploy dev succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
check "redeploy publishes .dev-inputs" 'same "$lp/Cargo.toml" "$lp/.dev-inputs/Cargo.toml"'
check "redeploy restarts only rust-backend via supervisorctl" \
    'grep -q "^exec visionclaw_container supervisorctl -c /app/supervisord.dev.conf restart rust-backend" "$lp/docker.calls"'
check "redeploy never recreates, stops or restarts the container" \
    '! grep -qE "^compose .* (up|stop|start|down)( |$)|^restart |^rm " "$lp/docker.calls"'
printf '[dependencies]\nsubtle = "2"\n' > "$lp/Cargo.toml.new" && mv -f "$lp/Cargo.toml.new" "$lp/Cargo.toml"
: > "$lp/docker.calls"
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="$good_hash" bash "$lp/scripts/launch.sh" redeploy dev 2>&1)"; rc=$?
check "redeploy after a Cargo.toml change succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
check "redeploy picks up the Cargo.toml change" 'grep -q subtle "$lp/.dev-inputs/Cargo.toml"'
check "and restarts rust-backend to build it" 'grep -q "restart rust-backend" "$lp/docker.calls"'
: > "$lp/docker.calls"
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="stale-hash" bash "$lp/scripts/launch.sh" redeploy dev 2>&1)"; rc=$?
check "redeploy refuses when the compose config drifted (rc=$rc)" '[[ $rc -ne 0 ]] && grep -q "up dev" <<< "$out"'
check "a refused redeploy restarts nothing" '! grep -q "restart rust-backend" "$lp/docker.calls"'
: > "$lp/docker.calls"
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="stale-hash" bash "$lp/scripts/launch.sh" up dev 2>&1)"; rc=$?
check "up dev with a drifted running container succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
check "up dev recreates via compose up with the new hash label" \
    'grep -q "unified.yml .* up -d --remove-orphans | HASH=$good_hash" "$lp/docker.calls"'
: > "$lp/docker.calls"
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="$good_hash" bash "$lp/scripts/launch.sh" up dev 2>&1)"; rc=$?
check "up dev with a current running container succeeds (rc=$rc)" '[[ $rc -eq 0 ]]'
check "up dev leaves a current container alone (no compose up)" '! grep -q "unified.yml .* up -d" "$lp/docker.calls"'
check "the hash is taken from the resolved config of the app service only" \
    'grep -q "unified.yml --profile dev config visionclaw | HASH=$" "$lp/docker.calls"'
: > "$lp/docker.calls"
rm -f "$lp/.dev-inputs/.redeployed"   # sources are newer than a 2010 start and no redeploy since
out="$(env -i PATH="$lp/bin:$PATH" HOME="$lp" DOCKER_CALLS="$lp/docker.calls" FAKE_LABEL="$good_hash" FAKE_STARTED="2010-01-01T00:00:00Z" bash "$lp/scripts/launch.sh" up dev 2>&1)"; rc=$?
check "up dev with newer sources redeploys rust-backend instead of stop/start (rc=$rc)" \
    '[[ $rc -eq 0 ]] && grep -q "restart rust-backend" "$lp/docker.calls" && ! grep -qE "^compose .* (stop|start) visionclaw" "$lp/docker.calls"'

echo "== 6b. needs_recompile sees manifests and crates"
nr="$TMP/nr"; mkdir -p "$nr/src" "$nr/client/src" "$nr/crates/a/src" "$nr/.dev-inputs"
for f in src/main.rs client/src/app.ts crates/a/src/lib.rs crates/a/Cargo.toml Cargo.toml Cargo.lock build.rs; do
    : > "$nr/$f"; touch -d '2001-01-01' "$nr/$f"
done
recompile() {
    bash -c '
        set -uo pipefail
        LAUNCH_SH_NO_MAIN=1 . "$1" >/dev/null 2>&1 || exit 90
        PROJECT_ROOT="$2"
        docker() {
            case "$1" in
                ps) echo visionclaw_container ;;
                inspect) echo "2010-01-01T00:00:00Z" ;;
            esac
        }
        needs_recompile visionclaw_container
    ' _ "$LAUNCH" "$nr"
}
check "nothing newer than the container: no recompile" '[[ "$(recompile)" == false ]]'
for f in Cargo.toml Cargo.lock crates/a/src/lib.rs crates/a/Cargo.toml; do
    touch "$nr/$f"
    check "a newer $f triggers recompile" '[[ "$(recompile)" == true ]]'
    touch -d '2001-01-01' "$nr/$f"
done
touch "$nr/Cargo.toml"; touch "$nr/.dev-inputs/.redeployed"
check "a redeploy after the change satisfies it (no repeat restart)" '[[ "$(recompile)" == false ]]'

echo "== 7. wiring"
check "entrypoint installs inputs before supervisord starts" \
    'awk "/dev-inputs.sh --once/{s=NR} /exec supervisord/{e=NR} END{exit !(s && e && s<e)}" "$ROOT/scripts/dev-entrypoint.sh"'
check "supervisord.dev.conf runs the live install loop" 'grep -q "dev-inputs.sh --watch" "$ROOT/supervisord.dev.conf"'
check "launch.sh hot-patches dev-inputs.sh" 'grep -q "scripts/lib/dev-inputs.sh:/app/scripts/lib/dev-inputs.sh" "$LAUNCH"'
check ".dev-inputs is pruned from the build-input walk" \
    '( . "$ROOT/scripts/lib/build-inputs.sh"; [[ " $BUILD_INPUT_PRUNE_DIRS " == *" .dev-inputs "* ]] )'
check "launch.sh help documents redeploy" 'grep -q "redeploy" <(sed -n "/^show_help/,/^}/p" "$LAUNCH")'

if [[ "${LIVE:-0}" == 1 ]]; then
    echo "== LIVE: running container (read-only docker exec/inspect)"
    C="${CONTAINER:-visionclaw_container}"
    check "/app/.host does not exist in $C" '! docker exec "$C" test -e /app/.host'
    while read -r target; do
        names="$(docker exec "$C" ls -a "$target" 2>/dev/null)"
        check "ls -a $target shows no secret file" '! grep -qE "^(\.env.*|GH|cth\.env)$" <<< "$names"'
    done < <(docker inspect -f '{{range .Mounts}}{{if eq .Type "bind"}}{{.Destination}}{{"\n"}}{{end}}{{end}}' "$C" | grep .)
    check "/app/.dev-inputs stamp verifies in the container" \
        'docker exec "$C" bash /app/scripts/lib/dev-inputs.sh --verify /app/.dev-inputs'
    check "/app/Cargo.toml matches the host's" \
        '[[ "$(docker exec "$C" sha256sum /app/Cargo.toml | cut -d" " -f1)" == "$(sha256sum < "$SECRETS_ROOT/Cargo.toml" | cut -d" " -f1)" ]]'
fi

echo
echo "passed=$PASS failed=$FAIL"
[[ $FAIL -eq 0 ]]
