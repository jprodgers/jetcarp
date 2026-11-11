"""Opening one version of a pack: its manifest, its indexes and its Parquet parts
(docs/src/reference/pack-format.md: "The store", "The manifest", "Tables, indexes and parts",
"Versions and compatibility", "Limits").

Everything read is checked as it is read: object hashes (a reader MUST check them before use), the
refusals (an unknown major version, `requires` or schema number), and the limits. Problems go into a
Report; a refused object is left out and the rest is still read, so a check reports all it can.
"""

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq

from . import identity as ident
from . import jcs, safejson, schemas
from .parquet_limits import ParquetLimitError, check_footer, page_totals
from .report import Report
from .store import StoreError, check_hash, FEED_RE, FINGERPRINT_RE, PARTITION_RE

KNOWN_REQUIRES = frozenset()  # 1.0 defines no features for `requires`
PACK_FORMAT, INDEX_FORMAT, STORE_FORMAT = "jetcarp-pack", "jetcarp-index", "jetcarp-store"


def sh(h):
    """A hash shortened for messages."""
    return h[:19] if isinstance(h, str) else repr(h)


def check_store_marker(store, report):
    """jetcarp-store.json: {"format":"jetcarp-store","version":"1.0"}. Returns False to refuse.
    An export has no marker: its pack.json is the manifest."""
    if store.is_export:
        return True
    try:
        data, doc = store.read_json_file(("jetcarp-store.json",), "jetcarp-store.json")
    except StoreError as e:
        report.fail(0, "store-marker", "jetcarp-store.json", e.message)
        return False
    if not isinstance(doc, dict) or doc.get("format") != STORE_FORMAT:
        report.fail(0, "store-marker", "jetcarp-store.json", 'format is not "jetcarp-store"')
        return False
    ok, known = ident.is_version(doc.get("version"))
    if not ok or not known:
        report.fail(0, "unknown-major", "jetcarp-store.json",
                    f"version {doc.get('version')!r} is not a 1.x version; refusing the store")
        return False
    if not safejson.is_canonical(data, doc):
        report.fail(1, "not-canonical", "jetcarp-store.json", "is not in RFC 8785 canonical form")
    return True


def list_refs(store, report):
    """Every refs/<feed>.json, parsed: {feed: (bytes, document)}. An export has none."""
    out = {}
    if store.is_export:
        return out
    try:
        names = store.list(("refs",))
    except StoreError as e:
        report.fail(0, e.code, "refs/", e.message)
        return out
    for name in names:
        where = f"refs/{name}"
        feed = name[:-5] if name.endswith(".json") else None
        if feed is None or not FEED_RE.match(feed):
            report.warn(0, "ref-name", where, "is not <feed>.json with a feed of 1 to 64 letters, digits and -")
            continue
        try:
            data, doc = store.read_json_file(("refs", name), where)
        except StoreError as e:
            report.fail(0, "ref-unreadable", where, e.message)
            continue
        if not isinstance(doc, dict) or doc.get("format") != "jetcarp-ref":
            report.fail(0, "ref-format", where, 'format is not "jetcarp-ref"')
            continue
        ok, known = ident.is_version(doc.get("version"))
        if not ok or not known:
            report.fail(0, "unknown-major", where, f"version {doc.get('version')!r} is not 1.x; refusing the ref")
            continue
        if doc.get("feed") != feed:
            report.fail(0, "ref-feed", where, f"names feed {doc.get('feed')!r}, not {feed!r}")
            continue
        try:
            check_hash(doc.get("manifest"))
        except StoreError as e:
            report.fail(0, "ref-manifest", where, e.message)
            continue
        out[feed] = (data, doc)
    return out


