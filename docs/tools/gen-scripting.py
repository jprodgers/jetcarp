#!/usr/bin/env python3
"""Generate the reference part of docs/src/reference/scripting.md, the Rhai API.

Inputs (the program's own):
  crates/jetcarp-script/src/help.md   the function reference that `jetcarp script --functions`,
                                      the app's Function reference tab and the MCP help print
  crates/jetcarp-script/src/*.rs      the `register_fn` calls: what the engine really registers
  `jetcarp script --functions`        (optional, --jetcarp PATH) to check the binary prints help.md

The page keeps its hand-written introduction. Everything between the markers
  <!-- BEGIN GENERATED: docs/tools/gen-scripting.py -->
  <!-- END GENERATED: docs/tools/gen-scripting.py -->
is replaced: help.md's sections (headings one level down), then an index of every registered
function with its argument types, each linked to the section that describes it.

The generator checks that the two lists agree: every function help.md shows is registered (with
that number of arguments), and every registered function is shown in help.md. Any difference is
printed and the page is not written (exit status 1), so the reference cannot drift from the engine.

Standard library only. Usage: python3 docs/tools/gen-scripting.py [--jetcarp target/release/jetcarp]
                                                                    [--check]   (check only)
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
SCRIPT_SRC = REPO / "crates" / "jetcarp-script" / "src"
HELP = SCRIPT_SRC / "help.md"
PAGE = DOCS / "src" / "reference" / "scripting.md"
BEGIN = "<!-- BEGIN GENERATED: docs/tools/gen-scripting.py -->"
END = "<!-- END GENERATED: docs/tools/gen-scripting.py -->"

# Rhai's own functions that help.md mentions; the engine does not register them.
RHAI_BUILTINS = {"print", "debug"}
# Registered so that `print(w)` and `debug(w)` show a world or a catalogue handle.
PRINTING = {"to_string", "to_debug"}

# Phrases in help.md that point outside the book, and the book link that replaces them on the page.
# A phrase that no longer occurs is reported, so this list cannot go stale unnoticed.
LINKS = {
    '(see the book\'s "The pattern library" guide)':
        "(see [The pattern library](../guides/pattern-library.md) and "
        "[Folders](folders.md#environment-variables))",
}

RUST_TYPES = {
    "INT": "int", "i64": "int", "u64": "int", "usize": "int",
    "FLOAT": "float", "f64": "float", "f32": "float",
    "bool": "bool", "&str": "string", "String": "string", "ImmutableString": "string",
    "Map": "map", "Array": "array", "Dynamic": "any",
    "Shared": "World", "&mut Shared": "World", "&mut LibHandle": "Library", "LibHandle": "Library",
}


# ---------------------------------------------------------------------------------------------
# The engine's registrations

def matching(text: str, i: int) -> int:
    """Index just past the bracket that closes the one at text[i] (strings skipped)."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[text[i]]]
    j = i + 1
    while j < len(text) and stack:
        c = text[j]
        if c == '"':
            j += 1
            while j < len(text) and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
        elif c == "'" and re.match(r"'(\\.|[^\\'])'", text[j:j + 4]):
            j = text.index("'", j + 1)
        elif c in pairs:
            stack.append(pairs[c])
        elif stack and c == stack[-1]:
            stack.pop()
        j += 1
    return j


def split_top(s: str) -> list:
    """Split on commas outside brackets, quotes and backticks."""
    out, depth, cur, quote = [], 0, "", None
    for c in s:
        if quote:
            cur += c
            if c == quote:
                quote = None
            continue
        if c in "\"'`":
            quote = c
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            out.append(cur)
            cur = ""
            continue
        cur += c
    if cur.strip():
        out.append(cur)
    return [x.strip() for x in out]


