"""Write the test stores into testdata/ (or the folder given), with EXPECTED.json.

    cd tools/jetcarp_pack && python3 -m tests.make_testdata [OUT]

Keys are made fresh with ssh-keygen in a temporary folder and thrown away afterwards, so running this
again gives new fingerprints, signatures and manifest hashes: the committed stores are the ones the
cross-check used, and this script is how they were made.
"""

import json
import os
import shutil
import sys
import tempfile

from jetcarp_pack import jcs

from . import stores

TESTDATA = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "testdata")

EXPECT = {
    "minimal": ("accept", None, "One version: square grid, B3/S23 and B2/S/C3, glider, block, blinker, "
                                "a Quick Start entry, and their events. The glider's id is the book's."),
    "export": ("accept", None, "The minimal store's version as an export (pack.json, index/, tables/, sigs/, keys/)."),
    "merge": ("accept", None, "Feeds test-a and test-b; test-a seq 2 merges both (parents: A's version, then B's), "
                              "signed by A's authorised laptop key. The glider is in both parents."),
    "retraction": ("accept", None, "Version 2 retracts the blinker and renames the glider (rev 2); its other "
                                   "tables keep version 1's index hashes."),
    "group-raters": ("accept", None, "A taste pack: a group rater whose two members have consented."),
    "ant-run": ("accept", None, "Langton's ant alone, its run column in the shape pack-format.md gives "
                                "(limits, and the run: outcome, steps, cells, reach, period, d, onset, speed)."),
    "key-rotation": ("accept", None, "R2 supersedes R1 (statement signed by R1); version 2 and its ref "
                                     "are by R2. A follower that accepted seq 1 from R1 accepts seq 2."),
    "revoked-by-successor": ("accept", None, "R2 supersedes R1; R1's revocation is signed by R2, so R2 stands. "
                                             "The version and its ref are by R2."),
    "negative/successor-of-self-revoked": ("refuse", "revoked-key", "R2 supersedes R1, and R1 revoked itself: R2 "
                                                                    "did not sign the revocation, so it does not stand."),
    "negative/revoked-machine-key": ("refuse", "no-publisher-signature", "Signed only by a machine key the "
                                                                         "publisher's statement marks revoked."),
    "negative/tampered-object": ("refuse", "object-hash", "One byte of the patterns part is flipped."),
    "negative/bad-signature": ("refuse", "bad-signature", "The manifest's signature is over other bytes."),
    "negative/unsigned": ("refuse", "unsigned", "The manifest has no signature (JETCARP: unless --allow-unsigned)."),
    "negative/unknown-requires": ("refuse", "unknown-requires", 'requires ["test-unknown-feature"].'),
    "negative/unknown-major": ("refuse", "unknown-major", 'version "2.0".'),
    "negative/revoked-key": ("refuse", "revoked-key", "The publisher's key statement is a revocation it signed itself."),
    "negative/bad-pin": ("refuse", "pin", "The square grid's row has one touch offset swapped, so its geometry "
                                          "does not give its pin."),
    "negative/rollback": ("refuse", "seq-rollback", "Accept retraction's ref (seq 2) first; this ref for "
                                                    "the same feed and publisher has seq 1 and must be refused."),
}


def main(out=None):
    out = out or TESTDATA
    expected = build_all(out)
    with open(os.path.join(out, "EXPECTED.json"), "wb") as f:
        f.write(jcs.canonical(expected) + b"\n")
    return expected


def build_all(out):
    expected = {"format": "jetcarp-pack-testdata", "version": "2", "keys": {}, "stores": {}}
    with tempfile.TemporaryDirectory(prefix="jetcarp-pack-keys-") as tmp:
        keys = stores.Keys(tmp)
        for attr in ("a", "a_laptop", "b", "r1", "r2"):
            k = getattr(keys, attr)
            expected["keys"][k.name] = {"fingerprint": k.fingerprint, "key": k.line}
        for name, build in stores.GOLDEN.items():
            root = stores.fresh(os.path.join(out, name))
            result = build(root, keys)
            refs = {}
            if os.path.isdir(os.path.join(root, "refs")):
                for f in sorted(os.listdir(os.path.join(root, "refs"))):
                    with open(os.path.join(root, "refs", f), "rb") as fh:
                        doc = json.loads(fh.read())
                    refs[doc["feed"]] = {"seq": doc["seq"], "manifest": doc["manifest"], "publisher": doc["publisher"]}
            expect, code, notes = EXPECT[name]
            expected["stores"][name] = {"expect": expect, "refusal": code, "manifest": result["manifest"],
                                        "parents": result.get("parents", []), "refs": refs, "notes": notes}
    return expected


if __name__ == "__main__":
    target = sys.argv[1] if len(sys.argv) > 1 else None
    if target is None:
        for name in os.listdir(TESTDATA):
            path = os.path.join(TESTDATA, name)
            if os.path.isdir(path):
                shutil.rmtree(path)
            elif name != "README.md":
                os.remove(path)
    main(target)
