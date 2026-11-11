"""Conformance level 0, browse (pack-format.md, "Conformance levels"): list a pack's tables, show rows,
and find a record by id. Every object is still checked against its hash before it is shown."""

import json

from . import identity as ident
from .pack import check_store_marker, list_refs, sh


def info(store, report):
    out = {"store": store.root, "export": store.is_export, "refs": [], "keys": []}
    if not check_store_marker(store, report):
        return out
    if store.is_export:
        out["manifest"] = store.manifest_hash
    for feed, (_, ref) in sorted(list_refs(store, report).items()):
        entry = {"feed": feed, "seq": ref.get("seq"), "publisher": ref.get("publisher"),
                 "manifest": ref.get("manifest"), "updated": ref.get("updated")}
        try:
            _, m = store.read_json_object(ref["manifest"], "manifest")
            entry.update({"title": m.get("title"), "version": m.get("version"),
                          "version_label": m.get("version_label"), "parents": len(m.get("parents", [])),
                          "tables": sorted(m.get("tables", {})), "retracts": len(m.get("retracts", []))})
        except Exception as e:  # shown, not fatal: browse lists what it can
            entry["error"] = str(e)
        out["refs"].append(entry)
    for name in store.list(("keys",)):
        out["keys"].append(name)
    return out


def tables(pack):
    out = []
    for name, t in sorted(pack.tables.items()):
        out.append({"table": name, "schema": t.schema, "index": t.index_hash, "indexes": len(t.indexes),
                    "parts": len(t.parts), "rows": t.row_count(),
                    "partitions": sorted({json.dumps(p.entry.get("partition", {}), sort_keys=True) for p in t.parts})})
    return out


def rows(pack, table, where=(), columns=None, limit=20):
    t = pack.table(table)
    if t is None:
        return None
    filters = []
    for w in where:
        name, _, value = w.partition("=")
        filters.append((name, value))
    out = []
    for _, _, row in t.rows():
        if all(str(row.get(n)) == v for n, v in filters):
            out.append({c: row.get(c) for c in columns} if columns else row)
            if len(out) >= limit:
                break
    return out


def retractions(pack):
    out = {}
    for x in pack.manifest.get("retracts", []) or []:
        if isinstance(x, dict) and isinstance(x.get("target"), str):
            out[x["target"]] = {"by": x.get("publisher"), "reason": x.get("reason"), "date": x.get("date")}
    return out


def find(pack, query):
    """A pattern or ant by id, unique prefix of 4 or more digits, alias or name."""
    query = query.strip()
    q = query[4:] if query.startswith("ant-") else query
    matches = []
    for table, prefix in (("patterns", ""), ("ants", "ant-")):
        for _, _, row in pack.rows(table):
            rid = row["id"]
            bare = rid[len(prefix):] if rid.startswith(prefix) else rid
            if rid == query or (len(q) >= 4 and bare.startswith(q.lower())):
                matches.append((table, row))
    if not matches:
        for _, _, a in pack.rows("aliases"):
            if a["old_id"] == query:
                return find(pack, a["target"].partition(":")[2])
        for _, _, a in pack.rows("annotations"):
            if a["field"] == "name":
                try:
                    name = ident.json_cell(a["value"], "name")
                except ValueError:
                    continue
                if isinstance(name, str) and name.lower() == query.lower():
                    return find(pack, a["target"].partition(":")[2])
    ids = {(t, r["id"]) for t, r in matches}
    if len(ids) != 1:
        return {"query": query, "matches": sorted(r["id"] for _, r in matches)}
    table, row = matches[0]
    kind = "pattern" if table == "patterns" else "ant"
    target = f"{kind}:{row['id']}"
    shown = dict(row)
    if shown.get("phases") is not None:  # the identity's compact form, for reading
        shown["phases"] = ident.format_phases(shown["phases"])
    out = {"table": table, "record": shown, "events": {}}
    for t, col in (("verifications", "pattern_id"), ("sightings", "pattern_id"), ("provenance", "target"),
                   ("annotations", "target"), ("media", "target"), ("ratings", "target")):
        key = row["id"] if col == "pattern_id" else target
        found = [r for _, _, r in pack.rows(t) if r.get(col) == key]
        if found:
            out["events"][t] = found
    latest = {}
    for a in out["events"].get("annotations", []):
        # Latest wins: the highest rev, then the later date, then the greater value by its bytes.
        k = (a["publisher"], a["field"])
        rank = (a["rev"], a["date"], a["value"].encode())
        if k not in latest or rank > (latest[k]["rev"], latest[k]["date"], latest[k]["value"].encode()):
            latest[k] = a
    if latest:
        out["annotations_latest"] = [{"publisher": p, "field": f, "value": a["value"]} for (p, f), a in sorted(latest.items())]
    r = retractions(pack).get(target)
    if r:
        out["retracted"] = r
    return out


def describe_manifest(pack):
    m = pack.manifest
    return {"manifest": pack.hash, "feed": m.get("feed"), "title": m.get("title"), "version": m.get("version"),
            "version_label": m.get("version_label"), "created": m.get("created"), "publisher": m.get("publisher"),
            "licence": m.get("licence"), "parents": [sh(p) for p in m.get("parents", [])],
            "retracts": m.get("retracts", [])}
