#!/usr/bin/env python3
"""Drive `jetcarp mcp` with a local model through an OpenAI-compatible API (LM Studio).

Python standard library only. For each task it starts a fresh `jetcarp mcp`, lists its tools,
offers them to the model as OpenAI function tools, runs the tool-calling loop, checks the
resulting world, and writes a transcript (Markdown, images as PNG files beside it).

    python3 tools/mcp_harness.py                       # all tasks
    python3 tools/mcp_harness.py --task glider         # one task
    python3 tools/mcp_harness.py --jetcarp target/release/jetcarp --base-url http://localhost:1234/v1

Images returned by render_image are passed back to the model as an image_url user message
(the model must accept images; use --no-images otherwise).
"""

import argparse
import base64
import json
import os
import re
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)

SYSTEM = (
    "You control JETCARP, a cellular-automaton simulator, through the tools provided. "
    "Do the user's task by calling tools; check results with tools instead of guessing. "
    "When the task is done, reply with a short answer that states the facts asked for."
)


class Mcp:
    """A minimal MCP client over stdio (legacy initialize handshake, as LM Studio uses)."""

    def __init__(self, cmd):
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1)
        self.next_id = 0
        r = self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                        "clientInfo": {"name": "jetcarp-harness", "version": "1"}})
        self.info = r
        self.notify("notifications/initialized")

    def notify(self, method, params=None):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()

    def request(self, method, params):
        self.next_id += 1
        msg = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError("jetcarp mcp exited: " + self.p.stderr.read())
        r = json.loads(line)
        if r.get("id") != self.next_id:
            raise RuntimeError(f"unexpected reply {line[:200]}")
        if "error" in r:
            raise RuntimeError(f"{method}: {r['error']}")
        return r["result"]

    def call(self, name, args):
        return self.request("tools/call", {"name": name, "arguments": args})

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)


def chat(base_url, model, messages, tools, max_tokens):
    body = {"model": model, "messages": messages, "tools": tools, "tool_choice": "auto",
            "temperature": 0.3, "max_tokens": max_tokens}
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=1800) as resp:
        return json.loads(resp.read())


def status(mcp):
    r = mcp.call("world_status", {})
    return json.loads(r["content"][0]["text"])


def bbox(mcp):
    t = mcp.call("read_cells", {})["content"][0]["text"]
    m = re.search(r"bounding box x (\d+)\.\.(\d+), y (\d+)\.\.(\d+)", t)
    return tuple(int(v) for v in m.groups()) if m else None


def has_rule(st, notation):
    return any(c["rule"].upper() == notation.upper() for c in st["cas"])


# Each task: prompt and a checker (mcp, final_text, tool_calls) -> (ok, note).
def check_hex_brain(mcp, text, calls):
    st = status(mcp)
    ok = (st["tiling"]["id"] == "hex" and st["tiling"]["boundary"] == "wrap"
          and has_rule(st, "B2/S/C3") and st["generation"] >= 200
          and any(c == "render_image" for c, _ in calls))
    return ok, f"tiling {st['tiling']['id']}/{st['tiling']['boundary']}, gen {st['generation']}, " \
               f"rules {[c['rule'] for c in st['cas']]}, rendered {any(c == 'render_image' for c, _ in calls)}"


def check_glider(mcp, text, calls):
    st = status(mcp)
    b = bbox(mcp)
    pop = sum(f["population"] for f in st["fields"])
    ok = has_rule(st, "B3/S23") and st["generation"] == 8 and pop == 5 and b is not None
    nums = set(int(n) for n in re.findall(r"\d+", text))
    mentioned = b is not None and b[0] in nums and b[2] in nums
    return ok and mentioned, f"gen {st['generation']}, population {pop}, bbox {b}, " \
                             f"answer mentions bbox: {mentioned}"


def check_ant(mcp, text, calls):
    st = status(mcp)
    ants = [c for c in st["cas"] if c["rule"].lower().startswith("ant") or "turmite" in c["rule"].lower()]
    steps = st["generation"] * (ants[0]["steps_per_gen"] if ants else 0)
    pop = sum(f["population"] for f in st["fields"])
    ok = bool(ants) and steps >= 11000 and str(pop) in text.replace(",", "")
    return ok, f"ant steps {steps}, live cells {pop}, answer has count: {str(pop) in text.replace(',', '')}"


