"""Records for test packs, on the square grid, made by simulation.

The canonical form here is the one the book's glider implies (library-format.md): over every phase in
each of the square grid's 8 symmetries, moved so its smallest i and j are 0, the start whose cells,
sorted by (j, i, k, s), are smallest. It reproduces the book's glider text and id exactly, and the
golden packs' block and blinker; for other patterns it is an inference that JETCARP's level 4 tests.

The square grid's `tilings` row is pack-format.md's own ("Geometry pins"): its pin and hashes are
recomputed from it here and must give the spec's values.
"""

import math

from jetcarp_pack import identity as ident
from jetcarp_pack import jcs, pins
from jetcarp_pack.replay import Lattice, moved, translation

SQUARE_KEY = jcs.tiling_key({"family": "square"})
SQUARE_PIN = "0145310ebd49abfe89eb01c42ecaf459"  # library-format.md and pack-format.md
# pack-format.md's square grid: clockwise on the y-down screen, from the tile's first corner.
EDGE = [(0, -1), (1, 0), (0, 1), (-1, 0)]
TOUCH = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)]
SYMMETRIES = [(1, 0, 0, 1), (0, -1, 1, 0), (-1, 0, 0, -1), (0, 1, -1, 0),
              (-1, 0, 0, 1), (1, 0, 0, -1), (0, 1, 1, 0), (0, -1, -1, 0)]

LIFE = {"notation": "B3/S23", "neighbourhood": "touch", "steps_per_gen": 1, "family": "lifelike",
        "semantics": "1", "states": 2, "birth": 1 << 3, "survive": (1 << 2) | (1 << 3)}
BRAIN = {"notation": "B2/S/C3", "neighbourhood": "touch", "steps_per_gen": 1, "family": "generations",
         "semantics": "1", "states": 3, "birth": 1 << 2, "survive": 0}

GLIDER_ID = "5a080db389c0cab3caf884d2596a1e70"  # library-format.md
GLIDER = "ooo/o/.o"
BLOCK = "oo/oo"
BLINKER = "ooo"
LWSS = ".o..o/o/o...o/oooo"


def lattice_row(pin_key, name, family, a1, a2, tile_types, edge, touch, tiling_group, params=None, edges=None):
    """A lattice `tilings` row, its pin and hashes computed from its geometry."""
    row = {"pin_key": pin_key, "name": name, "kind": "lattice", "family": family, "params": params,
           "edges": edges, "tiling_group": tiling_group, "a1": a1, "a2": a2, "tile_types": tile_types,
           "edge": edge, "touch": touch, "reference_level": None, "reference": None, "reference_tiles": None}
    row["polygon_hash"], row["neighbour_hash"], row["pin"] = pins.lattice_hashes(row)
    return row


def square_tiling():
    def offsets(lst):
        return [[{"di": di, "dj": dj, "k": 0} for di, dj in lst]]
    row = lattice_row(SQUARE_KEY, "Square", "square", [1.0, 0.0], [-0.0, 1.0],
                      [{"shape": 0, "corners": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]}],
                      offsets(EDGE), offsets(TOUCH), "square")
    assert row["pin"] == SQUARE_PIN, row["pin"]
    return row


def cells_from_picture(text, state=1):
    return {(i, j, 0): state for j, row in enumerate(text.split("/")) for i, ch in enumerate(row) if ch == "o"}


def _normalise(cells):
    i0 = min(c[0] for c in cells)
    j0 = min(c[1] for c in cells)
    return {(i - i0, j - j0, k): s for (i, j, k), s in cells.items()}


def _sorted_key(cells):
    return sorted((j, i, k, s) for (i, j, k), s in cells.items())


def evolve(lattice, rule, start, max_period=1000):
    """Phases of one period from `start`, and the displacement after it."""
    phases = [dict(start)]
    cur = dict(start)
    for t in range(1, max_period + 1):
        cur = lattice.step(cur, rule["birth"], rule["survive"], rule["states"])
        d = translation(cur, phases[0])
        if d is not None:
            return phases, d
        phases.append(cur)
    raise ValueError("no period found")


