#!/usr/bin/env bash
# Regenerate the README's banner: six scenes from docs/scenes/readme/, each run from its seed and
# filmed over its liveliest stretch, cropped to 4:3 and laid out three by two, as a looping animated
# WebP, with a still of the last frame for readers who ask for reduced motion.
#   docs/src/images/readme/six-worlds.webp        the animation (about 3.5 MB)
#   docs/src/images/readme/six-worlds-still.webp  its last frame
#
# Usage (from anywhere):  docs/tools/readme-hero.sh
#   JETCARP=path/to/jetcarp overrides the binary (default: target/release/jetcarp).
#   WORK (default target/readme-hero) holds the frames while they are made.
# Needs ffmpeg and libwebp's img2webp and cwebp. JETCARP's folders point into WORK, so nothing is
# written to your own settings or library.
set -euo pipefail
cd "$(dirname "$0")/../.."
J=${JETCARP:-target/release/jetcarp}
WORK=${WORK:-target/readme-hero}
OUT=docs/src/images/readme
mkdir -p "$WORK" "$OUT"
export JETCARP_CONFIG_DIR="$WORK/config" JETCARP_DATA_DIR="$WORK/data" JETCARP_OUT="$WORK/out"

# scene file, generations (first last step), crop of the 900-pixel render to 4:3
scenes=(
  "1-layered-spectre     0 118 2   crop=724:543:0:179"
  "2-cyclic-square       0 472 8   crop=800:600:50:0"
  "3-jaca-cairo-rainbow  0 236 4   crop=797:598:51:0"
  "4-hodgepodge-square   0 590 10  crop=800:600:50:0"
  "5-jaca-k2-rope-network 0 236 4  crop=789:592:55:0"
  "6-jaca-penrose-rainbow 0 236 4  crop=720:540:90:179"
)

inputs=()
for s in "${scenes[@]}"; do
  read -r name first last step crop <<<"$s"
  raw="$WORK/raw/$name" cut="$WORK/cut/$name"
  mkdir -p "$raw" "$cut"
  i=0
  for g in $(seq "$first" "$step" "$last"); do
    f=$(printf '%s/%03d.png' "$raw" "$i")
    [[ -f "$f" ]] || "$J" run "docs/scenes/readme/$name.json" --gens "$g" --png "$f" --max-side 900 >/dev/null
    i=$((i + 1))
  done
  ffmpeg -loglevel error -y -framerate 12 -i "$raw/%03d.png" \
    -vf "$crop,scale=400:300:flags=lanczos" "$cut/%03d.png"
  inputs+=(-framerate 12 -i "$cut/%03d.png")
done

# Three by two, every second frame, at 906 x 453.
grid="$WORK/grid"
mkdir -p "$grid"
ffmpeg -loglevel error -y "${inputs[@]}" -filter_complex \
  "xstack=inputs=6:layout=0_0|404_0|808_0|0_304|404_304|808_304:fill=0x0d1117,select='not(mod(n\,2))',scale=906:453:flags=lanczos" \
  -vsync 0 "$grid/%03d.png"
frames=("$grid"/*.png)
last=${frames[-1]}
img2webp -loop 0 -lossy -q 50 -m 6 -d 166 "${frames[@]:0:${#frames[@]}-1}" -d 2000 "$last" \
  -o "$OUT/six-worlds.webp" >/dev/null
cwebp -quiet -q 80 -m 6 "$last" -o "$OUT/six-worlds-still.webp"
ls -l "$OUT/six-worlds.webp" "$OUT/six-worlds-still.webp"
