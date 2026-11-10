"""Convert the k-uniform tilings (k <= 6, 1,248 tilings) to JETCARP's catalog format.

Source: J. E. Soto Sanchez's "Galebach collection" (tools/data/JSON_Galebach.json, downloaded
unchanged from https://chequesoto.info/tiling/JSON_Galebach.json, sha256 6d9dbc69...076d).
It encodes Brian Galebach's catalog of k-uniform tilings (probabilitysports.com/tilings.html) in
the integer representation of Soto Sanchez, Weyrich, Medeiros e Sa and de Figueiredo, "An integer
representation for periodic tilings of the plane by regular polygons", Computers & Graphics 95
(2021) 69-80. The collection is published under the Creative Commons Attribution 4.0
International licence (footer of https://chequesoto.info/tiling/).

Representation: points of Z[w], w = exp(i pi/6), as integer 4-vectors in the basis 1, w, w^2, w^3
(w^4 = w^2 - 1). Each entry gives two translations T1, T2 and "seeds", the tiling's vertices in
one translation cell; the vertex set is seeds + lattice and edges join vertices at distance 1.
Entry t1002 (4.8.8) is not in Z[w] and is built here directly.

Everything below is exact integer arithmetic except for reducing points into a cell. For each
tiling the script checks that every vertex star closes (angles 60/90/120/150 degrees summing to
360), that every polygon corner is a vertex, that the polygon areas fill the cell, finds the full
translation lattice (and reduces it), computes the symmetry group, the vertex orbits (must be k)
and the wallpaper group, and checks that no two tilings are isometric. Output, one record per
tiling, with exact geometry decoded by jetcarp-core (src/zomega.rs):
    "z": {"a1": [4 ints], "a2": [4 ints], "p": [[x0, x1, x2, x3, dir, n], ...]}
where a polygon is the regular n-gon with a corner at x, first edge along w^dir, counterclockwise.
Run:
    python3 tools/gen_kuniform.py > crates/jetcarp-core/data/tilings/kuniform.json
"""
import json, math, re, sys, hashlib
from fractions import Fraction
from itertools import product

SRC = "tools/data/JSON_Galebach.json"
S3 = math.sqrt(3)
WX = [1, S3 / 2, 0.5, 0]
WY = [0, 0.5, S3 / 2, 1]


def fl(z):
    return (sum(a * b for a, b in zip(z, WX)), sum(a * b for a, b in zip(z, WY)))


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def mul(k, a):
    return tuple(k * x for x in a)


def wmul(z):
    a, b, c, d = z
    return (-d, a, b + d, c)


def conj(z):
    a, b, c, d = z
    return (a + c, b, -c, -b - d)


UNIT = [(1, 0, 0, 0)]
for _ in range(11):
    UNIT.append(wmul(UNIT[-1]))


def lin(r, refl, z):
    """Apply w^r (after conjugation if refl)."""
    if refl:
        z = conj(z)
    for _ in range(r % 12):
        z = wmul(z)
    return z


GAP = {2: 3, 3: 4, 4: 6, 5: 12}  # angle in 30-degree steps -> polygon