def canonical_start(lattice, rule, start):
    phases, _ = evolve(lattice, rule, start)
    best = None
    for phase in phases:
        for a, b, c, d in SYMMETRIES:
            image = _normalise({(a * i + b * j, c * i + d * j, k): s for (i, j, k), s in phase.items()})
            if best is None or _sorted_key(image) < _sorted_key(best):
                best = image
    return best


def as_rows(cells):
    return [{"i": i, "j": j, "k": k, "s": s} for (j, i, k, s) in _sorted_key(cells)]


def pattern(rule, picture=None, cells=None, speed=None, nb="touch"):
    """A pattern row, made by running `rule` on the square grid from `picture` (or `cells`)."""
    lattice = Lattice(square_tiling(), nb)
    start = canonical_start(lattice, rule, cells if cells is not None else cells_from_picture(picture))
    phases, d = evolve(lattice, rule, start)
    return from_phases(phases, d, rule, speed=speed, nb=nb)


def from_phases(phases, d, rule, *, speed=None, nb="touch", tiling_key=SQUARE_KEY, pin=SQUARE_PIN,
                tiling_group="square", cls=None):
    """A pattern row for the given phases ({(i, j, k): s} each) and displacement."""
    dx, dy = d
    period = len(phases)
    cls = cls or ("spaceship" if (dx, dy) != (0, 0) else ("still_life" if period == 1 else "oscillator"))
    rows = [as_rows(p) for p in phases]
    canonical = ident.picture(rows[0])
    if canonical is None:  # several tile types or states: the book gives no rule to copy (choice)
        canonical = "cells " + ident.format_phases([rows[0]])
    identity = ident.format_pattern_identity(model="lifelike", nb=nb, steps=1, tiling=tiling_key, cls=cls,
                                             period=period, d=(dx, dy), rule="", canonical=canonical, phases=rows)
    sizes = [len(p) for p in phases]
    return {"id": ident.pattern_id(identity, pin), "identity": identity, "pin_key": tiling_key,
            "pin": pin, "model": "lifelike", "neighbourhood": nb, "steps_per_gen": 1, "class": cls,
            "period": period, "dx": dx, "dy": dy, "speed": speed, "cells": sizes[0], "min_cells": min(sizes),
            "max_cells": max(sizes), "phases": rows, "canonical": canonical, "rule": None, "site": None,
            "gun": None, "class_group": cls, "tiling_group": tiling_group, "family": rule["family"],
            "semantics": rule["semantics"]}


def verification(pattern, rule, publisher, date, main=True):
    return {"pattern_id": pattern["id"], "rule": rule["notation"], "publisher": publisher, "main": main,
            "periods": 16, "generations": 16 * pattern["period"], "field": "dead-edged frame 32x32",
            "sites": None, "engine_version": "0.4.0", "pin": pattern["pin"], "semantics": rule["semantics"],
            "date": date, "range": None, "mover": None}


def quickstart(pattern, title, date):
    """A Quick Start entry made from a pattern, its scene modelled on the golden packs' (scene-format.md)."""
    scene = jcs.dumps({"cas": [{"enabled": True, "rule": "B3/S23", "steps_per_gen": 1}], "format": 1,
                       "jetcarp_version": "0.4.0", "name": title, "rng_state": 1,
                       "tiling": {"boundary": "wrap", "family": "square", "size": {"rect": {"h": 32, "w": 32}}}})
    return {"id": ident.quickstart_id(scene, SQUARE_PIN), "title": title, "description": "A test entry.",
            "source": f"pattern:{pattern['id']}", "scene": scene, "scene_format": "1", "rule": "B3/S23",
            "rule_family": "lifelike", "semantics": "1", "behaviour": "spaceship", "pin_key": SQUARE_KEY,
            "pin": SQUARE_PIN, "tiling_group": "square"}


