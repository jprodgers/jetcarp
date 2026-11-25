#!/bin/sh
# JETCARP's field test, for Linux and macOS: test a JETCARP release on this computer, and get one
# report file to send by hand to whoever asked you to.
#
#   curl -LsSf https://raw.githubusercontent.com/jprodgers/jetcarp/main/testing/field/jetcarp-field-test.sh -o jetcarp-field-test.sh
#   sh jetcarp-field-test.sh
#
# It asks which release to test (the newest, release candidates included, unless you choose
# another) and for how long (Quick, a few minutes; Suite, 10 to 20; Extended, 30 to 60). It
# downloads that release and its SHA256SUMS from GitHub, checks the archive against them as
# JETCARP's installer does, unpacks it into a temporary folder and runs it from there, with
# JETCARP's settings, data and output folders inside that folder too: your own JETCARP, its
# settings and its library are never touched. Every test is a command-line run: no app window,
# no background host, nothing drawn on the screen, so nothing can flash. The tests themselves are
# in tests.json beside this script (or, run on its own, from the JETCARP repository).
#
# At the end it writes the report (JSON, with a short summary at the top), shows you what is in
# it and where it is, and asks whether to remove everything it added (the default) or keep this
# version, installed by JETCARP's own installer. Nothing is uploaded: JETCARP sends nothing
# anywhere, and neither does this script. It only downloads: the list of releases, the release
# and the test data.
#
# Options (each skips its question; see --help): --release TAG, --tier quick|suite|extended,
# --yes, --keep or --remove, --report PATH, --data DIR|URL. Ctrl-C stops the running test, cleans
# up and still writes a report, marked as stopped.
#
# POSIX sh, with awk, tar, ps and curl or wget: nothing else to install.
#
# shellcheck disable=SC2016 # the awk programs are in single quotes, for awk to read
set -u

FT_VERSION=1
REPO=jprodgers/jetcarp
DATA_URL="https://raw.githubusercontent.com/$REPO/main/testing/field"
API_URL="https://api.github.com/repos/$REPO/releases"
ESC=$(printf '\033')
TAB=$(printf '\t')
# A debugging aid: keep the temporary folder at the end.
KEEP_WORK=${FIELD_TEST_KEEP_WORK:-}

usage() {
    cat <<'EOF'
JETCARP's field test for Linux and macOS: test a JETCARP release on this computer and get one
report file to send by hand. Nothing is sent anywhere, nothing is drawn on screen, and your own
JETCARP is never touched.

  sh jetcarp-field-test.sh [OPTIONS]

  --release TAG    test this release (0.4.3-rc.2, v0.4.2, latest) instead of choosing one
  --tier T         quick (a few minutes), suite (10 to 20) or extended (30 to 60)
  --yes            ask nothing: the newest release, the quick tier, and remove everything at
                   the end, unless the other options say otherwise
  --keep           at the end, install the tested version with JETCARP's installer
  --remove         at the end, remove everything this script added (the default)
  --report PATH    write the report here (default: jetcarp-field-report-....json in this
                   folder, or in your home folder if this one can't be written)
  --data DIR|URL   where tests.json and the test data are (default: beside this script, else
                   the JETCARP repository on GitHub)
  -h, --help       this text
EOF
}

opt_release=""
opt_tier=""
opt_yes=""
opt_end=""
opt_report=""
opt_data=""
while [ $# -gt 0 ]; do
    case "$1" in
        --release) opt_release="${2:?--release needs a release, such as 0.4.3-rc.2}"; shift 2 ;;
        --release=*) opt_release="${1#*=}"; shift ;;
        --tier) opt_tier="${2:?--tier needs quick, suite or extended}"; shift 2 ;;
        --tier=*) opt_tier="${1#*=}"; shift ;;
        --yes | -y) opt_yes=1; shift ;;
        --keep) opt_end=keep; shift ;;
        --remove) opt_end=remove; shift ;;
        --report) opt_report="${2:?--report needs a file name}"; shift 2 ;;
        --report=*) opt_report="${1#*=}"; shift ;;
        --data) opt_data="${2:?--data needs a folder or a URL}"; shift 2 ;;
        --data=*) opt_data="${1#*=}"; shift ;;
        -h | --help) usage; exit 0 ;;
        *) printf 'jetcarp-field-test: unknown option %s (try --help)\n' "$1" >&2; exit 2 ;;
    esac
done
case "$opt_tier" in
    "" | quick | suite | extended) ;;
    *) printf 'jetcarp-field-test: --tier is quick, suite or extended, not %s\n' "$opt_tier" >&2; exit 2 ;;
esac

# ---- How it looks ---------------------------------------------------------------------------

TTY=""
[ -t 1 ] && TTY=1
# Live lines (a spinner, progress bars, redrawn in place) need a terminal that moves the cursor.
LIVE=""
[ -n "$TTY" ] && [ "${TERM:-dumb}" != dumb ] && LIVE=1
# Someone to ask: a terminal both ways, and no --yes.
ASK=""
[ -z "$opt_yes" ] && [ -t 0 ] && [ -t 1 ] && ASK=1
B="" D="" R="" RED="" GRN="" YEL="" CYA=""
if [ -n "$LIVE" ] && [ -z "${NO_COLOR:-}" ]; then
    B="${ESC}[1m" D="${ESC}[2m" R="${ESC}[0m" RED="${ESC}[31m" GRN="${ESC}[32m" YEL="${ESC}[33m" CYA="${ESC}[36m"
fi
# shellcheck disable=SC1003 # the last spinner frame is two backslashes, which awk reads as one
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
    *[Uu][Tt][Ff]-8* | *[Uu][Tt][Ff]8*) FULL="█" EMPTY="░" SPIN="⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏" ;;
    *) FULL="#" EMPTY="-" SPIN='| / - \\' ;;
esac
if sleep 0.2 2>/dev/null; then TICK=0.2; else TICK=1; fi
TICKN=0
STATUS_ON=""

cols() {
    _c=$(stty size 2>/dev/null </dev/tty | awk '{ print $2 }')
    case "$_c" in "" | *[!0-9]*) _c=$(tput cols 2>/dev/null) ;; esac
    case "$_c" in "" | *[!0-9]*) _c=80 ;; esac
    [ "$_c" -lt 40 ] && _c=40
    echo "$_c"
}
COLS=$(cols)

# The two live lines (this test, and the whole run) are taken off the screen before anything
# else is written.
status_clear() {
    if [ -n "$STATUS_ON" ]; then
        printf '\r%s[K\n%s[K%s[1A\r' "$ESC" "$ESC" "$ESC"
        STATUS_ON=""
    fi
}

# draw LABEL TEST_PERMILLE TEXT ALL_PERMILLE ALL_TEXT: the two live lines, a permille below 0
# drawing no bar.
draw() {
    [ -n "$LIVE" ] || return 0
    TICKN=$((TICKN + 1))
    awk -v e="$ESC" -v frames="$SPIN" -v n="$TICKN" -v lab="$1" -v tf="$2" -v ct="$3" \
        -v af="$4" -v at="$5" -v w="$COLS" -v full="$FULL" -v empty="$EMPTY" -v cy="$CYA" -v r="$R" '
    function bar(f, m,   k, s, i) {
        k = int(f * m / 1000 + 0.5); if (k > m) k = m; if (k < 0) k = 0
        s = ""; for (i = 0; i < k; i++) s = s full; for (; i < m; i++) s = s empty
        return s
    }
    function fit(s, m) { if (m < 4) m = 4; return length(s) > m ? substr(s, 1, m - 3) "..." : s }
    BEGIN {
        w = w - 1; bw = (w >= 72) ? 24 : 12
        nf = split(frames, fr, " "); sp = fr[n % nf + 1]
        # The label keeps at least 24 columns (or all of itself); the count gets the rest.
        avail = w - 4 - (tf >= 0 ? bw + 2 : 2)
        ll = length(lab); if (ll > 24) ll = 24
        if (length(ct) > avail - ll) ct = fit(ct, avail - ll)
        lab = fit(lab, avail - length(ct))
        if (tf >= 0) l1 = "  " cy sp r " " lab " " cy bar(tf, bw) r " " ct
        else l1 = "  " cy sp r " " lab (ct == "" ? "" : "  " ct)
        l2 = ""
        if (af >= 0) l2 = "    all  " cy bar(af, bw) r " " fit(at, w - 10 - bw)
        printf "\r%s[K%s\n\r%s[K%s%s[1A\r", e, l1, e, l2, e
    }'
    STATUS_ON=1
}

say() { status_clear; printf '%s\n' "$*"; }
info() { status_clear; printf '  %s\n' "$*"; }
dim() { status_clear; printf '  %s%s%s\n' "$D" "$*" "$R"; }
warn() { status_clear; printf '  %s%s%s\n' "$YEL" "$*" "$R"; }
heading() { status_clear; printf '\n%s%s%s\n' "$B$CYA" "$*" "$R"; }
die() {
    status_clear
    printf '%sjetcarp-field-test: %s%s\n' "$RED" "$*" "$R" >&2
    exit 1
}
tag() {
    case "$1" in
        pass) printf '%s[ ok ]%s' "$GRN" "$R" ;;
        warn) printf '%s[warn]%s' "$YEL" "$R" ;;
        fail) printf '%s[FAIL]%s' "$RED$B" "$R" ;;
        stop) printf '%s[stop]%s' "$YEL" "$R" ;;
        *) printf '%s[skip]%s' "$D" "$R" ;;
    esac
}

plaintag() {
    case "$1" in pass) echo "[ ok ]" ;; warn) echo "[warn]" ;; fail) echo "[FAIL]" ;; stop) echo "[stop]" ;; *) echo "[skip]" ;; esac
}

# ask PROMPT DEFAULT: the line typed (Enter: DEFAULT), in ANSWER; DEFAULT with no one to ask.
ask() {
    ANSWER=$2
    [ -n "$ASK" ] || return 0
    status_clear
    printf '%s%s%s ' "$B" "$1" "$R"
    if IFS= read -r ANSWER; then
        [ -n "$ANSWER" ] || ANSWER=$2
    else
        # The input ended: there is no one to ask from here on.
        ASK=""
        ANSWER=$2
        printf '\n'
    fi
}

# ---- Small helpers --------------------------------------------------------------------------

