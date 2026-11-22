#!/usr/bin/env python3
"""JETCARP's app icon: every raster of it, made from its master, packaging/icons/jetcarp.svg.

    python3 packaging/gen-icons.py           make them all
    python3 packaging/gen-icons.py --check   make them in a scratch folder, and fail if the files
                                             in the repository differ

Python 3.8 or newer, standard library only, and rsvg-convert (librsvg: `librsvg2-tools` on Fedora,
`librsvg2-bin` on Debian and Ubuntu, `librsvg` in Homebrew), or the program `RSVG_CONVERT` names.

Nothing is drawn by hand but the masters: to change the icon, change the SVG and run this. A
second, simpler drawing for the small sizes may sit beside it as packaging/icons/jetcarp-small.svg;
when it is there, the sizes up to SMALL_MAX (32 px) are rendered from it instead.

What it makes, in packaging/icons/:

- jetcarp-N.png for N = 16, 24, 32, 48, 64, 128, 256, 512 and 1024, each rendered from a master
  by rsvg-convert at that size (nothing is scaled from another raster);
- jetcarp.ico, for Windows: the PNGs from 16 to 256, each an entry of its own;
- jetcarp.icns, for macOS: the PNGs at the sizes and types macOS reads, 16 to 1024 and twice 16
  to 512 for Retina screens.

Both containers are written here, byte by byte, from the PNGs: each format can hold PNG entries,
so nothing else is needed. And copies of what the program embeds, since a crate published to
crates.io can only read files inside its own folder:

- crates/jetcarp-display/icons/: the PNGs from 16 to 512 (the Linux menu's icons, and the
  windows' icon) and jetcarp.icns (the macOS app bundle's);
- crates/jetcarp/jetcarp.ico: the Windows program's icon, which its build script embeds;
- docs/src/images/jetcarp-icon.png: the 256 px PNG, for the book.

--check renders the PNGs again and compares their pixels with the files'. librsvg's versions can
differ in a pixel's last bits of antialiasing, so a PNG passes when its pixels are the same or
nearly so (MEAN_TOLERANCE, FAR_TOLERANCE below), which a changed drawing is not. Everything else
(the containers, from the PNGs in the repository, and the copies) must match byte for byte.
"""

import argparse
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "packaging" / "icons"
MASTER = ICONS / "jetcarp.svg"
# The small sizes' own drawing, when there is one: used up to SMALL_MAX px.
SMALL_MASTER = ICONS / "jetcarp-small.svg"
SMALL_MAX = 32
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
# (type, pixels): the PNG-holding types of the .icns format, as Apple's iconutil names them.
ICNS_TYPES = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),  # 512 at 2x
    (b"ic11", 32),  # 16 at 2x
    (b"ic12", 64),  # 32 at 2x
    (b"ic13", 256),  # 128 at 2x
    (b"ic14", 512),  # 256 at 2x
]
# Copies of the files the program and the book use: (where, from packaging/icons).
COPIES = [(f"crates/jetcarp-display/icons/jetcarp-{n}.png", f"jetcarp-{n}.png") for n in SIZES if n <= 512] + [
    ("crates/jetcarp-display/icons/jetcarp.icns", "jetcarp.icns"),
    ("crates/jetcarp/jetcarp.ico", "jetcarp.ico"),
    ("docs/src/images/jetcarp-icon.png", "jetcarp-256.png"),
]
# --check: a PNG rendered again may differ from the file's by at most this much on average, per
# pixel, in its largest channel difference (of 255; colours weighted by alpha)...
MEAN_TOLERANCE = 1.0
# ...and only this share of its pixels may differ by more than FAR (a quarter of the range).
FAR = 64
FAR_TOLERANCE = 0.002


def die(msg):
    print(f"gen-icons.py: {msg}", file=sys.stderr)
    sys.exit(1)


def rsvg():
    exe = os.environ.get("RSVG_CONVERT") or shutil.which("rsvg-convert")
    if not exe:
        die(
            "rsvg-convert was not found: install librsvg (librsvg2-tools on Fedora, librsvg2-bin on "
            "Debian and Ubuntu, librsvg in Homebrew), or name it in RSVG_CONVERT"
        )
    return exe


def master_for(size):
    """The SVG a size is rendered from: the small drawing up to SMALL_MAX px if it is there."""
    return SMALL_MASTER if size <= SMALL_MAX and SMALL_MASTER.is_file() else MASTER


