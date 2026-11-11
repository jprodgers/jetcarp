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
    return {"glider": glider, "block": block, "blinker": blinker, "lwss": lwss, "ant": C.langtons_ant(),
            "quickstart": C.quickstart(glider, "Test glider", D1)}


def content(publisher, patterns, date, names=(), ants=(), rules=(C.LIFE,), quickstarts=(), rev=1):
    out = {"tilings": [C.square_tiling()], "rules": list(rules), "patterns": list(patterns), "ants": list(ants),
           "quickstart": list(quickstarts)}
    out.update(C.events(publisher, patterns, date, names={p["id"]: n for p, n in names}, ants=ants,
                        quickstarts=quickstarts, rev=rev))
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
    """One version: the square grid, B3/S23 and Brian's Brain, a glider, block and blinker, a Quick
    Start entry, and their events. `mutate(content)` may change the rows before writing
    (for tests)."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    c = content(keys.a.fingerprint, [r["glider"], r["block"], r["blinker"]], D1,
                names=[(r["glider"], "glider"), (r["block"], "block"), (r["blinker"], "blinker")],
                rules=(C.LIFE, C.BRAIN), quickstarts=[r["quickstart"]])
    if mutate:
        mutate(c)
    h, data, entries = publish(w, keys.a, "test-minimal", c, D1, title="Minimal test pack",
                               creators=[{"name": "Test Publisher A", "role": "creator"}])
    ref = w.ref("test-minimal", 1, h, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": h, "data": data, "entries": entries, "ref": ref, "records": r}


def build_merge(root, keys):
    """Two publishers' versions with the same glider, merged by A with two parents (A's previous
    version first) and signed on A's authorised laptop key."""
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
    imported = {"provenance": [C.provenance(f"pattern:{r['lwss']['id']}", keys.a.fingerprint, D2, C.SQUARE_PIN,
                                            kind="imported", search=None, pack=b1, source=None)]}
    m, _, _ = publish(w, keys.a, "test-a", imported, D2, previous=merged, signer=keys.a_laptop,
                      parents=[a1, b1], title="Merged test pack", version_label="test-merge")
    w.ref("test-a", 2, m, keys.a.fingerprint, D2, keys.a_laptop)
    return {"writer": w, "manifest": m, "parents": [a1, b1], "records": r}


def _retraction_v1(root, keys):
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    v1, data, e1 = publish(w, keys.a, "test-retraction",
                           content(keys.a.fingerprint, [r["glider"], r["blinker"]], D1,
                                   names=[(r["glider"], "glider")]), D1, title="Retraction test pack")
    return w, v1, e1, r


def build_retraction(root, keys):
    """Version 1, then version 2 retracting the blinker and renaming the glider (an annotation of
    rev 2, which wins over rev 1's); its other tables keep version 1's index hashes."""
    w, v1, e1, r = _retraction_v1(root, keys)
    ref1 = w.ref("test-retraction", 1, v1, keys.a.fingerprint, D1, keys.a)
    rename = {"annotations": [{"target": f"pattern:{r['glider']['id']}", "publisher": keys.a.fingerprint,
                               "field": "name", "value": '"test glider"', "score_model": None, "date": D2, "rev": 2}]}
    v2, _, _ = publish(w, keys.a, "test-retraction", rename, D2, previous=e1, parents=[v1],
                       title="Retraction test pack",
                       retracts=[{"target": f"pattern:{r['blinker']['id']}", "publisher": keys.a.fingerprint,
                                  "date": D2, "reason": "Test retraction: withdrawn to exercise retractions."}])
    w.ref("test-retraction", 2, v2, keys.a.fingerprint, D2, keys.a)
    return {"writer": w, "manifest": v2, "parents": [v1], "ref1": ref1, "records": r}


