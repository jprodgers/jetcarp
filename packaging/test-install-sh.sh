#!/bin/sh
# The shell installer refuses a download that does not match the release's SHA256SUMS
# (SECURITY.md, "Release files"). This makes a release folder of its own, with a stand-in
# program and SHA256SUMS written by `package.py sums` as a release's is, and runs the installer
# from it (--url FOLDER), each case with a scratch home:
#
#   1. intact: it installs, and has the program add JETCARP to the system's apps (`jetcarp
#      launcher install`, #105);
#   2. one byte of the archive changed: refused, and nothing installed;
#   3. the archive missing from SHA256SUMS: refused, and nothing installed;
#   4. --no-launcher: it installs, and asks for no launcher;
#   5. --uninstall, after case 1: the program takes its launcher out (`jetcarp launcher remove`)
#      before its folder and link go, and the settings folder the program wrote in is named as
#      kept;
#   6. another JETCARP, cargo's first on the PATH and pipx's in its folder: each named with what
#      removes it, and that `jetcarp` runs cargo's (#348); with none, nothing said;
#   7. --uninstall after `jetcarp uninstall --settings --data`: the program the installer runs
#      first makes none of those folders again, and a folder that holds nothing (the pictures
#      folder, emptied) is not named as kept (#350).
#
# "Nothing installed" means the scratch home is still empty afterwards: no program folder, no
# link, no folder made on the way to them, and nothing left in the temporary folder. The stand-in
# program writes each command it is given to `jetcarp-calls` in the scratch home, and, as JETCARP
# does on any run, writes in its settings folder and its runtime folder (#350).
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
# Only what this script passes may steer the installer, and the program's folders are the
# scratch home's (CI names its own for the tests).
unset JETCARP_VERSION JETCARP_INSTALL_DIR JETCARP_BIN_DIR JETCARP_DOWNLOAD_URL JETCARP_NO_PACKS JETCARP_USE_GH JETCARP_NO_LAUNCHER
unset JETCARP_CONFIG_DIR JETCARP_DATA_DIR JETCARP_RUNTIME_DIR JETCARP_OUT JETCARP_LIBRARY
unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_RUNTIME_DIR CARGO_HOME PIPX_HOME

version=0.0.0
work=$(mktemp -d 2>/dev/null || mktemp -d -t jetcarp-installer-test)
trap 'rm -rf "$work"' EXIT INT TERM
failures=0

ok() { printf '%s\n' "ok: $*"; }
bad() {
    printf '%s\n' "FAILED: $*"
    failures=$((failures + 1))
}

# The settings folder, the data folder and the runtime folder JETCARP uses in home $1 by default,
# one per line (on a Mac the data folder is in the settings folder).
default_folders() {
    case "$(uname -s)" in
        Darwin) printf '%s\n' "$1/Library/Application Support/jetcarp" "$1/Library/Application Support/jetcarp/data" ;;
        *) printf '%s\n' "$1/.config/jetcarp" "$1/.local/share/jetcarp" ;;
    esac
    printf '%s\n' "$1/.cache/jetcarp"
}

# The stand-in program, at $1: it notes each command it is given in `jetcarp-calls` in its home,
# and, as JETCARP does on any run, keeps what it read of the machine in its settings folder and
# notes the run in its runtime folder: each the one JETCARP_CONFIG_DIR and JETCARP_RUNTIME_DIR
# name, else the default.
stand_in() {
    cat >"$1" <<STANDIN
#!/bin/sh
printf '%s\\n' "\$*" >>"\$HOME/jetcarp-calls"
case "\$(uname -s)" in
    Darwin) settings="\$HOME/Library/Application Support/jetcarp" ;;
    *) settings="\$HOME/.config/jetcarp" ;;
esac
settings="\${JETCARP_CONFIG_DIR:-\$settings}"
runtime="\${JETCARP_RUNTIME_DIR:-\$HOME/.cache/jetcarp}"
mkdir -p "\$settings" "\$runtime/memory" && : >"\$settings/machine.json"
echo "jetcarp $version (a stand-in for the installer test)"
STANDIN
    chmod +x "$1"
}

