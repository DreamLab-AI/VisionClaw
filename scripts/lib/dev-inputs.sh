#!/usr/bin/env bash
# dev-inputs.sh — carry the host's root build files into the dev container
# without a single-file bind mount and without exposing the checkout
# (ADR-2008, 2026-10-07 amendment).
#
# The problem. A bind mount of one file pins the host inode at container start.
# git (merge, checkout, rebase) replaces a file by writing a new inode, so the
# running container kept the old Cargo.toml while the directory-mounted src/
# showed the new code, and the in-container build failed (`unresolved import
# subtle`) or, worse, succeeded against a stale manifest.
#
# The shape. The host publishes exactly the files below into the gitignored
# directory <checkout>/.dev-inputs/ and stamps them; compose mounts only that
# directory, read-only, at /app/.dev-inputs (a directory mount shows renames);
# the container copies them into /app when the stamp's digest matches. Nothing
# else from the checkout root is mounted, so .env, .env.prod, GH, cth.env and any
# future untracked file never reach the container.
#
# Why copies, not symlinks: cargo honours a symlinked manifest at its link path,
# but Vite resolves a symlinked vite.config.ts to its realpath, so `__dirname`,
# the `@` alias and loadEnv would point outside /app/client.
#
# Host side (launch.sh up/restart/build/redeploy, and the .githooks
# post-checkout/post-merge/post-rewrite hooks):
#   dev-inputs.sh --publish <checkout>          publish and stamp now
#   dev-inputs.sh --publish-if-deployed <dir>   same, only if <dir>/.dev-inputs
#                                               exists (hooks: never create it)
# Container side (dev-entrypoint.sh, rust-backend-wrapper.sh, supervisord):
#   dev-inputs.sh --verify [dir]                stamp present and digest matches
#   dev-inputs.sh --once                        verify, then install into /app
#   dev-inputs.sh --watch [secs]                --once forever (Vite stays live)
#   . dev-inputs.sh                             functions only

DEV_INPUTS_DIR="${DEV_INPUTS_DIR:-/app/.dev-inputs}"

# The root files the dev container needs that live outside any mounted
# directory. package.json / package-lock.json are deliberately absent: they
# describe node_modules baked into the image, so a change to them needs an image
# build (launch.sh classes them as critical), not a copy. Host client/ is not
# mounted because it holds client/.env.
DEV_INPUT_FILES="${DEV_INPUT_FILES:-Cargo.toml Cargo.lock build.rs client/index.html client/vite.config.ts client/tsconfig.json client/postcss.config.cjs}"

