#!/usr/bin/env python3
"""Render the documentation's animations from their scenes.

Each entry of docs/tools/anim/animations.json names a scene in docs/scenes/ and how to film it. The
script runs the scene in JETCARP (frames.rhai, one PNG per frame), crops and labels the frames, and
writes two files next to the other figures:

  docs/src/images/<id>.webp        a looping, lossless animated WebP
  docs/src/images/<id>-still.png   a still for readers who ask for reduced motion: the last frame
                                   (evolutions), a fading trail of the mover (spaceships, gliders) or
                                   the first phase (oscillators)

Kinds of entry:
  evo     a whole world ("frame": "whole") or a window of it ("frame": "centre", `tile_px` pixels per
          tile, centred at `centre_at`, fractions of the world's box), from generation `start` (0) to
          `start` + `gens`, `frames` frames spaced evenly (or eased with `ease` > 1, to spend more
          frames on the start). Labelled with the generation (or the step, for ants, with
          "label": "step").
  mover   a spaceship or glider: a window that holds its whole path over `gens` generations from
          generation `start` (0), one frame every `every` generations, at most `tile_px` pixels per
          tile; the still is a trail of `trail` (4) positions.
  osc     an oscillator: one period, one frame per generation.

Options common to all kinds: width (px), max_height (px), tile_px, ms (per frame), hold (ms on the
last frame), outlines (true, false or "auto"; auto draws them when tiles are at least 6 px), label_at
("right" puts the generation label at the bottom right), max_kb (size cap, default 360: frames are
dropped down to min_frames, then the frame is cropped towards its centre, until it fits).

Usage (needs Python 3.9+ with NumPy and Pillow; run through docs/tools/render-animations.sh):
  animate.py [--only SUBSTRING] [--jobs 4] [--threads 4] [--work DIR] [--list]
"""
import argparse
import io
import json
import os
import shutil
import sys
import time
import traceback
from multiprocessing import Pool

import numpy as np
from PIL import Image, ImageDraw, ImageFont, PngImagePlugin

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import jc  # noqa: E402

DOCS = os.path.join(jc.REPO, "docs")
SCENES = os.path.join(DOCS, "scenes")
IMAGES = os.path.join(DOCS, "src", "images")
MANIFEST = os.path.join(HERE, "animations.json")
THREADS = 4
WORK = os.path.join(jc.REPO, "target", "docs-anim")

try:
    FONT = ImageFont.load_default(size=11)
except TypeError:  # Pillow < 10.1
    FONT = ImageFont.load_default()


# ---------------------------------------------------------------- image helpers

def crop_fixed(im, cx, cy, W, H, fill):
    """A W x H crop centred on (cx, cy); anything outside the image is `fill`."""
    x0 = int(round(cx - W / 2))
    y0 = int(round(cy - H / 2))
    out = Image.new("RGB", (W, H), fill)
    sx0, sy0 = max(0, x0), max(0, y0)
    sx1, sy1 = min(im.width, x0 + W), min(im.height, y0 + H)
    if sx1 > sx0 and sy1 > sy0:
        out.paste(im.crop((sx0, sy0, sx1, sy1)), (sx0 - x0, sy0 - y0))
    return out


def label(im, text, right=False):
    d = ImageDraw.Draw(im, "RGBA")
    tw = d.textlength(text, font=FONT)
    x, y = (im.width - tw - 15 if right else 5), im.height - 19
    d.rounded_rectangle((x, y, x + tw + 10, y + 15), radius=4, fill=(10, 11, 15, 175))
    d.text((x + 5, y + 2), text, font=FONT, fill=(225, 228, 236, 255))
    return im


def union(boxes):
    bs = [b for b in boxes if b]
    if not bs:
        return None
    return (min(b[0] for b in bs), min(b[1] for b in bs), max(b[2] for b in bs), max(b[3] for b in bs))


def grow(box, min_w, min_h, min_ratio):
    """Grow a box about its centre to at least min_w x min_h and to an aspect ratio of at least min_ratio."""
    x0, y0, x1, y1 = box
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    w, h = max(x1 - x0, min_w), max(y1 - y0, min_h)
    if w >= h:
        h = max(h, min_ratio * w)
    else:
        w = max(w, min_ratio * h)
    return (cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2)


def eased_gens(G, N, gamma):
    gs = []
    for i in range(N):
        g = int(round(G * (i / (N - 1)) ** gamma)) if N > 1 else 0
        if gs and g <= gs[-1]:
            g = gs[-1] + 1
        gs.append(g)
    return gs


def trim_static(frames, gens, keep=2):
    """Drop frames at the end that repeat the last one (keeping `keep`)."""
    last = frames[-1].tobytes()
    k = len(frames) - 1
    while k > 0 and frames[k - 1].tobytes() == last:
        k -= 1
    end = min(len(frames), k + keep)
    return frames[:end], gens[:end]


