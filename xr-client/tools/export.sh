#!/usr/bin/env bash
# Export one commit of the XR client as a self-contained, stamped tree: the
# versioned, reproducible way onto the render host (the HP).
#
#   xr-client/tools/export.sh <ref> <dest-root>
#
# Writes <dest-root>/vc-<sha9>/ holding xr-client/ and the path-dependency crates
# it builds against, exactly as committed at <ref> (git archive: no working-tree
# edits, no ignored files), plus xr-client/build_stamp.json:
#   {"sha": <full sha>, "ref": <ref as given>, "exported_at": <UTC ISO-8601>}
# The HUD Session header shows that stamp, the library build time and the backend
# in use (scripts/build_stamp.gd). The tree builds with the tracked Cargo.lock
# (tools/gdext-fresh.sh passes --locked), so the same commit yields the same
# dependency versions on any host.
#
# Refuses: an unknown ref, an existing destination, a commit without
# xr-client/rust/Cargo.lock, and a path dependency this list does not carry.
# Exit: 0 exported (prints the destination) · 1 refused · 2 usage
set -euo pipefail

ref="${1:-}"; root="${2:-}"
if [ -z "$ref" ] || [ -z "$root" ]; then
  sed -n '2,18p' "$0" >&2
  exit 2
fi
repo="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
sha="$(git -C "$repo" rev-parse --verify --quiet "$ref^{commit}")" \
  || { echo "export: unknown ref '$ref'" >&2; exit 1; }
dest="$root/vc-${sha:0:9}"
[ ! -e "$dest" ] || { echo "export: $dest already exists (an export is immutable; remove it first)" >&2; exit 1; }
git -C "$repo" cat-file -e "$sha:xr-client/rust/Cargo.lock" 2>/dev/null \
  || { echo "export: $sha has no xr-client/rust/Cargo.lock; builds would float" >&2; exit 1; }

# The crates xr-client/rust builds against, by path. Check the list against the
# manifests at that commit (one level of transitive path deps), so a new path
# dependency fails the export instead of failing the build on the HP.
crates=(visionclaw-xr-presence visionclaw-tri-layout visionclaw-splat visionclaw-wire)
paths=(xr-client)
for c in "${crates[@]}"; do paths+=("crates/$c"); done
manifests=(xr-client/rust/Cargo.toml)
for c in "${crates[@]}"; do manifests+=("crates/$c/Cargo.toml"); done
for m in "${manifests[@]}"; do
  base="$(dirname "$m")"
  while IFS= read -r dep; do
    [ -n "$dep" ] || continue
    resolved="$(realpath -m --relative-to=. "$base/$dep")"
    case " ${paths[*]} " in
      *" $resolved "*) ;;
      *) case "$resolved" in xr-client/*) ;; *)
           echo "export: $m depends on path '$dep' ($resolved), which the export does not carry" >&2; exit 1 ;;
         esac ;;
    esac
  done < <(git -C "$repo" show "$sha:$m" | sed -n -E 's/.*[{ ,]path *= *"([^"]+)".*/\1/p' | grep -v '^src/')
done

mkdir -p "$root"
tmp="$(mktemp -d "$root/.vc-${sha:0:9}.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
git -C "$repo" archive --format=tar "$sha" -- "${paths[@]}" | tar -C "$tmp" -xf -
printf '{\n  "sha": "%s",\n  "ref": "%s",\n  "exported_at": "%s"\n}\n' \
  "$sha" "$ref" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$tmp/xr-client/build_stamp.json"
mv "$tmp" "$dest"
trap - EXIT
echo "$dest"
