#!/bin/sh
# JETCARP @VERSION@ installer for Linux and macOS.
#
#   curl -LsSf https://github.com/@REPO@/releases/latest/download/jetcarp-installer.sh | sh
#
# It downloads the release archive for this machine and the release's SHA256SUMS, checks the
# archive against it, unpacks it into one folder, and links the program into a folder on your
# PATH. Then the program adds JETCARP, with its icon, to your applications menu (Linux) or as
# JETCARP.app to ~/Applications (macOS): `jetcarp launcher install`. It changes nothing else: no
# shell startup files, no system folders, no sudo.
#
# Options, each also an environment variable (for `curl ... | sh -s -- OPTIONS` or `VAR=... sh`):
#   --version V     install release V instead of @VERSION@, such as 0.4.0, v0.4.0 or 0.4.0-rc.2
#                   (JETCARP_VERSION); pre-releases are never "latest", so name them
#   --dir DIR       the program's folder (JETCARP_INSTALL_DIR; default ~/.local/lib/jetcarp)
#   --bin-dir DIR   where the `jetcarp` link goes (JETCARP_BIN_DIR; default ~/.local/bin)
#   --no-packs      leave out the starter pack (JETCARP_NO_PACKS=1)
#   --no-launcher   add JETCARP to no applications menu or Applications folder
#                   (JETCARP_NO_LAUNCHER=1)
#   --url URL       where to get the release files: a URL or a local folder holding them
#                   (JETCARP_DOWNLOAD_URL; default
#                   https://github.com/@REPO@/releases/download/v<version>)
#   --gh            download with the GitHub CLI, `gh`, signed in: for a private repository
#                   (JETCARP_USE_GH=1)
#   --uninstall     remove the folder, the link and the launcher this installer made
#   -h, --help      this text
set -eu

VERSION="@VERSION@"
REPO="@REPO@"

version="${JETCARP_VERSION:-$VERSION}"
dir="${JETCARP_INSTALL_DIR:-}"
bin_dir="${JETCARP_BIN_DIR:-}"
url="${JETCARP_DOWNLOAD_URL:-}"
no_packs="${JETCARP_NO_PACKS:-}"
no_launcher="${JETCARP_NO_LAUNCHER:-}"
use_gh="${JETCARP_USE_GH:-}"
uninstall=""

say() { printf '%s\n' "jetcarp-installer: $*"; }
die() {
    printf '%s\n' "jetcarp-installer: error: $*" >&2
    exit 1
}