def build_rollback(root, keys):
    """The retraction store's version 1 with its ref at seq 1: refused after its seq-2 ref."""
    w, v1, _, _ = _retraction_v1(root, keys)
    w.ref("test-retraction", 1, v1, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": v1}


def build_group_raters(root, keys):
    """A taste pack (ratings, raters and a score model only), with a group rater whose two members
    have both consented; only the group agreed to share notes."""
    r = records()
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A")
    glider = f"pattern:{r['glider']['id']}"
    tables = {
        "raters": [
            {"rater": RATER_1, "consent_date": D0, "consent_scope": "public-permanent",
             "display_name": "Test Rater One", "orcid": None, "members": None, "notes": False},
            {"rater": RATER_2, "consent_date": D1, "consent_scope": "public-permanent",
             "display_name": None, "orcid": None, "members": None, "notes": False},
            {"rater": RATER_GROUP, "consent_date": D1, "consent_scope": "public-permanent",
             "display_name": None, "orcid": None, "members": [RATER_1, RATER_2], "notes": True},
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


def build_ant_run(root, keys):
    """Langton's ant on the square grid, alone (its `run` JSON in the shape pack-format.md gives)."""
    r = records()
    return build_small(root, keys, feed="test-ant",
                       tables={"tilings": [C.square_tiling()], "ants": [r["ant"]],
                               "provenance": [C.provenance(f"ant:{r['ant']['id']}", keys.a.fingerprint, D1,
                                                           C.SQUARE_PIN)]})


def build_revoked_machine_key(root, keys):
    """A version signed only by a machine key that the publisher's statement marks revoked."""
    w = StoreWriter(root)
    w.key_statement(keys.a, D0, name="Test Publisher A", authorised=[(keys.a_laptop, "laptop", True)])
    h, _, _ = publish(w, keys.a, "test-negative", {"tilings": [C.square_tiling()]}, D1, signer=keys.a_laptop)
    w.ref("test-negative", 1, h, keys.a.fingerprint, D1, keys.a)
    return {"writer": w, "manifest": h}


def _rotated(root, keys, revocation_signer):
    """R1, superseded by R2 (signed by R1), then R1 revoked; a version by R2."""
    w = StoreWriter(root)
    w.key_statement(keys.r2, D1, signer=keys.r1, supersedes=keys.r1.fingerprint, name="Test Publisher R")
    w.key_statement(keys.r1, D2, signer=revocation_signer, revoked=True, name="Test Publisher R")
    r = records()
    h, _, _ = publish(w, keys.r2, "test-rotation", content(keys.r1.fingerprint, [r["block"]], D3), D3)
    w.ref("test-rotation", 1, h, keys.r2.fingerprint, D3, keys.r2)
    return {"writer": w, "manifest": h}


def build_revoked_by_successor(root, keys):
    """R1's revocation is signed by its successor R2, so R2 stands (pack-format.md, "Revocation")."""
    return _rotated(root, keys, keys.r2)


def build_successor_of_self_revoked(root, keys):
    """R1 revoked itself; R2, signed by R1 and not by the revocation, does not stand."""
    return _rotated(root, keys, keys.r1)


def build_unsigned(root, keys):
    out = build_small(root, keys)
    shutil.rmtree(os.path.join(root, "sigs"))
    return out


def build_revoked_key(root, keys):
    """The minimal store, with the publisher's statement replaced by a revocation it signed itself."""
    out = build_minimal(root, keys)
    out["writer"].key_statement(keys.a, D2, revoked=True, name="Test Publisher A")
    return out


def build_bad_pin(root, keys):
    """The square grid's row with two touch offsets swapped and its pin and hashes kept."""
    tiling = C.square_tiling()
    touch = list(tiling["touch"][0])
    touch[0], touch[1] = touch[1], touch[0]
    tiling["touch"] = [touch]
    return build_small(root, keys, tables={"tilings": [tiling]})


def build_export(root, keys):
    """The minimal store's version, laid out as an export."""
    src = root + "-store"
    out = build_minimal(src, keys)
    write_export(src, out["manifest"], root)
    shutil.rmtree(src)
    return out


def write_export(store_root, manifest_hash, out):
    """An export as pack-format.md lays it out: pack.json, index/<hex>.json, tables/<table>/<key>=<value>/
    part-NNNNNN.parquet (numbered from 1 per partition folder, in index order), sigs/ and keys/."""
    import json

    def obj(h, ext):
        return os.path.join(store_root, "objects", "sha256", h[7:9], f"{h[7:]}.{ext}")

    def copy(src, *dest):
        path = os.path.join(out, *dest)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        shutil.copyfile(src, path)

    copy(obj(manifest_hash, "json"), "pack.json")
    with open(obj(manifest_hash, "json"), "rb") as f:
        manifest = json.loads(f.read())
    partition = {name: spec.partition for name, spec in schemas.TABLES.items()}
    for table, index_hash in sorted(manifest["tables"].items()):
        counters, stack = {}, [index_hash]
        while stack:
            h = stack.pop()
            copy(obj(h, "json"), "index", f"{h[7:]}.json")
            with open(obj(h, "json"), "rb") as f:
                index = json.loads(f.read())
            for entry in index["parts"]:
                folder = ("tables", table) + tuple(f"{k}={entry['partition'][k]}" for k in partition[table])
                counters[folder] = counters.get(folder, 0) + 1
                copy(obj(entry["object"], "parquet"), *folder, f"part-{counters[folder]:06d}.parquet")
            stack += list(reversed(index["children"]))
    hexd = manifest_hash[7:]
    sig_folder = os.path.join(store_root, "sigs", "sha256", hexd[:2], hexd)
    for name in sorted(os.listdir(sig_folder)):
        copy(os.path.join(sig_folder, name), "sigs", name)
    for name in sorted(os.listdir(os.path.join(store_root, "keys"))):
        copy(os.path.join(store_root, "keys", name), "keys", name)
    with open(os.path.join(out, "README.md"), "w", encoding="utf-8") as f:
        f.write(f"# {manifest['title']}\n\nAn export of pack version `{manifest_hash}`, made by the test writer "
                "of the independent pack reader.\n")


GOLDEN = {
    "minimal": build_minimal,
    "export": build_export,
    "merge": build_merge,
    "retraction": build_retraction,
    "group-raters": build_group_raters,
    "ant-run": build_ant_run,
    "key-rotation": build_key_rotation,
    "revoked-by-successor": build_revoked_by_successor,
    "negative/successor-of-self-revoked": build_successor_of_self_revoked,
    "negative/revoked-machine-key": build_revoked_machine_key,
    "negative/tampered-object": build_tampered_object,
    "negative/bad-signature": build_bad_signature,
    "negative/unsigned": build_unsigned,
    "negative/unknown-requires": build_unknown_requires,
    "negative/unknown-major": build_unknown_major,
    "negative/revoked-key": build_revoked_key,
    "negative/bad-pin": build_bad_pin,
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
