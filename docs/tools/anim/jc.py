"""Helpers for docs/tools/anim/animate.py: run frames.rhai in JETCARP, measure worlds and live pixels.

Adapted from the clip tool of JETCARP's curation picker. Needs NumPy and Pillow.
"""
import json
import os
import shutil
import subprocess

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
FRAMES = os.path.join(HERE, "frames.rhai")
JETCARP = os.environ.get("JETCARP") or os.path.join(REPO, "target", "release", "jetcarp")
if not os.path.isabs(JETCARP):
    JETCARP = os.path.join(REPO, JETCARP)


def render(scene, gens, max_side, outdir, outlines=True, bg=True, threads=4, timeout=3600):
    """Render the scene at each generation in `gens` (ascending) to outdir/f000.png, f001.png, ...

    With bg, also writes outdir/bg.png: the empty world drawn the same way (for live-pixel tests).
    Returns (info, frame paths); info has the final status, populations and done_at.
    """
    if os.path.isdir(outdir):
        shutil.rmtree(outdir)
    os.makedirs(outdir)
    g = ",".join(str(int(x)) for x in gens)
    cmd = [JETCARP, "script", FRAMES, "--threads", str(threads), "--out", outdir,
           "--set", f"scene={os.path.abspath(scene)}", "--set", f"gens={g}",
           "--set", f"max_side={int(round(max_side))}",
           "--set", f"outlines={'true' if outlines else 'false'}",
           "--set", f"bg={'true' if bg else 'false'}"]
    env = dict(os.environ, RAYON_NUM_THREADS=str(threads))
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
    if r.returncode != 0 or not os.path.exists(os.path.join(outdir, "info.json")):
        raise RuntimeError(f"render failed: {r.stdout[-1500:]} {r.stderr[-1500:]}")
    info = json.load(open(os.path.join(outdir, "info.json")))
    frames = [os.path.join(outdir, f"f{i:03d}.png") for i in range(len(gens))]
    return info, frames


def arr(path_or_img):
    im = Image.open(path_or_img) if isinstance(path_or_img, str) else path_or_img
    return np.asarray(im.convert("RGB"))


def colours(a):
    """(count, (r, g, b)) pairs, most common first."""
    flat = a.reshape(-1, 3)
    key = (flat[:, 0].astype(np.uint32) << 16) | (flat[:, 1].astype(np.uint32) << 8) | flat[:, 2]
    vals, counts = np.unique(key, return_counts=True)
    order = np.argsort(-counts)
    return [(int(counts[i]), (int(vals[i] >> 16), int((vals[i] >> 8) & 255), int(vals[i] & 255)))
            for i in order]


def world_metrics(outdir, cells):
    """Image size, tile size in px, the world's box, stage colour and outside colour, from bg.png
    (which must have been rendered without outlines)."""
    p = os.path.join(outdir, "bg.png")
    a = arr(p)
    cols = colours(a)
    stage = cols[0][1]
    outside = None
    if len(cols) > 1 and cols[1][0] > 0.002 * a.shape[0] * a.shape[1]:
        outside = cols[1][1]
    if outside is None:
        box = (0, 0, a.shape[1], a.shape[0])
        area = a.shape[0] * a.shape[1]
    else:
        m = ~np.all(a == np.array(outside, dtype=a.dtype), axis=2)
        ys, xs = np.nonzero(m)
        box = (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)
        area = int(m.sum())
    tile = (area / max(cells, 1)) ** 0.5
    return dict(size=Image.open(p).size, tile=tile, box=box, stage=stage, outside=outside or stage)


def live_bbox(frame, bga, thr=12):
    """Bounding box of the pixels that differ from the empty world, or None."""
    a = arr(frame).astype(np.int16)
    if a.shape != bga.shape:
        return None
    d = np.abs(a - bga).max(axis=2) > thr
    if not d.any():
        return None
    ys, xs = np.nonzero(d)
    return (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)
