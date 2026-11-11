"""Conformance level 1, integrity (pack-format.md, "Conformance levels"): every object hash, manifest
hash, content id and periodic pin.

Beyond the hashes, level 1 here checks that each table agrees with itself and with the others: typed
columns against the identity text they copy, records against the tilings they name, JSON cells in
their one spelling, the consent rules for ratings, and the manifest's members. Across versions it
checks that retractions are cumulative and that a version adds no event row its parents have; in an
export, that every file is where the layout puts it. Each kind of finding has its own code, so "the
bytes are intact" and "the content is consistent" stay apart.
"""

import hashlib
import unicodedata

from . import identity as ident
from . import jcs, pins, rules as rule_notation, schemas
from .pack import Pack, check_store_marker, list_refs, sh, check_fingerprint
from .store import StoreError, check_hash, FEED_RE

MANIFEST_FIELDS = {"format", "version", "requires", "feed", "title", "description", "version_label",
                   "created", "licence", "creators", "doi", "related", "parents", "publisher", "engine",
                   "tables", "searches", "retracts", "retain", "stats", "slice_of", "filter"}
STORE_PATHS = ("jetcarp-store.json", "refs", "objects", "sigs", "keys", "receipts", "views.sql", "torrents")
EXPORT_PATHS = ("pack.json", "index", "history", "tables", "tilings", "searches", "media", "sigs", "keys",
                "views.sql", "LICENSE", "README.md", "CITATION.cff")


def scan_objects(store, report):
    """Every object file is named <sha256>.<ext> (in an export, index/ and history/ by hash) and
    hashes to its name."""
    n = 0
    for comps, problem in store.object_files():
        where = "/".join(comps)
        if problem:
            report.fail(1, "object-name", where, problem)
            continue
        hexd, ext = comps[-1].split(".")
        try:
            f, _ = store.open_object("sha256:" + hexd, ext)
            f.close()
            n += 1
        except StoreError as e:
            report.fail(1, e.code, where, e.message)
    report.ok(1, "object files hashing to their names", n)
    known = EXPORT_PATHS if store.is_export else STORE_PATHS
    for name in store.list(()):
        if name not in known:
            report.info(1, "unknown-path", name, "ignored (a reader ignores top-level paths it does not know)")


def manifests_to_check(store, report, manifest=None):
    """The manifests named by refs (or the one given), with their parents and retained versions that
    the store holds. An export's earlier manifests (history/) come without their indexes, so only
    the export's own version is read in full."""
    if manifest:
        roots = [manifest]
    elif store.is_export:
        return [store.manifest_hash]
    else:
        roots = [doc["manifest"] for _, doc in list_refs(store, report).values()]
    order, seen = [], set()
    stack = list(reversed(roots))
    while stack and len(order) < 10_000:
        h = stack.pop()
        if h in seen:
            continue
        seen.add(h)
        order.append(h)
        if store.is_export:
            continue
        try:
            _, doc = store.read_json_object(h, "manifest")
        except StoreError:
            continue
        if not isinstance(doc, dict):
            continue
        for key in ("parents", "retain"):
            for p in doc.get(key, []) if isinstance(doc.get(key), list) else []:
                try:
                    check_hash(p)
                except StoreError:
                    continue
                if store.has_object(p, "json"):
                    stack.append(p)
    return order


def check_store(store, report, manifest=None, scan=True):
    if not check_store_marker(store, report):
        return []
    if scan:
        scan_objects(store, report)
    packs = {}
    cache = {}
    for h in manifests_to_check(store, report, manifest):
        pack = Pack(store, h, report, cache=cache)
        if pack.ok:
            check_pack(pack, report)
            packs[h] = pack
    for pack in packs.values():
        try:
            check_history(pack, packs, report)
        except Exception as e:  # never a crash
            report.fail(1, "check-error", pack.where, f"history check stopped on {type(e).__name__}: {e}")
    if store.is_export:
        for pack in packs.values():
            check_export_layout(pack, report)
    if not packs and not report.failed():
        report.fail(1, "no-manifest", store.root, "found no manifest to check")
    return list(packs.values())


def check_pack(pack, report):
    c = Checker(pack, report)
    for step in (c.manifest_fields, c.json_cells, c.tilings, c.rules, c.patterns, c.ants, c.quickstart,
                 c.events, c.media, c.ratings, c.optional_tables, c.retractions, c.searches, c.stats):
        try:
            step()
        except Exception as e:  # a hostile value this reader did not foresee: a failure, never a crash
            report.fail(1, "check-error", pack.where, f"{step.__name__} stopped on {type(e).__name__}: {str(e)[:200]}")


def _digest(row):
    return hashlib.sha256(repr(sorted(row.items())).encode()).digest()


def check_history(pack, packs, report):
    """Against each parent the store holds: retractions are cumulative, and no part the version adds
    repeats an event row a parent has (a writer adds only rows its parents lack)."""
    parents = [packs[p] for p in pack.manifest.get("parents", []) if p in packs]
    if not parents:
        return
    first = pack.manifest["parents"][0]
    if first in packs and packs[first].manifest.get("feed") != pack.manifest.get("feed"):
        report.warn(1, "parents-order", pack.where, "its first parent is of another feed (the feed's previous "
                                                    "version comes first)")
    mine = {(x.get("target"), x.get("publisher")) for x in pack.manifest.get("retracts", []) if isinstance(x, dict)}
    for parent in parents:
        for x in parent.manifest.get("retracts", []):
            if isinstance(x, dict) and (x.get("target"), x.get("publisher")) not in mine:
                report.fail(1, "retraction-dropped", pack.where, f"drops the retraction of {x.get('target')} by "
                                                                 f"{x.get('publisher')} that parent {sh(parent.hash)} lists")
    for name, table in pack.tables.items():
        if table.spec.kind != "event":
            continue
        parent_parts, parent_rows = set(), set()
        for parent in parents:
            t = parent.table(name)
            if t is None:
                continue
            for part in t.parts:
                parent_parts.add(part.hash)
                parent_rows.update(_digest(r) for r in part.rows)
        for part in table.parts:
            if part.hash in parent_parts:
                continue
            for n, row in enumerate(part.rows):
                if _digest(row) in parent_rows:
                    report.fail(1, "event-repeated", part.where(n), "repeats a row a parent already has, so it "
                                                                    "counts twice")


