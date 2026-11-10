#!/usr/bin/env python3
"""Ask a local model to curate new pattern-library entries (optional; nothing depends on it).

Python standard library only. For each library entry in state `new` (highest score first):

1. `jetcarp library show <id> --json` for the facts and `jetcarp library render <id> --ascii` for a
   text drawing (square and hexagon grids); `jetcarp library render <id> --png` for a picture.
2. Ask the model on an OpenAI-compatible API (LM Studio) for a short name, tags, a one-paragraph
   note, and an interest rating 1-5, as JSON. The picture is sent only when the model accepts
   images (LM Studio reports type "vlm", or a probe image is accepted); otherwise the prompt is
   text only: class, period, speed, cells, rarity, and the drawing.
3. Write the answer back with `jetcarp library update`: the notes get a paragraph marked
   "[LLM-suggested by <model>, <date>]", the tags gain `llm-reviewed`, the rating is set, the
   state becomes `reviewed`. The model's name is used only when the entry has none (known names
   such as "glider" are kept; the suggestion goes into the note). `discovered_by` is left alone:
   it records who found the pattern, not who described it.

    python3 tools/curate.py --limit 5                      # default library ($JETCARP_LIBRARY)
    python3 tools/curate.py --library /tmp/lib --dry-run   # show what would be written
    python3 tools/curate.py --base-url http://localhost:1234/v1 --model qwen/qwen3.8-27b

A log (prompts, answers, what was written) goes to tools/curate_logs/ unless --no-log.
"""

import argparse
import base64
import datetime
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
REPO = os.path.dirname(HERE)

SYSTEM = (
    "You are a curator of a catalog of cellular-automaton patterns (like the LifeWiki pattern "
    "pages). Each pattern was found by a computer search and verified exactly. Given the facts and "
    "a drawing, propose a short evocative name (1-3 words, lowercase unless a proper noun; do not "
    "reuse a well-known Game of Life name unless the pattern IS that object in Life on the square "
    "grid), 2-5 short tags, a one-paragraph note (2-4 sentences: what it is, what is notable about "
    "its shape or behaviour, how common it is), and an interest rating from 1 (dull, very common) "
    "to 5 (remarkable: e.g. a spaceship on an unusual tiling, a high-period oscillator). Be "
    "factual: describe only what the facts and drawing show. Every pattern was verified alone on an "
    "unbounded empty plane: there are no edges and no wrap-around. Reply with JSON only."
)

SCHEMA = {
    "type": "object",
    "properties": {
        "name": {"type": "string"},
        "tags": {"type": "array", "items": {"type": "string"}},
        "note": {"type": "string"},
        "rating": {"type": "integer", "minimum": 1, "maximum": 5},
    },
    "required": ["name", "tags", "note", "rating"],
}

# A 1x1 PNG for the image probe.
PROBE_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==")


def jetcarp(args, *cmd, check=True):
    full = [args.jetcarp, "library"] + (["--library", args.library] if args.library else []) + list(cmd)
    r = subprocess.run(full, capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(full)}: {r.stderr.strip() or r.stdout.strip()}")
    return r.stdout


def post(base_url, body, timeout=900):
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def accepts_images(args):
    """LM Studio's model list says "vlm" for vision models; otherwise send a probe image."""
    if args.images != "auto":
        return args.images == "yes"
    root = args.base_url.rstrip("/").rsplit("/v1", 1)[0]
    try:
        with urllib.request.urlopen(root + "/api/v0/models", timeout=10) as r:
            for m in json.loads(r.read()).get("data", []):
                if m.get("id") == args.model and m.get("type"):
                    return m["type"] == "vlm"
    except (urllib.error.URLError, ValueError, OSError):
        pass
    try:
        post(args.base_url, {"model": args.model, "max_tokens": 5, "messages": [{"role": "user", "content": [
            {"type": "text", "text": "Reply OK."},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode(PROBE_PNG).decode()}},
        ]}]}, timeout=300)
        return True
    except (urllib.error.URLError, OSError):
        return False


def facts(rec):
    lines = [
        f"Rule: {rec['rule']} ({rec['neighborhood']} neighbourhood)",
        f"Tiling: {rec['tiling_name']} ({rec['tile_types']} tile type(s) per unit cell)",
        f"Class: {rec['class'].replace('_', ' ')}, period {rec['period']}",
    ]
    if rec.get("speed"):
        lines.append(f"Speed: {rec['speed']} (moves {rec.get('displacement')} lattice steps per period)")
    lines += [
        f"Live cells: {rec['cells']} (between {rec['min_cells']} and {rec['max_cells']} over its phases), "
        f"bounding box {rec['bbox'][2] + 1} x {rec['bbox'][3] + 1}",
        f"Times found: {rec['seen']} (in soup searches and collisions; a rough rarity measure)",
        f"Verified exactly for {rec['verified']['periods']} periods",
        f"Interest score from the heuristic: {rec['score']} / 100",
    ]
    if rec.get("name"):
        lines.append(f"Current name: {rec['name']} (keep it if it is a standard name)")
    return "\n".join(lines)


def parse_answer(text):
    text = re.sub(r"<think>.*?</think>", "", text or "", flags=re.S).strip()
    m = re.search(r"\{.*\}", text, flags=re.S)
    if not m:
        raise ValueError(f"no JSON in answer: {text[:200]}")
    a = json.loads(m.group(0))
    name = str(a.get("name", "")).strip()[:40]
    tags = [re.sub(r"[^a-z0-9 -]", "", str(t).lower()).strip()[:24] for t in a.get("tags", [])][:5]
    note = " ".join(str(a.get("note", "")).split())[:900]
    rating = max(1, min(5, int(a.get("rating", 3))))
    return {"name": name, "tags": [t for t in tags if t], "note": note, "rating": rating}


