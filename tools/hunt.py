#!/usr/bin/env python3
"""Autonomous pattern hunts: a local model drives `jetcarp mcp` to find, catalog, and promote finds.

Python standard library only; builds on tools/mcp_harness.py (same MCP client and chat loop).
The model gets every JETCARP tool (run_script, jetcarp_help, library_search, library_list,
library_get, library_update, library_promote, new_world, ...) and an open-ended research goal,
and runs until it answers or runs out of turns. A checker then looks at the library and the
Quick Start folder (not at what the model says) and the transcript is written as text.

    python3 tools/hunt.py --library /tmp/hunt-lib --out /tmp/hunt-out          # all hunts
    python3 tools/hunt.py --task osc-3464 --library /tmp/lib --out /tmp/out
    python3 tools/hunt.py --goal "Find a spaceship in B2/S34 on hexagons and promote it" \\
            --library /tmp/lib --out /tmp/out                                  # your own goal

--library and --out set JETCARP_LIBRARY and JETCARP_OUT for the server, so a hunt never writes into
the real library or Documents. run_script calls ask for MCP progress notifications; the
harness prints them live. Transcripts (Markdown, text only: images are noted, not saved) go to
tools/mcp_transcripts/hunt-<task>.md and a summary to hunt-summary.json.
"""

import argparse
import glob
import json
import os
import sqlite3
import sys
import time
import urllib.error

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from mcp_harness import Mcp, chat, short  # noqa: E402

REPO = os.path.dirname(HERE)

SYSTEM = (
    "You are a research assistant running experiments in JETCARP, a cellular-automaton simulator, "
    "through the tools provided. Work like a scientist: plan briefly, run searches or scripts, "
    "read the results, check claims with tools (never guess ids, periods, or counts), and keep "
    "going until the goal is met. Prefer the library tools for soup searches (library_search) "
    "and run_script for sweeps or custom experiments (read jetcarp_help {\"topic\": \"scripts\"} "
    "first). When you are "
    "done, reply with a short report: what you ran, what you found (ids, periods), and what you "
    "promoted."
)


class HuntMcp(Mcp):
    """MCP client that asks for progress on run_script and prints the notifications."""

    def request(self, method, params):
        self.next_id += 1
        if method == "tools/call" and params.get("name") in ("run_script", "library_search"):
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
                p = r["params"]
                self.progress.append(p)
                print(f"      progress: {p.get('message') or p.get('progress')}", flush=True)
                continue
            if r.get("id") != self.next_id:
                raise RuntimeError(f"unexpected reply {line[:200]}")
            if "error" in r:
                raise RuntimeError(f"{method}: {r['error']}")
            return r["result"]


def catalog(lib):
    """The catalog file for a --library argument (a folder holding library.sqlite, or the file)."""
    return lib if lib.endswith((".sqlite", ".sqlite3", ".db")) else os.path.join(lib, "library.sqlite")


def query(lib, sql):
    path = catalog(lib)
    if not os.path.exists(path):
        return []
    con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql)]
    finally:
        con.close()


def lib_records(lib):
    out = []
    for r in query(lib, "SELECT id, class, period, rule, tiling, state, name, speed FROM patterns"):
        r["tiling"] = json.loads(r["tiling"])
        out.append(r)
    return out


def finds(lib):
    return sorted(r["id"] for r in query(lib, "SELECT id FROM quickstart"))


def tiling_is(rec, fam):
    return rec.get("tiling", {}).get("family") == fam


def check_osc(lib, out, before):
    recs = [r for r in lib_records(lib) if r["class"] == "oscillator" and r["period"] >= 3
            and r["rule"].upper() == "B3/S23" and tiling_is(r, "rhombitrihex")]
    promoted = [r for r in recs if r.get("state") == "promoted"]
    new_finds = [f for f in finds(lib) if f not in before]
    ok = bool(recs) and bool(promoted) and bool(new_finds)
    return ok, (f"{len(recs)} oscillators p>=3 of B3/S23 on 3.4.6.4 in the library "
                f"(periods {sorted({r['period'] for r in recs})}); promoted: "
                f"{[(r['id'][:8], r['period'], r.get('name')) for r in promoted]}; new Quick Start entries {len(new_finds)}")


def check_ship(lib, out, before):
    recs = [r for r in lib_records(lib) if r["class"] == "spaceship" and r["rule"].upper() == "B3/S23"
            and tiling_is(r, "trihex")]
    good = [r for r in recs if r.get("state") == "promoted" and r.get("name")]
    ok = bool(good) and any(f not in before for f in finds(lib))
    return ok, (f"{len(recs)} B3/S23 spaceships on trihex; named and promoted: "
                f"{[(r['id'][:8], r.get('name'), r.get('speed')) for r in good]}")