def read_manifest(store, h, report):
    """A manifest's bytes and document, or (None, None) if it must be refused."""
    where = f"manifest {sh(h)}"
    try:
        data, doc = store.read_json_object(h, "manifest")
    except StoreError as e:
        report.fail(1 if e.code == "object-hash" else 0, e.code, where, e.message)
        return None, None
    report.ok(1, "manifest hashes")
    if not isinstance(doc, dict):
        report.fail(0, "manifest-format", where, "is not a JSON object")
        return None, None
    if doc.get("format") != PACK_FORMAT:
        report.fail(0, "manifest-format", where, f"format is {doc.get('format')!r}, not {PACK_FORMAT!r}")
        return None, None
    ok, known = ident.is_version(doc.get("version"))
    if not ok:
        report.fail(0, "manifest-version", where, f"version {doc.get('version')!r} is not major.minor")
        return None, None
    if not known:
        report.fail(0, "unknown-major", where,
                    f"version {doc['version']} has a major version this reader does not know; refusing the pack")
        return None, None
    requires = doc.get("requires", [])
    if not isinstance(requires, list) or not all(isinstance(r, str) for r in requires):
        report.fail(0, "requires", where, "requires is not a list of strings")
        return None, None
    unknown = [r for r in requires if r not in KNOWN_REQUIRES]
    if unknown:
        report.fail(0, "unknown-requires", where,
                    f"requires {', '.join(map(repr, unknown))}, which this reader does not know; refusing the pack")
        return None, None
    if not safejson.is_canonical(data, doc):
        report.fail(1, "not-canonical", where, "is not in RFC 8785 canonical form, so its hash is not stable")
    return data, doc


class PartData:
    def __init__(self, table, h, entry, index, rows):
        self.table, self.hash, self.entry, self.index, self.rows = table, h, entry, index, rows

    def where(self, row=None):
        w = f"{self.table} part {sh(self.hash)}"
        return w if row is None else f"{w} row {row}"


class TableData:
    def __init__(self, spec, index_hash):
        self.spec, self.index_hash = spec, index_hash
        self.indexes = []
        self.entries = []
        self.parts = []
        self.schema = None

    def rows(self):
        for part in self.parts:
            for n, row in enumerate(part.rows):
                yield part, n, row

    def row_count(self):
        return sum(len(p.rows) for p in self.parts)


