#!/usr/bin/env python3
"""Compare a JETCARP soup hunt of Conway's Life with Catagolue's census (a release-candidate check).

Runs `jetcarp search hunt --rules B3/S23 --tilings square` with a fixed seed in a fresh, temporary
library, works out the apgcode of every object it stored from the object's own cells, and compares
the shares of the twelve commonest objects of Catagolue's b3s23/C1 census with the copy pinned in
docs/data/catagolue-b3s23-C1.tsv. The hunt's defaults (16 x 16 soups at density 0.5, an unbounded
plane) are Catagolue's C1 convention, so the shares should agree within chance.

The tolerance. Objects from one soup are not independent (a soup that makes one beehive tends to
make more), so the spread of a share over many soups is wider than a binomial count's. For each
object the script multiplies the binomial variance by the ratio PHI measured below, giving a z that
behaves like a standard normal one, and passes when every one of the twelve is within 4 of zero
and the sum of their squares is at most 32.9 (the 0.1% point of chi-squared with 12 degrees of
freedom). The defaults (seed 7, 150,000 soups, about a minute on 32 threads) are the run of the
November 2026 audit (issue 182). Of its two differences, the glider excess came from gliders that
the hunt failed to cut out (fixed in 0.4.3: with JETCARP 0.4.2 this check fails, the sum of
squares 35.4; with the fix it passes, 27.3), and the ship-tie deficit is chance: a fresh run of a
million soups (seed 11) gives -1.1%, z = -1.5.

Standard library only. Usage:
    python3 docs/tools/check-catagolue.py [--jetcarp PATH] [--soups N] [--seed S] [--threads T]
                                          [--keep DIR] [--library FILE]
--library skips the hunt and reads an existing library (it must hold that hunt alone).
Exit status 1 when the comparison fails.
"""
import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
PINNED = DOCS / "data" / "catagolue-b3s23-C1.tsv"
TOP = 12
MAX_Z = 4.0
MAX_CHI2 = 32.91  # chi-squared, 12 degrees of freedom, upper 0.1% point

# Per-soup variance of each object's count over the binomial variance, measured with JETCARP 0.4.3
# (after the glider fix) over 1,000,000 soups of seed 11: sum over soups of (x - p t)^2, x the
# object's copies and t all objects in the soup, divided by p (1 - p) times all objects.
PHI = {
    "xs4_33": 1.31, "xp2_7": 2.24, "xs6_696": 1.75, "xq4_153": 1.11, "xs7_2596": 1.10,
    "xs5_253": 1.11, "xs6_356": 0.86, "xs4_252": 1.03, "xs8_6996": 1.02, "xs7_25ac": 1.00,
    "xp2_7e": 1.02, "xs12_g8o653z11": 1.76,
}

CH = "0123456789abcdefghijklmnopqrstuv"