def check_sweep(lib, out, before):
    # The expected answer from the newest HexT sweep folder the model made.
    folders = sorted(d for d in glob.glob(os.path.join(out, "*")) if os.path.exists(os.path.join(d, "manifest.json")))
    hext = []
    for d in folders:
        rows = json.load(open(os.path.join(d, "manifest.json")))["rows"]
        if len(rows) == 256 and all(str(r.get("rule", "")).startswith("HexT ") for r in rows):
            hext.append((d, rows))
    if not hext:
        return False, f"no 256-rule HexT sweep in {out} (folders: {[os.path.basename(f) for f in folders]})"
    d, rows = hext[-1]
    nested = [r for r in rows if r.get("row_class") == "aperiodic" and r.get("row_hint") == "nested"]
    if not nested:
        return False, "the sweep has no nested rows"
    want = max(nested, key=lambda r: (r.get("png_bytes") or 0, -r["index"]))
    rows = [r for r in query(lib, "SELECT id, title, description, rule, scene FROM quickstart")
            if r["id"] not in before]
    got = [(r.get("description") or r.get("title") or "")[:80] for r in rows]
    ok = any(want["rule"] in " ".join(str(r.get(k) or "") for k in ("title", "description", "rule", "scene"))
             for r in rows)
    return ok, f"expected {want['rule']} (png {want['png_bytes']} B, {len(nested)} nested); promoted: {got}"


def check_job(lib, out, before):
    jobs = query(lib, "SELECT id, kind, status, progress FROM jobs WHERE kind = 'hunt'")
    done = []
    for j in jobs:
        try:
            p = json.loads(j["progress"] or "{}")
        except ValueError:
            p = {}
        if j["status"] == "done" and (p.get("done") or p.get("soups_done") or 0) >= 20000:
            done.append(j["id"])
    ships = [r for r in lib_records(lib) if r["class"] == "spaceship" and tiling_is(r, "snubsq")]
    promoted = [r for r in ships if r.get("state") == "promoted"]
    new_finds = [f for f in finds(lib) if f not in before]
    ok = bool(done) and bool(promoted) and bool(new_finds)
    return ok, (f"finished hunt jobs with >= 20000 soups: {done}; snub square spaceships {len(ships)}; promoted "
                f"{[(r['id'][:8], r.get('speed'), r.get('rule')) for r in promoted]}; new Quick Start entries {len(new_finds)}")


TASKS = {
    "osc-3464": ("Find and catalog oscillators of period 3 or more for Conway's Life (B3/S23) on the "
                 "3.4.6.4 tiling, then promote the most interesting one to the Quick Start.", check_osc),
    "trihex-ship": ("Find a spaceship for Conway's Life on the trihexagonal tiling (3.6.3.6), catalog it, "
                    "give it a descriptive name in the library, and promote it to the Quick Start.", check_ship),
    "hext-sweep": ("Sweep all 256 HexT rules (three-parent rules on flat-top hexagons) from one cell at the "
                   "top until each run is done. In the sweep's manifest, among the runs whose row_class is "
                   "aperiodic and row_hint is nested, find the one with the largest png_bytes, and promote "
                   "that run to the Quick Start.", check_sweep),
    "hunt-job": ("Run a background fast hunt of 20,000 soups for Conway's Life on the snub square tiling "
                 "(3.3.4.3.4). Wait for the job to finish by checking its status, then list the spaceships the "
                 "catalog has for that rule on that tiling and promote the fastest one to the Quick Start.", check_job),
}