def check_export_layout(pack, report):
    """Every file of an export is where pack-format.md's layout puts it."""
    store = pack.store
    files = store.data_files()
    for comps, problem in getattr(store, "data_problems", []):
        report.fail(1, "export-file", "/".join(comps), problem)
    expected = {}
    for name, table in pack.tables.items():
        counters = {}
        for entry in table.entries:
            folder = ("tables", name) + tuple(f"{k}={entry['partition'][k]}" for k in table.spec.partition
                                              if k in entry.get("partition", {}))
            counters[folder] = counters.get(folder, 0) + 1
            expected[entry["object"]] = folder + (f"part-{counters[folder]:06d}.parquet",)
    for _, _, row in pack.rows("tilings"):
        if row.get("reference"):
            expected[row["reference"]] = ("tilings", "reference", f"{row['family']}.parquet")
    for _, _, row in pack.rows("media"):
        kind = ident.target_kind(row["target"])
        ext = schemas.MEDIA_TYPES.get(row["media_type"])
        if kind and ext:
            expected[row["object"]] = ("media", kind[0], kind[1],
                                       f"{row['kind']}{'-seed' if row['from_seed'] else ''}.{ext}")
    for uuid, doc in pack.search_indexes.items() if hasattr(pack, "search_indexes") else ():
        for entry in doc.get("parts", []):
            expected[entry["object"]] = ("searches", uuid, "runs", f"{entry.get('segment')}.parquet")
        for score in doc.get("scores", []):
            for entry in score.get("parts", []):
                expected[entry["object"]] = ("searches", uuid, f"scores-{score.get('version')}",
                                             f"{entry.get('segment')}.parquet")
    n = 0
    for h, comps in expected.items():
        actual = files.get(h)
        if actual is None:
            continue  # reported where the object is read
        if actual != comps:
            report.fail(1, "export-layout", "/".join(actual), f"holds {sh(h)}, which belongs at {'/'.join(comps)}")
        else:
            n += 1
    for h, comps in files.items():
        if h not in expected:
            report.warn(1, "export-extra", "/".join(comps), "is named by nothing in the version")
    report.ok(1, "export files in their places", n)


