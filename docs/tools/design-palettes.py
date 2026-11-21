#!/usr/bin/env python3
"""Design JETCARP's palettes for colour-vision deficiency (#217): the ramps behind the built-in
palettes marked "passes" in the palettes reference, and a check that mirrors the program's own.

Each palette is a ramp in OKLCH (Ottosson's OKLab, in polar form): its stops run evenly from one
lightness, chroma and hue to another (or through the points given), and a colour outside sRGB loses
chroma until it fits. The check is `jetcarp_core::cvd`'s, step for step: Machado, Oliveira and
Fernandes' (2009) matrices on linear sRGB (protanopia, deuteranopia and tritanopia at severity 1,
their milder forms at 0.5), CIELAB with a D65 white, and CIEDE2000; neighbouring stops, and each
stop against the background, at least 10 apart under all seven ways of seeing. JACA's pairs also
keep every stop of one side at least 10 from every stop of the other, and from the glow of
annihilated cells. The program's check (`jetcarp palettes --json`, the `palettes_cvd` tests) is the
judge; this script only designs, and prints the palettes as Rust for `render.rs`.

Standard library only. Usage: python3 docs/tools/design-palettes.py [--rust] [--only NAME]
"""
import math
import sys

# --------------------------------------------------------------------------------------------
# Colour