def langtons_ant(steps=12000):
    """Langton's ant on an empty square grid, under this writer's own convention (start at 0,0
    facing -j, R turns from -j to +i, turn on the colour then repaint and step): the run's highway
    period, displacement and onset. JETCARP's start direction and turn sense may differ."""
    dirs = [(0, -1), (1, 0), (0, 1), (-1, 0)]
    black, pos, d = set(), (0, 0), 0
    track, visited = [], {(0, 0)}
    for _ in range(steps):
        track.append((pos, d))
        if pos in black:
            d = (d - 1) % 4
            black.discard(pos)
        else:
            d = (d + 1) % 4
            black.add(pos)
        pos = (pos[0] + dirs[d][0], pos[1] + dirs[d][1])
        visited.add(pos)
    track.append((pos, d))
    period = 104
    (x1, y1), _ = track[-1]
    (x0, y0), _ = track[-1 - period]
    dx, dy = x1 - x0, y1 - y0
    onset = len(track) - 1 - period
    while onset > 0:
        (a, da), (b, db) = track[onset - 1], track[onset - 1 + period]
        if da != db or (b[0] - a[0], b[1] - a[1]) != (dx, dy):
            break
        onset -= 1
    identity = ident.format_ant_identity(rule="Ant RL", tiling=SQUARE_KEY, start='"empty"')
    # The run as pack-format.md gives it: its limits, and what it found (cells: the coloured cells at
    # the end; reach: the box of cells the ant reached; speed: distance per step in cell sizes).
    xs, ys = [p[0] for p in visited], [p[1] for p in visited]
    speed = round(math.hypot(dx, dy) / period, 6)
    run = {"limits": {"max_period": 2000, "steps": steps, "symmetry": True},
           "run": {"cells": len(black), "d": [dx, dy], "onset": onset, "outcome": "highway", "period": period,
                   "reach": [min(xs), min(ys), max(xs), max(ys)], "speed": speed, "steps": steps}}
    return {"id": ident.ant_id(identity, SQUARE_PIN), "identity": identity, "pin_key": SQUARE_KEY,
            "pin": SQUARE_PIN, "rule": "Ant RL", "kind": "string", "family": "ant", "semantics": "1",
            "states": 1, "colors": 2, "start": '"empty"', "outcome": "highway",
            "period": period, "dx": dx, "dy": dy, "onset": onset, "speed": speed, "steps": steps,
            "cells": len(black), "run": jcs.dumps(run, allow_floats=True), "tiling_group": "square"}


SEARCH = "0b6f3c2a-5d4e-4f1a-9c8b-7a6e5d4c3b2a"


def provenance(target, publisher, date, pin, kind="found", seq=0, search=SEARCH, pack=None, source="search"):
    return {"target": target, "publisher": publisher, "seq": seq, "kind": kind, "finder": None, "orcid": None,
            "search_kind": "hunt" if search else None, "search": search, "machine": "linux-x86_64",
            "engine_version": "0.4.0", "pin": pin, "date": date, "source": source, "pack": pack, "run": None}


def events(publisher, patterns, date, rule=LIFE, names=None, search=SEARCH, ants=(), rev=1, quickstarts=()):
    """Verifications, sightings, provenance and names for records, by one publisher, added by the
    version whose ref seq is `rev`."""
    names = names or {}
    out = {"verifications": [], "sightings": [], "provenance": [], "annotations": []}
    for p in patterns:
        out["verifications"].append(verification(p, rule, publisher, date))
        out["sightings"].append({
            "pattern_id": p["id"], "rule": rule["notation"], "publisher": publisher, "source": search,
            "source_kind": "hunt", "count": 7, "soups": 5, "first_seen": date, "last_seen": date})
        out["provenance"].append(provenance(f"pattern:{p['id']}", publisher, date, p["pin"]))
        if p["id"] in names:
            out["annotations"].append({
                "target": f"pattern:{p['id']}", "publisher": publisher, "field": "name",
                "value": jcs.dumps(names[p["id"]]), "score_model": None, "date": date, "rev": rev})
    for a in ants:
        out["provenance"].append(provenance(f"ant:{a['id']}", publisher, date, a["pin"]))
    for q in quickstarts:
        out["provenance"].append(provenance(f"quickstart:{q['id']}", publisher, date, q["pin"], search=None,
                                            source="a test scene"))
    return out