def wechsler(cells):
    """The extended Wechsler format of a set of (x, y) cells, as in apgcodes."""
    x0 = min(x for x, _ in cells)
    y0 = min(y for _, y in cells)
    cells = {(x - x0, y - y0) for x, y in cells}
    w = max(x for x, _ in cells) + 1
    h = max(y for _, y in cells) + 1
    strips = []
    for s in range((h + 4) // 5):
        col = [sum(1 << (y - 5 * s) for y in range(5 * s, 5 * s + 5) if (x, y) in cells)
               for x in range(w)]
        while col and col[-1] == 0:
            col.pop()
        out, i = "", 0
        while i < len(col):
            if col[i]:
                out += CH[col[i]]
                i += 1
                continue
            j = i
            while j < len(col) and col[j] == 0:
                j += 1
            n = j - i
            while n > 0:
                if n >= 4:
                    k = min(n, 39)
                    out += "y" + (CH[k - 4] if k - 4 < 32 else "wxyz"[k - 36])
                    n -= k
                elif n == 3:
                    out, n = out + "x", 0
                elif n == 2:
                    out, n = out + "w", 0
                else:
                    out, n = out + "0", 0
            i = j
        strips.append(out)
    while strips and strips[-1] == "":
        strips.pop()
    return "z".join(strips)


def orientations(cells):
    for sx in (1, -1):
        for sy in (1, -1):
            for swap in (False, True):
                yield {((y * sy, x * sx) if swap else (x * sx, y * sy)) for x, y in cells}


def apgcode(rec):
    """The apgcode of a library record (square grid): the shortest, then the first in order, of
    its phases' codes in all eight orientations."""
    best = None
    for ph in rec["phases"]:
        cells = set()
        for tok in ph.split():
            i, j, _k, _s = map(int, tok.split(","))
            cells.add((i, j))
        for o in orientations(cells):
            c = wechsler(o)
            if best is None or (len(c), c) < (len(best), best):
                best = c
    prefix = {"still_life": f"xs{rec['min_cells']}", "oscillator": f"xp{rec['period']}",
              "spaceship": f"xq{rec['period']}"}.get(rec["class"])
    return f"{prefix}_{best}" if prefix else None


def pinned():
    total, rows = None, []
    for line in PINNED.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        code, n = line.split("\t")
        if code == "total":
            total = int(n)
        else:
            rows.append((code, int(n)))
    return total, rows


def find_jetcarp(given):
    if given:
        return given
    for p in (REPO / "target" / "release" / "jetcarp", REPO / "target" / "release" / "jetcarp.exe"):
        if p.exists():
            return str(p)
    return shutil.which("jetcarp") or sys.exit("no jetcarp binary: build it or pass --jetcarp")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--jetcarp")
    ap.add_argument("--soups", type=int, default=150_000)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--threads", type=int)
    ap.add_argument("--keep", help="keep the hunt's folders here instead of a temporary folder")
    ap.add_argument("--library", help="read this library instead of running a hunt")
    a = ap.parse_args()
    assert wechsler({(0, 0), (1, 0), (0, 1), (1, 1)}) == "33"

    jet = find_jetcarp(a.jetcarp)
    work = Path(a.keep) if a.keep else Path(tempfile.mkdtemp(prefix="jetcarp-catagolue-"))
    env = dict(os.environ)
    if a.library:
        lib = Path(a.library)
    else:
        for sub in ("config", "data", "out"):
            (work / sub).mkdir(parents=True, exist_ok=True)
        env.update(JETCARP_CONFIG_DIR=str(work / "config"), JETCARP_DATA_DIR=str(work / "data"),
                   JETCARP_OUT=str(work / "out"))
        lib = work / "data" / "library.sqlite"
        env["JETCARP_LIBRARY"] = str(lib)
        cmd = [jet, "search", "hunt", "--rules", "B3/S23", "--tilings", "square",
               "--soups", str(a.soups), "--seed", str(a.seed), "--library", str(lib)]
        if a.threads:
            cmd += ["--threads", str(a.threads)]
        print("$ " + " ".join(cmd[1:]), file=sys.stderr)
        subprocess.run(cmd, env=env, check=True, stdout=subprocess.DEVNULL)
    out = subprocess.run([jet, "library", "list", "--json", "--library", str(lib),
                          "--limit", "1000000"],
                         env=env, check=True, capture_output=True, text=True).stdout
    recs = json.loads(out)
    seen = {}
    for r in recs:
        code = apgcode(r)
        if code:
            seen[code] = seen.get(code, 0) + int(r["seen"])
    t = sum(int(r["seen"]) for r in recs)
    total, rows = pinned()
    print(f"JETCARP: {t:,} objects of {len(recs)} kinds; Catagolue b3s23/C1 (pinned): "
          f"{total:.4g} objects")
    print(f"{'apgcode':<18}{'JETCARP':>10}{'share':>10}{'Catagolue':>11}{'diff':>8}{'z':>7}")
    chi2, worst, ok = 0.0, 0.0, True
    for code, n in rows[:TOP]:
        p = n / total
        x = seen.get(code, 0)
        z = (x - t * p) / math.sqrt(PHI[code] * t * p * (1 - p))
        chi2 += z * z
        worst = max(worst, abs(z))
        flag = "  <--" if abs(z) > MAX_Z else ""
        print(f"{code:<18}{x:>10}{100 * x / t:>9.4f}%{100 * p:>10.4f}%"
              f"{100 * (x / (t * p) - 1):>+7.2f}%{z:>+7.2f}{flag}")
    unknown = [c for c in seen if c not in dict(rows)]
    ok = worst <= MAX_Z and chi2 <= MAX_CHI2
    print(f"largest |z| {worst:.2f} (at most {MAX_Z}); sum of z^2 {chi2:.1f} (at most {MAX_CHI2}); "
          f"{len(unknown)} kinds outside the pinned top {len(rows)}")
    print("PASS" if ok else "FAIL")
    if not a.keep and not a.library:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
