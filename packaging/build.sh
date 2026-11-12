#!/usr/bin/env bash
# Builds one release target exactly as the release workflow does, into dist/ (or $DIST):
# the Python wheel and the release archive, from one build of the program.
#
#   packaging/build.sh TARGET [PACKS_DIR]
#
# TARGET is one of the five release targets (packaging/package.py lists them). PACKS_DIR, when
# given, is the starter pack: a folder of pack stores, which goes into the archive's packs/ and
# the wheel's share/jetcarp/packs.
#
# Needs: Rust with the target installed, Python 3.8+, and maturin (pip install maturin). On Linux
# also ziglang (pip install ziglang): the program is linked with zig against glibc 2.28, so it
# runs on systems older than the one that built it. On macOS, Xcode's command line tools.
#
# Environment: DIST (default dist), PYTHON (default python3), MATURIN (default maturin),
# SOURCE_DATE_EPOCH (archive times; default the last commit's time).
set -euo pipefail

target=${1:?usage: packaging/build.sh TARGET [PACKS_DIR]}
packs=${2:-}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
py=${PYTHON:-python3}
maturin=${MATURIN:-maturin}
dist=${DIST:-$root/dist}
mkdir -p "$dist"
if [ -n "$packs" ]; then
    packs=$(cd "$packs" && pwd)
fi

version=$("$py" packaging/package.py version)
echo "JETCARP $version for $target"

# What differs by system: the Windows launcher, the Linux glibc floor, the macOS saver.
# (Never an empty array: macOS's bash 3.2 calls that unset under `set -u`.)
build_args=(--target "$target")
case "$target" in
    *-pc-windows-msvc) build_args+=(--features windows-launcher) ;;
    *-unknown-linux-gnu) build_args+=(--zig --compatibility manylinux_2_28) ;;
    aarch64-apple-darwin) wheel_arch=arm64 ;;
    x86_64-apple-darwin) wheel_arch=x86_64 ;;
    *)
        echo "unknown target $target" >&2
        exit 2
        ;;
esac
if [[ $target == *-apple-darwin ]]; then
    export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
fi
bins=target/$target/dist

wheel_data=(--out target/wheel-data)
if [ -n "$packs" ]; then
    wheel_data+=(--packs "$packs")
fi
if [[ $target == *-apple-darwin ]]; then
    cargo build --profile dist --locked -p jetcarp-saver --target "$target"
    # Ad hoc signed, as macOS needs of code it loads (no Developer ID for 0.4).
    codesign --force --sign - "$bins/libjetcarp_saver.dylib"
    wheel_data+=(--saver "$bins/libjetcarp_saver.dylib")
fi
"$py" packaging/package.py wheel-data "${wheel_data[@]}"

# One build of the program, for the wheel and the archive alike.
"$maturin" build "${build_args[@]}" --out "$dist"

if [[ $target == *-apple-darwin ]]; then
    # The linker signs arm64 programs ad hoc; sign again in case stripping undid it, and fail
    # if the wheel's copy (packed before this) is not validly signed.
    codesign --force --sign - "$bins/jetcarp"
    wheel=""
    for w in "$dist"/jetcarp-"$version"-*macosx_*_"$wheel_arch".whl; do
        wheel=$w
    done
    check=$(mktemp -d)
    "$py" -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$wheel" "$check"
    codesign --verify --verbose "$check/jetcarp-$version.data/scripts/jetcarp"
    rm -rf "$check"
fi
if [[ $target == *-unknown-linux-gnu ]]; then
    "$py" packaging/package.py check-elf "$bins/jetcarp" --max-glibc 2.28
fi

archive_args=(--target "$target" --bins "$bins" --out "$dist")
if [ -n "$packs" ]; then
    archive_args+=(--packs "$packs")
fi
"$py" packaging/package.py archive "${archive_args[@]}"
