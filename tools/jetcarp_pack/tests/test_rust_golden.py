"""JETCARP's own golden packs (crates/jetcarp-pack/tests/golden), read by this reader at levels 1 to 3.

The cross-check (SPEC_QUESTIONS.md, "The cross-check") found three disagreements in them: a provenance
target naming quickstart:golden-glider rather than the entry's qs- id, raters' consent_date written as
a date, and the media pack's PNG objects kept out of the repository by the root .gitignore. Each was
fixed on JETCARP's side and the golden packs were made again, with an ants pack added. So every pack
hashes.txt lists must now pass every level with no FAIL and no WARN: the two implementations agree."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import os
import unittest

from jetcarp_pack import sshsig
from jetcarp_pack.report import WARN
from jetcarp_pack.store import open_store

from .base import codes, fails, run

GOLDEN = os.path.join(os.path.dirname(__file__), "..", "..", "..", "crates", "jetcarp-pack", "tests", "golden")

# Every golden pack JETCARP keeps, the ants pack the cross-check asked for among them.
EXPECTED = {"minimal", "retraction", "merge", "searches", "media", "aperiodic", "ants", "export"}


def hashes():
    out = {}
    with open(os.path.join(GOLDEN, "hashes.txt"), encoding="utf-8") as f:
        for line in f:
            if line.strip() and not line.startswith("#"):
                name, h = line.split()
                out[name] = h
    return out


@unittest.skipUnless(os.path.isdir(GOLDEN), "JETCARP's golden packs are not here")
class RustGolden(unittest.TestCase):
    def test_every_pack_is_listed(self):
        self.assertLessEqual(EXPECTED, set(hashes()))

    def test_manifest_hashes(self):
        for name, h in hashes().items():
            with self.subTest(pack=name):
                store = open_store(os.path.join(GOLDEN, name))
                if store.is_export:
                    self.assertEqual(store.manifest_hash, h)
                else:
                    self.assertTrue(store.has_object(h, "json"))

    def test_levels(self):
        levels = (1, 2, 3) if sshsig.available() else (1, 3)
        for name in hashes():
            with self.subTest(pack=name):
                failed, warned = set(), set()
                for level in levels:
                    report = run(os.path.join(GOLDEN, name), level)
                    failed |= fails(report)
                    warned |= codes(report, WARN)
                self.assertEqual(failed, set())
                self.assertEqual(warned, set())


if __name__ == "__main__":
    unittest.main()