def run_hunt(name, prompt, checker, args):
    before = finds(args.library)
    mcp = HuntMcp([args.jetcarp, "mcp"])
    listed = mcp.request("tools/list", {})["tools"]
    tools = [{"type": "function", "function": {"name": t["name"], "description": t["description"],
                                               "parameters": t["inputSchema"]}} for t in listed]
    messages = [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]
    log = [f"# Hunt `{name}`\n\n**Goal:** {prompt}\n\nModel `{args.model}`, {len(tools)} tools.\n"]
    calls, errors, final, t0, tokens = [], 0, "", time.time(), 0
    for turn in range(args.max_turns):
        try:
            r = chat(args.base_url, args.model, messages, tools, args.max_tokens)
        except urllib.error.URLError as e:
            log.append(f"\n**HTTP error:** {e}\n")
            break
        tokens += (r.get("usage") or {}).get("completion_tokens") or 0
        msg = r["choices"][0]["message"]
        reasoning = msg.get("reasoning_content") or msg.get("reasoning")
        if reasoning:
            log.append(f"\n<details><summary>reasoning (turn {turn + 1})</summary>\n\n{short(reasoning, 2000)}\n\n</details>\n")
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
            ts = time.time()
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
                    texts.append("[image attached below]")
                    images.append(c)
            body = "\n".join(texts)
            flag = " (error)" if res.get("isError") else ""
            prog = f", {len(mcp.progress)} progress notifications" if getattr(mcp, "progress", None) else ""
            log.append(f"\n**Tool call** `{fname}` `{short(raw, 800)}`{flag} ({time.time() - ts:.1f} s{prog})\n\n"
                       f"```\n{short(body, 2500)}\n```\n")
            if len(body) > args.max_result_chars:
                # A local model's context is small: never pass a huge result on whole.
                log.append(f"\n(harness: result of {len(body)} chars cut to {args.max_result_chars})\n")
                body = (body[: args.max_result_chars]
                        + f"\n... [{len(body) - args.max_result_chars} more characters cut by the harness]")
            messages.append({"role": "tool", "tool_call_id": tc.get("id", fname), "content": body})
        if images and not args.no_images:
            parts = [{"type": "text", "text": "Image returned by render_image:"}]
            parts += [{"type": "image_url", "image_url": {"url": f"data:{c['mimeType']};base64,{c['data']}"}}
                      for c in images]
            messages.append({"role": "user", "content": parts})
        with open(os.path.join(args.transcripts, f"hunt-{name}.md"), "w") as f:
            f.write("".join(log))
    mcp.close()
    secs = time.time() - t0
    try:
        ok, note = checker(args.library, args.out, before)
    except Exception as e:  # noqa: BLE001 - report checker failures as hunt failures
        ok, note = False, f"checker failed: {e}"
    verdict = "PASS" if ok else "FAIL"
    log.append(f"\n**Result: {verdict}** ({note}). {len(calls)} tool calls, {errors} tool errors, "
               f"{secs:.0f} s, {tokens} completion tokens.\n")
    with open(os.path.join(args.transcripts, f"hunt-{name}.md"), "w") as f:
        f.write("".join(log))
    return {"task": name, "pass": ok, "note": note, "tool_calls": len(calls), "tool_errors": errors,
            "seconds": round(secs), "completion_tokens": tokens, "tools_used": [c for c, _ in calls],
            "final": final.strip()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--task", action="append", choices=sorted(TASKS), help="run only these hunts")
    ap.add_argument("--goal", help="run this goal instead of the built-in hunts (no checker)")
    ap.add_argument("--library", required=True, help="JETCARP_LIBRARY for the server (use a scratch folder)")
    ap.add_argument("--out", required=True, help="JETCARP_OUT for the server (use a scratch folder)")
    ap.add_argument("--max-turns", type=int, default=30)
    ap.add_argument("--max-tokens", type=int, default=12000)
    ap.add_argument("--no-images", action="store_true")
    ap.add_argument("--max-result-chars", type=int, default=24000,
                    help="cut tool results longer than this before the model sees them")
    ap.add_argument("--transcripts", default=os.path.join(HERE, "mcp_transcripts"))
    args = ap.parse_args()
    args.library, args.out = os.path.abspath(args.library), os.path.abspath(args.out)
    os.makedirs(args.library, exist_ok=True)
    os.makedirs(args.out, exist_ok=True)
    os.makedirs(args.transcripts, exist_ok=True)
    os.environ["JETCARP_LIBRARY"], os.environ["JETCARP_OUT"] = args.library, args.out
    hunts = [("goal", args.goal, lambda lib, out, before: (True, "no checker for a custom goal"))] if args.goal \
        else [(n, *TASKS[n]) for n in (args.task or list(TASKS))]
    results = []
    for name, prompt, checker in hunts:
        print(f"== {name}: {prompt}", flush=True)
        r = run_hunt(name, prompt, checker, args)
        print(f"   {'PASS' if r['pass'] else 'FAIL'}: {r['note']} ({r['tool_calls']} calls, "
              f"{r['tool_errors']} errors, {r['seconds']} s)", flush=True)
        results.append(r)
    with open(os.path.join(args.transcripts, "hunt-summary.json"), "w") as f:
        json.dump(results, f, indent=2)
    print(f"{sum(r['pass'] for r in results)}/{len(results)} passed; transcripts in {args.transcripts}")
    return 0 if all(r["pass"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
