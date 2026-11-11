"""JETCARP's own golden packs (crates/jetcarp-pack/tests/golden), read by this reader at levels 1 to 3.

Each pack must pass, except for the disagreements found in the cross-check and reported (see
SPEC_QUESTIONS.md, "Cross-check"). They are pinned here exactly, so a fix on either side shows up
as a failure to update this table."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import os
import unittest

from jetcarp_pack import sshsig
from jetcarp_pack.store import open_store

from .base import fails, run

GOLDEN = os.path.join(os.path.dirname(__file__), "..", "..", "..", "crates", "jetcarp-pack", "tests", "golden")

# FAIL codes this reader gives, per pack, and why (each one reported):
# - target: a provenance row names quickstart:golden-glider, not the entry's id (qs-...);
# - time: raters' consent_date is a date (2026-11-01), not an RFC 3339 time;
# - missing: the media pack's two PNG objects were never committed (the root .gitignore drops *.png).
KNOWN = {
    "minimal": {"target", "time"},
    "retraction": {"target", "time"},
    "merge": {"target", "time"},
    "searches": {"time"},
    "media": {"missing"},
    "aperiodic": set(),
    "export": {"target", "time"},
}


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
        for name, known in KNOWN.items():
            with self.subTest(pack=name):
                got = set()
                for level in levels:
                    got |= fails(run(os.path.join(GOLDEN, name), level))
                self.assertEqual(got, known)


if __name__ == "__main__":
    unittest.main()
