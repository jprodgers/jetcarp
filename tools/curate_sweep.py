#!/usr/bin/env python3
"""Ask a local vision model to pick the visually interesting runs of a sweep (optional).

Python standard library only; pictures are laid out by `jetcarp sheet --folder`. For a sweep folder
(manifest.json, run.json, one PNG per run):

1. Preselect candidates from the manifest's descriptors, with no model: drop runs with errors or
   no picture, drop `empty`/`uniform` row classes and pictures where one state fills over 97% of
   the cells, keep one run per final state hash (rules that
   leave the same state draw the same picture; the others are listed as twins), then split the
   rest into strata (row class and row hint, or for non-row rules: exact period found or not, and
   density) and take an even spread over each stratum's PNG sizes (a cheap complexity measure),
   so the model sees nested, regular, periodic, and shifting pictures, not only the busiest
   chaotic ones. Strata get sqrt-proportional quotas; `chaotic` counts half, `periodic` and
   `shifting` 0.6.
2. Round 1: candidates in random (seeded) batches of `--batch`, each shown as one labelled contact
   sheet (A, B, C, ...) with a line of facts per label; the model picks up to `--picks` per batch
   with a one-line reason (JSON).
3. Round 2: all round-1 picks on one or two sheets; the model ranks the best `--promote` (or 3)
   with a short title and a reason.
4. Writes every pick into the folder's `favourites.json` (the format of the app's results
   browser: index, n, rule, file, hash, generations, row_class, period, note, added) with
   `llm_note`, `llm_model`, and `llm_round`; the user's own favourites are kept. With
   `--promote K` the top K go to the Quick Start's "My finds" through
   `jetcarp library promote-run <folder> #<index> --title ... --note ...`.

    python3 tools/curate_sweep.py ~/Documents/jetcarp-out/2026-11-05_17-18-41_HexT3-sum
    python3 tools/curate_sweep.py <folder> --candidates 48 --batch 8 --picks 2 --promote 3 \\
            --library /tmp/lib            # or JETCARP_LIBRARY
    python3 tools/curate_sweep.py <folder> --preselect-only   # list the candidates, no model

A text log (candidates, prompts, answers, timings; no images) goes to tools/curate_logs/.
"""

import argparse
import base64
import collections
import datetime
import json
import math
import os
import random
import shutil
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)

SYSTEM = (
    "You are curating pictures from a cellular-automaton rule sweep for a gallery of the most "
    "visually interesting rules. Each picture is one rule run from the same start. Judge what you "
    "SEE: favour clear structure and variety - nested or self-similar shapes, particles or "
    "gliders that travel and interact, regions of different texture with boundaries between "
    "them, unusual symmetry or patterns that change down the picture. Rate low: plain stripes or "
    "uniform texture, near-empty pictures, and pure noise with no structure. Several pictures may "
    "look alike; prefer the clearest example. Reply with JSON only."
)

ROUND1_SCHEMA = {
    "type": "object",
    "properties": {
        "picks": {"type": "array", "items": {
            "type": "object",
            "properties": {"label": {"type": "string"}, "reason": {"type": "string"}},
            "required": ["label", "reason"]}},
    },
    "required": ["picks"],
}

ROUND2_SCHEMA = {
    "type": "object",
    "properties": {
        "ranking": {"type": "array", "items": {
            "type": "object",
            "properties": {"label": {"type": "string"}, "title": {"type": "string"},
                           "reason": {"type": "string"}},
            "required": ["label", "title", "reason"]}},
    },
    "required": ["ranking"],
}

LABELS = [chr(ord("A") + i) for i in range(26)]


# ---------------------------------------------------------------------------------------------
# Manifest and preselection

def load_manifest(folder):
    rows = {}
    full = os.path.join(folder, "manifest.json")
    if os.path.exists(full):
        for r in json.load(open(full))["rows"]:
            rows[r["index"]] = r
    part = os.path.join(folder, "manifest.partial.jsonl")
    if os.path.exists(part):
        for line in open(part):
            try:
                r = json.loads(line)
                rows[r["index"]] = r
            except (ValueError, KeyError):
                pass
    if not rows:
        raise SystemExit(f"{folder}: no manifest.json (not a sweep folder?)")
    return [rows[i] for i in sorted(rows)]


