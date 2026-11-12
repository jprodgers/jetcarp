"""Reading JSON from a pack without trusting it (pack-format.md, "Encodings" and "Limits").

`loads()` refuses, before parsing, input over the size limit, a byte-order mark, bytes that are not
UTF-8, and nesting deeper than the limit (so Python's recursive parser never meets a hostile depth).
While parsing it refuses duplicate keys, NaN and Infinity, integers outside +-(2^53 - 1), and (unless
allowed) any non-integer number. After parsing it refuses lone surrogates, which RFC 8785 cannot
write.
"""

import json
import re

from . import jcs

# A JSON string, written so the regular expression engine never backtracks.
_STRING = re.compile(r'"[^"\\]*(?:\\.[^"\\]*)*"', re.S)
_BRACKET = re.compile(r"[\[\]{}]")
_SURROGATE = re.compile("[\ud800-\udfff]")


class JsonError(ValueError):
    """JSON that a pack reader must refuse."""


def loads(data, *, max_bytes, max_depth=32, allow_floats=False, what="document"):
    if isinstance(data, str):
        data = data.encode("utf-8", "surrogatepass")
    if len(data) > max_bytes:
        raise JsonError(f"{what} is {len(data):,} bytes, over the limit of {max_bytes:,}")
    if data.startswith(b"\xef\xbb\xbf"):
        raise JsonError(f"{what} starts with a byte-order mark")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as e:
        raise JsonError(f"{what} is not UTF-8 ({e.reason} at byte {e.start})") from None
    depth = nesting_depth(text)
    if depth > max_depth:
        raise JsonError(f"{what} nests {depth} levels deep, over the limit of {max_depth}")

    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise JsonError(f"{what} repeats the key {key!r}")
            out[key] = value
        return out

    def parse_int(s):
        if len(s.lstrip("-")) > 17:
            raise JsonError(f"{what} has an integer outside +-(2^53 - 1): {s[:40]}")
        n = int(s)
        if abs(n) > jcs.MAX_SAFE_INTEGER:
            raise JsonError(f"{what} has an integer outside +-(2^53 - 1): {s}")
        return n

    def parse_float(s):
        if not allow_floats:
            raise JsonError(f"{what} has a non-integer number {s[:40]} (pack JSON holds integers only)")
        x = float(s)
        if x != x or x in (float("inf"), float("-inf")):
            raise JsonError(f"{what} has a number too large for a double: {s[:40]}")
        return x

    def parse_constant(s):
        raise JsonError(f"{what} has {s}, which is not JSON")

    try:
        value = json.loads(text, object_pairs_hook=pairs, parse_int=parse_int,
                           parse_float=parse_float, parse_constant=parse_constant)
    except JsonError:
        raise
    except (ValueError, RecursionError) as e:
        raise JsonError(f"{what} is not valid JSON: {e}") from None
    _check_strings(value, what)
    return value


def nesting_depth(text):
    """The deepest nesting of arrays and objects in `text`, ignoring brackets inside strings."""
    depth = deepest = 0
    for m in _BRACKET.finditer(_STRING.sub("", text)):
        if m.group(0) in "[{":
            depth += 1
            if depth > deepest:
                deepest = depth
        else:
            depth -= 1
    return deepest


def _check_strings(value, what):
    stack = [value]
    while stack:
        v = stack.pop()
        if isinstance(v, str):
            if _SURROGATE.search(v):
                raise JsonError(f"{what} has a string with a lone surrogate")
        elif isinstance(v, list):
            stack.extend(v)
        elif isinstance(v, dict):
            for k, item in v.items():
                if _SURROGATE.search(k):
                    raise JsonError(f"{what} has a key with a lone surrogate")
                stack.append(item)


def is_canonical(data, value):
    """Whether `data` is exactly the RFC 8785 form of the `value` it parsed to."""
    try:
        return jcs.canonical(value) == data
    except jcs.CanonicalError:
        return False
