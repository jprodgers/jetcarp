"""RFC 8785, the JSON Canonicalization Scheme (JCS), written from the RFC.

Pack documents (manifests, indexes, refs, key statements, receipts) hold only integers, strings,
booleans, null, arrays and objects (pack-format.md, "Encodings"), so `canonical()` refuses other numbers
unless `allow_floats` is set. Numbers are still written exactly as RFC 8785 says, because the
identity's `tiling=` line uses RFC 8785 values whose parameters may be floats (pack-format.md "Ids",
library-format.md).

An integer outside +-(2^53 - 1) is refused everywhere, floats allowed or not: a double cannot hold it
exactly, and a pack writes such an integer as a string of its decimal digits (pack-format.md,
"Encodings"; I-JSON, RFC 7493), as the `rfc8785` package refuses it too.
"""

import math
import re

MAX_SAFE_INTEGER = 2 ** 53 - 1

_ESCAPES = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}
_NEEDS_ESCAPE = re.compile('["\\\\\x00-\x1f]')
_SURROGATE = re.compile("[\ud800-\udfff]")


class CanonicalError(ValueError):
    """A value that has no RFC 8785 form (or none allowed in a pack document)."""


def canonical(value, *, allow_floats=False):
    """The RFC 8785 bytes of `value` (UTF-8)."""
    return dumps(value, allow_floats=allow_floats).encode("utf-8")


def dumps(value, *, allow_floats=False):
    """The RFC 8785 text of `value`."""
    out = []
    _write(value, out, allow_floats, 0)
    return "".join(out)


def _write(value, out, allow_floats, depth):
    if depth > 64:
        raise CanonicalError("nested more than 64 levels")
    if value is None:
        out.append("null")
    elif value is True:
        out.append("true")
    elif value is False:
        out.append("false")
    elif isinstance(value, str):
        out.append(string(value))
    elif isinstance(value, int):
        if abs(value) > MAX_SAFE_INTEGER:
            raise CanonicalError(f"integer {value} is outside +-(2^53 - 1) (such an integer is a string)")
        out.append(str(value))
    elif isinstance(value, float):
        if not allow_floats:
            raise CanonicalError(f"non-integer number {value!r} in a pack document")
        out.append(number(value))
    elif isinstance(value, (list, tuple)):
        out.append("[")
        for n, item in enumerate(value):
            if n:
                out.append(",")
            _write(item, out, allow_floats, depth + 1)
        out.append("]")
    elif isinstance(value, dict):
        for key in value:
            if not isinstance(key, str):
                raise CanonicalError(f"object key {key!r} is not a string")
        out.append("{")
        for n, key in enumerate(sorted(value, key=utf16_key)):
            if n:
                out.append(",")
            out.append(string(key))
            out.append(":")
            _write(value[key], out, allow_floats, depth + 1)
        out.append("}")
    else:
        raise CanonicalError(f"{type(value).__name__} has no JSON form")


def utf16_key(text):
    """Sort key: a string's UTF-16 code units (RFC 8785 section 3.2.3)."""
    return text.encode("utf-16-be", "surrogatepass")


def string(text):
    """A JSON string as RFC 8785 (ECMAScript's JSON.stringify) writes it."""
    if _SURROGATE.search(text):
        raise CanonicalError("string holds a lone surrogate, which is not valid I-JSON")
    return '"' + _NEEDS_ESCAPE.sub(_escape, text) + '"'


def _escape(match):
    ch = match.group(0)
    return _ESCAPES.get(ch) or "\\u%04x" % ord(ch)


def number(x):
    """A double as ECMAScript's Number.prototype.toString writes it (RFC 8785 section 3.2.2.3).

    Python's repr() gives the shortest digits that read back as the same double, choosing the
    closest when several are as short; that is the digit string ECMAScript specifies. Only the
    layout (where the point goes, when to use an exponent) differs, and is redone here.
    """
    if isinstance(x, bool) or not isinstance(x, (int, float)):
        raise CanonicalError(f"{x!r} is not a number")
    x = float(x)
    if math.isnan(x) or math.isinf(x):
        raise CanonicalError(f"{x!r} has no JSON form")
    if x == 0:
        return "0"  # minus zero too
    sign = "-" if x < 0 else ""
    text = repr(abs(x))
    mantissa, _, exp = text.partition("e")
    exponent = int(exp) if exp else 0
    whole, _, fraction = mantissa.partition(".")
    raw = whole + fraction
    significant = raw.lstrip("0")
    digits = significant.rstrip("0")
    trailing = len(significant) - len(digits)
    k = len(digits)
    # value = digits x 10^(n - k)
    n = exponent - len(fraction) + trailing + k
    if k <= n <= 21:
        return sign + digits + "0" * (n - k)
    if 0 < n <= 21:
        return sign + digits[:n] + "." + digits[n:]
    if -6 < n <= 0:
        return sign + "0." + "0" * (-n) + digits
    e = n - 1
    exp_text = ("+" if e >= 0 else "-") + str(abs(e))
    if k == 1:
        return sign + digits + "e" + exp_text
    return sign + digits[0] + "." + digits[1:] + "e" + exp_text


APERIODIC_FAMILIES = ("hat", "spectre", "penrose-kd", "penrose-rhomb")


def tiling_key(tiling):
    """The identity's tiling key (library-format.md, "The identity text of a pattern").

    `tiling` is a tiling as JETCARP writes it (a scene's tiling, say): its `family`, optional
    `params` and `edges`. Members come in a fixed order, `family`, `params`, `edges`, then
    `"boundary":"dead"`; each value is RFC 8785.
    - `params` is there whenever the tiling has a parameters value, even `{}`; on the aperiodic
      families `level` is taken out first, and `params` left out if that leaves it empty.
    - `edges` is there whenever the tiling has an edge list, even `[]`; each edge is written as the
      engine reads it: `mode`, `curved` (true when left out) and `nodes` (empty when left out), each
      node with its `t` and `d` only.
    """
    params = tiling.get("params")
    if tiling["family"] in APERIODIC_FAMILIES and isinstance(params, dict):
        params = {k: v for k, v in params.items() if k != "level"} or None
    edges = tiling.get("edges")
    if edges is not None:
        edges = [normalise_edge(e) for e in edges]
    members = ['"family":' + dumps(tiling["family"], allow_floats=True)]
    for name, value in (("params", params), ("edges", edges)):
        if value is not None:
            members.append(f'"{name}":' + dumps(value, allow_floats=True))
    members.append('"boundary":"dead"')
    return "{" + ",".join(members) + "}"


def normalise_edge(edge):
    if not isinstance(edge, dict) or "mode" not in edge:
        raise CanonicalError("an edge is not an object with a mode")
    nodes = edge.get("nodes", [])
    if not isinstance(nodes, list) or not all(isinstance(n, dict) and "t" in n and "d" in n for n in nodes):
        raise CanonicalError("an edge's nodes are not objects with t and d")
    return {"mode": edge["mode"], "curved": edge.get("curved", True),
            "nodes": [{"t": n["t"], "d": n["d"]} for n in nodes]}
