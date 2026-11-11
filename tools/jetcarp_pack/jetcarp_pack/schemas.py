"""The tables of pack 1.0 (pack spec section 11): columns, types, sort keys and partitions.

Types follow the spec's `name: type` lists; `?` there is `nullable=True` here. Where the spec leaves
something out, the choice is marked "(choice)" and listed in SPEC_QUESTIONS.md.
"""

from dataclasses import dataclass, field

import pyarrow as pa

S, I32, I64, B, F64 = pa.string(), pa.int32(), pa.int64(), pa.bool_(), pa.float64()

OFFSET = pa.struct([("di", I32), ("dj", I32), ("k", I32)])
CELL = pa.struct([("i", I32), ("j", I32), ("k", I32), ("s", I32)])
TILE_TYPE = pa.struct([("shape", I32), ("corners", pa.list_(pa.list_(F64)))])
SITE = pa.struct([
    ("patch", pa.struct([("family", S), ("params", S), ("level", I32)])),
    ("radius", I32),
    ("nb", S),
    ("tiles", pa.list_(pa.struct([("shape", I32), ("dist", I32), ("point", pa.list_(F64)),
                                  ("corners", pa.list_(pa.list_(F64)))]))),
    ("graph", pa.struct([
        ("degrees", B), ("chirality", B),
        ("cells", pa.list_(pa.struct([("shape", I32), ("flip", B), ("degree", I32),
                                      ("states", pa.list_(I32)), ("neighbours", pa.list_(I32))]))),
        ("around", pa.list_(pa.struct([("tiles", pa.list_(I32)), ("count", I32)]))),
    ])),
])


@dataclass
class Column:
    name: str
    type: pa.DataType
    nullable: bool = False
    required: bool = True  # False: a column the reader knows but a pack may leave out


@dataclass
class Table:
    name: str
    columns: list
    sort: tuple  # sort key columns, compared by UTF-8 bytes for text (section 23.4)
    kind: str  # "fact" (fixed by its key, duplicates must be identical) or "event"
    key: tuple = ()  # columns that identify a fact row
    partition: dict = field(default_factory=dict)  # partition key in an index entry -> column
    optional: bool = False  # one of the optional tables

    @property
    def schema_id(self):
        return f"jetcarp-{self.name} 1"

    def column(self, name):
        for c in self.columns:
            if c.name == name:
                return c
        return None

    def arrow_schema(self, include_optional=True):
        return pa.schema([pa.field(c.name, c.type, nullable=c.nullable)
                          for c in self.columns if include_optional or c.required])


def _c(name, type_, nullable=False, required=True):
    return Column(name, type_, nullable, required)