def render(svg, size, out):
    r = subprocess.run(
        [rsvg(), "--width", str(size), "--height", str(size), "--format", "png", "--output", str(out), str(svg)],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        die(f"rsvg-convert failed at {size} px: {r.stderr.strip()}")


def read_png(data):
    """(width, height, RGBA bytes) of an 8-bit PNG: greyscale, RGB, palette-free, with or without
    alpha, not interlaced (what rsvg-convert writes is 8-bit RGBA)."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    at, idat, ihdr = 8, [], None
    while at < len(data):
        (n,) = struct.unpack(">I", data[at : at + 4])
        kind, body = data[at + 4 : at + 8], data[at + 8 : at + 8 + n]
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
        at += 12 + n
    w, h, depth, color, _, _, interlace = ihdr
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
    if depth != 8 or channels is None or interlace:
        raise ValueError(f"unsupported PNG (depth {depth}, colour type {color}, interlace {interlace})")
    raw = zlib.decompress(b"".join(idat))
    stride = w * channels
    out = bytearray(h * stride)
    prev = bytearray(stride)
    for y in range(h):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        if f == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        elif f != 0:
            raise ValueError(f"bad PNG filter {f}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    if channels == 4:
        return w, h, bytes(out)
    rgba = bytearray()
    for i in range(0, len(out), channels):
        px = out[i : i + channels]
        if channels == 1:
            rgba += bytes([px[0], px[0], px[0], 255])
        elif channels == 2:
            rgba += bytes([px[0], px[0], px[0], px[1]])
        else:
            rgba += bytes([px[0], px[1], px[2], 255])
    return w, h, bytes(rgba)


def png_size(data):
    w, h = struct.unpack(">II", data[16:24])
    return w, h


def ico(pngs):
    """A .ico of PNG entries: ICONDIR, one ICONDIRENTRY per image (256 is written as 0), then the
    images. pngs: {pixels: PNG bytes}."""
    sizes = [n for n in ICO_SIZES if n in pngs]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = len(head) + 16 * len(sizes)
    entries, images = b"", b""
    for n in sizes:
        data = pngs[n]
        if png_size(data) != (n, n):
            die(f"the {n} px PNG is {png_size(data)}")
        b = 0 if n == 256 else n
        entries += struct.pack("<BBBBHHII", b, b, 0, 0, 1, 32, len(data), offset + len(images))
        images += data
    return head + entries + images


def icns(pngs):
    """An .icns of PNG entries: 'icns' and the file's length, a table of contents, then each
    entry: its type, its length with the 8 bytes before it, and the PNG."""
    chunks = []
    for kind, n in ICNS_TYPES:
        data = pngs[n]
        if png_size(data) != (n, n):
            die(f"the {n} px PNG is {png_size(data)}")
        chunks.append((kind, data))
    toc = b"".join(kind + struct.pack(">I", 8 + len(d)) for kind, d in chunks)
    body = b"TOC " + struct.pack(">I", 8 + len(toc)) + toc
    body += b"".join(kind + struct.pack(">I", 8 + len(d)) + d for kind, d in chunks)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def make(out_root):
    """Writes every file under out_root, laid out as in the repository. Returns their paths there,
    relative to it."""
    icons = out_root / "packaging" / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    pngs = {}
    made = []
    for n in SIZES:
        p = icons / f"jetcarp-{n}.png"
        render(master_for(n), n, p)
        pngs[n] = p.read_bytes()
        made.append(p.relative_to(out_root))
    for name, data in [("jetcarp.ico", ico(pngs)), ("jetcarp.icns", icns(pngs))]:
        (icons / name).write_bytes(data)
        made.append((icons / name).relative_to(out_root))
    for dst, src in COPIES:
        d = out_root / dst
        d.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(icons / src, d)
        made.append(Path(dst))
    return made


def compare_pixels(a, b):
    """(mean, share far): how far apart two PNGs' pixels are (module docstring)."""
    wa, ha, pa = read_png(a)
    wb, hb, pb = read_png(b)
    if (wa, ha) != (wb, hb):
        return None
    total, far = 0, 0
    for i in range(0, len(pa), 4):
        aa, ab = pa[i + 3], pb[i + 3]
        d = abs(aa - ab)
        for c in range(3):
            d = max(d, abs(pa[i + c] * aa - pb[i + c] * ab) // 255)
        total += d
        if d > FAR:
            far += 1
    n = wa * ha
    return total / n, far / n


def check():
    problems = []
    with tempfile.TemporaryDirectory(prefix="jetcarp-icons-") as tmp:
        fresh = Path(tmp)
        make(fresh)
        version = subprocess.run([rsvg(), "--version"], capture_output=True, text=True).stdout.split("\n")[0]
        committed = {}
        for n in SIZES:
            rel = Path("packaging/icons") / f"jetcarp-{n}.png"
            have = ROOT / rel
            if not have.is_file():
                problems.append(f"{rel} is missing")
                continue
            committed[n] = have.read_bytes()
            new = (fresh / rel).read_bytes()
            if new == committed[n]:
                print(f"ok: {rel} (the same bytes)")
                continue
            try:
                far = compare_pixels(committed[n], new)
            except ValueError as e:
                problems.append(f"{rel}: {e}")
                continue
            if far is None:
                problems.append(f"{rel} is not {n} by {n} pixels")
            elif far[0] > MEAN_TOLERANCE or far[1] > FAR_TOLERANCE:
                problems.append(
                    f"{rel} is not what the master renders to: its pixels differ by {far[0]:.2f} on "
                    f"average, and {100 * far[1]:.2f}% of them by more than {FAR}"
                )
            else:
                print(f"ok: {rel} (pixels within {far[0]:.2f} on average of {version}'s)")
        # The containers, from the PNGs in the repository; the copies, of the files in it.
        if len(committed) == len(SIZES):
            for name, data in [("jetcarp.ico", ico(committed)), ("jetcarp.icns", icns(committed))]:
                rel = Path("packaging/icons") / name
                have = ROOT / rel
                if not have.is_file() or have.read_bytes() != data:
                    problems.append(f"{rel} is not made of the PNGs beside it")
                else:
                    print(f"ok: {rel}")
        for dst, src in COPIES:
            have, want = ROOT / dst, ICONS / src
            if not have.is_file() or not want.is_file() or have.read_bytes() != want.read_bytes():
                problems.append(f"{dst} is not a copy of packaging/icons/{src}")
            else:
                print(f"ok: {dst}")
    if problems:
        for p in problems:
            print(f"gen-icons.py: {p}", file=sys.stderr)
        die("the icons are out of date: run python3 packaging/gen-icons.py")
    small = f" (and {SMALL_MASTER.name} up to {SMALL_MAX} px)" if SMALL_MASTER.is_file() else ""
    print(f"ok: every icon file matches packaging/icons/jetcarp.svg{small}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if the files in the repository are out of date")
    a = ap.parse_args()
    if not MASTER.is_file():
        die(f"{MASTER} is missing")
    if a.check:
        check()
    else:
        for p in make(ROOT):
            print(p.as_posix())


if __name__ == "__main__":
    main()
