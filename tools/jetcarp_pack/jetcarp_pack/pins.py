"""Geometry pins (pack-format.md, "Geometry pins"): the polygon hash, the neighbour hash and the pin,
recomputed from a lattice tiling's `tilings` row or an aperiodic family's reference patch.

Integers are little-endian u32 unless marked (`di`, `dj` are i32), floats the little-endian u64 of
their IEEE 754 bits (so -0.0 and 0.0 differ), strings their ASCII bytes with no terminator.
"""

import hashlib
import struct

SCHEME = "jetcarp-geometry-v1"


class PinError(ValueError):
    pass


def _u32(n):
    if not isinstance(n, int) or isinstance(n, bool) or not 0 <= n < 2 ** 32:
        raise PinError(f"{n!r} is not a u32")
    return struct.pack("<I", n)


def _i32(n):
    if not isinstance(n, int) or isinstance(n, bool) or not -2 ** 31 <= n < 2 ** 31:
        raise PinError(f"{n!r} is not an i32")
    return struct.pack("<i", n)


def _f64(x):
    if isinstance(x, bool) or not isinstance(x, (int, float)):
        raise PinError(f"{x!r} is not a number")
    return struct.pack("<d", float(x))  # the u64 of the bits, little-endian, is these bytes


def _point(p):
    if not isinstance(p, (list, tuple)) or len(p) != 2:
        raise PinError(f"{p!r} is not a point (x, y)")
    return _f64(p[0]) + _f64(p[1])


def _hash(kind, body):
    return hashlib.sha256(f"{SCHEME} {kind}\n".encode("ascii") + body).hexdigest()


def pin_of(polygon_hash, neighbour_hash):
    return hashlib.sha256(f"{SCHEME}\n{polygon_hash}\n{neighbour_hash}".encode("ascii")).hexdigest()[:32]


def lattice_hashes(row):
    """(polygon hash, neighbour hash, pin) of a lattice tiling's `tilings` row."""
    a1, a2, types = row.get("a1"), row.get("a2"), row.get("tile_types")
    if a1 is None or a2 is None or types is None or row.get("edge") is None or row.get("touch") is None:
        raise PinError("a lattice row needs a1, a2, tile_types, edge and touch")
    body = bytearray(b"lattice")
    body += _point(a1) + _point(a2) + _u32(len(types))
    for t in types:
        corners = t["corners"]
        body += _u32(t["shape"]) + _u32(len(corners))
        for c in corners:
            body += _point(c)
    polygon = _hash("polygons", bytes(body))
    body = bytearray()
    for nb in ("edge", "touch"):
        lists = row[nb]
        if len(lists) != len(types):
            raise PinError(f"{nb} has {len(lists)} lists for {len(types)} tile types")
        for slots in lists:
            body += _u32(len(slots))
            for o in slots:
                body += _i32(o["di"]) + _i32(o["dj"]) + _u32(o["k"])
    neighbour = _hash("neighbours", bytes(body))
    return polygon, neighbour, pin_of(polygon, neighbour)


def patch_hashes(tiles):
    """(polygon hash, neighbour hash, pin) of a reference patch: its rows, in `tile` order."""
    body = bytearray(b"patch")
    body += _u32(len(tiles))
    for n, t in enumerate(tiles):
        if t["tile"] != n:
            raise PinError(f"row {n} is tile {t['tile']}; tiles must run 0, 1, 2, ...")
        body += _u32(t["shape"]) + _u32(len(t["corners"]))
        for c in t["corners"]:
            body += _point(c)
    polygon = _hash("polygons", bytes(body))
    body = bytearray()
    for nb in ("edge", "touch"):
        for t in tiles:
            body += _u32(len(t[nb]))
            for other in t[nb]:
                if not 0 <= other < len(tiles):
                    raise PinError(f"tile {t['tile']} names neighbour {other}, out of range")
                body += _u32(other)
    for t in tiles:
        body.append(1 if t["open"] else 0)
    neighbour = _hash("neighbours", bytes(body))
    return polygon, neighbour, pin_of(polygon, neighbour)