now() { date +%s; }
awkc() { LC_ALL=C awk "$@"; }
commas() { awk -v n="$1" 'BEGIN { s = sprintf("%.0f", n); g = ""; if (s ~ /^-/) { g = "-"; s = substr(s, 2) }; o = ""; while (length(s) > 3) { o = "," substr(s, length(s) - 2) o; s = substr(s, 1, length(s) - 3) }; print g s o }'; }
dur() { awk -v s="$1" 'BEGIN { s = int(s + 0.5); if (s < 60) printf "%d s", s; else if (s < 3600) printf "%d min %02d s", s / 60, s % 60; else printf "%d h %02d min", s / 3600, (s % 3600) / 60 }'; }
left() { awk -v s="$1" 'BEGIN { if (s < 45) print "under a minute left"; else if (s < 5400) printf "about %d min left\n", int(s / 60 + 0.5); else printf "about %.1f h left\n", s / 3600 }'; }
human() { awk -v b="${1:-0}" 'BEGIN { if (b >= 1073741824) printf "%.1f GB", b / 1073741824; else if (b >= 1048576) printf "%.1f MB", b / 1048576; else if (b >= 1024) printf "%.0f KB", b / 1024; else printf "%d bytes", b }'; }
# Lines wrapped to the terminal, each one's continuation indented under its text.
wrap() { awk -v w=$((COLS - ${1:-0})) '{ n = split($0, a, " "); l = ""; ind = ""; for (i = 1; i <= n; i++) { if (l != "" && length(l) + 1 + length(a[i]) > w - 1) { print l; l = "         " a[i] } else l = (l == "" ? a[i] : l " " a[i]) } if (l != "") print l }'; }
tilde() { case "$1" in "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;; *) printf '%s' "$1" ;; esac; }
# vkey VERSION: a key that sorts releases in order, a release candidate before its release.
vkey() { printf '%s\n' "${1#v}" | awk -F '[.-]' '{ p = 99999; if ($4 == "rc") p = $5 + 0; else if ($4 != "") p = 0; printf "%05d%05d%05d%05d\n", $1, $2, $3, p }'; }
# in_range VERSION FROM TO: FROM <= VERSION <= TO, an empty bound being open.
in_range() {
    _v=$(vkey "$1")
    _f=""
    _t=""
    [ -n "$2" ] && _f=$(vkey "$2")
    [ -n "$3" ] && _t=$(vkey "$3")
    awk -v v="$_v" -v f="$_f" -v t="$_t" 'BEGIN { exit !((f == "" || (v "") >= (f "")) && (t == "" || (v "") <= (t ""))) }'
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{ print $1 }'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | awk '{ print $NF }'
    else
        die "no sha256sum, shasum or openssl to check the download with"
    fi
}

fetch() { # URL FILE
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 --connect-timeout 20 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -T 20 -O "$2" "$1"
    else
        return 1
    fi
}

# ---- JSON, with awk alone --------------------------------------------------------------------

# Flattens a JSON text into one line per value: its path (.key, [index]) and the value as JSON
# wrote it (strings keep their quotes and escapes). Records split at quotation marks, so a long
# line costs no more than a short one. `-v only="PATH PATH"` keeps only those paths.
AWK_FLAT='
BEGIN { RS = "\""; d = 0; instr = 0; lit = ""; sb = ""; nw = split(only, wl, " "); for (i = 1; i <= nw; i++) want[wl[i]] = 1 }
function path(   i, p) { p = ""; for (i = 1; i <= d; i++) p = p (t[i] == "o" ? "." k[i] : "[" x[i] "]"); return p }
function emit(v,   p) { p = path(); if (nw == 0 || (p in want)) print p "\t" v }
function flush() { if (lit != "") { emit(lit); lit = "" } }
function outside(s,   i, n, c) {
    n = length(s)
    for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        if (c == "{" || c == "[") { flush(); d++; t[d] = (c == "{") ? "o" : "a"; k[d] = ""; x[d] = 0; e[d] = (c == "{") }
        else if (c == "}" || c == "]") { flush(); d-- }
        else if (c == ",") { flush(); if (t[d] == "o") e[d] = 1; else x[d]++ }
        else if (c == ":") { flush(); e[d] = 0 }
        else if (c == " " || c == "\t" || c == "\n" || c == "\r") flush()
        else lit = lit c
    }
}
{
    if (!instr) { outside($0); instr = 1; next }
    s = sb $0; j = length(s); b = 0
    while (j > 0 && substr(s, j, 1) == "\\") { b++; j-- }
    if (b % 2) { sb = s "\""; next }
    sb = ""; instr = 0
    if (d > 0 && t[d] == "o" && e[d]) k[d] = s; else emit("\"" s "\"")
}
END { flush() }
'
# unq(v): a flattened value as text: strings unquoted, null empty.
AWK_UNQ='
function unq(v,   o, i, n, c) {
    if (v == "null") return ""
    if (substr(v, 1, 1) != "\"") return v
    v = substr(v, 2, length(v) - 2)
    if (index(v, "\\") == 0) return v
    o = ""; n = length(v)
    for (i = 1; i <= n; i++) {
        c = substr(v, i, 1)
        if (c == "\\" && i < n) {
            i++; c = substr(v, i, 1)
            if (c == "n" || c == "t") c = " "; else if (c == "r") c = ""; else if (c == "u") c = "\\u"
        }
        o = o c
    }
    return o
}
'
# js(s): s as a JSON string.
AWK_JS='
function js(s,   r, i, n, c) {
    if (!jsready) { for (i = 1; i < 32; i++) ctl[sprintf("%c", i)] = sprintf("\\u%04x", i); ctl["\t"] = "\\t"; ctl["\r"] = "\\r"; ctl["\n"] = "\\n"; jsready = 1 }
    r = ""; n = length(s)
    for (i = 1; i <= n; i++) { c = substr(s, i, 1); if (c == "\\") r = r "\\\\"; else if (c == "\"") r = r "\\\""; else if (c in ctl) r = r ctl[c]; else r = r c }
    return "\"" r "\""
}
'
jflat() { awkc -v only="${3:-}" "$AWK_FLAT" "$1" >"$2"; }
jget() { awkc -v p="$2" "BEGIN { FS = \"\\t\" } $AWK_UNQ"' $1 == p { print unq($2); f = 1; exit } END { exit !f }' "$1"; }
jarr() { awkc -v p="$2" "BEGIN { FS = \"\\t\" } $AWK_UNQ"' index($1, p "[") == 1 && substr($1, length(p) + 1) ~ /^\[[0-9]+\]$/ { print unq($2) }' "$1"; }
jlen() { awkc -v p="$2" 'BEGIN { FS = "\t"; m = -1 } index($1, p "[") == 1 { r = substr($1, length(p) + 2); sub(/\].*/, "", r); if (r ~ /^[0-9]+$/ && r + 0 > m) m = r + 0 } END { print m + 1 }' "$1"; }
jsub() { awkc -v p="$2" 'BEGIN { FS = "\t" } index($1, p) == 1 { print substr($1, length(p) + 1) "\t" $2 }' "$1"; }
# js TEXT, jsf FILE: as one JSON string (a file's lines joined with \n).
js() { printf '%s' "$1" | awkc "$AWK_JS"'{ o = o (NR > 1 ? "\n" : "") $0 } END { printf "%s", js(o) }'; }
jsf() { awkc "$AWK_JS"'{ o = o (NR > 1 ? "\n" : "") $0 } END { printf "%s", js(o) }' "$1"; }
# json_ok FILE: it looks like one JSON object or array (it is embedded in the report as it is).
json_ok() {
    [ -s "$1" ] && awk '{ s = $0; gsub(/[ \t\r]/, "", s); if (s != "") { if (f == "") f = substr(s, 1, 1); l = substr(s, length(s), 1) } } END { exit !((f == "{" && l == "}") || (f == "[" && l == "]")) }' "$1"
}
tget() { jget "$TESTS" ".tests[$1].$2"; }

# pick_range FLAT ARRAY: the index of the entry of ARRAY whose from..to holds the release being
# tested (the latest `from` when several do); nothing when none does.
pick_range() {
    _pn=$(jlen "$1" "$2")
    _pi=0
    _pbest=""
    _pkey=""
    while [ "$_pi" -lt "$_pn" ]; do
        _pf=$(jget "$1" "$2[$_pi].from")
        _pt=$(jget "$1" "$2[$_pi].to")
        if in_range "$V" "$_pf" "$_pt"; then
            _pk=$(vkey "${_pf:-0.0.0}")
            if [ -z "$_pkey" ] || expr "x$_pk" \> "x$_pkey" >/dev/null; then
                _pbest=$_pi
                _pkey=$_pk
            fi
        fi
        _pi=$((_pi + 1))
    done
    [ -n "$_pbest" ] && echo "$_pbest"
}

# ---- Where it runs --------------------------------------------------------------------------

OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
    x86_64 | amd64) ARCH=x86_64 ;;
    aarch64 | arm64) ARCH=aarch64 ;;
    *) die "JETCARP has no build for $ARCH processors" ;;
esac
case "$OS" in
    Linux)
        OSN=linux
        TARGET="$ARCH-unknown-linux-gnu"
        SYSTEM=$(sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null | tr -d '"')
        [ -n "$SYSTEM" ] || SYSTEM="Linux"
        ;;
    Darwin)
        OSN=macos
        # A shell under Rosetta reports x86_64 on Apple silicon: test the native build.
        if [ "$ARCH" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
            ARCH=aarch64
        fi
        TARGET="$ARCH-apple-darwin"
        SYSTEM="$(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null)"
        ;;
    *) die "this script is for Linux and macOS; on Windows, use jetcarp-field-test.ps1" ;;
esac
KERNEL=$(uname -sr)

# The display is closed to every run: with no display, nothing can open a window (Linux). The
# values are kept for JETCARP's installer, if you keep the version at the end.
ORIG_DISPLAY=${DISPLAY-}
ORIG_WAYLAND=${WAYLAND_DISPLAY-}
unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET
# The JETCARP and wgpu variables of this shell, kept the same way, then cleared: the tests run with
# their own.
ORIG_ENV=$(env | grep -E '^(JETCARP|WGPU)_[A-Za-z0-9_]*=' || true)
for _n in $(printf '%s\n' "$ORIG_ENV" | sed -n 's/^\([A-Za-z0-9_]*\)=.*/\1/p'); do
    unset "$_n"
done

# ---- State ----------------------------------------------------------------------------------

W=""
JET=""
JPID=""
NETPID=""
STARTED=""
STOPPED=""
FINISHED=""
CURRENT=""
REPORT=""
GPU_NAME=""
GPU_API=""
K=0
FRAG=0
N_PASS=0
N_WARN=0
N_FAIL=0
N_SKIP=0
EST_TOTAL=0
EST_DONE=0
EST_CUR=0
FACTOR=1000
R_SUM=0
E_SUM=0
THREADS=""
MEM=""
CPU=""
CORES=""
N_DEVICES=1
N_OTHERS=1
PAR_SHORT=""
PAR_LONG=""
PAR_NOTE=""

# shellcheck disable=SC2329 # run by the EXIT trap
cleanup() {
    status_clear
    if [ -n "$JPID" ]; then kill -TERM "$JPID" 2>/dev/null; fi
    if [ -n "$NETPID" ]; then kill "$NETPID" 2>/dev/null; fi
    case "$W" in
        */jetcarp-field.*)
            if [ -n "$KEEP_WORK" ]; then
                printf 'Kept the temporary folder: %s\n' "$W"
            else
                rm -rf "$W"
            fi
            ;;
    esac
}

# Ctrl-C, or the terminal closing: stop the running test, write what there is, clean up.
# shellcheck disable=SC2329 # run by the INT, TERM and HUP traps
on_stop() {
    trap '' INT TERM HUP
    if [ -n "$FINISHED" ]; then
        status_clear
        printf '\n'
        say "The report: $(tilde "$REPORT")"
        exit 130
    fi
    STOPPED=1
    status_clear
    printf '\n'
    if [ -n "$JPID" ]; then
        kill -TERM "$JPID" 2>/dev/null
        stop_wait
    fi
    if [ -n "$NETPID" ]; then kill "$NETPID" 2>/dev/null; fi
    warn "Stopped ($1)."
    if [ -n "$STARTED" ]; then
        if [ -n "$CURRENT" ]; then
            problem "stopped ($1) while this test ran"
            test_end stop "stopped part way"
        fi
        write_report
        say "A report of what had run, marked as stopped: $(tilde "$REPORT")"
    else
        say "Nothing had been tested, so there is no report."
    fi
    exit 130
}

# Waits up to 15 seconds for JPID to end after being asked to, then ends it.
stop_wait() {
    _sw=0
    while kill -0 "$JPID" 2>/dev/null && [ "$_sw" -lt 75 ]; do
        sleep 0.2 2>/dev/null || sleep 1
        _sw=$((_sw + 1))
    done
    kill -KILL "$JPID" 2>/dev/null
    wait "$JPID" 2>/dev/null
    JPID=""
}

# ---- Running JETCARP ------------------------------------------------------------------------

gpu_env() {
    if [ -n "$GPU_NAME" ]; then
        JETCARP_GPU=$GPU_NAME
        export JETCARP_GPU
    fi
    if [ -n "$GPU_API" ]; then
        WGPU_BACKEND=$GPU_API
        export WGPU_BACKEND
    fi
}

# jgo fg|bg|tty OUT ERR ARGFILE [ARGS...]: the scratch JETCARP with the arguments in ARGFILE (one
# a line), then ARGS, on the GPU named by GPU_NAME and GPU_API. fg waits (JRC is its status), bg
# leaves it running (JPID), tty runs it on the terminal, so that it can ask.
jgo() {
    _g_m=$1
    _g_o=$2
    _g_e=$3
    _g_a=$4
    shift 4
    _g_n=$#
    while IFS= read -r _g_l; do set -- "$@" "$_g_l"; done <"$_g_a"
    while [ "$_g_n" -gt 0 ]; do
        _g_x=$1
        shift
        set -- "$@" "$_g_x"
        _g_n=$((_g_n - 1))
    done
    CUR_OUT=$_g_o
    CUR_ERR=$_g_e
    case "$_g_m" in
        bg)
            (gpu_env && exec "$JET" "$@" 3<&-) >"$_g_o" 2>"$_g_e" </dev/null &
            JPID=$!
            ;;
        tty)
            (gpu_env && exec "$JET" "$@" 3<&-)
            JRC=$?
            ;;
        *)
            (gpu_env && exec "$JET" "$@" 3<&-) >"$_g_o" 2>"$_g_e" </dev/null
            JRC=$?
            ;;
    esac
}

# The overall line's numbers: where the whole run stands, and the time left.
all_permille() { # THIS_TEST_DONE (in estimate seconds)
    [ "$EST_TOTAL" -gt 0 ] || { echo 0; return; }
    echo $(((EST_DONE + $1) * 1000 / EST_TOTAL))
}
all_text() { # THIS_TEST_DONE
    _lft=$(((EST_TOTAL - EST_DONE - $1) * FACTOR / 1000))
    [ "$_lft" -lt 0 ] && _lft=0
    printf 'test %s of %s, %s' "$K" "$N_PLAN" "$(left "$_lft")"
}

# tick PERMILLE TEXT [ELAPSED]: redraw the live lines (a permille below 0: no count, so ELAPSED
# says how far the test has got).
tick() {
    if [ "$1" -ge 0 ]; then
        _cd=$((EST_CUR * $1 / 1000))
    else
        _cd=$((${3:-0} * 1000 / FACTOR))
        _cap=$((EST_CUR * 95 / 100))
        [ "$_cd" -gt "$_cap" ] && _cd=$_cap
    fi
    draw "$T_NAME" "$1" "$2" "$(all_permille "$_cd")" "$(all_text "$_cd")"
}