# A release folder, `$work/$1`, with an archive for each target the installer may choose.
make_release() {
    rel="$work/$1"
    mkdir -p "$rel"
    for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do
        name="jetcarp-$version-$target"
        mkdir -p "$work/build/$name"
        stand_in "$work/build/$name/jetcarp"
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

# Run the installer from release folder $1 with a fresh scratch home, `$work/home-$2`, and any
# further options after those; its output goes to `$work/out-$2`, and its exit status is returned.
run_installer() {
    rel=$1
    home="$work/home-$2"
    case_name=$2
    shift 2
    mkdir -p "$home" "$work/tmp-$case_name"
    set +e
    HOME="$home" TMPDIR="$work/tmp-$case_name" sh "$installer" --version "$version" --url "$rel" \
        --dir "$home/lib/jetcarp" --bin-dir "$home/bin" --no-packs "$@" >"$work/out-$case_name" 2>&1
    status=$?
    set -e
    sed 's/^/    | /' "$work/out-$case_name"
    return "$status"
}

# Whether the stand-in program in scratch home $1 was given the command $2.
called() {
    [ -f "$1/jetcarp-calls" ] && grep -qx "$2" "$1/jetcarp-calls"
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
    if called "$home" "launcher install"; then
        ok "intact: the program was asked to add JETCARP to the system's apps"
    else
        bad "intact: the installer did not run \`jetcarp launcher install\`"
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

echo "4. --no-launcher"
if run_installer "$work/intact" no-launcher --no-launcher; then
    home="$work/home-no-launcher"
    if [ ! -x "$home/lib/jetcarp/jetcarp" ]; then
        bad "no-launcher: the program is missing"
    elif called "$home" "launcher install"; then
        bad "no-launcher: the installer ran \`jetcarp launcher install\` all the same"
    else
        ok "no-launcher: installed, and no launcher asked for"
    fi
else
    bad "no-launcher: the installer failed"
fi

echo "5. --uninstall, after case 1"
home="$work/home-intact"
set +e
HOME="$home" TMPDIR="$work/tmp-intact" sh "$installer" --version "$version" --uninstall \
    --dir "$home/lib/jetcarp" --bin-dir "$home/bin" >"$work/out-uninstall" 2>&1
status=$?
set -e
sed 's/^/    | /' "$work/out-uninstall"
settings=$(default_folders "$home" | head -n 1)
if [ "$status" -ne 0 ]; then
    bad "uninstall: the installer failed"
elif [ -e "$home/lib/jetcarp" ] || [ -e "$home/bin/jetcarp" ]; then
    bad "uninstall: the program's folder or its link is still there"
elif ! called "$home" "launcher remove"; then
    bad "uninstall: the installer did not run \`jetcarp launcher remove\` first"
elif ! grep -qF "  $settings (" "$work/out-uninstall"; then
    bad "uninstall: the settings folder, which the program wrote in as it was installed, is not named as kept"
else
    ok "uninstall: the launcher, the program's folder and its link removed; the settings folder named as kept"
fi

echo "6. another JETCARP"
# The PATH a person might have: cargo's folder first. Only the system's own folders beside it, so
# a JETCARP installed on this machine is not counted.
system_path=/usr/bin:/bin:/usr/sbin:/sbin
others_here=""
for d in /opt/homebrew/bin /usr/local/bin /home/linuxbrew/.linuxbrew/bin; do
    if [ -x "$d/jetcarp" ]; then others_here=$d/jetcarp; fi
done
path_before=$PATH
set +e
PATH="$work/home-alone/bin:$system_path"
run_installer "$work/intact" alone
status=$?
PATH=$path_before
set -e
if [ "$status" -ne 0 ]; then
    bad "another, alone: the installer failed"
elif [ -n "$others_here" ]; then
    ok "another, alone: not checked, this machine has $others_here"
elif grep -q "installed on this computer as well" "$work/out-alone"; then
    bad "another, alone: other JETCARPs named where there are none"
else
    ok "another, alone: nothing said of other JETCARPs"
fi
home="$work/home-other"
mkdir -p "$home/.cargo/bin" "$home/.local/share/pipx/venvs/jetcarp/bin"
: >"$home/.cargo/.crates.toml"
stand_in "$home/.cargo/bin/jetcarp"
stand_in "$home/.local/share/pipx/venvs/jetcarp/bin/jetcarp"
set +e
PATH="$home/.cargo/bin:$home/bin:$system_path"
run_installer "$work/intact" other
status=$?
PATH=$path_before
set -e
out="$work/out-other"
if [ "$status" -ne 0 ]; then
    bad "another: the installer failed"
elif ! grep -q "other JETCARPs are installed on this computer as well" "$out"; then
    bad "another: the other JETCARPs are not named"
elif ! grep -qF "  $home/.cargo/bin/jetcarp (cargo's): cargo uninstall jetcarp" "$out"; then
    bad "another: cargo's is not named with what removes it"
elif ! grep -qF "  $home/.local/share/pipx/venvs/jetcarp/bin/jetcarp (pipx's): pipx uninstall jetcarp" "$out"; then
    bad "another: pipx's is not named with what removes it"
elif ! grep -qF "\`jetcarp\` runs $home/.cargo/bin/jetcarp, which comes first on your PATH" "$out"; then
    bad "another: it does not say that \`jetcarp\` runs cargo's"
else
    ok "another: cargo's and pipx's named with what removes each, and that \`jetcarp\` runs cargo's"
fi

echo "7. --uninstall after \`jetcarp uninstall --settings --data\`"
if run_installer "$work/intact" gone; then
    home="$work/home-gone"
    # What `jetcarp uninstall --settings --data` leaves: none of the folders, the runtime folder
    # too once nothing runs from it; and the pictures folder, which it never deletes, here empty.
    default_folders "$home" | while IFS= read -r f; do rm -rf "$f"; done
    mkdir -p "$home/Pictures/JETCARP"
    set +e
    HOME="$home" TMPDIR="$work/tmp-gone" sh "$installer" --version "$version" --uninstall \
        --dir "$home/lib/jetcarp" --bin-dir "$home/bin" >"$work/out-gone-uninstall" 2>&1
    status=$?
    set -e
    sed 's/^/    | /' "$work/out-gone-uninstall"
    back=$(default_folders "$home" | while IFS= read -r f; do if [ -e "$f" ]; then printf '%s ' "$f"; fi; done)
    if [ "$status" -ne 0 ]; then
        bad "gone: the installer's --uninstall failed"
    elif ! called "$home" "launcher remove"; then
        bad "gone: the installer did not run \`jetcarp launcher remove\`"
    elif [ -n "$back" ]; then
        bad "gone: the program the installer ran made these again: $back"
    elif grep -q "kept these folders" "$work/out-gone-uninstall"; then
        bad "gone: folders that hold nothing, or are gone, are named as kept"
    else
        ok "gone: no folder made again, and none named as kept"
    fi
else
    bad "gone: the installer failed"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures case(s) failed"
    exit 1
fi
echo "all seven cases passed"
