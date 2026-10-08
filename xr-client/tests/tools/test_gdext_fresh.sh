#!/usr/bin/env bash
# Tests for tools/gdext-fresh.sh: the launch guard that builds the GDExtension
# library the Godot editor binary loads (the .gdextension linux.debug entry)
# and refuses to launch on a library older than the Rust source.
#
# Each case builds a throwaway project with a fake cargo, so no Rust toolchain
# or Godot is needed.  Usage (from xr-client/):  bash tests/tools/test_gdext_fresh.sh
set -uo pipefail
here="$(cd "$(dirname "$0")/../.." && pwd)"
guard="$here/tools/gdext-fresh.sh"
pass=0
fail=0

work="$(mktemp -d "${TMPDIR:-/tmp}/gdext-fresh-test.XXXXXX")"
trap 'rm -r "$work"' EXIT

# A project laid out like xr-client: .gdextension with debug and release
# entries, rust/src, and a fake cargo that "builds" by touching the library in
# $CARGO_TARGET_DIR/<profile>/ (as real cargo honours CARGO_TARGET_DIR).
make_project() {
  local p="$work/$1"
  mkdir -p "$p/rust/src" "$p/rust/target/debug" "$p/rust/target/release" "$p/bin"
  cat > "$p/demo.gdextension" <<'EOF'
[configuration]
entry_symbol = "gdext_rust_init"

[libraries]
linux.debug.x86_64 = "res://rust/target/debug/libdemo.so"
linux.release.x86_64 = "res://rust/target/release/libdemo.so"
EOF
  echo 'pub fn f() {}' > "$p/rust/src/lib.rs"
  echo '[package]' > "$p/rust/Cargo.toml"
  cat > "$p/bin/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$PWD|$*|${CARGO_TARGET_DIR:-}" >> "$FAKE_CARGO_LOG"
[ "${FAKE_CARGO_FAIL:-0}" = 1 ] && { echo "error: could not compile" >&2; exit 101; }
[ "${FAKE_CARGO_NOOP:-0}" = 1 ] && exit 0
profile=debug
for a in "$@"; do [ "$a" = --release ] && profile=release; done
mkdir -p "${CARGO_TARGET_DIR:-target}/$profile"
touch "${CARGO_TARGET_DIR:-target}/$profile/libdemo.so"
EOF
  chmod +x "$p/bin/cargo"
  echo "$p"
}

# age a file: touch -d is GNU; fall back to a fixed epoch stamp
age() { touch -d "$2" "$1" 2>/dev/null || touch -t 200001010000 "$1"; }

run() { # run <project> [env...] -> sets out, rc
  local p="$1"; shift
  out="$(cd "$p" && env PATH="$p/bin:$PATH" FAKE_CARGO_LOG="$p/cargo.log" "$@" bash "$guard" "$p" 2>&1)"
  rc=$?
}

check() { # check <name> <condition-result>
  if [ "$2" = 0 ]; then pass=$((pass + 1)); echo "ok   - $1"; else fail=$((fail + 1)); echo "FAIL - $1"; echo "$out" | sed 's/^/       /'; fi
}

# 1. a stale library is rebuilt: the guard builds the debug profile in rust/
p="$(make_project rebuild)"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
run "$p"
check "stale debug library is rebuilt and the guard passes" $([ $rc = 0 ] && ! [ "$p/rust/src/lib.rs" -nt "$p/rust/target/debug/libdemo.so" ]; echo $?)
check "the build is a plain (debug) cargo build in rust/" $(grep -q "^$p/rust|build|" "$p/cargo.log" && ! grep -q -- --release "$p/cargo.log"; echo $?)

# 2. a library still older than the source after the build is refused (exit 3)
p="$(make_project refuse)"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
run "$p" FAKE_CARGO_NOOP=1
check "a library older than rust/src is refused" $([ $rc = 3 ] && echo "$out" | grep -q "older than" ; echo $?)
check "the refusal names the newest source file" $(echo "$out" | grep -q "rust/src/lib.rs"; echo $?)

# 3. a fresh release library does not satisfy the guard: Godot's editor binary loads debug
p="$(make_project decoy)"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
touch "$p/rust/target/release/libdemo.so"
run "$p" FAKE_CARGO_NOOP=1
check "a fresh release .so does not hide a stale debug .so" $([ $rc = 3 ]; echo $?)

# 4. a missing library is refused
p="$(make_project missing)"
run "$p" FAKE_CARGO_NOOP=1
check "a missing debug library is refused" $([ $rc = 3 ] && echo "$out" | grep -q "missing"; echo $?)

# 5. a failed build is refused, even with a fresh-looking library on disk
p="$(make_project buildfail)"
touch "$p/rust/target/debug/libdemo.so"
run "$p" FAKE_CARGO_FAIL=1
check "a failed cargo build is refused" $([ $rc = 2 ] && echo "$out" | grep -q "cargo build failed"; echo $?)

# 6. CARGO_TARGET_DIR in the caller's environment cannot redirect the build away
#    from the path Godot loads
p="$(make_project targetdir)"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
run "$p" CARGO_TARGET_DIR="$work/elsewhere"
check "CARGO_TARGET_DIR is pinned to the path the .gdextension names" $([ $rc = 0 ] && grep -q "|$p/rust/target$" "$p/cargo.log" && [ ! -e "$work/elsewhere" ]; echo $?)

# 7. GDEXT_SKIP_BUILD=1 checks without building, and still refuses a stale library
p="$(make_project skip)"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
run "$p" GDEXT_SKIP_BUILD=1
check "GDEXT_SKIP_BUILD=1 does not run cargo but still refuses" $([ $rc = 3 ] && [ ! -e "$p/cargo.log" ]; echo $?)

