"""Level 3, replay: periodic patterns re-run from the tilings and rules tables alone."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import unittest

from jetcarp_pack import jcs
from jetcarp_pack.replay import Lattice, moved

from . import content as C
from . import stores
from .base import StoreTest, codes, fails, run


def two_column_tiling():
    """The square grid described with a 2 x 1 unit cell: tile type k is the column's parity, so cell
    (i, j, k) is the square at x = 2i + k, y = j. A test-only tiling, with an invented pin."""
    def offsets(pairs):
        out = []
        for k in (0, 1):
            out.append([{"di": (k + a) // 2, "dj": b, "k": (k + a) % 2} for a, b in pairs])
        return out
    return C.lattice_row(jcs.tiling_key({"family": "test-two-column-square"}),
                         "Square grid, two-column cell (test only)", "test-two-column-square", [2.0, 0.0], [0.0, 1.0],
                         [{"shape": 0, "corners": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]},
                          {"shape": 0, "corners": [[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]]}],
                         offsets(C.EDGE), offsets(C.TOUCH), "test-two-column-square")


def brute_b0(birth, survive, start, generations, margin=12):
    """True B/S evolution on the unbounded plane: a window around the pattern, with every cell
    outside it in the background's state, which flips as the dead plane does under B0."""
    cells = {(i, j) for (i, j, _), s in start.items() if s}
    lo_i = min(i for i, _ in cells) - margin
    hi_i = max(i for i, _ in cells) + margin
    lo_j = min(j for _, j in cells) - margin
    hi_j = max(j for _, j in cells) + margin
    background, live, out = 0, set(cells), [set(cells)]
    for _ in range(generations):
        def alive(i, j):
            return (i, j) in live if lo_i <= i <= hi_i and lo_j <= j <= hi_j else bool(background)
        new = set()
        for i in range(lo_i, hi_i + 1):
            for j in range(lo_j, hi_j + 1):
                n = sum(alive(i + a, j + b) for a, b in C.TOUCH)
                if (birth >> n & 1) if not alive(i, j) else (survive >> n & 1):
                    new.add((i, j))
        background = (survive >> 8 & 1) if background else (birth & 1)
        live = new
        out.append((new, background, (lo_i, hi_i, lo_j, hi_j)))
    return out


