"""Generate JETCARP's isohedral tiling tables from Craig S. Kaplan's tactile-js.

tactile-js (https://github.com/isohedral/tactile-js) is BSD-3-Clause, Copyright (c) 2018 Craig S.
Kaplan. This script reads its `lib/tactile.js` as text (it does not run it), keeps the data tables
for the 81 non-degenerate isohedral types, and writes them as Rust. It also works out each type's
wallpaper group numerically (generic parameters and generic edge shapes), since tactile does not
list them. Run:
    python3 tools/gen_isohedral.py path/to/tactile.js > crates/jetcarp-core/src/isohedral/data.rs
"""
import math, random, re, sys

src = open(sys.argv[1]).read()

# ---------------------------------------------------------------- parse

consts = {}
for m in re.finditer(r"const ((?:es|esi|eo|dp|tvc|tc|ac|c)_\d+) = \[(.*?)\];", src, re.S):
    body = m.group(2).strip()
    if not body:
        consts[m.group(1)] = []
        continue
    items = [x.strip() for x in body.split(",") if x.strip()]
    vals = []
    for x in items:
        if x.startswith("EdgeShape."):
            vals.append(x.split(".")[1])
        elif x in ("true", "false"):
            vals.append(x == "true")
        else:
            vals.append(float(x))
    consts[m.group(1)] = vals

table = src[src.index("return [", src.index("const tiling_type_data")):]
types = []
for m in re.finditer(r"// IH(\d+)\s*\n\s*\{(.*?)\}", table, re.S):
    num = int(m.group(1))
    rec = {}
    for k, v in re.findall(r"(\w+)\s*:\s*(\w+)", m.group(2)):
        rec[k] = int(v) if v.isdigit() else consts[v]
    rec["num"] = num
    types.append(rec)
assert len(types) == 81, len(types)
listed = [int(x) for x in re.search(r"const tilingTypes = \[(.*?)\]", src, re.S).group(1).split(",")]
assert [t["num"] for t in types] == listed

# ---------------------------------------------------------------- geometry (as tactile.js)

def make_point(c, off, p):
    n = len(p)
    return (sum(c[off + i] * p[i] for i in range(n)), sum(c[off + n + i] * p[i] for i in range(n)))

def make_matrix(c, off, p):
    n = len(p)
    out = []
    for _ in range(6):
        out.append(sum(c[off + i] * p[i] for i in range(n)))
        off += n
    return out

def mul(A, B):
    return [A[0]*B[0] + A[1]*B[3], A[0]*B[1] + A[1]*B[4], A[0]*B[2] + A[1]*B[5] + A[2],
            A[3]*B[0] + A[4]*B[3], A[3]*B[1] + A[4]*B[4], A[3]*B[2] + A[4]*B[5] + A[5]]

def ap(A, q):
    return (A[0]*q[0] + A[1]*q[1] + A[2], A[3]*q[0] + A[4]*q[1] + A[5])

def match_seg(p, q):
    return [q[0]-p[0], p[1]-q[1], p[0], q[1]-p[1], q[0]-p[0], p[1]]

ORIENTS = [[1, 0, 0, 0, 1, 0], [-1, 0, 1, 0, -1, 0], [-1, 0, 1, 0, 1, 0], [1, 0, 0, 0, -1, 0]]

def instance(t, params):
    p = list(params) + [1.0]
    nv, np_, na = t["num_vertices"], t["num_params"], t["num_aspects"]
    verts = [make_point(t["vertex_coeffs"], i * 2 * (np_ + 1), p) for i in range(nv)]
    asp = [make_matrix(t["aspect_coeffs"], 6 * (np_ + 1) * i, p) for i in range(na)]
    t1 = make_point(t["translation_coeffs"], 0, p)
    t2 = make_point(t["translation_coeffs"], 2 * (np_ + 1), p)
    return verts, asp, t1, t2

def orient(t, e):
    eo = t["edge_orientations"]
    return 2 * int(eo[2 * e]) + int(eo[2 * e + 1])

def decorated(t, verts, shapes):
    """Prototile outline with each edge drawn from its canonical shape (tactile's `shape()`)."""
    pts = []
    nv = len(verts)
    for e in range(nv):
        T = mul(match_seg(verts[e], verts[(e + 1) % nv]), ORIENTS[orient(t, e)])
        o = orient(t, e)
        canon = shapes[int(t["edge_shape_ids"][e])]
        seg = [ap(T, q) for q in canon]
        if o in (1, 2):  # reversed
            seg = seg[::-1]
        pts.extend(seg[:-1])
    return pts

def canonical_shape(kind, rng):
    """A generic edge with only the symmetry its kind requires, from (0,0) to (1,0)."""
    if kind == "I":
        return [(0.0, 0.0), (1.0, 0.0)]
    if kind == "J":
        return [(0.0, 0.0), (0.21, rng.uniform(0.08, 0.2)), (0.63, -rng.uniform(0.05, 0.15)), (1.0, 0.0)]
    a, d = rng.uniform(0.15, 0.3), rng.uniform(0.08, 0.2)
    if kind == "U":
        return [(0.0, 0.0), (a, d), (1 - a, d), (1.0, 0.0)]
    return [(0.0, 0.0), (a, d), (0.5, 0.0), (1 - a, -d), (1.0, 0.0)]  # S