# jwait MODE ARG [TIMEOUT]: wait for JPID, drawing its progress, and set JRC. MODE: count WORD
# (the last "N/M WORD" on its error stream), runs (the last "N runs" there, against ARG seconds),
# time (ARG seconds, the last line of the error stream beside it), fit (host tune's measured
# sizes), or none. Past TIMEOUT seconds it is stopped. With SAMPLE_RSS set, its memory is read
# every 5 seconds into $CUR/rss.
jwait() {
    _jm=$1
    _ja=${2:-}
    _to=${3:-0}
    _j0=$(now)
    _jplain=$_j0
    _jrss=-5
    JTIMEDOUT=""
    while kill -0 "$JPID" 2>/dev/null; do
        _el=$(($(now) - _j0))
        _pm=-1
        _tx=$(dur "$_el")
        case "$_jm" in
            count)
                _c=$(tail -n 4 "$CUR_ERR" 2>/dev/null | awk -v w="$_ja" '{ s = $0; while (match(s, "[0-9][0-9,]*/[0-9][0-9,]* " w)) { m = substr(s, RSTART, RLENGTH); s = substr(s, RSTART + RLENGTH) } } END { if (m != "") { split(m, a, "[/ ]"); gsub(",", "", a[1]); gsub(",", "", a[2]); if (a[2] > 0) print int(a[1] * 1000 / a[2]), a[1], a[2] } }')
                if [ -n "$_c" ]; then
                    # shellcheck disable=SC2086 # the numbers, split on purpose
                    set -- $_c
                    _pm=$1
                    _tx="$(commas "$2") of $(commas "$3") $_ja"
                fi
                ;;
            runs)
                _pm=$((_el * 1000 / _ja))
                [ "$_pm" -gt 999 ] && _pm=999
                _r=$(tail -n 3 "$CUR_ERR" 2>/dev/null | awk '{ if (match($0, /[0-9][0-9,]* runs/)) m = substr($0, RSTART, RLENGTH) } END { print m }')
                _tx="$(dur "$_el") of $(dur "$_ja")${_r:+, $_r}${RSS_NOW:+, $RSS_NOW}"
                ;;
            time)
                _pm=$((_el * 1000 / _ja))
                [ "$_pm" -gt 990 ] && _pm=990
                _tx=$(tail -n 1 "$CUR_ERR" 2>/dev/null | tr -d '\r' | cut -c1-48)
                ;;
            fit)
                _c=$(awk '/"event":"rung"/ { if (match($0, /"rungs":[0-9]+/)) r = substr($0, RSTART + 8, RLENGTH - 8) } /"event":"measured"/ { m++ } END { if (r > 0) print int(m * 1000 / r), m + 0 }' "$CUR_OUT" 2>/dev/null)
                if [ -n "$_c" ]; then
                    # shellcheck disable=SC2086 # the numbers, split on purpose
                    set -- $_c
                    _pm=$1
                    [ "$_pm" -gt 999 ] && _pm=999
                    _tx="$2 sizes measured"
                fi
                ;;
        esac
        if [ -n "${SAMPLE_RSS:-}" ] && [ $((_el - _jrss)) -ge 5 ]; then
            _jrss=$_el
            _kb=$(ps -o rss= -p "$JPID" 2>/dev/null | tr -d ' ')
            if [ -n "$_kb" ]; then
                printf '%s %s\n' "$_el" "$_kb" >>"$CUR/rss"
                RSS_NOW="$(human $((_kb * 1024))) in use"
            fi
        fi
        tick "$_pm" "$_tx" "$_el"
        if [ -z "$LIVE" ] && [ $(($(now) - _jplain)) -ge 30 ]; then
            _jplain=$(now)
            printf '    ... %s\n' "$_tx"
        fi
        if [ "$_to" -gt 0 ] && [ "$_el" -gt "$_to" ]; then
            JTIMEDOUT=1
            kill -TERM "$JPID" 2>/dev/null
            stop_wait
            JRC=124
            return
        fi
        sleep "$TICK"
    done
    wait "$JPID"
    JRC=$?
    JPID=""
}

# ---- A test's record ------------------------------------------------------------------------

m_put() { # FILE KEY RAWJSON
    [ -s "$1" ] && printf ',\n' >>"$1"
    printf '        %s: %s' "$(js "$2")" "$3" >>"$1"
}
num() { m_put "$CUR/numbers" "$1" "$2"; }
numstr() { m_put "$CUR/numbers" "$1" "$(js "$2")"; }
det() { m_put "$CUR/details" "$1" "$2"; }
detfile() { # KEY FILE: a JSON file as it is (or, if it isn't JSON, as text)
    [ -s "$CUR/details" ] && printf ',\n' >>"$CUR/details"
    printf '        %s: ' "$(js "$1")" >>"$CUR/details"
    if json_ok "$2"; then cat "$2" >>"$CUR/details"; else jsf "$2" >>"$CUR/details"; fi
}
problem() { # TEXT [OUTPUT_FILE]: what went wrong, with the end of its output
    [ -s "$CUR/problems" ] && printf ',\n' >>"$CUR/problems"
    if [ -n "${2:-}" ] && [ -s "$2" ]; then
        printf '        {"what": %s, "output": %s}' "$(js "$1")" "$(tail -n 40 "$2" | awkc "$AWK_JS"'{ o = o (NR > 1 ? "\n" : "") $0 } END { printf "%s", js(o) }')" >>"$CUR/problems"
    else
        printf '        {"what": %s}' "$(js "$1")" >>"$CUR/problems"
    fi
}
members() { if [ -s "$1" ]; then printf '\n'; cat "$1"; printf '\n      '; fi; }

# est INDEX: a test's time on a typical laptop, in seconds, for the GPUs found so far.
est() {
    _es=$(tget "$1" seconds)
    _ep=$(tget "$1" per_adapter)
    case "$(tget "$1" adapters)" in
        chosen) _ea=1 ;;
        devices) _ea=$N_DEVICES ;;
        others) _ea=$N_OTHERS ;;
        *) _ea=0 ;;
    esac
    echo $((${_es:-0} + ${_ep:-0} * _ea))
}

test_begin() { # INDEX
    TI=$1
    K=$((K + 1))
    FRAG=$((FRAG + 1))
    T_ID=$(tget "$TI" id)
    T_NAME=$(tget "$TI" name)
    T_KIND=$(tget "$TI" kind)
    CUR="$W/tests/$FRAG-$T_ID"
    mkdir -p "$CUR"
    : >"$CUR/numbers"
    : >"$CUR/details"
    : >"$CUR/problems"
    T_CMD=""
    T_START=$(now)
    EST_CUR=$(est "$TI")
    CURRENT=$K
    heading "[$K/$N_PLAN] $T_NAME"
    dim "$(tget "$TI" about)"
}

# test_end RESULT SUMMARY: the test's record, and its line on screen.
test_end() {
    _secs=$(($(now) - T_START))
    {
        printf '    {\n'
        printf '      "id": %s,\n      "name": %s,\n      "kind": %s,\n' "$(js "$T_ID")" "$(js "$T_NAME")" "$(js "$T_KIND")"
        printf '      "result": %s,\n      "summary": %s,\n      "seconds": %s,\n' "$(js "$1")" "$(js "$2")" "$_secs"
        printf '      "command": %s,\n' "$(js "$T_CMD")"
        printf '      "numbers": {'
        members "$CUR/numbers"
        printf '},\n      "details": {'
        members "$CUR/details"
        printf '},\n      "problems": ['
        members "$CUR/problems"
        printf ']\n    }'
    } >"$W/out/$(printf '%03d' "$FRAG").json"
    case "$1" in
        pass) N_PASS=$((N_PASS + 1)) ;;
        warn) N_WARN=$((N_WARN + 1)) ;;
        fail) N_FAIL=$((N_FAIL + 1)) ;;
    esac
    status_clear
    printf '  %s %s %s(%s)%s\n' "$(tag "$1")" "$2" "$D" "$(dur "$_secs")" "$R"
    printf '%s %s: %s\n' "$(plaintag "$1")" "$T_NAME" "$2" >>"$W/summary.txt"
    EST_DONE=$((EST_DONE + EST_CUR))
    case "$T_KIND" in
        score | explore | busy) ;;
        *)
            if [ "$EST_CUR" -gt 0 ]; then
                R_SUM=$((R_SUM + _secs))
                E_SUM=$((E_SUM + EST_CUR))
                FACTOR=$((R_SUM * 1000 / E_SUM))
                [ "$FACTOR" -lt 250 ] && FACTOR=250
                [ "$FACTOR" -gt 6000 ] && FACTOR=6000
            fi
            ;;
    esac
    CURRENT=""
}

# A test this release can't run: its record says why.
test_skip() { # INDEX REASON
    FRAG=$((FRAG + 1))
    N_SKIP=$((N_SKIP + 1))
    _sn=$(tget "$1" name)
    printf '    {\n      "id": %s,\n      "name": %s,\n      "kind": %s,\n      "result": "skip",\n      "summary": %s\n    }' \
        "$(js "$(tget "$1" id)")" "$(js "$_sn")" "$(js "$(tget "$1" kind)")" "$(js "$2")" >"$W/out/$(printf '%03d' "$FRAG").json"
    printf '[skip] %s: %s\n' "$_sn" "$2" >>"$W/summary.txt"
}

# targs INDEX RUNDIR: the test's arguments, one a line, its placeholders filled in.
targs() {
    jarr "$TESTS" ".tests[$1].args" | while IFS= read -r _l; do
        case "$_l" in
            *"{work}"*) _l="${_l%%"{work}"*}$2${_l#*"{work}"}" ;;
        esac
        case "$_l" in
            "{scene:"*"}") _l=${_l#"{scene:"}; _l="$SCENES/${_l%"}"}.json" ;;
            "{minutes}") _l=$X_MINUTES ;;
            "{max_rss_gb}") _l=$X_RSS ;;
        esac
        printf '%s\n' "$_l"
    done
}
# The command as the report and the screen show it: no folders.
show_cmd() { # ARGFILE [MORE]
    _sc=$(awk -v w="$W" '{ a = $0; if (index(a, w) == 1) { n = split(a, p, "/"); a = p[n] }; printf " %s", a }' "$1")
    printf 'jetcarp%s%s' "$_sc" "${2:+ $2}"
}

# The GPUs a test runs on: lines "flag <tab> label <tab> JETCARP_GPU <tab> WGPU_BACKEND".
modes() { # CPU_TOO ADAPTERS
    [ "$1" = true ] && printf 'cpu\tthe processor\t\t\n'
    case "$2" in
        chosen) _role=chosen ;;
        devices) _role=device ;;
        others) _role=other ;;
        *) return 0 ;;
    esac
    [ -s "$W/adapters.tsv" ] || {
        printf 'gpu\tthe GPU JETCARP picks\t\t\n'
        return 0
    }
    awk -F '\t' -v r="$_role" '$1 == r && $2 == 1 { printf "gpu\t%s\t%s\t%s\n", $3, $4, $5 }' "$W/adapters.tsv"
}

# ---- The tests ------------------------------------------------------------------------------

t_doctor() {
    targs "$TI" "$CUR" >"$CUR/args"
    T_CMD=$(show_cmd "$CUR/args")
    dim "Running: $T_CMD"
    jgo bg "$CUR/doctor.json" "$CUR/err" "$CUR/args"
    jwait none "" 300
    if [ "$JRC" -ne 0 ] || ! json_ok "$CUR/doctor.json"; then
        problem "jetcarp doctor failed (exit status $JRC)" "$CUR/err"
        test_end fail "jetcarp doctor did not work here"
        return
    fi
    jflat "$CUR/doctor.json" "$W/doctor.flat"
    _f="$W/doctor.flat"
    THREADS=$(jget "$_f" .threads)
    CPU=$(jget "$_f" .machine.hardware.cpu)
    CORES=$(jget "$_f" .machine.hardware.cores)
    MEM=$(jget "$_f" .machine.hardware.memory_total)
    # Older releases don't say: the system does.
    if [ -z "$CPU" ]; then
        case "$OSN" in
            linux) CPU=$(sed -n 's/^model name[^:]*: //p' /proc/cpuinfo 2>/dev/null | head -n 1) ;;
            macos) CPU=$(sysctl -n machdep.cpu.brand_string 2>/dev/null) ;;
        esac
    fi
    if [ -z "$MEM" ]; then
        case "$OSN" in
            linux) MEM=$(awk '/^MemTotal:/ { printf "%.0f", $2 * 1024 }' /proc/meminfo 2>/dev/null) ;;
            macos) MEM=$(sysctl -n hw.memsize 2>/dev/null) ;;
        esac
    fi
    awkc "BEGIN { FS = \"\\t\" } $AWK_UNQ"'
        $1 ~ /^\.gpu\[[0-9]+\]\./ { i = $1; sub(/^\.gpu\[/, "", i); sub(/\].*/, "", i); f = $1; sub(/^\.gpu\[[0-9]+\]\./, "", f); v[i, f] = unq($2); if (i + 1 > n) n = i + 1 }
        function dflt(s, d) { return s == "" ? d : s }
        END { for (i = 0; i < n; i++) printf "%s\t%s\t%s\t%s\t%s\n", dflt(v[i, "name"], "(no name)"), dflt(v[i, "backend"], "?"), dflt(v[i, "kind"], "?"), dflt(v[i, "driver"], "no driver named"), v[i, "problem"] }' "$_f" >"$W/gpus.tsv"
    info "JETCARP $(jget "$_f" .version) on $SYSTEM, $ARCH"
    info "Processor: ${CPU:-not said}${CORES:+, $CORES cores}${THREADS:+, $THREADS threads}; memory: $(human "${MEM:-0}")"
    _ng=0
    while IFS="$TAB" read -r _gn _gb _gk _gd _gp; do
        _ng=$((_ng + 1))
        if [ -n "$_gp" ]; then
            dim "Graphics: $_gn ($_gb, $_gk, $_gd): $_gp"
        else
            info "Graphics: $_gn ($_gb, $_gk, $_gd)"
        fi
    done <"$W/gpus.tsv"
    [ "$_ng" -gt 0 ] || info "Graphics: no adapter found"
    N_DEVICES=$(awk -F '\t' '$5 == "" && $3 != "Cpu" && !s[$1]++ { n++ } END { print n + 0 }' "$W/gpus.tsv")
    _bad=$(awk -F '\t' '$1 ~ /self_test\.passed$/ && $2 == "false" { n++ } END { print n + 0 }' "$_f")
    num threads "${THREADS:-null}"
    num memory_bytes "${MEM:-null}"
    num adapters "$_ng"
    detfile doctor "$CUR/doctor.json"
    _sum="$SYSTEM; ${CPU:-a processor}; $(human "${MEM:-0}"); $_ng graphics adapter(s)"
    if [ "$_bad" -gt 0 ]; then
        _note=$(awkc "BEGIN { FS = \"\\t\" } $AWK_UNQ"' FNR == NR { if ($1 ~ /self_test\.passed$/ && $2 == "false") { p = $1; sub(/passed$/, "note", p); bad[p] = 1 }; next } ($1 in bad) && !said[$2]++ { printf "%s%s", (n++ ? "; " : ""), unq($2) }' "$_f" "$_f")
        problem "the GPU self-test failed: $_note"
        warn "The GPU self-test failed: $_note"
        test_end warn "$_sum; the GPU self-test failed"
    else
        test_end pass "$_sum"
    fi
}

