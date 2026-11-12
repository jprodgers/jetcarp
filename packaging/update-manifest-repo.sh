#!/usr/bin/env bash
# Writes the generated Homebrew tap or Scoop bucket into a checkout of its repository and commits
# it there. The repositories hold only what this writes (see packaging/homebrew/jetcarp.rb and
# packaging/scoop/jetcarp.json, the sources of truth); pushing is left to the caller, the release
# workflow.
#
#   packaging/update-manifest-repo.sh homebrew|scoop MANIFESTS_DIR CHECKOUT_DIR
#
# MANIFESTS_DIR is what `packaging/package.py manifests --sums SHA256SUMS --out DIR` wrote.
set -euo pipefail

kind=${1:?usage: update-manifest-repo.sh homebrew|scoop MANIFESTS_DIR CHECKOUT_DIR}
src=${2:?usage: update-manifest-repo.sh homebrew|scoop MANIFESTS_DIR CHECKOUT_DIR}
repo=${3:?usage: update-manifest-repo.sh homebrew|scoop MANIFESTS_DIR CHECKOUT_DIR}
root=$(cd "$(dirname "$0")/.." && pwd)
version=$("${PYTHON:-python3}" "$root/packaging/package.py" version)

case "$kind" in
    homebrew) files=(README.md Formula/jetcarp.rb) ;;
    scoop) files=(README.md bucket/jetcarp.json) ;;
    *)
        echo "unknown kind $kind (homebrew or scoop)" >&2
        exit 2
        ;;
esac

for f in "${files[@]}"; do
    mkdir -p "$repo/$(dirname "$f")"
    cp "$src/$kind/$f" "$repo/$f"
    git -C "$repo" add "$f"
done
if git -C "$repo" diff --cached --quiet; then
    echo "$kind: already JETCARP $version"
else
    git -C "$repo" commit -m "jetcarp $version"
    echo "$kind: committed JETCARP $version"
fi
