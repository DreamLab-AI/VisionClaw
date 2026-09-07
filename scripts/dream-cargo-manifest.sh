#!/usr/bin/env bash
# Dream-cycle evaluator: sanity-check the cargo metadata dump produced by
# the build step (/tmp/vc-meta.json). Checked-in script because the annexe
# ssh dispatch strips nested double quotes from inline entrypoints.
#
# Ordering dependency: dream.config.json's buildStep writes /tmp/vc-meta.json
# and MUST have run first. Guarded below so an out-of-order run fails with a
# greppable sentinel instead of a Python traceback.
set -u
if [ ! -f /tmp/vc-meta.json ]; then
  echo MANIFEST-FAIL
  echo "reason: /tmp/vc-meta.json absent — buildStep did not run or failed" >&2
  exit 1
fi
python3 - <<'PY' || exit 1
import json, sys
try:
    m = json.load(open('/tmp/vc-meta.json'))
    pk = [p['name'] for p in m['packages']]
except Exception as e:
    print('MANIFEST-FAIL')
    print('reason: /tmp/vc-meta.json unreadable: %s' % e, file=sys.stderr)
    sys.exit(1)
print('crates:', len(pk))
have = 'visionclaw-xr-presence' in pk
print('xr-presence:', have)
print('MANIFEST-OK' if have else 'MANIFEST-FAIL')
sys.exit(0 if have else 1)
PY