# 8. a fresh library with the build skipped passes
p="$(make_project fresh)"
age "$p/rust/src/lib.rs" "2 hours ago"; age "$p/rust/Cargo.toml" "2 hours ago"
touch "$p/rust/target/debug/libdemo.so"
run "$p" GDEXT_SKIP_BUILD=1
check "a library newer than every source passes" $([ $rc = 0 ]; echo $?)

# 9. Cargo.toml counts as source (a dependency bump changes the library)
p="$(make_project manifest)"
age "$p/rust/src/lib.rs" "2 hours ago"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "1 hour ago"
touch "$p/rust/Cargo.toml"
run "$p" GDEXT_SKIP_BUILD=1
check "a Cargo.toml newer than the library is refused" $([ $rc = 3 ]; echo $?)

# 10. a path dependency's source counts (crates/visionclaw-tri-layout and friends)
p="$(make_project pathdep)"
mkdir -p "$p/crates/dep/src"
printf '[package]\n[dependencies]\ndep = { path = "../crates/dep" }\n' > "$p/rust/Cargo.toml"
echo '[package]' > "$p/crates/dep/Cargo.toml"
age "$p/rust/src/lib.rs" "3 hours ago"; age "$p/rust/Cargo.toml" "3 hours ago"; age "$p/crates/dep/Cargo.toml" "3 hours ago"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
echo 'pub fn g() {}' > "$p/crates/dep/src/lib.rs"
run "$p" GDEXT_SKIP_BUILD=1
check "a path dependency's source newer than the library is refused" $([ $rc = 3 ] && echo "$out" | grep -q "crates/dep/src/lib.rs"; echo $?)

# ── launchers: tools/live.sh and tools/capture.sh run the guard before Godot ──
fake_godot() { # a Godot stand-in that records its arguments
  cat > "$1/bin/godot" <<'EOF2'
#!/usr/bin/env bash
echo "$PWD|$*" >> "$FAKE_GODOT_LOG"
EOF2
  chmod +x "$1/bin/godot"
}
launch() { # launch <project> <tool> [env...]  (capture.sh gets a script argument)
  local p="$1" tool="$2"; shift 2
  local targs=(); [ "$tool" = capture.sh ] && targs=(tests/visual/x.gd)
  out="$(env PATH="$p/bin:$PATH" FAKE_CARGO_LOG="$p/cargo.log" FAKE_GODOT_LOG="$p/godot.log" \
    GODOT="$p/bin/godot" XR_BACKEND_WS=ws://backend.test:4000 LIVE_DIR="$p/live" LIVE_PROJECT="$p" "$@" \
    bash "$here/tools/$tool" "${targs[@]}" 2>&1)"
  rc=$?
  sleep 0.3 # live.sh starts Godot detached
}

# 11. live.sh refuses to start Godot on a stale library
p="$(make_project live-stale)"; fake_godot "$p"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
launch "$p" live.sh FAKE_CARGO_NOOP=1
check "live.sh does not start Godot on a stale library" $([ $rc = 3 ] && [ ! -e "$p/godot.log" ] && [ ! -e "$p/live/RUNNING" ]; echo $?)

# 12. live.sh builds, then starts Godot in XR mode against the backend and writes RUNNING
p="$(make_project live-ok)"; fake_godot "$p"
touch "$p/rust/target/debug/libdemo.so"; age "$p/rust/target/debug/libdemo.so" "2 hours ago"
launch "$p" live.sh
check "live.sh builds and then starts Godot with --xr-mode on" $([ $rc = 0 ] && grep -q "^$p|.*--path \. .*--xr-mode on" "$p/godot.log" && grep -q "|build|" "$p/cargo.log"; echo $?)
check "live.sh records pid, log and backend in RUNNING" $(grep -q '^pid=[0-9]' "$p/live/RUNNING" && grep -q '^backend=ws://backend.test:4000$' "$p/live/RUNNING"; echo $?)

# 13. live.sh needs the backend named; it never guesses an address
p="$(make_project live-nobackend)"; fake_godot "$p"
out="$(env PATH="$p/bin:$PATH" FAKE_CARGO_LOG="$p/cargo.log" FAKE_GODOT_LOG="$p/godot.log" GODOT="$p/bin/godot" \
  LIVE_DIR="$p/live" LIVE_PROJECT="$p" XR_BACKEND_WS= bash "$here/tools/live.sh" 2>&1)"; rc=$?
check "live.sh without XR_BACKEND_WS refuses" $([ $rc = 2 ] && [ ! -e "$p/godot.log" ]; echo $?)

# 14. capture.sh runs a script windowed with XR off, passing its arguments through
p="$(make_project capture)"; fake_godot "$p"
launch "$p" capture.sh FAKE_CARGO_NOOP=1 GDEXT_SKIP_BUILD=0
check "capture.sh refuses a stale library too" $([ $rc = 3 ] && [ ! -e "$p/godot.log" ]; echo $?)
touch "$p/rust/target/debug/libdemo.so"
out="$(env PATH="$p/bin:$PATH" FAKE_CARGO_LOG="$p/cargo.log" FAKE_GODOT_LOG="$p/godot.log" GODOT="$p/bin/godot" \
  XR_BACKEND_WS=ws://backend.test:4000 LIVE_PROJECT="$p" bash "$here/tools/capture.sh" tests/visual/x.gd -- typed="a b" 2>&1)"; rc=$?
check "capture.sh runs the script with --xr-mode off and the arguments" $([ $rc = 0 ] && grep -q -- "--xr-mode off .*--script tests/visual/x.gd -- typed=a b" "$p/godot.log"; echo $?)

echo "gdext-fresh: $pass passed, $fail failed"
[ "$fail" = 0 ]
