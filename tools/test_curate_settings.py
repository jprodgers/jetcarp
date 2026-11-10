#!/usr/bin/env python3
"""Test tools/curate_settings.py end to end with the stub model (no LM Studio needed).

    cargo build --release -p jetcarp-cli && python3 tools/test_curate_settings.py

Builds a small catalog with `jetcarp stage1 run`, rates its settings through contact sheets with
`--stub`, and checks that every rating landed in `ratings` as `model:stub`, with no repeats on a
second session.
"""

import os
import sqlite3
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import curate_settings  # noqa: E402

JETCARP = os.path.join(os.path.dirname(HERE), "target", "release", "jetcarp")


@unittest.skipUnless(os.path.exists(JETCARP), "build jetcarp first: cargo build --release -p jetcarp-cli")
class StubCuration(unittest.TestCase):
    def test_stub_ratings_reach_the_catalog(self):
        lib = tempfile.mkdtemp(prefix="jetcarp-curate-test-")
        subprocess.run([JETCARP, "stage1", "--library", lib, "run", "--tilings", "hex",
                        "--birth", "2-3", "--survive", "2-4", "--grid", "32x32", "--gens", "200",
                        "--soups", "2", "--refine-top", "0", "--neighbours-top", "0"],
                       check=True, capture_output=True)
        args = ["--jetcarp", JETCARP, "--library", lib, "--stub", "--status",
                "stored,candidate,auto", "--candidates", "10", "--batch", "4", "--no-log"]
        self.assertEqual(curate_settings.main(args), 0)
        db = sqlite3.connect(os.path.join(lib, "catalog.sqlite"))
        rows = db.execute("SELECT target_kind, rater, rating, note FROM ratings").fetchall()
        self.assertEqual(len(rows), 10)
        for kind, rater, rating, note in rows:
            self.assertEqual((kind, rater), ("setting", "model:stub"))
            self.assertTrue(1 <= rating <= 5)
            self.assertIn("stub", note)
        # A second session rates only settings this model has not rated.
        self.assertEqual(curate_settings.main(args), 0)
        ids = [r[0] for r in db.execute("SELECT target_id FROM ratings")]
        self.assertEqual(len(ids), 20)
        self.assertEqual(len(set(ids)), 20)
        # 20 rated settings are enough for a refit.
        out = subprocess.run([JETCARP, "stage1", "--library", lib, "refit"], capture_output=True,
                             text=True)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("from 20 ratings of 20 settings", out.stdout)


if __name__ == "__main__":
    unittest.main()
