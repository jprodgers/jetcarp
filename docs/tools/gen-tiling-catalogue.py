#!/usr/bin/env python3
"""Generate the tiling catalogue pages:

  docs/src/tilings/catalogue.md           tiling families, isohedral types, k-uniform tilings k = 1 to 3
  docs/src/tilings/catalogue-kuniform.md  k-uniform tilings k = 4 to 6 (no thumbnails, to keep the book small)

Inputs: `jetcarp tilings` and `jetcarp isohedral` (neighbour counts as the program computes them) and
crates/jetcarp-core/data/tilings/kuniform.json (Galebach's catalogue in Soto Sanchez's integer form),
from which the edge and touch degrees are computed exactly here: in an edge-to-edge tiling by regular
polygons, an n-gon has n edge neighbours, and its touch (corner) neighbours number
sum over its corners of (polygons at that corner - 1), minus n.
The 1-uniform rows are checked against the program's own counts for the matching families.

Thumbnails (docs/src/images/tilings/thumbs/<id>.png) are drawn by JETCARP itself with
docs/tools/tiling-thumbs.rhai (run by docs/tools/render-figures.sh).

Standard library only. Usage: python3 docs/tools/gen-tiling-catalogue.py [--jetcarp PATH]
"""
import argparse
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
KU = REPO / "crates" / "jetcarp-core" / "data" / "tilings" / "kuniform.json"
OUT = DOCS / "src" / "tilings" / "catalogue.md"
OUT2 = DOCS / "src" / "tilings" / "catalogue-kuniform.md"
THUMBS = DOCS / "src" / "images" / "tilings" / "thumbs"


def wmul(z):
    a, b, c, d = z
    return (-d, a, b + d, c)


UNIT = [(1, 0, 0, 0)]
for _ in range(11):
    UNIT.append(wmul(UNIT[-1]))


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def mul(k, a):
    return tuple(k * x for x in a)