usage() {
    cat <<EOF
JETCARP $VERSION installer for Linux and macOS

  sh jetcarp-installer.sh [OPTIONS]
  curl -LsSf https://github.com/$REPO/releases/latest/download/jetcarp-installer.sh | sh -s -- [OPTIONS]

  --version V     install release V, such as 0.4.0 or 0.4.0-rc.2 (JETCARP_VERSION; default $VERSION)
  --dir DIR       the program's folder (JETCARP_INSTALL_DIR; default ~/.local/lib/jetcarp)
  --bin-dir DIR   where the \`jetcarp\` link goes (JETCARP_BIN_DIR; default ~/.local/bin)
  --no-packs      leave out the starter pack (JETCARP_NO_PACKS=1)
  --no-launcher   add JETCARP to no applications menu or Applications folder (JETCARP_NO_LAUNCHER=1)
  --url URL       a URL or local folder with the release files (JETCARP_DOWNLOAD_URL)
  --gh            download with the GitHub CLI, signed in, for a private repository
  --uninstall     remove the folder, the link and the launcher this installer made
  -h, --help      this text
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version="${2:?--version needs a version}"; shift 2 ;;
        --dir) dir="${2:?--dir needs a folder}"; shift 2 ;;
        --bin-dir) bin_dir="${2:?--bin-dir needs a folder}"; shift 2 ;;
        --url) url="${2:?--url needs a URL}"; shift 2 ;;
        --gh) use_gh=1; shift ;;
        --no-packs) no_packs=1; shift ;;
        --no-launcher) no_launcher=1; shift ;;
        --uninstall) uninstall=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) die "unknown option $1 (try --help)" ;;
    esac
done

version="${version#v}"
case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) die "$version is not a JETCARP version (such as 0.4.0 or 0.4.0-rc.2)" ;;
esac
[ -n "${HOME:-}" ] || die "HOME is not set"
dir="${dir:-$HOME/.local/lib/jetcarp}"
bin_dir="${bin_dir:-$HOME/.local/bin}"
url="${url:-https://github.com/$REPO/releases/download/v$version}"
url="${url%/}"
marker="$dir/.jetcarp-installer"

# Never replace or delete a folder this installer did not make.
case "$dir" in
    "" | / | "$HOME" | "$HOME/") die "refusing to use $dir as JETCARP's folder" ;;
esac
if [ -e "$dir" ] && [ ! -f "$marker" ]; then
    if [ -d "$dir" ] && [ -z "$(ls -A "$dir")" ]; then
        :
    else
        die "$dir exists and was not made by this installer; choose another with --dir"
    fi
fi

link_ours() {
    [ -L "$bin_dir/jetcarp" ] && [ "$(readlink "$bin_dir/jetcarp")" = "$dir/jetcarp" ]
}

# The folders `mkdir -p "$1"` would make, one per line, deepest first.
missing_dirs() {
    d=$1
    while [ -n "$d" ] && [ "$d" != "/" ] && [ "$d" != "." ] && [ ! -e "$d" ]; do
        printf '%s
' "$d"
        d=$(dirname "$d")
    done
}

# The folders an earlier install recorded as its own (`made` lines in the marker).
made_before() {
    if [ -f "$marker" ]; then
        sed -n 's/^made //p' "$marker"
    fi
}

if [ -n "$uninstall" ]; then
    [ -f "$marker" ] || die "no JETCARP installed by this installer in $dir"
    if [ -x "$dir/jetcarp" ]; then
        say "removing the program; what JETCARP set up outside its folders (the host, starting at login, the screen saver) is undone by \`jetcarp uninstall\`, not by this: if that is still to do, install again and run it"
    fi
    # The launcher the install asked the program for, taken out while the program is still there
    # (a version without the `launcher` command, before 0.4.3, made none). The program runs with
    # folders of its own, which go with it: a run of it may keep what it read of the machine in
    # its settings folder, or note itself in its runtime folder, and so make again a folder that
    # `jetcarp uninstall --settings --data` took (#350).
    scratch=$(mktemp -d 2>/dev/null || mktemp -d -t jetcarp)
    trap 'rm -rf "$scratch"' EXIT INT TERM
    program() {
        if [ "$(uname -s)" = Darwin ]; then
            # Not the settings folder on a Mac: a run there brings the screen saver up to date with
            # the settings in it (#287), which a folder of its own would empty.
            JETCARP_DATA_DIR="$scratch/data" JETCARP_RUNTIME_DIR="$scratch/runtime" \
                JETCARP_OUT="$scratch/output" "$dir/jetcarp" "$@"
        else
            JETCARP_CONFIG_DIR="$scratch/settings" JETCARP_DATA_DIR="$scratch/data" \
                JETCARP_RUNTIME_DIR="$scratch/runtime" JETCARP_OUT="$scratch/output" "$dir/jetcarp" "$@"
        fi
    }
    if [ -x "$dir/jetcarp" ] && program launcher status >/dev/null 2>&1; then
        if out=$(program launcher remove 2>&1); then
            printf '%s\n' "$out" | while IFS= read -r l; do say "$l"; done
        else
            say "could not remove JETCARP's launcher: $out"
        fi
    fi
    rm -rf "$scratch"
    if link_ours; then
        rm -f "$bin_dir/jetcarp"
        say "removed $bin_dir/jetcarp"
    fi
    made=$(made_before)
    rm -rf "$dir"
    say "removed $dir"
    # The folders the install made around it (such as ~/.local/lib), deepest first, if empty.
    printf '%s\n' "$made" | sort -ru | while IFS= read -r m; do
        if [ -n "$m" ] && [ -d "$m" ] && rmdir "$m" 2>/dev/null; then
            say "removed $m (empty, made by the install)"
        fi
    done
    # Named here, as `jetcarp paths` would have, since the program has gone: only the folders still
    # there that hold something, each with what it holds, so nothing is said of what `jetcarp
    # uninstall --settings --data` already took (#134, #142, #350). Each entry is "folder|what it
    # holds".
    case "$(uname -s)" in
        Darwin) set -- "$HOME/Library/Application Support/jetcarp|your settings and data" \
            "$HOME/Pictures/JETCARP|your pictures and exports" ;;
        *) set -- "${XDG_CONFIG_HOME:-$HOME/.config}/jetcarp|your settings" \
            "${XDG_DATA_HOME:-$HOME/.local/share}/jetcarp|your library and data" \
            "$HOME/Pictures/JETCARP|your pictures and exports" ;;
    esac
    first=1
    for k in "$@"; do
        folder=${k%|*}
        # A file in it, or in a folder in it: a folder of empty folders holds nothing.
        if [ -e "$folder" ] && [ -n "$(find "$folder" ! -type d 2>/dev/null | head -n 1)" ]; then
            if [ "$first" = 1 ]; then
                say "kept these folders; delete them by hand if you want them gone:"
                first=0
            fi
            say "  $folder (${k##*|})"
        fi
    done
    exit 0
