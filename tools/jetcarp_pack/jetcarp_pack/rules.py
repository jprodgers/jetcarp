"""B/S and Generations notation (docs/src/reference/rule-notation.md), read only to cross-check the
`rules` table's masks: replay itself uses the masks, as the spec intends (section 11)."""

import re

_FORM = re.compile(r"B([0-9,]*)/S([0-9,]*)(?:/[CG]?([0-9]+))?([VH]?)\Z")


def _counts(text):
    if not text:
        return []
    if "," in text:
        return [int(x) for x in text.split(",") if x != ""]
    return [int(ch) for ch in text]


def parse_bs(notation):
    """(birth mask, survive mask, states, neighbourhood or None) for a B/S or Generations rule, or None
    if `notation` is not one. `neighbourhood` is "edge" for a trailing V, else None: without V the
    automaton's setting decides."""
    text = re.sub(r"\s+", "", notation or "").upper()
    m = _FORM.match(text)
    if not m:
        return None
    try:
        birth, survive = _counts(m.group(1)), _counts(m.group(2))
    except ValueError:
        return None
    if any(n > 31 for n in birth + survive):
        return None
    states = int(m.group(3)) if m.group(3) else 2
    bm = sum(1 << n for n in set(birth))
    sm = sum(1 << n for n in set(survive))
    return bm, sm, states, ("edge" if m.group(4) == "V" else None)


def mask_text(mask):
    return ",".join(str(n) for n in range(64) if mask >> n & 1) or "none"