def outline_colour(bgpath, stage, outside):
    for _, c in jc.colours(jc.arr(bgpath)):
        if c != stage and c != outside:
            return c
    return None


def brighten(frames, col, stage, factor=2.2):
    """Make the tile outlines (colour `col`) stand out more against the stage."""
    if not col:
        return frames
    new = tuple(int(max(0, min(255, s + factor * (o - s)))) for o, s in zip(col, stage))
    c = np.array(col, dtype=np.uint8)
    out = []
    for f in frames:
        a = np.array(f)
        a[np.all(a == c, axis=2)] = new
        out.append(Image.fromarray(a))
    return out


def webp_bytes(fr, du):
    b = io.BytesIO()
    fr[0].save(b, "WEBP", save_all=True, append_images=fr[1:], duration=du, loop=0,
               lossless=True, quality=50, method=4)
    return b.getvalue()


def subsample(fr, du, n):
    """Keep about n evenly spaced frames (always the first and last), merging their durations."""
    if n >= len(fr):
        return fr, du
    idx = sorted(set(round(i * (len(fr) - 1) / (n - 1)) for i in range(n)))
    nd = []
    for a, i in enumerate(idx):
        j = idx[a + 1] if a + 1 < len(idx) else len(fr)
        nd.append(sum(du[i:j]))
    return [fr[i] for i in idx], nd


def labelled(frames, labels, right=False):
    return [label(f.copy(), t, right) for f, t in zip(frames, labels)] if labels else frames


def encode(frames, durations, maxb, min_frames, labels=None, right=False):
    """Lossless animated WebP under maxb bytes: drop frames down to min_frames, then crop the frame
    towards its centre (never rescale, which would blur the tiles). Labels are drawn last."""
    fr, du, lb = frames, durations, labels
    data = webp_bytes(labelled(fr, lb, right), du)
    steps = []
    while len(data) > maxb and len(fr) > min_frames:
        n = max(min_frames, int(len(fr) * maxb * 0.92 / len(data)))
        if n >= len(fr):
            n = len(fr) - 1
        fr, du = subsample(frames, durations, n)
        if labels:
            lb, _ = subsample(labels, durations, n)
        data = webp_bytes(labelled(fr, lb, right), du)
        steps.append(f"frames->{len(fr)}")
    scale = 1.0
    full = fr
    while len(data) > maxb and scale > 0.75:
        scale -= 0.05
        W, H = int(full[0].width * scale) // 2 * 2, int(full[0].height * scale) // 2 * 2
        x0, y0 = (full[0].width - W) // 2, (full[0].height - H) // 2
        fr = [f.crop((x0, y0, x0 + W, y0 + H)) for f in full]
        data = webp_bytes(labelled(fr, lb, right), du)
        steps.append(f"crop {W}x{H}")
    if len(data) > maxb:
        steps.append("OVER SIZE CAP")
    return data, labelled(fr, lb, right), steps

def save_still(im, path, scene, gen):
    meta = PngImagePlugin.PngInfo()
    meta.add_text("Software", "JETCARP docs/tools/anim/animate.py")
    meta.add_text("Scene", os.path.relpath(scene, jc.REPO))
    meta.add_text("Generation", str(gen))
    im.save(path, "PNG", pnginfo=meta, optimize=True)


def trail(bg, frames, picks=5):
    """A still of a mover: several of its positions over the empty tiling, older ones fainter."""
    base = np.asarray(bg, dtype=np.float32).copy()
    ref = np.asarray(bg, dtype=np.int16)
    n = len(frames)
    idx = sorted(set(round(i * (n - 1) / (picks - 1)) for i in range(picks)))
    for j, i in enumerate(idx):
        alpha = 0.18 + 0.82 * (j / max(1, len(idx) - 1)) ** 1.5
        f = np.asarray(frames[i], dtype=np.int16)
        m = np.abs(f - ref).max(axis=2) > 12
        base[m] = (1 - alpha) * base[m] + alpha * f[m]
    return Image.fromarray(base.clip(0, 255).astype(np.uint8))


def load_rgb(paths):
    return [Image.open(p).convert("RGB") for p in paths]


def first_ca(sc):
    cas = list(sc.get("cas", []))
    for cmd in sc.get("commands", []) or []:
        if cmd.get("op") == "add_ca":
            cas.append(cmd.get("ca", {}))
    return cas[0] if cas else {}


def probe(scene, wd, gens, side=1000):
    info, frs = jc.render(scene, gens, side, wd + "/probe", outlines=False, bg=True, threads=THREADS)
    wm = jc.world_metrics(wd + "/probe", info["status"]["cells"])
    bga = jc.arr(wd + "/probe/bg.png").astype(np.int16)
    return info, frs, wm, bga, side