def iso_from(p0, p1, q0, q1, reflect):
    """Isometry with p0->q0 and direction p0p1 -> q0q1 (optionally orientation-reversing)."""
    a = math.atan2(p1[1]-p0[1], p1[0]-p0[0])
    b = math.atan2(q1[1]-q0[1], q1[0]-q0[0])
    if reflect:
        th = b + a
        c, s = math.cos(th), math.sin(th)
        L = [c, s, s, -c]
    else:
        th = b - a
        c, s = math.cos(th), math.sin(th)
        L = [c, -s, s, c]
    x = (L[0]*p0[0] + L[1]*p0[1], L[2]*p0[0] + L[3]*p0[1])
    return [L[0], L[1], q0[0]-x[0], L[2], L[3], q0[1]-x[1]]

def same_set(a, b, tol):
    return all(any(abs(x[0]-y[0]) < tol and abs(x[1]-y[1]) < tol for y in b) for x in a)

def stabilizer(verts, pts):
    out = []
    nv = len(verts)
    scale = max(math.dist(verts[0], v) for v in verts)
    for k in range(nv):
        for step in (1, -1):
            for refl in (False, True):
                g = iso_from(verts[0], verts[1], verts[k], verts[(k + step) % nv], refl)
                if abs(math.dist(verts[0], verts[1]) - math.dist(verts[k], verts[(k + step) % nv])) > 1e-6 * scale:
                    continue
                if same_set([ap(g, q) for q in pts], pts, 1e-6 * scale):
                    out.append(g)
    return out

def lat_coords(v, t1, t2):
    det = t1[0]*t2[1] - t2[0]*t1[1]
    return ((v[0]*t2[1] - t2[0]*v[1]) / det, (t1[0]*v[1] - v[0]*t1[1]) / det)

def is_lattice(v, t1, t2):
    i, j = lat_coords(v, t1, t2)
    return abs(i - round(i)) < 1e-6 and abs(j - round(j)) < 1e-6

def wallpaper(t, seed):
    rng = random.Random(seed)
    params = [x + rng.uniform(-0.02, 0.02) for x in t["default_params"]]
    verts, asp, t1, t2 = instance(t, params)
    shapes = [canonical_shape(k, rng) for k in t["edge_shapes"]]
    pts = decorated(t, verts, shapes)
    stab = stabilizer(verts, pts)
    assert stab, "identity must be in the stabiliser"
    a0inv = inverse(asp[0])
    # Tile k is asp[k] applied to the prototile; symmetries map tile 0 to tile k.
    elems = [mul(mul(A, s), a0inv) for A in asp for s in stab]
    rots, refls = [], []
    for g in elems:
        det = g[0]*g[4] - g[1]*g[3]
        (rots if det > 0 else refls).append(g)
    nrot = len({round(math.degrees(math.atan2(g[3], g[0]))) % 360 for g in rots})
    if not refls:
        return {1: "p1", 2: "p2", 3: "p3", 4: "p4", 6: "p6"}[nrot]
    # Reflection elements over a few lattice translates: mirror (glide in lattice) or true glide.
    lines = []  # mirror lines (point, direction)
    dirs = {}
    for g in refls:
        ux, uy = math.cos(math.atan2(g[3], g[0]) / 2), math.sin(math.atan2(g[3], g[0]) / 2)
        key = round(math.degrees(math.atan2(uy, ux))) % 180
        for i in range(-3, 4):
            for j in range(-3, 4):
                w = (g[2] + i*t1[0] + j*t2[0], g[5] + i*t1[1] + j*t2[1])
                gl = w[0]*ux + w[1]*uy
                gv = (gl*ux, gl*uy)
                d = dirs.setdefault(key, [False, False])
                if is_lattice(gv, t1, t2):
                    d[0] = True
                    w2 = (w[0] - gv[0], w[1] - gv[1])
                    lines.append(((w2[0] / 2, w2[1] / 2), (ux, uy)))
                else:
                    d[1] = True
    mirror = any(d[0] for d in dirs.values())
    glide = any(d[1] for d in dirs.values())
    if nrot == 1:
        return "cm" if mirror and glide else ("pm" if mirror else "pg")
    if nrot == 2:
        if not mirror:
            return "pgg"
        if all(d[0] for d in dirs.values()):
            return "cmm" if glide else "pmm"
        return "pmg"
    if nrot == 6:
        return "p6m"
    # p3m1 / p31m and p4m / p4g: are all n-fold centres on mirror lines?
    n = nrot
    centres = []
    for g in rots:
        ang = round(math.degrees(math.atan2(g[3], g[0]))) % 360
        if ang != 360 // n:
            continue
        for i in range(-2, 3):
            for j in range(-2, 3):
                w = (g[2] + i*t1[0] + j*t2[0], g[5] + i*t1[1] + j*t2[1])
                # Fixed point of x -> Lx + w.
                a, b, c, d = 1 - g[0], -g[1], -g[3], 1 - g[4]
                det = a*d - b*c
                centres.append(((d*w[0] - b*w[1]) / det, (-c*w[0] + a*w[1]) / det))
    def on_mirror(c):
        for (p, u) in lines:
            for i in range(-4, 5):
                for j in range(-4, 5):
                    q = (p[0] + i*t1[0] + j*t2[0], p[1] + i*t1[1] + j*t2[1])
                    if abs((c[0]-q[0])*u[1] - (c[1]-q[1])*u[0]) < 1e-6:
                        return True
        return False
    all_on = all(on_mirror(c) for c in centres)
    if n == 3:
        return "p3m1" if all_on else "p31m"
    return "p4m" if all_on else "p4g"

