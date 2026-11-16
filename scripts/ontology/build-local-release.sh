#!/usr/bin/env bash
# Build the pod ontology release locally (ADR-2106 amendment, 2026-11-16).
#
# The local twin of ontology-publish.yml's build-ontology + publish-release
# jobs, which are parked (network-expensive). It builds the COMMITTED HEAD of
# the vault checkout in a clean detached worktree (never the live tree), runs
# the same gates (vault certify, oo-cert, rdflib re-parse, three-resource
# manifest), and publishes a release directory the server pulls with
# ONTOLOGY_PULL_URL=file:///vault/.ontology-release/current.
#
# Run it in agentbox (it needs the baked `vault` CLI and python3 with rdflib):
#   project/scripts/ontology/build-local-release.sh [--force]
#
# Layout under ONTOLOGY_RELEASE_DIR (default ~/workspace/.ontology-release,
# the container's /vault/.ontology-release):
#   gen-<identity>/   index.jsonld, SHA256SUMS and the four content files
#   current           relative symlink to the active generation, swapped by rename
# The identity (the manifest's visionflow:buildSha) changes with the vault
# commit and the vault binary, so the server re-pulls exactly when either moves.
#
# Shell, not Rust, for byte-for-byte parity with the workflow it replaces;
# a `vault release` subcommand is the follow-up.
set -euo pipefail

VG_REPO="${VG_REPO:-$HOME/workspace/visionGraph}"
OUT_ROOT="${ONTOLOGY_RELEASE_DIR:-$HOME/workspace/.ontology-release}"
VAULT_BIN="${VAULT_BIN:-vault}"
OO_CACHE="${OO_CERT_CACHE:-$HOME/workspace/.cache/oo-cert}"
PACK="$(cd "$(dirname "$0")" && pwd)/pack-pod-resources.py"
# Pinned as in ontology-publish.yml.
OO_CERT_TAG=v2.0.1
OO_CERT_ASSET=oo-cert-x86_64-unknown-linux-gnu
OO_CERT_SHA256=68223b4db193783a52bd2810a66ce2ad3a3780f963a5ef84753564c051e3e532
KEEP=2

force=0
[[ "${1:-}" == "--force" ]] && force=1

src_sha="$(git -C "$VG_REPO" rev-parse HEAD)"
vault_path="$(readlink -f "$(command -v "$VAULT_BIN")")"
vault_ver="$("$VAULT_BIN" --version | awk '{print $2}')"
vault_hash="$(sha256sum "$vault_path" | cut -c1-8)"
ident="local-${src_sha:0:12}-vault${vault_ver}-${vault_hash}"

mkdir -p "$OUT_ROOT"
if [[ $force -eq 0 && -f "$OUT_ROOT/current/index.jsonld" ]] &&
   grep -q "\"visionflow:buildSha\": *\"$ident\"" "$OUT_ROOT/current/index.jsonld"; then
  echo "up to date: $ident"
  exit 0
fi

work="$(mktemp -d "$OUT_ROOT/.build-XXXXXX")"
cleanup() {
  git -C "$VG_REPO" worktree remove --force "$work/src" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

echo "building $ident from $VG_REPO@${src_sha:0:12} with $vault_path"
git -C "$VG_REPO" worktree add -q --detach "$work/src" "$src_sha"

"$VAULT_BIN" --repo "$work/src" build --vault knowledge --out "$work/vault" >"$work/build.log" 2>&1 ||
  { tail -20 "$work/build.log"; exit 1; }

mkdir -p "$work/certificate"
"$VAULT_BIN" --repo "$work/src" --json certify --out "$work/certificate" >"$work/certificate/certify.json"
python3 -I -c "import json,sys;d=json.load(open(sys.argv[1]));print(f\"derivations {d['certificate']['derivations']}, whelk_parity {d['whelk_parity']}\")" \
  "$work/certificate/certify.json"

oo="$OO_CACHE/$OO_CERT_TAG/$OO_CERT_ASSET"
if [[ ! -x "$oo" ]]; then
  mkdir -p "$(dirname "$oo")"
  curl -fsSL -o "$oo.part" \
    "https://github.com/fabio-rovai/open-ontologies/releases/download/$OO_CERT_TAG/$OO_CERT_ASSET"
  mv "$oo.part" "$oo"
  chmod +x "$oo"
fi
echo "$OO_CERT_SHA256  $oo" | sha256sum --check --strict --quiet
"$oo" "$work/certificate/asserted.tsv" "$work/certificate/derivations.tsv" >"$work/certificate/oo-cert.json"
python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get('ok') else 1)" \
  "$work/certificate/oo-cert.json"

ONTOLOGY_SOURCE_REPO=jjohare/visionGraph ONTOLOGY_SOURCE_SHA="$src_sha" \
GITHUB_SHA="$ident" GITHUB_RUN_NUMBER=local GITHUB_OUTPUT= \
  python3 -I "$PACK" "$work/vault" "$work/pod" >"$work/pack.log"

# The workflow's "Validate packed resources" gate, unchanged.
python3 -I - "$work/pod" <<'PY'
import json, sys
from rdflib import Graph
pod = sys.argv[1]
g = Graph()
g.parse(f'{pod}/ontology-ttl/visionflow.ttl', format='turtle')
print(f'Turtle re-parses: {len(g)} triples')
with open(f'{pod}/ontology-jsonld/ontology.jsonld') as f:
    ld = json.load(f)
assert '@context' in ld and ld.get('@graph'), 'ontology.jsonld lacks @context/@graph'
print(f'JSON-LD: {len(ld["@graph"])} graph nodes')
with open(f'{pod}/ontology-jsonld/index.jsonld') as f:
    manifest = json.load(f)
assert len(manifest['ldp:contains']) == 3, 'manifest must list three resources'
print(f'Manifest: {manifest["visionflow:classes"]} classes, {manifest["visionflow:triples"]} triples')
PY

# Assemble as publish-release does; SHA256SUMS is written last.
gen="$work/gen"
mkdir -p "$gen"
cp "$work/pod/ontology-ttl/visionflow.ttl" "$work/pod/ontology-ttl/visionflow.stats.json" \
   "$work/pod/ontology-jsonld/context.jsonld" "$work/pod/ontology-jsonld/ontology.jsonld" \
   "$work/pod/ontology-jsonld/index.jsonld" "$gen/"
(cd "$gen" && sha256sum visionflow.ttl visionflow.stats.json context.jsonld ontology.jsonld index.jsonld >SHA256SUMS)

# Publish: move the generation in, then swap `current` by renaming a fresh
# relative symlink over it (relative so it resolves under /vault too).
rm -rf "$OUT_ROOT/gen-$ident"
mv "$gen" "$OUT_ROOT/gen-$ident"
ln -sfn "gen-$ident" "$OUT_ROOT/.current.new"
mv -T "$OUT_ROOT/.current.new" "$OUT_ROOT/current"
echo "published $OUT_ROOT/current -> gen-$ident"

# Keep the newest $KEEP generations; never the active one.
ls -1dt "$OUT_ROOT"/gen-* | tail -n +$((KEEP + 1)) | while read -r old; do
  [[ "$(basename "$old")" == "gen-$ident" ]] || rm -rf "$old"
done
