"""Identity texts and content ids (pack spec section 9, library-format.md), and the value formats
that pack tables and documents use."""

import datetime
import hashlib
import json
import re

from . import jcs

PATTERN_SCHEME = "jetcarp-pattern-v3"
ANT_SCHEME = "jetcarp-ant-v1"
GEOMETRY_SCHEME = "jetcarp-geometry-v1"
PATTERN_FIELDS = ("model", "nb", "steps", "tiling", "class", "period", "d", "rule", "canonical", "phases")
ANT_FIELDS = ("rule", "tiling", "start")

HEX32 = re.compile(r"[0-9a-f]{32}\Z")
ANT_ID = re.compile(r"ant-[0-9a-f]{32}\Z")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")
TIME = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\Z")
ORCID = re.compile(r"\d{4}-\d{4}-\d{4}-\d{3}[\dX]\Z")
POSITIVE = re.compile(r"[1-9][0-9]{0,17}\Z")
VERSION = re.compile(r"(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\Z")
SPDX = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+-]{0,63}\Z")
QUICKSTART_ID = re.compile(r"[A-Za-z0-9._-]{1,200}\Z")
_CELL = re.compile(r"(-?\d{1,10}),(-?\d{1,10}),(-?\d{1,10}),(-?\d{1,10})\Z")
_LOCAL_PATH = re.compile(r"(^|[\s'\"=(])(/home/|/Users/|/tmp/|/var/|/root/|~/|[A-Za-z]:\\\\?[A-Za-z])")


class IdentityError(ValueError):
    pass


def content_hash(identity, pin):
    """First 32 hex digits of SHA-256 over identity + "\\n" + pin (section 9)."""
    return hashlib.sha256((identity + "\n" + pin).encode("utf-8")).hexdigest()[:32]


def pattern_id(identity, pin):
    return content_hash(identity, pin)


def ant_id(identity, pin):
    return "ant-" + content_hash(identity, pin)


def parse_identity(text, scheme, fields):
    lines = text.split("\n")
    if lines[0] != scheme:
        raise IdentityError(f"first line is {lines[0][:60]!r}, not {scheme!r}")
    if len(lines) != len(fields) + 1:
        raise IdentityError(f"has {len(lines)} lines, not {len(fields) + 1}")
    out = {}
    for name, line in zip(fields, lines[1:]):
        if not line.startswith(name + "="):
            raise IdentityError(f"line {line[:40]!r} should start with {name}=")
        out[name] = line[len(name) + 1:]
    return out


def parse_pattern_identity(text):
    return parse_identity(text, PATTERN_SCHEME, PATTERN_FIELDS)


def parse_ant_identity(text):
    return parse_identity(text, ANT_SCHEME, ANT_FIELDS)


def cell_key(c):
    """Phases are sorted by row j, then column i, tile type k and state s."""
    return (c["j"], c["i"], c["k"], c["s"])


def format_phases(phases):
    return ";".join(" ".join(f"{c['i']},{c['j']},{c['k']},{c['s']}" for c in phase) for phase in phases)


def parse_phases(text, max_cells):
    phases = []
    for part in text.split(";"):
        cells = []
        if part:
            items = part.split(" ")
            if len(items) > max_cells:
                raise IdentityError(f"a phase has {len(items):,} cells, over the limit of {max_cells:,}")
            for item in items:
                m = _CELL.match(item)
                if not m:
                    raise IdentityError(f"{item[:40]!r} is not a cell i,j,k,s")
                i, j, k, s = (int(g) for g in m.groups())
                cells.append({"i": i, "j": j, "k": k, "s": s})
        phases.append(cells)
    return phases


def format_pattern_identity(*, model, nb, steps, tiling, cls, period, d, rule, canonical, phases):
    lines = [PATTERN_SCHEME, f"model={model}", f"nb={nb}", f"steps={steps}", f"tiling={tiling}",
             f"class={cls}", f"period={period}", f"d={d[0]},{d[1]}", f"rule={rule}",
             f"canonical={canonical}", "phases=" + format_phases(phases)]
    return "\n".join(lines)


def format_ant_identity(*, rule, tiling, start):
    return "\n".join([ANT_SCHEME, f"rule={rule}", f"tiling={tiling}", f"start={start}"])


