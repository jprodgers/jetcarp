"""Conformance level 1, integrity (pack spec section 16): every object hash, every manifest hash, and
every content id recomputed from its `identity` column and pin (section 9).

Beyond the hashes, level 1 here also checks that each table agrees with itself and with the others:
the typed columns against the identity text they copy, records against the tilings they name, the
consent rules for ratings, and the manifest's fields. These are reported under their own codes, so
"the bytes are intact" and "the content is consistent" stay apart.
"""

import hashlib
import unicodedata

from . import identity as ident
from . import schemas
from .pack import Pack, check_store_marker, list_refs, sh, check_fingerprint
from .store import StoreError, check_hash, FEED_RE

MANIFEST_FIELDS = {"format", "version", "requires", "feed", "title", "description", "version_label",
                   "created", "licence", "creators", "doi", "related", "parents", "publisher", "engine",
                   "tables", "searches", "retracts", "retain", "stats", "slice_of", "filter"}


def scan_objects(store, report):
    """Every file under objects/: named <sha256 hex>.<ext> in its folder, and hashing to its name."""
    n = 0
    for comps, problem in store.object_files():
        where = "/".join(comps)
        if problem:
            report.fail(1, "object-name", where, problem)
            continue
        h = "sha256:" + comps[-1].split(".")[0]
        try:
            f, _ = store.open_object(h, comps[-1].split(".")[1])
            f.close()
            n += 1
        except StoreError as e:
            report.fail(1, e.code, where, e.message)
    report.ok(1, "objects in the store hashing to their names", n)
    for name in store.list(()):
        if name not in ("jetcarp-store.json", "refs", "objects", "sigs", "keys", "receipts", "views.sql", "torrents"):
            report.info(1, "unknown-path", name, "ignored (a reader ignores top-level paths it does not know)")


def manifests_to_check(store, report, manifest=None):
    """The manifests named by refs (or the one given), with their parents and retained versions that
    the store holds."""
    roots = [manifest] if manifest else [doc["manifest"] for _, doc in list_refs(store, report).values()]
    order, seen = [], set()
    stack = list(reversed(roots))
    while stack and len(order) < 10_000:
        h = stack.pop()
        if h in seen:
            continue
        seen.add(h)
        order.append(h)
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
                if store.exists(store.object_components(p, "json")):
                    stack.append(p)
    return order


def check_store(store, report, manifest=None, scan=True):
    if not check_store_marker(store, report):
        return []
    if scan:
        scan_objects(store, report)
    packs = []
    cache = {}
    for h in manifests_to_check(store, report, manifest):
        pack = Pack(store, h, report, cache=cache)
        if pack.ok:
            check_pack(pack, report)
            packs.append(pack)
    if not packs and not report.failed():
        report.fail(1, "no-manifest", store.root, "found no manifest to check")
    return packs


