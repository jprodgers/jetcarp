"""Builders for the test stores: the golden ones in testdata/ and variants the unit tests tamper with.

Every name in them is invented ("Test Publisher A"); no real person's data is used.
"""

import os
import shutil

from jetcarp_pack import schemas
from jetcarp_pack.store import signer_name

from . import content as C
from .packwriter import StoreWriter, TestKey

D0, D1, D2, D3 = "2026-11-01T09:00:00Z", "2026-11-02T10:00:00Z", "2026-11-03T11:00:00Z", "2026-11-04T12:00:00Z"
RATER_1, RATER_2, RATER_GROUP = "rater-1f0c9a7d2b3e4c5a", "rater-6b2d8e4f1a3c5b7d", "rater-9e8d7c6b5a4f3e2d"


class Keys:
    def __init__(self, folder):
        self.a = TestKey(folder, "Test Publisher A")
        self.a_laptop = TestKey(folder, "Test Publisher A laptop")
        self.b = TestKey(folder, "Test Publisher B")
        self.r1 = TestKey(folder, "Test Publisher R")
        self.r2 = TestKey(folder, "Test Publisher R new key")


def records():
    glider = C.pattern(C.LIFE, C.GLIDER, speed="c/4 diagonal")
    block = C.pattern(C.LIFE, C.BLOCK)
    blinker = C.pattern(C.LIFE, C.BLINKER)
    lwss = C.pattern(C.LIFE, C.LWSS, speed="c/2 orthogonal")
    return {"glider": glider, "block": block, "blinker": blinker, "lwss": lwss, "ant": C.langtons_ant()}


def content(publisher, patterns, date, names=(), ants=(), rules=(C.LIFE,)):
    out = {"tilings": [C.square_tiling()], "rules": list(rules), "patterns": list(patterns), "ants": list(ants)}
    out.update(C.events(publisher, patterns, date, names={p["id"]: n for p, n in names}, ants=ants))
    return out


def publish(w, key, feed, tables_content, created, *, signer=None, previous=None, publisher=None, **fields):
    tables, entries = w.tables(tables_content, previous)
    count = {t: sum(e["rows"] for e in entries.get(t, [])) for t in ("patterns", "ants", "quickstart")}
    count["bytes"] = sum(e["bytes"] for es in entries.values() for e in es)
    fields.setdefault("stats", count)
    h, data = w.manifest(publisher or key.fingerprint, feed, tables, created, **fields)
    w.sign_manifest(h, data, signer or key)
    return h, data, entries


def build_minimal(root, keys, mutate=None):
    """One version: the square grid, B3/S23 and Brian's Brain, a glider, block and blinker, Langton's
    ant, and their events. `mutate(content)` may change the rows before writing (for tests)."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    c = content(keys.a.fingerprint, [r["glider"], r["block"], r["blinker"]], D1,
                names=[(r["glider"], "glider"), (r["block"], "block"), (r["blinker"], "blinker")],
                ants=[r["ant"]], rules=(C.LIFE, C.BRAIN))
    if mutate:
        mutate(c)
    h, data, entries = publish(w, keys.a, "test-minimal", c, D1, title="Minimal test pack",
                               creators=[{"name": "Test Publisher A", "role": "creator"}])
    ref = w.ref("test-minimal", 1, h, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": h, "data": data, "entries": entries, "ref": ref, "records": r}


def build_merge(root, keys):
    """Two publishers' versions with the same glider, merged by A with two parents and signed on A's
    authorised laptop key (section 24 fix 1)."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A", authorised=[(keys.a_laptop, "laptop")])
    w.key_statement(keys.b, D0, name="Test Publisher B")
    a1, _, ea = publish(w, keys.a, "test-a", content(keys.a.fingerprint, [r["glider"], r["block"]], D1,
                                                     names=[(r["glider"], "glider")]), D1, title="Test pack A")
    w.ref("test-a", 1, a1, keys.a.fingerprint, D1, keys.a)
    b1, _, eb = publish(w, keys.b, "test-b", content(keys.b.fingerprint, [r["glider"], r["lwss"]], D1,
                                                     names=[(r["lwss"], "LWSS")]), D1, title="Test pack B",
                        creators=[{"name": "Test Publisher B", "role": "creator"}])
    w.ref("test-b", 1, b1, keys.b.fingerprint, D1, keys.b)
    merged = {t: ea.get(t, []) + eb.get(t, []) for t in sorted(set(ea) | set(eb))}
    imported = {"provenance": [{
        "target": f"pattern:{r['lwss']['id']}", "publisher": keys.a.fingerprint, "seq": 0, "kind": "imported",
        "finder": None, "orcid": None, "search_kind": None, "search": None, "engine_version": "0.4.0",
        "pin": C.SQUARE_PIN, "date": D2, "source": b1}]}
    m, _, _ = publish(w, keys.a, "test-a", imported, D2, previous=merged, signer=keys.a_laptop,
                      parents=[a1, b1], title="Merged test pack", version_label="test-merge")
    w.ref("test-a", 2, m, keys.a.fingerprint, D2, keys.a_laptop)
    return {"writer": w, "manifest": m, "parents": [a1, b1], "records": r}


def _retraction_v1(root, keys):
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    v1, data, e1 = publish(w, keys.a, "test-retraction",
                           content(keys.a.fingerprint, [r["glider"], r["blinker"]], D1), D1, title="Retraction test pack")
    return w, v1, e1, r


