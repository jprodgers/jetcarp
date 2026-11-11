"""Conformance level 2, authenticity (pack spec sections 6, 7, 14, 16; section 24 fix 1): signatures
on manifests, refs and key statements, the chain of key statements (further machine keys,
supersession, revocation), and refs' sequence numbers.

The spec leaves several things open here; the choices are marked "(choice)" and listed in
SPEC_QUESTIONS.md. The main ones:
- A first key statement is signed by its own key. A superseding statement is signed by the key it
  supersedes. A revocation is signed by the revoked key or by a key that supersedes it.
- Further machine keys are listed in the publisher's key statement as `authorised`:
  [{"key": "ssh-ed25519 ...", "fingerprint": "SHA256:...", "label": "..."}].
- A publisher is the first key of a supersession chain; feeds are keyed by it in the state file.
- The revocation's date is its statement's `created`; a manifest whose `created`, or a ref whose
  `updated`, is not before it is refused.
"""

import json
import os
import re

from . import identity as ident
from . import jcs, safejson, sshsig
from .integrity import manifests_to_check
from .pack import check_store_marker, list_refs, sh, check_fingerprint
from .store import StoreError, check_hash, fingerprint_of_signer, signer_name

STATE_FORMAT = "jetcarp-pack-reader-state"


class Key:
    def __init__(self, fp, doc, raw, where):
        self.fp, self.doc, self.raw, self.where = fp, doc, raw, where
        self.line = doc.get("key")
        self.supersedes = doc.get("supersedes") or ""
        self.revoked = doc.get("revoked") is True
        self.created = doc.get("created")
        self.authorised = {}  # fingerprint -> key line, machine keys (section 24 fix 1; choice of field)
        self.valid = False  # its statement's signature checked out
        self.chain_ok = False  # its supersession is confirmed by the old key
        self.signed_by = None


