#!/usr/bin/env bash
# xr-gate — the XR client's pre-merge check (GitHub Actions is disabled on this
# repository, so this script is the check).
#
# Steps, each reported as PASS, FAIL or SKIPPED (a skipped step is never
# reported as passed):
#   1. gdext crate (xr-client/rust):           cargo fmt --check, clippy -D warnings, cargo test
#   2. presence crate (crates/visionclaw-xr-presence): the same three
#   3. launcher and export tool tests (xr-client/tests/tools/*.sh)
#   4. gdext debug library build (what the Godot editor binary loads)
#   5. GDScript parse check of every script under xr-client (needs Godot)
#   6. GUT unit tests, headless, plus tests/gut_guard.sh (needs Godot)
#
# Godot steps run on a throwaway copy of xr-client, because the 4.8 importer
# rewrites tracked .import files and writes .uid files: the working tree is
# left exactly as it was found.
#
# Usage:  scripts/xr-gate.sh                    run every step it can
#         scripts/xr-gate.sh --fetch-godot DIR  download the pinned engine into DIR
#                                               (SHA-512 checked) and print its path
# Environment:
#   GODOT                  Godot binary matching the client's engine (4.8-beta2);
#                          unset or missing => steps 4 and 5 are SKIPPED
#   XR_GATE_LIB_PATH       extra directories for the Godot binary's loader (e.g. one
#                          holding libfontconfig.so.1 on a host without it)
#   XR_GATE_ENGINE         expected `godot --version` prefix (default 4.8.beta2)
#   XR_GATE_KEEP=1         keep the throwaway copy and logs (path printed)
# Exit: 0 when no step failed (skips allowed and listed), 1 on any failure,
#       2 on bad usage.
set -uo pipefail

ENGINE_TAG="4.8-beta2"
ENGINE_ZIP="Godot_v4.8-beta2_linux.x86_64.zip"
ENGINE_SHA512="77470f53a9039cd22837935380230220167e098324e82ec448ee4b1f943f00afc424885f7d6631f385123d7a630a9399b3cc1b0e1dad1a4ac45c6356b3a0aa45"
ENGINE_URL="https://github.com/godotengine/godot-builds/releases/download/${ENGINE_TAG}/${ENGINE_ZIP}"

repo="$(cd "$(dirname "$0")/.." && pwd)"
xr="$repo/xr-client"
gdext="$xr/rust"
presence="$repo/crates/visionclaw-xr-presence"

if [ "${1:-}" = "--fetch-godot" ]; then
  dest="${2:?usage: xr-gate.sh --fetch-godot DIR}"
  mkdir -p "$dest/dl" "$dest/bin" || exit 2
  curl -fsSL -o "$dest/dl/$ENGINE_ZIP" "$ENGINE_URL" || { echo "xr-gate: download failed: $ENGINE_URL" >&2; exit 1; }
  echo "$ENGINE_SHA512  $dest/dl/$ENGINE_ZIP" | sha512sum -c --quiet - || { echo "xr-gate: SHA-512 mismatch for $ENGINE_ZIP" >&2; exit 1; }
  (cd "$dest/bin" && unzip -oq "../dl/$ENGINE_ZIP") || exit 1
  echo "$dest/bin/${ENGINE_ZIP%.zip}"
  exit 0