class Cell:
    """Exact lattice (Z-span of a1, a2 in Z[w]) with reduction of points mod the lattice."""

    def __init__(self, a1, a2):
        self.a1, self.a2 = a1, a2
        p, q = fl(a1), fl(a2)
        self.det = p[0] * q[1] - p[1] * q[0]
        assert abs(self.det) > 1e-9

    def coords(self, z):
        x, y = fl(z) if len(z) == 4 else z
        p, q = fl(self.a1), fl(self.a2)
        return ((x * q[1] - y * q[0]) / self.det, (p[0] * y - p[1] * x) / self.det)

    def shift(self, xy):
        """Integer (i, j) moving point xy into [0,1)^2 lattice coordinates (with rounding)."""
        u, v = self.coords(xy)
        ru, rv = round(u * 1e7), round(v * 1e7)
        return (ru // 10**7, rv // 10**7)

    def reduce(self, z):
        i, j = self.shift(z)
        return sub(z, add(mul(i, self.a1), mul(j, self.a2)))

    def area(self):
        return abs(self.det)


def poly_corners(x, d, n):
    out, p = [x], x
    for s in range(n - 1):
        p = add(p, UNIT[(d + s * (12 // n)) % 12])
        out.append(p)
    return out


def poly_area(n):
    return n / (4 * math.tan(math.pi / n))


def parse_source(path):
    text = open(path).read()
    text = text[text.index("{"):]
    text = re.sub(r",(\s*[\]}])", r"\1", text)
    return json.loads(text)


def vtype_canon(seq):
    """Canonical cyclic sequence (lexicographically least rotation or reflection)."""
    n = len(seq)
    cands = []
    for s in (seq, seq[::-1]):
        for r in range(n):
            cands.append(tuple(s[r:] + s[:r]))
    return min(cands)


def vtype_str(seq):
    parts, i = [], 0
    while i < len(seq):
        j = i
        while j < len(seq) and seq[j] == seq[i]:
            j += 1
        parts.append(str(seq[i]) + (f"^{j - i}" if j - i > 1 else ""))
        i = j
    return ".".join(parts)


class Tiling:
    def __init__(self, key, rec):
        self.key = key
        T1, T2 = tuple(rec["T1"]), tuple(rec["T2"])
        cell = Cell(T1, T2)
        seeds = {cell.reduce(tuple(s)) for s in rec["Seed"]}
        assert len(seeds) == len(rec["Seed"]), f"{key}: duplicate seeds"
        # Full translation group: seed differences that map the vertex set to itself.
        gens = [T1, T2]
        s0 = min(seeds)
        for s in seeds:
            t = sub(s, s0)
            if s != s0 and all(cell.reduce(add(x, t)) in seeds for x in seeds):
                gens.append(t)
        a1, a2 = reduced_basis(gens)
        self.cell = Cell(a1, a2)
        self.V = sorted({self.cell.reduce(s) for s in seeds})
        self.Vset = set(self.V)
        assert abs(len(seeds) * self.cell.area() / cell.area() - len(self.V)) < 1e-6
        self.a1, self.a2 = a1, a2
        self.stars = {}
        polys = {}
        for v in self.V:
            dirs = [k for k in range(12) if self.has(add(v, UNIT[k]))]
            gaps = [(dirs[(i + 1) % len(dirs)] - dirs[i]) % 12 or 12 for i in range(len(dirs))]
            assert sum(gaps) == 12 and all(g in GAP for g in gaps), f"{key}: bad star {dirs}"
            self.stars[v] = (dirs, [GAP[g] for g in gaps])
            for d, g in zip(dirs, gaps):
                n = GAP[g]
                cs = poly_corners(v, d, n)
                assert all(self.has(c) for c in cs), f"{key}: open polygon"
                cx = sum(fl(c)[0] for c in cs) / n, sum(fl(c)[1] for c in cs) / n
                i, j = self.cell.shift(cx)
                t = add(mul(i, a1), mul(j, a2))
                cs = [sub(c, t) for c in cs]
                k0 = min(range(n), key=lambda m: cs[m])  # canonical start corner
                polys[(n, frozenset(cs))] = (cs[k0], (d + k0 * (12 // n)) % 12, n)
        self.polys = sorted(polys.values())
        tot = sum(poly_area(n) for _, _, n in self.polys)
        assert abs(tot - self.cell.area()) < 1e-6, f"{key}: area {tot} vs {self.cell.area()}"
        self.vtypes = {v: vtype_canon(self.stars[v][1]) for v in self.V}

    def has(self, z):
        return self.cell.reduce(z) in self.Vset

    def maps_into(self, other, r, refl, t):
        # g(v + l) = g(v) + L(l): the linear part must also map the lattice into other's.
        for a in (self.a1, self.a2):
            if other.cell.reduce(lin(r, refl, a)) != (0, 0, 0, 0):
                return False
        return all(other.has(add(lin(r, refl, v), t)) for v in self.V)

    def isometries_to(self, other, first=False):
        """All (r, refl, t) mod other's lattice with g(self) = other."""
        if abs(len(self.V) / self.cell.area() - len(other.V) / other.cell.area()) > 1e-9:
            return []
        v0 = self.V[0]
        out = []
        for r, refl in product(range(12), (False, True)):
            lv = lin(r, refl, v0)
            for w in other.V:
                if other.vtypes[w] != self.vtypes[v0]:
                    continue
                t = sub(w, lv)
                if self.maps_into(other, r, refl, t):
                    out.append((r, refl, t))
                    if first:
                        return out
        return out

    def analyse(self):
        syms = self.isometries_to(self)
        self.syms = syms
        parent = {v: v for v in self.V}

        def find(x):
            while parent[x] != x:
                parent[x] = parent[parent[x]]
                x = parent[x]
            return x

        for r, refl, t in syms:
            for v in self.V:
                a, b = find(v), find(self.cell.reduce(add(lin(r, refl, v), t)))
                if a != b:
                    parent[a] = b
        orbits = {}
        for v in self.V:
            orbits.setdefault(find(v), []).append(v)
        self.orbits = sorted(orbits.values(), key=lambda o: (self.vtypes[o[0]], len(o)))
        self.orbit_types = sorted(self.vtypes[o[0]] for o in self.orbits)
        self.group = wallpaper(self, syms)


def reduced_basis(gens):
    """Gauss-reduced basis of the Z-span (rank 2) of exact vectors in Z[w]."""
    base = Cell(gens[0], gens[1])
    # Rational coordinates of each generator in terms of gens[0], gens[1].
    co = []
    for g in gens:
        u, v = base.coords(g)
        fu, fv = Fraction(u).limit_denominator(1000), Fraction(v).limit_denominator(1000)
        D = math.lcm(fu.denominator, fv.denominator)
        assert mul(D, g) == add(mul(int(fu * D), gens[0]), mul(int(fv * D), gens[1]))
        co.append((fu, fv))
    D = math.lcm(*[f.denominator for c in co for f in c])
    rows = [[int(u * D), int(v * D)] for u, v in co]
    # Hermite normal form of the integer row lattice (2 columns).
    b = hnf2(rows)

    def vec(row):
        num = add(mul(row[0], gens[0]), mul(row[1], gens[1]))
        assert all(x % D == 0 for x in num)
        return tuple(x // D for x in num)

    e1, e2 = vec(b[0]), vec(b[1])

    def n2(z):
        x, y = fl(z)
        return x * x + y * y

    def dt(a, c):
        p, q = fl(a), fl(c)
        return p[0] * q[0] + p[1] * q[1]

    for _ in range(1000):
        if n2(e1) > n2(e2) + 1e-9:
            e1, e2 = e2, e1
        x = dt(e1, e2) / n2(e1)
        if abs(x) <= 0.5 + 1e-9:  # reduced (ties left alone, or the loop could cycle)
            break
        m = round(x)
        e2 = sub(e2, mul(m, e1))
    else:
        raise AssertionError("basis reduction did not converge")
        e2 = sub(e2, mul(m, e1))
    # Positive orientation, a1 as close to horizontal as possible among short choices.
    p, q = fl(e1), fl(e2)
    if p[0] * q[1] - p[1] * q[0] < 0:
        e2 = mul(-1, e2)
    return e1, e2


def hnf2(rows):
    rows = [r[:] for r in rows if r != [0, 0]]
    # Column 0 gcd row.
    def egcd_combine(rows, col):
        while sum(1 for r in rows if r[col] != 0) > 1:
            rows.sort(key=lambda r: (r[col] == 0, abs(r[col])))
            piv = rows[0]
            for r in rows[1:]:
                if r[col]:
                    q = r[col] // piv[col]
                    r[0] -= q * piv[0]
                    r[1] -= q * piv[1]
        rows.sort(key=lambda r: (r[col] == 0, abs(r[col])))
        return rows
    rows = egcd_combine(rows, 0)
    first, rest = rows[0], [r for r in rows[1:] if r != [0, 0]]
    rest = egcd_combine(rest, 1)
    return [first, rest[0]]


def wallpaper(T, syms):
    """Wallpaper group name from the symmetries (r, refl, t) mod the lattice."""
    rots = {r for r, refl, _ in syms if not refl}
    order = len(rots)
    refl_parts = sorted({r for r, refl, _ in syms if refl})
    a1, a2 = fl(T.a1), fl(T.a2)
    window = [(i, j) for i in range(-3, 4) for j in range(-3, 4)]

    def tvec(t, i, j):
        x, y = fl(t)
        return (x + i * a1[0] + j * a2[0], y + i * a1[1] + j * a2[1])

    mirrors, glides = {}, {}
    for r, refl, t in syms:
        if not refl:
            continue
        # Axis direction of x -> w^r conj(x): angle r*15 degrees.
        ang = r * math.pi / 12
        ax = (math.cos(ang), math.sin(ang))
        for i, j in window:
            tx = tvec(t, i, j)
            g = tx[0] * ax[0] + tx[1] * ax[1]
            # Translations of the lattice along the axis.
            per = axis_period(a1, a2, ax)
            frac = (g / per) % 1.0 if per else 1.0
            if min(frac, 1 - frac) < 1e-7:
                # Drop the along-axis part: the mirror line passes through half the rest.
                mirrors.setdefault(r, []).append((tx[0] - g * ax[0], tx[1] - g * ax[1]))
            else:
                glides.setdefault(r, True)
    if not refl_parts:
        return {1: "p1", 2: "p2", 3: "p3", 4: "p4", 6: "p6"}[order]
    nm = len(mirrors)
    if order == 1:
        return ("cm" if glides else "pm") if nm else "pg"
    if order == 2:
        if nm == 2:
            return "cmm" if glides else "pmm"
        return "pmg" if nm == 1 else "pgg"
    if order == 6:
        return "p6m"
    # Order 3 or 4: are all rotation centres of that order on mirror lines?
    k = 12 // order
    centres = []
    for r, refl, t in syms:
        if refl or r != k:
            continue
        for i, j in window:
            tx = tvec(t, i, j)
            # Fixed point c of c -> R c + t.
            c, s = math.cos(r * math.pi / 6), math.sin(r * math.pi / 6)
            m = ((1 - c), s, -s, (1 - c))  # I - R
            det = m[0] * m[3] - m[1] * m[2]
            centres.append(((m[3] * tx[0] - m[1] * tx[1]) / det, (-m[2] * tx[0] + m[0] * tx[1]) / det))
    mirror_lines = []
    for r, txs in mirrors.items():
        for tx in txs:
            mirror_lines.append((r, (tx[0] / 2, tx[1] / 2)))  # passes through t/2 for pure mirrors
    def on_mirror(c):
        for r, p in mirror_lines:
            ang = r * math.pi / 12
            nx, ny = -math.sin(ang), math.cos(ang)
            for i, j in window:
                q = (c[0] + i * a1[0] + j * a2[0], c[1] + i * a1[1] + j * a2[1])
                if abs((q[0] - p[0]) * nx + (q[1] - p[1]) * ny) < 1e-7:
                    return True
        return False
    all_on = all(on_mirror(c) for c in centres)
    if order == 3:
        # Cross-check with the Hermann-Mauguin reading: p31m has mirrors along the shortest
        # lattice vectors, p3m1 mirrors along vectors sqrt(3) times longer.
        r0 = next(iter(mirrors))
        ang = r0 * math.pi / 12
        per = axis_period(a1, a2, (math.cos(ang), math.sin(ang)))
        short = min(math.hypot(*a1), math.hypot(*a2))
        hm = "p31m" if abs(per - short) < 1e-6 else "p3m1"
        assert abs(per - short) < 1e-6 or abs(per - S3 * short) < 1e-6, (per, short)
        got = "p3m1" if all_on else "p31m"
        assert got == hm, f"{T.key}: p3 mirror tests disagree ({got} vs {hm})"
        return got
    # Cross-check: only p4m has mirrors along the shortest lattice vectors.
    short = min(math.hypot(*a1), math.hypot(*a2))
    along = any(abs(axis_period(a1, a2, (math.cos(r * math.pi / 12), math.sin(r * math.pi / 12))) - short) < 1e-6
                for r in mirrors)
    got = "p4m" if all_on else "p4g"
    assert got == ("p4m" if along else "p4g"), f"{T.key}: p4 mirror tests disagree"
    return got


def axis_period(a1, a2, ax):
    best = None
    for i in range(-12, 13):
        for j in range(-12, 13):
            if i == 0 and j == 0:
                continue
            x, y = i * a1[0] + j * a2[0], i * a1[1] + j * a2[1]
            if abs(x * ax[1] - y * ax[0]) < 1e-7:
                L = math.hypot(x, y)
                if best is None or L < best:
                    best = L
    return best


ARCH = {
    "3^6": "triangular", "4^4": "square", "6^3": "hexagonal", "3^4.6": "snub hexagonal",
    "3^3.4^2": "elongated triangular", "3^2.4.3.4": "snub square", "3.4.6.4": "rhombitrihexagonal",
    "3.6.3.6": "trihexagonal", "3.12^2": "truncated hexagonal", "4.6.12": "truncated trihexagonal",
    "4.8^2": "truncated square",
}


def oct_record():
    w = 1 + math.sqrt(2)
    r, h = w / 2, 0.5
    octa = [[-h, -r], [h, -r], [r, -h], [r, h], [h, r], [-h, r], [-r, h], [-r, -h]]
    sq = [[r, h], [w - h, r], [r, w - h], [h, r]]
    return {"a1": [w, 0.0], "a2": [0.0, w], "polys": [octa, sq]}


MAX_SEEDS = 200  # largest cell in the source has fewer vertices; guards against bad input


def main():
    import resource
    resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))  # hard 2 GB cap
    kmax = int(sys.argv[1]) if len(sys.argv) > 1 else 6
    src = parse_source(SRC)
    keys = sorted(k for k in src if re.fullmatch(r"t[1-6]\d{3}", k) and int(k[1]) <= kmax)
    tilings, out = [], []
    for key in keys:
        if key == "t1002":
            tilings.append(None)
            continue
        assert len(src[key]["Seed"]) <= MAX_SEEDS, f"{key}: too many seeds"
        T = Tiling(key, src[key])
        T.analyse()
        k = int(key[1])
        assert len(T.orbits) == k, f"{key}: {len(T.orbits)} vertex orbits, expected {k}"
        tilings.append(T)
    # No two tilings isometric (cheap invariants first).
    seen = {}
    for T in tilings:
        if T is None:
            continue
        sig = (tuple(T.orbit_types), tuple(sorted(n for *_, n in T.polys)), round(len(T.V) / T.cell.area(), 9))
        for U in seen.get(sig, []):
            assert not T.isometries_to(U, first=True), f"{T.key} is isometric to {U.key}"
        seen.setdefault(sig, []).append(T)
    # Names: vertex types per orbit; tilings sharing a name get #1, #2, ... in catalog order.
    names = []
    for key, T in zip(keys, tilings):
        types = ["4.8^2"] if T is None else [vtype_str(t) for t in T.orbit_types]
        names.append("[" + "; ".join(types) + "]")
    dup = {}
    for n in names:
        dup[n] = dup.get(n, 0) + 1
    seen_n = {}
    for key, T, name in zip(keys, tilings, names):
        k, num = int(key[1]), int(key[2:])
        total = sum(1 for x in keys if x[1] == key[1])
        full = name
        if dup[name] > 1:
            seen_n[name] = seen_n.get(name, 0) + 1
            full += f" #{seen_n[name]}"
        rec = {"id": f"k{k}-{num:03d}", "name": full, "group": f"k-uniform (k={k})", "k": k}
        if k == 1:
            rec["name"] = f"{full[1:-1]} {ARCH[full[1:-1]]}"
        if T is None:
            rec["note"] = f"Galebach {k}-uniform #{num} of {total}. Symmetry p4m. Octagons and squares."
            rec.update(oct_record())
        else:
            counts = {}
            for *_, n in T.polys:
                counts[n] = counts.get(n, 0) + 1
            pc = ", ".join(f"{c} {POLY[n]}{'s' if c > 1 else ''}" for n, c in sorted(counts.items()))
            rec["note"] = (f"Galebach {k}-uniform #{num} of {total}. Symmetry {T.group}. "
                           f"{len(T.V)} vertices per cell; {pc}.")
            rec["z"] = {"a1": list(T.a1), "a2": list(T.a2),
                        "p": [list(x) + [d, n] for x, d, n in T.polys]}
        out.append(rec)
    by_k = {}
    for r in out:
        by_k[r["k"]] = by_k.get(r["k"], 0) + 1
    print("counts by k:", by_k, file=sys.stderr)
    groups = {}
    for T in tilings:
        if T is not None:
            groups[T.group] = groups.get(T.group, 0) + 1
    print("wallpaper groups:", groups, file=sys.stderr)
    print("max vertices/polygons per cell:", max(len(T.V) for T in tilings if T),
          max(len(T.polys) for T in tilings if T), file=sys.stderr)
    sha = hashlib.sha256(open(SRC, "rb").read()).hexdigest()
    doc = {
        "source": "tools/gen_kuniform.py from tools/data/JSON_Galebach.json "
                  f"(sha256 {sha}), https://chequesoto.info/tiling/JSON_Galebach.json",
        "license": "CC BY 4.0. Tilings: B. Galebach, n-uniform tilings, probabilitysports.com/tilings.html. "
                   "Integer encoding: J. E. Soto Sanchez, T. Weyrich, A. Medeiros e Sa, L. H. de Figueiredo, "
                   "Computers & Graphics 95 (2021) 69-80; collection CC BY 4.0 at chequesoto.info/tiling.",
        "tilings": out,
    }
    s = json.dumps(doc, separators=(",", ":"))
    # One tiling per line keeps diffs readable.
    s = s.replace(',{"id"', ',\n{"id"')
    print(s)


POLY = {3: "triangle", 4: "square", 6: "hexagon", 12: "dodecagon"}

if __name__ == "__main__":
    main()