class Checker:
    def __init__(self, pack, report):
        self.pack, self.r, self.m = pack, report, pack.manifest
        self.w = pack.where
        engine = self.m.get("engine") if isinstance(self.m.get("engine"), dict) else {}
        schemes = engine.get("schemes") if isinstance(engine.get("schemes"), dict) else {}
        self.schemes = schemes
        rs = engine.get("rule_semantics")
        self.rule_semantics = rs if isinstance(rs, dict) else {}
        self.tilings_by_key = {}
        self.rules_by_key = {}
        self.patterns_by_id = {}
        self.ants_by_id = {}
        self.quickstart_by_id = {}
        self.raters = {}

    # Manifest -----------------------------------------------------------------------------

    def manifest_fields(self):
        m, w, r = self.m, self.w, self.r

        def bad(field, why):
            r.fail(1, "manifest-field", w, f"{field} {why}")

        if not isinstance(m.get("feed"), str) or not FEED_RE.match(m["feed"]):
            bad("feed", "is not 1 to 64 letters, digits and -")
        for name in ("title", "description", "version_label", "doi"):
            if not isinstance(m.get(name), str):
                bad(name, "is not a string")
        if not ident.is_time(m.get("created")):
            bad("created", "is not an RFC 3339 UTC time with whole seconds and Z")
        if not isinstance(m.get("licence"), str) or not ident.SPDX.match(m["licence"]):
            bad("licence", "is not an SPDX identifier")
        creators = m.get("creators", [])
        if not isinstance(creators, list) or not all(isinstance(c, dict) and isinstance(c.get("name"), str)
                                                      and isinstance(c.get("role"), str) for c in creators):
            bad("creators", "is not a list of objects with a name and a role")
        else:
            for c in creators:
                if "orcid" in c and not ident.is_orcid(c["orcid"]):
                    r.warn(1, "orcid", w, f"creator {c['name']!r} has ORCID {c['orcid']!r}, whose check digit is wrong")
        related = m.get("related", [])
        if not isinstance(related, list) or not all(isinstance(x, dict) and isinstance(x.get("relation"), str)
                                                     and isinstance(x.get("id"), str) for x in related):
            bad("related", 'is not a list of {"relation", "id"}')
        for name in ("parents", "retain"):
            v = m.get(name, [])
            if not isinstance(v, list):
                bad(name, "is not a list")
                continue
            for h in v:
                try:
                    check_hash(h)
                except StoreError as e:
                    bad(name, e.message)
            if len(set(map(str, v))) != len(v):
                bad(name, "lists a manifest twice")
            if self.pack.hash in v:
                bad(name, "lists the manifest itself")
        if not check_fingerprint(m.get("publisher")):
            bad("publisher", "is not a SHA256 key fingerprint")
        engine = m.get("engine")
        if not isinstance(engine, dict):
            bad("engine", "is not an object")
        else:
            for name in ("name", "version"):
                if not isinstance(engine.get(name), str):
                    bad(f"engine.{name}", "is not a string")
            if not isinstance(engine.get("schemes"), dict) or not all(isinstance(v, str) for v in engine["schemes"].values()):
                bad("engine.schemes", "is not an object of strings")
            rs = engine.get("rule_semantics")
            if not isinstance(rs, dict):
                bad("engine.rule_semantics", "is not an object")
            else:
                for fam, v in rs.items():
                    if not isinstance(v, str) or not ident.POSITIVE.match(v):
                        bad(f"engine.rule_semantics.{fam}", f"is {v!r}, not a positive integer written as a string")
        for name in ("tables", "searches"):
            v = m.get(name, {})
            if not isinstance(v, dict):
                bad(name, "is not an object")
                continue
            for k, h in v.items():
                if name == "searches" and not ident.UUID.match(k):
                    bad(name, f"key {k!r} is not a search uuid")
                try:
                    check_hash(h)
                except StoreError as e:
                    bad(f"{name}.{k}", e.message)
        retracts = m.get("retracts", [])
        if not isinstance(retracts, list) or not all(isinstance(x, dict) for x in retracts):
            bad("retracts", "is not a list of objects")
        stats = m.get("stats", {})
        if not isinstance(stats, dict) or not all(isinstance(v, int) for v in stats.values()):
            bad("stats", "is not an object of counts")
        if "slice_of" in m:
            try:
                check_hash(m["slice_of"])
            except StoreError as e:
                bad("slice_of", e.message)
        unknown = sorted(set(m) - MANIFEST_FIELDS)
        if unknown:
            r.info(1, "unknown-field", w, f"ignoring members {unknown}")
        for path, s in _strings(m):
            if not unicodedata.is_normalized("NFC", s):
                r.warn(1, "not-nfc", w, f"{path} is not Unicode NFC (writers SHOULD write NFC)")
            if ident.looks_like_local_path(s):
                r.fail(1, "local-path", w, f"{path} looks like a local path: {ident.short(s)}")
        for name, expected in (("pattern", ident.PATTERN_SCHEME), ("ant", ident.ANT_SCHEME),
                               ("geometry", ident.GEOMETRY_SCHEME)):
            got = self.schemes.get(name)
            if got != expected:
                r.skip(1, "unknown-scheme", w, f"engine.schemes.{name} is {got!r}; this reader checks "
                                               f"{expected} only, so those ids are not checked")
        for h in m.get("retain", []) if isinstance(m.get("retain"), list) else []:
            if not self.pack.store.has_object(h, "json"):
                r.fail(1, "retain-missing", w, f"retains {sh(h)}, which the store does not hold (hosts MUST keep it)")
        for h in m.get("parents", []) if isinstance(m.get("parents"), list) else []:
            if not self.pack.store.has_object(h, "json"):
                r.info(1, "parent-missing", w, f"parent {sh(h)} is not here (history the reader does not have)")

    def json_cells(self):
        """Every JSON cell is RFC 8785 text (non-integer numbers allowed), and no text cell holds a
        local path ("Local paths MUST NOT appear anywhere in a pack")."""
        n = 0
        for name, table in self.pack.tables.items():
            cols = [c.name for c in table.spec.columns if c.json]
            text_cols = [c.name for c in table.spec.columns if c.type == schemas.S]
            for part, i, row in table.rows():
                for col in text_cols:
                    if row.get(col) and ident.looks_like_local_path(row[col]):
                        self.r.fail(1, "local-path", part.where(i), f"{col} looks like a local path: "
                                                                    f"{ident.short(row[col])}")
                for col in cols:
                    text = row.get(col)
                    if text is None:
                        continue
                    try:
                        value = ident.json_cell(text, f"{name}.{col}")
                    except ValueError as e:
                        self.r.fail(1, "cell-json", part.where(i), str(e))
                        continue
                    try:
                        canonical = jcs.dumps(value, allow_floats=True)
                    except jcs.CanonicalError as e:
                        self.r.fail(1, "cell-json", part.where(i), f"{col}: {e}")
                        continue
                    if canonical != text:
                        self.r.fail(1, "cell-not-canonical", part.where(i),
                                    f"{col} is not RFC 8785 text: {ident.short(text, 80)} (canonical: {ident.short(canonical, 80)})")
                    else:
                        n += 1
        if n:
            self.r.ok(1, "JSON cells in their one spelling", n)

    # Facts --------------------------------------------------------------------------------

    def _facts(self, name, key_of):
        """Rows of a fact table by key; a key's rows are the same wherever they appear."""
        out = {}
        for part, n, row in self.pack.rows(name):
            k = key_of(row)
            if k in out:
                first, where = out[k]
                if _comparable(first) != _comparable(row):
                    self.r.fail(1, "fact-conflict", part.where(n),
                                f"{name} {ident.short(k)} differs from the row at {where}")
                continue
            out[k] = (row, part.where(n))
        return {k: v[0] for k, v in out.items()}

    def tilings(self):
        rows = self._facts("tilings", lambda r: r["pin_key"])
        self.tilings_by_key = rows
        pinned = 0
        for key, row in rows.items():
            w = f"tilings {ident.short(key)}"
            for col, pattern in (("pin", ident.HEX32), ("polygon_hash", _HEX64), ("neighbour_hash", _HEX64)):
                if not pattern.match(row[col] or ""):
                    self.r.fail(1, "pin-format", w, f"{col} {row[col]!r} is not lower-case hex of its length")
            parsed = None
            try:
                parsed = ident.check_tiling_key(key)
            except (ident.IdentityError, ValueError) as e:
                self.r.fail(1, "tiling-key", w, f"pin_key {e}")
            rebuilt = ('{"family":' + jcs.string(row["family"])
                       + (',"params":' + row["params"] if row["params"] is not None else "")
                       + (',"edges":' + row["edges"] if row["edges"] is not None else "") + ',"boundary":"dead"}')
            if rebuilt != key:
                self.r.fail(1, "tiling-key", w, f"family, params and edges give {ident.short(rebuilt, 120)}, not the pin key")
            if parsed is not None:
                group = ident.tiling_group(parsed)
                if group is not None and row["tiling_group"] != group:
                    self.r.fail(1, "tiling-group", w, f"tiling_group {row['tiling_group']!r}; the family's group is {group!r}")
            if row["kind"] == "lattice":
                if self._lattice(row, w):
                    pinned += 1
            elif row["kind"] == "aperiodic":
                if self._aperiodic(row, w):
                    pinned += 1
            else:
                self.r.fail(1, "tiling-kind", w, f"kind {row['kind']!r} is not lattice or aperiodic")
        if rows:
            self.r.ok(1, "tilings rows", len(rows))
        if pinned:
            self.r.ok(1, "pins recomputed from geometry", pinned)

    def _check_pin(self, row, w, hashes):
        polygon, neighbour, pin = hashes
        ok = True
        for col, value in (("polygon_hash", polygon), ("neighbour_hash", neighbour), ("pin", pin)):
            if row[col] != value:
                self.r.fail(1, "pin", w, f"{col} is {row[col]}, but the geometry gives {value}")
                ok = False
        return ok

    def _lattice(self, row, w):
        for name in ("a1", "a2", "tile_types", "edge", "touch"):
            if row[name] is None:
                self.r.fail(1, "tiling-geometry", w, f"a lattice tiling has no {name}")
                return False
        for name in ("reference_level", "reference", "reference_tiles"):
            if row[name] is not None:
                self.r.fail(1, "tiling-geometry", w, f"a lattice tiling has {name}")
        types = len(row["tile_types"])
        if len(row["a1"]) != 2 or len(row["a2"]) != 2 or not types:
            self.r.fail(1, "tiling-geometry", w, "a1 and a2 are not 2-vectors, or there are no tile types")
            return False
        for nb in ("edge", "touch"):
            lists = row[nb]
            if len(lists) != types:
                self.r.fail(1, "tiling-geometry", w, f"{nb} has {len(lists)} lists for {types} tile types")
                return False
            offsets = []
            for t, lst in enumerate(lists):
                items = [(o["di"], o["dj"], o["k"]) for o in lst]
                if any(not 0 <= k < types for _, _, k in items):
                    self.r.fail(1, "tiling-geometry", w, f"an offset in {nb} names a tile type out of range")
                    return False
                if len(set(items)) != len(items):
                    self.r.fail(1, "tiling-neighbours", w, f"tile type {t} lists a {nb} neighbour twice")
                offsets.append(set(items))
            for t, items in enumerate(offsets):
                for di, dj, k in items:
                    if (-di, -dj, t) not in offsets[k]:
                        self.r.fail(1, "tiling-neighbours", w, f"{nb}: type {t} has ({di}, {dj}, {k}) but type {k} "
                                                               "lacks the way back (the relation is symmetric)")
                        break
        try:
            return self._check_pin(row, w, pins.lattice_hashes(row))
        except (pins.PinError, KeyError, TypeError) as e:
            self.r.fail(1, "pin", w, f"cannot compute the pin: {e}")
            return False

    def _aperiodic(self, row, w):
        for name in ("a1", "a2", "tile_types", "edge", "touch"):
            if row[name] is not None:
                self.r.fail(1, "tiling-geometry", w, f"an aperiodic tiling has {name}")
        if row["reference_level"] is None or row["reference"] is None or row["reference_tiles"] is None:
            self.r.fail(1, "tiling-geometry", w, "an aperiodic tiling needs reference_level, reference and "
                                                 "reference_tiles")
            return False
        try:
            check_hash(row["reference"])
        except StoreError as e:
            self.r.fail(1, e.code, w, f"reference: {e.message}")
            return False
        tiles = self.pack.read_parquet(schemas.REFERENCE_PATCH, row["reference"],
                                       f"reference patch {sh(row['reference'])}", counted="reference patch hashes")
        if tiles is None:
            return False
        if len(tiles) != row["reference_tiles"]:
            self.r.fail(1, "reference-patch", w, f"its reference patch has {len(tiles)} tiles, not {row['reference_tiles']}")
        tiles = sorted(tiles, key=lambda t: t["tile"])
        try:
            return self._check_pin(row, w, pins.patch_hashes(tiles))
        except (pins.PinError, KeyError, TypeError) as e:
            self.r.fail(1, "pin", w, f"cannot compute the pin from the reference patch: {e}")
            return False

    def _record_tiling(self, row, w):
        t = self.tilings_by_key.get(row["pin_key"])
        if t is None:
            self.r.fail(1, "record-tiling", w, f"its tiling {ident.short(row['pin_key'])} has no row in tilings")
        elif t["pin"] != row["pin"]:
            self.r.fail(1, "record-pin", w, f"pin {row['pin']} differs from its tiling's {t['pin']}")
        elif row.get("tiling_group") is not None and row["tiling_group"] != t["tiling_group"]:
            self.r.fail(1, "tiling-group", w, f"tiling_group {row['tiling_group']!r}; its tiling's is {t['tiling_group']!r}")
        return t

    def rules(self):
        rows = self._facts("rules", lambda r: (r["notation"], r["neighbourhood"], r["steps_per_gen"]))
        self.rules_by_key = rows
        for (notation, nb, steps), row in rows.items():
            w = f"rules {notation!r} {nb} {steps}"
            if nb not in ("touch", "edge"):
                self.r.fail(1, "rule-field", w, f"neighbourhood {nb!r} is not touch or edge")
            if steps is None or steps < 1:
                self.r.fail(1, "rule-field", w, f"steps_per_gen {steps} is not positive")
            if not ident.POSITIVE.match(row["semantics"] or ""):
                self.r.fail(1, "rule-field", w, f"semantics {row['semantics']!r} is not a positive integer string")
            if row["states"] is None or row["states"] < 0:
                self.r.fail(1, "rule-field", w, f"states {row['states']} is negative")
            self._semantics(row["family"], row["semantics"], w)
            if row["family"] in schemas.RULE_FAMILIES_WITH_MASKS:
                parsed = rule_notation.parse_bs(notation)
                if parsed is not None and parsed[3] == "edge":
                    self.r.fail(1, "rule-notation", w, "the notation ends in V: in a pack the neighbourhood is "
                                                       "its own column, never a suffix")
                for name in ("birth", "survive"):
                    v = row[name]
                    if v is None:
                        self.r.fail(1, "rule-masks", w, f"a {row['family']} rule has no {name} mask")
                    elif not 0 <= v < 2 ** 63:
                        self.r.fail(1, "rule-masks", w, f"{name} mask {v} is negative")
                if row["family"] == "lifelike" and row["states"] != 2:
                    self.r.fail(1, "rule-field", w, f"a B/S rule has {row['states']} states, not 2")
                if row["family"] == "generations" and not 3 <= (row["states"] or 0) <= 255:
                    self.r.fail(1, "rule-field", w, f"a Generations rule has {row['states']} states (3 to 255)")
        if rows:
            self.r.ok(1, "rules rows", len(rows))

    def _semantics(self, family, version, w):
        if family is None or version is None:
            return
        engine = self.rule_semantics.get(family)
        if engine is None:
            self.r.info(1, "needs-reverify", w, f"family {family!r} is not in the manifest's rule_semantics")
        elif engine != version:
            self.r.info(1, "needs-reverify", w, f"family {family} at semantics {version}, the engine's is {engine}")

    def patterns(self):
        rows = self._facts("patterns", lambda r: r["id"])
        self.patterns_by_id = rows
        check_ids = self.schemes.get("pattern") == ident.PATTERN_SCHEME
        good = 0
        for pid, row in rows.items():
            w = f"pattern {pid}"
            if not isinstance(pid, str) or not ident.HEX32.match(pid):
                self.r.fail(1, "id-format", w, "id is not 32 lower-case hex digits")
            if check_ids:
                computed = ident.pattern_id(row["identity"], row["pin"])
                if computed != pid:
                    self.r.fail(1, "content-id", w, f"identity and pin hash to {computed}, not the id")
                else:
                    good += 1
            self._pattern_identity(row, w)
            self._record_tiling(row, w)
            if row["class"] not in schemas.CLASSES:
                self.r.fail(1, "pattern-class", w, f"class {row['class']!r} is not one of {', '.join(schemas.CLASSES)}")
            if row["class_group"] != row["class"]:
                self.r.fail(1, "class-group", w, f"class_group {row['class_group']!r} is not its class {row['class']!r}")
            self._pattern_numbers(row, w)
            if not ident.POSITIVE.match(row["semantics"] or ""):
                self.r.fail(1, "pattern-field", w, f"semantics {row['semantics']!r} is not a positive integer string")
            self._semantics(row["family"], row["semantics"], w)
            tiling = self.tilings_by_key.get(row["pin_key"])
            if tiling is not None:
                if tiling["kind"] == "aperiodic" and row["site"] is None:
                    self.r.fail(1, "pattern-site", w, "a pattern on an aperiodic tiling has no site")
                if tiling["kind"] == "lattice" and row["site"] is not None:
                    self.r.fail(1, "pattern-site", w, "a pattern on a lattice has a site")
            if row["class"] == "gun":
                if row["gun"] is None:
                    self.r.fail(1, "pattern-gun", w, "a gun has no gun column")
                else:
                    gun = ident.json_cell(row["gun"], "gun")
                    missing = [k for k in schemas.GUN_MEMBERS if not isinstance(gun, dict) or k not in gun]
                    if missing:
                        self.r.fail(1, "pattern-gun", w, f"gun lacks {', '.join(missing)}")
            elif row["gun"] is not None:
                self.r.warn(1, "pattern-gun", w, f"a {row['class']} has a gun column")
        if check_ids and rows:
            self.r.ok(1, "pattern content ids", good)

    def _pattern_identity(self, row, w):
        try:
            idt = ident.parse_pattern_identity(row["identity"])
        except ident.IdentityError as e:
            self.r.fail(1, "identity-syntax", w, f"identity {e}")
            return
        expect = {
            "tiling": row["pin_key"], "model": row["model"], "nb": row["neighbourhood"],
            "steps": str(row["steps_per_gen"]), "class": row["class"], "period": str(row["period"]),
            "d": f"{row['dx']},{row['dy']}", "rule": row["rule"] or "", "canonical": row["canonical"],
        }
        for name, value in expect.items():
            if idt[name] != value:
                self.r.fail(1, "identity-column", w, f"identity has {name}={ident.short(idt[name])}, "
                                                     f"the columns {ident.short(value)}")
        if ident.format_phases(row["phases"] or []) != idt["phases"]:
            self.r.fail(1, "identity-phases", w, "typed phases differ from the identity's phases= line (they MUST equal it)")
        for n, phase in enumerate(row["phases"] or []):
            keys = [ident.cell_key(c) for c in phase]
            if keys != sorted(keys):
                self.r.fail(1, "phase-order", w, f"phase {n} is not sorted by j, i, k, s")
            if len({k[:3] for k in keys}) != len(keys):
                self.r.fail(1, "phase-cells", w, f"phase {n} lists a cell twice")

    def _pattern_numbers(self, row, w):
        lim = self.pack.limits
        period, phases = row["period"], row["phases"] or []
        if not 1 <= period <= lim.period:
            self.r.fail(1, "pattern-period", w, f"period {period} is not from 1 to {lim.period:,}")
            return
        if row["class"] in schemas.PERIODIC_CLASSES and len(phases) != period:
            self.r.fail(1, "pattern-phases", w, f"has {len(phases)} phases for period {period}")
        if row["class"] in ("puffer", "mover") and len(phases) != 1:
            self.r.warn(1, "pattern-phases", w, f"a {row['class']} has {len(phases)} phases (its start only)")
        if phases:
            sizes = [len(p) for p in phases]
            if row["cells"] != sizes[0]:
                self.r.fail(1, "pattern-cells", w, f"cells {row['cells']} but phase 0 has {sizes[0]}")
            if row["class"] in schemas.PERIODIC_CLASSES and (row["min_cells"], row["max_cells"]) != (min(sizes), max(sizes)):
                self.r.fail(1, "pattern-cells", w, f"min/max cells {row['min_cells']}/{row['max_cells']}, "
                                                   f"phases give {min(sizes)}/{max(sizes)}")
        moving = (row["dx"], row["dy"]) != (0, 0)
        cls = row["class"]
        if cls in ("still_life", "oscillator") and moving:
            self.r.fail(1, "pattern-class", w, f"a {cls} moves by {row['dx']},{row['dy']}")
        if cls == "spaceship" and not moving:
            self.r.fail(1, "pattern-class", w, "a spaceship does not move")
        if cls == "still_life" and period != 1:
            self.r.fail(1, "pattern-class", w, f"a still life has period {period}")
        if cls == "oscillator" and period == 1:
            self.r.fail(1, "pattern-class", w, "an oscillator has period 1")
        has_rule = bool(row["rule"])
        if cls in schemas.PERIODIC_CLASSES and has_rule:
            self.r.fail(1, "pattern-rule", w, f"a {cls} has rule {row['rule']!r}; only puffers, movers, guns and other do")
        if cls not in schemas.PERIODIC_CLASSES and not has_rule:
            self.r.fail(1, "pattern-rule", w, f"a {cls} has no exact rule")

    def ants(self):
        rows = self._facts("ants", lambda r: r["id"])
        self.ants_by_id = rows
        check_ids = self.schemes.get("ant") == ident.ANT_SCHEME
        good = 0
        for aid, row in rows.items():
            w = f"ant {aid}"
            if not isinstance(aid, str) or not ident.ANT_ID.match(aid):
                self.r.fail(1, "id-format", w, "id is not ant- and 32 lower-case hex digits")
            if check_ids:
                computed = ident.ant_id(row["identity"], row["pin"])
                if computed != aid:
                    self.r.fail(1, "content-id", w, f"identity and pin hash to {computed}, not the id")
                else:
                    good += 1
            try:
                idt = ident.parse_ant_identity(row["identity"])
                for name, value in (("rule", row["rule"]), ("tiling", row["pin_key"]), ("start", row["start"])):
                    if idt[name] != value:
                        self.r.fail(1, "identity-column", w, f"identity has {name}={ident.short(idt[name])}, "
                                                             f"the columns {ident.short(value)}")
            except ident.IdentityError as e:
                self.r.fail(1, "identity-syntax", w, f"identity {e}")
            self._record_tiling(row, w)
            if row["kind"] not in ("string", "table"):
                self.r.fail(1, "ant-field", w, f"kind {row['kind']!r} is not string or table")
            if row["family"] not in ("ant", "turmite"):
                self.r.fail(1, "ant-field", w, f"family {row['family']!r} is not ant or turmite")
            if row["outcome"] not in schemas.ANT_OUTCOMES:
                self.r.fail(1, "ant-field", w, f"outcome {row['outcome']!r} is not one of {', '.join(schemas.ANT_OUTCOMES)}")
            if row["outcome"] in ("highway", "periodic"):
                missing = [n for n in ("period", "dx", "dy", "onset") if row[n] is None]
                if missing:
                    self.r.warn(1, "ant-field", w, f"a {row['outcome']} run has no {', '.join(missing)}")
            if row["period"] is not None and not 1 <= row["period"] <= self.pack.limits.period:
                self.r.fail(1, "ant-field", w, f"period {row['period']} is out of range")
            if row["states"] < 1 or row["colors"] < 1:
                self.r.fail(1, "ant-field", w, f"states {row['states']} and colors {row['colors']} must be positive")
            self._semantics(row["family"], row["semantics"], w)
        if check_ids and rows:
            self.r.ok(1, "ant content ids", good)

    def quickstart(self):
        rows = self._facts("quickstart", lambda r: r["id"])
        self.quickstart_by_id = rows
        good = 0
        for qid, row in rows.items():
            w = f"quickstart {qid}"
            if not ident.QUICKSTART_ID.match(qid or ""):
                self.r.fail(1, "id-format", w, "id is not qs- and 32 lower-case hex digits")
            computed = ident.quickstart_id(row["scene"], row["pin"])
            if computed != qid:
                self.r.fail(1, "content-id", w, f"scene and pin hash to {computed}, not the id")
            else:
                good += 1
            if row["source"] is not None:
                self._target(row["source"], w, kinds=("pattern", "ant"))
            if not ident.POSITIVE.match(row["scene_format"] or ""):
                self.r.fail(1, "quickstart-field", w, f"scene_format {row['scene_format']!r} is not a version in decimal")
            self._record_tiling(row, w)
            self._semantics(row["rule_family"], row["semantics"], w)
        if rows:
            self.r.ok(1, "quickstart content ids", good)

    # Events -------------------------------------------------------------------------------

    def _target(self, target, w, kinds=None):
        parsed = ident.target_kind(target)
        if parsed is None or (kinds and parsed[0] not in kinds):
            self.r.fail(1, "target", w, f"target {ident.short(target)!r} is not "
                                        f"{' or '.join(k + ':<id>' for k in (kinds or ('pattern', 'ant', 'quickstart')))}")
            return None
        kind, rid = parsed
        table = {"pattern": self.patterns_by_id, "ant": self.ants_by_id, "quickstart": self.quickstart_by_id}.get(kind)
        if table is not None and rid not in table:
            self.r.warn(1, "target-missing", w, f"{kind} {rid} is not in this pack")
        return parsed

    def _publisher(self, value, w):
        if not check_fingerprint(value):
            self.r.fail(1, "publisher", w, f"publisher {ident.short(value)!r} is not a SHA256 key fingerprint")

    def _time(self, row, names, w):
        for n in names:
            if not ident.is_time(row[n]):
                self.r.fail(1, "time", w, f"{n} {row[n]!r} is not an RFC 3339 UTC time with whole seconds")

    def events(self):
        n = 0
        mains = {}
        for part, i, row in self.pack.rows("verifications"):
            w, n = part.where(i), n + 1
            pat = self.patterns_by_id.get(row["pattern_id"])
            if pat is None:
                self.r.warn(1, "target-missing", w, f"pattern {row['pattern_id']} is not in this pack")
            else:
                if (row["rule"], pat["neighbourhood"], pat["steps_per_gen"]) not in self.rules_by_key:
                    self.r.fail(1, "verification-rule", w, f"rule {row['rule']!r} has no rules row for "
                                                           f"{pat['neighbourhood']}, {pat['steps_per_gen']} steps")
                if row["pin"] != pat["pin"]:
                    self.r.info(1, "needs-reverify", w, "was verified on another pin")
                if row["generations"] < row["periods"] * pat["period"]:
                    self.r.warn(1, "verification-counts", w, f"{row['generations']} generations is less than "
                                                             f"{row['periods']} periods of {pat['period']}")
            if row["main"]:
                mains.setdefault((row["pattern_id"], row["publisher"]), set()).add(row["rule"])
            self._publisher(row["publisher"], w)
            self._time(row, ("date",), w)
            if row["periods"] < 0 or row["generations"] < 0:
                self.r.fail(1, "verification-counts", w, "negative counts")
            if not ident.POSITIVE.match(row["semantics"] or ""):
                self.r.fail(1, "verification-field", w, f"semantics {row['semantics']!r} is not a positive integer string")
            if row["range"] is not None:
                rng = ident.json_cell(row["range"], "range")
                missing = [k for k in schemas.RANGE_MEMBERS if not isinstance(rng, dict) or k not in rng]
                if missing:
                    self.r.fail(1, "verification-range", w, f"range lacks {', '.join(missing)}")
                if not row["main"]:
                    self.r.warn(1, "verification-range", w, "range is given on a row that is not main")
        for (pid, pub), rules in mains.items():
            if len(rules) > 1:
                self.r.warn(1, "verification-main", f"pattern {pid}", f"publisher {pub} marks {len(rules)} rules main "
                                                                      f"({', '.join(sorted(rules))}); one is expected")
        for pid, row in self.patterns_by_id.items():
            if not row["rule"] and not any(p == pid for p, _ in mains):
                self.r.fail(1, "pattern-rule", f"pattern {pid}", "has no rule and no main verification, so no rule "
                                                                 "to run it in")
        for part, i, row in self.pack.rows("sightings"):
            w, n = part.where(i), n + 1
            if row["pattern_id"] not in self.patterns_by_id:
                self.r.warn(1, "target-missing", w, f"pattern {row['pattern_id']} is not in this pack")
            self._publisher(row["publisher"], w)
            self._time(row, ("first_seen", "last_seen"), w)
            if row["count"] < 0 or row["soups"] < 0:
                self.r.fail(1, "sighting-counts", w, "negative counts")
            if row["source"] != "unattributed" and not ident.UUID.match(row["source"]):
                self.r.fail(1, "sighting-source", w, f"source {ident.short(row['source'])!r} is not a search uuid "
                                                     "or unattributed")
        seqs = {}
        for part, i, row in self.pack.rows("provenance"):
            w, n = part.where(i), n + 1
            self._target(row["target"], w)
            self._publisher(row["publisher"], w)
            self._time(row, ("date",), w)
            k = (row["target"], row["publisher"])
            if row["seq"] in seqs.setdefault(k, set()):
                self.r.fail(1, "provenance-seq", w, f"seq {row['seq']} repeats for {row['target']} by this publisher")
            seqs[k].add(row["seq"])
            if row["kind"] not in schemas.PROVENANCE_KINDS:
                self.r.warn(1, "provenance-kind", w, f"kind {row['kind']!r} is not found, imported or reverified")
            if row["orcid"] is not None and not ident.is_orcid(row["orcid"]):
                self.r.warn(1, "orcid", w, f"ORCID {row['orcid']!r} has a wrong check digit")
            if row["pack"] is not None:
                try:
                    check_hash(row["pack"])
                except StoreError as e:
                    self.r.fail(1, e.code, w, f"pack: {e.message}")
            if row["search"] is not None and not ident.UUID.match(row["search"]):
                self.r.fail(1, "provenance-field", w, f"search {ident.short(row['search'])!r} is not a uuid")
        for (target, pub), s in seqs.items():
            if sorted(s) != list(range(len(s))):
                self.r.warn(1, "provenance-seq", f"provenance {target}", f"seqs {sorted(s)[:8]} by {pub} do not run 0, 1, 2, ...")
        for part, i, row in self.pack.rows("annotations"):
            w, n = part.where(i), n + 1
            self._target(row["target"], w)
            self._publisher(row["publisher"], w)
            self._time(row, ("date",), w)
            if row["rev"] < 1:
                self.r.fail(1, "annotation-rev", w, f"rev {row['rev']} is not a ref seq (1 or more)")
            field = row["field"]
            if field not in schemas.ANNOTATION_FIELDS:
                self.r.info(1, "annotation-field", w, f"ignoring field {field!r}")
                continue
            v = ident.json_cell(row["value"], "value")
            ok = v is None or {
                "tags": isinstance(v, list) and all(isinstance(x, str) for x in v),
                "rating": isinstance(v, int) and not isinstance(v, bool) and 1 <= v <= 5,
                "score": isinstance(v, str) and bool(_DECIMAL.match(v)),
            }.get(field, isinstance(v, str))
            if not ok:
                self.r.fail(1, "annotation-value", w, f"{field} value {ident.short(row['value'])} is not of its field's kind")
            if (field == "score") != (row["score_model"] is not None):
                self.r.warn(1, "annotation-field", w, "score_model should be set for a score, and only then")
        if n:
            self.r.ok(1, "event rows", n)

    def media(self):
        for part, i, row in self.pack.rows("media"):
            w = part.where(i)
            self._target(row["target"], w)
            if row["kind"] not in schemas.MEDIA_KINDS:
                self.r.fail(1, "media-kind", w, f"kind {row['kind']!r} is not one of {', '.join(schemas.MEDIA_KINDS)}"
                                                " (frame lists are never packed)")
            ext = schemas.MEDIA_TYPES.get(row["media_type"])
            if ext is None:
                self.r.fail(1, "media-type", w, f"media_type {row['media_type']!r} is not one of {sorted(schemas.MEDIA_TYPES)}")
                continue
            try:
                f, size = self.pack.store.open_object(row["object"], ext)
                f.close()
                self.r.ok(1, "media object hashes")
                if size != row["bytes"]:
                    self.r.fail(1, "media-bytes", w, f"the file is {size:,} bytes; the row says {row['bytes']:,}")
            except StoreError as e:
                self.r.fail(1, e.code, w, f"object: {e.message}")

    def ratings(self):
        raters = self._facts("raters", lambda r: r["rater"])
        self.raters = raters
        for rid, row in raters.items():
            w = f"raters {rid}"
            if not ident.RATER_ID.match(rid or ""):
                self.r.fail(1, "rater-id", w, "is not rater- and 16 hex digits")
            if row["consent_scope"] != "public-permanent":
                self.r.fail(1, "rater-consent", w, f"consent_scope {row['consent_scope']!r} is not public-permanent")
            if not ident.is_time(row["consent_date"]):
                self.r.fail(1, "time", w, f"consent_date {row['consent_date']!r} is not an RFC 3339 UTC time "
                                          "with whole seconds and Z")
            if row["orcid"] is not None and not ident.is_orcid(row["orcid"]):
                self.r.warn(1, "orcid", w, f"ORCID {row['orcid']!r} has a wrong check digit")
            members = row["members"]
            if members is not None:
                if not members:
                    self.r.fail(1, "rater-group", w, "a group has no members")
                if row["display_name"] is not None or row["orcid"] is not None:
                    self.r.fail(1, "rater-group", w, "a group has no display name or ORCID of its own")
                for m in members:
                    member = raters.get(m)
                    if member is None:
                        self.r.fail(1, "rater-group", w, f"member {m!r} has no raters row, so has not consented")
                    elif member["members"] is not None:
                        self.r.fail(1, "rater-group", w, f"member {m!r} is itself a group (groups do not nest)")
                    elif member["consent_scope"] != "public-permanent":
                        self.r.fail(1, "rater-group", w, f"member {m!r} has not consented to public-permanent release")
                if len(set(members)) != len(members):
                    self.r.fail(1, "rater-group", w, "lists a member twice")
        n = 0
        for part, i, row in self.pack.rows("ratings"):
            w, n = part.where(i), n + 1
            rater = raters.get(row["rater"])
            if rater is None:
                self.r.fail(1, "rater-consent", w, f"rater {row['rater']!r} has no raters row, so no recorded consent")
            elif row["note"] is not None and not rater["notes"]:
                self.r.fail(1, "rater-consent", w, "has a note, but the rater did not agree to share notes")
            if ident.target_kind(row["target"]) is None:
                self.r.fail(1, "target", w, f"target {ident.short(row['target'])!r} is not a record or search_run target")
            values = schemas.SCALES.get(row["scale"])
            if values is None:
                self.r.fail(1, "rating-scale", w, f"scale {row['scale']!r} is not one of {', '.join(schemas.SCALES)}")
            elif row["verdict"] not in values:
                self.r.fail(1, "rating-scale", w, f"verdict {row['verdict']!r} is not on the scale {row['scale']}")
            self._time(row, ("date",), w)
        if n:
            self.r.ok(1, "ratings rows with consent", n)

    def optional_tables(self):
        aliases = self._facts("aliases", lambda r: r["old_id"])
        for old, row in aliases.items():
            w = f"aliases {old}"
            if row["reason"] not in schemas.ALIAS_REASONS:
                self.r.fail(1, "alias-reason", w, f"reason {row['reason']!r} is not geometry-change or scheme-change")
            self._target(row["target"], w)
            self._time(row, ("date",), w)
        for part, i, row in self.pack.rows("score_models"):
            w = part.where(i)
            if len(row["features"] or []) != len(row["weights"] or []):
                self.r.fail(1, "score-model", w, f"{len(row['features'])} features but {len(row['weights'])} weights")
            if row["rater"] is not None and row["rater"] not in self.raters:
                self.r.warn(1, "rater-consent", w, f"rater {row['rater']!r} has no raters row")

    # Manifest-level content -----------------------------------------------------------------

    def retractions(self):
        seen = set()
        for n, x in enumerate(self.m.get("retracts", []) if isinstance(self.m.get("retracts"), list) else []):
            w = f"{self.w} retracts[{n}]"
            if not isinstance(x, dict):
                continue
            self._target(x.get("target"), w)
            if not check_fingerprint(x.get("publisher")):
                self.r.fail(1, "retraction", w, f"publisher {x.get('publisher')!r} is not a fingerprint")
            if not isinstance(x.get("reason"), str) or not x["reason"]:
                self.r.fail(1, "retraction", w, "has no reason")
            if not ident.is_time(x.get("date")):
                self.r.fail(1, "retraction", w, f"date {x.get('date')!r} is not an RFC 3339 UTC time")
            k = (x.get("target"), x.get("publisher"))
            if k in seen:
                self.r.warn(1, "retraction", w, "repeats a retraction")
            seen.add(k)

    def searches(self):
        searches = self.m.get("searches") if isinstance(self.m.get("searches"), dict) else {}
        self.pack.search_indexes = {}
        store = self.pack.store
        for uuid, h in searches.items():
            w = f"search {uuid}"
            seen, stack, docs = set(), [(h, 1)], []
            while stack:
                ih, depth = stack.pop()
                if ih in seen:
                    continue
                seen.add(ih)
                if depth > store.limits.index_depth:
                    self.r.fail(0, "index-depth", w, f"search index {sh(ih)} is nested {depth} deep")
                    continue
                try:
                    raw, doc = store.read_json_object(ih, "search index")
                except StoreError as e:
                    self.r.fail(1, e.code, w, e.message)
                    continue
                self.r.ok(1, "index hashes")
                if not self._search_index(doc, raw, uuid, ih, w):
                    continue
                docs.append(doc)
                stack += [(c, depth + 1) for c in reversed(doc.get("children", []))]
            merged = {"parts": [p for d in docs for p in d.get("parts", [])],
                      "scores": [s for d in docs for s in d.get("scores", [])]}
            self.pack.search_indexes[uuid] = merged
            for entry in merged["parts"]:
                self._search_object(entry, w, segment=True)
            for score in merged["scores"]:
                for entry in score.get("parts", []) if isinstance(score, dict) else []:
                    self._search_object(entry, w, segment=False)

    def _search_index(self, doc, raw, uuid, h, w):
        if not isinstance(doc, dict) or doc.get("format") != "jetcarp-index" or doc.get("table") != "search":
            self.r.fail(0, "search-index", w, 'is not a jetcarp-index with table "search"')
            return False
        ok, known = ident.is_version(doc.get("version"))
        if not ok or not known:
            self.r.fail(0, "unknown-major", w, f"search index version {doc.get('version')!r} is not 1.x")
            return False
        if doc.get("schema") != "jetcarp-search 1":
            self.r.fail(0, "unknown-schema", w, f"schema {doc.get('schema')!r} is not jetcarp-search 1")
            return False
        if doc.get("search") != uuid:
            self.r.fail(1, "search-index", w, f"names search {doc.get('search')!r}, not the uuid the manifest maps")
        if not jcs.canonical(doc) == raw:
            self.r.fail(1, "not-canonical", w, f"search index {sh(h)} is not in RFC 8785 canonical form")
        parts, children = doc.get("parts", []), doc.get("children", [])
        if not isinstance(parts, list) or not isinstance(children, list) or not isinstance(doc.get("scores", []), list):
            self.r.fail(0, "search-index", w, "parts, scores and children must be lists")
            return False
        if len(parts) > self.pack.limits.parts_per_index or len(children) > self.pack.limits.parts_per_index:
            self.r.fail(0, "index-parts", w, "lists more than 10,000 parts or children; refused")
            return False
        for path, s in _strings({"spec": doc.get("spec"), "remeasure": doc.get("remeasure")}):
            if ident.looks_like_local_path(s):
                self.r.fail(1, "local-path", w, f"{path} looks like a local path: {ident.short(s)}")
        prev = 0
        for entry in parts:
            if not isinstance(entry, dict):
                self.r.fail(0, "search-index", w, "a part entry is not an object")
                return False
            first, last = entry.get("first"), entry.get("last")
            if not (isinstance(first, str) and first.isdigit() and isinstance(last, str) and last.isdigit()):
                self.r.fail(1, "search-index", w, "a part's first and last are not run ids in decimal")
            elif int(first) <= prev:
                self.r.warn(1, "search-index", w, "parts are not in run id order")
            else:
                prev = int(last)
        return True

    def _search_object(self, entry, w, segment):
        if not isinstance(entry, dict):
            self.r.fail(0, "search-index", w, "a part entry is not an object")
            return
        seg = entry.get("segment")
        if not isinstance(seg, str) or not _SEGMENT.match(seg):
            self.r.fail(0, "search-index", w, f"segment {seg!r} is not part- and digits; refused")
            return
        try:
            f, size = self.pack.store.open_object(entry.get("object"), "parquet")
        except StoreError as e:
            self.r.fail(1, e.code, w, f"{seg}: {e.message}")
            return
        f.close()
        self.r.ok(1, "search object hashes")
        if size != entry.get("bytes"):
            self.r.fail(1, "part-bytes", w, f"{seg} is {size:,} bytes; its index says {entry.get('bytes')}")

    def stats(self):
        stats = self.m.get("stats") if isinstance(self.m.get("stats"), dict) else {}
        for name in ("patterns", "ants", "quickstart"):
            t = self.pack.table(name)
            actual = t.row_count() if t else 0
            if name in stats and stats[name] != actual:
                self.r.info(1, "stats", self.w, f"stats.{name} is {stats[name]}; the table has {actual} rows "
                                                "(stats are not authoritative)")


import re as _re  # noqa: E402

_HEX64 = _re.compile(r"[0-9a-f]{64}\Z")
_DECIMAL = _re.compile(r"-?(0|[1-9][0-9]*)(\.[0-9]+)?(e[+-][0-9]+)?\Z")
_SEGMENT = _re.compile(r"part-[0-9]{1,12}\Z")


def _strings(value, path="$"):
    stack = [(path, value)]
    while stack:
        p, v = stack.pop()
        if isinstance(v, str):
            yield p, v
        elif isinstance(v, list):
            stack.extend((f"{p}[{i}]", x) for i, x in enumerate(v))
        elif isinstance(v, dict):
            stack.extend((f"{p}.{k}", x) for k, x in v.items())


def _comparable(row):
    return repr(sorted(row.items()))
