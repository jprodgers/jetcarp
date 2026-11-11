"""Write the golden test stores as testdata/golden.tar, with testdata/EXPECTED.json.

    cd tools/jetcarp_pack && python3 -m tests.make_testdata [OUT]

The stores are one uncompressed tar, not folders, because the spec's signature and key file names
hold a colon (`SHA256:...`), which Git for Windows refuses to check out (SPEC_QUESTIONS.md A3). The
tar is deterministic: members sorted, times 0, owners empty. With OUT given, the stores are also left
unpacked there.

Keys are made fresh with ssh-keygen in a temporary folder and thrown away afterwards, so running this
again gives new fingerprints, signatures and manifest hashes: the committed stores are the golden
ones, and this script is how they were made.
"""

import io
import json
import os
import sys
import tarfile
import tempfile

from jetcarp_pack import jcs

from . import stores

TESTDATA = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "testdata")


def write_tar(src, tar_path):
    members = []
    for dirpath, dirnames, filenames in os.walk(src):
        dirnames.sort()
        for name in filenames:
            members.append(os.path.relpath(os.path.join(dirpath, name), src).replace(os.sep, "/"))
    with tarfile.open(tar_path, "w", format=tarfile.USTAR_FORMAT) as tar:
        for rel in sorted(members):
            with open(os.path.join(src, *rel.split("/")), "rb") as f:
                data = f.read()
            info = tarfile.TarInfo(rel)
            info.size, info.mtime, info.mode = len(data), 0, 0o644
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            tar.addfile(info, io.BytesIO(data))


def extract_tar(tar_path, dest):
    """Unpack golden.tar (plain files only, no absolute or parent paths)."""
    with tarfile.open(tar_path) as tar:
        for m in tar.getmembers():
            parts = m.name.split("/")
            if not m.isfile() or m.name.startswith("/") or ".." in parts:
                raise ValueError(f"unexpected member {m.name!r} in {tar_path}")
            path = os.path.join(dest, *parts)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(tar.extractfile(m).read())

EXPECT = {
    "minimal": ("accept", None, "One version: square grid, B3/S23 and B2/S/C3, glider, block, blinker, "
                                "Langton's ant, and their events. The glider's id is the book's."),
    "merge": ("accept", None, "Feeds test-a and test-b; test-a seq 2 is a merge of both (two parents), "
                              "signed by A's authorised laptop key. The glider is in both parents."),
    "retraction": ("accept", None, "Version 2 keeps version 1's index hashes and retracts the blinker."),
    "group-raters": ("accept", None, "A taste pack: a group rater whose two members have consented."),
    "key-rotation": ("accept", None, "R2 supersedes R1 (statement signed by R1); version 2 and its ref "
                                     "are by R2. A follower that accepted seq 1 from R1 accepts seq 2."),
    "negative/tampered-object": ("refuse", "object-hash", "One byte of the patterns part is flipped."),
    "negative/bad-signature": ("refuse", "bad-signature", "The manifest's signature is over other bytes."),
    "negative/unknown-requires": ("refuse", "unknown-requires", 'requires ["test-unknown-feature"].'),
    "negative/unknown-major": ("refuse", "unknown-major", 'version "2.0".'),
    "negative/rollback": ("refuse", "seq-rollback", "Accept retraction's ref (seq 2) first; this ref for "
                                                    "the same feed and publisher has seq 1 and must be refused."),
}


def main(out=None):
    with tempfile.TemporaryDirectory(prefix="jetcarp-pack-stores-") as tmp:
        root = out or os.path.join(tmp, "stores")
        expected = build_all(root)
        write_tar(root, os.path.join(TESTDATA, "golden.tar"))
    with open(os.path.join(TESTDATA, "EXPECTED.json"), "wb") as f:
        f.write(jcs.canonical(expected) + b"\n")
    return expected


def build_all(out):
    expected = {"format": "jetcarp-pack-testdata", "version": "1", "archive": "golden.tar", "keys": {},
                "stores": {}}
    with tempfile.TemporaryDirectory(prefix="jetcarp-pack-keys-") as tmp:
        keys = stores.Keys(tmp)
        for attr in ("a", "a_laptop", "b", "r1", "r2"):
            k = getattr(keys, attr)
            expected["keys"][k.name] = {"fingerprint": k.fingerprint, "key": k.line}
        for name, build in stores.GOLDEN.items():
            root = stores.fresh(os.path.join(out, name))
            result = build(root, keys)
            refs = {}
            for f in sorted(os.listdir(os.path.join(root, "refs"))):
                with open(os.path.join(root, "refs", f), "rb") as fh:
                    doc = json.loads(fh.read())
                refs[doc["feed"]] = {"seq": doc["seq"], "manifest": doc["manifest"], "publisher": doc["publisher"]}
            expect, code, notes = EXPECT[name]
            expected["stores"][name] = {"expect": expect, "refusal": code, "manifest": result["manifest"],
                                        "parents": result.get("parents", []), "refs": refs, "notes": notes}
    return expected


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else None)
