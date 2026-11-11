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
    return {"pin_key": jcs.tiling_key({"family": "test-two-column-square"}), "pin": "0123456789abcdef" * 2,
            "name": "Square grid, two-column cell (test only)", "kind": "lattice", "a1": [2.0, 0.0],
            "a2": [0.0, 1.0],
            "tile_types": [{"shape": 0, "corners": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]},
                           {"shape": 0, "corners": [[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]]}],
            "edge": offsets(C.EDGE), "touch": offsets(C.TOUCH), "family": "test-two-column-square",
            "params": "{}", "reference_level": None}


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

    def test_b0_is_skipped(self):
        b0 = dict(C.LIFE, notation="B03/S23", birth=(1 << 0) | (1 << 3))
        report = self.make([C.square_tiling()], [b0], [C.pattern(C.LIFE, C.BLOCK)])
        self.assertIn("b0", codes(report, "SKIP"))

    def test_no_rule_is_skipped(self):
        stores.build_small(self.root(), self.keys, tables={"tilings": [C.square_tiling()], "rules": [C.LIFE],
                                                         "patterns": [C.pattern(C.LIFE, C.BLOCK)]})
        self.assertIn("no-rule", codes(run(self.root(), 3), "SKIP"))

    def test_missing_rule_row(self):
        report = self.make([C.square_tiling()], [C.LIFE], [C.pattern(C.LIFE, C.BLOCK)], verified_in=C.BRAIN)
        self.assertIn("rule-missing", fails(report))

    def test_aperiodic_is_skipped(self):
        hat = {"pin_key": jcs.tiling_key({"family": "hat"}), "pin": "ab" * 16, "name": "Hat", "kind": "aperiodic",
               "a1": None, "a2": None, "tile_types": None, "edge": None, "touch": None, "family": "hat",
               "params": "{}", "reference_level": 4}
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
