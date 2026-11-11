"""The committed golden stores (testdata/golden.tar): each is checked against EXPECTED.json, the
accepted ones pass every level, and each negative one is refused for its stated reason. The tar is
unpacked into a temporary folder; on Windows it can't be, since the spec's file names hold a colon."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import json
import os
import shutil
import tempfile
import unittest

from jetcarp_pack import sshsig
from jetcarp_pack.report import FAIL

from . import content as C
from .base import fails, run
from .make_testdata import extract_tar

TESTDATA = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "testdata")
with open(os.path.join(TESTDATA, "EXPECTED.json"), encoding="utf-8") as _f:
    EXPECTED = json.load(_f)
needs_ssh = unittest.skipUnless(sshsig.available(), "needs ssh-keygen")
_UNPACKED = None


def setUpModule():
    global _UNPACKED
    if os.name == "nt":
        raise unittest.SkipTest("the stores' file names hold a colon, which Windows does not allow")
    _UNPACKED = tempfile.mkdtemp(prefix="jetcarp-pack-golden-")
    extract_tar(os.path.join(TESTDATA, EXPECTED["archive"]), _UNPACKED)


def tearDownModule():
    if _UNPACKED:
        shutil.rmtree(_UNPACKED, True)


def store(name):
    return os.path.join(_UNPACKED, *name.split("/"))


class Golden(unittest.TestCase):
    def test_refs_match_expected(self):
        for name, exp in EXPECTED["stores"].items():
            for feed, ref in exp["refs"].items():
                with open(os.path.join(store(name), "refs", f"{feed}.json"), encoding="utf-8") as f:
                    doc = json.load(f)
                self.assertEqual((doc["seq"], doc["manifest"]), (ref["seq"], ref["manifest"]), name)

    def test_accepted_stores_pass_levels_1_and_3(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] != "accept":
                continue
            for level in (1, 3):
                with self.subTest(store=name, level=level):
                    report = run(store(name), level)
                    self.assertEqual([f.line() for f in report.findings if f.severity == FAIL], [])

    @needs_ssh
    def test_accepted_stores_pass_level_2(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] == "accept":
                with self.subTest(store=name):
                    report = run(store(name), 2)
                    self.assertEqual([f.line() for f in report.findings if f.severity == FAIL], [])

    @needs_ssh
    def test_refused_stores(self):
        for name, exp in EXPECTED["stores"].items():
            if exp["expect"] != "refuse" or exp["refusal"] == "seq-rollback":
                continue
            with self.subTest(store=name):
                got = fails(run(store(name), 1)) | fails(run(store(name), 2))
                self.assertIn(exp["refusal"], got)

    def test_minimal_holds_the_books_glider(self):
        report = run(store("minimal"), 1)
        self.assertEqual(report.passed[(1, "pattern content ids")], 3)
        path = os.path.join(store("minimal"), "objects")
        self.assertTrue(os.path.isdir(path))
        from jetcarp_pack.pack import Pack
        from jetcarp_pack.store import Store
        pack = Pack(Store(store("minimal")), EXPECTED["stores"]["minimal"]["manifest"])
        self.assertIn(C.GLIDER_ID, {r["id"] for _, _, r in pack.rows("patterns")})


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
