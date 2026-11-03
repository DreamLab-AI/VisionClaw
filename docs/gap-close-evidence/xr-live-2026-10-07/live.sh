#!/bin/bash
# Live SteamVR session for the operator: XR on, GL Compatibility, live backend.
# Started detached; PID + log path in ~/mce-xr/live/RUNNING. Stop with: kill $(sed -n 's/^pid=//p' ~/mce-xr/live/RUNNING)
cd ~/mce-xr/live/xr-client || exit 1
export DISPLAY=:0
export XAUTHORITY=$(ls -1 /run/user/$(id -u)/xauth_* 2>/dev/null | head -1)
export XR_BACKEND_WS=ws://10.10.10.2:4000
# throwaway dev key (any BIP-340 key works against the dev backend)
export XR_NOSTR_SECRET=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
LOG=~/mce-xr/live/live-$(date +%H%M%S).log
setsid nohup ~/godot_new/Godot_v4.6.1-stable_linux.x86_64 --path . --rendering-driver opengl3 --display-driver x11 \
  --xr-mode on --verbose --print-fps > "$LOG" 2>&1 < /dev/null &
PID=$!
printf 'pid=%s\nlog=%s\nstarted=%s\nbackend=%s\n' "$PID" "$LOG" "$(date -Is)" "$XR_BACKEND_WS" > ~/mce-xr/live/RUNNING
cat ~/mce-xr/live/RUNNING
