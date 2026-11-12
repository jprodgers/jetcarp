#!/usr/bin/env python3
"""Drive `jetcarp mcp` with a local model through an OpenAI-compatible API (LM Studio, llama.cpp,
vLLM, Ollama's OpenAI endpoint), to see how well a model uses JETCARP's tools.

Python standard library only. For each task it starts a fresh `jetcarp mcp`, lists its tools,
offers them to the model as OpenAI function tools, runs the tool-calling loop, checks the result
with the tools themselves (not what the model says), and writes a transcript (Markdown, images as
PNG files beside it).

    python3 contrib/llm/mcp_harness.py                       # every task
    python3 contrib/llm/mcp_harness.py --task glider         # one task
    python3 contrib/llm/mcp_harness.py --goal "Find a spaceship in B2/S34 on hexagons"   # your own
    python3 contrib/llm/mcp_harness.py --jetcarp target/release/jetcarp --base-url http://localhost:1234/v1

The server runs with all of JETCARP's folders (output, library, data, settings, runtime) in a
scratch folder (--work, default a new temporary folder), so a task never touches your own
library or pictures, and never finds your running app. Images the tools return are passed back to
the model as an image_url user message (the model must accept images; use --no-images otherwise).
"""

import argparse
import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

SYSTEM = (
    "You control JETCARP, a cellular-automaton research tool, through the tools provided. "
    "Do the user's task by calling tools; check results with tools instead of guessing (never "
    "guess ids, periods or counts). Call help first if you are unsure how the tools fit together. "
    "When the task is done, reply with a short answer that states the facts asked for."
)


class Mcp:
    """A minimal MCP client over stdio (the legacy initialize handshake, as LM Studio uses)."""

    def __init__(self, cmd, env):
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1, env=env)
        self.next_id = 0
        self.progress = []
        r = self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                        "clientInfo": {"name": "jetcarp-harness", "version": "2"}})
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
        if method == "tools/call" and params.get("name") in ("run_script", "search_start", "job_status"):
            params = dict(params, _meta={"progressToken": f"p{self.next_id}"})
        msg = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        self.progress = []
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("jetcarp mcp exited: " + self.p.stderr.read())
            r = json.loads(line)
            if r.get("method") == "notifications/progress":
                self.progress.append(r["params"])
                print(f"      progress: {r['params'].get('message') or r['params'].get('progress')}", flush=True)
                continue
            if r.get("id") != self.next_id:
                raise RuntimeError(f"unexpected reply {line[:200]}")
            if "error" in r:
                raise RuntimeError(f"{method}: {r['error']}")
            return r["result"]

    def call(self, name, args):
        return self.request("tools/call", {"name": name, "arguments": args})

    def text(self, name, args):
        """A call's text blocks, joined (an error raises)."""
        r = self.call(name, args)
        t = "\n".join(c["text"] for c in r.get("content", []) if c["type"] == "text")
        if r.get("isError"):
            raise RuntimeError(f"{name}: {t}")
        return t

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=30)


def chat(base_url, model, messages, tools, max_tokens):
    body = {"model": model, "messages": messages, "tools": tools, "tool_choice": "auto",
            "temperature": 0.3, "max_tokens": max_tokens}
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=1800) as resp:
        return json.loads(resp.read())


def short(s, n=1500):
    s = s if isinstance(s, str) else json.dumps(s)
    return s if len(s) <= n else s[:n] + f"... [{len(s) - n} more chars]"


# ---------------------------------------------------------------------------------------------
# Checks: each looks at the world or the library through the tools.

def status(mcp):
    r = mcp.call("world_status", {})
    return json.loads(r["content"][0]["text"])


def bbox(mcp):
    t = mcp.text("world_read", {})
    m = re.search(r"bounding box x (\d+)\.\.(\d+), y (\d+)\.\.(\d+)", t)
    return tuple(int(v) for v in m.groups()) if m else None


def has_rule(st, notation):
    return any(c["rule"].upper() == notation.upper() for c in st["cas"])


def rendered(calls):
    return any(c == "world_render" for c, _ in calls)


def check_brain(mcp, text, calls):
    st = status(mcp)
    ok = (st["tiling"]["id"] == "square" and st["tiling"]["boundary"] == "wrap"
          and has_rule(st, "B2/S/C3") and st["generation"] >= 200 and rendered(calls))
    return ok, (f"tiling {st['tiling']['id']}/{st['tiling']['boundary']}, gen {st['generation']}, "
                f"rules {[c['rule'] for c in st['cas']]}, rendered {rendered(calls)}")


def check_glider(mcp, text, calls):
    st = status(mcp)
    b = bbox(mcp)
    pop = sum(f["population"] for f in st["fields"])
    ok = has_rule(st, "B3/S23") and st["generation"] == 8 and pop == 5 and b is not None
    nums = set(int(n) for n in re.findall(r"\d+", text))
    mentioned = b is not None and b[0] in nums and b[2] in nums
    return ok and mentioned, (f"gen {st['generation']}, population {pop}, bbox {b}, "
                              f"answer mentions bbox: {mentioned}")


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
    ok = has_rule(st, "Sandpile") and stable and grains == 20000 and rendered(calls)
    return ok, f"stable {stable}, grains on grid + lost {grains}, rendered {rendered(calls)}"


def check_triangle(mcp, text, calls):
    st = status(mcp)
    pop = sum(f["population"] for f in st["fields"])
    ok = (st["tiling"]["id"] == "triangle" and has_rule(st, "B3/S23") and st["generation"] > 0
          and str(pop) in text.replace(",", ""))
    return ok, f"tiling {st['tiling']['id']}, gen {st['generation']}, status {st['status']}, population {pop}"


