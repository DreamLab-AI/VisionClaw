#!/usr/bin/env bash
# Run a capture or probe script windowed with XR off (tests/visual/*.gd,
# perf/benchmark.gd), after tools/gdext-fresh.sh has built the library the
# Godot editor binary loads and confirmed it is fresh. Output goes to stdout.
#
# Usage:  tools/capture.sh <script.gd> [-- <script args>]
# Environment: GODOT, XR_BACKEND_WS, DISPLAY / XAUTHORITY as for tools/live.sh;
#   CAPTURE_TIMEOUT seconds (default 240); CAPTURE_RESOLUTION (default 1600x900).
# Exit: Godot's status · 2 bad environment / build failed · 3 stale or missing library
set -uo pipefail
project="${LIVE_PROJECT:-$(cd "$(dirname "$0")/.." && pwd)}"
script="${1:?usage: capture.sh <script.gd> [-- <script args>]}"
shift
godot="${GODOT:-$(command -v godot4 || command -v godot || true)}"
[ -x "$godot" ] || { echo "capture.sh: Godot editor binary not found; set GODOT" >&2; exit 2; }

bash "$(dirname "$0")/gdext-fresh.sh" "$project" >&2 || exit $?

export DISPLAY="${DISPLAY:-:0}"
if [ -z "${XAUTHORITY:-}" ]; then
  XAUTHORITY="$(ls -1 "/run/user/$(id -u)"/xauth_* 2>/dev/null | head -1)"
  export XAUTHORITY
fi
[ -n "${XR_BACKEND_WS:-}" ] && export XR_BACKEND_WS
export XR_NOSTR_SECRET="${XR_NOSTR_SECRET:-$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')}"
cd "$project" || exit 2
timeout "${CAPTURE_TIMEOUT:-240}" "$godot" --path . --rendering-driver opengl3 --display-driver x11 \
  --xr-mode off --audio-driver Dummy --resolution "${CAPTURE_RESOLUTION:-1600x900}" --script "$script" "$@"
