"""A tiny pack writer, for tests only: it makes stores to test the reader against, and the stores in
testdata/ that the Rust importer is checked against. It follows the pack spec as written, with
pyarrow and this package's RFC 8785.

pyarrow cannot set Parquet's `created_by`, which section 3 requires to be `jetcarp-pack 1`, so the
writer rewrites that one field in the Thrift footer (`set_created_by`).
"""

import hashlib
import os
import subprocess

import pyarrow as pa
import pyarrow.parquet as pq

from jetcarp_pack import jcs, schemas, sshsig
from jetcarp_pack.pack import _key_text, _sort_value
from jetcarp_pack.store import signer_name

RULE_FAMILIES = ["lifelike", "generations", "ltl", "elementary", "hexw", "hext", "layered", "jaca", "ant",
                 "turmite", "wireworld", "cyclic", "greenberg_hastings", "rps", "forest_fire", "hodgepodge",
                 "margolus", "lenia", "sandpile", "jvn29", "priority", "spectre_glider", "table"]
ENGINE = {
    "name": "JETCARP", "version": "0.4.0",
    "schemes": {"pattern": "jetcarp-pattern-v3", "ant": "jetcarp-ant-v1", "geometry": "jetcarp-geometry-v1",
                "tables": "jetcarp-tables-1"},
    "rule_semantics": {f: "1" for f in RULE_FAMILIES},
}


# Parquet footer --------------------------------------------------------------------------------

