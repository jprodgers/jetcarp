"""Conformance level 2, authenticity (pack-format.md: "Refs", "Signatures and keys"): signatures on
manifests, refs and key statements, the chain of key statements (machine keys, successors,
revocations), and refs' sequence numbers.

The rules, as the spec gives them:
- Signatures are sshsig, by Ed25519 keys only; a signature by another kind of key counts as none.
- A manifest's detached signature is taken to be by the key inside it, and the file's `<signer>`
  (the 64 hex digits of that key's SHA-256) must be that key's.
- A first key statement is signed by its own key; a successor by the key it supersedes, whose own
  statement must stand; a revocation by the revoked key or by a successor that key signed.
- A revoked key signs nothing, whatever the dates say. A successor stands after its predecessor's
  revocation only if it signed that revocation itself. A revocation, once seen, is kept for good (in
  the reader's state file).
- The publisher's id is the first key of its chain. Manifests and refs name the current key; refs
  are signed by it or by a machine key its statement authorises (and has not revoked).
- A follower keeps the last (seq, manifest) per (publisher id, feed): the same pair again is no
  update; the same seq with another manifest, or a lower seq, is refused.
"""

import base64
import hashlib
import json
import os

from . import identity as ident
from . import jcs, safejson, sshsig
from .integrity import manifests_to_check
from .pack import check_store_marker, list_refs, sh, check_fingerprint
from .store import SIGNER_RE, StoreError, check_hash, fingerprint_of_signer, signer_name

STATE_FORMAT = "jetcarp-pack-reader-state"
ED25519 = "ssh-ed25519"


class Key:
    def __init__(self, fp, doc, where):
        self.fp, self.doc, self.where = fp, doc, where
        self.line = doc.get("key")
        self.supersedes = doc.get("supersedes")
        self.revoked = doc.get("revoked") is True
        self.authorised = {}  # fingerprint -> (key line, revoked)
        self.verified = False  # its signature is by the key that must sign it
        self.signed_by = None
        self.stands = False


