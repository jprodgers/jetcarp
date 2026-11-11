"""Conformance level 3, replay (pack spec sections 11, 16 and 23): re-run periodic patterns from the
`tilings` and `rules` tables alone, and confirm their phases, period, displacement and class.

A lattice cell (i, j, k) has the neighbours (i + di, j + dj, k') for each offset (di, dj, k') in its
tile type's list (section 11). B/S and Generations rules are run from their bit masks: a dead cell
with n live neighbours is born if bit n of `birth` is set; a live cell stays live if bit n of
`survive` is set, and otherwise starts to age (Generations) or dies (B/S); an ageing cell moves to
the next state each generation, and after the last to 0. Only state-1 cells count as live
neighbours (docs/src/automata/generations.md). The field is unbounded and dead (section 23.3): live
cells are kept sparsely, so no edge is ever reached.

Each pattern is replayed in every rule it was verified in (`verifications.rule`), for `periods`
periods, every generation compared with its stored phase moved by the displacement so far.
"""

import time
from collections import defaultdict

from . import identity as ident
from . import rules as rule_notation
from . import schemas
from .integrity import manifests_to_check
from .pack import Pack, check_store_marker, list_refs


class LatticeError(ValueError):
    pass


class Lattice:
    def __init__(self, tiling, nb):
        if tiling["kind"] != "lattice":
            raise LatticeError("is not a lattice")
        if nb not in ("touch", "edge"):
            raise LatticeError(f"neighbourhood {nb!r} is not touch or edge")
        types = tiling["tile_types"] or []
        offsets = tiling[nb]
        if not types or offsets is None or len(offsets) != len(types):
            raise LatticeError(f"has {len(types)} tile types but {len(offsets or [])} {nb} lists")
        self.types = len(types)
        self.forward = []
        for t, lst in enumerate(offsets):
            row = []
            for o in lst:
                if o["k"] is None or not 0 <= o["k"] < self.types or o["di"] is None or o["dj"] is None:
                    raise LatticeError(f"tile type {t} has an offset to tile type {o['k']}, out of range")
                row.append((o["di"], o["dj"], o["k"]))
            self.forward.append(row)
        # Cell y = (i, j, t) has the neighbour x = (i + di, j + dj, k); so from a live x of type k,
        # the cells that count it are x - (di, dj), of type t.
        self.reverse = [[] for _ in range(self.types)]
        for t, row in enumerate(self.forward):
            for di, dj, k in row:
                self.reverse[k].append((-di, -dj, t))
        fwd = [set(row) for row in self.forward]
        self.symmetric = all((-di, -dj, t) in fwd[k] for t, row in enumerate(self.forward) for di, dj, k in row)

    def step(self, live, birth, survive, states):
        counts = defaultdict(int)
        for (i, j, k), s in live.items():
            if s == 1:
                for di, dj, t in self.reverse[k]:
                    counts[(i + di, j + dj, t)] += 1
        new = {}
        for cell, s in live.items():
            if s == 1:
                if survive >> counts.get(cell, 0) & 1:
                    new[cell] = 1
                elif states > 2:
                    new[cell] = 2
            elif s + 1 < states:
                new[cell] = s + 1
        for cell, n in counts.items():
            if cell not in live and birth >> n & 1:
                new[cell] = 1
        return new


def to_cells(phase):
    out = {}
    for c in phase:
        key = (c["i"], c["j"], c["k"])
        if key in out:
            raise LatticeError(f"cell {key} is listed twice")
        out[key] = c["s"]
    return out


def moved(cells, dx, dy):
    return {(i + dx, j + dy, k): s for (i, j, k), s in cells.items()}


def translation(a, b):
    """The lattice vector (di, dj) that carries cells b onto cells a, or None."""
    if len(a) != len(b) or not a:
        return None
    first_a = min(a, key=lambda c: (c[1], c[0], c[2], a[c]))
    first_b = min(b, key=lambda c: (c[1], c[0], c[2], b[c]))
    if first_a[2] != first_b[2]:
        return None
    d = (first_a[0] - first_b[0], first_a[1] - first_b[1])
    return d if moved(b, *d) == a else None