def check_hunt(mcp, text, calls):
    jobs = json.loads(mcp.text("job_status", {"kind": "hunt"}))["jobs"]
    done = [j for j in jobs if j["status"] == "done"]
    ships = json.loads(mcp.text("library_query", {"class": "spaceship", "limit": 20}))["patterns"]
    qs = json.loads(mcp.text("library_query", {"kind": "quickstart", "source": "promotion"}))["entries"]
    promoted = [q for q in qs if q["made_from"] == "pattern"]
    fastest = {"c/2 orthogonal": 2, "c/4 diagonal": 1}
    best = max((fastest.get(s["speed"], 0) for s in ships), default=0)
    picked = [s for s in ships if s["state"] == "promoted"]
    ok = bool(done) and bool(promoted) and any(fastest.get(s["speed"], 0) == best for s in picked)
    return ok, (f"hunts done {len(done)}; spaceships {[(s['label'], s['speed']) for s in ships]}; "
                f"promoted {[(s['label'], s['speed']) for s in picked]}")


TASKS = {
    "brain": ("Make a 120 by 80 square grid with wrap edges, run Brian's Brain for 200 generations, "
              "and show me a picture.", check_brain),
    "glider": ("Draw a glider in Life and tell me where it is after 8 generations.", check_glider),
    "ant": ("Put one Langton's ant on a 60 by 60 square grid with dead edges, let it take 11000 steps, "
            "and tell me how many cells are black.", check_ant),
    "sandpile": ("Drop 20000 grains of sand on the centre of a 61x61 square grid with dead edges, let it "
                 "topple until it is stable, and show me what it looks like.", check_sandpile),
    "triangle": ("Run Conway's Life on a triangle tiling of about 3000 cells from a random start until it "
                 "settles (give up after 1000 generations). Tell me the final status and how many cells "
                 "are alive.", check_triangle),
    "hunt": ("Run a hunt of 5,000 soups for Conway's Life in the background and wait for it to finish. "
             "Tell me which spaceships it found, and add the fastest one to the Quick Start.", check_hunt),
}


def run_task(name, prompt, checker, args, env):
    mcp = Mcp([args.jetcarp, "mcp"], env)
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
            print(f"   turn {turn + 1}: {fname} {short(raw, 160)}", flush=True)
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
                    with open(os.path.join(args.out, fn), "wb") as f:
                        f.write(base64.b64decode(c["data"]))
                    texts.append(f"[image {fn} attached below]")
                    images.append(c)
                    log.append(f"\n![{fn}]({fn})\n")
            body = "\n".join(texts)
            flag = " (error)" if res.get("isError") else ""
            log.append(f"\n**Tool call** `{fname}` `{short(raw, 600)}`{flag}\n\n```\n{short(body)}\n```\n")
            if len(body) > args.max_result_chars:
                # A local model's context is small: never pass a huge result on whole.
                body = (body[: args.max_result_chars]
                        + f"\n... [{len(body) - args.max_result_chars} more characters cut by the harness]")
            messages.append({"role": "tool", "tool_call_id": tc.get("id", fname), "content": body})
        if images and not args.no_images:
            parts = [{"type": "text", "text": "The images the tools returned:"}]
            parts += [{"type": "image_url", "image_url": {"url": f"data:{c['mimeType']};base64,{c['data']}"}}
                      for c in images]
            messages.append({"role": "user", "content": parts})
        with open(os.path.join(args.out, f"{name}.md"), "w") as f:  # progress so far
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
    with open(os.path.join(args.out, f"{name}.md"), "w") as f:
        f.write("".join(log))
    return {"task": name, "pass": ok, "note": note, "tool_calls": len(calls), "tool_errors": errors,
            "seconds": round(secs), "tools_used": [c for c, _ in calls], "final": final.strip()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--task", action="append", choices=sorted(TASKS), help="run only these tasks")
    ap.add_argument("--goal", help="run this goal instead of the built-in tasks (no checker)")
    ap.add_argument("--max-turns", type=int, default=24)
    ap.add_argument("--max-tokens", type=int, default=8192)
    ap.add_argument("--max-result-chars", type=int, default=24000,
                    help="cut tool results longer than this before the model sees them")
    ap.add_argument("--no-images", action="store_true", help="do not show the tools' images to the model")
    ap.add_argument("--work", help="the server's folders go here (default: a new temporary folder)")
    ap.add_argument("--out", default="mcp_transcripts", help="where the transcripts go")
    args = ap.parse_args()
    work = os.path.abspath(args.work or tempfile.mkdtemp(prefix="jetcarp-harness-"))
    env = dict(os.environ)
    for k, sub in [("JETCARP_OUT", "out"), ("JETCARP_LIBRARY", "library"), ("JETCARP_DATA_DIR", "data"),
                   ("JETCARP_CONFIG_DIR", "config"), ("JETCARP_RUNTIME_DIR", "runtime")]:
        env[k] = os.path.join(work, sub)
        os.makedirs(env[k], exist_ok=True)
    os.makedirs(args.out, exist_ok=True)
    print(f"server folders in {work}; transcripts in {args.out}")
    tasks = ([("goal", args.goal, lambda mcp, text, calls: (True, "no checker for a goal of your own"))]
             if args.goal else [(n, *TASKS[n]) for n in (args.task or list(TASKS))])
    results = []
    for name, prompt, checker in tasks:
        print(f"== {name}", flush=True)
        r = run_task(name, prompt, checker, args, env)
        print(f"   {'PASS' if r['pass'] else 'FAIL'}: {r['note']} ({r['tool_calls']} calls, "
              f"{r['tool_errors']} errors, {r['seconds']} s)", flush=True)
        results.append(r)
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(results, f, indent=2)
    print(f"{sum(r['pass'] for r in results)}/{len(results)} passed; transcripts in {args.out}")
    return 0 if all(r["pass"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
