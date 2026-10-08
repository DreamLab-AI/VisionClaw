#!/usr/bin/env bash
# test-dev-proxy-throughput.sh — regression guard for large responses through
# the dev entry point (nginx :3001), 2026-10-08.
#
# A guard, not a reproduction. A 17 s memory-cloud load was reported through
# :3001; measured hop by hop, no proxy hop was slow (46 MB vectors blob:
# direct 0.05 s, via nginx 0.11-0.20 s, Chrome via nginx 0.8 s). The 17 s
# window was the client's whole load (see memoryCloud/loadTiming.ts). What was
# measurably wasteful was the backend compressing the f32 blob (gzip/br cost
# about 0.5 s of CPU to save 8%), so section 2 asserts it is sent unencoded,
# and every section keeps the proxy from regressing into the slow modes
# (gzip, limit_rate, buffering to disk) that would turn 0.2 s into seconds.
#
# Sections:
#   1. static: nginx.dev.conf logs request and upstream times (so a slow
#      request can be attributed after the fact), and the /api/ location
#      neither compresses nor rate-limits; /wss keeps its Upgrade headers;
#   2. live (LIVE=1): the vectors blob and /api/graph/data through the proxy
#      under a ceiling and within a factor of direct, the blob unencoded with
#      no-store kept, and the /wss handshake answered with 101.
#
# Static only (any checkout):
#   bash scripts/tests/test-dev-proxy-throughput.sh
# Live, inside the dev container (reads the deployed /etc/nginx/nginx.conf):
#   docker exec -i visionclaw_container env LIVE=1 bash -s \
#     < scripts/tests/test-dev-proxy-throughput.sh
# Overrides: DIRECT (http://localhost:4000), PROXY (http://localhost:3001),
#   CEILING_S (2.0), FACTOR (4), SLACK_S (0.3), NGINX_CONF.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/../.." 2>/dev/null && pwd)"
if [[ -z "${NGINX_CONF:-}" ]]; then
  if [[ -f "$ROOT/nginx.dev.conf" ]]; then NGINX_CONF="$ROOT/nginx.dev.conf"; else NGINX_CONF=/etc/nginx/nginx.conf; fi
fi
DIRECT="${DIRECT:-http://localhost:4000}"
PROXY="${PROXY:-http://localhost:3001}"
CEILING_S="${CEILING_S:-2.0}"
FACTOR="${FACTOR:-4}"
SLACK_S="${SLACK_S:-0.3}"
BROWSER_AE='gzip, deflate, br, zstd'

