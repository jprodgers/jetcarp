#!/usr/bin/env bash
# Regenerate every figure in the JETCARP documentation from its scene, so the pictures never drift
# from the program. Each figure is one scene file in docs/scenes/<section>/<name>.json, run for a
# fixed number of generations and written to docs/src/images/<section>/<name>.png. JETCARP embeds
# the scene, version and state hash in each PNG's text chunks.
#
# Usage (from anywhere):  docs/tools/render-figures.sh [filter]
#   JETCARP=path/to/jetcarp overrides the binary (default: target/release/jetcarp).
#   A filter renders only figures whose name contains it, e.g. `docs/tools/render-figures.sh sandpile`.
#
# The animated figures (spaceships, gliders, waves, battles) are made from their scenes by
# docs/tools/render-animations.sh, listed in docs/tools/anim/animations.json.
#
# After rendering, regenerate the generated pages if the program's data changed:
#   python3 docs/tools/gen-bibliography.py
#   python3 docs/tools/gen-named-rules.py
#   python3 docs/tools/gen-tiling-catalogue.py
set -euo pipefail
cd "$(dirname "$0")/../.."
JETCARP=${JETCARP:-target/release/jetcarp}
FILTER=${1:-}

# render <section>/<name> <generations> <longest side in px>
render() {
  local id=$1 gens=$2 side=$3
  [[ -n "$FILTER" && "$id" != *"$FILTER"* ]] && return 0
  mkdir -p "docs/src/images/$(dirname "$id")"
  "$JETCARP" run "docs/scenes/$id.json" --gens "$gens" --max-side "$side" \
    --png "docs/src/images/$id.png" > /dev/null
  printf '%-48s %6s gens  %7s bytes\n' "$id" "$gens" "$(stat -c %s "docs/src/images/$id.png")"
}

# other <section>/<name> <jetcarp subcommand and options...>: a figure made by another JETCARP
# command (contact sheets); the output path is appended as --png.
other() {
  local id=$1; shift
  [[ -n "$FILTER" && "$id" != *"$FILTER"* ]] && return 0
  mkdir -p "docs/src/images/$(dirname "$id")"
  "$JETCARP" "$@" --png "docs/src/images/$id.png" > /dev/null
  printf '%-48s %11s  %7s bytes\n' "$id" "(command)" "$(stat -c %s "docs/src/images/$id.png")"
}

# qsheet <section>/<name> <filter>: a contact sheet of the Quick Start entries matching a filter,
# numbered in the order `jetcarp quickstart --filter <filter>` lists them.
qsheet() {
  local id=$1 filter=$2
  [[ -n "$FILTER" && "$id" != *"$FILTER"* ]] && return 0
  mkdir -p "docs/src/images/$(dirname "$id")"
  "$JETCARP" quickstart --sheet "docs/src/images/$id.png" --filter "$filter" --cols 10 --thumb 84 \
    --gens 200 > /dev/null
  printf '%-48s %11s  %7s bytes\n' "$id" "(sheet)" "$(stat -c %s "docs/src/images/$id.png")"
}

# ---- Cellular automata --------------------------------------------------------------------
render automata/generalising-touch        1   360
render automata/generalising-edge         1   360
render automata/generalising-layers       9   640
render automata/life-hex                 60   800
render automata/life-triangle           300   800
render automata/elementary-rule30       200   800
render automata/elementary-rule110      200   800
other  automata/elementary-sheet        sheet --kind w --from 0 --count 64 --cols 8 --thumb 110
render automata/hex-hexw3               200   800
render automata/hex-hext3-sum588        200   640
render automata/hex-hext3-sum314        200   640
other  automata/hex-hexw-sheet          sheet --kind hexw --from 0 --count 16 --cols 8 --thumb 110
render automata/layered-spectre         400   720
render automata/layered-cairo           400   640
render automata/layered-square          200   480
render automata/sandpile-identity-cairo   0   640
render automata/sandpile-pile-k2       2000   480
render automata/sandpile-pile-hat      2000   480
render automata/vonneumann-construct     20   552
render automata/ruletable-hex            47   480
qsheet automata/named-generations-sheet Generations

# ---- Tilings --------------------------------------------------------------------------------
render tilings/graph-brick                  1   300
render tilings/graph-pyth                   1   300
render tilings/graph-rhomb                  1   300
render tilings/archimedean-rhombitrihex   200   720
render tilings/kuniform-k3-011            100   720
render tilings/isohedral-ih28             200   640
other  tilings/isohedral-sheet            isohedral --cols 9 --thumb 88
render tilings/editable-lizard             30   640
render tilings/penrose-kd-sandpile       3000   560
render tilings/penrose-rhomb-b2s34        200   560
render tilings/hat-b2s34                  200   560
render tilings/spectre-b2s23v             200   560

# Thumbnails for the tiling catalogue (docs/src/tilings/catalogue.md).
if [[ -z "$FILTER" || "thumbs" == *"$FILTER"* ]]; then
  mkdir -p docs/src/images/tilings/thumbs
  "$JETCARP" script docs/tools/tiling-thumbs.rhai --out docs/src/images/tilings/thumbs 2>/dev/null
fi
