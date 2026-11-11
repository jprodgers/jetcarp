"""Records for test packs, on the square grid, made by simulation.

The canonical form here is the one the book's glider implies (library-format.md): over every phase in
each of the square grid's 8 symmetries, moved so its smallest i and j are 0, the start whose cells,
sorted by (j, i, k, s), are smallest. It reproduces the book's glider text and id exactly; for other
patterns it is an inference, and JETCARP's level 4 may disagree (see SPEC_QUESTIONS.md).

The tilings row's geometry (corners, axis directions, neighbour order) is this writer's guess, since
section 23.1 leaves it to the engine; only the pin is JETCARP's (the book's square-grid pin).
"""

from jetcarp_pack import identity as ident
from jetcarp_pack import jcs
from jetcarp_pack.replay import Lattice, moved, translation

SQUARE_KEY = jcs.tiling_key({"family": "square"})
SQUARE_PIN = "0145310ebd49abfe89eb01c42ecaf459"  # library-format.md
EDGE = [(1, 0), (0, 1), (-1, 0), (0, -1)]
TOUCH = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]
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


def square_tiling():
    def offsets(lst):
        return [[{"di": di, "dj": dj, "k": 0} for di, dj in lst]]
    return {"pin_key": SQUARE_KEY, "pin": SQUARE_PIN, "name": "Square", "kind": "lattice",
            "a1": [1.0, 0.0], "a2": [0.0, 1.0],
            "tile_types": [{"shape": 0, "corners": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]}],
            "edge": offsets(EDGE), "touch": offsets(TOUCH), "family": "square", "params": "{}",
            "reference_level": None}


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


def verification(pattern, rule, publisher, date):
    return {"pattern_id": pattern["id"], "rule": rule["notation"], "publisher": publisher, "periods": 16,
            "generations": 16 * pattern["period"], "field": "dead-edged frame 32x32", "engine_version": "0.4.0",
            "pin": pattern["pin"], "semantics": "1", "date": date, "range": None}


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
    return {"id": ident.ant_id(identity, SQUARE_PIN), "identity": identity, "pin_key": SQUARE_KEY,
            "pin": SQUARE_PIN, "rule": "Ant RL", "kind": "string", "start": '"empty"', "outcome": "highway",
            "period": period, "dx": dx, "dy": dy, "onset": onset, "steps": steps, "cells": len(visited),
            "tiling_group": "square", "family": "ant", "semantics": "1"}


SEARCH = "0b6f3c2a-5d4e-4f1a-9c8b-7a6e5d4c3b2a"


def events(publisher, patterns, date, rule="B3/S23", names=None, search=SEARCH, ants=()):
    """Verifications, sightings, provenance and names for records, by one publisher."""
    names = names or {}
    out = {"verifications": [], "sightings": [], "provenance": [], "annotations": []}
    for p in patterns:
        out["verifications"].append({
            "pattern_id": p["id"], "rule": rule, "publisher": publisher, "periods": 16,
            "generations": 16 * p["period"], "field": "dead-edged frame 32x32", "engine_version": "0.4.0",
            "pin": p["pin"], "semantics": "1", "date": date, "range": None})
        out["sightings"].append({
            "pattern_id": p["id"], "rule": rule, "publisher": publisher, "source": search,
            "count": 7, "soups": 5, "first_seen": date, "last_seen": date})
        out["provenance"].append({
            "target": f"pattern:{p['id']}", "publisher": publisher, "seq": 0, "kind": "found", "finder": None,
            "orcid": None, "search_kind": "census", "search": search, "engine_version": "0.4.0",
            "pin": p["pin"], "date": date, "source": "search"})
        if p["id"] in names:
            out["annotations"].append({
                "target": f"pattern:{p['id']}", "publisher": publisher, "field": "name",
                "value": jcs.dumps(names[p["id"]]), "score_model": None, "date": date})
    for a in ants:
        out["provenance"].append({
            "target": f"ant:{a['id']}", "publisher": publisher, "seq": 0, "kind": "found", "finder": None,
            "orcid": None, "search_kind": "ants", "search": search, "engine_version": "0.4.0",
            "pin": a["pin"], "date": date, "source": "search"})
    return out