def _varint(buf, pos):
    shift = result = 0
    while True:
        b = buf[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            return result, pos
        shift += 7


def _encode_varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def _skip(buf, pos, ttype, in_container=False):
    if ttype in (1, 2):  # booleans: in the field header, or one byte in a container
        return pos + 1 if in_container else pos
    if ttype == 3:
        return pos + 1
    if ttype in (4, 5, 6):
        return _varint(buf, pos)[1]
    if ttype == 7:
        return pos + 8
    if ttype == 8:
        n, pos = _varint(buf, pos)
        return pos + n
    if ttype in (9, 10):
        h = buf[pos]
        pos += 1
        size, et = h >> 4, h & 0x0F
        if size == 15:
            size, pos = _varint(buf, pos)
        for _ in range(size):
            pos = _skip(buf, pos, et, True)
        return pos
    if ttype == 11:
        size, pos = _varint(buf, pos)
        if size:
            kv = buf[pos]
            pos += 1
            for _ in range(size):
                pos = _skip(buf, pos, kv >> 4, True)
                pos = _skip(buf, pos, kv & 0x0F, True)
        return pos
    if ttype == 12:
        while True:
            h = buf[pos]
            pos += 1
            if h == 0:
                return pos
            if h >> 4 == 0:
                pos = _varint(buf, pos)[1]
            pos = _skip(buf, pos, h & 0x0F)
    raise ValueError(f"unknown Thrift compact type {ttype}")


def set_created_by(data, value):
    """Rewrite FileMetaData.created_by (Thrift field 6) in a Parquet file's footer."""
    assert data[:4] == b"PAR1" and data[-4:] == b"PAR1"
    n = int.from_bytes(data[-8:-4], "little")
    footer = data[-8 - n:-8]
    pos, fid = 0, 0
    while True:
        h = footer[pos]
        pos += 1
        if h == 0:
            raise ValueError("the footer has no created_by")
        ttype = h & 0x0F
        if h >> 4:
            fid += h >> 4
        else:
            z, pos = _varint(footer, pos)
            fid = (z >> 1) ^ -(z & 1)
        if fid == 6 and ttype == 8:
            length, start = _varint(footer, pos)
            enc = value.encode("utf-8")
            footer = footer[:pos] + _encode_varint(len(enc)) + enc + footer[start + length:]
            break
        pos = _skip(footer, pos, ttype)
    return data[:-8 - n] + footer + len(footer).to_bytes(4, "little") + b"PAR1"


def parquet_bytes(table_name, rows, loose=False, **options):
    """A part's bytes. `loose` makes every column nullable, so tests can write nulls the spec forbids."""
    spec = schemas.TABLES[table_name]
    rows = sorted(rows, key=lambda r: tuple(_sort_value(r.get(c)) for c in spec.sort))
    schema = spec.arrow_schema()
    if loose:
        schema = pa.schema([f.with_nullable(True) for f in schema])
    table = pa.Table.from_pylist([{c.name: r.get(c.name) for c in spec.columns} for r in rows], schema=schema)
    sink = pa.BufferOutputStream()
    kw = dict(compression="zstd", write_statistics=True, write_page_index=True, store_schema=False)
    kw.update(options)
    pq.write_table(table, sink, **kw)
    return set_created_by(sink.getvalue().to_pybytes(), "jetcarp-pack 1"), rows


# Keys ----------------------------------------------------------------------------------------

class TestKey:
    """An Ed25519 key made with ssh-keygen in a folder the caller owns (never ~/.ssh)."""

    def __init__(self, folder, name):
        self.path = os.path.join(folder, name.replace(" ", "-"))
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", name, "-f", self.path], check=True)
        with open(self.path + ".pub") as f:
            self.line = " ".join(f.read().split()[:2])
        self.fingerprint = sshsig.key_fingerprint(self.line)
        self.name = name

    def sign(self, namespace, data):
        r = subprocess.run(["ssh-keygen", "-Y", "sign", "-f", self.path, "-n", namespace], input=data,
                           capture_output=True, check=True)
        return r.stdout.decode("ascii")


# Stores --------------------------------------------------------------------------------------

def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


class StoreWriter:
    def __init__(self, root, marker=True):
        self.root = root
        os.makedirs(root, exist_ok=True)
        if marker:
            self.write(("jetcarp-store.json",), jcs.canonical({"format": "jetcarp-store", "version": "1.0"}))

    def write(self, comps, data):
        path = os.path.join(self.root, *comps)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)
        return path

    def put(self, data, ext):
        h = sha(data)
        hexd = h[7:]
        path = os.path.join(self.root, "objects", "sha256", hexd[:2], f"{hexd}.{ext}")
        if not os.path.exists(path):
            self.write(("objects", "sha256", hexd[:2], f"{hexd}.{ext}"), data)
        return h

    def put_json(self, doc):
        data = jcs.canonical(doc)
        return self.put(data, "json"), data

    def part(self, table_name, rows, partition=None, **options):
        spec = schemas.TABLES[table_name]
        data, rows = parquet_bytes(table_name, rows, **options)
        h = self.put(data, "parquet")
        first = _key_text(rows[0].get(spec.sort[0])) if rows else ""
        last = _key_text(rows[-1].get(spec.sort[0])) if rows else ""
        if partition is None:
            partition = {k: rows[0][col] for k, col in spec.partition.items()} if rows else {}
        return {"object": h, "rows": len(rows), "bytes": len(data), "partition": partition,
                "first": first, "last": last}

    def entries(self, table_name, rows):
        """One part per partition (section 8.2), in partition order."""
        spec = schemas.TABLES[table_name]
        groups = {}
        for r in rows:
            groups.setdefault(tuple(r[col] for col in spec.partition.values()), []).append(r)
        return [self.part(table_name, groups[k]) for k in sorted(groups)]

    def index(self, table_name, entries, children=(), schema=None):
        doc = {"format": "jetcarp-index", "version": "1.0", "table": table_name,
               "schema": schema or schemas.TABLES[table_name].schema_id,
               "parts": list(entries), "children": list(children)}
        return self.put_json(doc)[0]

    def tables(self, content, previous=None):
        """Index hashes for {table: rows}. With `previous` ({table: entries}), parts already
        published are kept and only new rows go into new parts (append-only, section 8.1)."""
        out, entries_out = {}, {}
        previous = previous or {}
        for name in sorted(set(content) | set(previous)):
            rows = content.get(name)
            entries = list(previous.get(name, []))
            if rows:
                entries += self.entries(name, rows)
            if entries:
                out[name] = self.index(name, entries)
                entries_out[name] = entries
        return out, entries_out

    def manifest(self, publisher, feed, tables, created, **fields):
        doc = {
            "format": "jetcarp-pack", "version": "1.0", "requires": [], "feed": feed,
            "title": fields.pop("title", "Test pack"), "description": fields.pop("description", "A pack made for tests."),
            "version_label": fields.pop("version_label", "test"), "created": created, "licence": "CC0-1.0",
            "creators": fields.pop("creators", [{"name": "Test Publisher", "role": "creator"}]),
            "doi": "", "related": [], "parents": [], "publisher": publisher, "engine": ENGINE,
            "tables": tables, "searches": {}, "retracts": [], "retain": [], "stats": {},
        }
        doc.update(fields)
        return self.put_json(doc)

    def sign_manifest(self, h, data, key, payload=None):
        hexd = h[7:]
        sig = key.sign("jetcarp-manifest", data if payload is None else payload)
        self.write(("sigs", "sha256", hexd[:2], hexd, signer_name(key.fingerprint) + ".sig"), sig.encode("ascii"))

    def key_statement(self, key, created, *, signer=None, supersedes="", revoked=False, name=None,
                      authorised=(), **extra):
        doc = {"format": "jetcarp-key", "version": "1.0", "key": key.line, "fingerprint": key.fingerprint,
               "created": created, "supersedes": supersedes, "revoked": revoked}
        if name:
            doc["name"] = name
        if authorised:
            doc["authorised"] = [{"key": k.line, "fingerprint": k.fingerprint, "label": label} for k, label in authorised]
        doc.update(extra)
        doc["signature"] = (signer or key).sign("jetcarp-key", jcs.canonical(doc))
        self.write(("keys", signer_name(key.fingerprint) + ".json"), jcs.canonical(doc))
        return doc

    def ref_doc(self, feed, seq, manifest, publisher, updated, key):
        doc = {"format": "jetcarp-ref", "version": "1.0", "feed": feed, "seq": seq, "manifest": manifest,
               "updated": updated, "publisher": publisher}
        doc["signature"] = key.sign("jetcarp-ref", jcs.canonical(doc))
        return doc

    def ref(self, feed, seq, manifest, publisher, updated, key):
        doc = self.ref_doc(feed, seq, manifest, publisher, updated, key)
        self.write(("refs", f"{feed}.json"), jcs.canonical(doc))
        return doc
