#!/usr/bin/env python3
"""Generate docs/src/reference/mcp.md, the MCP tools reference, from the server itself.

Input: the server's own `tools/list`, `resources/list` and `resources/templates/list`, asked over
stdio from `jetcarp mcp` (run with its folders pointed at an empty temporary folder, so nothing of
the user's is read, and no app or host of theirs is found). Every tool gets its description, its
arguments from its input schema (an action's own options listed under it), its hints, and an
example call from EXAMPLES below.

The examples are checked against the schemas: every argument an example gives must be one the
tool takes (nested actions too), and every tool must have an example; a tool or argument that is
gone fails the run (exit status 1), so the page cannot drift from the server.

The page keeps its hand-written introduction. Everything between the markers
  <!-- BEGIN GENERATED: docs/tools/gen-mcp.py -->
  <!-- END GENERATED: docs/tools/gen-mcp.py -->
is replaced.

Standard library only. Usage: python3 docs/tools/gen-mcp.py [--jetcarp target/release/jetcarp]
                                                             [--check]   (check only)
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
PAGE = DOCS / "src" / "reference" / "mcp.md"
BEGIN = "<!-- BEGIN GENERATED: docs/tools/gen-mcp.py -->"
END = "<!-- END GENERATED: docs/tools/gen-mcp.py -->"

# One example call per tool. Checked against the tool's schema.
EXAMPLES = {
    "help": {"topic": "searches"},
    "reference": {"what": "tilings", "search": "k2-"},
    "world_new": {"tiling": "hex", "width": 60, "height": 60, "boundary": "wrap", "cas": ["B24/S35"]},
    "world_status": {"save_path": "scenes/hex.json"},
    "world_read": {"format": "objects"},
    "world_edit": {"cells": {"pattern": ".O.\n..O\nOOO", "x": 10, "y": 10}},
    "world_step": {"generations": 200, "stop_when_settled": True},
    "world_render": {"max_side": 800, "save_path": "hex.png"},
    "run_script": {"script": "sweep(#{ rules: \"W\", until: \"done\", png: #{ size: [400, 300] } });",
                   "max_seconds": 120},
    "library_query": {"class": "spaceship", "tiling": "hex", "limit": 5},
    "library_curate": {"id": "glider", "changes": {"rating": 4, "append_notes": "LLM: the c/4 glider"}},
    "search_start": {"hunt": {"rules": ["B3/S23"], "soups": 20000}, "wait_s": 120},
    "job_status": {"job": 3, "finds": 10},
    "job_stop": {"job": 3},
    "search_runs": {"job": 4, "sheet": {"ids": [12, 40, 7]}},
    "media_render": {"pattern": "glider", "poster": True},
    "pack": {"info": {"path": "packs/starter"}},
    "files_list": {"path": "piles"},
    "app_display": {"screensaver": True, "profile": "Hodgepodge"},
    "use_app": {"attach": True},
    "host_control": {"action": "next"},
}

# Where a tool comes from, when not every server lists it.
ONLY = {
    "app_display": "Listed by the app's own server, and by `jetcarp mcp`, which passes it to the app "
                   "once attached (`use_app`); an error otherwise.",
    "use_app": "`jetcarp mcp` (stdio) only.",
    "host_control": "`jetcarp mcp` (stdio) only.",
}


def ask(jetcarp: str) -> dict:
    msgs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "gen-mcp", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {"jsonrpc": "2.0", "id": 3, "method": "resources/list", "params": {}},
        {"jsonrpc": "2.0", "id": 4, "method": "resources/templates/list", "params": {}},
    ]
    with tempfile.TemporaryDirectory(prefix="jetcarp-gen-mcp-") as tmp:
        env = dict(os.environ)
        for k, sub in [("JETCARP_CONFIG_DIR", "config"), ("JETCARP_DATA_DIR", "data"), ("JETCARP_OUT", "out"),
                       ("JETCARP_LIBRARY", "library"), ("JETCARP_RUNTIME_DIR", "runtime")]:
            env[k] = os.path.join(tmp, sub)
        r = subprocess.run([jetcarp, "mcp"], input="".join(json.dumps(m) + "\n" for m in msgs),
                           capture_output=True, text=True, env=env, timeout=120)
        if tmp in r.stdout:
            sys.exit("the server's lists name the temporary folder: it would put a local path on the page")
    out = {}
    for line in r.stdout.splitlines():
        m = json.loads(line)
        if "error" in m:
            sys.exit(f"jetcarp mcp: {m['error']}")
        out[m["id"]] = m["result"]
    if sorted(out) != [1, 2, 3, 4]:
        sys.exit(f"jetcarp mcp answered {sorted(out)}; stderr: {r.stderr.strip()}")
    return out


def type_of(s: dict) -> str:
    if "enum" in s:
        return ", ".join(f"`{json.dumps(e)}`" for e in s["enum"])
    t = s.get("type")
    if isinstance(t, list):
        t = " or ".join(t)
    if t == "array" and isinstance(s.get("items"), dict) and s["items"].get("type"):
        it = s["items"]["type"]
        t = f"array of {' or '.join(it) if isinstance(it, list) else it}"
    return t or "any"


def cell(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


def check_example(name: str, schema: dict, args: dict, at: str, problems: list):
    props = schema.get("properties") or {}
    for k, v in args.items():
        if k not in props:
            problems.append(f"the example for {name} gives `{at}{k}`, which the tool does not take")
            continue
        p = props[k]
        if isinstance(v, dict) and p.get("properties"):
            check_example(name, p, v, f"{at}{k}.", problems)


def arguments(schema: dict) -> list:
    props = schema.get("properties") or {}
    required = set(schema.get("required") or [])
    rows = ["| Argument | Type | Meaning |", "| --- | --- | --- |"]
    for k in sorted(props):
        p = props[k]
        req = " (required)" if k in required else ""
        rows.append(f"| `{k}`{req} | {cell(type_of(p))} | {cell(p.get('description', ''))} |")
        sub = p.get("properties") or {}
        sub_req = set(p.get("required") or [])
        for k2 in sorted(sub):
            p2 = sub[k2]
            r2 = " (required)" if k2 in sub_req else ""
            rows.append(f"| `{k}.{k2}`{r2} | {cell(type_of(p2))} | {cell(p2.get('description', ''))} |")
    if not props:
        rows = ["No arguments."]
    return rows


def generate(jetcarp: str) -> str:
    got = ask(jetcarp)
    info = got[1]
    tools = got[2]["tools"]
    resources = got[3]["resources"]
    templates = got[4]["resourceTemplates"]
    problems = []
    names = [t["name"] for t in tools]
    for n in names:
        if n not in EXAMPLES:
            problems.append(f"{n} has no example in docs/tools/gen-mcp.py")
    for n in EXAMPLES:
        if n not in names:
            problems.append(f"docs/tools/gen-mcp.py has an example for {n}, which the server no longer lists")
    for t in tools:
        if t["name"] in EXAMPLES:
            check_example(t["name"], t["inputSchema"], EXAMPLES[t["name"]], "", problems)
    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        sys.exit(1)
    s = [BEGIN, "",
         f"The server calls itself `{info['serverInfo']['name']}` ({info['serverInfo']['title']}) and offers "
         f"{len(tools)} tools and {len(resources)} resources. Its instructions to the client:", "",
         "> " + info["instructions"], "",
         "## Tools", "",
         "| Tool | What it does |", "| --- | --- |"]
    for t in tools:
        s.append(f"| [`{t['name']}`](#{t['name']}) | {cell(t['title'])} |")
    s.append("")
    for t in tools:
        a = t.get("annotations", {})
        hints = []
        if a.get("readOnlyHint"):
            hints.append("reads only")
        if a.get("destructiveHint"):
            hints.append("may replace or delete something")
        if t["name"] in ONLY:
            hints.append(ONLY[t["name"]])
        s += [f"### `{t['name']}`", "", f"**{t['title']}.** " + t["description"], ""]
        if hints:
            s += ["*" + "; ".join(hints) + "*", ""]
        s += arguments(t["inputSchema"])
        s += ["", "Example:", "", "```json",
              json.dumps({"name": t["name"], "arguments": EXAMPLES[t["name"]]}, indent=2, ensure_ascii=False),
              "```", ""]
    fixed = [r for r in resources if not r["uri"].startswith("jetcarp://docs/")]
    docs = [r for r in resources if r["uri"].startswith("jetcarp://docs/")]
    s += ["## Resources", "",
          "Resources are what a client can read without calling a tool. Clients that do not offer them "
          "can reach the same text through `help` and `reference`.", "",
          "| URI | Type | What |", "| --- | --- | --- |"]
    for r in fixed:
        s.append(f"| `{r['uri']}` | {r['mimeType']} | {cell(r['title'])}: {cell(r['description'])} |")
    s += ["",
          f"And the {len(docs)} pages of this book, `jetcarp://docs/<page>` by their path without `.md` "
          "(`jetcarp://docs/guides/searching` is [Searching for patterns](../guides/searching.md)), as the "
          "server lists them:", "",
          "<details><summary>The book's pages</summary>", ""]
    for r in docs:
        s.append(f"- `{r['uri']}`: {r['title']}")
    s += ["", "</details>", "",
          "Templates, for one record at a time:", "",
          "| URI template | Type | What |", "| --- | --- | --- |"]
    for r in templates:
        s.append(f"| `{r['uriTemplate']}` | {r['mimeType']} | {cell(r['title'])}: {cell(r['description'])} |")
    s += ["", END]
    return "\n".join(s) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=str(REPO / "target" / "release" / "jetcarp"))
    ap.add_argument("--check", action="store_true", help="only check that the page is up to date")
    args = ap.parse_args()
    page = PAGE.read_text()
    if BEGIN not in page or END not in page:
        sys.exit(f"{PAGE} has no generated section (markers {BEGIN} ... {END})")
    head, rest = page.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    new = head + generate(args.jetcarp).rstrip("\n") + tail
    if args.check:
        if new != page:
            print(f"{PAGE.relative_to(REPO)} is out of date: run python3 docs/tools/gen-mcp.py "
                  "(and then docs/tools/copy-mcp-resources.py)")
            return 1
        print(f"ok: {PAGE.relative_to(REPO)} is up to date")
        return 0
    if new != page:
        PAGE.write_text(new)
        print(f"wrote {PAGE.relative_to(REPO)}")
    else:
        print(f"{PAGE.relative_to(REPO)} unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