# Which adapters JETCARP runs on: one short run on each, to see.
t_adapters() {
    T_CMD="jetcarp run <scene> --gens 1 --gpu --json, once for each graphics adapter"
    dim "Running: $T_CMD"
    : >"$W/adapters.tsv"
    : >"$CUR/used"
    _probe_n=0
    _probe_total=$(($(awk 'END { print NR }' "$W/gpus.tsv" 2>/dev/null) + 1))
    probe "" "" chosen "the GPU JETCARP picks"
    awk -F '\t' '$5 == "" && $3 != "Cpu" && !s[$1]++ { print $1 }' "$W/gpus.tsv" >"$CUR/devices"
    while IFS= read -r _dn; do probe "$_dn" "" device "$_dn"; done <"$CUR/devices"
    while IFS="$TAB" read -r _gn _gb _gk _gd _gp; do
        _api=$(printf '%s' "$_gb" | tr '[:upper:]' '[:lower:]')
        if grep -F -x -q "$_gn$TAB$_api" "$CUR/used"; then continue; fi
        printf '%s\t%s\n' "$_gn" "$_api" >>"$CUR/used"
        probe "$_gn" "$_api" other "$_gn, $_gb"
    done <"$W/gpus.tsv"
    N_DEVICES=$(awk -F '\t' '$1 == "device" && $2 == 1 { n++ } END { print n + 0 }' "$W/adapters.tsv")
    N_OTHERS=$(awk -F '\t' '$1 == "other" && $2 == 1 { n++ } END { print n + 0 }' "$W/adapters.tsv")
    awkc "$AWK_JS"'BEGIN { FS = "\t"; printf "[" } { printf "%s\n          {\"role\": %s, \"used\": %s, \"label\": %s, \"gpu\": %s, \"api\": %s, \"note\": %s}", (NR > 1 ? "," : ""), js($1), ($2 == 1 ? "true" : "false"), js($3), js($4), js($5), js($6) } END { printf "\n        ]" }' "$W/adapters.tsv" >"$CUR/adapters.json"
    detfile adapters "$CUR/adapters.json"
    num gpus_used "$N_DEVICES"
    num other_apis_used "$N_OTHERS"
    _chosen=$(awk -F '\t' '$1 == "chosen" { print ($2 == 1 ? $3 : "none: " $6) }' "$W/adapters.tsv")
    if [ "$N_DEVICES" -eq 0 ]; then
        test_end warn "no GPU JETCARP can use here: the tests run on the processor ($_chosen)"
    else
        test_end pass "JETCARP runs on $N_DEVICES GPU(s), $N_OTHERS more through other APIs; it picks $_chosen"
    fi
}
probe() { # JETCARP_GPU WGPU_BACKEND ROLE LABEL
    GPU_NAME=$1
    GPU_API=$2
    _probe_n=$((_probe_n + 1))
    tick $((_probe_n * 1000 / _probe_total)) "$4"
    jgo fg "$CUR/o" "$CUR/e" /dev/null run "$SCENES/0.json" --gens 1 --gpu --json
    jflat "$CUR/o" "$CUR/o.flat" ".backend"
    _b=$(jget "$CUR/o.flat" .backend)
    GPU_NAME=""
    GPU_API=""
    case "$_b" in
        gpu*)
            _lab=$(printf '%s' "$_b" | awk '{ s = $0; sub(/;.*/, "", s); sub(/^gpu \(/, "", s); sub(/\)$/, "", s); print s }')
            _used_api=$(printf '%s' "$_lab" | awk '{ s = $0; sub(/.*, /, "", s); print tolower(s) }')
            _note=$(printf '%s' "$_b" | sed -n 's/^[^;]*; //p')
            # Asked for an adapter it won't use (WARP, say), JETCARP falls back to another:
            # that adapter isn't used, and the one it ran on is tested in its own place (#371).
            _other=""
            if [ -n "$1" ]; then
                case "$_lab" in "$1"*) ;; *) _other=1 ;; esac
            fi
            [ -n "$2" ] && [ "$_used_api" != "$2" ] && _other=1
            if [ -n "$_other" ]; then
                printf '%s\t0\t%s\t%s\t%s\t%s\n' "$3" "$4" "$1" "$2" "JETCARP ran it on $_lab instead" >>"$W/adapters.tsv"
                dim "Not used: $4: JETCARP ran it on $_lab instead"
                return 0
            fi
            [ "$3" = device ] && printf '%s\t%s\n' "$1" "$_used_api" >>"$CUR/used"
            printf '%s\t1\t%s\t%s\t%s\t%s\n' "$3" "$_lab" "$1" "$2" "$_note" >>"$W/adapters.tsv"
            case "$3" in
                chosen) info "JETCARP picks: $_lab" ;;
                *) info "Runs on: $_lab" ;;
            esac
            ;;
        *)
            _why=$(printf '%s' "$_b" | sed -n 's/.*no GPU: //p')
            [ -n "$_why" ] || _why=$(tail -n 1 "$CUR/e" 2>/dev/null)
            [ -n "$_why" ] || _why="it ran on the processor"
            printf '%s\t0\t%s\t%s\t%s\t%s\n' "$3" "$4" "$1" "$2" "$_why" >>"$W/adapters.tsv"
            dim "Not used: $4: $_why"
            ;;
    esac
}
# Reads one field of a run's JSON output (jget on raw output: flattened first).
run_field() { # FILE PATHS...: "path <tab> value" lines
    _rf=$1
    shift
    awkc -v only="$*" "$AWK_FLAT" "$_rf"
}

