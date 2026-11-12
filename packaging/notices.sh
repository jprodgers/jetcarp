#!/usr/bin/env bash
# Writes THIRD_PARTY_LICENSES.html, the notices of the Rust libraries in JETCARP's release builds,
# with cargo-about (packaging/about.toml, packaging/about.hbs). With --check, writes nothing and
# fails if the committed file is not what this would write (CI runs that).
#
# cargo-about runs offline, from the crates' own files, so the result depends only on Cargo.lock
# and the cargo-about version below. Install that version where you like, for example
#   cargo install --locked cargo-about --version 0.9.3 --features cli --root <folder>
# and point CARGO_ABOUT at it if it is not on the PATH.
set -euo pipefail

want=0.9.3
about=${CARGO_ABOUT:-cargo-about}
root=$(cd "$(dirname "$0")/.." && pwd)
out=$root/THIRD_PARTY_LICENSES.html

have=$("$about" --version 2>/dev/null | awk '{print $2}') || true
if [ "$have" != "$want" ]; then
    echo "error: cargo-about $want is needed (found: ${have:-none})." >&2
    echo "  cargo install --locked cargo-about --version $want --features cli" >&2
    exit 2
fi

cd "$root"
cargo fetch --locked --quiet
raw=$(mktemp)
tmp=$(mktemp)
trap 'rm -f "$raw" "$tmp"' EXIT
"$about" generate --frozen -c packaging/about.toml packaging/about.hbs -o "$raw"
# Some crates' licence files have CRLF line endings; git would store them as LF anyway.
tr -d '\r' <"$raw" >"$tmp"

if [ "${1:-}" = "--check" ]; then
    if ! cmp -s "$tmp" "$out"; then
        echo "error: THIRD_PARTY_LICENSES.html is out of date; run packaging/notices.sh and commit it." >&2
        diff -u "$out" "$tmp" | head -40 >&2 || true
        exit 1
    fi
    echo "THIRD_PARTY_LICENSES.html is up to date."
else
    cat "$tmp" >"$out"
    echo "wrote $out"
fi
