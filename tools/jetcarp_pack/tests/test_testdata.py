"""The committed test stores in testdata/: each is checked against EXPECTED.json, the accepted ones
pass every level, and each negative one is refused for its stated reason."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import json
import os
import shutil
import tempfile
import unittest

from jetcarp_pack import sshsig
from jetcarp_pack.pack import Pack
from jetcarp_pack.report import FAIL
from jetcarp_pack.store import open_store

from . import content as C
from .base import fails, run

TESTDATA = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "testdata")
with open(os.path.join(TESTDATA, "EXPECTED.json"), encoding="utf-8") as _f:
    EXPECTED = json.load(_f)
needs_ssh = unittest.skipUnless(sshsig.available(), "needs ssh-keygen")


def store(name):
    return os.path.join(TESTDATA, *name.split("/"))


def failures(report):
    return [f.line() for f in report.findings if f.severity == FAIL]


class Stores(unittest.TestCase):
    def test_refs_match_expected(self):
        for name, exp in EXPECTED["stores"].items():
            for feed, ref in exp["refs"].items():
                with open(os.path.join(store(name), "refs", f"{feed}.json"), encoding="utf-8") as f:
                    doc = json.load(f)
                self.assertEqual((doc["seq"], doc["manifest"]), (ref["seq"], ref["manifest"]), name)

    def test_accepted_stores_pass_levels_1_and_3(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] == "accept":
                for level in (1, 3):
                    with self.subTest(store=name, level=level):
                        self.assertEqual(failures(run(store(name), level)), [])

    @needs_ssh
    def test_accepted_stores_pass_level_2(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] == "accept":
                with self.subTest(store=name):
                    self.assertEqual(failures(run(store(name), 2)), [])

    @needs_ssh
    def test_refused_stores(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] != "refuse" or exp["refusal"] == "seq-rollback":
                continue
            with self.subTest(store=name):
                got = fails(run(store(name), 1)) | fails(run(store(name), 2))
                self.assertIn(exp["refusal"], got)

    def test_minimal_holds_the_books_glider(self):
        pack = Pack(open_store(store("minimal")), EXPECTED["stores"]["minimal"]["manifest"])
        self.assertIn(C.GLIDER_ID, {r["id"] for _, _, r in pack.rows("patterns")})

    def test_export_is_the_minimal_version(self):
        self.assertEqual(EXPECTED["stores"]["export"]["manifest"], EXPECTED["stores"]["minimal"]["manifest"])
        self.assertEqual(open_store(store("export")).manifest_hash, EXPECTED["stores"]["minimal"]["manifest"])


@needs_ssh
class Following(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="jetcarp-pack-state-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.state = os.path.join(self.tmp, "state.json")

    def test_rollback_after_retraction(self):
        self.assertEqual(fails(run(store("retraction"), 2, state=self.state, accept=True)), set())
        self.assertIn("seq-rollback", fails(run(store("negative/rollback"), 2, state=self.state)))

    def test_key_rotation_continues_the_feed(self):
        r1 = EXPECTED["keys"]["Test Publisher R"]["fingerprint"]
        v1 = EXPECTED["stores"]["key-rotation"]["parents"][0]
        with open(self.state, "w") as f:
            json.dump({"format": "jetcarp-pack-reader-state", "version": "1.0",
                       "feeds": {f"{r1} test-rotation": {"seq": 1, "manifest": v1}}}, f)
        self.assertEqual(fails(run(store("key-rotation"), 2, state=self.state, accept=True)), set())
        with open(self.state) as f:
            self.assertEqual(json.load(f)["feeds"][f"{r1} test-rotation"]["seq"], 2)


if __name__ == "__main__":
    unittest.main()
