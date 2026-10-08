#!/usr/bin/env bash
# Launch guard: build the GDExtension library that the Godot *editor* binary
# loads, and refuse to launch on one older than its Rust source.
#
# The editor binary (Godot_v4.x-stable_linux.x86_64, used by tools/live.sh and
# the HP capture scripts) always loads the .gdextension `linux.debug.x86_64`
# entry, rust/target/debug/…, even with --xr-mode on and no editor window. A
# `cargo build --release` alone leaves that library stale, and the client then
# runs old Rust silently (2026-10-08: hit.position was ignored on the headset).
#
# Usage:  gdext-fresh.sh [<project dir, default: xr-client>]
#   GDEXT_SKIP_BUILD=1   check only, do not run cargo
#   GDEXT_ENTRY=<key>    library entry to check (default linux.debug.x86_64)
# Exit:   0 fresh · 2 build failed / bad project · 3 library missing or stale
set -uo pipefail

project="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
project="$(cd "$project" 2>/dev/null && pwd)" || { echo "gdext-fresh: no project dir ${1:-}" >&2; exit 2; }
entry="${GDEXT_ENTRY:-linux.debug.x86_64}"

ext="$(find "$project" -maxdepth 1 -name '*.gdextension' | head -1)"
[ -n "$ext" ] || { echo "gdext-fresh: no .gdextension in $project" >&2; exit 2; }
rel="$(sed -n -E "s|^[[:space:]]*${entry//./\\.}[[:space:]]*=[[:space:]]*\"res://([^\"]+)\".*|\\1|p" "$ext" | head -1)"
[ -n "$rel" ] || { echo "gdext-fresh: $ext has no $entry entry" >&2; exit 2; }
lib="$project/$rel"
crate="$project/rust"
[ -f "$crate/Cargo.toml" ] || { echo "gdext-fresh: no $crate/Cargo.toml" >&2; exit 2; }

# The profile and target dir come from the path Godot loads: <target>/<profile>/lib….so
profile_dir="$(dirname "$lib")"
profile="$(basename "$profile_dir")"
target_dir="$(dirname "$profile_dir")"

if [ "${GDEXT_SKIP_BUILD:-0}" != 1 ]; then
  args=(build)
  case "$profile" in
    debug) ;;
    release) args+=(--release) ;;
    *) args+=(--profile "$profile") ;;
  esac
  echo "gdext-fresh: cargo ${args[*]} (for $entry → $rel)"
  # pin the target dir: a caller's CARGO_TARGET_DIR would build somewhere Godot never looks
  if ! (cd "$crate" && CARGO_TARGET_DIR="$target_dir" cargo "${args[@]}"); then
    echo "gdext-fresh: cargo build failed; refusing to launch" >&2
    exit 2
  fi
fi

if [ ! -f "$lib" ]; then
  echo "gdext-fresh: $rel is missing; refusing to launch" >&2
  exit 3
fi

# Sources that change the library: the crate's src and manifests, and every
# path dependency's src and manifest (crates/visionclaw-tri-layout and others).
sources=("$crate/src" "$crate/Cargo.toml")
[ -f "$crate/Cargo.lock" ] && sources+=("$crate/Cargo.lock")
while IFS= read -r dep; do
  d="$(cd "$crate/$dep" 2>/dev/null && pwd)" || continue
  [ -d "$d/src" ] && sources+=("$d/src")
  [ -f "$d/Cargo.toml" ] && sources+=("$d/Cargo.toml")
done < <(sed -n -E 's/.*path[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "$crate/Cargo.toml")

newer="$(find "${sources[@]}" -type f -newer "$lib" -print 2>/dev/null | head -1)"
if [ -n "$newer" ]; then
  echo "gdext-fresh: $rel is older than ${newer#"$project"/}; refusing to launch" >&2
  exit 3
fi
echo "gdext-fresh: $rel is fresh"