class KeyRing:
    """Every key statement in keys/, checked."""

    def __init__(self, store, report):
        self.store, self.r = store, report
        self.keys = {}
        self.successors = {}
        timeout = store.limits.subprocess_seconds
        try:
            names = store.list(("keys",))
        except StoreError as e:
            self.r.fail(2, e.code, "keys/", e.message)
            names = []
        for name in names:
            where = f"keys/{name}"
            self._load(name, where)
        # A key's public line is authentic by its fingerprint (the file name), so each statement can
        # be checked on its own: a first statement against its own key, a successor against the key
        # it supersedes. Revocations come second, since a successor may sign one.
        for key in self.keys.values():
            if not key.revoked:
                self._verify(key, timeout)
        for key in self.keys.values():
            if key.revoked:
                self._verify(key, timeout)
        for key in self.keys.values():
            if key.valid and key.chain_ok and key.supersedes:
                self.successors.setdefault(key.supersedes, []).append(key)
        for old, new in self.successors.items():
            if len(new) > 1:
                self.r.fail(2, "key-fork", f"keys/{signer_name(old)}.json",
                            f"is superseded by {len(new)} keys ({', '.join(k.fp for k in new)}); refusing them")
                for k in new:
                    k.chain_ok = False
                self.successors[old] = []
        n = sum(1 for k in self.keys.values() if k.valid)
        if n:
            self.r.ok(2, "key statements", n)

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
        if doc.get("fingerprint") != fp:
            self.r.fail(2, "key-fingerprint", where, f"names fingerprint {doc.get('fingerprint')!r}, not its file's {fp}")
            return
        try:
            actual = sshsig.key_fingerprint(doc.get("key"))
        except sshsig.SigError as e:
            self.r.fail(2, "key-format", where, str(e))
            return
        if actual != fp:
            self.r.fail(2, "key-fingerprint", where, f"its key's fingerprint is {actual}, not {fp}")
            return
        if not ident.is_time(doc.get("created")):
            self.r.fail(2, "key-format", where, f"created {doc.get('created')!r} is not an RFC 3339 UTC time")
        sup = doc.get("supersedes", "")
        if sup and not check_fingerprint(sup):
            self.r.fail(2, "key-format", where, f"supersedes {sup!r} is not a fingerprint")
            return
        if not isinstance(doc.get("revoked", False), bool):
            self.r.fail(2, "key-format", where, "revoked is not true or false")
            return
        if doc.get("orcid") and not ident.is_orcid(doc["orcid"]):
            self.r.warn(2, "orcid", where, f"ORCID {doc['orcid']!r} has a wrong check digit")
        if not safejson.is_canonical(raw, doc):
            self.r.warn(2, "not-canonical", where, "is not in RFC 8785 canonical form")
        key = Key(fp, doc, raw, where)
        authorised = doc.get("authorised", [])
        if not isinstance(authorised, list):
            self.r.fail(2, "key-authorised", where, "authorised is not a list")
            authorised = []
        for n, entry in enumerate(authorised):
            try:
                if not isinstance(entry, dict):
                    raise sshsig.SigError("is not an object")
                efp = sshsig.key_fingerprint(entry.get("key"))
                if entry.get("fingerprint") != efp:
                    raise sshsig.SigError(f"names {entry.get('fingerprint')!r} but its key is {efp}")
                key.authorised[efp] = entry["key"]
            except sshsig.SigError as e:
                self.r.fail(2, "key-authorised", where, f"authorised[{n}] {e}")
        self.keys[fp] = key

    def _verify(self, key, timeout):
        doc = dict(key.doc)
        signature = doc.pop("signature", None)
        if not isinstance(signature, str):
            self.r.fail(2, "unsigned", key.where, "has no signature")
            return
        try:
            body = jcs.canonical(doc)
        except jcs.CanonicalError as e:
            self.r.fail(2, "key-format", key.where, str(e))
            return
        candidates = []
        if key.revoked:
            # A revocation: by the key itself, or by a confirmed successor (choice).
            candidates.append((key, False))
            candidates += [(k, False) for k in self.keys.values()
                           if k.supersedes == key.fp and k.valid and k.chain_ok]
        else:
            if key.supersedes:
                old = self.keys.get(key.supersedes)
                if old is not None:
                    candidates.append((old, True))
                else:
                    self.r.warn(2, "key-chain", key.where, f"supersedes {key.supersedes}, whose statement is not in keys/")
            candidates.append((key, False))
        messages = []
        for signer, confirms_chain in candidates:
            ok, msg = sshsig.verify("jetcarp-key", body, signature, signer.line, timeout)
            if ok:
                key.valid, key.signed_by = True, signer.fp
                key.chain_ok = confirms_chain
                break
            messages.append(f"{signer.fp}: {msg}")
        if not key.valid:
            self.r.fail(2, "bad-signature", key.where, "signature does not verify: " + "; ".join(messages))
            return
        if key.supersedes and not key.chain_ok:
            github = key.doc.get("github")
            hint = f" (confirm at https://github.com/{github}.keys)" if github else ""
            self.r.skip(2, "key-unconfirmed", key.where,
                        f"supersedes {key.supersedes} but is signed only by itself; a lost key can be confirmed "
                        f"through GitHub, which this reader does not do offline{hint}")

    # Lookups ------------------------------------------------------------------------------

    def get(self, fp):
        k = self.keys.get(fp)
        return k if k is not None and k.valid else None

    def root(self, fp):
        """The first key of fp's supersession chain: the publisher's identity (choice)."""
        seen = set()
        k = self.get(fp)
        while k is not None and k.chain_ok and k.supersedes and k.fp not in seen:
            seen.add(k.fp)
            nxt = self.get(k.supersedes)
            if nxt is None:
                break
            k = nxt
        return k.fp if k is not None else fp

    def current(self, fp):
        """The newest key that fp's chain leads to (section 6: the publisher's current key)."""
        seen = set()
        k = self.get(fp)
        while k is not None and self.successors.get(k.fp) and k.fp not in seen:
            seen.add(k.fp)
            k = self.successors[k.fp][0]
        return k

    def signing_keys(self, fp):
        """Keys that sign for publisher fp: its own and its authorised machine keys."""
        k = self.get(fp)
        if k is None:
            return {}
        out = {k.fp: k.line}
        out.update(k.authorised)
        return out

    def revocation(self, fp):
        """The revocation date of a key, if its statement revokes it."""
        k = self.keys.get(fp)
        if k is not None and k.valid and k.revoked:
            return k.created
        return None


