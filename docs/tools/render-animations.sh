#!/usr/bin/env bash
# Regenerate every animation in the JETCARP documentation from its scene, as render-figures.sh does
# for the still figures. The list is docs/tools/anim/animations.json: each entry names a scene in
# docs/scenes/ and how to film it. docs/tools/anim/animate.py runs the scene in JETCARP (through
# docs/tools/anim/frames.rhai), crops and labels the frames, and writes
#   docs/src/images/<id>.webp        a looping lossless animated WebP
#   docs/src/images/<id>-still.png   the still shown to readers who ask for reduced motion
# Pages show them with
#   <picture>
#     <source srcset="<id>-still.png" media="(prefers-reduced-motion: reduce)">
#     <img src="<id>.webp" alt="..." loading="lazy">
#   </picture>
#
# Usage (from anywhere):  docs/tools/render-animations.sh [filter]
#   A filter renders only animations whose id contains it, e.g. `docs/tools/render-animations.sh cyclic`.
#   JETCARP=path/to/jetcarp overrides the binary (default: target/release/jetcarp).
#   PYTHON=path/to/python: a Python 3.9 or later with NumPy and Pillow (default: python3).
#   JOBS (default 4) animations are rendered at once, each with THREADS (default 4) JETCARP threads.
#   WORK (default target/docs-anim) holds the frames while they are made; it can be large.
set -euo pipefail
cd "$(dirname "$0")/../.."
PYTHON=${PYTHON:-python3}
if ! "$PYTHON" -I -c "import numpy, PIL" 2>/dev/null; then
  echo "render-animations.sh: $PYTHON cannot import NumPy and Pillow; set PYTHON=/path/to/python" >&2
  exit 1
fi
args=(--jobs "${JOBS:-4}" --threads "${THREADS:-4}" --work "${WORK:-target/docs-anim}")
if [[ -n "${1:-}" ]]; then
  args+=(--only "$1")
fi
exec "$PYTHON" -I docs/tools/anim/animate.py "${args[@]}"
