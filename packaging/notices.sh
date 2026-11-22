#!/usr/bin/env bash
# Writes THIRD_PARTY_LICENSES.html, the notices of the Rust libraries in JETCARP's release builds,
# with cargo-about (packaging/about.toml, packaging/about.hbs). With --check, writes nothing and
# fails if the committed file is not what this would write (CI runs that).
#
# cargo-about runs offline, from the crates' own files, so the result depends only on Cargo.lock
# and the cargo-about version below. Install that version where you like, for example
#   cargo install --locked cargo-about --version 0.9.3 --features cli --root <folder>
# and point CARGO_ABOUT at it if it is not on the PATH.
#
# cargo-about filters the dependency graph by all of about.toml's targets at once, which keeps a
# crate's dependencies for one system when the crate itself is built only for another (tray-icon,
# built for Windows and macOS, brought in its Linux-only GTK crates: #123). So it also runs once per
# target, and packaging/notices_union.py keeps only the crates some target builds. Two checks guard
# that step: rendering cargo-about's own data unfiltered must give exactly cargo-about's page, and
# the crates kept must be exactly those `cargo tree` lists for the targets (JETCARP's own aside).
# notices_union.py also puts the fonts the page is set in (crates/jetcarp-core/fonts/) into its
# style sheet, where about.hbs has a line for them, so the template holds no encoded font.
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
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
config=packaging/about.toml
template=packaging/about.hbs
targets=$(python3 -I -c 'import sys, tomllib; print(" ".join(tomllib.load(open(sys.argv[1], "rb"))["targets"]))' "$config")

"$about" generate --frozen -c "$config" "$template" -o "$work/about.html"
"$about" generate --frozen -c "$config" --format json -o "$work/all.json"
per_target=()
for t in $targets; do
    "$about" generate --frozen -c "$config" --format json --target "$t" -o "$work/$t.json"
    per_target+=("$work/$t.json")
    cargo tree --frozen --workspace --target "$t" -e normal --prefix none --format '{p}' >>"$work/tree.txt"
done

# The renderer must give exactly what cargo-about gives from the same data.
python3 -I packaging/notices_union.py --render-only "$template" "$work/all.json" >"$work/rendered.html"
if ! cmp -s "$work/about.html" "$work/rendered.html"; then
    echo "error: packaging/notices_union.py does not render about.hbs as cargo-about does:" >&2
    diff -u "$work/about.html" "$work/rendered.html" | head -40 >&2 || true
    exit 1
fi

python3 -I packaging/notices_union.py "$template" "$work/all.json" "${per_target[@]}" >"$work/raw.html"
# Some crates' licence files have CRLF line endings; git would store them as LF anyway.
tr -d '\r' <"$work/raw.html" >"$work/notices.html"

# The crates kept are the ones the release targets build: no more, no fewer.
python3 -I - "$work/tree.txt" "${per_target[@]}" <<'EOF'
import json, sys
tree = set()
for line in open(sys.argv[1]):
    if not line.strip():
        continue
    name, version = line.split()[:2]
    if not name.startswith("jetcarp"):
        tree.add(f"{name}@{version.lstrip('v')}")
kept = set()
for path in sys.argv[2:]:
    for lic in json.load(open(path))["licenses"]:
        kept.update(f"{u['crate']['name']}@{u['crate']['version']}" for u in lic["used_by"])
if kept != tree:
    print("error: the notices' crates are not the release targets' crates", file=sys.stderr)
    print(f"  in the notices only: {sorted(kept - tree)}", file=sys.stderr)
    print(f"  in the builds only: {sorted(tree - kept)}", file=sys.stderr)
    sys.exit(1)
EOF

if [ "${1:-}" = "--check" ]; then
    if ! cmp -s "$work/notices.html" "$out"; then
        echo "error: THIRD_PARTY_LICENSES.html is out of date; run packaging/notices.sh and commit it." >&2
        diff -u "$out" "$work/notices.html" | head -40 >&2 || true
        exit 1
    fi
    echo "THIRD_PARTY_LICENSES.html is up to date."
else
    cat "$work/notices.html" >"$out"
    echo "wrote $out"
fi