def check_pack(pack, report):
    c = Checker(pack, report)
    for step in (c.manifest_fields, c.tilings, c.rules, c.patterns, c.ants, c.quickstart, c.events, c.media,
                 c.ratings, c.optional_tables, c.retractions, c.searches, c.stats):
        try:
            step()
        except Exception as e:  # a hostile value this reader did not foresee: a failure, never a crash
            report.fail(1, "check-error", pack.where, f"{step.__name__} stopped on {type(e).__name__}: {str(e)[:200]}")


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

        def is_str(name, empty_ok=True):
            v = m.get(name)
            if not isinstance(v, str) or (not empty_ok and not v):
                bad(name, "is not a string" if empty_ok else "is not a non-empty string")
                return None
            return v

        if not isinstance(m.get("feed"), str) or not FEED_RE.match(m["feed"]):
            bad("feed", "is not a name of letters, digits and -")
        for name in ("title", "description", "version_label", "doi"):
            is_str(name)
        if not ident.is_time(m.get("created")):
            bad("created", "is not an RFC 3339 UTC time with whole seconds and Z")
        if not isinstance(m.get("licence"), str) or not ident.SPDX.match(m["licence"]):
            bad("licence", "is not an SPDX identifier")
        creators = m.get("creators", [])
        if not isinstance(creators, list) or not all(isinstance(c, dict) and isinstance(c.get("name"), str)
                                                      for c in creators):
            bad("creators", "is not a list of objects with a name")
        else:
            for c in creators:
                if c.get("orcid") and not ident.is_orcid(c["orcid"]):
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
        for name, kind in (("tables", "index"), ("searches", "index")):
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
            r.info(1, "unknown-field", w, f"ignoring fields {unknown}")
        for path, s in _strings(m):
            if not unicodedata.is_normalized("NFC", s):
                r.warn(1, "not-nfc", w, f"{path} is not Unicode NFC (section 23.4)")
            if ident.looks_like_local_path(s):
                r.warn(1, "local-path", w, f"{path} looks like a local path: {ident.short(s)}")
        for name, expected in (("pattern", ident.PATTERN_SCHEME), ("ant", ident.ANT_SCHEME),
                               ("geometry", ident.GEOMETRY_SCHEME)):
            got = self.schemes.get(name)
            if got != expected:
                r.skip(1, "unknown-scheme", w, f"engine.schemes.{name} is {got!r}; this reader checks "
                                               f"{expected} only, so those ids are not checked")
        # Parents and retained versions.
        for name in ("parents", "retain"):
            for h in m.get(name, []) if isinstance(m.get(name), list) else []:
                try:
                    present = self.pack.store.exists(self.pack.store.object_components(h, "json"))
                except StoreError:
                    continue
                if not present and name == "retain":
                    r.fail(1, "retain-missing", w, f"retains {sh(h)}, which the store does not hold (hosts MUST keep it)")
                elif not present:
                    r.info(1, "parent-missing", w, f"parent {sh(h)} is not in this store")

    # Facts --------------------------------------------------------------------------------

    def _facts(self, name, key_of):
        """Rows of a fact table by key; duplicates must be identical (section 1, promise 4)."""
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
        lattice = 0
        for key, row in rows.items():
            w = f"tilings pin_key {ident.short(key)}"
            if not isinstance(row["pin"], str) or not ident.HEX32.match(row["pin"]):
                self.r.fail(1, "pin-format", w, f"pin {row['pin']!r} is not 32 lower-case hex digits")
            try:
                parsed = ident.check_tiling_key(key)
            except (ident.IdentityError, ValueError) as e:
                self.r.fail(1, "tiling-key", w, f"pin_key {e}")
                parsed = None
            if parsed is not None:
                if row["family"] != parsed["family"]:
                    self.r.fail(1, "tiling-family", w, f"family {row['family']!r} differs from the key's {parsed['family']!r}")
                try:
                    params = ident.json_cell(row["params"], "params")
                except ValueError as e:
                    self.r.fail(1, "tiling-params", w, str(e))
                    params = None
                if params is not None:
                    key_params = parsed.get("params")
                    mine = params
                    if parsed["family"] in ident.jcs.APERIODIC_FAMILIES and isinstance(mine, dict):
                        mine = {k: v for k, v in mine.items() if k != "level"}
                    if (mine or None) != (key_params or None):
                        self.r.fail(1, "tiling-params", w, f"params {ident.short(row['params'])} differ from the key's")
            if row["kind"] == "lattice":
                lattice += 1
                self._lattice_geometry(row, w)
            elif row["kind"] == "aperiodic":
                if row["reference_level"] is None:
                    self.r.fail(1, "tiling-geometry", w, "an aperiodic tiling has no reference_level")
            else:
                self.r.fail(1, "tiling-kind", w, f"kind {row['kind']!r} is not lattice or aperiodic")
        if rows:
            self.r.ok(1, "tilings rows", len(rows))
            self.r.skip(1, "pin-not-recomputed", f"{self.w} tilings",
                        "pins are not recomputed: the spec points to pin.rs for the byte layout but does not "
                        "copy it (section 11)")

    def _lattice_geometry(self, row, w):
        for name in ("a1", "a2", "tile_types", "edge", "touch"):
            if row[name] is None:
                self.r.fail(1, "tiling-geometry", w, f"a lattice tiling has no {name}")
                return
        if len(row["a1"]) != 2 or len(row["a2"]) != 2:
            self.r.fail(1, "tiling-geometry", w, "a1 and a2 are not 2-vectors")
        types = len(row["tile_types"])
        if not types:
            self.r.fail(1, "tiling-geometry", w, "has no tile types")
        for t in row["tile_types"]:
            if len(t["corners"]) < 3 or any(len(c) != 2 for c in t["corners"]):
                self.r.fail(1, "tiling-geometry", w, "a tile type's corners are not a polygon of 2-vectors")
                break
        for nb in ("edge", "touch"):
            if len(row[nb]) != types:
                self.r.fail(1, "tiling-geometry", w, f"{nb} has {len(row[nb])} lists for {types} tile types")
                continue
            for lst in row[nb]:
                if any(o["k"] is None or not 0 <= o["k"] < types or o["di"] is None or o["dj"] is None for o in lst):
                    self.r.fail(1, "tiling-geometry", w, f"an offset in {nb} names a tile type out of range")
                    break
        if row["reference_level"] is not None:
            self.r.warn(1, "tiling-geometry", w, "a lattice tiling has a reference_level")

    def _record_tiling(self, row, w):
        t = self.tilings_by_key.get(row["pin_key"])
        if t is None:
            self.r.fail(1, "record-tiling", w, f"its tiling {ident.short(row['pin_key'])} has no row in tilings")
        elif t["pin"] != row["pin"]:
            self.r.fail(1, "record-pin", w, f"pin {row['pin']} differs from its tiling's {t['pin']}")
        try:
            group = ident.tiling_group(ident.check_tiling_key(row["pin_key"]))
        except (ident.IdentityError, ValueError):
            group = None
        if group is not None and row.get("tiling_group") is not None and row["tiling_group"] != group:
            self.r.warn(1, "tiling-group", w, f"tiling_group {row['tiling_group']!r}; its tiling's group is {group!r}")
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
            if not isinstance(row["semantics"], str) or not ident.POSITIVE.match(row["semantics"]):
                self.r.fail(1, "rule-field", w, f"semantics {row['semantics']!r} is not a positive integer string")
            self._semantics(row["family"], row["semantics"], w)
            if row["family"] in schemas.RULE_FAMILIES_WITH_MASKS:
                for name in ("birth", "survive"):
                    v = row[name]
                    if v is None:
                        self.r.fail(1, "rule-masks", w, f"a {row['family']} rule has no {name} mask")
                    elif not 0 <= v < 2 ** 63:
                        self.r.fail(1, "rule-masks", w, f"{name} mask {v} is negative")
                if row["family"] == "lifelike" and row["states"] != 2:
                    self.r.fail(1, "rule-field", w, f"a B/S rule has {row['states']} states, not 2")
                if row["family"] == "generations" and not 3 <= (row["states"] or 0) <= 255:
                    self.r.warn(1, "rule-field", w, f"a Generations rule has {row['states']} states (3 to 255 expected)")
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
                self.r.warn(1, "class-group", w, f"class_group {row['class_group']!r} differs from class "
                                                 f"{row['class']!r} (the spec does not say how they relate)")
            self._pattern_numbers(row, w)
            if row.get("semantics") is not None or row.get("family") is not None:
                self._semantics(row.get("family"), row.get("semantics"), w)
            tiling = self.tilings_by_key.get(row["pin_key"])
            if tiling is not None:
                if tiling["kind"] == "aperiodic" and row["site"] is None:
                    self.r.warn(1, "pattern-site", w, "a pattern on an aperiodic tiling has no site")
                if tiling["kind"] == "lattice" and row["site"] is not None:
                    self.r.warn(1, "pattern-site", w, "a pattern on a lattice has a site")
            if row["class"] == "gun":
                if row["gun"] is None:
                    self.r.warn(1, "pattern-gun", w, "a gun has no gun column")
                else:
                    try:
                        ident.json_cell(row["gun"], "gun")
                    except ValueError as e:
                        self.r.fail(1, "pattern-gun", w, str(e))
        if check_ids and rows:
            self.r.ok(1, "pattern content ids", good)

    def _pattern_identity(self, row, w):
        try:
            idt = ident.parse_pattern_identity(row["identity"])
        except ident.IdentityError as e:
            self.r.fail(1, "identity-syntax", w, f"identity {e}")
            return
        if not unicodedata.is_normalized("NFC", row["identity"]):
            self.r.warn(1, "not-nfc", w, "identity is not Unicode NFC")
        expect = {
            "tiling": row["pin_key"], "model": row["model"], "nb": row["neighbourhood"],
            "steps": str(row["steps_per_gen"]), "class": row["class"], "period": str(row["period"]),
            "d": f"{row['dx']},{row['dy']}", "rule": row["rule"] or "", "canonical": row["canonical"],
        }
        for name, value in expect.items():
            if idt[name] != value:
                self.r.fail(1, "identity-column", w, f"identity has {name}={ident.short(idt[name])}, "
                                                     f"the columns {ident.short(value)}")
        typed = ident.format_phases(row["phases"] or [])
        if idt["phases"] != typed:
            try:
                parsed = ident.parse_phases(idt["phases"], self.pack.limits.phase_cells)
            except ident.IdentityError:
                parsed = None
            same = parsed is not None and len(parsed) == len(row["phases"]) and all(
                sorted(map(ident.cell_key, a)) == sorted(map(ident.cell_key, b)) for a, b in zip(parsed, row["phases"]))
            if same:
                self.r.warn(1, "identity-phases", w, "typed phases hold the identity's cells in another order")
            else:
                self.r.fail(1, "identity-phases", w, "typed phases differ from the identity's phases= line")
        for n, phase in enumerate(row["phases"] or []):
            keys = [ident.cell_key(c) for c in phase]
            if keys != sorted(keys):
                self.r.warn(1, "phase-order", w, f"phase {n} is not sorted by j, i, k, s")
            if len({k[:3] for k in keys}) != len(keys):
                self.r.fail(1, "phase-cells", w, f"phase {n} lists a cell twice")

    def _pattern_numbers(self, row, w):
        lim = self.pack.limits
        period, phases = row["period"], row["phases"] or []
        if period is None or not 1 <= period <= lim.period:
            self.r.fail(1, "pattern-period", w, f"period {period} is not from 1 to {lim.period:,}")
            return
        if row["class"] in schemas.PERIODIC_CLASSES and len(phases) != period:
            self.r.fail(1, "pattern-phases", w, f"has {len(phases)} phases for period {period}")
        if row["class"] in ("puffer", "mover") and len(phases) != 1:
            self.r.warn(1, "pattern-phases", w, f"a {row['class']} has {len(phases)} phases (its start only expected)")
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
            self.r.warn(1, "pattern-rule", w, f"a {cls} has no exact rule")

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
            if row["outcome"] not in schemas.ANT_OUTCOMES:
                self.r.fail(1, "ant-field", w, f"outcome {row['outcome']!r} is not one of {', '.join(schemas.ANT_OUTCOMES)}")
            if row["outcome"] in ("highway", "periodic"):
                missing = [n for n in ("period", "dx", "dy", "onset") if row[n] is None]
                if missing:
                    self.r.warn(1, "ant-field", w, f"a {row['outcome']} run has no {', '.join(missing)}")
            if row["period"] is not None and not 1 <= row["period"] <= self.pack.limits.period:
                self.r.fail(1, "ant-field", w, f"period {row['period']} is out of range")
            if row.get("semantics") is not None:
                self._semantics(row.get("family"), row.get("semantics"), w)
        if check_ids and rows:
            self.r.ok(1, "ant content ids", good)

    def quickstart(self):
        rows = self._facts("quickstart", lambda r: r["id"])
        self.quickstart_by_id = rows
        for qid, row in rows.items():
            w = f"quickstart {qid}"
            if not ident.QUICKSTART_ID.match(qid or ""):
                self.r.fail(1, "id-format", w, "id is not a short name of letters, digits, '.', '_' and '-'")
            if row["source"] is not None:
                self._target(row["source"], w, kinds=("pattern", "ant"))
            try:
                ident.json_cell(row["scene"], "scene")
            except ValueError as e:
                self.r.fail(1, "quickstart-scene", w, str(e))
            if (row["pin_key"] is None) != (row["pin"] is None):
                self.r.fail(1, "quickstart-pin", w, "has one of pin_key and pin without the other")
            elif row["pin_key"] is not None:
                self._record_tiling(row, w)
            self._semantics(row["rule_family"], row["semantics"], w)
        if rows:
            self.r.skip(1, "quickstart-id", f"{self.w} quickstart",
                        f"{len(rows)} Quick Start ids not checked: the spec gives no identity text for them")

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
        for part, i, row in self.pack.rows("verifications"):
            w, n = part.where(i), n + 1
            pat = self.patterns_by_id.get(row["pattern_id"])
            if pat is None:
                self.r.warn(1, "target-missing", w, f"pattern {row['pattern_id']} is not in this pack")
            else:
                if (row["rule"], pat["neighbourhood"], pat["steps_per_gen"]) not in self.rules_by_key:
                    self.r.warn(1, "verification-rule", w, f"rule {row['rule']!r} has no rules row for "
                                                           f"{pat['neighbourhood']}, {pat['steps_per_gen']} steps")
                if row["pin"] != pat["pin"]:
                    self.r.info(1, "needs-reverify", w, "was verified on another pin")
                if row["generations"] < row["periods"] * pat["period"]:
                    self.r.warn(1, "verification-counts", w, f"{row['generations']} generations is less than "
                                                             f"{row['periods']} periods of {pat['period']}")
            self._publisher(row["publisher"], w)
            self._time(row, ("date",), w)
            if row["periods"] < 0 or row["generations"] < 0:
                self.r.fail(1, "verification-counts", w, "negative counts")
            if not ident.POSITIVE.match(row["semantics"] or ""):
                self.r.fail(1, "verification-field", w, f"semantics {row['semantics']!r} is not a positive integer string")
            if row["range"] is not None:
                try:
                    ident.json_cell(row["range"], "range")
                except ValueError as e:
                    self.r.fail(1, "verification-field", w, str(e))
        for part, i, row in self.pack.rows("sightings"):
            w, n = part.where(i), n + 1
            if row["pattern_id"] not in self.patterns_by_id:
                self.r.warn(1, "target-missing", w, f"pattern {row['pattern_id']} is not in this pack")
            self._publisher(row["publisher"], w)
            self._time(row, ("first_seen", "last_seen"), w)
            if row["count"] < 0 or row["soups"] < 0:
                self.r.fail(1, "sighting-counts", w, "negative counts")
            if ident.looks_like_local_path(row["source"]):
                self.r.fail(1, "local-path", w, f"source {ident.short(row['source'])!r} looks like a local path")
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
            for col in ("source", "search"):
                if row[col] and ident.looks_like_local_path(row[col]):
                    self.r.fail(1, "local-path", w, f"{col} {ident.short(row[col])!r} looks like a local path")
        for (target, pub), s in seqs.items():
            if sorted(s) != list(range(len(s))):
                self.r.warn(1, "provenance-seq", f"provenance {target}", f"seqs {sorted(s)[:8]} by {pub} do not run 0, 1, 2, ...")
        for part, i, row in self.pack.rows("annotations"):
            w, n = part.where(i), n + 1
            self._target(row["target"], w)
            self._publisher(row["publisher"], w)
            self._time(row, ("date",), w)
            if row["field"] not in schemas.ANNOTATION_FIELDS:
                self.r.warn(1, "annotation-field", w, f"field {row['field']!r} is not one of {', '.join(schemas.ANNOTATION_FIELDS)}")
            try:
                v = ident.json_cell(row["value"], "value")
                ok = (isinstance(v, str) or (isinstance(v, int) and not isinstance(v, bool))
                      or (isinstance(v, list) and all(isinstance(x, str) for x in v)))
                if not ok:
                    self.r.fail(1, "annotation-value", w, f"value {ident.short(row['value'])} is not a string, "
                                                          "list of strings or integer")
            except ValueError as e:
                self.r.fail(1, "annotation-value", w, str(e))
            if (row["field"] == "score") != (row["score_model"] is not None):
                self.r.warn(1, "annotation-field", w, "score_model should be set for a score, and only then")
        if n:
            self.r.ok(1, "event rows", n)

    def media(self):
        n = 0
        for part, i, row in self.pack.rows("media"):
            w, n = part.where(i), n + 1
            self._target(row["target"], w)
            if row["kind"] not in schemas.MEDIA_KINDS:
                self.r.fail(1, "media-kind", w, f"kind {row['kind']!r} is not one of {', '.join(schemas.MEDIA_KINDS)}"
                                                " (packs carry no frame lists, section 25)")
            ext = schemas.MEDIA_TYPES.get(row["media_type"])
            if ext is None:
                self.r.fail(1, "media-type", w, f"media_type {row['media_type']!r} is not one of {sorted(schemas.MEDIA_TYPES)}")
                continue
            try:
                f, _ = self.pack.store.open_object(row["object"], ext)
                f.close()
                self.r.ok(1, "media object hashes")
            except StoreError as e:
                self.r.fail(1, e.code, w, f"object: {e.message}")
            try:
                ident.json_cell(row["render"], "render")
            except ValueError as e:
                self.r.fail(1, "media-render", w, str(e))

    def ratings(self):
        raters = self._facts("raters", lambda r: r["rater"])
        self.raters = raters
        for rid, row in raters.items():
            w = f"raters {rid}"
            if row["consent_scope"] != "public-permanent":
                self.r.fail(1, "rater-consent", w, f"consent_scope {row['consent_scope']!r} is not public-permanent")
            if not ident.is_time(row["consent_date"]):
                self.r.fail(1, "time", w, f"consent_date {row['consent_date']!r} is not an RFC 3339 UTC time")
            if row["orcid"] is not None and not ident.is_orcid(row["orcid"]):
                self.r.warn(1, "orcid", w, f"ORCID {row['orcid']!r} has a wrong check digit")
            members = row.get("members")
            if members is not None:
                if not members:
                    self.r.fail(1, "rater-group", w, "a group has no members")
                if row["display_name"] is not None or row["orcid"] is not None:
                    self.r.fail(1, "rater-group", w, "a group has no display name or ORCID of its own (section 25)")
                for m in members:
                    member = raters.get(m)
                    if member is None:
                        self.r.fail(1, "rater-group", w, f"member {m!r} has no raters row, so has not consented")
                    elif member.get("members") is not None:
                        self.r.fail(1, "rater-group", w, f"member {m!r} is itself a group")
                    elif member["consent_scope"] != "public-permanent":
                        self.r.fail(1, "rater-group", w, f"member {m!r} has not consented to public-permanent release")
                if len(set(members)) != len(members):
                    self.r.fail(1, "rater-group", w, "lists a member twice")
        n = 0
        for part, i, row in self.pack.rows("ratings"):
            w, n = part.where(i), n + 1
            if row["rater"] not in raters:
                self.r.fail(1, "rater-consent", w, f"rater {row['rater']!r} has no raters row, so no recorded consent")
            if ident.target_kind(row["target"]) is None:
                self.r.fail(1, "target", w, f"target {ident.short(row['target'])!r} is not a record or search_run target")
            self._time(row, ("date",), w)
        if n:
            self.r.ok(1, "ratings rows with consent", n)

    def optional_tables(self):
        for part, i, row in self.pack.rows("aliases"):
            w = part.where(i)
            if row["reason"] not in schemas.ALIAS_REASONS:
                self.r.fail(1, "alias-reason", w, f"reason {row['reason']!r} is not geometry-change or scheme-change")
            self._target(row["target"], w)
            self._time(row, ("date",), w)
        models = self._facts("score_models", lambda r: (r["model"], r["version"]))
        for (model, version), row in models.items():
            w = f"score_models {model} {version}"
            if len(row["features"] or []) != len(row["weights"] or []):
                self.r.fail(1, "score-model", w, f"{len(row['features'])} features but {len(row['weights'])} weights")
            try:
                ident.json_cell(row["metrics"], "metrics")
            except ValueError as e:
                self.r.fail(1, "score-model", w, str(e))
            if row["rater"] is not None and row["rater"] not in self.raters:
                self.r.warn(1, "rater-consent", w, f"rater {row['rater']!r} has no raters row")

    # Manifest-level content -----------------------------------------------------------------

    def retractions(self):
        for n, x in enumerate(self.m.get("retracts", []) if isinstance(self.m.get("retracts"), list) else []):
            w = f"{self.w} retracts[{n}]"
            if not isinstance(x, dict):
                continue
            self._target(x.get("target"), w)
            if not isinstance(x.get("reason"), str) or not x["reason"]:
                self.r.fail(1, "retraction", w, "has no reason")
            if not ident.is_time(x.get("date")):
                self.r.fail(1, "retraction", w, f"date {x.get('date')!r} is not an RFC 3339 UTC time")

    def searches(self):
        searches = self.m.get("searches") if isinstance(self.m.get("searches"), dict) else {}
        store = self.pack.store
        for uuid, h in searches.items():
            w = f"search {uuid}"
            try:
                raw, doc = store.read_json_object(h, "search index")
            except StoreError as e:
                self.r.fail(1, e.code, w, e.message)
                continue
            self.r.ok(1, "index hashes")
            parts = doc.get("parts", []) if isinstance(doc, dict) else []
            for entry in parts[: store.limits.parts_per_index]:
                obj = entry.get("object") if isinstance(entry, dict) else None
                found = False
                for ext in ("parquet", "json"):  # an entry does not say its extension (choice)
                    try:
                        if store.exists(store.object_components(obj, ext)):
                            f, _ = store.open_object(obj, ext)
                            f.close()
                            found = True
                            self.r.ok(1, "search object hashes")
                            break
                    except StoreError as e:
                        self.r.fail(1, e.code, w, e.message)
                        found = True
                        break
                if not found:
                    self.r.fail(1, "missing", w, f"object {sh(obj)} is not in the store")
            self.r.skip(1, "search-runs", w, "search runs are hashed but not read (their schema is search-runs.md's)")

    def stats(self):
        stats = self.m.get("stats") if isinstance(self.m.get("stats"), dict) else {}
        for name in ("patterns", "ants", "quickstart"):
            t = self.pack.table(name)
            actual = t.row_count() if t else 0
            if name in stats and stats[name] != actual:
                self.r.info(1, "stats", self.w, f"stats.{name} is {stats[name]}; the table has {actual} rows "
                                                "(stats are not authoritative)")


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


def row_digest(row):
    return hashlib.sha256(_comparable(row).encode()).hexdigest()
