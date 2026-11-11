"""Hostile stores (pack-format.md, "Limits"): oversized, deep, decompression bombs and path tricks are
refused, without crashing or hanging."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import json
import os
import secrets
import unittest

from jetcarp_pack import jcs
from jetcarp_pack.limits import Limits
from jetcarp_pack.store import Store, StoreError

from . import content as C
from . import stores
from .base import StoreTest, current, fails, republish, run
from .packwriter import StoreWriter


def object_path(root, h, ext):
    return os.path.join(root, "objects", "sha256", h[7:9], f"{h[7:]}.{ext}")


class PathTricks(StoreTest):
    def test_symlinked_object_is_refused(self):
        out = stores.build_minimal(self.root(), self.keys)
        part = out["entries"]["patterns"][0]["object"]
        path = object_path(self.root(), part, "parquet")
        os.rename(path, path + ".real")
        os.symlink(path + ".real", path)
        self.assertIn("symlink", fails(run(self.root(), 1)))

    def test_symlinked_folder_is_refused(self):
        out = stores.build_minimal(self.root(), self.keys)
        h = out["manifest"]
        folder = os.path.dirname(object_path(self.root(), h, "json"))
        os.rename(folder, folder + "-real")
        os.symlink(folder + "-real", folder)
        self.assertIn("symlink", fails(run(self.root(), 1)))

    @unittest.skipUnless(hasattr(os, "mkfifo"), "needs named pipes")
    def test_fifo_does_not_hang(self):
        out = stores.build_minimal(self.root(), self.keys)
        path = object_path(self.root(), out["manifest"], "json")
        os.remove(path)
        os.mkfifo(path)
        self.assertIn("not-a-file", fails(run(self.root(), 1)))

    def test_hashes_never_become_paths(self):
        feed, ref, _, _ = current(stores.build_minimal(self.root(), self.keys)["writer"].root)
        for bad, code in (("sha256:../../../../etc/passwd", "bad-hash"), ("SHA256:" + "0" * 64, "unknown-hash-algorithm"),
                          ("md5:" + "0" * 32, "unknown-hash-algorithm"), ("sha256:" + "A" * 64, "bad-hash")):
            with self.subTest(bad=bad):
                doc = dict(ref, manifest=bad)
                StoreWriter(self.root(), marker=False).write(("refs", f"{feed}.json"), jcs.canonical(doc))
                report = run(self.root(), 1)
                self.assertTrue({"ref-manifest", "no-manifest"} & fails(report), report.text())
        store = Store(self.root())
        with self.assertRaises(StoreError):
            store.path("objects", "..", "x")
        with self.assertRaises(StoreError):
            store.object_components("sha256:" + "0" * 64, "../json")

    def test_unknown_hash_algorithm_in_tables(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d["tables"].update(tilings="sha512:" + "0" * 128), self.keys.a)
        self.assertIn("unknown-hash-algorithm", fails(run(self.root(), 1)))

    def test_misnamed_objects_are_reported(self):
        stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        w.write(("objects", "sha256", "zz", "x.json"), b"{}")
        w.write(("objects", "sha256", "00", "11" * 32 + ".json"), b"{}")
        w.write(("objects", "sha256", "00", "00" * 32 + ".exe"), b"{}")
        report = run(self.root(), 1)
        self.assertIn("object-name", fails(report))
        self.assertGreaterEqual(sum(1 for f in report.findings if f.code == "object-name"), 3)

    def test_store_marker(self):
        stores.build_minimal(self.root(), self.keys)
        os.remove(os.path.join(self.root(), "jetcarp-store.json"))
        self.assertIn("store-marker", fails(run(self.root(), 1)))
        StoreWriter(self.root(), marker=False).write(("jetcarp-store.json",), b'{"format":"jetcarp-store","version":"2.0"}')
        self.assertIn("unknown-major", fails(run(self.root(), 1)))


class Documents(StoreTest):
    def test_oversized_manifest(self):
        h = stores.build_minimal(self.root(), self.keys)["manifest"]
        self.assertIn("too-large", fails(run(self.root(), 1, Limits(document_bytes=500), manifest=h)))

    def test_deep_manifest(self):
        stores.build_minimal(self.root(), self.keys)
        deep = b'{"format":"jetcarp-pack","x":' + b"[" * 5000 + b"]" * 5000 + b"}"
        republish(self.root(), lambda d: None, self.keys.a, raw=lambda d: deep)
        report = run(self.root(), 1)
        self.assertIn("bad-json", fails(report))
        self.assertTrue(any("nests 5001 levels" in f.message for f in report.findings))

    def test_too_many_parts(self):
        stores.build_minimal(self.root(), self.keys)
        self.assertIn("index-parts", fails(run(self.root(), 1, Limits(parts_per_index=2))))

    def test_index_nesting(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        entries = out["entries"]["tilings"]
        child = w.index("tilings", entries)
        for _ in range(3):
            child = w.index("tilings", [], children=[child])
        republish(self.root(), lambda d: d["tables"].update(tilings=child), self.keys.a)
        self.assertClean(run(self.root(), 1, Limits(index_depth=4)))
        self.assertIn("index-depth", fails(run(self.root(), 1, Limits(index_depth=3))))

    def test_shared_children_are_read_once(self):
        """A child listed many times at many levels is a union, not an exponential walk."""
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        node = w.index("tilings", out["entries"]["tilings"])
        for _ in range(8 - 1):
            node = w.index("tilings", [], children=[node] * 50)
        republish(self.root(), lambda d: d["tables"].update(tilings=node), self.keys.a)
        report = run(self.root(), 1)
        self.assertClean(report)


class Parquet(StoreTest):
    def quickstart_part(self, rows, **options):
        stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        entry = w.part("quickstart", rows, **options)
        index = w.index("quickstart", [entry])
        republish(self.root(), lambda d: d["tables"].update(quickstart=index), self.keys.a)

    def scene_rows(self, n, scene):
        return [{"id": f"qs-{i:032x}", "title": "t", "description": "d", "source": None, "scene": scene,
                 "scene_format": "1", "rule": "B3/S23", "rule_family": "lifelike", "semantics": "1",
                 "behaviour": None, "pin_key": C.SQUARE_KEY, "pin": C.SQUARE_PIN, "tiling_group": "square"}
                for i in range(n)]

    def test_decompression_ratio(self):
        scene = json.dumps({"pad": "a" * 900_000})
        self.quickstart_part(self.scene_rows(8, scene), use_dictionary=False)
        report = run(self.root(), 1)
        self.assertIn("decompression-limit", fails(report))

    def test_dictionary_bomb(self):
        scene = json.dumps({"pad": secrets.token_hex(10_000)})
        self.quickstart_part(self.scene_rows(4000, scene))
        report = run(self.root(), 1, Limits(decoded_slack=1 << 20))
        self.assertIn("decoded-limit", fails(report))

    def test_string_cell(self):
        stores.build_minimal(self.root(), self.keys)
        self.assertIn("string-limit", fails(run(self.root(), 1, Limits(string_cell=100))))

    def test_phase_cells(self):
        stores.build_minimal(self.root(), self.keys)
        self.assertIn("phase-limit", fails(run(self.root(), 1, Limits(phase_cells=4))))

    def test_period(self):
        stores.build_minimal(self.root(), self.keys)
        self.assertIn("pattern-period", fails(run(self.root(), 1, Limits(period=2))))

    def test_nulls_are_refused_not_crashed_on(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        g = dict(out["records"]["glider"])
        g["phases"] = [[dict(c) for c in p] for p in g["phases"]]
        g["phases"][0][0]["i"] = None  # a null inside a non-null nested column
        b = dict(out["records"]["block"], period=None)  # a null in a non-null column
        index = w.index("patterns", [w.part("patterns", [g], loose=True), w.part("patterns", [b], loose=True)])
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        for level in (1, 3):
            report = run(self.root(), level)
            self.assertIn("null", fails(report))
            self.assertNotIn("check-error", fails(report))

    def test_not_parquet(self):
        stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        junk = b"PAR1" + b"\x00" * 64 + b"PAR1"
        h = w.put(junk, "parquet")
        index = w.index("patterns", [{"object": h, "rows": 1, "bytes": len(junk), "partition": {"class_group": "x",
                                      "tiling_group": "y"}, "first": "", "last": ""}])
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertTrue({"parquet-footer", "parquet-unreadable"} & fails(run(self.root(), 1)))

    def test_footer_limits(self):
        """A footer that claims more than 16 MiB, or nests Thrift deeper than 32, is refused before
        pyarrow reads it."""
        from jetcarp_pack import parquet_limits as pl
        import io
        big = b"PAR1" + b"\x00" * 20 + (17 << 20).to_bytes(4, "little") + b"PAR1"
        with self.assertRaisesRegex(pl.ParquetLimitError, "footer"):
            pl.check_footer(io.BytesIO(big), len(big))
        deep = b"\x1c" * 40 + b"\x00" * 41  # struct field 1 holding a struct, 40 deep
        data = b"PAR1" + deep + len(deep).to_bytes(4, "little") + b"PAR1"
        with self.assertRaisesRegex(pl.ParquetLimitError, "nests more than 32"):
            pl.check_footer(io.BytesIO(data), len(data))

    def test_partition_values(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        e = [dict(x) for x in out["entries"]["patterns"]]
        e[0]["partition"] = {"class_group": "../../x", "tiling_group": "square"}
        index = w.index("patterns", e)
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertIn("partition-value", fails(run(self.root(), 1)))

    def test_wrong_column_type(self):
        stores.build_minimal(self.root(), self.keys)
        import pyarrow as pa
        import pyarrow.parquet as pq
        t = pa.table({"pin_key": [C.SQUARE_KEY], "pin": [1]})
        sink = pa.BufferOutputStream()
        pq.write_table(t, sink)
        w = StoreWriter(self.root(), marker=False)
        data = sink.getvalue().to_pybytes()
        h = w.put(data, "parquet")
        index = w.index("tilings", [{"object": h, "rows": 1, "bytes": len(data), "partition": {},
                                     "first": C.SQUARE_KEY, "last": C.SQUARE_KEY}])
        republish(self.root(), lambda d: d["tables"].update(tilings=index), self.keys.a)
        self.assertTrue({"schema-column", "schema-type"} & fails(run(self.root(), 1)))


if __name__ == "__main__":
    unittest.main()