def ask(args, rec, ascii_art, png_path, images):
    prompt = "Pattern facts:\n" + facts(rec)
    if ascii_art.strip():
        prompt += ("\n\nText drawing ('O' live, '.' empty" +
                   ("; hexagon rows are offset by half a cell" if rec["tiling"]["family"] == "hex" else "") +
                   "):\n" + ascii_art.strip())
    if images and png_path:
        prompt += "\n\nThe picture shows each phase side by side."
    prompt += "\n\nReply with JSON: {\"name\": ..., \"tags\": [...], \"note\": ..., \"rating\": 1-5}"
    content = prompt
    if images and png_path:
        with open(png_path, "rb") as f:
            b64 = base64.b64encode(f.read()).decode()
        content = [{"type": "text", "text": prompt},
                   {"type": "image_url", "image_url": {"url": "data:image/png;base64," + b64}}]
    body = {
        "model": args.model, "temperature": 0.4, "max_tokens": args.max_tokens,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": content}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "curation", "schema": SCHEMA, "strict": True}},
    }
    t0 = time.time()
    try:
        r = post(args.base_url, body)
    except urllib.error.HTTPError:
        body.pop("response_format")  # servers without structured output
        r = post(args.base_url, body)
    msg = r["choices"][0]["message"]
    if not (msg.get("content") or "").strip():
        # A thinking model can spend the whole token budget reasoning and return no answer
        # (qwen3 did on 2 of 6 entries at 4,000 tokens). Ask again without thinking.
        text = body["messages"][1]["content"]
        if isinstance(text, list):
            text[0]["text"] += "\n/no_think"
        else:
            body["messages"][1]["content"] = text + "\n/no_think"
        r = post(args.base_url, body)
        msg = r["choices"][0]["message"]
    return prompt, msg.get("content") or "", msg.get("reasoning_content") or "", time.time() - t0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--library", help="library folder (a folder or .sqlite file; default: $JETCARP_LIBRARY or <Documents>/jetcarp-out/catalog.sqlite)")
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--limit", type=int, default=5)
    ap.add_argument("--ids", nargs="*", help="curate these entries (any state) instead of the top new ones")
    ap.add_argument("--images", choices=["auto", "yes", "no"], default="auto")
    ap.add_argument("--max-tokens", type=int, default=8000)
    ap.add_argument("--dry-run", action="store_true", help="ask the model but write nothing back")
    ap.add_argument("--no-log", action="store_true")
    args = ap.parse_args()

    if args.ids:
        recs = [json.loads(jetcarp(args, "show", i, "--json")) for i in args.ids]
    else:
        recs = json.loads(jetcarp(args, "list", "--state", "new", "--json", "--limit", str(args.limit)))
    if not recs:
        print("no new entries to curate")
        return 0
    images = accepts_images(args)
    print(f"model {args.model} at {args.base_url}; images {'yes' if images else 'no (text only)'}; {len(recs)} entries")
    today = datetime.date.today().isoformat()
    log = [f"# Curation run {datetime.datetime.now().isoformat(timespec='seconds')}\n",
           f"Model `{args.model}`, images: {images}.\n"]
    tmp = tempfile.mkdtemp(prefix="jetcarp-curate-")
    done = 0
    for rec in recs:
        rid = rec["id"]
        ascii_art = jetcarp(args, "render", rid, "--ascii", "--max-phases", "4")
        if re.match(r"phase 0: \S", ascii_art):
            ascii_art = ""  # no drawing for this tiling (raw cell lists are not useful to the model)
        png = os.path.join(tmp, rid + ".png")
        try:
            jetcarp(args, "render", rid, "--png", png, "--px", "140", "--max-phases", "8")
        except RuntimeError:
            png = None
        try:
            prompt, answer, reasoning, secs = ask(args, rec, ascii_art, png, images)
            a = parse_answer(answer)
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            print(f"{rid}: model call failed: {e}")
            log.append(f"\n## {rid}\n\nFailed: {e}\n")
            continue
        keep_name = bool(rec.get("name"))
        note = f"[LLM-suggested by {args.model}, {today}] {a['note']}"
        if keep_name and a["name"] and a["name"].lower() != rec["name"].lower():
            note += f" (Suggested name: {a['name']}.)"
        upd = ["update", rid, "--append-notes", note, "--rating", str(a["rating"]), "--state", "reviewed",
               "--add-tag", "llm-reviewed"]
        for t in a["tags"]:
            upd += ["--add-tag", t]
        if not keep_name and a["name"]:
            upd += ["--name", a["name"]]
        print(f"{rid}  {rec.get('name') or '(unnamed)'} -> name {a['name']!r}, rating {a['rating']}, "
              f"tags {a['tags']} ({secs:.0f}s)\n    {a['note']}")
        if not args.dry_run:
            jetcarp(args, *upd)
            done += 1
        log.append(f"\n## {rid} ({rec['class']}, {rec['rule']} on {rec['tiling_name']})\n\n"
                   f"**Prompt:**\n\n```\n{prompt}\n```\n\n**Answer** ({secs:.0f}s):\n\n```\n{answer.strip()}\n```\n\n"
                   + (f"<details><summary>reasoning</summary>\n\n{reasoning[:3000]}\n\n</details>\n\n" if reasoning else "")
                   + f"**Written:** {'nothing (dry run)' if args.dry_run else ' '.join(upd[2:])}\n")
    if not args.no_log:
        d = os.path.join(HERE, "curate_logs")
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, datetime.datetime.now().strftime("curate-%Y%m%d-%H%M%S.md"))
        with open(path, "w") as f:
            f.write("".join(log))
        print(f"log: {path}")
    print(f"{done} entr{'y' if done == 1 else 'ies'} updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