class KeyRing:
    """Every key statement in keys/, checked, with revocations already known."""

    def __init__(self, store, report, known_revoked=()):
        self.store, self.r = store, report
        self.keys = {}
        self.revoked = set(known_revoked)
        self.timeout = store.limits.subprocess_seconds
        try:
            names = store.list(("keys",))
        except StoreError as e:
            self.r.fail(2, e.code, "keys/", e.message)
            names = []
        for name in names:
            self._load(name, f"keys/{name}")
        for key in self.keys.values():
            if not key.revoked:
                self._verify_statement(key)
        for key in self.keys.values():
            if key.revoked:
                self._verify_revocation(key)
        for fp in sorted(self.revoked):
            k = self.keys.get(fp)
            if k is not None and not k.revoked:
                self.r.warn(2, "revocation-kept", k.where, "serves a statement that does not revoke the key, but the "
                                                           "key was revoked before: it stays revoked")
        self._settle()
        n = sum(1 for k in self.keys.values() if k.stands)
        if n:
            self.r.ok(2, "key statements standing", n)

    def _load(self, name, where):
        if not name.endswith(".json"):
            self.r.warn(2, "key-name", where, "is not <signer>.json")
            return
        try:
            fp = fingerprint_of_signer(name[:-5])
            raw, doc = self.store.read_json_file(("keys", name), where)
        except StoreError as e:
            self.r.fail(2, e.code, where, e.message)
            return
        if not isinstance(doc, dict) or doc.get("format") != "jetcarp-key":
            self.r.fail(2, "key-format", where, 'format is not "jetcarp-key"')
            return
        ok, known = ident.is_version(doc.get("version"))
        if not ok or not known:
            self.r.fail(2, "unknown-major", where, f"version {doc.get('version')!r} is not 1.x; refused")
            return
        if not safejson.is_canonical(raw, doc):
            self.r.fail(2, "not-canonical", where, "is not in RFC 8785 canonical form; refused")
            return
        if doc.get("fingerprint") != fp:
            self.r.fail(2, "key-fingerprint", where, f"names fingerprint {doc.get('fingerprint')!r}, not its file's {fp}")
            return
        try:
            kind, _ = sshsig.parse_public_key(doc.get("key"))
            actual = sshsig.key_fingerprint(doc.get("key"))
        except sshsig.SigError as e:
            self.r.fail(2, "key-format", where, str(e))
            return
        if actual != fp:
            self.r.fail(2, "key-fingerprint", where, f"its key's fingerprint is {actual}, not {fp}")
            return
        if kind != ED25519:
            self.r.fail(2, "key-type", where, f"holds a {kind} key; format 1.0 uses Ed25519 keys only")
            return
        if not ident.is_time(doc.get("created")):
            self.r.fail(2, "key-format", where, f"created {doc.get('created')!r} is not an RFC 3339 UTC time")
        sup = doc.get("supersedes")
        if sup is not None and not check_fingerprint(sup):
            self.r.fail(2, "key-format", where, f"supersedes {sup!r} is not a fingerprint")
            return
        if not isinstance(doc.get("revoked", False), bool):
            self.r.fail(2, "key-format", where, "revoked is not true or false")
            return
        for member in ("name", "orcid", "github"):
            if member in doc and (not isinstance(doc[member], str) or not doc[member]):
                self.r.warn(2, "key-format", where, f"{member} is empty; empty members are left out")
        if doc.get("orcid") and not ident.is_orcid(doc["orcid"]):
            self.r.warn(2, "orcid", where, f"ORCID {doc['orcid']!r} has a wrong check digit")
        key = Key(fp, doc, where)
        authorised = doc.get("authorised", [])
        if not isinstance(authorised, list):
            self.r.fail(2, "key-authorised", where, "authorised is not a list")
            authorised = []
        for n, entry in enumerate(authorised):
            try:
                if not isinstance(entry, dict):
                    raise sshsig.SigError("is not an object")
                mkind, _ = sshsig.parse_public_key(entry.get("key"))
                efp = sshsig.key_fingerprint(entry.get("key"))
                if entry.get("fingerprint") != efp:
                    raise sshsig.SigError(f"names {entry.get('fingerprint')!r} but its key is {efp}")
                if mkind != ED25519:
                    raise sshsig.SigError(f"is a {mkind} key; Ed25519 only")
                key.authorised[efp] = (entry["key"], entry.get("revoked") is True)
            except sshsig.SigError as e:
                self.r.fail(2, "key-authorised", where, f"authorised[{n}] {e}")
        self.keys[fp] = key

    def _check(self, key, signer):
        doc = dict(key.doc)
        signature = doc.pop("signature", None)
        if not isinstance(signature, str):
            return False, "has no signature"
        return sshsig.verify("jetcarp-key", jcs.canonical(doc), signature, signer.line, self.timeout)

    def _verify_statement(self, key):
        if key.supersedes is None:
            ok, msg = self._check(key, key)
            if ok:
                key.verified, key.signed_by = True, key.fp
            else:
                self.r.fail(2, "bad-signature", key.where, f"a first statement must be signed by its own key: {msg}")
            return
        old = self.keys.get(key.supersedes)
        if old is None:
            self.r.fail(2, "key-chain", key.where, f"supersedes {key.supersedes}, whose statement is not in keys/")
            return
        ok, msg = self._check(key, old)
        if ok:
            key.verified, key.signed_by = True, old.fp
            return
        github = key.doc.get("github")
        if github and self._check(key, key)[0]:
            self.r.skip(2, "key-unconfirmed", key.where,
                        f"supersedes {key.supersedes} but is signed only by itself; a lost key can be confirmed "
                        f"at https://github.com/{github}.keys, which this reader does not do offline")
        else:
            self.r.fail(2, "bad-signature", key.where, f"a successor must be signed by the key it supersedes: {msg}")

    def _verify_revocation(self, key):
        candidates = [key] + [k for k in self.keys.values() if k.supersedes == key.fp and not k.revoked]
        for signer in candidates:
            ok, _ = self._check(key, signer)
            if ok:
                key.verified, key.signed_by = True, signer.fp
                self.revoked.add(key.fp)
                self.r.info(2, "revoked", key.where, f"revokes {key.fp} (signed by {signer.fp})")
                return
        self.r.fail(2, "bad-signature", key.where, "a revocation must be signed by the revoked key or a successor")

    def _settle(self):
        """Which statements stand, following chains from their first keys."""
        def stands(key, seen):
            if key.fp in seen:
                self.r.fail(2, "key-chain", key.where, "its supersession chain loops")
                return False
            if key.revoked:
                return False
            if not key.verified:
                return False
            if key.supersedes is None:
                return True
            old = self.keys.get(key.supersedes)
            if old is None:
                return False
            if old.fp in self.revoked:
                if old.signed_by != key.fp:
                    self.r.fail(2, "revoked-key", key.where, f"supersedes {old.fp}, which is revoked, and did not sign "
                                                             "that revocation itself: it does not stand")
                    return False
                return True
            return stands(old, seen | {key.fp})

        for key in self.keys.values():
            key.stands = stands(key, frozenset()) and key.fp not in self.revoked
        successors = {}
        for key in self.keys.values():
            if key.stands and key.supersedes:
                successors.setdefault(key.supersedes, []).append(key)
        self.successors = {}
        for old, new in successors.items():
            if len(new) > 1:
                self.r.fail(2, "key-fork", f"keys/{signer_name(old)}.json",
                            f"is superseded by {len(new)} keys ({', '.join(k.fp for k in new)}); refusing them")
                for k in new:
                    k.stands = False
            else:
                self.successors[old] = new[0]

    # Lookups ------------------------------------------------------------------------------

    def get(self, fp):
        k = self.keys.get(fp)
        return k if k is not None and k.stands else None

    def root(self, fp):
        """The first key of fp's chain: the publisher's id."""
        k, seen = self.keys.get(fp), set()
        while k is not None and k.supersedes and k.fp not in seen and k.verified:
            seen.add(k.fp)
            nxt = self.keys.get(k.supersedes)
            if nxt is None:
                break
            k = nxt
        return k.fp if k is not None else fp

    def current(self, fp):
        """The newest standing key that fp's chain leads to."""
        k, seen = self.get(fp), set()
        while k is not None and k.fp in self.successors and k.fp not in seen:
            seen.add(k.fp)
            k = self.successors[k.fp]
        return k

    def signing_keys(self, fp):
        """Keys whose signatures count for publisher key fp: its own and its machine keys, none revoked."""
        k = self.get(fp)
        if k is None:
            return {}
        out = {k.fp: k.line}
        for mfp, (line, revoked) in k.authorised.items():
            if not revoked and mfp not in self.revoked:
                out[mfp] = line
        return out