def picture(phase):
    """The canonical picture of a phase on a one-tile lattice, as library-format.md shows it
    (`ooo/o/.o`): rows by j from the top of the bounding box, `o` for a live cell, `.` for a dead
    one, each row cut after its last live cell. Inferred from the book's one example: states above
    1 and empty rows are not covered there, so this returns None for them."""
    if not phase or any(c["s"] != 1 or c["k"] != 0 for c in phase):
        return None
    i0 = min(c["i"] for c in phase)
    j0 = min(c["j"] for c in phase)
    rows = {}
    for c in phase:
        rows.setdefault(c["j"] - j0, set()).add(c["i"] - i0)
    if len(rows) != max(rows) + 1:
        return None
    return "/".join("".join("o" if i in rows[j] else "." for i in range(max(rows[j]) + 1))
                    for j in range(len(rows)))


def check_tiling_key(text):
    """Check that `text` is a tiling key in its one spelling; returns the parsed object."""
    from . import safejson
    value = safejson.loads(text, max_bytes=1 << 20, max_depth=32, allow_floats=True, what="tiling key")
    if not isinstance(value, dict) or not isinstance(value.get("family"), str):
        raise IdentityError("is not an object with a family")
    if value.get("boundary") != "dead":
        raise IdentityError('does not end with "boundary":"dead"')
    extra = set(value) - {"family", "params", "edges", "boundary"}
    if extra:
        raise IdentityError(f"has members {sorted(extra)} beyond family, params, edges, boundary")
    if value["family"] in jcs.APERIODIC_FAMILIES and isinstance(value.get("params"), dict) \
            and "level" in value["params"]:
        raise IdentityError("an aperiodic tiling's key must leave out its level")
    try:
        expected = jcs.tiling_key(value)
    except jcs.CanonicalError as e:
        raise IdentityError(str(e)) from None
    if expected != text:
        raise IdentityError(f"is not in its one spelling, which is {expected[:200]}")
    return value


def tiling_group(key):
    """The partition group of a tiling (section 8.2), from its parsed tiling key. Isohedral
    families are taken to be named `ih<n>` (from library-format.md's IH58; choice)."""
    family = key["family"]
    if family == "periodic":
        catalog = (key.get("params") or {}).get("catalog")
        m = re.match(r"(k[1-6])-", catalog) if isinstance(catalog, str) else None
        return m.group(1) if m else None
    if re.fullmatch(r"ih\d+", family):
        return "isohedral"
    return family


def is_time(value):
    if not isinstance(value, str) or not TIME.match(value):
        return False
    try:
        datetime.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
        return True
    except ValueError:
        return False


def is_orcid(value):
    if not isinstance(value, str) or not ORCID.match(value):
        return False
    digits = value.replace("-", "")
    total = 0
    for ch in digits[:-1]:
        total = (total + int(ch)) * 2
    check = (12 - total % 11) % 11
    return digits[-1] == ("X" if check == 10 else str(check))


def is_version(value, major=1):
    """A `major.minor` version string; returns (ok, known_major)."""
    m = VERSION.match(value) if isinstance(value, str) else None
    if not m:
        return False, False
    return True, int(m.group(1)) == major


def looks_like_local_path(text):
    return bool(_LOCAL_PATH.search(text))


def target_kind(target):
    """Split a record target such as `pattern:<id>`; returns (kind, id) or None if malformed."""
    if not isinstance(target, str):
        return None
    kind, sep, rest = target.partition(":")
    if not sep:
        return None
    if kind == "pattern" and HEX32.match(rest):
        return kind, rest
    if kind == "ant" and ANT_ID.match(rest):
        return kind, rest
    if kind == "quickstart" and QUICKSTART_ID.match(rest):
        return kind, rest
    if kind == "search_run":
        uuid, sep, run = rest.partition(":")
        if sep and UUID.match(uuid) and run and len(run) <= 100:
            return kind, rest
    return None


def json_cell(text, what):
    """A JSON value held in a table cell (floats allowed: cells are not pack documents)."""
    from . import safejson
    return safejson.loads(text, max_bytes=1 << 20, max_depth=32, allow_floats=True, what=what)


def short(value, n=60):
    s = value if isinstance(value, str) else json.dumps(value)
    return s if len(s) <= n else s[:n] + "..."