def check_sandpile(mcp, text, calls):
    st = status(mcp)
    piles = [f for f in st["fields"] if f["kind"] == "height"]
    stable = bool(piles) and all(f.get("stable") for f in piles)
    grains = sum(f["population"] + f.get("grains_lost_off_edges", 0) for f in piles)
    rendered = any(c == "render_image" for c, _ in calls)
    ok = has_rule(st, "Sandpile") and stable and grains == 20000 and rendered
    return ok, f"stable {stable}, grains on grid + lost {grains}, rendered {rendered}"


def check_triangle(mcp, text, calls):
    st = status(mcp)
    pop = sum(f["population"] for f in st["fields"])
    ok = st["tiling"]["id"] == "triangle" and has_rule(st, "B3/S23") and st["generation"] > 0 \
        and str(pop) in text.replace(",", "")
    return ok, f"tiling {st['tiling']['id']}, gen {st['generation']}, status {st['status']}, population {pop}"


def data_dir():
    """JETCARP's data folder, as `jetcarp paths` reports it (JETCARP_DATA_DIR overrides it)."""
    if os.environ.get("JETCARP_DATA_DIR"):
        return os.environ["JETCARP_DATA_DIR"]
    if sys.platform == "win32":
        return os.path.join(os.environ.get("LOCALAPPDATA", os.path.expanduser("~")), "jetcarp")
    if sys.platform == "darwin":
        return os.path.expanduser("~/Library/Application Support/jetcarp/data")
    xdg = os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share")
    return os.path.join(xdg, "jetcarp")


EXPLORE = os.path.join(data_dir(), "explore")


def check_explore_query(mcp, text, calls):
    names = [c for c, _ in calls]
    with sqlite3.connect(f"file:{EXPLORE}/search.sqlite?mode=ro", uri=True) as c:
        best = c.execute("SELECT id FROM runs WHERE stage=1 AND fam='greenberg_hastings' "
                         "ORDER BY score DESC LIMIT 1").fetchone()[0]
    ok = "explore_query" in names and "explore_sheet" in names and str(best) in text.replace(",", "")
    return ok, f"best run {best}, in answer: {str(best) in text.replace(',', '')}, calls {sorted(set(names))}"


def check_explore_search(mcp, text, calls):
    names = [c for c, _ in calls]
    db = f"{EXPLORE}/llmtest.sqlite"
    runs = 0
    if os.path.exists(db):
        with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as c:
            runs = c.execute("SELECT COUNT(*) FROM runs").fetchone()[0]
    ok = runs > 0 and "explore_start" in names and "explore_status" in names
    return ok, f"runs in the new database {runs}, calls {sorted(set(names))}"


TASKS = {
    "explore-query": ("The search database named search holds a finished search. Find its three best "
                      "Greenberg-Hastings runs, show me a contact sheet of them, and tell me their run ids "
                      "and scores.", check_explore_query),
    "explore-search": ("Start a small search in a new database named llmtest that runs for 0.02 hours. "
                       "Check on it until it has runs in it, then tell me which rule family has the most "
                       "runs so far.", check_explore_search),
    "hex-brain": ("Make a hexagon grid with wrap edges, run Brian's Brain for 200 generations, "
                  "and show me a picture.", check_hex_brain),
    "glider": ("Draw a glider in Life and tell me where it is after 8 generations.", check_glider),
    "ant": ("Put one Langton's ant on a 60 by 60 square grid with dead edges, let it take 11000 steps, "
            "and tell me how many cells are black.", check_ant),
    "sandpile": ("Drop 20000 grains of sand on the centre of a 61x61 square grid with dead edges, let it "
                 "topple until it is stable, and show me what it looks like.", check_sandpile),
    "triangle": ("Run Conway's Life on a triangle tiling of about 3000 cells from a random start until it "
                 "settles (give up after 1000 generations). Tell me the final status and how many cells "
                 "are alive.", check_triangle),
}


def short(s, n=1500):
    s = s if isinstance(s, str) else json.dumps(s)
    return s if len(s) <= n else s[:n] + f"... [{len(s) - n} more chars]"