class Pack:
    """One version of a pack, read from a store."""

    def __init__(self, store, manifest_hash, report=None, tables=None, cache=None):
        self.store, self.hash = store, manifest_hash
        self.report = report if report is not None else Report()
        self.cache = cache  # shared between versions: an entry read once is not read again
        self.limits = store.limits
        self.tables = {}
        self.data, self.manifest = read_manifest(store, manifest_hash, self.report)
        self.ok = self.manifest is not None
        if not self.ok:
            return
        tables_field = self.manifest.get("tables", {})
        if not isinstance(tables_field, dict):
            self.report.fail(0, "manifest-field", self.where, "tables is not an object")
            tables_field = {}
        for name, index_hash in sorted(tables_field.items()):
            if tables is not None and name not in tables:
                continue
            spec = schemas.TABLES.get(name)
            if spec is None:
                self.report.info(0, "unknown-table", self.where, f"ignoring table {name!r}, which 1.0 does not define")
                continue
            self.tables[name] = self._load_table(spec, index_hash)

    @property
    def where(self):
        return f"manifest {sh(self.hash)}"

    def table(self, name):
        return self.tables.get(name)

    def rows(self, name):
        t = self.tables.get(name)
        return t.rows() if t else iter(())

    # Indexes ------------------------------------------------------------------------------

    def _load_table(self, spec, index_hash):
        data = TableData(spec, index_hash)
        entries = []
        seen_index, seen_parts = set(), set()
        stack = [(index_hash, 1, f"table {spec.name}")]
        while stack:
            h, depth, via = stack.pop()
            where = f"{spec.name} index {sh(h)}"
            if depth > self.limits.index_depth:
                self.report.fail(0, "index-depth", where,
                                 f"is nested {depth} deep, over the limit of {self.limits.index_depth}")
                continue
            if h in seen_index:
                continue
            seen_index.add(h)
            try:
                check_hash(h)
                raw, doc = self.store.read_json_object(h, "index")
            except StoreError as e:
                self.report.fail(1 if e.code == "object-hash" else 0, e.code, where, f"({via}) {e.message}")
                continue
            self.report.ok(1, "index hashes")
            if not self._check_index(doc, raw, spec, where, data):
                continue
            data.indexes.append(h)
            for entry in doc.get("parts", []):
                if not self._check_entry(entry, where):
                    continue
                if entry["object"] in seen_parts:
                    continue
                seen_parts.add(entry["object"])
                entries.append((entry, h))
            for child in reversed(doc.get("children", [])):
                stack.append((child, depth + 1, f"child of {sh(h)}"))
        data.entries = [e for e, _ in entries]  # in index order, children after their parent's parts
        for entry, h in entries:
            key = (spec.name, jcs.canonical(entry))
            if self.cache is not None and key in self.cache:
                rows = self.cache[key]
            else:
                rows = self._read_part(spec, entry, h)
                if self.cache is not None:
                    self.cache[key] = rows
            if rows is not None:
                data.parts.append(PartData(spec.name, entry["object"], entry, h, rows))
        return data

    def _check_index(self, doc, raw, spec, where, data):
        if not isinstance(doc, dict) or doc.get("format") != INDEX_FORMAT:
            self.report.fail(0, "index-format", where, f'format is not "{INDEX_FORMAT}"')
            return False
        ok, known = ident.is_version(doc.get("version"))
        if not ok or not known:
            self.report.fail(0, "unknown-major", where, f"version {doc.get('version')!r} is not 1.x; refusing the index")
            return False
        if doc.get("table") != spec.name:
            self.report.fail(0, "index-table", where, f"is for table {doc.get('table')!r}, not {spec.name!r}")
            return False
        schema = doc.get("schema")
        if not isinstance(schema, str) or not schema.startswith(f"jetcarp-{spec.name} "):
            self.report.fail(0, "index-schema", where, f"schema {schema!r} is not jetcarp-{spec.name} <n>")
            return False
        if schema != spec.schema_id:
            self.report.fail(0, "unknown-schema", where,
                             f"schema {schema!r} has a number this reader does not know; refusing the table")
            return False
        if data.schema is None:
            data.schema = schema
        parts, children = doc.get("parts", []), doc.get("children", [])
        if not isinstance(parts, list) or not isinstance(children, list):
            self.report.fail(0, "index-format", where, "parts and children must be lists")
            return False
        if len(parts) > self.limits.parts_per_index:
            self.report.fail(0, "index-parts", where,
                             f"lists {len(parts):,} parts, over the limit of {self.limits.parts_per_index:,}; refused")
            return False
        if len(children) > self.limits.parts_per_index:
            self.report.fail(0, "index-children", where,
                             f"lists {len(children):,} children, over the limit of {self.limits.parts_per_index:,}; refused")
            return False
        for c in children:
            try:
                check_hash(c)
            except StoreError as e:
                self.report.fail(0, e.code, where, f"child: {e.message}")
                return False
        compacts = doc.get("compacts")
        if compacts is not None and not isinstance(compacts, list):
            self.report.fail(0, "index-format", where, "compacts is not a list")
            return False
        if compacts is not None:
            self.report.info(0, "index-compacts", where, f"compacts {len(compacts)} earlier parts (reserved; ignored)")
        if not safejson.is_canonical(raw, doc):
            self.report.fail(1, "not-canonical", where, "is not in RFC 8785 canonical form")
        return True

    def _check_entry(self, entry, where):
        if not isinstance(entry, dict):
            self.report.fail(0, "index-entry", where, "a part entry is not an object")
            return False
        try:
            check_hash(entry.get("object"))
        except StoreError as e:
            self.report.fail(0, e.code, where, f"part entry: {e.message}")
            return False
        for name in ("rows", "bytes"):
            v = entry.get(name)
            if not isinstance(v, int) or isinstance(v, bool) or v < 0:
                self.report.fail(0, "index-entry", where, f"part {sh(entry['object'])}: {name} is not a count")
                return False
        if entry["bytes"] > self.limits.object_bytes:
            self.report.fail(0, "too-large", where, f"part {sh(entry['object'])} is {entry['bytes']:,} bytes, "
                                                    f"over the limit of {self.limits.object_bytes:,}; refused")
            return False
        part = entry.get("partition", {})
        if not isinstance(part, dict) or not all(isinstance(v, str) for v in part.values()):
            self.report.fail(0, "index-entry", where, f"part {sh(entry['object'])}: partition is not an object of strings")
            return False
        bad = [v for v in part.values() if not PARTITION_RE.match(v)]
        if bad:
            self.report.fail(0, "partition-value", where, f"part {sh(entry['object'])}: partition value {bad[0]!r} is not "
                                                          "1 to 64 lower-case letters, digits, - and _; refused")
            return False
        for name in ("first", "last"):
            if not isinstance(entry.get(name, ""), str):
                self.report.fail(0, "index-entry", where, f"part {sh(entry['object'])}: {name} is not a string")
                return False
        return True

    # Parts --------------------------------------------------------------------------------

    def _read_part(self, spec, entry, index_hash):
        where = f"{spec.name} part {sh(entry['object'])}"
        rows = self.read_parquet(spec, entry["object"], where, entry)
        if rows is None:
            return None
        self._check_part_rows(spec, entry, rows, where)
        return rows

    def read_parquet(self, spec, h, where, entry=None, counted="part hashes"):
        """The rows of a Parquet object, read under every limit; None if refused. With `entry` (its
        index entry), its size and row count are checked against it."""
        lim = self.limits
        try:
            f, size = self.store.open_object(h, "parquet", lim.object_bytes)
        except StoreError as e:
            self.report.fail(1 if e.code == "object-hash" else 0, e.code, where, e.message)
            return None
        self.report.ok(1, counted)
        with f:
            if entry is not None and size != entry["bytes"]:
                self.report.fail(1, "part-bytes", where, f"is {size:,} bytes; its index says {entry['bytes']:,}")
            try:
                check_footer(f, size)
            except ParquetLimitError as e:
                self.report.fail(0, "parquet-footer", where, f"{e}; refused")
                return None
            try:
                pf = pq.ParquetFile(f, thrift_string_size_limit=lim.document_bytes,
                                    thrift_container_size_limit=1_000_000)
                md = pf.metadata
                arrow_schema = pf.schema_arrow
            except Exception as e:  # pyarrow raises many kinds; none may crash the reader
                self.report.fail(0, "parquet-unreadable", where, f"{type(e).__name__}: {str(e)[:200]}")
                return None
            if not self._check_parquet_meta(f, md, size, entry, where):
                return None
            columns = self._check_columns(spec, arrow_schema, where)
            if columns is None:
                return None
            return self._decode(pf, spec, columns, size, where)

    def _check_parquet_meta(self, f, md, size, entry, where):
        lim = self.limits
        if entry is not None and md.num_rows != entry["rows"]:
            self.report.fail(1, "part-rows", where, f"holds {md.num_rows:,} rows; its index says {entry['rows']:,}")
        try:
            uncompressed, values = page_totals(f, md)
        except ParquetLimitError as e:
            self.report.fail(0, "parquet-pages", where, f"{e}; refused")
            return False
        except OSError as e:
            self.report.fail(0, "parquet-unreadable", where, str(e)[:200])
            return False
        if uncompressed > lim.decompression_ratio * size:
            self.report.fail(0, "decompression-limit", where,
                             f"its pages decompress to {uncompressed:,} bytes, over {lim.decompression_ratio}x its "
                             f"{size:,} stored bytes; refused")
            return False
        if values > lim.decompression_ratio * size + (1 << 20):
            self.report.fail(0, "values-limit", where,
                             f"its data pages hold {values:,} values, over {lim.decompression_ratio} per byte of the "
                             "part plus 2^20; refused")
            return False
        codecs, no_stats, no_index = set(), set(), set()
        for g in range(md.num_row_groups):
            rg = md.row_group(g)
            for c in range(rg.num_columns):
                col = rg.column(c)
                if col.compression != "ZSTD":
                    codecs.add(col.compression)
                if not col.is_stats_set:
                    no_stats.add(col.path_in_schema)
                if not (col.has_column_index and col.has_offset_index):
                    no_index.add(col.path_in_schema)
        if codecs:
            self.report.warn(1, "parquet-compression", where, f"uses {sorted(codecs)}, not zstd")
        if no_stats:
            self.report.warn(1, "parquet-statistics", where, f"has no statistics for {sorted(no_stats)[:5]}")
        if no_index:
            self.report.warn(1, "parquet-page-index", where, f"has no page index for {sorted(no_index)[:5]}")
        return True

    def _check_columns(self, spec, arrow_schema, where):
        names = set(arrow_schema.names)
        present = []
        for col in spec.columns:
            if col.name not in names:
                if col.required:
                    self.report.fail(0, "schema-column", where, f"has no column {col.name!r}; refused")
                    return None
                continue  # a nullable column left out is read as null
            problems = schemas.type_problems(col.type, arrow_schema.field(col.name).type, col.name)
            if problems:
                self.report.fail(0, "schema-type", where, "; ".join(problems[:3]) + "; refused")
                return None
            present.append(col.name)
        unknown = sorted(names - {c.name for c in spec.columns})
        if unknown:
            self.report.info(0, "unknown-column", where, f"ignoring columns {unknown}")
        return present

    def _decode(self, pf, spec, columns, size, where):
        lim = self.limits
        budget = lim.decompression_ratio * size + lim.decoded_slack
        decoded = 0
        rows = []
        try:
            for batch in pf.iter_batches(batch_size=lim.batch_rows, columns=columns, use_threads=False):
                decoded += batch.nbytes
                if decoded > budget:
                    self.report.fail(0, "decoded-limit", where,
                                     f"decodes to over {budget:,} bytes ({lim.decompression_ratio}x its stored size "
                                     f"plus {lim.decoded_slack:,}); refused")
                    return None
                for name in columns:
                    arr = batch.column(name)
                    longest = _longest_string(arr)
                    if longest > lim.string_cell:
                        self.report.fail(0, "string-limit", where,
                                         f"column {name} has a string of {longest:,} bytes, over the limit of "
                                         f"{lim.string_cell:,}; refused")
                        return None
                    col = spec.column(name)
                    if not col.nullable and arr.null_count:
                        first = pc.index(pc.is_null(arr), True).as_py()
                        self.report.fail(0, "null", where, f"column {name} is null in row {len(rows) + first}, "
                                                           "but may not be; refused")
                        return None
                    if name != "site" and _nested_nulls(arr):
                        self.report.fail(0, "null", where, f"column {name} holds a null inside a list or "
                                                           "struct; refused")
                        return None
                if spec.name == "patterns" and "phases" in columns:
                    inner = pc.list_flatten(batch.column("phases"))
                    if len(inner):
                        most = pc.max(pc.list_value_length(inner)).as_py() or 0
                        if most > lim.phase_cells:
                            self.report.fail(0, "phase-limit", where,
                                             f"a phase has {most:,} cells, over the limit of {lim.phase_cells:,}; refused")
                            return None
                for name in columns:
                    if self.spec_int32(spec, name):
                        arr = batch.column(name)
                        if len(arr) and arr.null_count < len(arr) and not pa.types.is_int32(arr.type):
                            lo, hi = pc.min_max(arr).values()
                            if lo.as_py() < -2 ** 31 or hi.as_py() >= 2 ** 31:
                                self.report.fail(0, "schema-type", where, f"column {name} holds values that do not "
                                                                          "fit int32; refused")
                                return None
                missing = [c.name for c in spec.columns if c.name not in columns]
                for row in batch.to_pylist():
                    for name in missing:
                        row[name] = None
                    rows.append(row)
        except MemoryError:
            self.report.fail(0, "decoded-limit", where, "ran out of memory while decoding; refused")
            return None
        except Exception as e:
            self.report.fail(0, "parquet-unreadable", where, f"{type(e).__name__}: {str(e)[:200]}")
            return None
        return rows

    @staticmethod
    def spec_int32(spec, name):
        col = spec.column(name)
        return col is not None and pa.types.is_int32(col.type)

    def _check_part_rows(self, spec, entry, rows, where):
        # Sorted, by UTF-8 bytes for text (sections 8 and 23.4), within the part.
        def key(row):
            return tuple(_sort_value(row.get(c)) for c in spec.sort)
        prev = None
        for n, row in enumerate(rows):
            k = key(row)
            if prev is not None and k < prev:
                self.report.fail(1, "part-sort", where, f"row {n} sorts before row {n - 1} by {', '.join(spec.sort)}")
                break
            prev = k
        # first and last: the first sort column's value in the first and last row, as text.
        if rows:
            first, last = _key_text(rows[0].get(spec.sort[0])), _key_text(rows[-1].get(spec.sort[0]))
        else:
            first = last = ""
        for name, value in (("first", first), ("last", last)):
            if entry.get(name, "") != value:
                self.report.fail(1, "part-range", where, f"its index says {name} is {entry.get(name)!r}; "
                                                         f"the part's is {value!r}")
        # Partition values hold for every row.
        part = entry.get("partition", {})
        if set(part) != set(spec.partition):
            self.report.fail(1, "part-partition", where,
                             f"partition has keys {sorted(part)}; table {spec.name} is partitioned by "
                             f"{', '.join(spec.partition) or 'nothing'}")
        else:
            for column in spec.partition:
                bad = [n for n, r in enumerate(rows) if r.get(column) != part[column]]
                if bad:
                    self.report.fail(1, "part-partition", where,
                                     f"row {bad[0]} has {column} {rows[bad[0]].get(column)!r}, but the part is "
                                     f"partition {column}={part[column]!r}")