def loop_names(text: str, at: int, var: str) -> list:
    """The names a `for <var> in [...]` or `for (<var>, ..) in [...]` loop before `at` takes, with
    each entry's other tuple fields."""
    pat = re.compile(r"for\s+(\(\s*)?" + re.escape(var) + r"\b[^\n]*?\bin\s+\[")
    found = None
    for m in pat.finditer(text, 0, at):
        found = m
    if not found:
        raise SystemExit(f"gen-scripting: cannot find the loop that names `{var}`")
    lb = found.end() - 1
    body = text[lb + 1:matching(text, lb) - 1]
    entries = []
    for item in split_top(body):
        if item.startswith("("):
            fields = split_top(item[1:-1])
        else:
            fields = [item]
        name = fields[0]
        if not (name.startswith('"') and name.endswith('"')):
            raise SystemExit(f"gen-scripting: loop entry {item!r} has no literal name")
        entries.append((name[1:-1], fields[1:]))
    return entries


def registrations() -> list:
    """(kind, name, [argument types]) for every `register_fn` call. Kind is `world` (a method of
    a World, its first parameter `&mut Shared`), `library` (of a catalogue handle), or `global`."""
    regs = []
    for path in sorted(SCRIPT_SRC.glob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in re.finditer(r"\.register_fn\(", text):
            lp = m.end() - 1
            call = text[lp + 1:matching(text, lp) - 1]
            args = split_top(call)
            if len(args) < 2:
                raise SystemExit(f"gen-scripting: {path.name}: cannot read register_fn({call[:60]}…)")
            first, func = args[0], ",".join(args[1:])
            pm = re.match(r"(?:move\s+)?\|([^|]*)\|", func.strip())
            if pm:
                params = [p.split(":", 1)[1].strip() for p in split_top(pm.group(1)) if ":" in p]
            elif re.fullmatch(r"[A-Za-z_][\w:]*", func.strip()):
                params = fn_params(func.strip().split("::")[-1], path, text)
            else:
                raise SystemExit(f"gen-scripting: {path.name}: cannot read the function of register_fn({first}, …)")
            params = [p for p in params if p != "NativeCallContext"]
            if first.startswith('"'):
                names = [first.strip('"')]
            else:
                entries = loop_names(text, m.start(), first)
                # The global form of a search exists only for the entries marked global.
                window = text[max(0, m.start() - 200):m.start()]
                if re.search(r"if\s+global\s*\{\s*(let\s+c\s*=\s*ctx\.clone\(\);\s*)?\w*$", window):
                    entries = [e for e in entries if e[1] and e[1][-1] == "true"]
                names = [e[0] for e in entries]
            if params and params[0] == "&mut Shared":
                kind, rest = "world", params[1:]
            elif params and params[0] in ("&mut LibHandle", "LibHandle"):
                kind, rest = "library", params[1:]
            else:
                kind, rest = "global", params
            for n in names:
                regs.append((kind, n, [RUST_TYPES.get(t, t) for t in rest], path.name))
    return regs


def fn_params(name: str, path: Path, text: str) -> list:
    """The parameter types of a function registered by name: a closure kept in a variable
    (`let info = move |l: &mut LibHandle| …`) or a named function (`out::stamp`)."""
    m = re.search(r"\blet\s+" + re.escape(name) + r"\s*=\s*(?:move\s+)?\|([^|]*)\|", text)
    if m:
        return [p.split(":", 1)[1].strip() for p in split_top(m.group(1)) if ":" in p]
    candidates = [text] + [p.read_text(encoding="utf-8") for p in sorted(SCRIPT_SRC.glob("*.rs"))]
    candidates.append((REPO / "crates" / "jetcarp-api" / "src" / "out.rs").read_text(encoding="utf-8"))
    for t in candidates:
        m = re.search(r"\bfn\s+" + re.escape(name) + r"\s*(?:<[^>]*>)?\(([^)]*)\)", t)
        if m:
            return [p.split(":", 1)[1].strip() for p in split_top(m.group(1)) if ":" in p]
    raise SystemExit(f"gen-scripting: {path.name}: cannot find fn {name}")


# ---------------------------------------------------------------------------------------------
# help.md

CALL = re.compile(r"^(?:(w|lib|library\(\))\.)?([a-z_][a-z0-9_]*)\((.*)\)$", re.S)


def code_spans(line: str) -> list:
    return [s.replace("\\|", "|") for s in re.findall(r"`([^`]+)`", line.replace("``", ""))]


def documented(help_text: str) -> dict:
    """(kind, name) -> {"arities": set, "forms": [call text], "section": heading}."""
    out = {}
    section = None
    for line in help_text.splitlines():
        h = re.match(r"^##\s+(.*)$", line)
        if h:
            section = h.group(1).strip()
            continue
        # The first column of a table row names the functions the row describes; other code
        # spans (descriptions, paragraphs) only mention them.
        first_spans = []
        if line.startswith("|") and not re.match(r"^\|\s*-", line):
            cells = re.split(r"(?<!\\)\|", line)
            if len(cells) > 2:
                first_spans = code_spans(cells[1])
        for span in code_spans(line):
            m = CALL.match(span.strip())
            if not m:
                continue
            recv, name, args = m.groups()
            kind = {"w": "world", "lib": "library", "library()": "library"}.get(recv, "global")
            n = len(split_top(args)) if args.strip() else 0
            d = out.setdefault((kind, name), {"arities": set(), "forms": [], "section": None, "main": None})
            d["arities"].add(n)
            if span in first_spans:
                if span not in d["forms"]:
                    d["forms"].append(span)
                if d["main"] is None:
                    d["main"] = section
            if d["section"] is None:
                d["section"] = section
    return out


def heading_id(text: str) -> str:
    out = []
    for ch in text.strip().lower():
        if ch.isalnum() or ch in "-_":
            out.append(ch)
        elif ch.isspace():
            out.append("-")
    return "".join(out)


# ---------------------------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jetcarp", help="also check that this binary prints help.md for `script --functions`")
    ap.add_argument("--check", action="store_true", help="check only; do not write the page")
    args = ap.parse_args()

    help_text = HELP.read_text(encoding="utf-8")
    problems = []

    if args.jetcarp:
        try:
            printed = subprocess.run([args.jetcarp, "script", "--functions"], capture_output=True,
                                     text=True, check=True).stdout
            if printed != help_text:
                problems.append(f"{args.jetcarp} script --functions does not print {HELP.relative_to(REPO)} "
                                "(rebuild the program)")
        except (OSError, subprocess.CalledProcessError) as e:
            problems.append(f"could not run {args.jetcarp} script --functions: {e}")

    regs = registrations()
    reg = {}
    for kind, name, types, src in regs:
        d = reg.setdefault((kind, name), {"sigs": [], "files": set()})
        if types not in d["sigs"]:
            d["sigs"].append(types)
        d["files"].add(src)
    doc = documented(help_text)

    # A bare name in a description (`objects()`, `query(#{ works_in })`) is shorthand for the
    # method of that name when no global function has it.
    for (kind, name) in list(doc):
        d = doc[(kind, name)]
        if (kind == "global" and d["main"] is None and (kind, name) not in reg
                and ((("world", name) in reg) or (("library", name) in reg))):
            del doc[(kind, name)]
    for (kind, name), d in sorted(doc.items()):
        if name in RHAI_BUILTINS and kind == "global":
            continue
        if (kind, name) not in reg:
            problems.append(f"help.md shows {kind} function `{name}`, which the engine does not register")
            continue
        arities = {len(s) for s in reg[(kind, name)]["sigs"]}
        for n in sorted(d["arities"] - arities):
            problems.append(f"help.md shows `{name}` ({kind}) with {n} argument(s); the engine registers "
                            f"{sorted(arities)}")
    for (kind, name), d in sorted(reg.items()):
        if name in PRINTING:
            continue
        if (kind, name) not in doc:
            problems.append(f"the engine registers {kind} function `{name}` "
                            f"({', '.join(sorted(d['files']))}), which help.md does not show")

    for phrase in LINKS:
        if phrase not in help_text:
            problems.append(f"gen-scripting.py's LINKS phrase is no longer in help.md: {phrase!r}")

    if problems:
        for p in problems:
            print(f"gen-scripting: {p}", file=sys.stderr)
        print(f"gen-scripting: {len(problems)} problem(s); {PAGE.relative_to(REPO)} not written", file=sys.stderr)
        sys.exit(1)

    # The reference part: help.md, headings one level down, minus its title.
    body = help_text
    for phrase, repl in LINKS.items():
        body = body.replace(phrase, repl)
    lines = body.splitlines()
    if lines and lines[0].startswith("# "):
        lines = lines[1:]
    intro, sections, seen_section = [], [], False
    for line in lines:
        if line.startswith("## "):
            seen_section = True
        (sections if seen_section else intro).append(line)
    sections = ["#" + l if l.startswith("## ") else l for l in sections]

    n_funcs = len([k for k in reg if k[1] not in PRINTING])
    n_sigs = sum(len(d["sigs"]) for k, d in reg.items() if k[1] not in PRINTING)
    out = [
        BEGIN,
        "",
        '<p class="generated-note">Generated by <code>docs/tools/gen-scripting.py</code> from the '
        "program's own function reference (<code>crates/jetcarp-script/src/help.md</code>, which "
        "<code>jetcarp script --functions</code> prints) and from the functions the scripting engine "
        f"registers ({n_funcs} functions, {n_sigs} signatures). The generator refuses to write this "
        "part if the two disagree. Edit <code>help.md</code>, not this page.</p>",
        "",
        "## Function reference",
        "",
        "### The language",
        "",
    ]
    out += [l for l in intro if l.strip() or out[-1].strip()]
    out.append("")
    out += sections
    while out and not out[-1].strip():
        out.pop()

    # The index.
    out += [
        "",
        "## Index of functions",
        "",
        "Every function the engine registers, with the types of its arguments in each form "
        "(`int`, `float`, `string`, `bool`, `map` (`#{ … }`), `array` (`[ … ]`), `any`). Methods of a "
        "world are called as `w.name(…)` and methods of a catalogue handle as `lib.name(…)`; Rhai also "
        "accepts `name(w, …)`. Worlds and catalogue handles can be printed with `print(w)`.",
        "",
        "| Function | Argument types | Described in |",
        "| --- | --- | --- |",
    ]
    prefix = {"world": "w.", "library": "lib.", "global": ""}
    order = sorted((k for k in reg if k[1] not in PRINTING), key=lambda k: (k[1], k[0]))
    for kind, name in order:
        d = doc[(kind, name)]
        sigs = sorted(reg[(kind, name)]["sigs"], key=len)
        types = "; ".join("(" + ", ".join(s) + ")" for s in sigs)
        sec = d["main"] or d["section"]
        out.append(f"| `{prefix[kind]}{name}` | {types} | [{sec}](#{heading_id(sec)}) |")
    out += ["", END]
    generated = "\n".join(out) + "\n"

    page = PAGE.read_text(encoding="utf-8")
    if BEGIN not in page or END not in page:
        raise SystemExit(f"gen-scripting: {PAGE.relative_to(REPO)} has no generated-part markers")
    head, rest = page.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    new = head + generated + tail.lstrip("\n")
    if args.check:
        print(f"gen-scripting: ok ({n_funcs} functions, {n_sigs} signatures; page "
              f"{'up to date' if new == page else 'would change'})", file=sys.stderr)
        sys.exit(0 if new == page else 1)
    PAGE.write_text(new, encoding="utf-8")
    print(f"gen-scripting: wrote {PAGE.relative_to(REPO)} ({n_funcs} functions, {n_sigs} signatures)",
          file=sys.stderr)


if __name__ == "__main__":
    main()
