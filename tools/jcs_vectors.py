"""Write crates/jetcarp-core/tests/data/jcs_vectors.txt: RFC 8785 (JCS) test vectors made by an
independent implementation, the pure-Python `rfc8785` package, for JETCARP's
`crates/jetcarp-core/tests/jcs_vectors.rs`.

Each line is three tab-separated fields:
    value   <a JSON value>    <its RFC 8785 text>
    tiling  <a tiling spec>   <the `tiling=` line of an identity text on it>

The values are random JSON: doubles from random bits (every magnitude, subnormals included) and
decimal-looking doubles, integers in the range RFC 8785 allows (below 2^53), strings with control
characters, non-ASCII and astral characters, and objects whose keys sort differently by UTF-16
code units than by code points. The tilings are random specs with parameters, edge shapes, sizes
and boundaries; their line is written as docs/src/reference/library-format.md says: the members
`family`, `params`, `edges` and `boundary` in that order, each value in RFC 8785 form, an
aperiodic family's `level` parameter, the size and the boundary left out (the boundary is always
"dead").

Inputs are written by Python's json module (floats in their shortest round-trip form, non-ASCII
characters as \\u escapes), which serde_json reads exactly. The seed is fixed, so the file is the
same each run. Run, in a virtual environment that has the package:
    python3 -m venv venv && venv/bin/pip install rfc8785
    venv/bin/python -I tools/jcs_vectors.py > crates/jetcarp-core/tests/data/jcs_vectors.txt
"""
import json, random, struct, sys

import rfc8785

rng = random.Random(8785)
APERIODIC = {"hat", "spectre", "penrose-kd", "penrose-rhomb"}
FAMILIES = ["square", "hex", "cairo", "rhombille", "ih28", "ih58", "periodic", "brick", "hex_flat"]


def jcs(v):
    return rfc8785.dumps(v).decode("utf-8")


def double():
    r = rng.random()
    if r < 0.5:
        while True:
            x = struct.unpack("<d", rng.getrandbits(64).to_bytes(8, "little"))[0]
            if x == x and abs(x) != float("inf"):
                return x
    if r < 0.85:
        x = float(f"{rng.randint(1, 10 ** rng.randint(1, 17))}e{rng.randint(-30, 30)}")
        return -x if rng.random() < 0.3 else x
    return rng.choice([0.0, -0.0, 1.0, -1.0, 0.5, 1e-7, 1e-6, 1.5e-6, 1e20, 1e21, 1e22, 0.1, 1 / 3,
                       2.0 ** 53, 123456789012345680000.0, 5e-324, 1.7976931348623157e308])


def integer():
    if rng.random() < 0.5:
        return rng.randint(-1000, 1000)
    return rng.randint(-(2 ** 53 - 1), 2 ** 53 - 1)


def char():
    r = rng.random()
    if r < 0.45:
        return chr(rng.randint(0x20, 0x7E))
    if r < 0.6:
        return chr(rng.randint(0, 0x1F))
    if r < 0.7:
        return chr(rng.choice([0x7F, 0x80, 0x85, 0xA0, 0xF6, 0xFF]))
    if r < 0.85:
        while True:
            c = rng.randint(0x100, 0xFFFF)
            if not 0xD800 <= c <= 0xDFFF:
                return chr(c)
    return chr(rng.randint(0x10000, 0x10FFFF))


def string(n=8):
    return "".join(char() for _ in range(rng.randint(0, n)))


def key():
    # Mostly short keys from a small pool, so objects share prefixes; some sort differently in
    # UTF-16 than by code point (U+FB33 against the astral U+1F600, for example).
    if rng.random() < 0.5:
        return rng.choice(["a", "b", "aa", "ab", "A", "B", "1", "10", "", "\r", "€", "דּ",
                           "\U0001F600", "\u0080", "ö", "level", "family", "params"])
    return string(4)


def value(depth=0):
    r = rng.random()
    if depth >= 2 or r < 0.45:
        s = rng.random()
        if s < 0.45:
            return double()
        if s < 0.6:
            return integer()
        if s < 0.85:
            return string()
        return rng.choice([None, True, False])
    if r < 0.7:
        return [value(depth + 1) for _ in range(rng.randint(0, 4))]
    return {key(): value(depth + 1) for _ in range(rng.randint(0, 5))}


def tiling():
    t = {"family": rng.choice(FAMILIES + sorted(APERIODIC)) if rng.random() < 0.95 else string(5)}
    if rng.random() < 0.8:
        p = {}
        for _ in range(rng.randint(0, 4)):
            p[rng.choice(["a", "b", "angle", "scale", "level", "catalog", "x", "y", "é"])] = \
                rng.choice([double, integer, string, lambda: [double() for _ in range(3)]])()
        if t["family"] in APERIODIC and rng.random() < 0.5:
            p["level"] = rng.randint(1, 6)
        t["params"] = p
    if rng.random() < 0.4:
        edges = []
        for _ in range(rng.randint(0, 3)):
            e = {"mode": rng.choice("JUSI"),
                 "nodes": [{"t": double(), "d": double()} for _ in range(rng.randint(0, 2))]}
            if rng.random() < 0.6:
                e["curved"] = rng.random() < 0.5
            edges.append(e)
        t["edges"] = edges
    if rng.random() < 0.5:
        t["size"] = rng.choice([{"cells": 4096}, {"level": 3}, {"rect": {"w": 8, "h": 6}}])
    t["boundary"] = rng.choice(["dead", "wrap", "live"])
    return t


def tiling_line(t):
    out = ['"family":' + jcs(t["family"])]
    params = t.get("params")
    if params is not None and t["family"] in APERIODIC:
        params = {k: v for k, v in params.items() if k != "level"} or None
    if params is not None:
        out.append('"params":' + jcs(params))
    if "edges" in t:
        edges = [{"mode": e["mode"], "curved": e.get("curved", True), "nodes": e["nodes"]}
                 for e in t["edges"]]
        out.append('"edges":' + jcs(edges))
    out.append('"boundary":"dead"')
    return "{" + ",".join(out) + "}"


lines = ["# RFC 8785 vectors from the rfc8785 Python package " + "(tools/jcs_vectors.py)"]
for _ in range(300):
    v = value()
    lines.append("value\t" + json.dumps(v) + "\t" + jcs(v))
for _ in range(300):
    t = tiling()
    lines.append("tiling\t" + json.dumps(t) + "\t" + tiling_line(t))
sys.stdout.write("\n".join(lines) + "\n")