def check_authenticity(store, report, manifest=None, state_path=None, update_state=False):
    if not check_store_marker(store, report):
        return
    if not sshsig.available():
        report.skip(2, "no-ssh-keygen", store.root, "ssh-keygen is not installed, so no signature can be checked")
        return
    ring = KeyRing(store, report)
    state = load_state(state_path, report) if state_path else None
    accepted = {}
    refs = list_refs(store, report)
    for feed, (raw, ref) in sorted(refs.items()):
        result = check_ref(store, ring, report, feed, raw, ref, state)
        if result:
            accepted[result[0]] = result[1]
    if state_path is None and refs:
        report.info(2, "no-state", "refs/", "no state file given, so seq was checked only for being a positive "
                                            "integer, not against the last accepted (use --state)")
    for h in manifests_to_check(store, report, manifest):
        check_manifest_signatures(store, ring, report, h)
    check_receipts(store, ring, report)
    if state_path and update_state and accepted:
        save_state(state_path, state or {}, accepted)


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
    hexd = check_hash(h)
    folder = ("sigs", "sha256", hexd[:2], hexd)
    try:
        names = store.list(folder)
    except StoreError as e:
        report.fail(2, e.code, where, e.message)
        return
    if not names:
        report.fail(2, "unsigned", where, f"has no signatures in {'/'.join(folder)}/")
        return
    if ring.get(publisher) is None:
        report.fail(2, "no-key", where, f"its publisher {publisher} has no valid statement in keys/")
    allowed = ring.signing_keys(publisher)
    publisher_ok = False
    timeout = store.limits.subprocess_seconds
    for name in names:
        sw = f"{'/'.join(folder)}/{name}"
        if not name.endswith(".sig"):
            report.warn(2, "sig-name", sw, "is not <signer>.sig")
            continue
        try:
            signer = fingerprint_of_signer(name[:-4])
            text = store.read(folder + (name,), store.limits.document_bytes).decode("ascii")
        except StoreError as e:
            report.fail(2, e.code, sw, e.message)
            continue
        except UnicodeDecodeError:
            report.fail(2, "bad-signature", sw, "is not ASCII armour")
            continue
        line = allowed.get(signer)
        endorsement = line is None
        if endorsement:
            k = ring.get(signer)
            if k is None:
                report.warn(2, "unknown-signer", sw, f"is by {signer}, which has no valid statement in keys/")
                continue
            line = k.line
        ok, msg = sshsig.verify("jetcarp-manifest", data, text, line, timeout)
        if not ok:
            report.fail(2, "bad-signature", sw, msg)
            continue
        revoked = ring.revocation(signer)
        if revoked is not None:
            if not (isinstance(doc.get("created"), str) and doc["created"] < revoked):
                report.fail(2, "revoked-key", sw, f"is by {signer}, revoked on {revoked}, and the manifest was "
                                                  f"created {doc.get('created')}")
                continue
            report.warn(2, "revoked-key", sw, f"is by {signer}, revoked later ({revoked})")
        if endorsement:
            report.info(2, "endorsement", sw, f"endorsed by {signer}")
        else:
            publisher_ok = True
            report.ok(2, "manifest signatures")
    if not publisher_ok:
        report.fail(2, "no-publisher-signature", where,
                    f"no valid signature by its publisher {publisher} or the publisher's authorised keys")


