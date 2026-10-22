#!/usr/bin/env bash
# test-launch-sidecars.sh — launch.sh's optional-sidecar selection (codescape, blender, isaac-sim).
# No Docker: it sources launch.sh with LAUNCH_SH_NO_MAIN=1 and drives the pure helpers.
#   bash scripts/tests/test-launch-sidecars.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAUNCH="$HERE/../launch.sh"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
ok()  { echo "  ok   $1"; }
bad() { echo "  FAIL $1"; fail=1; }
check() { if [[ "$2" == "$3" ]]; then ok "$1"; else bad "$1 (got '$2', want '$3')"; fi; }

# Each case runs in a subshell with a fresh state file so nothing leaks between cases.
run() { ( export LAUNCH_SIDECAR_STATE="$TMP/state.$RANDOM"; export LAUNCH_SH_NO_MAIN=1
          if [[ -n "${T_CODESCAPE:-}" ]]; then export CODESCAPE="$T_CODESCAPE"; else unset CODESCAPE; fi
          source "$LAUNCH" 2>/dev/null; "$@" ); }

echo "defaults and precedence"
check "nothing saved, nothing set: all off" "$(run bash -c 'true' ; run eval 'sidecar_state_load; sidecar_summary')" "none"
check "CODESCAPE=1 in the environment is no longer an input" \
  "$(T_CODESCAPE=1 run eval 'sidecar_state_load; sidecar_summary')" "none"
check "a saved choice is used even with CODESCAPE=1 in the environment" \
  "$( ( export LAUNCH_SIDECAR_STATE="$TMP/s1" LAUNCH_SH_NO_MAIN=1; printf 'codescape=0\nblender=1\nisaac-sim=0\n' > "$TMP/s1"
        CODESCAPE=1 source "$LAUNCH" 2>/dev/null; sidecar_state_load; sidecar_summary ) )" "blender"

echo "remember last time"
check "save then load round-trips" \
  "$( ( export LAUNCH_SIDECAR_STATE="$TMP/s2" LAUNCH_SH_NO_MAIN=1; source "$LAUNCH" 2>/dev/null
        sidecar_state_load; SIDECAR_ON[blender]=1; SIDECAR_ON[isaac-sim]=1; sidecar_state_save
        sidecar_state_load; sidecar_summary ) )" "blender isaac-sim"
check "state file is plain key=value, one per sidecar" "$(sort "$TMP/s2" | tr '\n' ' ')" "blender=1 codescape=0 isaac-sim=1 "

echo "state file is data, never code"
printf 'codescape=1\n$(touch %s/pwned)\nbogus=1\nblender=7\n' "$TMP" > "$TMP/s3"
check "hostile and invalid lines are ignored" \
  "$( ( export LAUNCH_SIDECAR_STATE="$TMP/s3" LAUNCH_SH_NO_MAIN=1; source "$LAUNCH" 2>/dev/null; sidecar_state_load; sidecar_summary ) )" "codescape"
[[ ! -e "$TMP/pwned" ]] && ok "nothing in the state file was executed" || bad "state file content was executed"

echo "--sidecars list"
check "list applies" "$(run eval 'sidecar_apply_list codescape,isaac-sim; sidecar_summary')" "codescape isaac-sim"
check "none clears" "$(run eval 'sidecar_apply_list codescape; sidecar_apply_list none; sidecar_summary')" "none"
out="$(run eval 'sidecar_apply_list nonsense' 2>&1)"; rc=$?
[[ $rc -ne 0 && "$out" == *"Unknown sidecar"* ]] && ok "unknown name is an error, not ignored" || bad "unknown name accepted (rc=$rc)"

echo "menu keys"
check "space toggles the item under the cursor" \
  "$(run eval 'sidecar_state_load; SIDECAR_CURSOR=1; sidecar_menu_key " "; sidecar_summary')" "blender"
check "down then toggle hits the next item" \
  "$(run eval 'sidecar_state_load; SIDECAR_CURSOR=0; sidecar_menu_key DOWN; sidecar_menu_key DOWN; sidecar_menu_key x; sidecar_summary')" "isaac-sim"
check "up wraps from the top" "$(run eval 'SIDECAR_CURSOR=0; sidecar_menu_key UP; echo $SIDECAR_CURSOR')" "2"
check "a selects all, n selects none" \
  "$(run eval 'sidecar_state_load; sidecar_menu_key a; s=$(sidecar_summary); sidecar_menu_key n; echo "$s|$(sidecar_summary)"')" "codescape blender isaac-sim|none"
rc=$(run eval 'sidecar_menu_key ENTER && echo 0 || echo 1'); check "enter ends the menu" "$rc" "1"

