#!/usr/bin/env bash
# Writes Formula/jetcarp.rb into a checkout of the Homebrew tap (jprodgers/homebrew-tap) from a
# release's SHA256SUMS, and with --commit commits it there. It builds nothing: the formula
# installs the release archives. Pushing is left to the caller (the release workflow's
# homebrew job).
#
#   packaging/homebrew/update-tap.sh TAP_DIR SHA256SUMS [--commit]
set -euo pipefail

tap=${1:?usage: update-tap.sh TAP_DIR SHA256SUMS [--commit]}
sums=${2:?usage: update-tap.sh TAP_DIR SHA256SUMS [--commit]}
root=$(cd "$(dirname "$0")/../.." && pwd)
py=${PYTHON:-python3}

version=$("$py" "$root/packaging/package.py" version)
mkdir -p "$tap/Formula"
"$py" "$root/packaging/package.py" formula --sums "$sums" --out "$tap/Formula/jetcarp.rb"

if [ "${3:-}" = "--commit" ]; then
    git -C "$tap" add Formula/jetcarp.rb
    if git -C "$tap" diff --cached --quiet; then
        echo "the tap already has JETCARP $version"
    else
        git -C "$tap" commit -m "jetcarp $version"
    fi
fi