def inverse(A):
    det = A[0]*A[4] - A[1]*A[3]
    a, b, d, e = A[4]/det, -A[1]/det, -A[3]/det, A[0]/det
    return [a, b, -(a*A[2] + b*A[5]), d, e, -(d*A[2] + e*A[5])]

# ---------------------------------------------------------------- emit

KIND = {"J": "Direct", "U": "Mirror", "S": "SelfHalfTurn", "I": "Straight"}
ORIENT = ["Id", "Rot", "Flip", "Rofl"]
WORD = {"J": "free", "U": "mirror", "S": "half-turn", "I": "straight"}

def f(x):
    s = repr(float(x))
    return s

def floats(xs):
    return "&[" + ", ".join(f(x) for x in xs) + "]"

out = []
w = out.append
w("//! Isohedral tiling types IH01-IH93 (the 81 that tactile supports).")
w("//!")
w("//! Generated by `tools/gen_isohedral.py` from tactile-js `lib/tactile.js`. Do not edit by hand.")
w("//! Data: Copyright (c) 2018 Craig S. Kaplan, BSD-3-Clause (see `crates/jetcarp-core/src/isohedral/LICENSE-tactile`).")
w("//! The wallpaper groups are computed by the generator; tactile does not list them.")
w("")
w("use super::IhType;")
w("use crate::edges::{EdgeClass, GlueKind, Orient};")
w("use crate::families::{Family, FamilyKind};")
w("")
w("#[rustfmt::skip]")
w("pub static IH_TYPES: [IhType; 81] = [")
syms = []
for t in types:
    syms.append(wallpaper(t, t["num"]))
    w("    IhType {")
    w(f"        num: {t['num']},")
    w(f"        np: {t['num_params']},")
    w(f"        na: {t['num_aspects']},")
    w(f"        nv: {t['num_vertices']},")
    w(f"        defaults: {floats(t['default_params'])},")
    w(f"        vertex: {floats(t['vertex_coeffs'])},")
    w(f"        translation: {floats(t['translation_coeffs'])},")
    w(f"        aspect: {floats(t['aspect_coeffs'])},")
    w("    },")
w("];")
w("")
w("#[rustfmt::skip]")
w("pub static IH_FAMILIES: [Family; 81] = [")
for t, sym in zip(types, syms):
    nv = t["num_vertices"]
    ids = [int(x) for x in t["edge_shape_ids"]]
    classes = []
    for ci, kind in enumerate(t["edge_shapes"]):
        es = [e for e in range(nv) if ids[e] == ci]
        os = ", ".join("Orient::" + ORIENT[orient(t, e)] for e in es)
        classes.append(f"EdgeClass {{ edges: &[{', '.join(map(str, es))}], kind: GlueKind::{KIND[kind]}, orient: &[{os}] }}")
    counts = {}
    for k in t["edge_shapes"]:
        counts[k] = counts.get(k, 0) + 1
    parts = [f"{n} {WORD[k]}" for k, n in sorted(counts.items(), key=lambda kv: "JUSI".index(kv[0]))]
    np_ = t["num_params"]
    note = (f"Kaplan's IH{t['num']:02}: {nv} corners, {len(t['edge_shapes'])} edge shape"
            f"{'s' if len(t['edge_shapes']) != 1 else ''} ({', '.join(parts)}), "
            f"{t['num_aspects']} orientation{'s' if t['num_aspects'] != 1 else ''} per unit cell, "
            f"{np_} shape parameter{'s' if np_ != 1 else ''}.")
    w("    Family {")
    w(f"        id: \"ih{t['num']:02}\",")
    w(f"        name: \"Isohedral IH{t['num']:02}\",")
    w("        group: \"Isohedral (Kaplan)\",")
    w(f"        sym: \"{sym}\",")
    w(f"        note: \"{note}\",")
    w("        kind: FamilyKind::Reshapeable,")
    w("        classes: &[")
    for c in classes:
        w(f"            {c},")
    w("        ],")
    w("    },")
w("];")
print("\n".join(out))
from collections import Counter
print(Counter(syms), file=sys.stderr)
print(" ".join(f"IH{t['num']:02}={s}" for t, s in zip(types, syms)), file=sys.stderr)