dev_inputs_log() {
    echo "[DEV-INPUTS][$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

dev_inputs_sha() {
    sha256sum < "$1" | cut -d' ' -f1
}

# Digest of the input set in <dir>: one "path sha" line per file, "path -" for an
# absent one, hashed. Host and container compute it identically, so a stamp
# written after a complete publish matches only a complete set.
dev_inputs_digest() {
    local dir="$1" rel
    for rel in $DEV_INPUT_FILES; do
        if [[ -f "$dir/$rel" ]]; then
            printf '%s %s\n' "$rel" "$(dev_inputs_sha "$dir/$rel")"
        else
            printf '%s -\n' "$rel"
        fi
    done | sha256sum | cut -d' ' -f1
}

# Value of key=value line <key> in <dir>/.stamp.
dev_inputs_stamp_field() {
    sed -n "s/^$2=//p" "$1/.stamp" 2>/dev/null | tail -n 1
}

# Replace <dst> with <src> by copy-then-rename in <dst>'s directory, so a reader
# never sees a half-written file and a watcher sees one event. No-op when the
# content already matches, which keeps mtimes (and the rebuild decision) still.
# Echoes "updated" when it wrote.
dev_inputs_place() {
    local src="$1" dst="$2" tmp
    if [[ -f "$dst" && ! -L "$dst" ]] && [[ "$(dev_inputs_sha "$src")" == "$(dev_inputs_sha "$dst")" ]]; then
        return 0
    fi
    mkdir -p "$(dirname "$dst")" || return 1
    tmp="$(dirname "$dst")/.dev-inputs-tmp.$(basename "$dst").$$"
    if cp -f "$src" "$tmp" && mv -f "$tmp" "$dst"; then
        echo updated
        return 0
    fi
    rm -f "$tmp"
    return 1
}

# Host side: publish the input files of checkout <root> into <root>/.dev-inputs
# and write the stamp last. The stamp names the commit the files came from and
# whether any of them differ from it, so the backend log says what it built.
dev_inputs_publish() {
    local root="$1" out="$1/.dev-inputs" rel res changed=0 sha dirty digest tmp
    mkdir -p "$out" || return 1
    for rel in $DEV_INPUT_FILES; do
        if [[ -f "$root/$rel" ]]; then
            res="$(dev_inputs_place "$root/$rel" "$out/$rel")" || { dev_inputs_log "ERROR: could not publish $rel"; return 1; }
            if [[ "$res" == updated ]]; then changed=1; dev_inputs_log "published $rel"; fi
        elif [[ -e "$out/$rel" ]]; then
            rm -f "$out/$rel"; changed=1; dev_inputs_log "unpublished $rel (absent in checkout)"
        fi
    done
    sha="$(git -C "$root" rev-parse HEAD 2>/dev/null || echo unknown)"
    dirty=0
    # shellcheck disable=SC2086 # the file list is word-split on purpose
    if [[ "$sha" != unknown ]] && ! (cd "$root" && git diff --quiet HEAD -- $DEV_INPUT_FILES 2>/dev/null); then
        dirty=1
    fi
    digest="$(dev_inputs_digest "$out")"
    if [[ "$changed" == 0 && "$(dev_inputs_stamp_field "$out" source_sha)" == "$sha" \
          && "$(dev_inputs_stamp_field "$out" source_dirty)" == "$dirty" \
          && "$(dev_inputs_stamp_field "$out" inputs_sha256)" == "$digest" ]]; then
        return 0
    fi
    tmp="$out/.stamp.tmp.$$"
    if ! {
        echo "source_sha=$sha"
        echo "source_dirty=$dirty"
        echo "inputs_sha256=$digest"
        echo "published_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    } > "$tmp" || ! mv -f "$tmp" "$out/.stamp"; then
        rm -f "$tmp"
        dev_inputs_log "ERROR: could not write $out/.stamp"
        return 1
    fi
    dev_inputs_log "stamped source_sha=$sha source_dirty=$dirty"
}

# Container side: 0 when <dir>/.stamp exists and its digest matches the files.
# A publish in flight (files renamed, stamp not yet) fails, so callers retry.
dev_inputs_verify() {
    local dir="${1:-$DEV_INPUTS_DIR}" want got
    if [[ ! -d "$dir" ]]; then
        echo "$dir is missing: the dev service must mount <checkout>/.dev-inputs there, published by ./scripts/launch.sh (up|restart|redeploy) dev"
        return 1
    fi
    want="$(dev_inputs_stamp_field "$dir" inputs_sha256)"
    if [[ -z "$want" ]]; then
        echo "$dir/.stamp is missing: run ./scripts/launch.sh redeploy dev on the host"
        return 1
    fi
    got="$(dev_inputs_digest "$dir")"
    if [[ "$got" != "$want" ]]; then
        echo "$dir does not match its stamp (publish in progress, or edited by hand): run ./scripts/launch.sh redeploy dev on the host"
        return 1
    fi
    return 0
}

# Verify with retries (DEV_INPUTS_VERIFY_RETRIES, 1 s apart) to ride out a
# publish in progress; logs and returns 1 on final failure.
dev_inputs_verify_retrying() {
    local tries="${DEV_INPUTS_VERIFY_RETRIES:-5}" i reason
    for ((i = 1; i <= tries; i++)); do
        reason="$(dev_inputs_verify "$DEV_INPUTS_DIR")" && return 0
        ((i < tries)) && sleep 1
    done
    dev_inputs_log "ERROR: $reason"
    return 1
}

# One line naming what /app is built from; the wrapper prints it every start.
dev_inputs_describe() {
    echo "source_sha=$(dev_inputs_stamp_field "$DEV_INPUTS_DIR" source_sha) source_dirty=$(dev_inputs_stamp_field "$DEV_INPUTS_DIR" source_dirty) published_at=$(dev_inputs_stamp_field "$DEV_INPUTS_DIR" published_at)"
}

# Container side: verify, then copy the published files into $APP_ROOT; remove
# the app copy of a file no longer published. Refuses (and changes nothing)
# when the stamp does not verify.
dev_inputs_install() {
    local app_root="${APP_ROOT:-/app}" rel res
    dev_inputs_verify_retrying || return 1
    for rel in $DEV_INPUT_FILES; do
        if [[ -f "$DEV_INPUTS_DIR/$rel" ]]; then
            res="$(dev_inputs_place "$DEV_INPUTS_DIR/$rel" "$app_root/$rel")" || { dev_inputs_log "ERROR: could not install $rel"; return 1; }
            [[ "$res" == updated ]] && dev_inputs_log "installed $rel ($(dev_inputs_describe))"
        elif [[ -e "$app_root/$rel" ]]; then
            rm -f "$app_root/$rel"
            dev_inputs_log "removed $rel (no longer published)"
        fi
    done
    return 0
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    case "${1:---once}" in
        --publish)
            [[ -n "${2:-}" ]] || { echo "usage: $0 --publish <checkout>" >&2; exit 2; }
            dev_inputs_publish "$2"
            ;;
        --publish-if-deployed)
            [[ -n "${2:-}" && -d "$2/.dev-inputs" ]] || exit 0
            dev_inputs_publish "$2"
            ;;
        --verify)
            reason="$(dev_inputs_verify "${2:-$DEV_INPUTS_DIR}")" || { echo "$reason" >&2; exit 1; }
            ;;
        --once)
            dev_inputs_install && dev_inputs_log "inputs current ($(dev_inputs_describe))"
            ;;
        --watch)
            interval="${2:-${DEV_INPUTS_INTERVAL:-2}}"
            dev_inputs_log "installing from $DEV_INPUTS_DIR every ${interval}s: $DEV_INPUT_FILES"
            # Log every install; log a failure once, not every tick, until its
            # reason changes or it clears.
            last=""
            while :; do
                if out="$(DEV_INPUTS_VERIFY_RETRIES=1 dev_inputs_install 2>&1)"; then
                    [[ -n "$out" ]] && echo "$out"
                    last=""
                else
                    reason="${out#*\] }"   # drop the "[DEV-INPUTS][time] " prefix
                    [[ "$reason" != "$last" ]] && echo "$out"
                    last="$reason"
                fi
                sleep "$interval"
            done
            ;;
        *)
            echo "usage: $0 --publish <checkout> | --publish-if-deployed <checkout> | --verify [dir] | --once | --watch [secs]" >&2
            exit 2
            ;;
    esac
fi