TABLES = {t.name: t for t in [
    Table("tilings", [
        _c("pin_key", S), _c("pin", S), _c("name", S), _c("kind", S),
        _c("a1", pa.list_(F64), True), _c("a2", pa.list_(F64), True),
        _c("tile_types", pa.list_(TILE_TYPE), True),
        _c("edge", pa.list_(pa.list_(OFFSET)), True),
        _c("touch", pa.list_(pa.list_(OFFSET)), True),
        _c("family", S), _c("params", S),
        _c("reference_level", I32, True),
    ], sort=("pin_key",), kind="fact", key=("pin_key",)),
    Table("rules", [
        _c("notation", S), _c("neighbourhood", S), _c("steps_per_gen", I32),
        _c("family", S), _c("semantics", S), _c("states", I32),
        _c("birth", I64, True), _c("survive", I64, True),
    ], sort=("notation", "neighbourhood", "steps_per_gen"), kind="fact",
        key=("notation", "neighbourhood", "steps_per_gen")),
    Table("patterns", [
        _c("id", S), _c("identity", S), _c("pin_key", S), _c("pin", S),
        _c("model", S), _c("neighbourhood", S), _c("steps_per_gen", I32),
        _c("class", S), _c("period", I64), _c("dx", I32), _c("dy", I32),
        _c("speed", S, True),
        _c("cells", I32), _c("min_cells", I32), _c("max_cells", I32),
        _c("phases", pa.list_(pa.list_(CELL))),
        _c("canonical", S), _c("rule", S, True), _c("site", SITE, True), _c("gun", S, True),
        _c("class_group", S), _c("tiling_group", S),
        # Section 10 says every record carries its family and that family's semantics version;
        # section 11's column list leaves them out (choice: optional, nullable).
        _c("family", S, True, required=False), _c("semantics", S, True, required=False),
    ], sort=("id",), kind="fact", key=("id",),
        partition={"class": "class_group", "tiling_group": "tiling_group"}),
    Table("verifications", [
        _c("pattern_id", S), _c("rule", S), _c("publisher", S),
        _c("periods", I64), _c("generations", I64), _c("field", S),
        _c("engine_version", S), _c("pin", S), _c("semantics", S), _c("date", S),
        _c("range", S, True),
    ], sort=("pattern_id", "rule", "publisher"), kind="event"),
    Table("sightings", [
        _c("pattern_id", S), _c("rule", S), _c("publisher", S), _c("source", S),
        _c("count", I64), _c("soups", I64), _c("first_seen", S), _c("last_seen", S),
    ], sort=("pattern_id", "rule", "publisher", "source"), kind="event"),
    Table("provenance", [
        _c("target", S), _c("publisher", S), _c("seq", I32), _c("kind", S),
        _c("finder", S, True), _c("orcid", S, True),
        _c("search_kind", S, True), _c("search", S, True),
        _c("engine_version", S), _c("pin", S), _c("date", S), _c("source", S, True),
    ], sort=("target", "publisher", "seq"), kind="event"),
    Table("annotations", [
        _c("target", S), _c("publisher", S), _c("field", S), _c("value", S),
        _c("score_model", S, True), _c("date", S),
    ], sort=("target", "publisher", "field", "date"), kind="event"),
    Table("ants", [
        _c("id", S), _c("identity", S), _c("pin_key", S), _c("pin", S),
        _c("rule", S), _c("kind", S), _c("start", S), _c("outcome", S),
        _c("period", I64, True), _c("dx", I32, True), _c("dy", I32, True), _c("onset", I64, True),
        _c("steps", I64), _c("cells", I64), _c("tiling_group", S),
        _c("family", S, True, required=False), _c("semantics", S, True, required=False),
    ], sort=("id",), kind="fact", key=("id",),
        # Section 8.2 partitions ants by class too, but the table has no class column (choice).
        partition={"tiling_group": "tiling_group"}),
    Table("quickstart", [
        _c("id", S), _c("title", S), _c("description", S), _c("source", S, True),
        _c("scene", S), _c("scene_format", S),
        _c("rule_family", S), _c("semantics", S), _c("pin_key", S, True), _c("pin", S, True),
        _c("tiling_group", S),
    ], sort=("id",), kind="fact", key=("id",), partition={"tiling_group": "tiling_group"}),
    Table("media", [
        _c("target", S), _c("kind", S), _c("from_seed", B), _c("object", S),
        _c("media_type", S), _c("width", I32), _c("height", I32), _c("frames", I32),
        _c("render_key", S), _c("render", S),
    ], sort=("target", "kind"), kind="event"),
    Table("aliases", [
        _c("old_id", S), _c("target", S), _c("reason", S), _c("date", S),
    ], sort=("old_id",), kind="event", optional=True),  # sort order (choice)
    Table("ratings", [
        _c("rater", S), _c("target", S), _c("scale", S), _c("verdict", S),
        _c("note", S, True), _c("round", S, True), _c("date", S),
    ], sort=("rater", "target", "date"), kind="event", optional=True),
    Table("raters", [
        # Section 11 names the columns but not their types (choice: strings).
        _c("rater", S), _c("consent_date", S), _c("consent_scope", S),
        _c("display_name", S, True), _c("orcid", S, True),
        # Section 25 item 4: a group rater lists its members.
        _c("members", pa.list_(S), True, required=False),
    ], sort=("rater",), kind="fact", key=("rater",), optional=True),  # sort order (choice)
    Table("score_models", [
        _c("model", S), _c("version", S), _c("kind", S), _c("features", pa.list_(S)),
        _c("weights", pa.list_(F64)), _c("intercept", F64), _c("fitted_on", S),
        _c("rater", S, True), _c("metrics", S),
    ], sort=("model", "version"), kind="fact", key=("model", "version"), optional=True),
]}

CLASSES = ("still_life", "oscillator", "spaceship", "puffer", "mover", "gun", "other")
PERIODIC_CLASSES = ("still_life", "oscillator", "spaceship")
ANT_OUTCOMES = ("highway", "periodic", "symmetric", "bounded", "chaotic")
PROVENANCE_KINDS = ("found", "imported", "reverified")
ANNOTATION_FIELDS = ("name", "label", "tags", "notes", "state", "rating", "score")
MEDIA_KINDS = ("gif", "webp", "poster", "thumb")
MEDIA_TYPES = {"image/png": "png", "image/gif": "gif", "image/webp": "webp"}
ALIAS_REASONS = ("geometry-change", "scheme-change")
RULE_FAMILIES_WITH_MASKS = ("lifelike", "generations")


def normalise_type(t):
    """A type with list field names and string/list widths ignored, for comparing schemas."""
    if pa.types.is_large_string(t) or (hasattr(pa.types, "is_string_view") and pa.types.is_string_view(t)):
        return pa.string()
    if pa.types.is_list(t) or pa.types.is_large_list(t):
        return pa.list_(normalise_type(t.value_type))
    if pa.types.is_struct(t):
        return pa.struct([pa.field(t.field(i).name, normalise_type(t.field(i).type))
                          for i in range(t.num_fields)])
    return t


def type_problems(expected, actual, path):
    """How `actual` differs from `expected`; extra struct fields are allowed (a minor change)."""
    expected, actual = normalise_type(expected), normalise_type(actual)
    if pa.types.is_struct(expected):
        if not pa.types.is_struct(actual):
            return [f"{path} is {actual}, not a struct"]
        names = {actual.field(i).name: actual.field(i).type for i in range(actual.num_fields)}
        out = []
        for i in range(expected.num_fields):
            f = expected.field(i)
            if f.name not in names:
                out.append(f"{path}.{f.name} is missing")
            else:
                out += type_problems(f.type, names[f.name], f"{path}.{f.name}")
        return out
    if pa.types.is_list(expected):
        if not pa.types.is_list(actual):
            return [f"{path} is {actual}, not a list"]
        return type_problems(expected.value_type, actual.value_type, f"{path}[]")
    if expected != actual:
        return [f"{path} is {actual}, not {expected}"]
    return []
