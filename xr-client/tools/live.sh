#!/usr/bin/env bash
# Start the live XR client detached (SteamVR / OpenXR on, GL Compatibility),
# after tools/gdext-fresh.sh has built the library the Godot editor binary
# loads and confirmed it is not older than the Rust source.
#
# Configuration: live.env in LIVE_DIR (or the file LIVE_ENV names) is read first;
# a variable already set in the environment wins. tools/live.env.example lists
# the keys. This is where the deployed build's backend URL is pinned.
# Environment:
#   XR_BACKEND_WS   backend WebSocket, e.g. ws://<backend-host>:4000 (required)
#   GODOT           Godot 4 editor binary (default: godot4, then godot, on PATH)
#   LIVE_DIR        where RUNNING and live-<time>.log go (default: xr-client's parent)
#   XR_NOSTR_SECRET client key; a throwaway key is generated when unset
#   DISPLAY / XAUTHORITY  X session (default :0 and the user's xauth file)
# Stop with: kill "$(sed -n 's/^pid=//p' "$LIVE_DIR/RUNNING")"
# Exit: 0 started · 2 bad environment / build failed · 3 stale or missing library
set -uo pipefail
project="${LIVE_PROJECT:-$(cd "$(dirname "$0")/.." && pwd)}"
live_dir="${LIVE_DIR:-$(dirname "$project")}"
env_file="${LIVE_ENV:-$live_dir/live.env}"
if [ -f "$env_file" ]; then
  # Load the file without letting it override what the caller already set.
  while IFS='=' read -r key value; do
    case "$key" in ''|\#*) continue ;; esac
    [[ "$key" =~ ^[A-Z_][A-Z0-9_]*$ ]] || continue
    [ -n "${!key:-}" ] || export "$key=$value"
  done < "$env_file"
fi
[ -n "${XR_BACKEND_WS:-}" ] || { echo "live.sh: set XR_BACKEND_WS (ws://<backend-host>:4000), or put it in $env_file" >&2; exit 2; }
godot="${GODOT:-$(command -v godot4 || command -v godot || true)}"
[ -x "$godot" ] || { echo "live.sh: Godot editor binary not found; set GODOT" >&2; exit 2; }

bash "$(dirname "$0")/gdext-fresh.sh" "$project" || exit $?

export DISPLAY="${DISPLAY:-:0}"
if [ -z "${XAUTHORITY:-}" ]; then
  XAUTHORITY="$(ls -1 "/run/user/$(id -u)"/xauth_* 2>/dev/null | head -1)"
  export XAUTHORITY
fi
export XR_BACKEND_WS
# any BIP-340 key works against a dev backend
export XR_NOSTR_SECRET="${XR_NOSTR_SECRET:-$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')}"
mkdir -p "$live_dir"
log="$live_dir/live-$(date +%H%M%S).log"
cd "$project" || exit 2
setsid nohup "$godot" --path . --rendering-driver opengl3 --display-driver x11 \
  --xr-mode on --verbose --print-fps > "$log" 2>&1 < /dev/null &
pid=$!
stamp="$(sed -n 's/.*"sha": *"\([0-9a-f]*\)".*/\1/p' "$project/build_stamp.json" 2>/dev/null || true)"
printf 'pid=%s\nlog=%s\nstarted=%s\nbackend=%s\nbuild=%s\n' "$pid" "$log" "$(date -Is)" "$XR_BACKEND_WS" "${stamp:-unstamped}" > "$live_dir/RUNNING"
cat "$live_dir/RUNNING"