t_busy() {
    T_CMD="(this script: the list of programs, the load on the processor, the power supply)"
    while :; do
        busy_look
        [ -z "$BUSY_PROGS" ] && [ -z "$BUSY_LOAD" ] && [ -z "$ON_BATTERY" ] && break
        [ -n "$BUSY_PROGS" ] && warn "JETCARP is already running here ($BUSY_PROGS): close the app, and stop the background host (jetcarp host stop), for numbers that compare."
        [ -n "$BUSY_LOAD" ] && warn "The processor is busy (load $LOAD1 on $NPROC threads): close games, video, builds and other heavy programs."
        [ -n "$ON_BATTERY" ] && warn "This laptop is on battery: plug it in, if you can, for numbers that compare."
        [ -n "$ASK" ] || break
        ask "Press Enter to look again, or type g to go on as it is:" ""
        case "$ANSWER" in g | G | go | Go) break ;; esac
        [ -n "$ASK" ] || break
    done
    num load_1min "${LOAD1:-null}"
    num threads "${NPROC:-null}"
    num on_battery "$([ -n "$ON_BATTERY" ] && echo true || echo false)"
    numstr other_jetcarp "$BUSY_PROGS"
    if [ -z "$BUSY_PROGS" ] && [ -z "$BUSY_LOAD" ] && [ -z "$ON_BATTERY" ]; then
        test_end pass "nothing found: load $LOAD1 on $NPROC threads"
    else
        _w=""
        [ -n "$BUSY_PROGS" ] && _w="JETCARP already running ($BUSY_PROGS)"
        [ -n "$BUSY_LOAD" ] && _w="${_w:+$_w; }load $LOAD1 on $NPROC threads"
        [ -n "$ON_BATTERY" ] && _w="${_w:+$_w; }on battery"
        test_end warn "measured anyway: $_w"
    fi
}
busy_look() {
    BUSY_PROGS=$(ps -A -o pid= -o comm= 2>/dev/null | awk '{ p = $1; sub(/^ *[0-9]+ +/, ""); n = split($0, a, "/"); c = a[n]; if (c == "jetcarp" || c == "jetcarpw") { k++; l = l (k > 1 ? ", " : "") p } } END { if (k) printf "%d program%s: process%s %s", k, (k > 1 ? "s" : ""), (k > 1 ? "es" : ""), l }')
    NPROC=${THREADS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)}
    case "$OSN" in
        linux) LOAD1=$(awk '{ print $1 }' /proc/loadavg 2>/dev/null) ;;
        macos) LOAD1=$(sysctl -n vm.loadavg 2>/dev/null | awk '{ print $2 }') ;;
    esac
    BUSY_LOAD=""
    if [ -n "${LOAD1:-}" ] && awk -v l="$LOAD1" -v n="$NPROC" 'BEGIN { m = n * 0.3; if (m < 1.5) m = 1.5; exit !(l > m) }'; then
        BUSY_LOAD=1
    fi
    ON_BATTERY=""
    case "$OSN" in
        linux)
            _ac=""
            _bat=""
            for _p in /sys/class/power_supply/*; do
                [ -r "$_p/type" ] || continue
                case "$(cat "$_p/type" 2>/dev/null)" in
                    Mains) if [ "$(cat "$_p/online" 2>/dev/null)" = 1 ]; then _ac=1; else _ac=${_ac:-0}; fi ;;
                    Battery) _bat=1 ;;
                esac
            done
            [ "$_bat" = 1 ] && [ "$_ac" = 0 ] && ON_BATTERY=1
            ;;
        macos) pmset -g batt 2>/dev/null | head -n 1 | grep -q "Battery Power" && ON_BATTERY=1 ;;
    esac
}

t_score() {
    targs "$TI" "$CUR" >"$CUR/args"
    T_CMD=$(show_cmd "$CUR/args")
    _rep=$(tget "$TI" report)
    _rep="${_rep%%"{work}"*}$CUR${_rep#*"{work}"}"
    dim "Running: $T_CMD"
    if [ -n "$ASK" ] && [ "$(tget "$TI" attach)" = true ]; then
        dim "It says what it measures as it goes; if something would lower the score, it says so and asks first."
        status_clear
        jgo tty "" "" "$CUR/args"
    else
        jgo bg "$CUR/out" "$CUR/err" "$CUR/args"
        jwait time 70 600
    fi
    if [ "$JRC" -ne 0 ] || ! json_ok "$_rep"; then
        if grep -q "not measured" "$CUR/err" 2>/dev/null; then
            test_end warn "not measured: stopped at its question"
            return
        fi
        problem "jetcarp score failed (exit status $JRC)" "$CUR/err"
        test_end fail "jetcarp score did not work here"
        return
    fi
    jflat "$_rep" "$CUR/score.flat"
    _sc=$(jget "$CUR/score.flat" .score.score)
    _load=$(jget "$CUR/score.flat" .score.conditions.under_load)
    _wrong=$(awk -F '\t' '$1 ~ /^\.score\.parts\[[0-9]+\]\.correct$/ && $2 == "false" { n++ } END { print n + 0 }' "$CUR/score.flat")
    num score "${_sc:-null}"
    num under_load "${_load:-null}"
    num wrong_answers "$_wrong"
    num seconds "$(jget "$CUR/score.flat" .score.seconds || echo null)"
    detfile report "$_rep"
    _scs=$(commas "${_sc:-0}")
    if [ "$_wrong" -gt 0 ]; then
        problem "$_wrong of the score's workloads gave a wrong answer" "$CUR/err"
        test_end fail "$_scs, but $_wrong workload(s) gave a wrong answer"
    elif [ "$_load" = true ]; then
        test_end warn "$_scs, taken under load"
    else
        test_end pass "$_scs, taken with the computer quiet"
    fi
}

t_parity() {
    _set=$(tget "$TI" set)
    _gens=$(tget "$TI" gens)
    if [ "$_set" = long ]; then _exp=$PAR_LONG; else _exp=$PAR_SHORT; fi
    # The scenes, with each one's expected hash (empty with no set for this release).
    if [ "$_set" = long ]; then
        _ids=${PAR_LONG:-$PAR_LONG_ANY}
    else
        _ids=$SCENES_TSV
    fi
    awk -F '\t' -v spec="$(tget "$TI" pick)" '
        BEGIN { k = split(spec, it, ","); for (i = 1; i <= k; i++) { s = it[i]; gsub(/^ +| +$/, "", s); if (s == "all") ty[i] = "all"; else if (s ~ /^every /) { ty[i] = "every"; a[i] = substr(s, 7) + 0 } else if (s ~ /^[0-9]+-[0-9]+$/) { ty[i] = "range"; split(s, q, "-"); a[i] = q[1] + 0; b[i] = q[2] + 0 } else { ty[i] = "one"; a[i] = s + 0 } } }
        function sel(n,   i) { for (i = 1; i <= k; i++) { if (ty[i] == "all") return 1; if (ty[i] == "every" && a[i] > 0 && n % a[i] == 0) return 1; if (ty[i] == "range" && n >= a[i] && n <= b[i]) return 1; if (ty[i] == "one" && n == a[i]) return 1 } return 0 }
        sel($1 + 0) { print $1 "\t" $2 }' "$_ids" >"$CUR/picked"
    if [ -n "$_exp" ]; then
        awk -F '\t' 'FNR == NR { h[$1] = $3; next } { print $1 "\t" $2 "\t" h[$1] }' "$_exp" "$CUR/picked" >"$CUR/list"
    else
        awk -F '\t' '{ print $1 "\t" $2 "\t" }' "$CUR/picked" >"$CUR/list"
    fi
    modes "$(tget "$TI" cpu)" "$(tget "$TI" adapters)" >"$CUR/modes"
    _nsc=$(awk 'END { print NR }' "$CUR/list")
    _nmo=$(awk 'END { print NR }' "$CUR/modes")
    T_CMD="jetcarp run <scene> --gens $_gens --json, with --cpu on the processor and --gpu on each GPU"
    if [ "$_nmo" -eq 0 ]; then
        test_end skip "no adapters of this kind here"
        N_SKIP=$((N_SKIP + 1))
        return
    fi
    dim "Running: $T_CMD"
    [ -n "$_exp" ] || warn "JETCARP $V's expected hashes aren't checked yet: the GPUs are compared with the processor instead."
    _nogpu=""
    if grep -q '^cpu' "$CUR/modes" && ! grep -q '^gpu' "$CUR/modes"; then
        _nogpu=1
        warn "No GPU here that JETCARP uses: the processor alone."
    fi
    _total=$((_nsc * _nmo))
    _done=0
    _mi=0
    _all_ok=1
    _fellback=""
    : >"$CUR/modes.json"
    _first_flag=$(awk -F '\t' 'NR == 1 { print $1 }' "$CUR/modes")
    while IFS="$TAB" read -r _flag _lab _gname _gapi; do
        _mi=$((_mi + 1))
        GPU_NAME=$_gname
        GPU_API=$_gapi
        : >"$CUR/mode-$_mi.tsv"
        while IFS="$TAB" read -r _n _id _h; do
            tick $((_done * 1000 / _total)) "$_done of $_total runs, $_lab"
            jgo fg "$CUR/o" "$CUR/e" /dev/null run "$SCENES/$_n.json" --gens "$_gens" --json "--$_flag"
            _hash=ERROR
            _secs=0
            _back=""
            if [ "$JRC" -eq 0 ]; then
                while IFS="$TAB" read -r _k _v; do
                    case "$_k" in
                        .hash) _hash=$(printf '%s' "$_v" | tr -d '"') ;;
                        .seconds) _secs=$_v ;;
                        .backend) _back=$(printf '%s' "$_v" | sed 's/^"//; s/"$//; s/\\"/"/g') ;;
                    esac
                done <<EOF
$(run_field "$CUR/o" .hash .seconds .backend)
EOF
            fi
            if [ "$_hash" = ERROR ] && [ "$(awk 'END { print NR }' "$CUR/problems")" -lt 5 ]; then
                problem "scene $_n ($_id) failed on $_lab (exit status $JRC)" "$CUR/e"
            fi
            printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$_n" "$_id" "$_h" "$_hash" "$_secs" "$_back" >>"$CUR/mode-$_mi.tsv"
            _done=$((_done + 1))
        done <"$CUR/list"
        GPU_NAME=""
        GPU_API=""
        _cpuf=""
        [ "$_flag" = gpu ] && [ -s "$CUR/mode-1.tsv" ] && [ "$_first_flag" = cpu ] && _cpuf="$CUR/mode-1.tsv"
        [ -s "$CUR/modes.json" ] && printf ',\n' >>"$CUR/modes.json"
        _st=$(awkc -F '\t' -v cpuf="$_cpuf" -v flag="$_flag" -v gens="$_gens" -v lab="$_lab" -v gname="$_gname" -v gapi="$_gapi" -v jf="$CUR/modes.json" "$AWK_JS"'
            FILENAME == cpuf { cpu[$1] = $4; next }
            {
                of++; secs += $5; g = 0
                if ($4 == "ERROR") err++
                if (flag == "gpu") {
                    if (match($6, /[0-9]+ of [0-9]+ generations on the cpu/)) { split(substr($6, RSTART, RLENGTH), q, " "); g = q[3] - q[1] }
                    else if ($6 ~ /^gpu/) g = gens
                    if (g == gens) fully++
                    if (g > 0) some++
                }
                if (first == "") first = $6
                bad = 0
                if ($3 != "") { en++; if ($4 == $3) ok++; else bad = 1 }
                if (cpuf != "" && ($1 in cpu)) { cn++; if (cpu[$1] == $4) agree++; else if ($3 == "") bad = 1 }
                if (bad && nb < 50) mis[++nb] = sprintf("{\"n\": %d, \"id\": %s, \"got\": %s, \"expected\": %s, \"processor\": %s}", $1, js($2), js($4), js($3), js(cpu[$1]))
                runs[of] = sprintf("[%d, %s, %s, %d]", $1, js($4), ($5 == "" ? 0 : $5), g)
            }
            END {
                printf "        {\"device\": %s, \"flag\": \"--%s\", \"gpu\": %s, \"api\": %s, \"count\": %d, \"expected\": %d, \"as_expected\": %d, \"compared_with_processor\": %d, \"agree_with_processor\": %d, \"all_on_gpu\": %d, \"errors\": %d, \"seconds\": %.3f, \"backend\": %s,\n          \"mismatches\": [", js(lab), flag, js(gname), js(gapi), of, en, ok, cn, agree, fully, err, secs, js(first) >> jf
                for (i = 1; i <= nb; i++) printf "%s%s", (i > 1 ? ", " : ""), mis[i] >> jf
                printf "],\n          \"runs\": [" >> jf
                for (i = 1; i <= of; i++) printf "%s\n            %s", (i > 1 ? "," : ""), runs[i] >> jf
                printf "\n          ]}" >> jf
                print of + 0, en + 0, ok + 0, cn + 0, agree + 0, fully + 0, err + 0, some + 0, nb + 0, secs
            }' ${_cpuf:+"$_cpuf"} "$CUR/mode-$_mi.tsv")
        # shellcheck disable=SC2086 # the counts, split on purpose
        set -- $_st
        _of=$1 _en=$2 _ok=$3 _cn=$4 _ag=$5 _fully=$6 _err=$7 _some=$8 _nbad=$9
        _t=$(dur "${10}")
        if [ "$_en" -gt 0 ]; then
            _line="$_ok of $_en hashes as expected"
        elif [ "$_cn" -gt 0 ]; then
            _line="$_ag of $_cn agree with the processor"
        else
            _line="$_of runs"
        fi
        [ "$_flag" = gpu ] && _line="$_line; $_fully of $_of all on the GPU"
        if [ "$_nbad" -gt 0 ] || [ "$_err" -gt 0 ]; then
            _all_ok=""
            printf '    %s %s: %s%s%s %s(%s)%s\n' "$(tag fail)" "$_lab" "$RED" "$_line" "$R" "$D" "$_t" "$R"
            [ "$_nbad" -gt 0 ] && problem "$_nbad scene(s) on $_lab did not reach the hash they should (the first are under details.modes)"
        else
            status_clear
            printf '    %s %s: %s %s(%s)%s\n' "$(tag pass)" "$_lab" "$_line" "$D" "$_t" "$R"
        fi
        if [ "$_flag" = gpu ] && [ "$_some" -eq 0 ]; then
            _fellback="${_fellback:+$_fellback, }$_lab"
        fi
        printf '%s\t%s\t%s\t%s\n' "$_lab" "$_ok" "$_en" "$_of" >>"$CUR/tally"
    done <"$CUR/modes"
    {
        printf '[\n'
        cat "$CUR/modes.json"
        printf '\n        ]'
    } >"$CUR/modes.all.json"
    detfile modes "$CUR/modes.all.json"
    det expected_set "$(js "${PAR_NOTE:-none for this release}")"
    det generations "$_gens"
    det scenes "$_nsc"
    _ok_all=$(awk -F '\t' '{ a += $2; b += ($3 > 0 ? $3 : $4) } END { print a + 0, b + 0 }' "$CUR/tally")
    num runs "$_total"
    num as_expected "${_ok_all% *}"
    _on="the processor"
    _ngm=$(grep -c '^gpu' "$CUR/modes")
    [ "$_ngm" -gt 0 ] && _on="$_on and $_ngm GPU(s)"
    grep -q '^cpu' "$CUR/modes" || _on="$_ngm adapter(s)"
    _sum="$_nsc scenes x $(commas "$_gens") generations on $_on: ${_ok_all% *} of ${_ok_all#* } hashes as they should be"
    [ -n "$_exp" ] || _sum="$_nsc scenes x $(commas "$_gens") generations on $_on; the GPUs held to the processor's hashes (this release's expected ones aren't checked yet)"
    [ -n "$_nogpu" ] && _sum="$_sum (no GPU to compare)"
    if [ -z "$_all_ok" ]; then
        test_end fail "$_sum"
    elif [ -n "$_fellback" ]; then
        problem "every run on $_fellback fell back to the processor"
        test_end warn "$_sum; $_fellback ran on the processor"
    else
        test_end pass "$_sum"
    fi
}

t_check() {
    _ex=$(pick_range "$TESTS" ".tests[$TI].expect")
    : >"$CUR/expect"
    [ -n "$_ex" ] && jsub "$TESTS" ".tests[$TI].expect[$_ex].values." >"$CUR/expect"
    jarr "$TESTS" ".tests[$TI].same" >"$CUR/same"
    _word=$(tget "$TI" progress)
    _say=$(tget "$TI" say)
    # One run per device: none (as written), or --cpu and --gpu on each GPU.
    if [ "$(jlen "$TESTS" ".tests[$TI].devices")" -gt 0 ]; then
        _cpu=false
        jarr "$TESTS" ".tests[$TI].devices" | grep -q -x cpu && _cpu=true
        if jarr "$TESTS" ".tests[$TI].devices" | grep -q -x gpu; then
            modes "$_cpu" "$(tget "$TI" adapters)" >"$CUR/modes"
        else
            modes "$_cpu" none >"$CUR/modes"
        fi
    else
        printf 'none\tas written\t\t\n' >"$CUR/modes"
    fi
    targs "$TI" "<temp>" >"$CUR/args.shown"
    T_CMD=$(show_cmd "$CUR/args.shown")
    grep -q '^cpu' "$CUR/modes" && T_CMD="$T_CMD, with --cpu on the processor and --gpu on each GPU"
    dim "Running: $T_CMD"
    [ -z "$_ex" ] && [ "$(jlen "$TESTS" ".tests[$TI].expect")" -gt 0 ] && warn "No known results for JETCARP $V: the runs are compared with each other."
    _ri=0
    _bad=""
    _errs=""
    : >"$CUR/runs.json"
    while IFS="$TAB" read -r _flag _lab _gname _gapi; do
        _ri=$((_ri + 1))
        _rd="$CUR/run-$_ri"
        mkdir -p "$_rd"
        targs "$TI" "$_rd" >"$_rd/args"
        GPU_NAME=$_gname
        GPU_API=$_gapi
        if [ "$_flag" = none ]; then
            jgo bg "$_rd/out.json" "$_rd/err" "$_rd/args"
        else
            jgo bg "$_rd/out.json" "$_rd/err" "$_rd/args" "--$_flag"
        fi
        if [ -n "$_word" ]; then jwait count "$_word" 1800; else jwait none "" 1800; fi
        GPU_NAME=""
        GPU_API=""
        [ -s "$CUR/runs.json" ] && printf ',\n' >>"$CUR/runs.json"
        if [ "$JRC" -ne 0 ] || ! json_ok "$_rd/out.json"; then
            _errs=1
            problem "the run on $_lab failed (exit status $JRC${JTIMEDOUT:+, stopped: it ran too long})" "$_rd/err"
            printf '    %s %s: exit status %s\n' "$(tag fail)" "$_lab" "$JRC"
            printf '          {"device": %s, "exit": %s}' "$(js "$_lab")" "$JRC" >>"$CUR/runs.json"
            continue
        fi
        jflat "$_rd/out.json" "$_rd/flat"
        # Known results, then the same as the first run.
        _mm=""
        if [ -s "$CUR/expect" ]; then
            _mm=$(awkc -F '\t' 'FNR == NR { want["." $1] = $2; next } ($1 in want) { got[$1] = $2 } END { for (p in want) if (got[p] != want[p]) printf "%s is %s, not %s; ", substr(p, 2), (p in got ? got[p] : "missing"), want[p] }' "$CUR/expect" "$_rd/flat")
        fi
        : >"$_rd/same"
        if [ -s "$CUR/same" ]; then
            awk -F '\t' 'FNR == NR { s["." $1] = 1; next } { for (p in s) if ($1 == p || index($1, p ".") == 1 || index($1, p "[") == 1) { print; break } }' "$CUR/same" "$_rd/flat" | sort >"$_rd/same"
        fi
        if [ "$_ri" -gt 1 ] && [ -s "$CUR/run-1/same" ] && ! cmp -s "$CUR/run-1/same" "$_rd/same"; then
            _mm="${_mm}its results differ from the first run's ($(diff "$CUR/run-1/same" "$_rd/same" | awk '/^>/ { sub(/^> \./, ""); printf "%s%s", (n++ ? "; " : ""), $0 }' | tr '\t' ' ' | cut -c1-300)); "
        fi
        _shown=""
        [ -n "$_say" ] && _shown=$(jget "$_rd/flat" ".$_say")
        {
            printf '          {"device": %s, "exit": 0, "output": ' "$(js "$_lab")"
            cat "$_rd/out.json"
            printf '}'
        } >>"$CUR/runs.json"
        if [ -n "$_mm" ]; then
            _bad=1
            problem "on $_lab: ${_mm%; }"
            status_clear
            printf '    %s %s: %s%s%s\n' "$(tag fail)" "$_lab" "$RED" "${_mm%; }" "$R"
        else
            status_clear
            _shown=$(printf '%s' "$_shown" | sed 's/^[a-z]* [0-9]*: //; s/ | .*//' | cut -c1-120)
            [ "$_flag" = none ] && _lab=$_say
            printf '    %s %s: %s\n' "$(tag pass)" "$_lab" "$_shown"
        fi
    done <"$CUR/modes"
    {
        printf '[\n'
        cat "$CUR/runs.json"
        printf '\n        ]'
    } >"$CUR/runs.all.json"
    detfile runs "$CUR/runs.all.json"
    num runs "$_ri"
    if [ -n "$_ex" ]; then det known_results_from "$(js "$(jget "$TESTS" ".tests[$TI].expect[$_ex].from")")"; fi
    if [ -n "$_errs" ]; then
        test_end fail "a run failed"
    elif [ -n "$_bad" ]; then
        test_end fail "the results are not the known ones"
    elif [ -n "$_ex" ]; then
        test_end pass "$_ri run(s), every result the known one"
    else
        test_end pass "$_ri run(s), the same results each time (no known results for this release)"
    fi
}