def build_retraction(root, keys):
    """Version 1, then version 2 with the same tables (the same index hashes) retracting the blinker."""
    w, v1, e1, r = _retraction_v1(root, keys)
    ref1 = w.ref("test-retraction", 1, v1, keys.a.fingerprint, D1, keys.a)
    v2, _, _ = publish(w, keys.a, "test-retraction", {}, D2, previous=e1, parents=[v1], title="Retraction test pack",
                       retracts=[{"target": f"pattern:{r['blinker']['id']}", "date": D2,
                                  "reason": "Test retraction: withdrawn to exercise retractions."}])
    w.ref("test-retraction", 2, v2, keys.a.fingerprint, D2, keys.a)
    return {"writer": w, "manifest": v2, "parents": [v1], "ref1": ref1, "records": r}


def build_rollback(root, keys):
    """The retraction store's version 1 with its ref at seq 1: refused after its seq-2 ref."""
    w, v1, _, _ = _retraction_v1(root, keys)
    w.ref("test-retraction", 1, v1, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": v1}


def build_group_raters(root, keys):
    """A taste pack (ratings, raters and a score model only), with a group rater whose two members
    have both consented (section 25 item 4)."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    glider = f"pattern:{r['glider']['id']}"
    tables = {
        "raters": [
            {"rater": RATER_1, "consent_date": D0, "consent_scope": "public-permanent",
             "display_name": "Test Rater One", "orcid": None, "members": None},
            {"rater": RATER_2, "consent_date": D1, "consent_scope": "public-permanent",
             "display_name": None, "orcid": None, "members": None},
            {"rater": RATER_GROUP, "consent_date": D1, "consent_scope": "public-permanent",
             "display_name": None, "orcid": None, "members": [RATER_1, RATER_2]},
        ],
        "ratings": [
            {"rater": RATER_GROUP, "target": glider, "scale": "keep-maybe-drop 1", "verdict": "keep",
             "note": "Agreed together: a clean, fast ship.", "round": "round 1", "date": D1},
            {"rater": RATER_1, "target": glider, "scale": "stars-1-5 1", "verdict": "5", "note": None,
             "round": "round 2", "date": D2},
            {"rater": RATER_2, "target": f"search_run:{C.SEARCH}:17", "scale": "keep-maybe-drop 1",
             "verdict": "maybe", "note": None, "round": "round 2", "date": D2},
        ],
        "score_models": [
            {"model": "test-taste", "version": "1", "kind": "logistic 1", "features": ["period", "cells"],
             "weights": [0.5, -0.25], "intercept": 0.125, "fitted_on": "round 1: 1 group rating",
             "rater": RATER_GROUP, "metrics": '{"auc":"0.75"}'},
        ],
    }
    h, _, _ = publish(w, keys.a, "test-taste", tables, D2, title="Test taste pack")
    w.ref("test-taste", 1, h, keys.a.fingerprint, D2, keys.a)
    return {"writer": w, "manifest": h}


def build_key_rotation(root, keys):
    """Version 1 by key R1; R2 supersedes R1 (signed by R1); version 2 by R2, its ref signed by R2."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.r1, D0, name="Test Publisher R")
    v1, _, e1 = publish(w, keys.r1, "test-rotation", content(keys.r1.fingerprint, [r["block"]], D1), D1,
                        title="Key rotation test pack")
    w.key_statement(keys.r2, D2, signer=keys.r1, supersedes=keys.r1.fingerprint, name="Test Publisher R")
    v2, _, _ = publish(w, keys.r2, "test-rotation", {}, D3, previous=e1, parents=[v1], title="Key rotation test pack")
    w.ref("test-rotation", 2, v2, keys.r2.fingerprint, D3, keys.r2)
    return {"writer": w, "manifest": v2, "parents": [v1]}


def build_small(root, keys, feed="test-negative", tables=None, **fields):
    """A store with one small version (the square tiling only, unless `tables` is given)."""
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    t = {"tilings": [C.square_tiling()]} if tables is None else tables
    h, data, entries = publish(w, keys.a, feed, t, D1, **fields)
    w.ref(feed, 1, h, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": h, "data": data, "entries": entries}


def build_tampered_object(root, keys):
    r = records()
    out = build_small(root, keys, tables={"tilings": [C.square_tiling()], "patterns": [r["glider"]]})
    part = out["entries"]["patterns"][0]["object"][7:]
    path = os.path.join(root, "objects", "sha256", part[:2], f"{part}.parquet")
    with open(path, "r+b") as f:
        f.seek(100)
        b = f.read(1)
        f.seek(100)
        f.write(bytes([b[0] ^ 0x01]))
    return out


def build_bad_signature(root, keys):
    out = build_small(root, keys)
    w, h, data = out["writer"], out["manifest"], out["data"]
    w.sign_manifest(h, data, keys.a, payload=data + b" ")  # a signature over other bytes
    return out


def build_unknown_requires(root, keys):
    return build_small(root, keys, tables={}, requires=["test-unknown-feature"])


def build_unknown_major(root, keys):
    return build_small(root, keys, tables={}, version="2.0")


GOLDEN = {
    "minimal": build_minimal,
    "merge": build_merge,
    "retraction": build_retraction,
    "group-raters": build_group_raters,
    "key-rotation": build_key_rotation,
    "negative/tampered-object": build_tampered_object,
    "negative/bad-signature": build_bad_signature,
    "negative/unknown-requires": build_unknown_requires,
    "negative/unknown-major": build_unknown_major,
    "negative/rollback": build_rollback,
}


def fresh(root):
    if os.path.exists(root):
        shutil.rmtree(root)
    return root


def signer_file(key):
    return signer_name(key.fingerprint)


def table_names():
    return sorted(schemas.TABLES)
