#!/bin/sh
# The shell installer refuses a download that does not match the release's SHA256SUMS
# (SECURITY.md, "Release files"). This makes a release folder of its own, with a stand-in
# program and SHA256SUMS written by `package.py sums` as a release's is, and runs the installer
# from it (--url FOLDER) three times, each with a scratch home:
#
#   1. intact: it installs;
#   2. one byte of the archive changed: refused, and nothing installed;
#   3. the archive missing from SHA256SUMS: refused, and nothing installed.
#
# "Nothing installed" means the scratch home is still empty afterwards: no program folder, no
# link, no folder made on the way to them, and nothing left in the temporary folder.
#
# Usage: packaging/test-install-sh.sh [INSTALLER]
# INSTALLER is the installer to test (default: packaging/install.sh). CI tests the one a release
# ships, made by `package.py installers`.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
installer=${1:-$here/install.sh}
[ -f "$installer" ] || {
    echo "no installer at $installer" >&2
    exit 2
}
# Only what this script passes may steer the installer.
unset JETCARP_VERSION JETCARP_INSTALL_DIR JETCARP_BIN_DIR JETCARP_DOWNLOAD_URL JETCARP_NO_PACKS JETCARP_USE_GH

version=0.0.0
work=$(mktemp -d 2>/dev/null || mktemp -d -t jetcarp-installer-test)
trap 'rm -rf "$work"' EXIT INT TERM
failures=0

ok() { printf '%s\n' "ok: $*"; }
bad() {
    printf '%s\n' "FAILED: $*"
    failures=$((failures + 1))
}

# A release folder, `$work/$1`, with an archive for each target the installer may choose.
make_release() {
    rel="$work/$1"
    mkdir -p "$rel"
    for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do
        name="jetcarp-$version-$target"
        mkdir -p "$work/build/$name"
        printf '#!/bin/sh\necho "jetcarp %s (a stand-in for the installer test)"\n' "$version" >"$work/build/$name/jetcarp"
        chmod +x "$work/build/$name/jetcarp"
        tar -czf "$rel/$name.tar.gz" -C "$work/build" "$name"
    done
    python3 "$here/package.py" sums "$rel" >/dev/null
}

# Change one byte in the middle of file $1.
flip_byte() {
    size=$(wc -c <"$1")
    at=$((size / 2))
    old=$(od -An -tu1 -j "$at" -N1 "$1" | tr -d ' ')
    new=$(((old + 1) % 256))
    # shellcheck disable=SC2059 # the format is the byte, written as an octal escape.
    printf "\\$(printf '%03o' "$new")" | dd of="$1" bs=1 seek="$at" conv=notrunc 2>/dev/null
}

# Run the installer from release folder $1 with a fresh scratch home, `$work/home-$2`; its output
# goes to `$work/out-$2`, and its exit status is returned.
run_installer() {
    home="$work/home-$2"
    mkdir -p "$home" "$work/tmp-$2"
    set +e
    HOME="$home" TMPDIR="$work/tmp-$2" sh "$installer" --version "$version" --url "$1" \
        --dir "$home/lib/jetcarp" --bin-dir "$home/bin" --no-packs >"$work/out-$2" 2>&1
    status=$?
    set -e
    sed 's/^/    | /' "$work/out-$2"
    return "$status"
}

# Case $1 must have been refused with a message holding $2, leaving its home and its temporary
# folder empty.
refused() {
    home="$work/home-$1"
    if ! grep -q "$2" "$work/out-$1"; then
        bad "$1: the refusal does not say \"$2\""
    elif [ -n "$(ls -A "$home")" ] || [ -n "$(ls -A "$work/tmp-$1")" ]; then
        bad "$1: refused, but something was left behind:"
        find "$home" "$work/tmp-$1" | sed 's/^/    /'
    else
        ok "$1: refused (\"$2\"); nothing installed"
    fi
}

echo "testing $installer"

echo "1. intact"
make_release intact
if run_installer "$work/intact" intact; then
    home="$work/home-intact"
    if [ -x "$home/lib/jetcarp/jetcarp" ] && [ -L "$home/bin/jetcarp" ] &&
        grep -q "against SHA256SUMS" "$work/out-intact"; then
        ok "intact: checked against SHA256SUMS and installed"
    else
        bad "intact: the installer succeeded but the program or its link is missing"
    fi
else
    bad "intact: the installer failed on an intact release"
fi

echo "2. one byte of the archive changed"
make_release tampered
for a in "$work"/tampered/*.tar.gz; do
    flip_byte "$a"
done
if run_installer "$work/tampered" tampered; then
    bad "tampered: the installer accepted an archive that does not match SHA256SUMS"
else
    refused tampered "does not match SHA256SUMS"
fi

echo "3. the archive missing from SHA256SUMS"
make_release unlisted
grep -v '\.tar\.gz$' "$work/unlisted/SHA256SUMS" >"$work/sums" || true
echo "0000000000000000000000000000000000000000000000000000000000000000  jetcarp-$version-README.txt" >>"$work/sums"
mv "$work/sums" "$work/unlisted/SHA256SUMS"
if run_installer "$work/unlisted" unlisted; then
    bad "unlisted: the installer accepted an archive SHA256SUMS does not list"
else
    refused unlisted "SHA256SUMS does not list"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures case(s) failed"
    exit 1
fi
echo "all three cases passed"