def check_ref(store, ring, report, feed, raw, ref, state):
    where = f"refs/{feed}.json"
    seq = ref.get("seq")
    bad = False
    if not isinstance(seq, int) or isinstance(seq, bool) or seq < 1:
        report.fail(2, "ref-seq", where, f"seq {seq!r} is not a positive integer")
        bad = True
    if not ident.is_time(ref.get("updated")):
        report.fail(2, "ref-field", where, f"updated {ref.get('updated')!r} is not an RFC 3339 UTC time")
    publisher = ref.get("publisher")
    if not check_fingerprint(publisher):
        report.fail(2, "publisher", where, f"publisher {publisher!r} is not a fingerprint")
        return None
    if not safejson.is_canonical(raw, ref):
        report.warn(2, "not-canonical", where, "is not in RFC 8785 canonical form")
    body = dict(ref)
    signature = body.pop("signature", None)
    if not isinstance(signature, str):
        report.fail(2, "unsigned", where, "has no signature")
        return None
    current = ring.current(publisher)
    if current is None:
        report.fail(2, "no-key", where, f"its publisher {publisher} has no valid statement in keys/")
        return None
    if current.fp != publisher:
        report.fail(2, "superseded-key", where, f"names publisher {publisher}, whose current key is {current.fp}")
        return None
    try:
        signer = sshsig.parse_signature(signature)["fingerprint"]
    except sshsig.SigError as e:
        report.fail(2, "bad-signature", where, f"signature {e}")
        return None
    allowed = ring.signing_keys(publisher)
    if signer not in allowed:
        report.fail(2, "bad-signature", where, f"is signed by {signer}, which is not {publisher} or one of its "
                                               "authorised keys")
        return None
    ok, msg = sshsig.verify("jetcarp-ref", jcs.canonical(body), signature, allowed[signer],
                            store.limits.subprocess_seconds)
    if not ok:
        report.fail(2, "bad-signature", where, msg)
        return None
    revoked = ring.revocation(signer)
    if revoked is not None:
        report.fail(2, "revoked-key", where, f"is signed by {signer}, revoked on {revoked}")
        return None
    report.ok(2, "ref signatures")
    # The manifest it points to belongs to the same feed and publisher.
    try:
        _, m = store.read_json_object(ref["manifest"], "manifest")
    except StoreError as e:
        report.fail(2, e.code, where, f"its manifest: {e.message}")
        return None
    if not isinstance(m, dict) or m.get("feed") != feed:
        report.fail(2, "ref-manifest", where, f"points to a manifest of feed {m.get('feed') if isinstance(m, dict) else None!r}")
        bad = True
    elif ring.root(m.get("publisher", "")) != ring.root(publisher):
        report.fail(2, "ref-manifest", where, f"points to a manifest by {m.get('publisher')}, not {publisher}")
        bad = True
    if bad:
        return None
    # Rollback (section 6): seq must be greater than the last accepted for this feed.
    feed_id = f"{ring.root(publisher)} {feed}"
    if state is not None:
        last = state.get(feed_id)
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


def check_receipts(store, ring, report):
    """Receipts (section 14): each names its folder's manifest and is signed by its verifier."""
    for aa in store.list(("receipts", "sha256")):
        for mhex in store.list(("receipts", "sha256", aa)):
            for name in store.list(("receipts", "sha256", aa, mhex)):
                where = f"receipts/sha256/{aa}/{mhex}/{name}"
                if not re.fullmatch(r"[0-9a-f]{64}\.json", name):
                    report.warn(2, "receipt-name", where, "is not <64 hex>.json")
                    continue
                try:
                    _, doc = store.read_json_file(("receipts", "sha256", aa, mhex, name), where)
                except StoreError as e:
                    report.fail(2, e.code, where, e.message)
                    continue
                if not isinstance(doc, dict) or doc.get("format") != "jetcarp-receipt":
                    report.fail(2, "receipt-format", where, 'format is not "jetcarp-receipt"')
                    continue
                if doc.get("manifest") != f"sha256:{mhex}":
                    report.fail(2, "receipt-manifest", where, f"names {doc.get('manifest')!r}, not its folder's manifest")
                body = dict(doc)
                signature = body.pop("signature", "")
                k = ring.get(doc.get("verifier", ""))
                if k is None:
                    report.warn(2, "unknown-signer", where, f"verifier {doc.get('verifier')!r} has no statement in keys/")
                    continue
                ok, msg = sshsig.verify("jetcarp-receipt", jcs.canonical(body), signature, k.line,
                                        store.limits.subprocess_seconds)
                if ok:
                    report.ok(2, "receipt signatures")
                else:
                    report.fail(2, "bad-signature", where, msg)


def load_state(path, report):
    if not os.path.exists(path):
        return {}
    try:
        with open(path, "rb") as f:
            doc = safejson.loads(f.read(), max_bytes=16 << 20, what="state file")
        if doc.get("format") != STATE_FORMAT:
            raise ValueError("not a reader state file")
        return {k: v for k, v in doc.get("feeds", {}).items() if isinstance(v, dict) and isinstance(v.get("seq"), int)}
    except (OSError, ValueError) as e:
        report.fail(2, "state", path, f"cannot read the state file: {e}")
        return {}


def save_state(path, state, accepted):
    state = dict(state)
    state.update(accepted)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(json.dumps({"format": STATE_FORMAT, "version": "1.0", "feeds": state}, indent=1, sort_keys=True))
    os.replace(tmp, path)