fi
[ $# -eq 0 ] || { sed -n '2,33p' "$0" >&2; exit 2; }

declare -a results=()
failed=0
record() { # record <step> <PASS|FAIL|SKIPPED> [detail]
  results+=("$(printf '%-8s %-44s %s' "$2" "$1" "${3:-}")")
  [ "$2" = "FAIL" ] && failed=1
  printf '== %s: %s %s\n' "$1" "$2" "${3:-}"
}
run() { # run <step> <dir> <command...>
  local step="$1" dir="$2"; shift 2
  printf '\n-- %s: %s\n' "$step" "$*"
  if (cd "$dir" && "$@"); then record "$step" PASS; else record "$step" FAIL; fi
}

# 1-2. Rust: both XR crates. The gdext crate is its own workspace (tracked
# Cargo.lock); the presence crate is a member of the root workspace.
run "gdext: cargo fmt --check"        "$gdext" cargo fmt --check
run "gdext: cargo clippy -D warnings" "$gdext" cargo clippy --locked --all-targets -- -D warnings
run "gdext: cargo test"               "$gdext" cargo test --locked
run "presence: cargo fmt --check"     "$presence" cargo fmt --check
run "presence: cargo clippy -D warnings" "$repo" cargo clippy --locked -p visionclaw-xr-presence --all-targets -- -D warnings
run "presence: cargo test"            "$repo" cargo test --locked -p visionclaw-xr-presence

# 3. Shell tests for the launch guard, live.sh and the versioned export.
for t in "$xr"/tests/tools/test_*.sh; do
  run "tools: $(basename "$t")" "$xr" bash "$t"
done

# 4. The debug cdylib the .gdextension maps for linux.debug.x86_64. Without it
# BinaryProtocolClient is absent and GUT's client tests would pass vacuously.
lib_ok=0
printf '\n-- gdext: cargo build (debug library)\n'
if (cd "$gdext" && cargo build --locked) && [ -f "$gdext/target/debug/libvisionclaw_xr_gdext.so" ]; then
  record "gdext: debug library build" PASS; lib_ok=1
else
  record "gdext: debug library build" FAIL
fi

# 5-6. Godot.
godot="${GODOT:-}"
if [ -z "$godot" ] || [ ! -x "$godot" ]; then
  why="GODOT unset or not executable (scripts/xr-gate.sh --fetch-godot DIR fetches $ENGINE_TAG)"
  record "godot: GDScript parse check" SKIPPED "$why"
  record "godot: GUT unit tests" SKIPPED "$why"
else
  export LD_LIBRARY_PATH="${XR_GATE_LIB_PATH:+$XR_GATE_LIB_PATH:}${LD_LIBRARY_PATH:-}"
  want="${XR_GATE_ENGINE:-4.8.beta2}"
  have="$("$godot" --headless --version 2>/dev/null | tail -1)"
  missing="$(ldd "$godot" 2>/dev/null | grep 'not found' || true)"
  if [[ "$have" != "$want"* ]]; then
    record "godot: GDScript parse check" FAIL "engine is '$have', the client runs $want"
    record "godot: GUT unit tests" FAIL "engine is '$have', the client runs $want"
  elif [ -n "$missing" ]; then
    # Present but unable to load its libraries: a broken environment, not a skip.
    record "godot: GDScript parse check" FAIL "loader: $(echo "$missing" | awk '{print $1}' | tr '\n' ' ')(set XR_GATE_LIB_PATH)"
    record "godot: GUT unit tests" FAIL "loader: missing libraries"
  elif [ "$lib_ok" -ne 1 ]; then
    record "godot: GDScript parse check" FAIL "no gdext debug library"
    record "godot: GUT unit tests" FAIL "no gdext debug library"
  else
    work="$(mktemp -d "${TMPDIR:-/tmp}/xr-gate.XXXXXX")"
    [ "${XR_GATE_KEEP:-0}" = "1" ] || trap 'rm -rf "$work"' EXIT
    copy="$work/xr-client"
    mkdir -p "$copy/rust"
    tar -C "$xr" --exclude=./rust/target --exclude=./.godot --exclude=./tests/report -cf - . | tar -C "$copy" -xf -
    ln -s "$gdext/target" "$copy/rust/target"
    # Parity tests read sibling trees (e.g. ../client/src) by path: link the rest
    # of the repository beside the copy, read-only use.
    for entry in "$repo"/* "$repo"/.[!.]*; do
      [ -e "$entry" ] || continue
      name="$(basename "$entry")"
      [ "$name" = "xr-client" ] || [ "$name" = ".git" ] || ln -s "$entry" "$work/$name"
    done
    mkdir -p "$copy/tests/report"

    printf '\n-- godot: import (%s)\n' "$have"
    timeout 900 "$godot" --headless --path "$copy" --import > "$work/import.log" 2>&1
    import_rc=$?

    # 5. Parse check: every .gd file, after the import has built the class cache.
    parse_fail=0
    : > "$work/parse.log"
    while IFS= read -r f; do
      rel="${f#"$copy"/}"
      out="$("$godot" --headless --path "$copy" --check-only -s "res://$rel" 2>&1)"
      if [ $? -ne 0 ] || grep -qE 'Parse Error|Compile Error|SCRIPT ERROR' <<<"$out"; then
        parse_fail=$((parse_fail + 1))
        printf '%s\n%s\n' "== $rel" "$out" >> "$work/parse.log"
      fi
    done < <(find "$copy" -path "$copy/addons" -prune -o -path "$copy/.godot" -prune -o -name '*.gd' -print | sort)
    n_gd="$(find "$copy" -path "$copy/addons" -prune -o -path "$copy/.godot" -prune -o -name '*.gd' -print | wc -l | tr -d ' ')"
    if [ "$import_rc" -ne 0 ]; then
      record "godot: GDScript parse check" FAIL "import exited $import_rc ($work/import.log)"
    elif [ "$parse_fail" -ne 0 ] || grep -qE 'Parse Error|Compile Error' "$work/import.log"; then
      grep -E '^== ' "$work/parse.log" | head -20
      record "godot: GDScript parse check" FAIL "$parse_fail of $n_gd scripts"
    else
      record "godot: GDScript parse check" PASS "$n_gd scripts"
    fi

    # 6. GUT, then the guard that catches skipped (unparseable) test scripts.
    printf '\n-- godot: GUT\n'
    timeout 1200 "$godot" --headless --xr-mode off --path "$copy" \
      -s res://addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json \
      -gjunit_xml_file=res://tests/report/junit.xml > "$work/gut.log" 2>&1
    gut_rc=$?
    plain="$(sed 's/\x1b\[[0-9;]*m//g' "$work/gut.log")"
    totals="$(grep -E '^(Scripts|Tests|Passing Tests|Failing Tests|Risky/Pending) ' <<<"$plain" | awk '{print $NF}' | paste -sd' ' -)"
    pending="$(sed -n -E 's/^Risky\/Pending[[:space:]]+([0-9]+).*/\1/p' <<<"$plain" | tail -1)"
    if (cd "$copy" && bash tests/gut_guard.sh "$work/gut.log" "$work/import.log") && [ "$gut_rc" -eq 0 ]; then
      record "godot: GUT unit tests" PASS "scripts/tests/passing/[failing]/pending: $totals; ${pending:-0} pending are not passes"
    else
      grep -E -A3 '^- test_|\[Failed\]' <<<"$plain" | grep -B1 -A3 'Failed' | head -40
      hint=""
      grep -q 'libfontconfig.so.1: cannot open' "$work/gut.log" \
        && hint=" (Godot could not load libfontconfig.so.1: label tests fail; set XR_GATE_LIB_PATH)"
      record "godot: GUT unit tests" FAIL "exit $gut_rc; $totals$hint"
    fi
    [ "${XR_GATE_KEEP:-0}" = "1" ] && echo "xr-gate: kept $work"
  fi
fi

printf '\n== xr-gate summary (%s)\n' "$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || echo '?')"
printf '%s\n' "${results[@]}"
exit "$failed"