def check_replay(store, report, manifest=None, periods=2, all_versions=False):
    if not check_store_marker(store, report):
        return
    if manifest:
        heads = [manifest]
    elif all_versions:
        heads = manifests_to_check(store, report)
    else:
        heads = [doc["manifest"] for _, doc in list_refs(store, report).values()]
    if not heads:
        report.fail(3, "no-manifest", store.root, "found no manifest to replay")
    cache = {}
    for h in heads:
        pack = Pack(store, h, report, tables={"tilings", "rules", "patterns", "verifications"}, cache=cache)
        if pack.ok:
            Replayer(pack, report, periods).run()


class Replayer:
    def __init__(self, pack, report, periods):
        self.pack, self.r, self.periods, self.lim = pack, report, max(1, periods), pack.limits

    def run(self):
        tilings = {row["pin_key"]: row for _, _, row in self.pack.rows("tilings")}
        rules = {(r["notation"], r["neighbourhood"], r["steps_per_gen"]): r for _, _, r in self.pack.rows("rules")}
        verified = defaultdict(set)
        for _, _, v in self.pack.rows("verifications"):
            verified[v["pattern_id"]].add(v["rule"])
        self._check_rule_masks(rules)
        lattices = {}
        seen = set()
        for _, _, row in self.pack.rows("patterns"):
            pid = row["id"]
            if pid in seen:
                continue
            seen.add(pid)
            w = f"pattern {pid}"
            if row["class"] not in schemas.PERIODIC_CLASSES:
                self.r.skip(3, "not-periodic", w, f"class {row['class']} is not replayed: level 3 covers still lifes, "
                                                  "oscillators and spaceships")
                continue
            tiling = tilings.get(row["pin_key"])
            if tiling is None:
                self.r.skip(3, "no-tiling", w, "its tiling is not in the tilings table")
                continue
            if tiling["kind"] != "lattice":
                self.r.skip(3, "aperiodic", w, "is on an aperiodic tiling: the pack carries no neighbour graph "
                                               "to replay it on")
                continue
            key = (row["pin_key"], row["neighbourhood"])
            if key not in lattices:
                try:
                    lattices[key] = Lattice(tiling, row["neighbourhood"])
                except LatticeError as e:
                    lattices[key] = None
                    self.r.fail(3, "lattice", f"tiling {ident.short(row['pin_key'])}", str(e))
                else:
                    if not lattices[key].symmetric:
                        self.r.warn(3, "asymmetric-neighbours", f"tiling {ident.short(row['pin_key'])}",
                                    f"its {row['neighbourhood']} offsets are not symmetric (a has b but b lacks a)")
            lattice = lattices[key]
            if lattice is None:
                continue
            names = sorted(verified.get(pid, ()))
            if not names:
                self.r.skip(3, "no-rule", w, "has no verifications, so no rule to replay it in")
                continue
            for notation in names:
                rule = rules.get((notation, row["neighbourhood"], row["steps_per_gen"]))
                rw = f"{w} in {notation}"
                if rule is None:
                    self.r.fail(3, "rule-missing", rw, f"the rules table has no row for ({notation!r}, "
                                                       f"{row['neighbourhood']}, {row['steps_per_gen']})")
                    continue
                if rule["family"] not in schemas.RULE_FAMILIES_WITH_MASKS or rule["birth"] is None \
                        or rule["survive"] is None:
                    self.r.skip(3, "rule-family", rw, f"family {rule['family']} needs its own simulator")
                    continue
                if rule["birth"] & 1:
                    self.r.skip(3, "b0", rw, "birth on 0 switches on the whole dead plane; such rules run as two "
                                             "alternating rules in JETCARP, which the spec does not describe")
                    continue
                try:
                    self.replay(lattice, rule, row, rw)
                except Exception as e:  # never a crash on a value this reader did not foresee
                    self.r.fail(3, "check-error", rw, f"replay stopped on {type(e).__name__}: {str(e)[:200]}")

    def _check_rule_masks(self, rules):
        for (notation, nb, steps), rule in rules.items():
            if rule["family"] not in schemas.RULE_FAMILIES_WITH_MASKS:
                continue
            w = f"rules {notation!r} {nb}"
            parsed = rule_notation.parse_bs(notation)
            if parsed is None:
                self.r.skip(3, "rule-notation", w, "notation is not a B/S form this reader parses")
                continue
            bm, sm, states, vnb = parsed
            if (bm, sm) != (rule["birth"], rule["survive"]):
                self.r.fail(3, "rule-masks", w, f"masks birth {rule_notation.mask_text(rule['birth'] or 0)} / survive "
                                                f"{rule_notation.mask_text(rule['survive'] or 0)} differ from the "
                                                f"notation's {rule_notation.mask_text(bm)} / {rule_notation.mask_text(sm)}")
            if states != rule["states"]:
                self.r.fail(3, "rule-masks", w, f"states {rule['states']} differ from the notation's {states}")
            if vnb == "edge" and nb != "edge":
                self.r.fail(3, "rule-masks", w, "the notation ends in V (edge neighbours) but the row says touch")
            expected_family = "lifelike" if states == 2 else "generations"
            if rule["family"] != expected_family:
                self.r.warn(3, "rule-family", w, f"family {rule['family']}, but a {states}-state rule is {expected_family}")
            self.r.ok(3, "rule masks agreeing with their notation")

    def replay(self, lattice, rule, row, w):
        lim = self.lim
        period, dx, dy, steps = row["period"], row["dx"], row["dy"], row["steps_per_gen"]
        states = rule["states"]
        try:
            phases = [to_cells(p) for p in row["phases"]]
        except LatticeError as e:
            self.r.fail(3, "phase-cells", w, str(e))
            return
        if len(phases) != period:
            self.r.fail(3, "replay-phases", w, f"has {len(phases)} phases for period {period}")
            return
        for n, ph in enumerate(phases):
            for (i, j, k), s in ph.items():
                if not 0 <= k < lattice.types:
                    self.r.fail(3, "phase-cells", w, f"phase {n} has tile type {k}; the tiling has {lattice.types}")
                    return
                if not 1 <= s < states:
                    self.r.fail(3, "phase-cells", w, f"phase {n} has state {s}; the rule has states 0 to {states - 1}")
                    return
        periods = self.periods
        if period * periods * steps > lim.replay_generations:
            periods = lim.replay_generations // (period * steps)
            if periods < 1:
                self.r.skip(3, "budget", w, f"period {period} is over the replay budget of "
                                            f"{lim.replay_generations:,} generations")
                return
        start = time.monotonic()
        cur = dict(phases[0])
        for t in range(1, period * periods + 1):
            for _ in range(steps):
                cur = lattice.step(cur, rule["birth"], rule["survive"], states)
            if len(cur) > lim.replay_cells or time.monotonic() - start > lim.replay_seconds:
                self.r.skip(3, "budget", w, f"stopped at generation {t}: over the replay budget")
                return
            q, n = divmod(t, period)
            expected = moved(phases[n], q * dx, q * dy)
            if cur != expected:
                missing = sorted(set(expected.items()) - set(cur.items()))
                extra = sorted(set(cur.items()) - set(expected.items()))
                what = f"phase {n}" + (f" moved by {q * dx},{q * dy}" if q else "")
                self.r.fail(3, "replay-mismatch", w,
                            f"generation {t} differs from {what}: {len(missing)} cells missing "
                            f"{_cells(missing)}, {len(extra)} extra {_cells(extra)}")
                return
        for t in range(1, period):
            d = translation(phases[t], phases[0])
            if d is not None:
                self.r.fail(3, "period-not-minimal", w, f"phase {t} is phase 0 moved by {d[0]},{d[1]}, so the "
                                                        f"period is not {period}")
                return
        moving = (dx, dy) != (0, 0)
        cls = "spaceship" if moving else ("still_life" if period == 1 else "oscillator")
        if cls != row["class"]:
            self.r.fail(3, "replay-class", w, f"replays as a {cls}, not a {row['class']}")
            return
        self.r.ok(3, "patterns replayed")
        if lattice.types == 1:
            pic = ident.picture(row["phases"][0])
            if pic is not None and pic != row["canonical"]:
                self.r.warn(3, "picture", w, f"canonical {row['canonical']!r}, but phase 0 draws as {pic!r} "
                                             "(the picture rule is inferred from the book's example)")
            ph0 = row["phases"][0]
            if ph0 and (min(c["i"] for c in ph0), min(c["j"] for c in ph0)) != (0, 0):
                self.r.warn(3, "phase-origin", w, "phase 0 is not moved to the origin (min i and min j 0)")


def _cells(items, n=4):
    shown = " ".join(f"{i},{j},{k},{s}" for (i, j, k), s in items[:n])
    return f"[{shown}{' ...' if len(items) > n else ''}]"