def _sort_value(v):
    if isinstance(v, str):
        return (1, v.encode("utf-8", "surrogatepass"))
    if isinstance(v, bool) or v is None:
        return (0, int(bool(v)))  # false before true
    if isinstance(v, (int, float)):
        return (0, v)
    return (2, repr(v).encode())


def _key_text(v):
    return v if isinstance(v, str) else ("" if v is None else str(v))


def _nested_nulls(arr):
    """Whether a list or struct column holds a null below its top level."""
    t = arr.type
    if pa.types.is_list(t) or pa.types.is_large_list(t):
        values = arr.flatten()
        return values.null_count > 0 or _nested_nulls(values)
    if pa.types.is_struct(t):
        return any(f.null_count > 0 or _nested_nulls(f) for f in arr.flatten())
    return False


def _longest_string(arr):
    t = arr.type
    if pa.types.is_string(t) or pa.types.is_large_string(t) or pa.types.is_binary(t) or pa.types.is_large_binary(t):
        if len(arr) == 0 or arr.null_count == len(arr):
            return 0
        return pc.max(pc.binary_length(arr)).as_py() or 0
    if pa.types.is_list(t) or pa.types.is_large_list(t):
        return _longest_string(arr.flatten())
    if pa.types.is_struct(t):
        return max((_longest_string(f) for f in arr.flatten()), default=0)
    return 0


def open_pack(store, report, manifest=None, feed=None):
    """Pick a manifest: the one given, an export's pack.json, the feed's ref, or the only ref."""
    if manifest:
        return manifest
    if store.is_export:
        return store.manifest_hash
    refs = list_refs(store, report)
    if feed:
        if feed not in refs:
            report.fail(0, "no-ref", f"refs/{feed}.json", "no such ref")
            return None
        return refs[feed][1]["manifest"]
    if len(refs) == 1:
        return next(iter(refs.values()))[1]["manifest"]
    if not refs:
        report.fail(0, "no-ref", "refs/", "the store has no refs; name a manifest with --manifest")
    else:
        report.fail(0, "no-ref", "refs/", f"the store has {len(refs)} feeds ({', '.join(sorted(refs))}); "
                                          "choose one with --feed or --manifest")
    return None


def check_fingerprint(value):
    return isinstance(value, str) and bool(FINGERPRINT_RE.match(value))
