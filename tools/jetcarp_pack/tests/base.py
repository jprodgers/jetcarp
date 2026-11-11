"""Helpers shared by the tests: test keys (made once per run, in a temporary folder), temporary
stores, and running a level into a Report."""

import atexit
import os
import shutil
import tempfile
import unittest

try:
    import pyarrow  # noqa: F401
except ImportError:  # the RFC 8785 tests still run; everything that reads Parquet is skipped
    raise unittest.SkipTest("needs pyarrow (see README.md)")

from jetcarp_pack import authenticity, integrity, jcs, replay
from jetcarp_pack.limits import Limits
from jetcarp_pack.pack import list_refs
from jetcarp_pack.report import FAIL, Report
from jetcarp_pack.store import Store

from . import stores
from .packwriter import StoreWriter

_KEYS = None


def keys():
    """Test keys, made with ssh-keygen in a temporary folder (never ~/.ssh) and removed at exit."""
    global _KEYS
    if _KEYS is None:
        folder = tempfile.mkdtemp(prefix="jetcarp-pack-test-keys-")
        atexit.register(shutil.rmtree, folder, True)
        _KEYS = stores.Keys(folder)
    return _KEYS


def run(root, level, limits=None, **kw):
    store = Store(root, limits or Limits())
    report = Report()
    if level == 1:
        integrity.check_store(store, report, kw.get("manifest"))
    elif level == 2:
        authenticity.check_authenticity(store, report, kw.get("manifest"), kw.get("state"), kw.get("accept", False))
    elif level == 3:
        replay.check_replay(store, report, kw.get("manifest"), kw.get("periods", 2))
    return report


def fails(report):
    return {f.code for f in report.findings if f.severity == FAIL}


def codes(report, severity):
    return {f.code for f in report.findings if f.severity == severity}


def current(root, feed=None):
    """(feed, ref, manifest bytes, manifest document) of a store's ref."""
    store = Store(root)
    refs = list_refs(store, Report())
    feed = feed or next(iter(refs))
    ref = refs[feed][1]
    data, doc = store.read_json_object(ref["manifest"], "manifest")
    return feed, ref, data, doc


def republish(root, change, key, *, seq=None, raw=None, publisher=None, sign=True, ref_key=None):
    """A new version made by `change(doc)` from the store's current manifest, signed by `key`, with
    its ref one seq up. `raw(doc)` may give other bytes than the canonical ones."""
    feed, ref, _, doc = current(root)
    change(doc)
    w = StoreWriter(root, marker=False)
    data = raw(doc) if raw else jcs.canonical(doc)
    h = w.put(data, "json")
    if sign:
        w.sign_manifest(h, data, key)
    w.ref(feed, ref["seq"] + 1 if seq is None else seq, h, publisher or doc["publisher"], stores.D2, ref_key or key)
    return h, data


class StoreTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="jetcarp-pack-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.keys = keys()

    def root(self, name="store"):
        return os.path.join(self.tmp, name)

    def assertClean(self, report, msg=None):
        bad = [f.line() for f in report.findings if f.severity == FAIL]
        self.assertEqual(bad, [], msg)

    def assertFails(self, report, code):
        self.assertIn(code, fails(report), "\n".join(f.line() for f in report.findings))