fi

# `$1` with its symbolic links followed (`readlink -f` is not in every system's readlink).
resolve() {
    p=$1
    n=0
    while [ -L "$p" ] && [ "$n" -lt 40 ]; do
        t=$(readlink "$p")
        case "$t" in
            /*) p=$t ;;
            *) p=$(dirname "$p")/$t ;;
        esac
        n=$((n + 1))
    done
    printf '%s\n' "$p"
}

# Whose the JETCARP program `$1` (its links followed) is, and what removes it, as
# "whose|command"; "|" when neither can be told.
whose_jetcarp() {
    case "$1" in
        */Cellar/jetcarp/*) printf '%s\n' "Homebrew's|brew uninstall jetcarp" ;;
        */pipx/venvs/jetcarp/*) printf '%s\n' "pipx's|pipx uninstall jetcarp" ;;
        */uv/tools/jetcarp/*) printf '%s\n' "uv's|uv tool uninstall jetcarp" ;;
        *)
            d=$(dirname "$1")
            if [ -f "$d/.jetcarp-installer" ]; then
                printf '%s\n' "this installer's, in another folder|curl -LsSf https://github.com/$REPO/releases/latest/download/jetcarp-installer.sh | sh -s -- --uninstall --dir '$d'"
            elif [ -f "$(dirname "$d")/.crates.toml" ] || [ -f "$(dirname "$d")/.crates2.json" ]; then
                printf '%s\n' "cargo's|cargo uninstall jetcarp"
            else
                printf '%s\n' "|"
            fi
            ;;
    esac
}

# Says which other JETCARPs there are: every `jetcarp` on the PATH that is not this install's, and
# those of cargo, pipx and uv in their own folders, whether on the PATH or not. Each once, with
# what removes it, and whether the one `jetcarp` runs is another's.
other_jetcarps() {
    mine=$(resolve "$dir/jetcarp")
    first=""
    found=""
    seen=""
    old_ifs=$IFS
    IFS=:
    # shellcheck disable=SC2086 # split on the PATH's colons, on purpose.
    set -- ${PATH:-}
    IFS=$old_ifs
    cargo_home=${CARGO_HOME:-$HOME/.cargo}
    for p in "$@" "$cargo_home/bin" "${PIPX_HOME:-$HOME/.local/share/pipx}/venvs/jetcarp/bin" \
        "$HOME/.local/pipx/venvs/jetcarp/bin" "$HOME/Library/Application Support/pipx/venvs/jetcarp/bin" \
        "${XDG_DATA_HOME:-$HOME/.local/share}/uv/tools/jetcarp/bin" \
        /opt/homebrew/bin /usr/local/bin /home/linuxbrew/.linuxbrew/bin; do
        [ -n "$p" ] && [ -x "$p/jetcarp" ] && [ ! -d "$p/jetcarp" ] || continue
        real=$(resolve "$p/jetcarp")
        # The first on the PATH is the one `jetcarp` runs.
        if [ -z "$first" ]; then
            case ":${PATH:-}:" in *":$p:"*) first=$real ;; esac
        fi
        [ "$real" = "$mine" ] && continue
        case "|$seen|" in *"|$real|"*) continue ;; esac
        seen="$seen|$real"
        found="$found$real
"
    done
    [ -n "$found" ] || return 0
    if [ "$(printf '%s' "$found" | grep -c .)" -gt 1 ]; then
        say "other JETCARPs are installed on this computer as well; to keep only this one, remove them:"
    else
        say "another JETCARP is installed on this computer as well; to keep only this one, remove it:"
    fi
    printf '%s' "$found" | while IFS= read -r real; do
        [ -n "$real" ] || continue
        w=$(whose_jetcarp "$real")
        who=${w%%|*}
        cmd=${w#*|}
        if [ -n "$who" ]; then
            say "  $real ($who): $cmd"
        else
            say "  $real: remove it with what installed it"
        fi
    done
    if [ -n "$first" ] && [ "$first" != "$mine" ]; then
        say "\`jetcarp\` runs $first, which comes first on your PATH, until it is removed"
    fi
}

# Never replace a `jetcarp` on the PATH that another tool put there.
if [ -L "$bin_dir/jetcarp" ] && ! link_ours; then
    die "$bin_dir/jetcarp already links to $(readlink "$bin_dir/jetcarp") (pipx or uv, perhaps); remove that JETCARP first, or choose another --bin-dir"
elif [ -e "$bin_dir/jetcarp" ] && ! [ -L "$bin_dir/jetcarp" ]; then
    die "$bin_dir/jetcarp is a program this installer did not put there (cargo install, perhaps); remove it first, or choose another --bin-dir"
fi

# The release archive for this machine.
os=$(uname -s)
arch=$(uname -m)
case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "no JETCARP build for $arch processors; see https://jprodgers.github.io/jetcarp/getting-started/installing.html" ;;
esac
case "$os" in
    Linux)
        target="$arch-unknown-linux-gnu"
        if ldd --version 2>&1 | grep -qi musl; then
            die "this system uses musl, and JETCARP's Linux builds need glibc 2.28 or newer; \`cargo install jetcarp\` builds it here instead"
        fi
        glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}') || glibc=""
        if [ -n "$glibc" ]; then
            major=${glibc%%.*}
            minor=${glibc#*.}
            minor=${minor%%.*}
            if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 28 ]; }; then
                die "JETCARP needs glibc 2.28 or newer; this system has $glibc"
            fi
        fi
        ;;
    Darwin)
        # A shell under Rosetta reports x86_64 on Apple silicon: install the native build.
        if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
            arch=aarch64
        fi
        target="$arch-apple-darwin"
        ;;
    *) die "no JETCARP build for $os; on Windows use jetcarp-installer.ps1" ;;
esac
archive="jetcarp-$version-$target.tar.gz"

fetch() { # NAME: a file of the release, into $tmp
    if [ -n "$use_gh" ]; then
        command -v gh >/dev/null 2>&1 || die "--gh needs the GitHub CLI (https://cli.github.com), signed in"
        gh release download "v$version" --repo "$REPO" --pattern "$1" --dir "$tmp" --clobber
    elif [ -d "$url" ]; then
        cp "$url/$1" "$tmp/$1"
    elif command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$tmp/$1" "$url/$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$tmp/$1" "$url/$1"
    else
        die "neither curl nor wget is installed"
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | awk '{print $NF}'
    else
        die "no sha256sum, shasum or openssl to check the download with"
    fi
}

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t jetcarp)
trap 'rm -rf "$tmp"' EXIT INT TERM

from="$url"
[ -z "$use_gh" ] || from="$REPO's release v$version, with gh"
say "getting JETCARP $version for $target from $from"
fail() {
    case "$version" in
        *-*) hint=" (a pre-release: is v$version published? If the repository is private, try --gh)" ;;
        *) hint="" ;;
    esac
    die "could not get $1 from $from$hint"
}
fetch SHA256SUMS || fail SHA256SUMS
fetch "$archive" || fail "$archive"

want=$(awk -v f="$archive" '$2 == f || $2 == "*" f {print $1}' "$tmp/SHA256SUMS")
[ -n "$want" ] || die "SHA256SUMS does not list $archive"
got=$(sha256 "$tmp/$archive")
[ "$want" = "$got" ] || die "$archive does not match SHA256SUMS (expected $want, got $got)"
say "checked $archive against SHA256SUMS"

mkdir -p "$tmp/unpacked"
tar -xzf "$tmp/$archive" -C "$tmp/unpacked"
src="$tmp/unpacked/jetcarp-$version-$target"
[ -x "$src/jetcarp" ] || die "$archive has no jetcarp program"
if [ -n "$no_packs" ]; then
    rm -rf "$src/packs"
fi
# Swap the new folder in, keeping the old one until the new one is in place.
parent=$(dirname "$dir")
# The marker names the version, and every folder an install made, for --uninstall to remove
# when empty (an update keeps the ones the first install made).
{
    printf 'JETCARP %s %s\n' "$version" "$target"
    { missing_dirs "$parent"; missing_dirs "$bin_dir"; made_before; } | sort -ru | sed '/^$/d; s/^/made /'
} >"$src/.jetcarp-installer"
mkdir -p "$parent"
new="$dir.new.$$"
old="$dir.old.$$"
rm -rf "$new"
mv "$src" "$new" 2>/dev/null || cp -R "$src" "$new"
if [ -e "$dir" ]; then
    mv "$dir" "$old"
fi
mv "$new" "$dir"
rm -rf "$old"

mkdir -p "$bin_dir"
ln -sf "$dir/jetcarp" "$bin_dir/jetcarp"

say "installed $("$dir/jetcarp" --version 2>/dev/null || echo "JETCARP $version") in $dir"
say "linked $bin_dir/jetcarp"
if [ -d "$dir/packs" ]; then
    say "the starter pack is in $dir/packs"
fi
# JETCARP among the system's apps, with its icon (#105). A version without the `launcher` command
# (before 0.4.3) is left without one, and nothing is said.
if [ -z "$no_launcher" ] && "$dir/jetcarp" launcher status >/dev/null 2>&1; then
    if out=$("$dir/jetcarp" launcher install 2>&1); then
        printf '%s\n' "$out" | while IFS= read -r l; do say "$l"; done
    else
        say "could not add JETCARP to the system's apps (\`jetcarp launcher install\` tries again): $out"
    fi
fi
# Other JETCARPs on this computer (#348): two on the PATH run whichever comes first, and a menu
# may open the other. Each is named, with what removes it.
other_jetcarps
case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *)
        say "$bin_dir is not on your PATH. Add it, for example with this line in ~/.profile, ~/.bashrc or ~/.zshrc:"
        # shellcheck disable=SC2016 # $PATH is printed as it is, for the reader's shell.
        printf '\n    export PATH="%s:$PATH"\n\n' "$bin_dir"
        ;;
esac
say "start JETCARP with \`jetcarp\`; \`jetcarp doctor\` shows what it sees on this machine"