class Replay(StoreTest):
    def make(self, tilings, rules, patterns, verified_in=None):
        pub = self.keys.a.fingerprint
        tables = {"tilings": tilings, "rules": rules, "patterns": patterns,
                  "verifications": [C.verification(p, verified_in or rules[0], pub, stores.D1) for p in patterns]}
        stores.build_small(self.root(), self.keys, tables=tables)
        return run(self.root(), 3)

    def test_minimal(self):
        stores.build_minimal(self.root(), self.keys)
        report = run(self.root(), 3)
        self.assertClean(report)
        self.assertEqual(report.passed[(3, "patterns replayed")], 3)
        self.assertEqual(report.passed[(3, "rule masks agreeing with their notation")], 2)

    def test_generations(self):
        ship = C.pattern(C.BRAIN, cells={(0, 0, 0): 1, (0, 1, 0): 1, (1, 0, 0): 2, (1, 1, 0): 2})
        self.assertEqual((ship["class"], ship["period"]), ("spaceship", 1))
        report = self.make([C.square_tiling()], [C.BRAIN], [ship])
        self.assertClean(report)
        self.assertEqual(report.passed[(3, "patterns replayed")], 1)

    def test_several_tile_types(self):
        """On a two-column cell the glider is seen again only after it has moved a whole cell: period 8,
        displacement (-1, -2) in cells, though it moves (-1, -1) squares every 4 generations."""
        tiling = two_column_tiling()
        lattice = Lattice(tiling, "touch")
        glider = {(x // 2, y, x % 2): 1 for (x, y, _), _s in C.cells_from_picture(C.GLIDER).items()}
        phases, d = C.evolve(lattice, C.LIFE, glider)
        self.assertEqual((len(phases), d), (8, (-1, -2)))
        row = C.from_phases(phases, d, C.LIFE, tiling_key=tiling["pin_key"], pin=tiling["pin"],
                            tiling_group="test-two-column-square")
        report = self.make([tiling], [C.LIFE], [row])
        self.assertClean(report)
        self.assertEqual(report.passed[(3, "patterns replayed")], 1)

    def test_wrong_phase(self):
        lattice = Lattice(C.square_tiling(), "touch")
        phases, d = C.evolve(lattice, C.LIFE, C.cells_from_picture(C.GLIDER))
        phases[2] = moved(phases[2], 0, 1)  # one phase stored a row off
        report = self.make([C.square_tiling()], [C.LIFE], [C.from_phases(phases, d, C.LIFE)])
        self.assertIn("replay-mismatch", fails(report))
        self.assertTrue(any("generation 2" in f.message for f in report.findings))

    def test_period_not_minimal(self):
        lattice = Lattice(C.square_tiling(), "touch")
        phases, d = C.evolve(lattice, C.LIFE, C.cells_from_picture(C.BLINKER))
        row = C.from_phases(phases * 2, d, C.LIFE)
        self.assertEqual(row["period"], 4)
        self.assertIn("period-not-minimal", fails(self.make([C.square_tiling()], [C.LIFE], [row])))

    def test_masks_disagree_with_notation(self):
        bad = dict(C.LIFE, survive=1 << 3)  # S3, not S23
        report = self.make([C.square_tiling()], [bad], [C.pattern(C.LIFE, C.BLINKER)])
        f = fails(report)
        self.assertIn("rule-masks", f)
        self.assertIn("replay-mismatch", f)

    def test_b0_alternation_matches_the_true_evolution(self):
        """B0 rules run as two complementary rules on even and odd generations (life-like.md): checked
        against a brute-force run of the real rule, whose background flashes."""
        lattice = Lattice(C.square_tiling(), "touch")
        start = C.cells_from_picture(".oo/oo/.o")  # the R-pentomino
        for birth_counts, survive_counts in (((0, 3), (2, 3)), ((0, 1, 3), (0, 2, 5)), ((0, 2, 4, 6), (1, 3, 7))):
            birth = sum(1 << n for n in birth_counts)
            survive = sum(1 << n for n in survive_counts)
            truth = brute_b0(birth, survive, start, 6)
            cur = dict(start)
            for t in range(1, 7):
                cur = lattice.step_b0(cur, birth, survive, odd=(t - 1) % 2 == 1)
                live, background, (lo_i, hi_i, lo_j, hi_j) = truth[t]
                shown = {(i, j) for (i, j, _) in cur}
                inside = {(i, j) for i in range(lo_i, hi_i + 1) for j in range(lo_j, hi_j + 1)}
                expected = (inside - live) if background else live  # shown complemented on odd generations
                self.assertEqual(background, t % 2, (birth_counts, survive_counts, t))
                self.assertEqual(shown & inside, expected, (birth_counts, survive_counts, t))

    def test_b0_pattern_replays(self):
        """In B012345678/S every shape is frozen as shown: the plane and the pattern swap each generation."""
        rule = dict(C.LIFE, notation="B012345678/S", birth=(1 << 9) - 1, survive=0)
        row = C.from_phases([C.cells_from_picture(C.GLIDER)], (0, 0), rule)
        report = self.make([C.square_tiling()], [rule], [row])
        self.assertClean(report)
        self.assertEqual(report.passed[(3, "patterns replayed")], 1)

    def test_b0_that_survives_everything_is_skipped(self):
        rule = dict(C.LIFE, notation="B0/S8", birth=1, survive=1 << 8)
        row = C.from_phases([C.cells_from_picture(C.BLOCK)], (0, 0), rule)
        self.assertIn("b0", codes(self.make([C.square_tiling()], [rule], [row]), "SKIP"))

    def test_no_rule_is_skipped(self):
        stores.build_small(self.root(), self.keys, tables={"tilings": [C.square_tiling()], "rules": [C.LIFE],
                                                         "patterns": [C.pattern(C.LIFE, C.BLOCK)]})
        self.assertIn("no-rule", codes(run(self.root(), 3), "SKIP"))

    def test_missing_rule_row(self):
        report = self.make([C.square_tiling()], [C.LIFE], [C.pattern(C.LIFE, C.BLOCK)], verified_in=C.BRAIN)
        self.assertIn("rule-missing", fails(report))

    def test_aperiodic_is_skipped(self):
        hat = {"pin_key": jcs.tiling_key({"family": "hat"}), "pin": "ab" * 16, "polygon_hash": "cd" * 32,
               "neighbour_hash": "ef" * 32, "name": "Hat", "kind": "aperiodic", "family": "hat", "params": None,
               "edges": None, "tiling_group": "hat", "a1": None, "a2": None, "tile_types": None, "edge": None,
               "touch": None, "reference_level": 4, "reference": "sha256:" + "0" * 64, "reference_tiles": 4000}
        block = C.from_phases([C.cells_from_picture(C.BLOCK)], (0, 0), C.LIFE, tiling_key=hat["pin_key"],
                              pin=hat["pin"], tiling_group="hat")
        report = self.make([hat], [C.LIFE], [block])
        self.assertIn("aperiodic", codes(report, "SKIP"))

    def test_asymmetric_neighbours_warn(self):
        tiling = C.square_tiling()
        tiling["touch"] = [tiling["touch"][0][:-1]]  # one offset left out
        report = self.make([tiling], [C.LIFE], [C.pattern(C.LIFE, C.BLOCK)])
        self.assertIn("asymmetric-neighbours", codes(report, "WARN"))


if __name__ == "__main__":
    unittest.main()