def run_task(name, args, out_dir):
    prompt, checker = TASKS[name]
    mcp = Mcp([args.jetcarp, "mcp"])
    listed = mcp.request("tools/list", {})["tools"]
    tools = [{"type": "function", "function": {"name": t["name"], "description": t["description"],
                                               "parameters": t["inputSchema"]}} for t in listed]
    messages = [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]
    log = [f"# Task `{name}`\n", f"**Prompt:** {prompt}\n",
           f"Model `{args.model}`, {len(tools)} tools, server {mcp.info.get('serverInfo')}.\n"]
    calls, errors, final, t0, img_n = [], 0, "", time.time(), 0
    for turn in range(args.max_turns):
        try:
            r = chat(args.base_url, args.model, messages, tools, args.max_tokens)
        except urllib.error.URLError as e:
            log.append(f"\n**HTTP error:** {e}\n")
            break
        msg = r["choices"][0]["message"]
        reasoning = msg.get("reasoning_content") or msg.get("reasoning")
        if reasoning:
            log.append(f"\n<details><summary>reasoning (turn {turn + 1})</summary>\n\n{short(reasoning, 3000)}\n\n</details>\n")
        content = msg.get("content") or ""
        if content.strip():
            log.append(f"\n**Assistant:** {content.strip()}\n")
        tcs = msg.get("tool_calls") or []
        messages.append({"role": "assistant", "content": content, "tool_calls": tcs} if tcs
                        else {"role": "assistant", "content": content})
        if not tcs:
            final = content
            break
        images = []
        for tc in tcs:
            fname = tc["function"]["name"]
            raw = tc["function"].get("arguments") or "{}"
            try:
                a = json.loads(raw) if isinstance(raw, str) else raw
            except json.JSONDecodeError as e:
                a, res = None, {"content": [{"type": "text", "text": f"arguments are not valid JSON: {e}"}],
                                "isError": True}
            if a is not None:
                try:
                    res = mcp.call(fname, a)
                except RuntimeError as e:
                    res = {"content": [{"type": "text", "text": str(e)}], "isError": True}
            calls.append((fname, a))
            errors += bool(res.get("isError"))
            texts = []
            for c in res.get("content", []):
                if c["type"] == "text":
                    texts.append(c["text"])
                elif c["type"] == "image":
                    img_n += 1
                    fn = f"{name}-{img_n}.png"
                    with open(os.path.join(out_dir, fn), "wb") as f:
                        f.write(base64.b64decode(c["data"]))
                    texts.append(f"[image {fn} attached below]")
                    images.append(c)
                    log.append(f"\n![{fn}]({fn})\n")
            body = "\n".join(texts)
            flag = " (error)" if res.get("isError") else ""
            log.append(f"\n**Tool call** `{fname}` `{short(raw, 600)}`{flag}\n\n```\n{short(body)}\n```\n")
            messages.append({"role": "tool", "tool_call_id": tc.get("id", fname), "content": body})
        if images and not args.no_images:
            parts = [{"type": "text", "text": "Image returned by render_image:"}]
            parts += [{"type": "image_url", "image_url": {"url": f"data:{c['mimeType']};base64,{c['data']}"}}
                      for c in images]
            messages.append({"role": "user", "content": parts})
        with open(os.path.join(out_dir, f"{name}.md"), "w") as f:  # progress so far
            f.write("".join(log))
    secs = time.time() - t0
    try:
        ok, note = checker(mcp, final, calls)
    except Exception as e:  # noqa: BLE001 - report checker failures as task failures
        ok, note = False, f"checker failed: {e}"
    mcp.close()
    verdict = "PASS" if ok else "FAIL"
    log.append(f"\n**Result: {verdict}** ({note}). {len(calls)} tool calls, {errors} tool errors, "
               f"{secs:.0f} s.\n")
    with open(os.path.join(out_dir, f"{name}.md"), "w") as f:
        f.write("".join(log))
    return {"task": name, "pass": ok, "note": note, "tool_calls": len(calls), "tool_errors": errors,
            "seconds": round(secs), "tools_used": [c for c, _ in calls], "final": final.strip()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--task", action="append", choices=sorted(TASKS), help="run only these tasks")
    ap.add_argument("--max-turns", type=int, default=24)
    ap.add_argument("--max-tokens", type=int, default=8192)
    ap.add_argument("--no-images", action="store_true", help="do not show rendered images to the model")
    ap.add_argument("--out", default=os.path.join(HERE, "mcp_transcripts"))
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    results = []
    for name in args.task or list(TASKS):
        print(f"== {name}", flush=True)
        r = run_task(name, args, args.out)
        print(f"   {'PASS' if r['pass'] else 'FAIL'}: {r['note']} ({r['tool_calls']} calls, "
              f"{r['tool_errors']} errors, {r['seconds']} s)", flush=True)
        results.append(r)
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(results, f, indent=2)
    print(f"{sum(r['pass'] for r in results)}/{len(results)} passed; transcripts in {args.out}")
    return 0 if all(r["pass"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