def stratum(r):
    c = r.get("row_class")
    if c:
        return c + ("/" + r["row_hint"] if r.get("row_hint") else "")
    p = "period" if r.get("period") is not None else "no-period"
    d = r.get("density") or 0
    return f"{p}/density-{'low' if d < 0.15 else 'mid' if d < 0.5 else 'high'}"


def preselect(rows, folder, n, seed):
    """Candidates spread over strata and complexity, one per distinct final state."""
    usable, twins, dropped = [], collections.defaultdict(list), collections.Counter()
    by_hash = {}
    for r in rows:
        if r.get("error"):
            dropped["error"] += 1
        elif not r.get("file") or not os.path.exists(os.path.join(folder, r["file"])):
            dropped["no picture"] += 1
        elif r.get("row_class") in ("empty", "uniform") or r.get("skipped"):
            dropped[r.get("row_class") or r.get("skipped")] += 1
        elif max((v for k, v in r.items() if k.startswith("frac_") and isinstance(v, (int, float))),
                 default=0) > 0.97:
            dropped["one state fills over 97%"] += 1
        elif r.get("hash") in by_hash:
            twins[by_hash[r["hash"]]["index"]].append(r["rule"])
            dropped["same picture as another rule"] += 1
        else:
            by_hash[r.get("hash")] = r
            usable.append(r)
    strata = collections.defaultdict(list)
    for r in usable:
        strata[stratum(r)].append(r)
    # Pure noise and plain stripes are rarely the interesting ones: chaotic, periodic, and
    # shifting strata count less.
    damp = {"aperiodic/chaotic": 0.5, "periodic": 0.6, "shifting": 0.6}
    weight = {k: math.sqrt(len(v)) * damp.get(k, 1.0) for k, v in strata.items()}
    total = sum(weight.values()) or 1
    quota = {k: max(1 if len(strata) > n else 2, round(n * w / total)) for k, w in weight.items()}
    picked = []
    for k, v in sorted(strata.items()):
        v = sorted(v, key=lambda r: (r.get("png_bytes") or 0, r["index"]))
        q = min(quota[k], len(v))
        # Even spread over the complexity range, skipping the lowest tenth (the dullest).
        lo = len(v) // 10 if len(v) >= 10 else 0
        idx = sorted({lo + round(i * (len(v) - 1 - lo) / max(1, q - 1)) for i in range(q)}) if q > 1 else [len(v) * 2 // 3]
        picked += [v[i] for i in idx]
    rng = random.Random(seed)
    if len(picked) > n:
        picked = rng.sample(picked, n)
    rng.shuffle(picked)
    stats = {"runs": len(rows), "distinct pictures": len(usable), "dropped": dict(dropped),
             "strata": {k: len(v) for k, v in sorted(strata.items())}}
    return picked, twins, stats


def facts(r):
    bits = [r["rule"]]
    if r.get("row_class"):
        bits.append(r["row_class"] + (f" ({r['row_hint']})" if r.get("row_hint") else ""))
        if r.get("row_period"):
            bits.append(f"rows repeat every {r['row_period']}" +
                        (f", shifted {r['row_shift']}" if r.get("row_shift") else ""))
    elif r.get("period") is not None:
        bits.append(f"period {r['period']}")
    if r.get("density") is not None:
        bits.append(f"{100 * r['density']:.0f}% live")
    return ", ".join(bits)


# ---------------------------------------------------------------------------------------------
# Model

def post(base_url, body, timeout=1800):
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def ask(args, prompt, png, schema, name):
    with open(png, "rb") as f:
        b64 = base64.b64encode(f.read()).decode()
    content = [{"type": "text", "text": prompt},
               {"type": "image_url", "image_url": {"url": "data:image/png;base64," + b64}}]
    body = {
        "model": args.model, "temperature": 0.3, "max_tokens": args.max_tokens,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": content}],
        "response_format": {"type": "json_schema", "json_schema": {"name": name, "schema": schema, "strict": True}},
    }
    t0 = time.time()
    try:
        r = post(args.base_url, body)
    except urllib.error.HTTPError:
        body.pop("response_format")  # servers without structured output
        r = post(args.base_url, body)
    msg = r["choices"][0]["message"]
    retried = False
    if not (msg.get("content") or "").strip():
        # A thinking model can spend the whole budget reasoning (see tools/curate.py).
        content[0]["text"] += "\n/no_think"
        r = post(args.base_url, body)
        msg = r["choices"][0]["message"]
        retried = True
    usage = r.get("usage") or {}
    return (msg.get("content") or "", msg.get("reasoning_content") or "", time.time() - t0,
            usage.get("completion_tokens"), retried)


def parse_json(text):
    text = re.sub(r"<think>.*?</think>", "", text or "", flags=re.S).strip()
    m = re.search(r"\{.*\}", text, flags=re.S)
    if not m:
        raise ValueError(f"no JSON in answer: {text[:200]!r}")
    return json.loads(m.group(0))


# ---------------------------------------------------------------------------------------------

def sheet(args, folder, rows, png, cols):
    cmd = [args.jetcarp, "sheet", "--folder", folder, "--png", png,
           "--runs", ",".join(f"#{r['index']}" for r in rows),
           "--labels", ",".join(LABELS[: len(rows)]),
           "--cols", str(cols), "--thumb", str(args.thumb), "--label-scale", "4"]
    subprocess.run(cmd, check=True, capture_output=True, text=True)


def save_favourites(folder, jetcarp_version, picks, model):
    path = os.path.join(folder, "favourites.json")
    favs = {}
    if os.path.exists(path):
        try:
            for f in json.load(open(path)).get("favourites", []):
                favs[f["index"]] = f
        except (ValueError, KeyError):
            pass
    now = int(time.time())
    for r, note, rnd in picks:
        f = favs.get(r["index"]) or {k: r[k] for k in
                                      ("index", "n", "rule", "file", "hash", "generations", "row_class", "period")
                                      if k in r}
        f.setdefault("note", "")
        f.setdefault("added", now)
        f["llm_note"] = note
        f["llm_model"] = model
        f["llm_round"] = max(rnd, f.get("llm_round", 0))
        favs[r["index"]] = f
    with open(path, "w") as fh:
        json.dump({"jetcarp_version": jetcarp_version, "sweep": os.path.basename(folder.rstrip("/")),
                   "favourites": [favs[i] for i in sorted(favs)]}, fh, indent=2)
    return path


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder", help="sweep folder (manifest.json, run.json, PNGs)")
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--library", help="library folder for --promote (a folder or .sqlite file; default: $JETCARP_LIBRARY or <Documents>/jetcarp-out/catalog.sqlite)")
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--candidates", type=int, default=48, help="runs shown to the model in round 1")
    ap.add_argument("--batch", type=int, default=8, help="pictures per contact sheet in round 1")
    ap.add_argument("--picks", type=int, default=2, help="most picks per round-1 sheet")
    ap.add_argument("--promote", type=int, default=0, help="promote the top K of round 2 to the Quick Start")
    ap.add_argument("--thumb", type=int, default=400, help="thumbnail size (long side, pixels)")
    ap.add_argument("--seed", type=int, default=1, help="candidate sampling and batch order")
    ap.add_argument("--no-facts", action="store_true", help="show only the pictures, no descriptor line per label")
    ap.add_argument("--max-tokens", type=int, default=8000)
    ap.add_argument("--preselect-only", action="store_true", help="print the candidates and stop (no model)")
    ap.add_argument("--dry-run", action="store_true", help="ask the model but write nothing")
    ap.add_argument("--no-log", action="store_true")
    args = ap.parse_args()
    folder = os.path.abspath(args.folder)
    run = json.load(open(os.path.join(folder, "run.json")))
    rows = load_manifest(folder)
    cands, twins, stats = preselect(rows, folder, args.candidates, args.seed)
    log = [f"# Sweep curation {datetime.datetime.now().isoformat(timespec='seconds')}\n\n",
           f"Folder `{os.path.basename(folder)}`, model `{args.model}`, {len(cands)} candidates "
           f"(batches of {args.batch}, up to {args.picks} picks each, seed {args.seed}).\n\n",
           f"Preselection: {json.dumps(stats)}\n\n## Candidates\n\n"]
    for r in cands:
        log.append(f"- #{r['index']} {facts(r)}; png {r.get('png_bytes')} B"
                   + (f"; same picture as {len(twins[r['index']])} other rules" if twins.get(r["index"]) else "") + "\n")
    print(f"{stats['runs']} runs, {stats['distinct pictures']} distinct pictures; {len(cands)} candidates "
          f"from {len(stats['strata'])} strata: {stats['strata']}")
    if args.preselect_only:
        for r in cands:
            print(f"  #{r['index']:<6} {facts(r)}")
        return 0

    tmp = tempfile.mkdtemp(prefix="jetcarp-curate-sweep-")
    round1, t_all, calls = [], time.time(), []
    batches = [cands[i:i + args.batch] for i in range(0, len(cands), args.batch)]
    for b, rows_b in enumerate(batches):
        png = os.path.join(tmp, f"r1-{b}.png")
        sheet(args, folder, rows_b, png, cols=4 if len(rows_b) > 6 else 3)
        prompt = (f"Contact sheet {b + 1} of {len(batches)}: {len(rows_b)} pictures labelled "
                  f"{LABELS[0]}-{LABELS[len(rows_b) - 1]} (the label is above each picture). Pick at most "
                  f"{args.picks} that are the most visually interesting, or none if all are dull. For each "
                  "give the label and a one-line reason saying what is in the picture.")
        if not args.no_facts:
            prompt += "\n\nFacts (from the run's descriptors):\n" + "\n".join(
                f"{LABELS[i]}: {facts(r)}" for i, r in enumerate(rows_b))
        prompt += '\n\nReply with JSON: {"picks": [{"label": "A", "reason": "..."}]}'
        try:
            answer, reasoning, secs, toks, retried = ask(args, prompt, png, ROUND1_SCHEMA, "picks")
            picks = parse_json(answer).get("picks", [])
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            print(f"batch {b + 1}: model call failed: {e}")
            log.append(f"\n## Round 1, sheet {b + 1}\n\nFailed: {e}\n")
            continue
        calls.append((secs, toks, retried))
        got = []
        for p in picks[: args.picks]:
            lab = str(p.get("label", "")).strip().upper()[:1]
            if lab in LABELS[: len(rows_b)] and all(g[0] is not rows_b[LABELS.index(lab)] for g in got):
                got.append((rows_b[LABELS.index(lab)], " ".join(str(p.get("reason", "")).split())[:300]))
        round1 += got
        print(f"sheet {b + 1}/{len(batches)}: {len(got)} picks in {secs:.0f}s"
              f"{' (retried without thinking)' if retried else ''}: "
              + "; ".join(f"{r['rule']}: {why}" for r, why in got))
        log.append(f"\n## Round 1, sheet {b + 1}\n\n"
                   + "".join(f"- {LABELS[i]} = #{r['index']} {facts(r)}\n" for i, r in enumerate(rows_b))
                   + f"\n**Answer** ({secs:.0f} s, {toks} tokens{', retried with /no_think' if retried else ''}):\n\n"
                   f"```\n{answer.strip()}\n```\n"
                   + (f"\n<details><summary>reasoning (first 1500 chars)</summary>\n\n{reasoning[:1500]}\n\n</details>\n"
                      if reasoning else ""))

    final = []
    if round1:
        top = args.promote or 3
        shown = round1[:20]
        png = os.path.join(tmp, "r2.png")
        sheet(args, folder, [r for r, _ in shown], png, cols=5 if len(shown) > 12 else 4)
        prompt = (f"These {len(shown)} pictures (labelled {LABELS[0]}-{LABELS[len(shown) - 1]}) were picked "
                  f"from a sweep of {stats['runs']} rules. Rank the best {min(top, len(shown))} for a gallery: "
                  "for each give the label, a short title (2-4 words, describing what it looks like), and a "
                  "one-line reason. Prefer variety: do not rank two near-identical pictures.")
        if not args.no_facts:
            prompt += "\n\nFacts:\n" + "\n".join(f"{LABELS[i]}: {facts(r)}; earlier note: {why}"
                                                for i, (r, why) in enumerate(shown))
        prompt += '\n\nReply with JSON: {"ranking": [{"label": "A", "title": "...", "reason": "..."}]}'
        try:
            answer, reasoning, secs, toks, retried = ask(args, prompt, png, ROUND2_SCHEMA, "ranking")
            calls.append((secs, toks, retried))
            for p in parse_json(answer).get("ranking", [])[:top]:
                lab = str(p.get("label", "")).strip().upper()[:1]
                if lab in LABELS[: len(shown)] and all(f[0] is not shown[LABELS.index(lab)][0] for f in final):
                    final.append((shown[LABELS.index(lab)][0], str(p.get("title", "")).strip()[:40],
                                  " ".join(str(p.get("reason", "")).split())[:300]))
            log.append("\n## Round 2\n\n"
                       + "".join(f"- {LABELS[i]} = #{r['index']} {r['rule']}\n" for i, (r, _) in enumerate(shown))
                       + f"\n**Answer** ({secs:.0f} s, {toks} tokens):\n\n```\n{answer.strip()}\n```\n")
            print(f"round 2 ({secs:.0f}s): " + "; ".join(f"{r['rule']} '{t}': {why}" for r, t, why in final))
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            print(f"round 2: model call failed: {e}")
            log.append(f"\n## Round 2\n\nFailed: {e}\n")

    written = []
    if not args.dry_run and round1:
        notes = {r["index"]: (why, 1) for r, why in round1}
        for r, t, why in final:
            notes[r["index"]] = (f"{t}: {why}" if t else why, 2)
        by_index = {r["index"]: r for r, _ in round1}
        path = save_favourites(folder, run.get("jetcarp_version", ""),
                               [(by_index[i], n, rnd) for i, (n, rnd) in notes.items()], args.model)
        written.append(f"favourites: {path} ({len(notes)} runs)")
        for r, t, why in final[: args.promote]:
            cmd = [args.jetcarp, "library"] + (["--library", args.library] if args.library else []) + [
                "promote-run", folder, f"#{r['index']}", "--title", t or r["rule"],
                "--note", f"{why} [picked by {args.model}]"]
            out = subprocess.run(cmd, capture_output=True, text=True)
            written.append(out.stdout.strip() if out.returncode == 0 else f"promote failed: {out.stderr.strip()}")
    secs = time.time() - t_all
    toks = [c[1] for c in calls if c[1]]
    summary = (f"{len(calls)} model calls in {secs:.0f} s ({sum(c[2] for c in calls)} retried without thinking"
               + (f", {sum(toks)} completion tokens" if toks else "") + f"); {len(round1)} round-1 picks, "
               f"{len(final)} ranked")
    print(summary)
    for w in written:
        print(w)
    log.append(f"\n## Result\n\n{summary}\n\n" + "".join(f"- {w}\n" for w in written)
               + "".join(f"\n- ranked {k + 1}: #{r['index']} {r['rule']} '{t}': {why}" for k, (r, t, why) in enumerate(final))
               + "\n")
    shutil.rmtree(tmp, ignore_errors=True)
    if not args.no_log:
        d = os.path.join(HERE, "curate_logs")
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, datetime.datetime.now().strftime("sweep-%Y%m%d-%H%M%S.md"))
        with open(path, "w") as f:
            f.write("".join(log))
        print(f"log: {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