def outlines_for(a, tile_px):
    o = a.get("outlines", "auto")
    return tile_px >= 6 if o == "auto" else bool(o)


# ---------------------------------------------------------------- kinds

def do_evo(a, scene, wd, sc):
    G, N, gamma = a["gens"], a.get("frames", 72), a.get("ease", 1.0)
    W = a.get("width", 400)
    info, _, wm, _, side = probe(scene, wd, [0])
    t0 = wm["tile"]
    sx0, sy0, sx1, sy1 = wm["box"]
    sw, sh = sx1 - sx0, sy1 - sy0
    if a.get("frame", "whole") == "whole":
        k = min(W / sw, a.get("max_height", 400) / sh)
        Wo, Ho = int(round(sw * k)), int(round(sh * k))
    else:
        k = a.get("tile_px", 9) / t0
        Wo, Ho = W, a.get("height", int(W * 0.75))
    Wo, Ho = Wo // 2 * 2, Ho // 2 * 2
    outl = outlines_for(a, t0 * k)
    fx, fy = a.get("centre_at", [0.5, 0.5])
    spg = first_ca(sc).get("steps_per_gen", 1) if a.get("label") == "step" else 1

    start = a.get("start", 0)

    def run(G):
        gens_f = [start + g for g in eased_gens(G, N, gamma)]
        info2, frs2 = jc.render(scene, gens_f, side * k, wd + "/final", outlines=outl, bg=True, threads=THREADS)
        return gens_f, info2, frs2

    gens_f, info2, frs2 = run(G)
    da = info2.get("done_at")
    if da is not None and da - start < 0.75 * G and not a.get("keep_gens"):
        gens_f, info2, frs2 = run(int((da - start) * 1.08) + 4)
    im0 = Image.open(frs2[0])
    kx, ky = im0.width / wm["size"][0], im0.height / wm["size"][1]
    cx, cy = (sx0 + fx * sw) * kx, (sy0 + fy * sh) * ky
    oc = outline_colour(wd + "/final/bg.png", wm["stage"], wm["outside"]) if outl else None
    frames = brighten([crop_fixed(im, cx, cy, Wo, Ho, wm["outside"]) for im in load_rgb(frs2)], oc, wm["stage"])
    frames, gk = trim_static(frames, list(gens_f))
    word = a.get("label", "gen")
    labels = [f"{word} {g * spg:,}" for g in gk] if word else None
    du = [a.get("ms", 90)] * (len(frames) - 1) + [a.get("hold", 1500)]
    still, still_gen = frames[-1], gk[-1]
    return frames, du, still, still_gen, labels, dict(tile_px=round(t0 * kx, 1), gens=gk[-1], outlines=outl)


def window(a, scene, wd, gens_f, aspect):
    """Render gens_f in a window holding all live pixels, at most tile_px px per tile."""
    info, frs, wm, bga, side = probe(scene, wd, gens_f)
    t0 = wm["tile"]
    boxes = [jc.live_bbox(f, bga) for f in frs]
    if not any(boxes):
        raise RuntimeError("no live cells")
    u = union(boxes)
    m = a.get("margin", 2.0) * t0
    u = (u[0] - m, u[1] - m, u[2] + m, u[3] + m)
    mt = a.get("min_tiles", 10) * t0
    win = grow(u, mt, mt, aspect)
    Wr, Hr = win[2] - win[0], win[3] - win[1]
    s = min(a.get("tile_px", 14) / t0, a.get("width", 400) / Wr, a.get("max_height", 360) / Hr)
    outl = outlines_for(a, t0 * s)
    info2, frs2 = jc.render(scene, gens_f, side * s, wd + "/final", outlines=outl, bg=True, threads=THREADS)
    im0 = Image.open(frs2[0])
    kx, ky = im0.width / wm["size"][0], im0.height / wm["size"][1]
    W, H = int(round(Wr * kx)) // 2 * 2, int(round(Hr * ky)) // 2 * 2
    cx, cy = (win[0] + win[2]) / 2 * kx, (win[1] + win[3]) / 2 * ky
    oc = outline_colour(wd + "/final/bg.png", wm["stage"], wm["outside"]) if outl else None
    frames = brighten([crop_fixed(im, cx, cy, W, H, wm["outside"]) for im in load_rgb(frs2)], oc, wm["stage"])
    bg = brighten([crop_fixed(Image.open(wd + "/final/bg.png").convert("RGB"), cx, cy, W, H, wm["outside"])],
                  oc, wm["stage"])[0]
    return frames, bg, dict(tile_px=round(t0 * kx, 1), outlines=outl)