def to_linear(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def from_linear(x):
    x = min(max(x, 0.0), 1.0)
    v = 12.92 * x if x <= 0.0031308 else 1.055 * x ** (1 / 2.4) - 0.055
    return round(v * 255)


def oklab_to_linear(L, a, b):
    l_ = L + 0.3963377774 * a + 0.2158037573 * b
    m_ = L - 0.1055613458 * a - 0.0638541728 * b
    s_ = L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_ ** 3, m_ ** 3, s_ ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def oklch(L, C, H):
    """An sRGB colour from OKLCH (L 0-1, C, H in degrees); chroma is reduced until it fits."""

    def at(c):
        a, b = c * math.cos(math.radians(H)), c * math.sin(math.radians(H))
        rgb = oklab_to_linear(L, a, b)
        return rgb if all(-1e-4 <= v <= 1 + 1e-4 for v in rgb) else None

    best = at(C)
    if best is None:
        lo, hi = 0.0, C
        best = at(0.0)
        for _ in range(40):
            mid = (lo + hi) / 2
            got = at(mid)
            if got is None:
                hi = mid
            else:
                lo, best = mid, got
    return tuple(from_linear(v) for v in best)


MACHADO = {
    "protanopia": [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
    "deuteranopia": [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
    "tritanopia": [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]],
    "protanomaly": [[0.458064, 0.679578, -0.137642], [0.092785, 0.846313, 0.060902], [-0.007494, -0.016807, 1.024301]],
    "deuteranomaly": [[0.547494, 0.607765, -0.155259], [0.181692, 0.781742, 0.036566], [-0.010410, 0.027275, 0.983136]],
    "tritanomaly": [[1.017277, 0.027029, -0.044306], [-0.006113, 0.958479, 0.047634], [0.006379, 0.248708, 0.744913]],
}
VISIONS = ["normal"] + list(MACHADO)


def lab_of_linear(r, g, b):
    x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b
    y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b
    z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b
    d = 6 / 29

    def f(t):
        return t ** (1 / 3) if t > d ** 3 else t / (3 * d * d) + 4 / 29

    fx, fy, fz = f(x / 0.95047), f(y), f(z / 1.08883)
    return (116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz))


def seen(rgb, vision):
    lin = [to_linear(c) for c in rgb]
    if vision != "normal":
        m = MACHADO[vision]
        lin = [min(max(sum(m[k][i] * lin[i] for i in range(3)), 0.0), 1.0) for k in range(3)]
    return lab_of_linear(*lin)


def ciede2000(p, q):
    l1, a1, b1 = p
    l2, a2, b2 = q
    c1, c2 = math.hypot(a1, b1), math.hypot(a2, b2)
    cb7 = ((c1 + c2) / 2) ** 7
    g = 0.5 * (1 - math.sqrt(cb7 / (cb7 + 25 ** 7)))
    a1p, a2p = (1 + g) * a1, (1 + g) * a2
    c1p, c2p = math.hypot(a1p, b1), math.hypot(a2p, b2)

    def hue(ap, b):
        return 0.0 if ap == 0 and b == 0 else math.degrees(math.atan2(b, ap)) % 360

    h1p, h2p = hue(a1p, b1), hue(a2p, b2)
    dl, dc = l2 - l1, c2p - c1p
    if c1p * c2p == 0:
        dh = 0.0
    else:
        d = h2p - h1p
        dh = d - 360 if d > 180 else d + 360 if d < -180 else d
    dhh = 2 * math.sqrt(c1p * c2p) * math.sin(math.radians(dh / 2))
    lb, cbp = (l1 + l2) / 2, (c1p + c2p) / 2
    if c1p * c2p == 0:
        hb = h1p + h2p
    elif abs(h1p - h2p) <= 180:
        hb = (h1p + h2p) / 2
    elif h1p + h2p < 360:
        hb = (h1p + h2p + 360) / 2
    else:
        hb = (h1p + h2p - 360) / 2
    t = (1 - 0.17 * math.cos(math.radians(hb - 30)) + 0.24 * math.cos(math.radians(2 * hb))
         + 0.32 * math.cos(math.radians(3 * hb + 6)) - 0.20 * math.cos(math.radians(4 * hb - 63)))
    dtheta = 30 * math.exp(-(((hb - 275) / 25) ** 2))
    rc = 2 * math.sqrt(cbp ** 7 / (cbp ** 7 + 25 ** 7))
    sl = 1 + 0.015 * (lb - 50) ** 2 / math.sqrt(20 + (lb - 50) ** 2)
    sc = 1 + 0.045 * cbp
    sh = 1 + 0.015 * cbp * t
    rt = -math.sin(math.radians(2 * dtheta)) * rc
    x, y, z = dl / sl, dc / sc, dhh / sh
    return math.sqrt(x * x + y * y + z * z + rt * y * z)


def least(stops, bg):
    """The smallest difference between neighbouring stops, or a stop and the background."""
    worst = (math.inf, "")
    for v in VISIONS:
        labs = [seen(s, v) for s in stops]
        b = seen(bg, v)
        for i in range(len(labs) - 1):
            d = ciede2000(labs[i], labs[i + 1])
            if d < worst[0]:
                worst = (d, f"stops {i + 1}-{i + 2} {v}")
        for i, l in enumerate(labs):
            d = ciede2000(l, b)
            if d < worst[0]:
                worst = (d, f"stop {i + 1}-bg {v}")
    return worst


def between(a, b):
    worst = (math.inf, "")
    for v in VISIONS:
        lb = [seen(s, v) for s in b]
        for i, x in enumerate(a):
            lx = seen(x, v)
            for j, y in enumerate(lb):
                d = ciede2000(lx, y)
                if d < worst[0]:
                    worst = (d, f"{i + 1}x{j + 1} {v}")
    return worst


def hexrgb(h):
    return ((h >> 16) & 255, (h >> 8) & 255, h & 255)


# Lightness (OKLab L) of each stop, by ground and number of stops: CIEDE2000 weighs a step in
# lightness less towards white and black than in the middle, so the steps there are larger.
def _ok(lstar):
    """OKLab lightness for a CIELAB L* (exact for greys: L* = 116 L - 16)."""
    return [(x + 16) / 116 for x in lstar]


SCHEDULES = {
    ("dark", 5): _ok([91, 75, 59, 44, 30]),
    ("dark", 4): _ok([96, 74, 54, 34]),
    ("deep", 6): _ok([93, 78, 64, 51, 38, 26]),
    ("paper", 5): _ok([12, 29, 46, 62, 79]),
    ("paper", 4): _ok([14, 36, 57, 78]),
    ("jaca-hi", 4): _ok([84, 68, 52, 37]),
    ("jaca-lo", 4): _ok([78, 62, 46, 31]),
}


def ramp(points, n, ground=None, lstar=None):
    """`n` stops through OKLCH `points` [(L, C, H), ...], evenly in t; with a `ground`, each stop's
    lightness comes from SCHEDULES instead, and the points give only chroma and hue."""
    out = []
    segs = len(points) - 1
    sched = _ok(lstar) if lstar else SCHEDULES.get((ground, n)) if ground else None
    for i in range(n):
        t = i / (n - 1) * segs
        k = min(int(t), segs - 1)
        f = t - k
        (l0, c0, h0), (l1, c1, h1) = points[k], points[k + 1]
        l = sched[i] if sched else l0 + (l1 - l0) * f
        out.append(oklch(l, c0 + (c1 - c0) * f, h0 + (h1 - h0) * f))
    return out


# --------------------------------------------------------------------------------------------
# The palettes. Stops run young (or low) first. Lightness carries each ramp, at least 12 L* a step;
# hue and chroma give it its character.

BG, ST = 0x1D2029, 0x101217          # JETCARP's dark slate
DEEP, DEEP_ST = 0x0E1118, 0x07090C   # JACA's backdrop (Cobalt and Saffron's)
PAPER, PAPER_ST = 0xF6F3EA, 0xE4DFD2 # a light ground
GLOW = (0xFF, 0xF3, 0xC4)

# name: (points, stops, background, stage, note)
PALETTES = [
    # --- On the dark slate, light to dark: perceptually even ramps, several hues each
    ("Dusk", [(0.97, 0.10, 100), (0.78, 0.16, 55), (0.61, 0.17, 15), (0.45, 0.15, 295), (0.31, 0.13, 265)], 5, BG, ST, [97, 78, 60, 46, 30]),
    ("Furnace", [(0.97, 0.09, 100), (0.78, 0.17, 65), (0.58, 0.21, 30), (0.38, 0.14, 15)], 5, BG, ST),
    ("Kelp", [(0.95, 0.15, 115), (0.76, 0.16, 145), (0.56, 0.11, 175), (0.38, 0.07, 205)], 5, BG, ST, [93, 72, 57, 43, 30]),
    ("Abyss", [(0.95, 0.07, 195), (0.75, 0.12, 220), (0.55, 0.15, 255), (0.38, 0.15, 280)], 5, BG, ST),
    ("Rosewood", [(0.95, 0.05, 60), (0.78, 0.11, 30), (0.60, 0.15, 5), (0.40, 0.12, -15)], 5, BG, ST),
    ("Harbour", [(0.95, 0.12, 100), (0.78, 0.07, 95), (0.62, 0.02, 240), (0.48, 0.07, 255), (0.36, 0.10, 260)], 5, BG, ST),
    ("Lagoon", [(0.96, 0.08, 185), (0.80, 0.12, 185), (0.64, 0.12, 195), (0.49, 0.10, 210), (0.37, 0.08, 225)], 5, BG, ST, [91, 73, 58, 44, 30]),
    # --- On the dark slate, one hue
    ("Heather", [(0.95, 0.04, 305), (0.37, 0.15, 300)], 5, BG, ST),
    ("Graphite", [(0.97, 0.0, 0), (0.38, 0.0, 0)], 5, BG, ST),
    ("Honey", [(0.96, 0.09, 95), (0.80, 0.15, 80), (0.62, 0.13, 65), (0.38, 0.08, 55)], 5, BG, ST),
    ("Spruce", [(0.95, 0.07, 150), (0.38, 0.09, 160)], 5, BG, ST, [91, 74, 58, 44, 30]),
    ("Fjord", [(0.96, 0.03, 230), (0.38, 0.06, 245)], 5, BG, ST),
    ("Copper", [(0.95, 0.05, 65), (0.75, 0.12, 50), (0.56, 0.13, 40), (0.38, 0.09, 35)], 5, BG, ST),
    ("Peony", [(0.95, 0.05, 350), (0.38, 0.15, 345)], 5, BG, ST),
    ("Sulphur", [(0.96, 0.15, 108), (0.38, 0.08, 115)], 5, BG, ST),
    ("Frost", [(0.97, 0.03, 225), (0.38, 0.13, 260)], 5, BG, ST),
    ("Sepia", [(0.95, 0.04, 80), (0.38, 0.05, 60)], 5, BG, ST),
    # --- Longer ramps, on a deeper ground
    ("Midnight Garden", [(0.96, 0.12, 110), (0.80, 0.14, 150), (0.64, 0.12, 190), (0.48, 0.13, 255), (0.33, 0.12, 285)], 6, 0x0B0C10, 0x050608, [97, 76, 62, 50, 38, 24]),
    ("Lava Flow", [(0.97, 0.08, 100), (0.84, 0.15, 85), (0.70, 0.19, 55), (0.56, 0.20, 30), (0.43, 0.17, 20)], 5, 0x0B0C10, 0x050608, [97, 80, 64, 50, 37]),
    ("Nebula", [(0.96, 0.06, 200), (0.80, 0.10, 240), (0.65, 0.15, 290), (0.51, 0.18, 330), (0.39, 0.15, 350), (0.29, 0.10, 360)], 6, 0x0B0C10, 0x050608, [94, 79, 65, 52, 39, 26]),
    # --- On paper, dark to light
    ("Vellum", [(0.30, 0.10, 260), (0.88, 0.04, 235)], 5, PAPER, PAPER_ST),
    ("Linen", [(0.30, 0.06, 50), (0.88, 0.04, 75)], 5, PAPER, PAPER_ST),
    ("Ink Wash", [(0.22, 0.0, 0), (0.86, 0.0, 0)], 5, PAPER, PAPER_ST),
    ("Bluebell", [(0.32, 0.14, 285), (0.88, 0.05, 300)], 5, PAPER, PAPER_ST),
    ("Fern", [(0.30, 0.08, 150), (0.88, 0.08, 135)], 5, PAPER, PAPER_ST),
    ("Brick", [(0.32, 0.12, 30), (0.88, 0.05, 40)], 5, PAPER, PAPER_ST),
    ("Mulberry", [(0.30, 0.12, 345), (0.88, 0.05, 350)], 5, PAPER, PAPER_ST),
    ("Driftwood", [(0.30, 0.08, 255), (0.55, 0.02, 200), (0.88, 0.07, 90)], 5, PAPER, PAPER_ST),
]

# Closed wheels for cyclic rules: down in lightness through cool hues, up through warm ones, and
# back to the first stop, so neighbours differ in lightness and stops of one lightness differ in
# blue against orange, which every deficiency keeps.
WHEELS = [
    ("Tidewheel", [(90, 0.06, 200), (73, 0.11, 235), (56, 0.14, 270), (40, 0.13, 320), (56, 0.15, 40), (73, 0.14, 75)], 0x0B0C10, 0x050608),
    ("Compass Rose", [(88, 0.05, 90), (71, 0.13, 60), (55, 0.16, 25), (40, 0.14, 330), (55, 0.13, 260), (71, 0.10, 225)], 0x0B0C10, 0x050608),
]

# JACA's pairs: +1 and -1, on JACA's deep ground, apart from each other and from the glow.
PAIRS = [
    # Cool against warm: what every deficiency keeps.
    (("Indigo", [(0.80, 0.09, 255), (0.36, 0.15, 270)], 4), ("Marmalade", [(0.86, 0.14, 80), (0.45, 0.14, 40)], 4)),
    (("Sky", [(0.86, 0.08, 230), (0.40, 0.11, 245)], 4), ("Rust", [(0.80, 0.10, 55), (0.40, 0.11, 35)], 4)),
    (("Iris", [(0.84, 0.07, 285), (0.40, 0.17, 290)], 4), ("Gold", [(0.80, 0.14, 95), (0.45, 0.10, 85)], 4, [80, 64, 48, 33])),
    (("Lapis", [(0.78, 0.12, 265), (0.34, 0.16, 280)], 4), ("Amber", [(0.88, 0.15, 90), (0.50, 0.12, 60)], 4)),
    (("Ice", [(0.88, 0.08, 230), (0.44, 0.10, 245)], 4), ("Ember Red", [(0.72, 0.17, 50), (0.38, 0.14, 25)], 4)),
    (("Delft", [(0.82, 0.07, 250), (0.38, 0.12, 260)], 4), ("Ochre", [(0.84, 0.12, 85), (0.45, 0.10, 70)], 4)),
]


GROUNDS = {BG: "dark", 0x0B0C10: "deep", PAPER: "paper"}


def build():
    out = []
    for name, pts, n, bg, st, *own in PALETTES:
        out.append((name, ramp(pts, n, GROUNDS.get(bg), own[0] if own else None), bg, st, None))
    for name, pts, bg, st in WHEELS:
        stops = [oklch((l + 16) / 116, c, h) for l, c, h in pts]
        out.append((name, stops + [stops[0]], bg, st, None))
    for (pn, pp, pn_n, *pl), (nn, np_, nn_n, *nl) in PAIRS:
        a = ramp(pp, pn_n, "jaca-hi", pl[0] if pl else None)
        b = ramp(np_, nn_n, "jaca-lo", nl[0] if nl else None)
        out.append((pn, a, DEEP, DEEP_ST, nn))
        out.append((nn, b, DEEP, DEEP_ST, pn))
    return out


def lightness(rgb):
    return seen(rgb, "normal")[0]


def main():
    only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
    rust = "--rust" in sys.argv
    pals = build()
    by = {p[0]: p for p in pals}
    bad = 0
    for name, stops, bg, st, partner in pals:
        if only and name != only:
            continue
        d, where = least(stops, hexrgb(bg))
        ls = [lightness(s) for s in stops]
        steps = [ls[i] - ls[i + 1] for i in range(len(ls) - 1)]
        wheel = len(stops) > 2 and stops[0] == stops[-1]
        steady = wheel or all(s >= 12 for s in steps) or all(-s >= 12 for s in steps)
        extra = ""
        ok = d >= 10.5 and steady
        if partner:
            w = between(stops, by[partner][1])
            g = between(stops, [GLOW])
            extra = f"  vs {partner} {w[0]:.1f} ({w[1]})  glow {g[0]:.1f}"
            ok = ok and w[0] >= 11 and g[0] >= 10.5
        bad += not ok
        hexes = ", ".join(f"0x{r:02x}{g:02x}{b:02x}" for r, g, b in stops)
        if rust:
            print(f'    pal("{name}", &[{hexes}], 0x{bg:06x}, 0x{st:06x}),')
        else:
            mark = "ok  " if ok else "FAIL"
            print(f"{mark} {name:16} least {d:5.1f} ({where}) steady={steady} L={[round(x) for x in ls]}{extra}")
    if not rust:
        print(f"{len(pals)} palettes, {len(pals) - bad} pass")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
