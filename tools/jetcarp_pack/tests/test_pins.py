"""Geometry pins recomputed from the byte layout pack-format.md gives, against its worked example,
the repository's twelve vectors and the golden aperiodic pack's reference patch (read as data)."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import json
import os
import unittest

import pyarrow.parquet as pq

from jetcarp_pack import pins

from . import content as C

REPO = os.path.join(os.path.dirname(__file__), "..", "..", "..")
VECTORS = os.path.join(REPO, "crates", "jetcarp-pack", "tests", "data", "pin_vectors.jsonl")
GOLDEN = os.path.join(REPO, "crates", "jetcarp-pack", "tests", "golden")


class Pins(unittest.TestCase):
    def test_the_specs_square_grid(self):
        row = C.square_tiling()
        self.assertEqual(row["polygon_hash"], "f5a761f20c098aba5f00361946f6b309ccbb17ab5f6aca2474d0aa3e64a8075b")
        self.assertEqual(row["neighbour_hash"], "12a567c8a5480b1b7ca4a8d4ccd2439565888a698ace2567508ed2c954c0bbdb")
        self.assertEqual(row["pin"], "0145310ebd49abfe89eb01c42ecaf459")

    def test_minus_zero_counts(self):
        row = dict(C.square_tiling(), a2=[0.0, 1.0])
        self.assertNotEqual(pins.lattice_hashes(row)[2], C.SQUARE_PIN)

    @unittest.skipUnless(os.path.exists(VECTORS), "the repository's pin_vectors.jsonl is not here")
    def test_vectors(self):
        n = 0
        with open(VECTORS, encoding="utf-8") as f:
            for line in f:
                row = json.loads(line)
                with self.subTest(tiling=row["name"]):
                    self.assertEqual(pins.lattice_hashes(row), (row["polygon_hash"], row["neighbour_hash"], row["pin"]))
                n += 1
        self.assertEqual(n, 12)

    @unittest.skipUnless(os.path.isdir(os.path.join(GOLDEN, "aperiodic")), "the golden packs are not here")
    def test_reference_patch(self):
        h = "5589466102692e2e327fccdede0e119c7cc43f62976e028851396abeb83bbceb"
        path = os.path.join(GOLDEN, "aperiodic", "objects", "sha256", h[:2], h + ".parquet")
        tiles = pq.read_table(path).to_pylist()
        self.assertEqual(len(tiles), 4000)
        self.assertEqual(pins.patch_hashes(tiles), (
            "feb5a714508afdbea7c7c56670e852cccf9e42dbfeaa9f1d64c491c4b97ee2b4",
            "7bd9af15938bffc998582eac4419e885e5013af64a08cef36d1e9fdae692dce7",
            "627092d6ad4b5e1db6173d78c9c9e28a"))


if __name__ == "__main__":
    unittest.main()