def check_authenticity(store, report, manifest=None, state_path=None, update_state=False):
    if not check_store_marker(store, report):
        return
    if not sshsig.available():
        report.skip(2, "no-ssh-keygen", store.root, "ssh-keygen is not installed, so no signature can be checked")
        return
    state = load_state(state_path, report) if state_path else {"feeds": {}, "revoked": []}
    ring = KeyRing(store, report, state["revoked"])
    accepted = {}
    refs = list_refs(store, report)
    for feed, (raw, ref) in sorted(refs.items()):
        result = check_ref(store, ring, report, feed, raw, ref, state["feeds"] if state_path else None)
        if result:
            accepted[result[0]] = result[1]
    if state_path is None and refs:
        report.info(2, "no-state", "refs/", "no state file given, so seq was checked only for being a positive "
                                            "integer, not against the last accepted (use --state)")
    for h in manifests_to_check(store, report, manifest):
        check_manifest_signatures(store, ring, report, h)
    if state_path and update_state:
        save_state(state_path, state, accepted, ring.revoked)


def check_manifest_signatures(store, ring, report, h):
    where = f"manifest {sh(h)}"
    try:
        data, doc = store.read_json_object(h, "manifest")
    except StoreError as e:
        report.fail(2, e.code, where, e.message)
        return
    publisher = doc.get("publisher") if isinstance(doc, dict) else None
    if not check_fingerprint(publisher):
        report.fail(2, "publisher", where, f"publisher {publisher!r} is not a fingerprint")
        return
    try:
        files = store.signature_files(h)
    except StoreError as e:
        report.fail(2, e.code, where, e.message)
        return
    if not files:
        report.fail(2, "unsigned", where, "has no signatures")
        return
    if publisher in ring.revoked:
        report.fail(2, "revoked-key", where, f"its publisher key {publisher} is revoked, and signs nothing")
        return
    if ring.get(publisher) is None:
        report.fail(2, "no-key", where, f"its publisher {publisher} has no standing statement in keys/")
    allowed = ring.signing_keys(publisher)
    by_publisher = []
    for name, comps in files:
        sw = "/".join(comps)
        if not name.endswith(".sig") or not SIGNER_RE.match(name[:-4]):
            report.warn(2, "sig-name", sw, "is not <64 hex>.sig")
            continue
        try:
            text = store.read(comps, store.limits.document_bytes).decode("ascii")
            info = sshsig.parse_signature(text)
        except StoreError as e:
            report.fail(2, e.code, sw, e.message)
            continue
        except (UnicodeDecodeError, sshsig.SigError) as e:
            report.fail(2, "bad-signature", sw, f"is not an SSH signature: {e}")
            continue
        if info["key_type"] != ED25519:
            report.warn(2, "not-ed25519", sw, f"is by a {info['key_type']} key, which counts as no signature")
            continue
        if hashlib.sha256(info["key_blob"]).hexdigest() != name[:-4]:
            report.fail(2, "sig-name", sw, f"is named for another key: the signature is by {info['fingerprint']}")
            continue
        line = f"{ED25519} {base64.b64encode(info['key_blob']).decode()}"
        ok, msg = sshsig.verify("jetcarp-manifest", data, text, line, store.limits.subprocess_seconds)
        if not ok:
            report.fail(2, "bad-signature", sw, msg)
            continue
        signer = info["fingerprint"]
        if signer in ring.revoked:
            report.warn(2, "revoked-key", sw, f"is by {signer}, which is revoked: it counts as no signature")
        elif signer in allowed:
            by_publisher.append(signer)
            report.ok(2, "manifest signatures")
        else:
            report.info(2, "endorsement", sw, f"endorsed by {signer}")
    # A machine key's signature is at that key's name: a reader over HTTP looks under the publisher's
    # key and each key its statement authorises, so a version signed only by a machine key is found.
    if not by_publisher:
        report.fail(2, "no-publisher-signature", where,
                    f"no valid signature by its publisher {publisher} or a machine key it authorises")