echo "rendering"
r="$(run eval 'sidecar_state_load; SIDECAR_ON[blender]=1; SIDECAR_CURSOR=1; sidecar_menu_render' | sed 's/\x1b\[[0-9;]*[A-Za-z]//g; s/\r//g')"
[[ "$r" == *"( ) Codescape"* && "$r" == *"(*) Blender"* && "$r" == *"> (*) Blender"* ]] \
  && ok "radio marks and cursor render" || bad "render wrong: $r"

echo "non-interactive launch reuses last time without asking"
printf 'codescape=1\nblender=0\nisaac-sim=0\n' > "$TMP/s4"
out="$( LAUNCH_SIDECAR_STATE="$TMP/s4" LAUNCH_SH_NO_MAIN=1 bash -c 'source "'"$LAUNCH"'" 2>/dev/null; SIDECAR_PROMPT=true; SIDECARS_FLAG=""; select_sidecars </dev/null; echo "CS=$CODESCAPE"' 2>&1 )"
[[ "$out" == *"CS=1"* && "$out" == *"last time"* ]] && ok "no TTY: saved choice applied, CODESCAPE exported" || bad "non-TTY path wrong: $out"


echo "menu under a real pty (tmux panes are ptys: the menu must not hang an agent)"
pty_run() {  # pty_run <state> <timeout> <keys-or-NONE> ; prints the RESULT line and the saved file
  python3 -I - "$1" "$2" "$3" "$LAUNCH" <<'PY'
import os, pty, sys, time, select
state, timeout, keys, launch = sys.argv[1:5]
env = dict(os.environ, LAUNCH_SIDECAR_STATE=state, LAUNCH_SH_NO_MAIN="1", LAUNCH_SIDECAR_TIMEOUT=timeout)
code = 'source "%s"; SIDECAR_PROMPT=true; SIDECARS_FLAG=""; select_sidecars; echo "RESULT $(sidecar_summary)"' % launch
pid, fd = pty.fork()
if pid == 0:
    os.execvpe("bash", ["bash", "-c", code], env)
out = b""; start = time.time()
def pump(t):
    global out
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try: out += os.read(fd, 4096)
            except OSError: return
pump(0.8)
if keys != "NONE":
    for k in keys.split(","):
        os.write(fd, {"DOWN": b"\x1b[B", "SPACE": b" ", "ENTER": b"\r"}[k]); pump(0.25)
# wait up to 8s for the process to finish by itself
deadline = time.time() + 8
done = False
while time.time() < deadline:
    pump(0.2)
    try:
        wpid, _ = os.waitpid(pid, os.WNOHANG)
        if wpid: done = True; break
    except ChildProcessError: done = True; break
txt = out.decode(errors="replace")
res = [l.strip() for l in txt.replace("\r", "").splitlines() if l.startswith("RESULT")]
print(("FINISHED " if done else "HUNG ") + "%.1fs " % (time.time() - start) + (res[-1] if res else "NORESULT"))
PY
}
printf 'codescape=1\nblender=0\nisaac-sim=0\n' > "$TMP/p1"
out="$(pty_run "$TMP/p1" 2 NONE)"
[[ "$out" == FINISHED*"RESULT codescape" ]] && ok "no input: times out and applies the saved default ($out)" || bad "no-input pty: $out"
check "the default is also written back" "$(tr '\n' ' ' < "$TMP/p1")" "codescape=1 blender=0 isaac-sim=0 "
printf 'codescape=1\nblender=0\nisaac-sim=0\n' > "$TMP/p2"
out="$(pty_run "$TMP/p2" 2 DOWN,SPACE,ENTER)"
[[ "$out" == FINISHED*"RESULT codescape blender" ]] && ok "keys: down, space, enter selects blender too" || bad "key pty: $out"
check "and saves it for next time" "$(tr '\n' ' ' < "$TMP/p2")" "codescape=1 blender=1 isaac-sim=0 "
printf 'codescape=0\nblender=0\nisaac-sim=0\n' > "$TMP/p3"
out="$(pty_run "$TMP/p3" 3 DOWN)"      # a keypress, then silence: the countdown must be cancelled, so it waits (HUNG is correct)
[[ "$out" == HUNG* ]] && ok "any keypress cancels the countdown (it waits for the operator)" || bad "countdown not cancelled: $out"

echo "launch.sh syntax and flags"
bash -n "$LAUNCH" && ok "bash -n" || bad "bash -n"
out="$(bash -c 'LAUNCH_SH_NO_MAIN=1 source "'"$LAUNCH"'" up --sidecars=none --no-prompt 2>/dev/null; echo "$ENVIRONMENT|$SIDECARS_FLAG|$SIDECAR_PROMPT"')"
check "a flag in the environment slot falls back to dev" "$out" "dev|none|false"

exit $fail