t_fit() {
    targs "$TI" "$CUR" >"$CUR/args"
    T_CMD=$(show_cmd "$CUR/args")
    dim "Running: $T_CMD"
    [ "$OSN" = linux ] && dim "With no display open to it (DISPLAY and WAYLAND_DISPLAY are unset for every run), it can't show anything."
    jgo bg "$CUR/events" "$CUR/err" "$CUR/args"
    jwait fit "" 900
    awk 'BEGIN { printf "[" } /^[ \t]*\{/ { printf "%s\n          %s", (n++ ? "," : ""), $0 } END { printf "\n        ]" }' "$CUR/events" >"$CUR/events.json"
    detfile events "$CUR/events.json"
    grep '"event":"done"' "$CUR/events" | tail -n 1 >"$CUR/done"
    _fail=$(grep '"event":"failed"' "$CUR/events" | tail -n 1)
    _meas=$(grep -c '"event":"measured"' "$CUR/events")
    if [ "$JRC" -ne 0 ] || [ ! -s "$CUR/done" ]; then
        problem "the fit failed (exit status $JRC)${_fail:+: $_fail}" "$CUR/err"
        test_end fail "Fit to this computer did not finish"
        return
    fi
    jflat "$CUR/done" "$CUR/done.flat"
    _s=$(jget "$CUR/done.flat" .seconds)
    _n=$(jget "$CUR/done.flat" .scenes)
    num seconds "${_s:-null}"
    num sizes_measured "$_meas"
    num scenes "${_n:-null}"
    _fps=$(grep '"event":"measured"' "$CUR/events" | awk '{ if (match($0, /"fps":[0-9.]+/)) { f = sprintf("%.0f", substr($0, RSTART + 6, RLENGTH - 6)); if (!s[f]++) l = l (n++ ? ", " : "") f } } END { printf "%s", l }')
    test_end pass "fitted ${_n:-its} scenes in $(dur "${_s:-0}"), $_meas size(s) measured${_fps:+ at $_fps frames a second}"
}

t_speed() {
    modes true "$(tget "$TI" adapters)" >"$CUR/modes"
    _ns=$(jlen "$TESTS" ".tests[$TI].scenes")
    _nm=$(awk 'END { print NR }' "$CUR/modes")
    _total=$((_ns * _nm))
    _done=0
    _bad=""
    T_CMD="jetcarp run <scene> --gens N --json, with --cpu on the processor and --gpu on each GPU"
    dim "Running: $T_CMD"
    : >"$CUR/speed.tsv"
    _j=0
    while [ "$_j" -lt "$_ns" ]; do
        _name=$(tget "$TI" "scenes[$_j].name")
        _file="$DATA/$(tget "$TI" "scenes[$_j].scene")"
        _g=$(tget "$TI" "scenes[$_j].gens")
        _line=""
        _first=""
        while IFS="$TAB" read -r _flag _lab _gname _gapi; do
            GPU_NAME=$_gname
            GPU_API=$_gapi
            T_NAME_SAVED=$T_NAME
            jgo bg "$CUR/o" "$CUR/e" /dev/null run "$_file" --gens "$_g" --json "--$_flag"
            T_NAME="$T_NAME_SAVED: $_name, $_lab"
            jwait none "" 900
            T_NAME=$T_NAME_SAVED
            GPU_NAME=""
            GPU_API=""
            _done=$((_done + 1))
            if [ "$JRC" -ne 0 ]; then
                problem "$_name on $_lab failed (exit status $JRC)" "$CUR/e"
                _bad=1
                continue
            fi
            jflat "$CUR/o" "$CUR/o.flat" ".hash .seconds .cells .backend"
            _h=$(jget "$CUR/o.flat" .hash)
            _s=$(jget "$CUR/o.flat" .seconds)
            _c=$(jget "$CUR/o.flat" .cells)
            _b=$(jget "$CUR/o.flat" .backend)
            _rate=$(awk -v g="$_g" -v s="$_s" 'BEGIN { if (s > 0) printf "%.1f", g / s; else print 0 }')
            printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$_name" "$_lab" "$_flag" "$_g" "$_s" "$_c" "$_h" "$_rate" "$_b" >>"$CUR/speed.tsv"
            _line="${_line:+$_line; }$_lab $(commas "$_rate")/s"
            if [ -z "$_first" ]; then
                _first=$_h
            elif [ "$_h" != "$_first" ]; then
                _bad=1
                problem "$_name on $_lab reached a different hash from the processor's: $_h, not $_first"
            fi
        done <"$CUR/modes"
        info "$_name, generations a second: $_line"
        _j=$((_j + 1))
    done
    awkc "$AWK_JS"'BEGIN { FS = "\t"; printf "[" } { printf "%s\n          {\"scene\": %s, \"device\": %s, \"flag\": \"--%s\", \"gens\": %d, \"seconds\": %s, \"cells\": %s, \"gens_per_second\": %s, \"cell_gens_per_second\": %.0f, \"hash\": %s, \"backend\": %s}", (NR > 1 ? "," : ""), js($1), js($2), $3, $4, ($5 == "" ? "null" : $5), ($6 == "" ? "null" : $6), $8, ($5 > 0 ? $6 * $4 / $5 : 0), js($7), js($9) } END { printf "\n        ]" }' "$CUR/speed.tsv" >"$CUR/speed.json"
    detfile runs "$CUR/speed.json"
    _best=$(awk -F '\t' '$3 == "cpu" { printf "%s%s %s/s", (n++ ? ", " : ""), $1, $8 }' "$CUR/speed.tsv")
    if [ -n "$_bad" ]; then
        test_end fail "a run failed or reached another hash"
    else
        test_end pass "on the processor: $_best; the same hash on every device"
    fi
}

t_explore() {
    X_MINUTES=$(tget "$TI" minutes)
    _share=$(tget "$TI" max_rss_share)
    X_RSS=$(awk -v m="${MEM:-0}" -v s="${_share:-0.25}" 'BEGIN { g = m * s / 1073741824; if (g < 1) g = 1; printf "%.1f", g }')
    targs "$TI" "$CUR" >"$CUR/args"
    T_CMD=$(show_cmd "$CUR/args")
    dim "Running: $T_CMD"
    dim "It keeps every core but two busy for $X_MINUTES minutes, and stops itself past $X_RSS GB."
    : >"$CUR/rss"
    SAMPLE_RSS=1
    RSS_NOW=""
    jgo bg "$CUR/out.json" "$CUR/err" "$CUR/args"
    jwait runs $((X_MINUTES * 60)) $((X_MINUTES * 60 + 600))
    SAMPLE_RSS=""
    awk 'BEGIN { printf "[" } { printf "%s[%d, %.1f]", (n++ ? ", " : ""), $1, $2 / 1024 } END { printf "]" }' "$CUR/rss" >"$CUR/rss.json"
    det memory_mb_every_5_seconds "$(cat "$CUR/rss.json")"
    _mem=$(awk '{ v[++n] = $2; if ($2 > p) p = $2 } END { if (n < 8) { print p + 0, 0, 0; exit } for (i = int(n / 4) + 1; i <= int(n / 2); i++) { a += v[i]; na++ } for (i = int(3 * n / 4) + 1; i <= n; i++) { b += v[i]; nb++ } a /= na; b /= nb; print p + 0, a, b }' "$CUR/rss")
    # shellcheck disable=SC2086 # three numbers
    set -- $_mem
    _peak=$1 _q2=$2 _q4=$3
    num peak_mb "$(awk -v p="$_peak" 'BEGIN { printf "%.1f", p / 1024 }')"
    num cap_gb "$X_RSS"
    if [ "$JRC" -ne 0 ] || ! json_ok "$CUR/out.json"; then
        problem "the search failed (exit status $JRC${JTIMEDOUT:+, stopped: it ran far past its time})" "$CUR/err"
        test_end fail "the search did not finish"
        return
    fi
    detfile output "$CUR/out.json"
    jflat "$CUR/out.json" "$CUR/flat"
    _stopped=$(jget "$CUR/flat" .stopped)
    _runs=$(jget "$CUR/flat" .progress.screened)
    _per=$(jget "$CUR/flat" .progress.per_s)
    _panics=$(jget "$CUR/flat" .progress.panics)
    _err=$(jget "$CUR/flat" .progress.error)
    _gerr=$(jget "$CUR/flat" .progress.gpu_error)
    num runs "${_runs:-null}"
    num runs_per_second "${_per:-null}"
    _climb=$(awk -v a="$_q2" -v b="$_q4" 'BEGIN { if (a > 0 && b > a * 1.5 && b - a > 262144) print 1 }')
    _sum="$(commas "${_runs:-0}") runs, $(commas "${_per:-0}") a second; memory at most $(human $((_peak * 1024)))"
    if [ -n "$_err" ] || [ "${_panics:-0}" != 0 ]; then
        problem "the search reported a problem: ${_err:-$_panics panic(s)}" "$CUR/err"
        test_end fail "$_sum; it reported a problem"
    elif [ -n "$_stopped" ] && [ "$_stopped" != budget ]; then
        problem "the search stopped early: $_stopped" "$CUR/err"
        test_end fail "$_sum; it stopped early ($_stopped)"
    elif [ -n "$_climb" ]; then
        problem "its memory climbed: about $(human "$(awk -v a="$_q2" 'BEGIN { printf "%.0f", a * 1024 }')") in its second quarter, $(human "$(awk -v b="$_q4" 'BEGIN { printf "%.0f", b * 1024 }')") in its last"
        test_end warn "$_sum; its memory climbed"
    elif [ -n "$_gerr" ]; then
        problem "the GPU reported a problem: $_gerr"
        test_end warn "$_sum; the GPU reported a problem"
    else
        test_end pass "$_sum, steady"
    fi
}

# ---- The report ------------------------------------------------------------------------------