def corners(x, d, n):
    out, p = [tuple(x)], tuple(x)
    for s in range(n - 1):
        p = add(p, UNIT[(d + s * (12 // n)) % 12])
        out.append(p)
    return out


def float_degrees(rec):
    """Same count for a record stored as floating-point polygons (4.8.8, whose octagons are not in Z[w])."""
    a1, a2 = rec["a1"], rec["a2"]
    key = lambda x, y: (round(x * 1e6), round(y * 1e6))
    valence = Counter()
    R = 3
    for i in range(-R, R + 1):
        for j in range(-R, R + 1):
            tx, ty = i * a1[0] + j * a2[0], i * a1[1] + j * a2[1]
            for poly in rec["polys"]:
                for x, y in poly:
                    valence[key(x + tx, y + ty)] += 1
    edge, touch, shapes = Counter(), Counter(), Counter()
    for poly in rec["polys"]:
        n = len(poly)
        edge[n] += 1
        shapes[n] += 1
        touch[sum(valence[key(x, y)] - 1 for x, y in poly) - n] += 1
    return edge, touch, shapes


def kuniform_degrees(rec):
    if "z" not in rec:
        return float_degrees(rec)
    z = rec["z"]
    a1, a2 = tuple(z["a1"]), tuple(z["a2"])
    polys = [(p[:4], p[4], p[5]) for p in z["p"]]
    valence = Counter()
    R = 3
    for i in range(-R, R + 1):
        for j in range(-R, R + 1):
            t = add(mul(i, a1), mul(j, a2))
            for x, d, n in polys:
                for c in corners(add(tuple(x), t), d, n):
                    valence[c] += 1
    edge, touch = Counter(), Counter()
    for x, d, n in polys:
        cs = corners(tuple(x), d, n)
        edge[n] += 1
        touch[sum(valence[c] - 1 for c in cs) - n] += 1
    return edge, touch, Counter(n for _, _, n in polys)


def parse_list(text, pattern):
    rows = []
    for line in text.splitlines():
        m = re.match(pattern, line)
        if m:
            rows.append(m.groupdict())
    return rows


def degrees(s):
    return ", ".join(x.strip() for x in s.split(","))


def thumb(id_, alt):
    p = THUMBS / f"{id_}.png"
    if p.exists():
        return f'<img src="../images/tilings/thumbs/{id_}.png" alt="{alt}" width="120" loading="lazy">'
    return ""


POLY = {3: "triangles", 4: "squares", 6: "hexagons", 8: "octagons", 12: "dodecagons"}


def ku_row(rec, with_thumb):
    edge, touch, shapes = kuniform_degrees(rec)
    total = sum(shapes.values())
    sym = ""
    m = re.search(r"Symmetry (\w+)", rec.get("note", ""))
    if m:
        sym = m.group(1)
    mix = ", ".join(f"{POLY[n]} {shapes[n] / total:.0%}" for n in sorted(shapes))
    mean_touch = sum(k * v for k, v in touch.items()) / total
    e = ", ".join(str(k) for k in sorted(edge))
    t = ", ".join(str(k) for k in sorted(touch))
    cells = [rec["id"], f"`{rec['name']}`", sym, str(total), mix, e, t, f"{mean_touch:.2f}"]
    if with_thumb:
        cells.insert(0, thumb(rec["id"], rec["name"]))
    return "| " + " | ".join(cells) + " |", edge, touch


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jetcarp", default=str(REPO / "target" / "release" / "jetcarp"))
    args = ap.parse_args()
    fam_text = subprocess.run([args.jetcarp, "tilings"], capture_output=True, text=True, check=True).stdout
    ih_text = subprocess.run([args.jetcarp, "isohedral"], capture_output=True, text=True, check=True).stdout
    fams = parse_list(
        fam_text,
        r"(?P<id>\S+)\s+(?P<name>.+?)\s+(?P<sym>\S+)\s+storage (?P<storage>\S+)\s+edge \[(?P<edge>[^\]]*)\] touch \[(?P<touch>[^\]]*)\]",
    )
    ihs = parse_list(
        ih_text,
        r"(?P<id>ih\d+) (?P<sym>\S+)\s+params (?P<params>\d+) aspects\s+(?P<aspects>\d+) edge \[(?P<edge>[^\]]*)\] touch \[(?P<touch>[^\]]*)\]",
    )
    ku = json.loads(KU.read_text(encoding="utf-8"))
    tilings = ku["tilings"]
    byk = defaultdict(list)
    for rec in tilings:
        byk[rec["k"]].append(rec)

    # Check the computed degrees against the program for the regular and Archimedean tilings.
    famdeg = {f["name"].split(" ")[0]: (f["edge"], f["touch"]) for f in fams}
    checked = 0
    for rec in byk[1]:
        vt = rec["name"].split(" ")[0]
        key = {"6^3": "Hexagon", "4^4": "Square", "3^6": "Any", "3.6.3.6": "3.6.3.6", "4.8^2": "4.8.8",
               "3^2.4.3.4": "3.3.4.3.4", "3^3.4^2": "3.3.3.4.4", "3^4.6": "3.3.3.3.6", "3.4.6.4": "3.4.6.4",
               "3.12^2": "3.12.12", "4.6.12": "4.6.12"}.get(vt)
        if key and key in famdeg:
            edge, touch, _ = kuniform_degrees(rec)
            want_e = sorted(int(x) for x in famdeg[key][0].split(","))
            want_t = sorted(int(x) for x in famdeg[key][1].split(","))
            if sorted(edge) != want_e or sorted(touch) != want_t:
                print(f"MISMATCH {rec['id']} {vt}: computed edge {sorted(edge)} touch {sorted(touch)}; "
                      f"jetcarp {want_e} {want_t}", file=sys.stderr)
                sys.exit(1)
            checked += 1
    print(f"checked {checked} 1-uniform tilings against `jetcarp tilings`", file=sys.stderr)

    header = [
        "<!-- GENERATED FILE: do not edit by hand. Generator: docs/tools/gen-tiling-catalogue.py, from",
        "     `jetcarp tilings`, `jetcarp isohedral` and crates/jetcarp-core/data/tilings/kuniform.json. -->",
        "",
        "> **Generated page.** Built by `docs/tools/gen-tiling-catalogue.py` from the program's own tiling",
        "> lists and data files; thumbnails are drawn by JETCARP (`docs/tools/tiling-thumbs.rhai`). Do not",
        "> edit by hand.",
        "",
    ]
    L = ["# Tiling catalogue", ""] + header
    L += [
        f"Every tiling JETCARP can run, with its neighbour counts. That is {len(fams)} tiling families "
        f"(some of them reshapeable), {len(ihs)} isohedral types, and the {len(tilings)} k-uniform tilings of "
        "Galebach's catalogue, which include the regular and Archimedean tilings again under their "
        "catalogue ids.",
        "",
        "**How to read the degrees.** A tile's *edge* neighbours share a side with it; its *touch* neighbours "
        "share at least one corner (see [Tilings as neighbour graphs](neighbour-graphs.md)). The lists give "
        "every value that occurs in the interior of the tiling. Rules that count neighbours (Life-like, "
        "Generations, JACA and others) see these numbers, which is why the same rule behaves so differently "
        "from one tiling to the next.",
        "",
        "Thumbnails show a small patch with each tile in a random colour so the shapes stand out. "
        "Reshapeable families and isohedral types are drawn with their default shapes.",
        "",
        "## Tiling families",
        "",
        "These are the families you pick by name (`tiling: {\"family\": \"cairo\"}` in a scene). *Symmetry* is the "
        "wallpaper group of the default shape (`none` for the aperiodic tilings). *Storage* is how JETCARP "
        "holds the cells: `dense` for a plain lattice, `periodic` for a repeating unit cell, `explicit` for a "
        "finite aperiodic patch.",
        "",
        "| | Id | Name | Symmetry | Storage | Edge degrees | Touch degrees |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for f in fams:
        L.append(f"| {thumb(f['id'], f['name'])} | `{f['id']}` | {f['name']} | {f['sym']} | {f['storage']} | "
                 f"{degrees(f['edge'])} | {degrees(f['touch'])} |")
    L += [
        "",
        "Pages: [Regular and Archimedean tilings](regular-archimedean.md), [Editable tile families](editable-families.md), "
        "[Penrose tilings](penrose.md), [The hat and the spectre](hat-spectre.md).",
        "",
        "## Isohedral types",
        "",
        f"The {len(ihs)} isohedral types that Craig Kaplan's Tactile library supports (IH01 to IH93 in "
        "Grünbaum and Shephard's numbering, less the 12 types Tactile does not support). *Parameters* is the number of "
        "shape sliders; *aspects* is the number of differently oriented copies of the tile in one unit cell. "
        "See [Isohedral tilings](isohedral.md).",
        "",
        "| | Id | Symmetry | Parameters | Aspects | Edge degrees | Touch degrees |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in ihs:
        L.append(f"| {thumb(r['id'], r['id'].upper())} | `{r['id']}` | {r['sym']} | {r['params']} | {r['aspects']} | "
                 f"{degrees(r['edge'])} | {degrees(r['touch'])} |")
    L += [
        "",
        "## k-uniform tilings, k = 1 to 3",
        "",
        "Edge-to-edge tilings by regular polygons with k kinds of vertex (orbits under the tiling's symmetries), "
        "from Brian Galebach's catalogue in José Ezequiel Soto Sánchez's encoding (CC BY 4.0). The name lists the "
        "vertex type of each orbit. *Tiles* is the number of tiles in one unit cell; *mix* is the share of each "
        "polygon; *mean touch* is the average touch degree over the tiles. k = 4 to 6 are on "
        "[their own page](catalogue-kuniform.md). See [k-uniform tilings](k-uniform.md).",
        "",
    ]
    counts = ", ".join(f"k = {k}: {len(byk[k])}" for k in sorted(byk))
    L += [f"Counts: {counts}.", ""]
    for k in (1, 2, 3):
        L += [f"### k = {k} ({len(byk[k])} tilings)", "",
              "| | Id | Vertex types | Symmetry | Tiles | Mix | Edge degrees | Touch degrees | Mean touch |",
              "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
        for rec in byk[k]:
            row, _, _ = ku_row(rec, True)
            L.append(row)
        L.append("")
    OUT.write_text("\n".join(L), encoding="utf-8")

    L2 = ["# k-uniform tilings, k = 4 to 6", ""] + header
    L2 += [
        "The rest of Galebach's catalogue, without thumbnails to keep the book small. To see one, run it: "
        "`jetcarp script` with `world(#{ tiling: \"k5-058\" })`, or pick it in the app's *Tiling catalog*. "
        "Columns as on the [main catalogue page](catalogue.md#k-uniform-tilings-k--1-to-3).",
        "",
    ]
    for k in (4, 5, 6):
        L2 += [f"## k = {k} ({len(byk[k])} tilings)", "",
               "| Id | Vertex types | Symmetry | Tiles | Mix | Edge degrees | Touch degrees | Mean touch |",
               "| --- | --- | --- | --- | --- | --- | --- | --- |"]
        for rec in byk[k]:
            row, _, _ = ku_row(rec, False)
            L2.append(row)
        L2.append("")
    OUT2.write_text("\n".join(L2), encoding="utf-8")
    print(f"wrote {OUT} and {OUT2}", file=sys.stderr)


if __name__ == "__main__":
    main()
