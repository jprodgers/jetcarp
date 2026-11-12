# Helpers for local language models

Two optional Python scripts for working with JETCARP and a language model that runs on your own
computer, behind an OpenAI-compatible API (LM Studio, llama.cpp's server, vLLM, Ollama's OpenAI
endpoint). Nothing in JETCARP needs them, and they are not installed with it.

Both need only Python 3.9 or newer and its standard library, the `jetcarp` program (by default
`target/release/jetcarp` in this repository; `--jetcarp PATH` for another), and a model server
(by default `http://localhost:1234/v1`; `--base-url` and `--model` to change them).

| Script | What it is for |
| --- | --- |
| `mcp_harness.py` | Tries a model on JETCARP's MCP tools: for each task (a glider, Brian's Brain, Langton's ant, a sandpile, Life on triangles, a hunt and its spaceships) it starts `jetcarp mcp`, offers the model the tools, runs the tool-calling loop, checks the result through the tools themselves, and writes a transcript. `--goal "..."` runs a goal of your own. The server's folders go in a scratch folder, so your library and pictures are never touched. |
| `curate.py` | Asks a model to curate new library patterns: a short name, tags, a note and a rating for each, written back with `jetcarp library update` (notes marked as the model's, tag `llm-reviewed`). `--dry-run` writes nothing; `--library` picks a library other than your own. |

```text
$ python3 contrib/llm/mcp_harness.py --task glider --model qwen/qwen3.8-27b
$ python3 contrib/llm/curate.py --library /path/to/a/copy --limit 5 --dry-run
```

To drive JETCARP from an assistant such as Claude Code or Claude Desktop instead, you need neither:
see the book's tutorial *Driving JETCARP from an AI assistant* (`docs/src/tutorials/mcp.md`).