# Writes the report (finished, or stopped) to REPORT, with what identifies this computer taken
# out.
write_report() {
    _end=$(now)
    _result=pass
    [ "$N_WARN" -gt 0 ] && _result=warn
    [ "$N_FAIL" -gt 0 ] && _result=fail
    [ -n "$STOPPED" ] && _result=stopped
    _head="JETCARP $V$([ "$PRE" = true ] && echo ' (release candidate)') on $SYSTEM, $ARCH: $TIER_NAME, $(dur $((_end - RUN_START))), $(date '+%Y-%m-%d %H:%M %Z')"
    _counts="$N_PASS passed, $N_WARN with warnings, $N_FAIL failed, $N_SKIP skipped$([ -n "$STOPPED" ] && echo '; stopped before the end')"
    {
        printf '%s\n%s\n' "$_head" "$_counts"
        cat "$W/summary.txt" 2>/dev/null
    } >"$W/summary.all"
    _raw="$W/report.raw.json"
    {
        printf '{\n  "format": "jetcarp-field-report",\n  "format_version": 1,\n'
        printf '  "summary": ['
        awkc "$AWK_JS"'{ printf "%s\n    %s", (NR > 1 ? "," : ""), js($0) }' "$W/summary.all"
        printf '\n  ],\n'
        printf '  "result": %s,\n  "stopped": %s,\n' "$(js "$_result")" "$([ -n "$STOPPED" ] && echo true || echo false)"
        printf '  "counts": {"pass": %d, "warn": %d, "fail": %d, "skip": %d},\n' "$N_PASS" "$N_WARN" "$N_FAIL" "$N_SKIP"
        printf '  "release": {"tag": %s, "version": %s, "prerelease": %s, "archive": %s, "sha256": %s, "program": %s},\n' \
            "$(js "v$V")" "$(js "$V")" "${PRE:-false}" "$(js "$ARCHIVE")" "$(js "${ARCHIVE_SHA:-}")" "$(js "${PROGRAM_VERSION:-}")"
        printf '  "tier": %s,\n' "$(js "$TIER")"
        printf '  "started": %s,\n  "ended": %s,\n  "seconds": %s,\n' "$(js "$RUN_STARTED")" "$(js "$(date +%Y-%m-%dT%H:%M:%S%z)")" $((_end - RUN_START))
        printf '  "platform": {"os": %s, "system": %s, "kernel": %s, "arch": %s, "target": %s, "terminal": %s, "colour": %s},\n' \
            "$(js "$OSN")" "$(js "$SYSTEM")" "$(js "$KERNEL")" "$(js "$ARCH")" "$(js "$TARGET")" \
            "$([ -n "$TTY" ] && echo true || echo false)" "$([ -n "$CYA" ] && echo true || echo false)"
        printf '  "machine": {"cpu": %s, "cores": %s, "threads": %s, "memory_bytes": %s,\n    "gpus": [' \
            "$(js "$CPU")" "${CORES:-null}" "${THREADS:-null}" "${MEM:-null}"
        if [ -s "$W/gpus.tsv" ]; then
            awkc "$AWK_JS"'BEGIN { FS = "\t" } { printf "%s\n      {\"name\": %s, \"backend\": %s, \"kind\": %s, \"driver\": %s, \"problem\": %s}", (NR > 1 ? "," : ""), js($1), js($2), js($3), js($4), ($5 == "" ? "null" : js($5)) }' "$W/gpus.tsv"
            printf '\n    '
        fi
        printf ']},\n'
        printf '  "script": {"name": "jetcarp-field-test.sh", "version": %s, "data": %s},\n' "$FT_VERSION" "$(js "$DATA_SHOWN")"
        printf '  "tests": ['
        _first=1
        for _fr in "$W"/out/*.json; do
            [ -f "$_fr" ] || continue
            if [ -n "$_first" ]; then printf '\n'; else printf ',\n'; fi
            _first=""
            cat "$_fr"
        done
        printf '\n  ],\n'
        printf '  "privacy": {"sent": "nothing: this file stays on this computer until you send it", "taken_out": ['
        printf '%s]}\n' '"the temporary test folder, shown as [scratch]", "your home folder, shown as ~", "your user name, shown as [user]", "the name of this computer, shown as [machine]", "its network addresses, shown as [ip]"'
        printf '}\n'
    } >"$_raw"
    scrub "$_raw" >"$REPORT.part" && mv "$REPORT.part" "$REPORT"
}

# What identifies this computer, each with what stands for it: the longest first.
scrub_rules() {
    _wp=$(cd "$W" 2>/dev/null && pwd -P)
    _hp=$(cd "$HOME" 2>/dev/null && pwd -P)
    {
        [ -n "$_wp" ] && printf '%s\t[scratch]\t0\n' "$_wp"
        printf '%s\t[scratch]\t0\n' "$W"
        case "$HOME" in "" | / | //) ;; *)
            [ -n "$_hp" ] && [ "$_hp" != / ] && printf '%s\t~\t0\n' "$_hp"
            printf '%s\t~\t0\n' "$HOME"
            ;;
        esac
        for _u in "$(id -un 2>/dev/null)" "${USER:-}" "${LOGNAME:-}"; do
            case "$_u" in "" | ? | ?? | root | user | admin | administrator | runner | guest | ubuntu | owner | test) ;; *) printf '%s\t[user]\t1\n' "$_u" ;; esac
        done
        for _h in "$(hostname 2>/dev/null)" "$(uname -n 2>/dev/null)" "$(scutil --get ComputerName 2>/dev/null)" "$(scutil --get LocalHostName 2>/dev/null)"; do
            _hs=${_h%%.*}
            for _x in "$_h" "$_hs"; do
                case "$_x" in "" | ? | ?? | localhost | fedora | ubuntu | debian | linux | raspberrypi | archlinux | mint | pop-os | manjaro | mac | macbook | imac | runner) ;; *) printf '%s\t[machine]\t1\n' "$_x" ;; esac
            done
        done
        {
            [ "$OSN" = linux ] && hostname -I 2>/dev/null | tr ' ' '\n'
            ifconfig 2>/dev/null | awk '$1 == "inet" || $1 == "inet6" { a = $2; sub(/^addr:/, "", a); sub(/%.*/, "", a); print a }'
            ip -o addr 2>/dev/null | awk '{ a = $4; sub(/\/.*/, "", a); print a }'
        } | awk 'NF && $1 != "127.0.0.1" && $1 != "::1" && !s[$1]++ { print $1 "\t[ip]\t1" }'
    } | awk -F '\t' '!s[tolower($1)]++ { print length($1) "\t" $0 }' | sort -rn | cut -f2-
}
scrub() { # FILE: its text, with what identifies this computer replaced
    scrub_rules >"$W/scrub.rules"
    awkc -F '\t' '
        FNR == NR { n++; from[n] = $1; to[n] = $2; word[n] = $3; low[n] = tolower($1); next }
        function rep(s, i,   out, ls, p, f, pre, post) {
            out = ""; f = length(from[i])
            while ((p = index(tolower(s), low[i])) > 0) {
                pre = (p > 1) ? substr(s, p - 1, 1) : ""; post = substr(s, p + f, 1)
                if (word[i] == 1 && (pre ~ /[A-Za-z0-9_]/ || post ~ /[A-Za-z0-9_]/)) { out = out substr(s, 1, p + f - 1); s = substr(s, p + f); continue }
                out = out substr(s, 1, p - 1) to[i]; s = substr(s, p + f)
            }
            return out s
        }
        { s = $0; for (i = 1; i <= n; i++) if (index(tolower(s), low[i])) s = rep(s, i); print s }' "$W/scrub.rules" "$1"
}

# ---- From start to end ----------------------------------------------------------------------

load_data() {
    _here=.
    case "$0" in */*) _here=${0%/*} ;; esac
    _src=""
    if [ -n "$opt_data" ]; then
        case "$opt_data" in
            http://* | https://*) _src=${opt_data%/} ;;
            *)
                [ -f "$opt_data/tests.json" ] || die "$opt_data has no tests.json"
                DATA=$opt_data
                ;;
        esac
    elif [ -f "$_here/tests.json" ] && [ -f "$_here/parity/scenes.tsv" ]; then
        DATA=$_here
    else
        _src=$DATA_URL
    fi
    if [ -n "$_src" ]; then
        info "Getting the test data from $_src"
        DATA="$W/data"
        mkdir -p "$DATA"
        fetch "$_src/tests.json" "$DATA/tests.json" || die "could not get tests.json from $_src"
        jflat "$DATA/tests.json" "$W/tests.flat"
        awkc "BEGIN { FS = \"\\t\" } $AWK_UNQ"'
            $1 == ".parity.scenes" || $1 ~ /^\.parity\.sets\[[0-9]+\]\.(short|long)$/ || $1 ~ /^\.tests\[[0-9]+\]\.scenes\[[0-9]+\]\.scene$/ { print unq($2) }' "$W/tests.flat" |
            while IFS= read -r _f; do
                case "$_f" in /* | *..*) die "tests.json names a file outside its folder: $_f" ;; esac
                mkdir -p "$DATA/${_f%/*}"
                fetch "$_src/$_f" "$DATA/$_f" || die "could not get $_f from $_src"
            done || exit 1
        DATA_SHOWN=$_src
    else
        DATA_SHOWN="the folder beside the script"
        dim "The test data: $(tilde "$(cd "$DATA" && pwd)")"
    fi
    TESTS="$W/tests.flat"
    jflat "$DATA/tests.json" "$TESTS"
    _fmt=$(jget "$TESTS" .format)
    [ "$_fmt" = 1 ] || die "tests.json is format ${_fmt:-unknown}, and this script reads format 1: get the newest jetcarp-field-test.sh"
    # The data names the oldest script that knows everything in it; an older copy stops here
    # rather than misreading a test it doesn't know (#371).
    _need=$(jget "$TESTS" .script)
    case "$_need" in '' | *[!0-9]*) _need=1 ;; esac
    [ "$_need" -le "$FT_VERSION" ] || die "this test data needs script $_need or newer, and this copy is script $FT_VERSION. Download jetcarp-field-test.sh again: https://raw.githubusercontent.com/$REPO/main/testing/field/jetcarp-field-test.sh"
    SCENES="$W/scenes"
    SCENES_TSV="$DATA/$(jget "$TESTS" .parity.scenes)"
    mkdir -p "$SCENES"
    awk -F '\t' -v d="$SCENES" '{ f = d "/" $1 ".json"; print $3 > f; close(f) }' "$SCENES_TSV"
}

list_releases() {
    info "Getting the list of JETCARP releases from GitHub"
    : >"$W/releases.tsv"
    REL_ERR=""
    _pg=1
    while [ "$_pg" -le 10 ]; do
        if ! fetch "$API_URL?per_page=30&page=$_pg" "$W/rel.json"; then
            REL_ERR="GitHub's list of releases could not be read"
            break
        fi
        jflat "$W/rel.json" "$W/rel.flat"
        _before=$(awk 'END { print NR }' "$W/releases.tsv")
        awkc -v target="$TARGET" "BEGIN { FS = \"\\t\" } $AWK_UNQ"'
            function rel(p) { sub(/^\[/, "", p); sub(/\].*/, "", p); return p + 0 }
            $1 ~ /^\[[0-9]+\]\.tag_name$/ { i = rel($1); tag[i] = unq($2); if (i + 1 > m) m = i + 1 }
            $1 ~ /^\[[0-9]+\]\.prerelease$/ { pre[rel($1)] = $2 }
            $1 ~ /^\[[0-9]+\]\.draft$/ { dr[rel($1)] = $2 }
            $1 ~ /^\[[0-9]+\]\.published_at$/ { pub[rel($1)] = substr(unq($2), 1, 10) }
            $1 ~ /^\[[0-9]+\]\.assets\[[0-9]+\]\.name$/ { j = $1; sub(/\.name$/, "", j); an[j] = unq($2) }
            $1 ~ /^\[[0-9]+\]\.assets\[[0-9]+\]\.size$/ { j = $1; sub(/\.size$/, "", j); sz[j] = $2 }
            END {
                for (j in an) { r = rel(j); v = tag[r]; sub(/^v/, "", v); if (an[j] == "jetcarp-" v "-" target ".tar.gz") size[r] = sz[j] }
                for (i = 0; i < m; i++) if (tag[i] != "" && dr[i] != "true") { v = tag[i]; sub(/^v/, "", v); printf "%s\t%s\t%s\t%s\n", v, (pre[i] == "true" ? "true" : "false"), pub[i], size[i] }
            }' "$W/rel.flat" >>"$W/releases.tsv"
        _after=$(awk 'END { print NR }' "$W/releases.tsv")
        [ $((_after - _before)) -lt 30 ] && break
        _pg=$((_pg + 1))
    done
}

choose_release() {
    _nrel=$(awk 'END { print NR }' "$W/releases.tsv")
    if [ -n "$opt_release" ]; then
        _want=${opt_release#v}
        [ "$_want" = latest ] && _want=$(sed -n 1p "$W/releases.tsv" | cut -f1)
        [ -n "$_want" ] || die "could not find the newest release ($REL_ERR)"
        _row=$(awk -F '\t' -v v="$_want" '$1 == v' "$W/releases.tsv")
        if [ -z "$_row" ] && [ "$_nrel" -gt 0 ]; then
            die "there is no release $opt_release (the newest is $(sed -n 1p "$W/releases.tsv" | cut -f1))"
        fi
        [ -n "$_row" ] || _row="$_want$TAB$(case "$_want" in *-*) echo true ;; *) echo false ;; esac)$TAB$TAB"
    elif [ "$_nrel" -eq 0 ]; then
        die "${REL_ERR:-GitHub lists no releases}; name one with --release (such as 0.4.3-rc.2)"
    elif [ -n "$ASK" ]; then
        heading "Which release?"
        awk -F '\t' -v d="$D" -v r="$R" '
            NR <= 25 { printf "  %3d) %-16s %-18s %s%s%s\n", NR, $1, ($2 == "true" ? "release candidate" : "release"), d, $3 (NR == 1 ? "   the newest" : "") ($4 == "" ? "   (no build for this computer)" : ""), r }' "$W/releases.tsv"
        while :; do
            ask "Press Enter for 1 (the newest), or type a number:" 1
            case "$ANSWER" in
                *[!0-9]* | "") warn "Type a number from the list." ;;
                *)
                    _row=$(sed -n "${ANSWER}p" "$W/releases.tsv")
                    [ -n "$_row" ] && break
                    warn "There is no $ANSWER in the list."
                    ;;
            esac
        done
    else
        _row=$(sed -n 1p "$W/releases.tsv")
    fi
    V=$(printf '%s' "$_row" | cut -f1)
    PRE=$(printf '%s' "$_row" | cut -f2)
    SIZE=$(printf '%s' "$_row" | cut -f4)
    [ -n "$SIZE" ] || [ "$_nrel" -eq 0 ] || die "JETCARP $V has no build for this computer ($TARGET)"
    case "$V" in [0-9]*.[0-9]*.[0-9]*) ;; *) die "$V is not a JETCARP version" ;; esac
}

choose_tier() {
    TIER=$opt_tier
    if [ -z "$TIER" ] && [ -n "$ASK" ]; then
        heading "How long?"
        _i=0
        while [ "$_i" -lt "$(jlen "$TESTS" .tiers)" ]; do
            printf '  %d) %s%-9s%s %s\n' $((_i + 1)) "$B" "$(jget "$TESTS" ".tiers[$_i].name")" "$R" "$(jget "$TESTS" ".tiers[$_i].minutes")"
            printf '%s\n' "$(jget "$TESTS" ".tiers[$_i].about")" | wrap 7 | sed "s/^ */       $D/; s/\$/$R/"
            _i=$((_i + 1))
        done
        dim "The times are for a typical laptop: this computer may take more or less."
        while :; do
            ask "Press Enter for 1 ($(jget "$TESTS" '.tiers[0].name')), or type a number:" 1
            case "$ANSWER" in
                [1-9]) TIER=$(jget "$TESTS" ".tiers[$((ANSWER - 1))].id") && [ -n "$TIER" ] && break ;;
            esac
            warn "Type 1, 2 or 3."
        done
    fi
    [ -n "$TIER" ] || TIER=quick
    _t=0
    while [ "$(jget "$TESTS" ".tiers[$_t].id")" != "$TIER" ]; do
        _t=$((_t + 1))
        [ "$_t" -lt 10 ] || die "tests.json has no tier $TIER"
    done
    TIER_NAME=$(jget "$TESTS" ".tiers[$_t].name")
}