pass=0
fail=0
ok() { echo "  ok   $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL $1"; fail=$((fail + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# The body of one `location <prefix> {` block, comments stripped.
location_block() {
  awk -v want="$1" '
    { sub(/#.*/, "") }
    !inblk && $1 == "location" && index($0, want) { inblk = 1; depth = 0 }
    inblk {
      print
      depth += gsub(/\{/, "{"); depth -= gsub(/\}/, "}")
      if (depth == 0 && /\}/) exit
    }' "$NGINX_CONF"
}

echo "1. static: $NGINX_CONF"
if [[ ! -f "$NGINX_CONF" ]]; then
  bad "nginx config present"
else
  logfmt="$(awk '/log_format main/,/;/' "$NGINX_CONF")"
  api="$(location_block '^~ /api/')"
  wss="$(location_block '/wss')"
  check "log_format main records \$request_time" '[[ "$logfmt" == *"\$request_time"* ]]'
  check "log_format main records \$upstream_response_time" '[[ "$logfmt" == *"\$upstream_response_time"* ]]'
  check "/api/ location found" '[[ -n "$api" ]]'
  check "/api/ does not gzip (the backend negotiates encoding)" '! grep -Eq "^\s*gzip\s+on" <<<"$api"'
  check "no gzip on at http level" '! awk "{sub(/#.*/, \"\")} /^\s*gzip\s+on/" "$NGINX_CONF" | grep -q .'
  check "/api/ has no limit_rate" '! grep -q "limit_rate" <<<"$api"'
  check "/api/ streams (proxy_buffering off: no temp-file spill)" 'grep -Eq "proxy_buffering\s+off" <<<"$api"'
  check "/api/ speaks HTTP/1.1 upstream (keepalive)" 'grep -Eq "proxy_http_version\s+1\.1" <<<"$api"'
  check "/wss forwards the Upgrade header" 'grep -q "Upgrade \$http_upgrade" <<<"$wss"'
  check "/wss forwards Connection upgrade" 'grep -q "Connection \$connection_upgrade" <<<"$wss"'
fi

if [[ "${LIVE:-0}" != "1" ]]; then
  echo "2. live: skipped (LIVE=1 to run against $PROXY)"
else
  echo "2. live: direct $DIRECT, proxy $PROXY"
  # time_total and the content-encoding header of one GET, body discarded.
  probe() { # url accept-encoding -> "seconds bytes encoding cache-control"
    local hdr
    hdr="$(mktemp)"
    local t
    t="$(curl -s -m 60 -o /dev/null -D "$hdr" -H "Accept-Encoding: $2" \
      -w '%{time_total} %{size_download}' "$1")"
    local enc cc
    enc="$(tr -d '\r' <"$hdr" | awk -F': ' 'tolower($1)=="content-encoding"{print $2}' | tail -1)"
    cc="$(tr -d '\r' <"$hdr" | awk -F': ' 'tolower($1)=="cache-control"{print $2}' | tail -1)"
    rm -f "$hdr"
    echo "$t ${enc:-none} ${cc:-none}"
  }
  within() { # proxied direct -> true when proxied <= min-ceiling and <= direct*FACTOR+SLACK
    awk -v p="$1" -v d="$2" -v c="$CEILING_S" -v f="$FACTOR" -v s="$SLACK_S" \
      'BEGIN { exit !(p <= c && p <= d * f + s) }'
  }

  snap="$(curl -s -m 30 "$PROXY/api/memory-cloud" | grep -o '"snapshotId":"[0-9a-f]*"' | head -1 | cut -d'"' -f4)"
  if [[ -z "$snap" ]]; then
    bad "memory-cloud snapshot id from $PROXY/api/memory-cloud (configured and admitted?)"
  else
    url="/api/memory-cloud/vectors?snapshot=$snap"
    read -r d_t d_n _ _ <<<"$(probe "$DIRECT$url" identity)"
    for ae in identity "$BROWSER_AE"; do
      read -r p_t p_n p_enc p_cc <<<"$(probe "$PROXY$url" "$ae")"
      echo "       vectors [$ae]: direct ${d_t}s, proxy ${p_t}s, ${p_n} bytes, encoding ${p_enc}"
      check "vectors [$ae] is the full blob (${p_n} = ${d_n} bytes, >= 40 MB)" '[[ "$p_n" == "$d_n" ]] && (( p_n >= 40000000 ))'
      check "vectors [$ae] through the proxy within ${CEILING_S}s and ${FACTOR}x direct" 'within "$p_t" "$d_t"'
      check "vectors [$ae] sent unencoded (f32 compresses 8% for 0.5 s CPU)" '[[ "$p_enc" == none || "$p_enc" == identity ]]'
      check "vectors [$ae] keeps Cache-Control: no-store" '[[ "$p_cc" == no-store ]]'
    done
  fi

  read -r gd_t _ _ _ <<<"$(probe "$DIRECT/api/graph/data" gzip)"
  read -r gp_t gp_n gp_enc _ <<<"$(probe "$PROXY/api/graph/data" gzip)"
  echo "       graph/data [gzip]: direct ${gd_t}s, proxy ${gp_t}s, ${gp_n} bytes, encoding ${gp_enc}"
  check "graph/data [gzip] through the proxy within ${CEILING_S}s and ${FACTOR}x direct" 'within "$gp_t" "$gd_t"'

  ws="$(curl -s -i -N --max-time 2 -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
    -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
    "$PROXY/wss" | head -1 | tr -d '\r')"
  check "/wss handshake through the proxy answers 101 ($ws)" '[[ "$ws" == *" 101 "* ]]'
fi

echo "passed $pass, failed $fail"
((fail == 0))