def check_ref(store, ring, report, feed, raw, ref, feeds):
    where = f"refs/{feed}.json"
    seq = ref.get("seq")
    if not isinstance(seq, int) or isinstance(seq, bool) or seq < 1:
        report.fail(2, "ref-seq", where, f"seq {seq!r} is not a positive integer")
        return None
    if not ident.is_time(ref.get("updated")):
        report.fail(2, "ref-field", where, f"updated {ref.get('updated')!r} is not an RFC 3339 UTC time")
    publisher = ref.get("publisher")
    if not check_fingerprint(publisher):
        report.fail(2, "publisher", where, f"publisher {publisher!r} is not a fingerprint")
        return None
    if not safejson.is_canonical(raw, ref):
        report.fail(2, "not-canonical", where, "is not in RFC 8785 canonical form; refused")
        return None
    body = dict(ref)
    signature = body.pop("signature", None)
    if not isinstance(signature, str):
        report.fail(2, "unsigned", where, "has no signature")
        return None
    if publisher in ring.revoked:
        report.fail(2, "revoked-key", where, f"names publisher key {publisher}, which is revoked")
        return None
    current = ring.current(publisher)
    if current is None:
        report.fail(2, "no-key", where, f"its publisher {publisher} has no standing statement in keys/")
        return None
    if current.fp != publisher:
        report.fail(2, "superseded-key", where, f"names publisher {publisher}, whose current key is {current.fp}")
        return None
    try:
        info = sshsig.parse_signature(signature)
    except sshsig.SigError as e:
        report.fail(2, "bad-signature", where, f"signature {e}")
        return None
    allowed = ring.signing_keys(publisher)
    if info["fingerprint"] not in allowed:
        why = "revoked" if info["fingerprint"] in ring.revoked else f"not {publisher} or a machine key it authorises"
        report.fail(2, "bad-signature", where, f"is signed by {info['fingerprint']}, which is {why}")
        return None
    ok, msg = sshsig.verify("jetcarp-ref", jcs.canonical(body), signature, allowed[info["fingerprint"]],
                            store.limits.subprocess_seconds)
    if not ok:
        report.fail(2, "bad-signature", where, msg)
        return None
    report.ok(2, "ref signatures")
    try:
        _, m = store.read_json_object(ref["manifest"], "manifest")
    except StoreError as e:
        report.fail(2, e.code, where, f"its manifest: {e.message}")
        return None
    if not isinstance(m, dict) or m.get("feed") != feed:
        report.fail(2, "ref-manifest", where, f"points to a manifest of feed {m.get('feed') if isinstance(m, dict) else None!r}")
        return None
    if ring.root(m.get("publisher", "")) != ring.root(publisher):
        report.fail(2, "ref-manifest", where, f"points to a manifest by {m.get('publisher')}, not {publisher}")
        return None
    feed_id = f"{ring.root(publisher)} {feed}"
    if feeds is not None:
        last = feeds.get(feed_id)
        if last is not None:
            if seq < last["seq"]:
                report.fail(2, "seq-rollback", where, f"seq {seq} is below {last['seq']}, the last accepted; "
                                                      "refusing a rollback")
                return None
            if seq == last["seq"]:
                if ref["manifest"] != last["manifest"]:
                    report.fail(2, "seq-reused", where, f"seq {seq} was accepted before for another manifest "
                                                        f"({sh(last['manifest'])})")
                else:
                    report.info(2, "seq-unchanged", where, f"seq {seq} is the one accepted last: no update")
                return None
    return feed_id, {"seq": seq, "manifest": ref["manifest"]}


def load_state(path, report):
    empty = {"feeds": {}, "revoked": []}
    if not os.path.exists(path):
        return empty
    try:
        with open(path, "rb") as f:
            doc = safejson.loads(f.read(), max_bytes=16 << 20, what="state file")
        if not isinstance(doc, dict) or doc.get("format") != STATE_FORMAT:
            raise ValueError("not a reader state file")
        feeds = {k: v for k, v in doc.get("feeds", {}).items() if isinstance(v, dict) and isinstance(v.get("seq"), int)}
        revoked = [fp for fp in doc.get("revoked", []) if check_fingerprint(fp)]
        return {"feeds": feeds, "revoked": revoked}
    except (OSError, ValueError) as e:
        report.fail(2, "state", path, f"cannot read the state file: {e}")
        return empty


def save_state(path, state, accepted, revoked):
    feeds = dict(state["feeds"])
    feeds.update(accepted)
    doc = {"format": STATE_FORMAT, "version": "1.0", "feeds": feeds,
           "revoked": sorted(set(state["revoked"]) | set(revoked))}
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(json.dumps(doc, indent=1, sort_keys=True))
    os.replace(tmp, path)