plan() {
    NT=$(jlen "$TESTS" .tests)
    : >"$W/plan.tsv"
    N_PLAN=0
    _i=0
    while [ "$_i" -lt "$NT" ]; do
        if jarr "$TESTS" ".tests[$_i].tiers" | grep -q -x "$TIER"; then
            _from=$(tget "$_i" from)
            _to=$(tget "$_i" to)
            if in_range "$V" "$_from" "$_to"; then
                printf '%s\trun\t\n' "$_i" >>"$W/plan.tsv"
                N_PLAN=$((N_PLAN + 1))
            elif [ -n "$_to" ] && ! in_range "$V" "" "$_to"; then
                printf '%s\tskip\tnot for JETCARP after %s\n' "$_i" "$_to" >>"$W/plan.tsv"
            else
                printf '%s\tskip\tneeds JETCARP %s or later\n' "$_i" "$_from" >>"$W/plan.tsv"
            fi
        fi
        _i=$((_i + 1))
    done
    # The parity set for this release.
    _ps=$(pick_range "$TESTS" .parity.sets)
    PAR_LONG_ANY="$DATA/$(jget "$TESTS" '.parity.sets[0].long')"
    if [ -n "$_ps" ]; then
        PAR_SHORT="$DATA/$(jget "$TESTS" ".parity.sets[$_ps].short")"
        PAR_LONG="$DATA/$(jget "$TESTS" ".parity.sets[$_ps].long")"
        PAR_NOTE="from $(jget "$TESTS" ".parity.sets[$_ps].from")$(_x=$(jget "$TESTS" ".parity.sets[$_ps].to"); [ -n "$_x" ] && echo " to $_x"): $(jget "$TESTS" ".parity.sets[$_ps].hash")"
    fi
    EST_TOTAL=0
    while IFS="$TAB" read -r _i _what _why; do
        [ "$_what" = run ] && EST_TOTAL=$((EST_TOTAL + $(est "$_i")))
    done <"$W/plan.tsv"
}

show_plan() {
    heading "The plan: JETCARP $V, $TIER_NAME"
    _k=0
    while IFS="$TAB" read -r _i _what _why; do
        if [ "$_what" = run ]; then
            _k=$((_k + 1))
            printf '  %2d  %-46s %s~%s%s\n' "$_k" "$(tget "$_i" name)" "$D" "$(dur "$(est "$_i")")" "$R"
        else
            printf '  %s  %-46s %s%s%s\n' " -" "$(tget "$_i" name)" "$D" "skipped: $_why" "$R"
        fi
    done <"$W/plan.tsv"
    info "About $(dur "$EST_TOTAL") on a typical laptop, longer with several GPUs."
    info "Nothing is drawn on screen, nothing is sent anywhere, and your own JETCARP, its settings and"
    info "its library are not touched: this version runs from a temporary folder, with folders of its own."
}

install_scratch() {
    heading "Getting JETCARP $V"
    BASE="https://github.com/$REPO/releases/download/v$V"
    ARCHIVE="jetcarp-$V-$TARGET.tar.gz"
    mkdir -p "$W/download" "$W/jetcarp"
    info "From $BASE"
    fetch "$BASE/SHA256SUMS" "$W/download/SHA256SUMS" || die "could not get SHA256SUMS for $V (is v$V published?)"
    fetch "$BASE/$ARCHIVE" "$W/download/$ARCHIVE" &
    NETPID=$!
    while kill -0 "$NETPID" 2>/dev/null; do
        _got=0
        [ -f "$W/download/$ARCHIVE" ] && _got=$(wc -c <"$W/download/$ARCHIVE" | tr -d ' ')
        if [ -n "${SIZE:-}" ] && [ "$SIZE" -gt 0 ]; then
            draw "Downloading JETCARP $V" $((${_got:-0} * 1000 / SIZE)) "$(human "${_got:-0}") of $(human "$SIZE")" -1 ""
        else
            draw "Downloading JETCARP $V" -1 "$(human "${_got:-0}")" -1 ""
        fi
        sleep "$TICK"
    done
    wait "$NETPID" || die "could not download $ARCHIVE"
    NETPID=""
    status_clear
    ARCHIVE_SHA=$(sha256 "$W/download/$ARCHIVE")
    _want=$(awk -v f="$ARCHIVE" '$2 == f || $2 == "*" f { print $1 }' "$W/download/SHA256SUMS")
    [ -n "$_want" ] || die "SHA256SUMS does not list $ARCHIVE"
    [ "$_want" = "$ARCHIVE_SHA" ] || die "$ARCHIVE does not match SHA256SUMS (expected $_want, got $ARCHIVE_SHA): nothing was run"
    info "Downloaded $ARCHIVE ($(human "$(wc -c <"$W/download/$ARCHIVE")")); it matches the release's SHA256SUMS"
    tar -xzf "$W/download/$ARCHIVE" -C "$W/jetcarp" || die "could not unpack $ARCHIVE"
    JET="$W/jetcarp/jetcarp-$V-$TARGET/jetcarp"
    [ -x "$JET" ] || die "$ARCHIVE has no jetcarp program"
    # JETCARP's own folders, inside the temporary one.
    JETCARP_CONFIG_DIR="$W/folders/settings"
    JETCARP_DATA_DIR="$W/folders/data"
    JETCARP_OUT="$W/folders/output"
    JETCARP_RUNTIME_DIR="$W/folders/runtime"
    JETCARP_LIBRARY="$W/folders/data/library.sqlite"
    export JETCARP_CONFIG_DIR JETCARP_DATA_DIR JETCARP_OUT JETCARP_RUNTIME_DIR JETCARP_LIBRARY
    mkdir -p "$JETCARP_CONFIG_DIR" "$JETCARP_DATA_DIR" "$JETCARP_OUT" "$JETCARP_RUNTIME_DIR"
    chmod 700 "$JETCARP_RUNTIME_DIR"
    PROGRAM_VERSION=$("$JET" --version 2>"$W/version.err" </dev/null) || die "JETCARP $V's program does not run here: $(head -c 300 "$W/version.err")"
    info "Unpacked $PROGRAM_VERSION into $(tilde "$W"), with its settings, data and output folders inside it"
}

run_tests() {
    STARTED=1
    RUN_START=$(now)
    RUN_STARTED=$(date +%Y-%m-%dT%H:%M:%S%z)
    mkdir -p "$W/out" "$W/tests"
    : >"$W/summary.txt"
    while IFS="$TAB" read -r _rt_i _rt_what _rt_why <&3; do
        if [ "$_rt_what" != run ]; then
            heading "$(tget "$_rt_i" name)"
            printf '  %s %s\n' "$(tag skip)" "$_rt_why"
            test_skip "$_rt_i" "$_rt_why"
            continue
        fi
        test_begin "$_rt_i"
        case "$T_KIND" in
            doctor) t_doctor ;;
            adapters) t_adapters ;;
            busy) t_busy ;;
            score) t_score ;;
            parity) t_parity ;;
            check) t_check ;;
            fit) t_fit ;;
            speed) t_speed ;;
            explore) t_explore ;;
            *)
                problem "this script doesn't know tests of the kind $T_KIND: get the newest jetcarp-field-test.sh"
                test_end skip "not known to this script"
                N_SKIP=$((N_SKIP + 1))
                ;;
        esac
        # After the doctor, the GPUs are known: the estimates follow.
        if [ "$T_KIND" = doctor ] || [ "$T_KIND" = adapters ]; then
            _rest=0
            _seen=""
            while IFS="$TAB" read -r _j _w2 _y2; do
                [ "$_j" = "$_rt_i" ] && _seen=1 && continue
                [ -n "$_seen" ] && [ "$_w2" = run ] && _rest=$((_rest + $(est "$_j")))
            done <"$W/plan.tsv"
            EST_TOTAL=$((EST_DONE + _rest))
        fi
    done 3<"$W/plan.tsv"
}

finish() {
    write_report
    # Every test ran and the report is written: a Ctrl-C from here on (while the report is
    # shown, or at the last question) leaves it as it is, not marked as stopped (#371).
    FINISHED=1
    _secs=$(($(now) - RUN_START))
    heading "Finished in $(dur "$_secs"): $N_PASS passed, $N_WARN with warnings, $N_FAIL failed, $N_SKIP skipped"
    tail -n +3 "$W/summary.all" | wrap 2 | awk -v red="$RED" -v yel="$YEL" -v d="$D" -v r="$R" '
        /^\[FAIL\]/ { c = red } /^\[warn\]/ { c = yel } /^\[skip\]/ { c = d } /^\[( ok |stop)\]/ { c = "" }
        { printf "  %s%s%s\n", c, $0, (c != "" ? r : "") }'
    heading "The report"
    info "$(tilde "$REPORT") ($(human "$(wc -c <"$REPORT")"))"
    info "It holds the release, this computer's system, processor, memory and graphics as JETCARP sees"
    info "them, each test's result, numbers and timings, and the output of anything that went wrong."
    info "Taken out: your user name, your home folder (shown as ~), this computer's name and its"
    info "network addresses. Nothing was sent: send this one file by hand to whoever asked you to test."
    ask "Show the whole file now? [Y/n]" y
    case "$ANSWER" in
        [Yy]*)
            if [ -n "$ASK" ]; then
                # less with -E leaves by itself at the end of the file, and its prompt says how
                # to leave sooner: never a bare "(END)" with no way out shown (#371).
                if command -v less >/dev/null 2>&1; then
                    info "Space shows the next page; q comes back here."
                    LESS='' less -E -X -R '-PsSpace for the next page, q to go back to the test' "$REPORT"
                else
                    cat "$REPORT"
                fi
            fi
            ;;
    esac
    end_choice
    say ""
    say "The report: $(tilde "$REPORT")"
}

installed_note() {
    _m="$HOME/.local/lib/jetcarp/.jetcarp-installer"
    if [ -f "$_m" ]; then
        printf 'in place of the %s installed in ~/.local/lib/jetcarp (your settings and library stay)' "$(head -n 1 "$_m" | awk '{ print $1, $2 }')"
    else
        printf 'in ~/.local/lib/jetcarp, linked from ~/.local/bin/jetcarp'
    fi
}

end_choice() {
    _c=$opt_end
    if [ -z "$_c" ] && [ -n "$ASK" ]; then
        heading "Keep JETCARP $V?"
        info "1) Remove everything this script added: the temporary folder, with the program and its test"
        info "   folders (the default)"
        info "2) Keep JETCARP $V: install it with JETCARP's own installer, $(installed_note)"
        ask "Press Enter for 1, or type 2:" 1
        case "$ANSWER" in 2 | k | keep) _c=keep ;; *) _c=remove ;; esac
    fi
    [ -n "$_c" ] || _c=remove
    if [ "$_c" = keep ]; then
        heading "Installing JETCARP $V"
        if fetch "$BASE/jetcarp-installer.sh" "$W/download/jetcarp-installer.sh"; then
            _iw=$(awk '$2 == "jetcarp-installer.sh" || $2 == "*jetcarp-installer.sh" { print $1 }' "$W/download/SHA256SUMS")
            if [ -n "$_iw" ] && [ "$_iw" = "$(sha256 "$W/download/jetcarp-installer.sh")" ]; then
                info "With the release's own installer (it matches SHA256SUMS): sh jetcarp-installer.sh --version $V"
                (
                    for _n in JETCARP_CONFIG_DIR JETCARP_DATA_DIR JETCARP_OUT JETCARP_RUNTIME_DIR JETCARP_LIBRARY; do unset "$_n"; done
                    while IFS='=' read -r _k _v; do [ -n "$_k" ] && export "$_k=$_v"; done <<EOF
$ORIG_ENV
EOF
                    [ -n "$ORIG_DISPLAY" ] && export DISPLAY="$ORIG_DISPLAY"
                    [ -n "$ORIG_WAYLAND" ] && export WAYLAND_DISPLAY="$ORIG_WAYLAND"
                    sh "$W/download/jetcarp-installer.sh" --version "$V" --url "$W/download"
                ) >"$W/install.log" 2>&1
                _irc=$?
                sed 's/^/    /' "$W/install.log"
                [ "$_irc" -eq 0 ] || warn "The installer stopped (exit status $_irc): nothing of this version was kept."
            else
                warn "The installer does not match SHA256SUMS: nothing was installed."
            fi
        else
            warn "Could not get JETCARP's installer: nothing was installed."
        fi
    fi
    if [ -n "$KEEP_WORK" ]; then
        info "Keeping the temporary folder, as FIELD_TEST_KEEP_WORK asks: $(tilde "$W")"
    else
        rm -rf "$W"
        info "Removed the temporary folder ($(tilde "$W")): nothing of the test is left but the report."
        W=""
    fi
}

main() {
    printf '%sJETCARP field test%s %s(script %s)%s\n' "$B$CYA" "$R" "$D" "$FT_VERSION" "$R"
    say "This tests a JETCARP release on this computer, and writes one report file for you to send."
    say "It shows nothing on screen (nothing can flash), sends nothing anywhere, and leaves your own"
    say "JETCARP alone. Ctrl-C stops it at any point."
    for _t in awk tar ps; do
        command -v "$_t" >/dev/null 2>&1 || die "this script needs $_t"
    done
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || die "this script needs curl or wget"
    W=$(mktemp -d "${TMPDIR:-/tmp}/jetcarp-field.XXXXXX") || die "could not make a temporary folder"
    W=$(cd "$W" && pwd)
    trap cleanup EXIT
    trap 'on_stop Ctrl-C' INT
    trap 'on_stop "a request to stop"' TERM
    trap 'on_stop "the terminal closing"' HUP
    heading "Getting ready"
    load_data
    list_releases
    choose_release
    choose_tier
    if [ -n "$opt_report" ]; then
        case "$opt_report" in /*) REPORT=$opt_report ;; *) REPORT="$PWD/$opt_report" ;; esac
    else
        _rd=$PWD
        { [ -w "$_rd" ] && [ "$_rd" != / ]; } || _rd=$HOME
        REPORT="$_rd/jetcarp-field-report-$V-$TIER-$(date +%Y%m%d-%H%M).json"
    fi
    (: >"$REPORT.check.$$") 2>/dev/null || die "can't write the report to $REPORT (choose another place with --report)"
    rm -f "$REPORT.check.$$"
    plan
    show_plan
    ask "Start? [Y/n]" y
    case "$ANSWER" in [Nn]*) say "Nothing was run."; exit 0 ;; esac
    install_scratch
    run_tests
    finish
    [ "$N_FAIL" -eq 0 ] || exit 1
}

# On one line, so that the shell has read all of it before it runs: a copy of this file changed
# while it runs can't change what runs after.
main; exit