def do_mover(a, scene, wd, sc):
    e = a.get("every", 1)
    s0 = a.get("start", 0)
    gens_f = list(range(s0, s0 + a["gens"] + 1, e))
    frames, bg, meta = window(a, scene, wd, gens_f, a.get("aspect", 0.6))
    labels = [f"gen {g:,}" for g in gens_f] if a.get("label") else None
    du = [a.get("ms", 90)] * (len(frames) - 1) + [a.get("hold", 600)]
    still = trail(bg, frames, a.get("trail", 4))
    return frames, du, still, gens_f[-1], labels, dict(meta, gens=gens_f[-1], every=e)


def do_osc(a, scene, wd, sc):
    p = a["period"]
    gens_f = list(range(a.get("start", 0), a.get("start", 0) + p))
    frames, bg, meta = window(a, scene, wd, gens_f, a.get("aspect", 0.75))
    du = [a.get("ms", max(70, min(250, 2400 // p)))] * len(frames)
    return frames, du, frames[0], gens_f[0], None, dict(meta, period=p)


KINDS = {"evo": do_evo, "mover": do_mover, "osc": do_osc}


# ---------------------------------------------------------------- driver

def one(a):
    aid = a["id"]
    wd = os.path.join(WORK, aid.replace("/", "__"))
    t = time.time()
    try:
        scene = os.path.join(SCENES, a["scene"])
        sc = json.load(open(scene))
        frames, du, still, still_gen, labels, meta = KINDS[a["kind"]](a, scene, wd, sc)
        maxb = int(a.get("max_kb", 360) * 1000)
        data, kept, steps = encode(frames, du, maxb, a.get("min_frames", 40 if a["kind"] == "evo" else 24),
                                   labels, a.get("label_at") == "right")
        nf = len(kept)
        if kept[0].size != still.size:  # the frame was cropped to fit: crop the still the same way
            x0, y0 = (still.width - kept[0].width) // 2, (still.height - kept[0].height) // 2
            still = still.crop((x0, y0, x0 + kept[0].width, y0 + kept[0].height))
        if labels:
            still = label(still.copy(), labels[-1], a.get("label_at") == "right")
        out = os.path.join(IMAGES, aid)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        open(out + ".webp", "wb").write(data)
        save_still(still, out + "-still.png", scene, still_gen)
        shutil.rmtree(wd, ignore_errors=True)
        return dict(id=aid, w=kept[0].width, h=kept[0].height, frames=nf, bytes=len(data),
                    still=os.path.getsize(out + "-still.png"), steps=steps, ms=sum(du),
                    seconds=round(time.time() - t, 1), **meta)
    except Exception as ex:  # noqa: BLE001
        return dict(id=aid, error=f"{type(ex).__name__}: {ex}", trace=traceback.format_exc()[-1500:])


def init(work, threads):
    global WORK, THREADS
    WORK, THREADS = work, threads


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--only", help="render only animations whose id contains this")
    ap.add_argument("--jobs", type=int, default=4, help="animations rendered at once")
    ap.add_argument("--threads", type=int, default=4, help="JETCARP threads per animation")
    ap.add_argument("--work", default=os.path.join(jc.REPO, "target", "docs-anim"),
                    help="folder for the frames (default target/docs-anim)")
    ap.add_argument("--list", action="store_true", help="list the animations and exit")
    a = ap.parse_args()
    if not os.path.exists(jc.JETCARP):
        sys.exit(f"JETCARP binary not found: {jc.JETCARP} (build it, or set JETCARP=...)")
    entries = json.load(open(MANIFEST))["animations"]
    if a.only:
        entries = [e for e in entries if a.only in e["id"]]
    if a.list:
        for e in entries:
            print(f"{e['id']:44} {e['kind']:6} {e['scene']}")
        return
    os.makedirs(a.work, exist_ok=True)
    total, failed = 0, 0
    with Pool(a.jobs, initializer=init, initargs=(a.work, a.threads)) as pool:
        for r in pool.imap_unordered(one, entries):
            if "error" in r:
                failed += 1
                print(f"FAILED {r['id']}: {r['error'][:300]}\n{r['trace']}", flush=True)
                continue
            total += r["bytes"] + r["still"]
            print(f"{r['id']:44} {r['w']:4}x{r['h']:<4} {r['frames']:3} frames {r['ms'] / 1000:5.1f} s "
                  f"{r['bytes'] / 1000:6.0f} KB  still {r['still'] / 1000:4.0f} KB  tile {r['tile_px']} px"
                  f"{'  ' + ', '.join(r['steps']) if r['steps'] else ''}", flush=True)
    print(f"{len(entries) - failed} animations, {total / 1e6:.2f} MB with stills" +
          (f", {failed} FAILED" if failed else ""))
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
