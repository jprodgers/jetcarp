# MCP tools

JETCARP is an MCP server (the [Model Context Protocol](https://modelcontextprotocol.io)), so an AI
assistant can build worlds, run them, look at them, search for patterns and read the library
through the same operations the app and the command line use. This page lists every tool with its
arguments and an example, and the resources the server offers. For a walk through a first session,
see [Driving JETCARP from an AI assistant](../tutorials/mcp.md).

## Connecting

The server runs over standard input and output: the assistant starts `jetcarp mcp` and talks to it
as long as it runs.

- **Claude Code:** `claude mcp add jetcarp -- jetcarp mcp`
- **Claude Desktop:** add JETCARP to `claude_desktop_config.json` (Settings, Developer, Edit
  Config) and restart Claude Desktop:

  ```json
  {
    "mcpServers": {
      "jetcarp": {"command": "jetcarp", "args": ["mcp"]}
    }
  }
  ```

  If Claude Desktop does not find the program, give its full path as `command` (`which jetcarp`
  prints it; on Windows `where jetcarp`).
- **Any other client** (LM Studio, editors with MCP support): a stdio server whose command is
  `jetcarp` with the argument `mcp`. The standalone `jetcarp-mcp` program takes the same options,
  without the GPU (it builds as a static binary for sandboxed apps).

`jetcarp mcp` takes these options:

| Option | Meaning |
| --- | --- |
| `--app` | Start attached to the running JETCARP app, so the world tools act on the world in its window (switch on *AI control* there first). The `use_app` tool does the same during a session |
| `--allow DIR` | Let the tools read and write in this folder too (repeatable) |
| `--port N`, `--token T` | The app's port and token, when they are not the ones it recorded |

## What the tools may do

- **Files.** The tools read and write files only in JETCARP's output folder (`JETCARP_OUT`, else
  `<Pictures>/JETCARP`), the library's folder, and folders given with `--allow`; they also read the
  packs that came with JETCARP. Relative paths start in the output folder. A path outside these is
  refused with an error that names the folders. See
  [Folders and environment variables](folders.md).
- **Limits per call.** Worlds of at most 4 million cells; at most 1,000,000 generations and 2
  billion cell updates (cells times generations) per `world_step`; scripts of at most 600 seconds
  (60 by default); waits for a job of at most 600 seconds. Long searches run as background jobs
  instead, with no such limits: they stop when the server exits and stay resumable, or run in a
  process of their own with `detach`.
- **What is not offered.** Making the publisher's signing key (`jetcarp pack key`) and recording a
  rater's consent to publish their ratings (`jetcarp pack rater`) are for people only, at a command
  line.
- **Cancelling.** A call the client cancels (`notifications/cancelled`) stops soon: scripts, steps,
  waits and compounds passes. With a progress token, scripts' sweeps and waits report progress.

Every tool's arguments come from the input types of the operations it calls, the same ones the
command line and scripts read, so the three cannot drift apart. Small models' habits are forgiven:
a number or `true` given as text, a list as comma-separated text, or an action's options as JSON
text are read as meant.

A client hands the whole tool list to the model with every request, so the list stays short
(about 45 KB): an argument with many settings, such as each of `search_start`'s strategies or
`pack`'s actions, is listed with a line about what it does and the resource that holds its
settings in full, `jetcarp://schema/{tool}/{argument}` (for example
`jetcarp://schema/search_start/hunt`). A client without resources gets the same JSON Schema from
`help {"schema": "search_start/hunt"}`, and a setting the server refuses is answered with the
resource that lists the right ones. The tables below show every setting.

The rest of this page is generated from the server's own `tools/list`, `resources/list` and
settings resources by `docs/tools/gen-mcp.py`.

<!-- BEGIN GENERATED: docs/tools/gen-mcp.py -->

The server calls itself `jetcarp` (JETCARP cellular automata) and offers 21 tools and 113 resources. Its instructions to the client:

> JETCARP runs cellular automata (the Game of Life and many other rules) on square, hexagon, triangle, Archimedean, k-uniform and aperiodic (Spectre, hat, Penrose) tilings, and keeps a library of the patterns its searches find. Typical flow: world_new (a tiling and rules) -> world_edit (draw or paste) -> world_step -> world_read / world_render. Long searches: search_start, job_status, then library_query for the finds. Call help for worked examples, reference for rule names, tilings, palettes and seeds. The JETCARP book is available as resources (jetcarp://docs/...) and through help {"page": ...}.

## Tools

| Tool | What it does |
| --- | --- |
| [`help`](#help) | JETCARP help and worked examples |
| [`reference`](#reference) | Rules, tilings, palettes, seeds, commands |
| [`world_new`](#world_new) | Make or open a world |
| [`world_status`](#world_status) | World status |
| [`world_read`](#world_read) | Read cells, a cell, or the objects |
| [`world_edit`](#world_edit) | Draw, paste, reseed, or apply commands |
| [`world_step`](#world_step) | Run generations |
| [`world_render`](#world_render) | Render a picture |
| [`run_script`](#run_script) | Run a JETCARP script |
| [`library_query`](#library_query) | List or read library records |
| [`library_curate`](#library_curate) | Curate library records |
| [`search_start`](#search_start) | Start or resume a search |
| [`job_status`](#job_status) | Job status, finds and report |
| [`job_stop`](#job_stop) | Stop a job |
| [`search_runs`](#search_runs) | An explore search's runs |
| [`media_render`](#media_render) | Render animations and posters |
| [`pack`](#pack) | Packs and the website export |
| [`files_list`](#files_list) | List output files |
| [`use_app`](#use_app) | Attach to the JETCARP app |
| [`host_control`](#host_control) | JETCARP background host |
| [`app_display`](#app_display) | The app's displays and screensaver |

### `help`

**JETCARP help and worked examples.** Start here if you are new to JETCARP. Explains what JETCARP is and how these tools fit together, with worked examples you can copy. `topic`: "overview" (default: concepts, coordinates, a glider in Life, Brian's Brain, a rule on hexagons, a sandpile), "world" (drawing, pasting, reseeding, commands, objects), "library" (finding, reading and curating patterns, ant runs and Quick Start entries), "searches" (long searches as background jobs: start, wait, read the finds, stop, resume), "scripts" (every run_script function), "packs" (checking, importing and publishing packs), "setup" (this installation as its user set it up: profile, folders, packs, GPU, library; read only, since setup is the user's choice) or "pages" (the pages of the JETCARP book). `page` returns one book page as Markdown, e.g. {"page": "guides/searching"} (the resources jetcarp://docs/{page}). `schema` returns the settings of a tool argument that names a jetcarp://schema/ resource, e.g. {"schema": "search_start/hunt"}.

*reads only*

| Argument | Type | Meaning |
| --- | --- | --- |
| `page` | string | A page of the JETCARP book by its path without .md, e.g. "guides/searching" (topic pages lists them). |
| `schema` | string | A JSON Schema: a tool argument's settings ("search_start/hunt" is jetcarp://schema/search_start/hunt), "scene.json" or "pack.json". |
| `topic` | `"overview"`, `"world"`, `"library"`, `"searches"`, `"scripts"`, `"packs"`, `"setup"`, `"pages"` | Which guide (default overview). |

Example:

```json
{
  "name": "help",
  "arguments": {
    "topic": "searches"
  }
}
```

### `reference`

**Rules, tilings, palettes, seeds, commands.** Lists what there is to choose from. `what`: "rules" (default: the named rules with notation, family and a line each, and how rule notation works; any name or notation works as a CA's `rule`, e.g. "Life", "B36/S23", "Brian's Brain", "W30", "Ant RL"), "tilings" (tiling ids for world_new and their neighbour counts; with `search`, catalog tilings such as "k2-005" or "3.4.6.4"), "isohedral" (Kaplan's 81 isohedral tile types), "palettes" (built-in palettes and their colours), "seeds" (the forms a CA's `seed` takes), or "commands" (the command objects world_edit's `commands` applies). Examples: {"what": "rules", "family": "Generations"}, {"what": "tilings", "search": "k2-"}.

*reads only*

| Argument | Type | Meaning |
| --- | --- | --- |
| `family` | string | rules: only this family (part of its name, any case), e.g. "Life-like", "Generations", "Agents". |
| `limit` | integer | tilings: With `search`: most matches listed (default 60). |
| `search` | string | tilings: List the tilings (families, catalog tilings, isohedral types) whose id or name contains this text, any case, e.g. "k2-", "3.4.6.4", "ih4". Default: the tiling families. |
| `what` | `"rules"`, `"tilings"`, `"isohedral"`, `"palettes"`, `"seeds"`, `"commands"` | What to list (default rules). |

Example:

```json
{
  "name": "reference",
  "arguments": {
    "what": "tilings",
    "search": "k2-"
  }
}
```

### `world_new`

**Make or open a world.** Replaces the current world. Build one from a tiling (the grid of cells) and the CAs (cellular automata) that run on it: {"tiling": "hex", "width": 60, "height": 60, "boundary": "wrap", "cas": ["B24/S35"]}. Each CA starts from its rule's default seed (a random soup for Life-like rules) unless given one: {"cas": [{"rule": "Life", "seed": "empty"}]} is a blank field to draw on (reference {"what": "seeds"} lists the seeds, {"what": "tilings"} the tilings). Size: width and height (8 to 4096 unit cells each), or `cells` (about that many, up to 4000000), or `level` on aperiodic tilings. Or open one: `quickstart` (a Quick Start entry by handle, e.g. "life"), `pattern` (a library pattern on a grid sized for it, e.g. "glider"), `scene` (scene JSON, as world_status {"scene": true} returns it) or `path` (a scene file). Returns the world's status with each CA's id. Next: world_edit, world_step, world_read, world_render.

*may replace or delete something*

| Argument | Type | Meaning |
| --- | --- | --- |
| `boundary` | string | "wrap" (default), "dead", or "live" (aperiodic tilings are always dead). |
| `cas` | any | The CAs, in order: rule names or notation ("Life", "B3/S23"), or CA objects as in a scene ({"rule": "Life", "seed": "random:0.2"}); one string is one CA. |
| `cells` | integer | Instead of width and height: about this many cells in a roughly square region. |
| `height` | integer | Grid height in unit cells. |
| `level` | integer | Aperiodic tilings: the substitution level. |
| `name` | string | Scene name. |
| `path` | string | A scene .json file. |
| `pattern` | string | A library pattern (id, prefix or name, e.g. `glider`) alone on a wrapped grid sized for it, in a rule it runs in. |
| `quickstart` | string | A Quick Start entry by handle (`life`, `find:<id>`, `atlas:<...>`, `qs-<hex>`): the scene the app's Quick Start opens. |
| `rng_seed` | integer or string | Random seed (default 1): the same seed gives the same run. Any `u64`; beyond 2^53 - 1 also as a string of decimal digits. |
| `scene` | any | Scene JSON: the object, or its text. |
| `tiling` | string | Tiling id (reference {"what": "tilings"}): "square" (default), "hex", "triangle", "cairo", "spectre" (aperiodic), a vertex configuration such as "3.4.6.4", or a catalog id such as "k2-005". |
| `tiling_params` | any | Advanced: tile shape parameters (JETCARP tile JSON `params`). |
| `width` | integer | Grid width in unit cells (with height). |

Example:

```json
{
  "name": "world_new",
  "arguments": {
    "tiling": "hex",
    "width": 60,
    "height": 60,
    "boundary": "wrap",
    "cas": [
      "B24/S35"
    ]
  }
}
```

### `world_status`

**World status.** Reports the current world: tiling and grid size (width x height for coordinates), generation, state hash, whether it is running, empty, frozen, or repeating with a period (checked exactly), population per field, and the CAs with their ids and rules. Cheap; call it whenever you need ids or sizes. {"scene": true} returns the world's scene JSON instead (world_new {"scene": ...} reproduces this exact state); {"save_path": "glider.json"} also writes it to a file (the app and `jetcarp run` open it).

| Argument | Type | Meaning |
| --- | --- | --- |
| `save_path` | string | The .json file to write (its folder is created). |
| `scene` | boolean | Also return the scene: tiling, CAs, random seed, and the command log since the last reset (opening it reproduces this exact state). |

Example:

```json
{
  "name": "world_status",
  "arguments": {
    "save_path": "scenes/hex.json"
  }
}
```

### `world_read`

**Read cells, a cell, or the objects.** Looks at a CA's field. With no arguments: a text picture of the smallest box around the non-empty cells (one row per line, prefixed by its row number y, with a ruler of column numbers; '.' empty, 'O' live, digits for higher states or sandpile grains), the live-cell count and the bounding box. x, y, width, height show a fixed region; {"format": "list"} gives [x, y, state] triples instead. {"format": "objects"} runs the object finder: every still life, oscillator, spaceship and gun in the field, named and classed, with where it is (`catalog`: true also stores each distinct one in the library). {"cell": {"x": 3, "y": 4}} describes one cell: its tile type, centre, value in every field, and its neighbours in clockwise order (the neighbour layout on hexagons, triangles and other tilings, before you draw).

| Argument | Type | Meaning |
| --- | --- | --- |
| `ca` | integer or string | Which CA: its id number or its name (world_status lists them). Default: the first CA. |
| `catalog` | boolean | With format objects: Also verify each distinct still life, oscillator and spaceship and add it to the library (copies add to its seen count), as a search would. |
| `cell` | object | One cell to describe instead: {"x", "y"} or {"index"}. Settings: `jetcarp://schema/world_read/cell`. |
| `cell.index` | integer | Cell index (instead of x, y; required on aperiodic tilings). |
| `cell.x` | integer | Column. |
| `cell.y` | integer | Row. |
| `format` | `"picture"`, `"list"`, `"objects"` | "picture" (default), "list" of non-empty cells as [x, y, state], or "objects" (the object finder). |
| `height` | integer | Region height (default 40 with x, y; at most 200). |
| `max_period` | integer | With format objects: Longest period looked for (default 64, at most 1024). |
| `width` | integer | Region width (default 40 with x, y; at most 200). |
| `x` | integer | Left column of the region (default: the box around every non-empty cell). |
| `y` | integer | Top row of the region. |

Example:

```json
{
  "name": "world_read",
  "arguments": {
    "format": "objects"
  }
}
```

### `world_edit`

**Draw, paste, reseed, or apply commands.** Changes the world through logged commands (world_status {"scene": true} replays them). Give one of: `cells` to draw a text picture, RLE, [x, y] cells or cell indices, e.g. {"cells": {"pattern": ".O.\n..O\nOOO", "x": 10, "y": 10}} (a glider moving down-right; x = column, y = row, (0, 0) is the top-left cell); `paste` to place a library pattern, e.g. {"paste": {"id": "glider", "x": 10, "y": 10, "orientation": 1}}; `reseed` to put new life into a settled world, now ({"reseed": {"mode": "blobs"}}) or by policies; or `commands`, the app's own commands in order, e.g. {"commands": [{"op": "add_ca", "ca": {"rule": "Ant RL", "seed": "ants:3"}}]}; reference {"what": "commands"} lists them. `ca` picks the CA (id or name; default the first) for cells, paste and reseed. Next: world_step, world_read.

| Argument | Type | Meaning |
| --- | --- | --- |
| `ca` | integer or string | Which CA: its id number or its name (world_status lists them). Default: the first CA. |
| `cells` | object | Cells to draw into a CA's field: one of `pattern`, `rle`, `cells` or `indices` (or only `clear_first`). Only the live cells of a pattern are written; `clear_first` empties the field first, and `erase` clears the cells instead. On sandpiles the values are grains added. Settings: `jetcarp://schema/world_edit/cells`. |
| `cells.cells` | any | Cells as [x, y] or [x, y, state] (or {"x", "y", "state"}). |
| `cells.clear_first` | boolean | Empty the whole field before drawing. |
| `cells.erase` | boolean | Set the listed or pattern cells to empty instead. |
| `cells.indices` | any | Cells by index, as index or [index, state] (the only form on aperiodic tilings). |
| `cells.pattern` | string | Text picture, one line per row: '.' empty, 'O' (or '*', '#', 'X') live, '1'-'9' states. |
| `cells.rle` | string | Run-length encoded pattern (LifeWiki RLE; header lines are ignored). |
| `cells.state` | integer | State for cells given without one (default 1). |
| `cells.x` | integer | Column of the pattern's top-left corner (default: centred). |
| `cells.y` | integer | Row of the pattern's top-left corner (default: centred). |
| `commands` | array of object | Command objects, each with an "op" field (one object, or their JSON text, works too): add_ca, remove_ca, update_ca, seed, clear, set_cells, step, reset, set_rng_state, set_tiling, sandpile, set_palette, reseed, set_reseed, ... |
| `paste` | object | A library pattern to paste, and where. The world must use the pattern's tiling; on aperiodic patches give `index`, or `at` "original" (where it was found). Settings: `jetcarp://schema/world_edit/paste`. |
| `paste.at` | string | "center" (default on lattices) or "original" (aperiodic: where it was found; the default there). |
| `paste.id` (required) | string | Pattern id, unique id prefix, or name ("glider"). |
| `paste.index` | integer | Cell index for the corner (lattice) or the anchor tile (aperiodic patches). |
| `paste.orientation` | integer | Index into the tiling's symmetries (0 = as stored). |
| `paste.phase` | integer | Which phase to paste (default 0). |
| `paste.x` | integer | Column of the pattern's top-left corner (with y). |
| `paste.y` | integer | Row of the pattern's top-left corner (with x). |
| `reseed` | object | Reseed now, set the automatic policies, or report them. `mode` reseeds now; with neither `mode` nor `policies` it reports the policies and the recent activity. Policies: {"policies": [{"mode": "rain", "when": {"kind": "settled"}, "cooldown": 300}]}. Settings: `jetcarp://schema/world_edit/reseed`. |
| `reseed.activity_gens` | integer | Recent generations of activity to report (default 64, at most 512). |
| `reseed.mode` | any | Reseed now with this mode: "random", "rain", "blobs", a name and a sign ("rain:pos"), or an object such as {"kind": "rain", "drops": 30}. |
| `reseed.policies` | any | Replace the automatic policies ([] clears them). |

Example:

```json
{
  "name": "world_edit",
  "arguments": {
    "cells": {
      "pattern": ".O.\n..O\nOOO",
      "x": 10,
      "y": 10
    }
  }
}
```

### `world_step`

**Run generations.** Advances the world by `generations` (default 1; at most 1,000,000 per call, and at most 2 billion cell updates, cells x generations); every enabled CA steps together. Returns the new generation, populations and status (running, empty, frozen, or repeating with its period). {"stop_when_settled": true} stops as soon as the world is empty, frozen or exactly periodic (up to 5000 generations when no count is given). Stops early when the client cancels the call.

| Argument | Type | Meaning |
| --- | --- | --- |
| `generations` | integer | Generations to run (default 1, or up to 5000 with `stop_when_settled`). |
| `stop_when_settled` | boolean | Stop as soon as the world is empty, frozen, or exactly periodic. |

Example:

```json
{
  "name": "world_step",
  "arguments": {
    "generations": 200,
    "stop_when_settled": true
  }
}
```

### `world_render`

**Render a picture.** Renders the whole world as a PNG and returns it, so you (and the user) can see it: empty cells dark, live cells in the CA's palette, brighter for young cells. `save_path` also writes the PNG (scene embedded). {"wallpaper": {"size": "4K"}} writes desktop-sized pictures instead and returns their paths (also on a fresh grid sized for the picture, or one picture per monitor, cut so they line up).

| Argument | Type | Meaning |
| --- | --- | --- |
| `max_side` | integer | Longest image side in pixels, 64 to 2048 (default 640). |
| `outlines` | boolean | Draw tile outlines (default: only when tiles are at least 7 px). |
| `save_path` | string | Also write the PNG to this file (scene embedded; its folder is created). |
| `wallpaper` | object | Write desktop-sized pictures of the world instead. They go under the output folder's wallpapers/ unless `out` is given. E.g. {"size": "4K"}; {"size": "2560x1440", "fresh": true, "cell_px": 3, "gens": 300, "palette": "Paper"} (a new grid of the world's tiling sized for the picture); {"monitors": "2560x1440+0+0,1920x1080+2560+180", "split": true} (one picture per monitor, cut so they line up). Settings: `jetcarp://schema/world_render/wallpaper`. |
| `wallpaper.cell_px` | number | Tile size in pixels for `fresh` and `fit` repeat (default 4). |
| `wallpaper.center` | array of number | World point at the image centre (with `scale`) instead of fitting. |
| `wallpaper.fit` | string | cover (default: fill and crop, no stage showing), contain (the whole grid, letterboxed), or repeat (a wrap-edge grid tiled at `cell_px`). |
| `wallpaper.fresh` | boolean | Render a new grid of the world's tiling sized to fill the image at `cell_px` (its CAs and seeds, without its edits). |
| `wallpaper.gens` | integer | Generations to run first (on the host's world: only with `fresh`). |
| `wallpaper.monitors` | string | Monitors as WIDTHxHEIGHT+X+Y, comma separated (xrandr's layout): the image spans them. |
| `wallpaper.out` | string | Output .png file or folder (default <Pictures>/JETCARP/wallpapers/). |
| `wallpaper.outline` | number | Draw tile outlines when tiles are at least this many pixels across (default 0: never). |
| `wallpaper.palette` | string | Colour every CA with this palette first: a name (`reference {"what": "palettes"}` lists them) or hex colours like `#4fc3ff` or `#103050-#4fc3ff`. |
| `wallpaper.scale` | number | Pixels per world unit (with `center`). |
| `wallpaper.size` | string | WIDTHxHEIGHT or a preset: HD, WUXGA, QHD, WQXGA, UWQHD, 4K, 5K, 8K, Phone (default 3840x2160). Ignored with `monitors`. |
| `wallpaper.split` | boolean | With `monitors`: one image per monitor instead of the whole span. |

Example:

```json
{
  "name": "world_render",
  "arguments": {
    "max_side": 800,
    "save_path": "hex.png"
  }
}
```

### `run_script`

**Run a JETCARP script.** Runs a JETCARP script (the Rhai language) and returns its log, errors with line numbers, output folders and sweep summaries. Scripts make their own worlds (the current world is not changed), run them until something happens, save PNGs, and run parallel rule sweeps into new folders under the output folder. Example: `let w = world(#{ tiling: "hex", size: [120, 80], boundary: "dead" }); w.add_ca("HexW3 8229", #{ seed: "top_center" }); let r = w.run(#{ until: "done" }); print(r.generations); w.save_png(out_dir("test") + "/a.png");` or `sweep(#{ rules: "W", until: "done", png: #{ size: [400, 300] } });`. help {"topic": "scripts"} lists every function. Limits: 600 seconds at most (default 60, `max_seconds`), worlds of at most 4000000 cells; files are written only inside the output folder and read only there. Stops early when the client cancels the call.

| Argument | Type | Meaning |
| --- | --- | --- |
| `cell_px` | number | Tile size in pixels for screen-sized worlds (default 8). |
| `check_only` | boolean | Only check the script for syntax errors. |
| `log_lines` | integer | Most log lines to return, the last ones (default 200). |
| `max_seconds` | number | Stop the script after this many seconds (default 60). |
| `params` | object | Values the script reads as `params.key`. |
| `screen` | string | Screen size for size: "screen" worlds and the default image size, e.g. "1920x1080" (default). |
| `script` (required) | string | The script text (Rhai). |

Example:

```json
{
  "name": "run_script",
  "arguments": {
    "script": "sweep(#{ rules: \"W\", until: \"done\", png: #{ size: [400, 300] } });",
    "max_seconds": 120
  }
}
```

### `library_query`

**List or read library records.** Lists or reads the library: what searches found and what you and the user kept. `kind`: "pattern" (default: verified still lifes, oscillators, spaceships, guns, puffers and movers, each with the rules it was verified in and the range of rules it runs identically in), "ant" (ant and turmite runs, classed highway, periodic, symmetric, bounded or chaotic), "quickstart" (the app's Quick Start entries: built-in examples, packs, explore atlases, promotions) or "summary" (counts). Listings come best first (`limit`, default 20, at most 100; `offset` pages); each filter says which kind it applies to. With `id`, one record in full: a pattern's rules, sightings, the cells of its phases as text, and which way it moves per paste orientation; an ant run's outcome and proof; a Quick Start entry with its scene. Examples: {"class": "spaceship", "tiling": "hex"}, {"works_in": "B36/S23"}, {"kind": "ant", "outcome": "highway"}, {"id": "glider", "phases": 4}. Next: world_new {"pattern": id} or world_edit {"paste": {"id": id}} to run one; library_curate to curate; media_render for pictures.

*reads only*

| Argument | Type | Meaning |
| --- | --- | --- |
| `class` | string | Patterns of this class: still_life, oscillator, spaceship, puffer, gun, mover. |
| `discovered_by` | string | Patterns found by: search, script, llm, user. |
| `hidden` | boolean | Quick Start: Only hidden entries (true) or only shown ones (false); default both. |
| `id` | string | Read this record in full instead: a pattern's id, prefix or name ("glider"), an ant run (ant-...) or a Quick Start handle (find:..., life). |
| `job` | integer | Ants of this search job. |
| `kind` | `"pattern"`, `"ant"`, `"quickstart"`, `"summary"` | What to list or read (default pattern; with `id`, the id's form says). |
| `limit` | integer | At most this many records (default 20, at most 100). |
| `max_period` | integer | Patterns with a period of at most this. |
| `min_period` | integer | Patterns with a period of at least this. |
| `min_score` | number | Patterns scoring at least this (0-100). |
| `neighborhood` | string | Patterns with this neighbourhood: `touch` or `edge`. |
| `offset` | integer | Skip this many patterns first (paging). |
| `order` | string | Pattern order: score (default), seen, rare, newest, updated, cells, period, rating, id. |
| `outcome` | string | Ants with this outcome: highway, periodic, symmetric, bounded, chaotic. |
| `phases` | integer | Phases to draw as text (default 1). |
| `rule` | string | Patterns verified in a rule whose notation contains this; ants: this turn string or table (`RL`, `Turmite LLRR`; its mirror image matches too). |
| `sort` | string | Ant order: score (default), onset, period, speed, rule. |
| `source` | string | Quick Start: only entries from this source: builtin, pack, atlas, promotion, or all (default: all but builtin). |
| `state` | string | Patterns in this curation state: new, reviewed, promoted, rejected. |
| `tag` | string | Patterns with this tag. |
| `text` | string | Patterns whose name or label contains this; Quick Start entries whose title, rule or tiling contains it. |
| `tiling` | string | Tiling family id (`hex`), catalog id (`k2-001`), or part of the tiling name. |
| `works_in` | string | Patterns that run in this rule (inside their rule range, or verified there). |

Example:

```json
{
  "name": "library_query",
  "arguments": {
    "class": "spaceship",
    "tiling": "hex",
    "limit": 5
  }
}
```

### `library_curate`

**Curate library records.** Changes the library. With `id` (a pattern's id, prefix or name; ant-...; or a Quick Start handle) give one of: `changes` (name, tags, notes, rating 1-5, state), e.g. {"id": "glider", "changes": {"rating": 4, "append_notes": "LLM: a clean c/4 ship"}} (mark text you wrote as yours); `hidden` (Quick Start entries: true leaves it out of the menu, packs and the website; false shows it again); or `remove`: true (deletes the record for good, a pattern with its rules, sightings and Quick Start entry; ask the user first). Without `id`: `quickstart` adds an entry to the app's Quick Start under "My finds", e.g. {"quickstart": {"pattern": "<id>", "title": "Trihex glider"}}; or `compounds` finds stored patterns that are really several independent objects (two gliders side by side) and lists their parts ({"compounds": {}}; with {"apply": true} it stores the parts instead).

*may replace or delete something*

| Argument | Type | Meaning |
| --- | --- | --- |
| `changes` | object | Patterns and ant runs: the changes (what is not given stays). Patterns: name, tags or add_tags / remove_tags, notes or append_notes, rating 1-5 (0 clears), state new / reviewed / promoted / rejected, discovered_by; ant runs: name, notes, rating, state. Settings: `jetcarp://schema/library_curate/changes`. |
| `changes.add_tags` | array of string | Patterns: tags to add. |
| `changes.append_notes` | string | Patterns: add a paragraph to the notes. |
| `changes.discovered_by` | string | Patterns: who found it (search, script, llm, user). |
| `changes.name` | string | New name (empty clears it). |
| `changes.notes` | string | Replace the notes. |
| `changes.rating` | number | 1 (dull) to 5 (remarkable); 0 clears it on patterns. |
| `changes.remove_tags` | array of string | Patterns: tags to remove. |
| `changes.state` | string | Patterns: new, reviewed, promoted, rejected. Ants: any word. |
| `changes.tags` | array of string | Patterns: replace all tags. |
| `compounds` | object | Find stored patterns that are several independent objects, as a resumable job. {} lists them with their parts (nothing changes); {"apply": true} stores each compound's parts in its place (credited with its sightings) and removes it, or keeps it tagged `compound` when curated. Settings: `jetcarp://schema/library_curate/compounds`. |
| `compounds.apply` | boolean | Change the library: store each compound's parts (credited with its seen count and sightings, and given its history), then remove the compound, or keep it tagged `compound` when someone curated it or a part does not run in all its rules. Without it (the default) nothing changes: the compounds are only listed. |
| `compounds.job` | integer | Resume this compounds job instead of starting one. |
| `compounds.minutes` | number | Stop after this many minutes (default 5, at most 10), resumable with `job`. |
| `hidden` | boolean | Quick Start entries: leave it out of the menu, packs and the website (true), or show it again (false). |
| `id` | string | The record: a pattern's id, unique prefix or name; an ant run's id (ant-...); or a Quick Start entry's handle (find:..., atlas:..., life). |
| `kind` | `"pattern"`, `"ant"`, `"quickstart"` | What `id` names (default: from its form). |
| `quickstart` | object | Add an entry to the Quick Start under "My finds". It takes one of `pattern`, `ant`, `folder` with `run`, `search_run` (with `job`), or `scene`, e.g. {"pattern": "<id>", "title": "Trihex glider"}, {"ant": "ant-..."}, {"search_run": <run id>, "job": <explore job>}, {"folder": "<sweep folder>", "run": "#12"} (a run of a run_script sweep), {"scene": "<scene file>"}. Settings: `jetcarp://schema/library_curate/quickstart`. |
| `quickstart.ant` | string | An ant run (`ant-...`): its build-up from step 0; the run's state becomes promoted. |
| `quickstart.description` | string | Patterns, ants, search runs and scenes: the description (default: generated, or the scene's). |
| `quickstart.favourite` | boolean | Sweep runs: also star the run in the folder's favourites.json (default true). |
| `quickstart.folder` | string | A sweep output folder (with `run`): that run's tiling, rule, and start, so it replays as in the sweep. |
| `quickstart.job` | integer | With `search_run`: the explore job (default: the newest). |
| `quickstart.note` | string | Sweep runs: a note put before the generated description (why it is interesting). |
| `quickstart.palette` | string | Ants: colour the scene with this built-in palette. |
| `quickstart.pattern` | string | A library pattern (id, unique prefix, or name): a scene with it centred on a suitable wrapped grid; the pattern's state becomes promoted. |
| `quickstart.run` | string | With `folder`: the run's rule text (`HexT3 sum 588`), manifest index (`#12`), or series number (`n=588`). |
| `quickstart.scene` | string | A scene file. |
| `quickstart.search_run` | integer | A stored run of an explore search, by its id: on the large grid it was confirmed on, its card on the small grid it was found on, as the atlas shows its entries. |
| `quickstart.title` | string | The entry's title (default: the pattern's name or label, the ant's rule, the run's rule, the scene's name). |
| `remove` | boolean | Delete the record for good (true). |

Example:

```json
{
  "name": "library_curate",
  "arguments": {
    "id": "glider",
    "changes": {
      "rating": 4,
      "append_notes": "LLM: the c/4 glider"
    }
  }
}
```

### `search_start`

**Start or resume a search.** Starts a search as a library job and returns its job id at once; job_status follows it, job_stop stops it. Give one strategy as a key holding its settings (every setting is optional; each strategy names the resource listing them): `hunt`, `search`, `ants`, `census`, `mover`, `pile`, `collide`, `evolve` or `explore` (help {"topic": "searches"} explains each). Or {"resume": job} carries on a stopped job. The job runs on a thread of this server, so it stops when the server exits (it stays resumable); {"detach": true} runs it in a process of its own that outlives the server (explore searches do this by default where they can). `wait_s` waits for the job to end, and `budget` ends this run early. Results go to the library (library_query), an explore search's runs to search_runs. Examples: {"hunt": {"rules": ["B3/S23"], "soups": 20000}, "wait_s": 120}, {"ants": {"tilings": "hex", "strings": [2, 4]}}, {"pile": {"tiling": "hex", "grains": "1e6"}}, {"collide": {"a": "block", "b": "glider"}, "wait_s": 60}, {"explore": {"tilings": "square,hex", "families": "life_like,cyclic"}, "budget": {"minutes": 5}}.

| Argument | Type | Meaning |
| --- | --- | --- |
| `ants` | object | Ant and turmite rules on a periodic tiling, every run classed and stored. Turn strings such as RL or LLRR, or turmite tables, from an empty field or soups; each run is classed highway (proved exactly), periodic, symmetric, bounded or chaotic. Settings: `jetcarp://schema/search_start/ants`. |
| `ants.all_max` | integer | Tables: enumerate every table up to this many, else sample `sample` of them (default 1,000,000). |
| `ants.alphabet` | string | Turn letters to use, e.g. "LR" (default: the tiling's). |
| `ants.density` | number | Soup density (default 0.5). |
| `ants.max_period` | integer | Longest highway or orbit period looked for (default 20,000). |
| `ants.rules` | array or string | Explicit rules ("RL", "LLRR", "RL2N", "1R1,1L0/1R1,0N0"): a list, or text separated by spaces or semicolons. With no strings, tables, or rules: strings of 2 to 6. |
| `ants.sample` | integer | Tables sampled when there are more than `all_max` (default 10,000). |
| `ants.seed` | integer or string | Seed of the soups and of sampled tables (default 1). |
| `ants.size` | array or integer or string | Soup size in unit cells: n, "WxH", or [w, h] (default 8x8). |
| `ants.soups` | integer | Seeded soups per rule (default: an empty field). |
| `ants.steps` | integer | Steps before a run is classed as it is (default 1,000,000). |
| `ants.strings` | array or integer or string | Turn string lengths: [min, max], one length, or "2-6". |
| `ants.symmetry` | boolean | Test for mirror-symmetric growth (default true). |
| `ants.tables` | array or integer or string | Turmite tables: [states, colours] or "2x2". |
| `ants.tilings` | array or string | Periodic tilings: ids, catalog ids, vertex configurations, or sets (default square). |
| `budget` | object | This run's budget: `minutes`, `items` or `until`. It ends at the next checkpoint after it; the job stays resumable. Settings: `jetcarp://schema/search_start/budget`. |
| `budget.items` | integer | Stop at the next checkpoint after this many units in this run: soups (per rule and tiling for hunts), runs, rules, or generations. |
| `budget.minutes` | number | Stop at the next checkpoint after this many minutes. |
| `budget.until` | integer | Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed. |
| `census` | object | Rules classed by what their soups do (die, freeze, oscillate, grow, ...), with a table and a contact sheet. Settings: `jetcarp://schema/search_start/census`. |
| `census.all` | boolean | Every B/S rule over `birth` and `survive` (with `rules` too: added to them). |
| `census.all_max` | integer | With no rules: every B/S rule over `birth` and `survive` when there are at most this many, else a sample (default 32,768; 0: always a sample, added to `rules` when there are some). |
| `census.birth` | array or string | Birth digits for B/S rules: "1-8", "1,2,3,6", "1-max" (the tiling's largest degree), or a list (default 1 to max). |
| `census.boundary` | string | wrap, dead, or live, for a tiling given by id (default: as the id gives it, dead). |
| `census.cells` | integer | About this many cells (instead of `size`). |
| `census.census` | any | A whole census spec (as a census report's `spec` holds it), instead of the fields above. |
| `census.density` | number | Soup density (default 0.37). |
| `census.explode` | number | A run is called exploded once its population exceeds this fraction of the cells (default 0.5). |
| `census.gens` | integer | Generation limit per run (default 2000). |
| `census.level` | integer | Aperiodic patch level (instead of `size`). |
| `census.nb` | string | Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge). |
| `census.rules` | array or string | Rules: a list, or text separated by spaces, commas, or semicolons ("B3/S23 B36/S23 B2/S/C3"). |
| `census.rules_file` | string | A file of more rules, one per line (`#` comments). |
| `census.sample` | integer | B/S rules sampled (seeded, distinct) when there are more than `all_max` (default 2000). |
| `census.sample_seed` | integer or string | The sample's seed (default: from `seed`). |
| `census.seed` | integer or string | Default 1. |
| `census.size` | string | Grid size for a tiling given by id: "WxH" unit cells, about N cells, or "L<level>" (default 64x64; aperiodic tilings: their default level). |
| `census.soups` | integer | Soups per rule (default 8; soup s uses RNG state seed + s). |
| `census.states` | integer | Generations rules with this many states (B/S rules otherwise). |
| `census.survive` | array or string | Survival digits for B/S rules, as `birth` (default 0 to max). |
| `census.tiling` | object or string | The tiling: an id ("hex", "k2-01", "spectre") or a tiling map (default square). |
| `collide` | object | Pattern `b` shot into pattern `a` at every offset and phase. Each run is classed, and new products are verified and stored. Settings: `jetcarp://schema/search_start/collide`. |
| `collide.a` | string | Pattern A (stays at the origin): id, unique prefix, or name. |
| `collide.b` | string | Pattern B (placed at each offset). |
| `collide.dx` | array or integer or string | B's x offsets instead: [lo, hi] or "lo..hi". |
| `collide.dy` | array or integer or string | B's y offsets instead: [lo, hi] or "lo..hi". |
| `collide.gens` | integer | Generations per run (default 1024). |
| `collide.margin` | integer | Empty cells around the pair (default 40). |
| `collide.offsets` | integer or string | B's corner offsets on both axes: n (-n..n) or "lo..hi" (default 6). |
| `collide.orientation_a` | integer | A's orientation (index into the tiling's symmetries). |
| `collide.orientation_b` | integer | B's orientation. |
| `collide.phases` | array or string | B's phases: "all" (default), a list, or "0,1". |
| `collide.rule` | string | The rule to run (both must work in it; default: A's rule if B works in it, else B's). |
| `collide.store` | boolean | Store the products in the catalog (default true; false only reports them). |
| `detach` | boolean | Run in a process of its own (`jetcarp jobs resume JOB`) and answer at once. |
| `evolve` | object | The evolutionary glider search, storing the movers it verifies. Multi-state priority rules evolve towards rules whose seeds travel far. Settings: `jetcarp://schema/search_start/evolve`. |
| `evolve.cap` | integer | Population cap during the GA (default 64; explosive rules score 0). |
| `evolve.champions` | integer | Distinct top genomes verified at the end (default 16). |
| `evolve.folder` | string | The run's folder (default: a new `evolve-<tiling>` folder in `out`). |
| `evolve.gens` | integer | GA generations (default 800). |
| `evolve.horizon` | integer | Generations per seed run (default 2500). |
| `evolve.nb` | string | touch (default; the paper's vertex adjacency) or edge. |
| `evolve.out` | string | The folder a new run folder goes in (default: the output root). |
| `evolve.pop` | integer | Population size (default 256). |
| `evolve.radius` | integer | Arena radius in edge rings (default 48). |
| `evolve.seed` | integer or string | RNG seed: the run is a pure function of it (default 1). |
| `evolve.states` | integer | States per cell, 2 to 8 (default 4). |
| `evolve.stop_on_glider` | boolean | Stop at the first genome that passes verification. |
| `evolve.store` | boolean | Store the verified movers in the catalog (default true). |
| `evolve.threads` | integer | The search's own worker threads (its results do not depend on them). |
| `evolve.tiling` | string | Tiling: an id: spectre (default), hat, penrose-kd, square, hex, k2-017, ... |
| `evolve.track_gens` | integer | Generations the mover tracker may run on aperiodic patches (default 20,000). |
| `evolve.verify` | array or string | Verification radii as multiples of the radius: a list or "1,2,4" (default 1,2,4). |
| `explore` | object | The broad search over tilings, rule families, rules and seeds (search_runs reads its runs). It keeps the best runs of each kind of behaviour and breeds from them, storing every run in the job's own database. Not deterministic: a resumed search continues from its archive. Settings: `jetcarp://schema/search_start/explore`. |
| `explore.batch` | integer | Most worlds in a GPU batch (default 1024). |
| `explore.confirm_cells` | integer | Cells of a confirmation run, on a bigger grid (default 64,000). |
| `explore.confirm_gens` | integer | Generations of a confirmation run (default 4096). |
| `explore.confirm_seeds` | integer | Seeds an elite is confirmed with (default 3). |
| `explore.families` | array or string | Only these rule families (life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider), a list or text (default: all). |
| `explore.gpu` | boolean | Run the rules a GPU can run on the GPU when there is one, in this and later runs (default false; without a GPU the CPU runs everything). |
| `explore.max_rss_gb` | number | Stop (paused, resumable) when the process uses more memory than this many GB. |
| `explore.screen_cells` | integer | About this many cells in a screening run's grid (default 16,000). |
| `explore.screen_gens` | integer | Generations of a screening run (default 1024). |
| `explore.seed` | integer or string | Where the random draws start (default 0). |
| `explore.tilings` | array or string | Only these tilings: ids (square, hex, trihex, spectre, ih28, k2-001, ...), a list or text separated by commas or spaces (default: every tiling the search knows, 1,359 today; the progress line counts them). |
| `gpu` | boolean | Use the GPU where there is one: hunts run the start of each soup there (results are identical), piles topple there, explore runs there the rules it can. Default false; explore true. |
| `hunt` | object | Small random soups on an unbounded plane, each run until it settles. Every object is classed, and new patterns are verified and stored; B/S and Generations rules without B0, on any periodic tiling; ships that fly away are cut out. Thousands of soups a second for Life. Settings: `jetcarp://schema/search_start/hunt`. |
| `hunt.density` | number | Soup density (default 0.5). |
| `hunt.gens` | integer | Generations before a soup that has not settled is censused as it is (default 20,000). |
| `hunt.margin` | integer | Empty margin around the soup before escaping ships are cut (default 0: automatic). |
| `hunt.max_period` | integer | Longest period looked for (default 64). |
| `hunt.nb` | string | Neighbourhood for every rule: touch or edge (default: the notation decides). |
| `hunt.pop_cap` | integer | Live cells above which a soup counts as exploded (default 0: automatic). |
| `hunt.rules` | array or string | Rules (B/S and Generations without B0; a `V` suffix means edge neighbours): a list, or text separated by spaces, commas, or semicolons (default B3/S23). |
| `hunt.rules_file` | string | A file of more rules, one per line (`#` comments). |
| `hunt.seed` | integer or string | Soup i uses an RNG state from (seed, i) (default 1). |
| `hunt.shape` | string | Soup shape: rect (default) or disc. |
| `hunt.size` | array or integer or string | Soup size in unit cells: "WxH", n, or [w, h] (default 16x16). |
| `hunt.soups` | integer | Soups per rule and tiling (default 10,000; 0: until stopped). |
| `hunt.store` | any | Storage policy: "all" (every new object as a full record), or {max_cells, max_volume, min_seen} (default 64, 4000, 2: larger still lifes and oscillators are counted, not stored, until seen that often). |
| `hunt.tilings` | array or string | Tilings: family ids, catalog ids, or sets (all-periodic, archimedean, kuniform:2, isohedral), as a list or text; tiling maps work too (default square). |
| `label` | string | A name to find the job by (scripts, the dashboard). |
| `mover` | object | A pattern on an aperiodic tiling followed far, and stored with its distance when verified. It runs on a sliding window of generated supertiles (Spectre, hat, Penrose). Settings: `jetcarp://schema/search_start/mover`. |
| `mover.at` | integer | The tile for `states`. |
| `mover.cells` | array or string | Starting cells: [[tile, state], ...] or "1203:1,1204:3". |
| `mover.fixed_levels` | integer | Spectre: follow the fixed patches' Delta chain up to this level (default 8). |
| `mover.gens` | integer | Generation limit (default 100 per ring plus 20,000). |
| `mover.level` | integer | Patch level the cells are given on (default 4). |
| `mover.margin` | number | World units of tiles kept around the live cells (default 24). |
| `mover.max_pop` | integer | Stop as grown above this many live cells (default 2000). |
| `mover.nb` | string | Neighbourhood: touch (default) or edge. |
| `mover.rings` | integer | Stop, verified, at this many edge rings (default: run to `gens`). |
| `mover.rule` | string | Rule notation (default SpectreGlider; "Priority 4: ...", B/S). |
| `mover.source` | string | Who started it, for the record's provenance (default "search_start"). |
| `mover.states` | array or string | Instead of cells: states on a tile and an edge neighbour (default [1, 3]), tried near the patch's deepest tile (or at `at`) until one travels. |
| `mover.store` | boolean | Store a verified mover in the catalog (default false). |
| `mover.tiling` | string | Aperiodic tiling: spectre (default), hat, penrose-kd, penrose-rhomb. |
| `pile` | object | One large sandpile: N grains dropped on one cell of an unbounded grid. It is built by doubling, with pictures in its folder under the output folder. Settings: `jetcarp://schema/search_start/pile`. |
| `pile.centre` | string | cell (default), vertex, or edge: where the grains go. |
| `pile.folder` | string | The pile's own folder: carried on from where it stopped when it holds a step file (the job whose folder it is, or a new job for it), else the new pile is built there. A finished pile given picture options (palette, reverse, px, margin, preview, png) is exported again in them. |
| `pile.gpu` | boolean | Topple on the GPU when there is one (periodic tilings). |
| `pile.grains` | integer or string | Grains: 1000000, "1e7", "2^30" (up to 2^64 - 1; default 10,000). |
| `pile.level` | integer | Aperiodic tilings: patch level (default: the smallest that should hold the pile). |
| `pile.margin` | number | Empty cells around the pile in the image (default 4). |
| `pile.name` | string | The run folder's name (default `<tiling>-<grains>`, after the date). |
| `pile.nb` | string | edge (default; von Neumann on squares) or touch (Moore). |
| `pile.out` | string | The folder a new pile's run folder goes in (default `<output>/piles`). |
| `pile.palette` | string | Palette for the heights, with its background and stage (default Ember, or the folder's own). |
| `pile.png` | boolean | Write the full-size pile.png (default true). |
| `pile.preview` | integer | Longest side of preview.png (default 1024; 0: none). |
| `pile.px` | number | Pixels across a cell in pile.png (default 2; below 1.5 each pixel averages the cells in it). |
| `pile.reverse` | boolean | Run the palette the other way (height 1 at its last stop). |
| `pile.tiling` | string | Tiling id: square (default), hex, triangle, snubsq, cairo, k3-012, ih41, spectre, hat, ... |
| `resume` | integer | Carry on this job (stopped, paused, or cut off) instead of starting one. |
| `search` | object | Soups on a bounded grid of any tiling, aperiodic patches too. Each runs for `gens`, and the objects left are verified and stored. Settings: `jetcarp://schema/search_start/search`. |
| `search.boundary` | string | wrap (default) or dead. |
| `search.density` | number | Soup density (default 0.37). |
| `search.gens` | integer | Generations per soup (default 2000). |
| `search.height` | integer | Grid height in unit cells. |
| `search.max_period` | integer | Longest period looked for (default 64). |
| `search.nb` | string | Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge). |
| `search.rules` | array or string | Rules (Life-like and Generations rules without B0): a list, or text separated by spaces, commas, or semicolons (default B3/S23). |
| `search.rules_file` | string | A file of more rules, one per line (`#` comments). |
| `search.seed` | integer or string | Soup s uses RNG state seed + s (default 1). |
| `search.size` | array or integer or string | Grid size: "WxH" unit cells, about N cells ("4000"), "L<level>" on aperiodic tilings, or [w, h] (default 64x64). |
| `search.soup_radius` | number | Soup only a disc at the centre: its radius as a fraction of half the shorter side; with a dead boundary, escaping spaceships are caught at the edge. |
| `search.soups` | integer | Soups per rule and tiling (default 16). |
| `search.store` | boolean | Store what is found in the catalog (default true; false only reports it). |
| `search.tilings` | array or string | Tilings: ids, catalog ids, vertex configurations, or sets, as a list or text (default square). |
| `search.width` | integer | Grid width in unit cells (with `height`, instead of `size`). |
| `threads` | integer | Worker threads (default 0: all cores; results do not depend on them). |
| `wait_s` | number | Wait up to this many seconds for the job to end, then answer with its status (default 0: answer at once). |

Example:

```json
{
  "name": "search_start",
  "arguments": {
    "hunt": {
      "rules": [
        "B3/S23"
      ],
      "soups": 20000
    },
    "wait_s": 120
  }
}
```

### `job_status`

**Job status, finds and report.** Reports a library job's progress, whatever started it and whatever its kind (hunts, ant searches, piles, movers, collisions, censuses, evolve and explore searches). With `wait_s` (at most 600) it waits for a running job to end before answering, so one call replaces many polls. `finds` adds the job's newest and rarest finds (pattern ids for library_query); `report` gives a Markdown report of what it searched and found instead. Without `job`, lists the jobs, newest first, optionally of one kind or status. Examples: {"job": 3, "wait_s": 300}, {"job": 3, "finds": 10}, {"job": 3, "report": true}, {"kind": "ants"}.

*reads only*

| Argument | Type | Meaning |
| --- | --- | --- |
| `finds` | integer | Include this many of its newest and rarest finds (hunts, bounded searches, collisions). |
| `job` | integer | The job (default: list the jobs, filtered as job_status). |
| `kind` | string | Listing: only jobs of this kind. |
| `label` | string | Listing: only jobs with this label. |
| `limit` | integer | Listing: at most this many. |
| `prefix` | string | Listing: only jobs whose label starts with this. |
| `report` | boolean | With `job`: a Markdown report of what it searched, for how long, and what it found. |
| `status` | string | Listing: only jobs with this status. |
| `wait_s` | number | Wait up to this many seconds for a running job to stop running before answering (default 0: answer now). |

Example:

```json
{
  "name": "job_status",
  "arguments": {
    "job": 3,
    "finds": 10
  }
}
```

### `job_stop`

**Stop a job.** Stops a running job at its next checkpoint (a few seconds), in whatever process runs it: here, the command line, a script, or the app. It stays resumable: search_start {"resume": job} carries it on (a hunt, ant search, pile, evolve or mover job then ends exactly as an uninterrupted run would; an explore search continues from its archive). Example: {"job": 3}.

| Argument | Type | Meaning |
| --- | --- | --- |
| `job` (required) | integer | The job id. |

Example:

```json
{
  "name": "job_stop",
  "arguments": {
    "job": 3
  }
}
```

### `search_runs`

**An explore search's runs.** Reads an explore search's stored runs (search_start {"explore": ...} makes one; `job` picks it, default the newest). With no action: its runs, best first, with their measures (fate, settle time, period, density, activity, movers, score: about 0 to 3, higher is more interesting), filtered by family, tiling, fate, min_score, min_movers, stage or reviewed. Or one action: `sheet`, a labelled picture of runs at several moments, to judge by eye ({"sheet": {"ids": [12, 40, 7]}}); `scene`, one run's scene JSON for world_new ({"scene": {"run": 12}}); `try`, a configuration of your own run now, with its measures and a picture ({"try": {"tiling": "hex", "rule": "B2/S34", "seed": "random:0.3"}}); or `review`, your verdict on a run ({"review": {"run": 12, "verdict": "keep", "note": "slow gliders"}}), which feeds what is published and the score. The sealed runs are also Parquet files (runs/part-*.parquet in the search's folder) for DuckDB, Polars or pyarrow.

| Argument | Type | Meaning |
| --- | --- | --- |
| `family` | string | Only this rule family (life_like, generations, ltl, jaca, cyclic, ...). |
| `fate` | string | Only runs that ended this way: died, froze, cycle, running. |
| `ids` | array of integer | These runs, instead of the filters. |
| `job` | integer | The explore job (default: the newest). |
| `limit` | integer | Most runs to return (default 40, at most 200). |
| `min_movers` | integer | Only runs with at least this many moving objects. |
| `min_score` | number | Only runs with at least this score. |
| `order` | string | score (default, best first), newest, or id. |
| `review` | object | Record a verdict on a run. Settings: `jetcarp://schema/search_runs/review`. |
| `review.note` | string | What was seen, in a sentence. |
| `review.run` | integer | The run reviewed. |
| `review.verdict` | string | keep, maybe, or drop. |
| `reviewed` | string | Only reviewed runs: any, keep, maybe, or drop. |
| `scene` | object | One run's scene JSON. Settings: `jetcarp://schema/search_runs/scene`. |
| `scene.run` (required) | integer | The run's id. |
| `scene.save` | string | Also save the scene here (JSON). |
| `sheet` | object | A contact sheet of runs, returned as a picture (at most 36 runs). Settings: `jetcarp://schema/search_runs/sheet`. |
| `sheet.cols` | integer | Thumbnails per row (default 8). |
| `sheet.fracs` | array of number | Moments to show, as fractions of each run (default [0.25, 1]). |
| `sheet.ids` | array of integer | Run ids to show. |
| `sheet.palette` | string | Draw every run in this palette; several, separated by commas, compare them side by side (one row per run, at the first moment). |
| `sheet.png` | string | Also write the PNG here. |
| `sheet.px` | integer | Pixels per thumbnail (default 200). |
| `stage` | integer | 1 screened (default), 2 confirmation runs on bigger grids. |
| `tiling` | string | Only this tiling: an id such as hex or k2-017. |
| `try` | object | Try one configuration now (CPU), returning its measures and a picture. Settings: `jetcarp://schema/search_runs/try`. |
| `try.cells` | integer | About this many cells (default 16,000). |
| `try.gens` | integer | Generations (default 1024). |
| `try.nb` | string | touch or edge (default: touch, or what the notation says). |
| `try.png` | string | Also write the picture (the run's end) here. |
| `try.px` | integer | Picture size in pixels (default 420). |
| `try.rng` | integer or string | The random seed (default 1). |
| `try.rule` (required) | string | Rule notation. |
| `try.seed` | any | The starting pattern: a seed shorthand ("random:0.3", "pattern:blobs:0.4:by_region", "pattern:symmetric:0.5") or object (default: the rule's own). |
| `try.tiling` (required) | string | Tiling id: square, hex, trihex, spectre, k2-001, ih28, ... |

Example:

```json
{
  "name": "search_runs",
  "arguments": {
    "job": 4,
    "sheet": {
      "ids": [
        12,
        40,
        7
      ]
    }
  }
}
```

### `media_render`

**Render animations and posters.** Renders media and returns the file paths: per-step PNG frames, an animation (GIF when small, else animated WebP), a poster frame and a thumbnail, in one house style. Give one of `pattern` (alone on an empty field for one seamless period; spaceships in a window that moves with them, guns with their stream), `ant` (a highway's loop in a window moving with the ant, or with mode seed the run from step 0), `quickstart` (from its seed), `scene` (from its seed, into the output folder) or `missing` (every record whose media are missing or stale). Library records' media are stored once by content in the library's media/ folder, and up-to-date media are not rendered again. `poster`: true also returns the first poster inline, to look at. Examples: {"pattern": "glider", "poster": true}, {"ant": "ant-...", "mode": "seed", "gens": 300}, {"missing": true, "class": ["spaceship"], "limit": 20}.

| Argument | Type | Meaning |
| --- | --- | --- |
| `ant` | string | An ant run (`ant-...`): a highway's moving loop alone, or the build-up from the seed. |
| `class` | array of string | With `missing`: only patterns of these classes (spaceship, oscillator, gun, ...). |
| `every` | integer | From the seed: a frame every this many generations (default 1). |
| `force` | boolean | Render even when up to date. |
| `format` | string | auto (default: GIF up to 320x320 pixels and 256 colours, else WebP), gif, or webp. |
| `frames` | boolean | Write the per-step PNG frames (default true). |
| `gens` | integer | From the seed: generations (default 200; row rules one per row). |
| `limit` | integer | With `missing`: at most this many targets (0: no limit). |
| `missing` | boolean | Every pattern, shown Quick Start entry, and notable ant run whose media are missing or stale. |
| `mode` | string | auto (default: patterns alone, everything else from the seed), alone, or seed. |
| `only_patterns` | boolean | With `missing`: patterns only. |
| `only_quickstart` | boolean | With `missing`: Quick Start entries only. |
| `out` | string | With `scene` or a built-in Quick Start entry: the folder for its files (default `<output folder>/media/<scene name>`). |
| `palette` | string | A palette instead of the house style: a built-in name, or hex colours like `#103050-#4fc3ff`. |
| `pattern` | string | A library pattern: id, unique id prefix, or name. |
| `periods` | integer | Alone: full periods shown (default 1). |
| `poster` | boolean | Also return the first target's poster inline (PNG). |
| `quickstart` | string | A Quick Start entry: a catalog one (`find:...`, media stored in the catalog) or a built-in one (`life`), rendered from its seed into `out`. |
| `scene` | any | Any scene (JSON: the object or its text), from its seed, into `out` (not the catalog). |

Example:

```json
{
  "name": "media_render",
  "arguments": {
    "pattern": "glider",
    "poster": true
  }
}
```

### `pack`

**Packs and the website export.** Packs: signed, content-addressed folders of library records (Parquet tables and JSON) that anyone can read, check and import. Give one action as a key holding its options: `info` (a version's summary), `check` (every hash, id and signature, importing nothing), `import` (into the library, records merged by id), `publish` (the library, or a selection, as a new signed version of a feed in a store), `export` (one version laid out with readable file names, for a single download) or `site` (the website export). Paths must be inside the output folder (relative paths start there), the library's folder, or the packs that came with JETCARP. Publishing signs with the publisher key in JETCARP's settings, which only the user makes (`jetcarp pack key`). Examples: {"info": {"path": "packs/starter"}}, {"import": {"path": "packs/starter"}}, {"publish": {"feed": "my-finds", "to": "packs/my-finds", "query": {"class": "spaceship"}}}, {"export": {"from": "packs/my-finds", "to": "my-finds-1"}}.

| Argument | Type | Meaning |
| --- | --- | --- |
| `check` | object | Check a store or export without importing it. Every hash, id and signature is checked. Settings: `jetcarp://schema/pack/check`. |
| `check.allow_unsigned` | boolean | Import a version that its publisher did not sign. |
| `check.feed` | string | The feed, when the store has several. |
| `check.manifest` | string | A version other than the feed's newest (`sha256:...`). |
| `check.path` (required) | string | The store's or export's folder. |
| `check.sample` | integer | How many new patterns to verify again with `sample` (default 3000). |
| `check.seed` | integer or string | The sample's seed (default: from the manifest hash). |
| `check.strict` | boolean | Refuse two different rows for one record id. |
| `check.verify` | string | `sample` (default), `full` or `none`. |
| `check.verify_seconds` | number | Most seconds verification may take (default 600). |
| `export` | object | Lay one version out with readable file names. It comes from a store (`from`), or from publishing the library first (`publish`). Settings: `jetcarp://schema/pack/export`. |
| `export.from` | string | Export a version of this store (default: publish the library first, as `pack publish` would, into a scratch store beside the export). |
| `export.manifest` | string | The version (default: the newest of the store's feed). |
| `export.publish` | object | Without `from`: what to publish first (the publish options; its `to` is ignored). |
| `export.to` (required) | string | The export's folder (conventionally `<feed>-<version label>`). |
| `import` | object | Import a store or export into the library. Records are merged by id, and a sample is verified again. Settings: `jetcarp://schema/pack/import`. |
| `import.allow_unsigned` | boolean | Import a version that its publisher did not sign. |
| `import.feed` | string | The feed, when the store has several. |
| `import.manifest` | string | A version other than the feed's newest (`sha256:...`). |
| `import.path` (required) | string | The store's or export's folder. |
| `import.sample` | integer | How many new patterns to verify again with `sample` (default 3000). |
| `import.seed` | integer or string | The sample's seed (default: from the manifest hash). |
| `import.strict` | boolean | Refuse two different rows for one record id. |
| `import.verify` | string | `sample` (default), `full` or `none`. |
| `import.verify_seconds` | number | Most seconds verification may take (default 600). |
| `info` | object | A version's summary. It gives the title, publisher and signature, tables and searches, without reading every file. Settings: `jetcarp://schema/pack/info`. |
| `info.feed` | string | The feed, when the store has several. |
| `info.manifest` | string | A version other than the feed's newest (`sha256:...`). |
| `info.path` (required) | string | The store's or export's folder. |
| `publish` | object | Publish the library (or a selection) as a new signed version of a feed. A selection is `ids` or `query`. Settings: `jetcarp://schema/pack/publish`. |
| `publish.created` | string | The version's time, RFC 3339 UTC (default: now), for reproducible packs. |
| `publish.description` | string | What the pack holds, for people. |
| `publish.feed` (required) | string | The feed: letters, digits and `-` (`jetcarp-starter`). |
| `publish.ids` | array of string | Only these records (pattern, ant and Quick Start ids or prefixes, or names). |
| `publish.licence` | string | An SPDX identifier (default CC0-1.0; JETCARP's own feeds take only CC0-1.0). |
| `publish.merge` | array of string | Merge other versions: a store's folder, or `<folder>@sha256:<manifest>`. |
| `publish.no_ants` | boolean | Leave out ant and turmite runs. |
| `publish.no_media` | boolean | Leave out pictures and animations. |
| `publish.no_quickstart` | boolean | Leave out Quick Start entries. |
| `publish.no_ratings` | boolean | Leave out ratings (otherwise only those of raters who consented go in). |
| `publish.no_searches` | boolean | Leave out search runs. |
| `publish.query` | any | Only the patterns a library query finds (the `library_query` filters, as JSON). |
| `publish.retract` | array of string | Withdraw records: `pattern:<id>=reason` (also `ant:` and `quickstart:`). |
| `publish.searches` | array of string | Only these explore searches (uuids); default every one with a folder. |
| `publish.title` | string | The pack's title, for people. |
| `publish.to` (required) | string | The store's folder (made if missing; a new version of the feed if it has one). |
| `publish.version_label` | string | Free text for people (`2026.11`). |
| `publish.views` | boolean | Write views.sql (DuckDB views over the store). |
| `site` | object | The website export into the folder `to`. It is the public part of the library as JSON and media, with manifest.json and a viewer page. Settings: `jetcarp://schema/pack/site`. |
| `site.ant_outcome` | array of string | Only ants with these outcomes (highway, symmetric, ...; default all). |
| `site.ants` | array of string | Ant and turmite curation states exported (default promoted and reviewed; empty: none). |
| `site.class` | array of string | Only patterns of these classes. |
| `site.frames` | boolean | Copy the per-step PNG frames too. |
| `site.min_osc_period` | integer | Leave out oscillators with a shorter period than this. |
| `site.min_rating` | integer | Only patterns rated at least this (1-5). |
| `site.min_score` | number | Only patterns with at least this interest score. |
| `site.page` | boolean | Write the viewer page, index.html (a grid of every item over manifest.json; default true). |
| `site.palette` | string | Render missing media in this built-in palette instead of the house style. |
| `site.quickstart` | array of string | Quick Start sources exported, their entries that are not hidden: promotion, pack, atlas, builtin (default promotion and pack; empty: none). |
| `site.render` | boolean | Render missing media first (default true). |
| `site.state` | array of string | Pattern curation states exported (default promoted and reviewed; rejected never). |
| `site.to` (required) | string | The export folder. |

Example:

```json
{
  "name": "pack",
  "arguments": {
    "info": {
      "path": "packs/starter"
    }
  }
}
```

### `files_list`

**List output files.** Lists what JETCARP wrote, read only: the output folder (root "out", the default: script runs, sweep folders, piles/, wallpapers/, media of scenes) or the library's folder (root "catalog": library.sqlite, media/, search folders). `path` is a folder relative to the root, or an absolute folder from another tool's output (it must be inside a root). Each entry has its name, kind, size, modified time, and for folders what marks them (manifest.json: a sweep or export; pile.json: a pile). `file` returns a small text file (JSON, CSV, text, Markdown, Rhai; the first 64 KB). Examples: {}, {"path": "piles"}, {"file": "piles/hex-1000000/pile.json"}.

*reads only*

| Argument | Type | Meaning |
| --- | --- | --- |
| `file` | string | A text file inside the root to read instead of listing (json, csv, txt, md, rhai, log, html, tsv; first 64 KB). |
| `limit` | integer | Most entries listed, newest first (default 100, at most 500). |
| `path` | string | Folder inside the root, relative or absolute (default: the root itself). |
| `root` | string | Which root: "out" (default: the output folder) or "catalog" (the catalog's folder). |

Example:

```json
{
  "name": "files_list",
  "arguments": {
    "path": "piles"
  }
}
```

### `use_app`

**Attach to the JETCARP app.** Chooses which world the other tools act on. attach=true: the world in the JETCARP desktop app the user is watching, so they see every change as it happens (the user must first switch it on in the app: Tools, AI control..., Allow AI control; or start the app with `jetcarp --control`. The app then also offers app_display, its displays and screensaver). attach=false: this server's own private world (the default unless the server was started with --app). Returns the attached world's status.

*`jetcarp mcp` (stdio) only.*

| Argument | Type | Meaning |
| --- | --- | --- |
| `attach` | boolean | true = act on the app's world, false = the private world. Default true. |
| `port` | integer | App port, if not the one the app recorded. |
| `token` | string | App token shown in its AI control window, if needed. |

Example:

```json
{
  "name": "use_app",
  "arguments": {
    "attach": true
  }
}
```

### `host_control`

**JETCARP background host.** Controls the JETCARP background host, the separate program that keeps live scenes running as wallpapers or windows without the app. Returns its status: each screen's rotation, item, generation and caption, memory use, and recent problems. Fails if the host is not running; the user starts it with `jetcarp host start`. Examples: {}, {"action": "next"}, {"action": "pause"}, {"action": "wallpaper", "on": false}.

*`jetcarp mcp` (stdio) only.*

| Argument | Type | Meaning |
| --- | --- | --- |
| `action` | `"status"`, `"next"`, `"previous"`, `"pause"`, `"resume"`, `"rotation"`, `"wallpaper"`, `"login"`, `"open_app"`, `"reload"`, `"quit"` | Default status. wallpaper puts every screen on the desktop background (GNOME: with the extension) or back in a window; login sets whether the host starts at login; open_app opens the app; reload rereads host.json. |
| `name` | string | With action rotation: the rotation to run (saved). |
| `on` | boolean | With wallpaper or login: on or off. Default on. |
| `screen` | integer | With next, previous, or rotation: which screen, counted from 0. Default: every screen for next and previous, the first for rotation. |

Example:

```json
{
  "name": "host_control",
  "arguments": {
    "action": "next"
  }
}
```

### `app_display`

**The app's displays and screensaver.** Reads and changes how the JETCARP app shows the grid. With no arguments it returns the state: where the grid shows (the window, full screen, or outputs: chrome-less full-screen windows on chosen monitors, spanning one canvas or mirroring one view), the monitors (by the index `monitors` takes), the canvas and cell size, running state and speed, the menu and panes, keep-awake, and the screensaver: its mode, rotation and profiles. Arguments change those, several at once, in a sensible order (outputs and cell size before the screensaver starts). Examples: {"screensaver": true, "profile": "Hodgepodge"}, {"saver": {"source": "rotation", "rotation": "Classics, hourly"}}, {"mode": "outputs", "monitors": [0, 1], "arrangement": "span"}, {"cell_px": 1, "refill": true}, {"skip": "next"}.

*Listed by the app's own server, and by `jetcarp mcp`, which passes it to the app once attached (`use_app`); an error otherwise.*

| Argument | Type | Meaning |
| --- | --- | --- |
| `arrangement` | `"span"`, `"mirror"` | span: one canvas across the outputs by their positions; mirror: the same view on each. |
| `cell_px` | number | Cell size in physical pixels (1 to 64), used by refill and pixel_zoom. |
| `delete_rotation` | string | Delete a rotation of yours by name (a built-in's edited copy reverts to the built-in). |
| `fullscreen` | boolean | Shorthand: true is mode fullscreen, false is mode window. |
| `fullscreen_monitor` | integer or null | Monitor index for full screen; null: the monitor the window is on. |
| `gpu` | boolean | Step on the GPU where the rules allow it (results are identical). |
| `hide_preview` | boolean | With outputs on, draw no grid in the main window (saves GPU work). |
| `identify` | boolean | true: show each monitor's index full screen on it for three seconds. |
| `keep_awake` | boolean | Keep the computer awake (no blanking, locking or suspend) while the app is open. |
| `menu_autohide` | boolean | Auto-hide the menu bar. |
| `mode` | `"window"`, `"fullscreen"`, `"outputs"` | Where the grid shows: the main window, the main window full screen, or the output windows. |
| `monitors` | array of integer | Output monitors by index (full screen on each). Also sets mode outputs unless mode is given. |
| `panes_hidden` | boolean | Hide every pane (the grid fills the main window). |
| `pixel_zoom` | boolean | true: zoom so one cell edge is cell_px pixels, the camera snapped to whole pixels. |
| `playing` | boolean | Run (true) or pause (false) generations. |
| `profile` | string | A screensaver profile by name (the state's `profiles` lists them; a complete setup): the screensaver shows it from now on, switching to it at once if running. |
| `refill` | boolean | true: rebuild the tiling now to fill the canvas at cell_px (periodic tilings), zoomed pixel-exact. |
| `rotation` | string | A rotation by name (the state's `rotations` lists them): the screensaver runs it from now on, switching to it at once if running. |
| `save_rotation` | object | Create or replace a rotation. It is {name, description, order (in_order, shuffle, daily, hourly), default_minutes (or null), look, script (Rhai), items: [{source, label, end, hold_s, look, schedule}]}. source is {kind: profile, name}, {kind: scene, path}, {kind: quick_start, id}, or {kind: series, rules, from, to, family, boundary, seed}. end is a list of {kind: minutes, minutes}, {kind: generations, gens}, {kind: settled, latest}, {kind: low_activity, below, for_gens}, {kind: done}, or {kind: clock, at: "HH:MM"}; any one ends the item. schedule is {days: ["mon", ...], from, to}. Settings: `jetcarp://schema/app_display/save_rotation`. |
| `saver` | object | Screensaver settings to change (saved). They are source (profile, rotation, current, scene, quickstart), profile (a profile name), rotation (a rotation name), scene (a scene file), quickstart (a Quick Start id), cell_px, refill, speed (0: max), gpu (these four for the current world, scene files, and Quick Start ids; profiles carry their own), keep_awake, exit_any_input (else Esc only), control (start the AI-control endpoint), hide_windows, cursor_hide_s, caption (true or false to show what is running, or an object with show, corner (BottomLeft, BottomRight, TopLeft, TopRight), size, generation, seconds). Settings: `jetcarp://schema/app_display/saver`. |
| `screensaver` | boolean | Start (true) or end (false) screensaver mode: no controls, the grid running on the outputs (or full screen). |
| `skip` | `"next"`, `"previous"` | While the screensaver runs a rotation: show the next or previous item now. |
| `speed` | number | Generations per second; 0 runs as fast as possible. |
| `status_bar` | boolean | Show the status bar. |
| `units` | `"pixels"`, `"logical"` | Span units: physical pixels, or logical pixels (for monitors of mixed scale factors). |
| `windows` | array of string | Outputs as plain windows instead of monitors, each "WxH@X,Y" (pixels, at canvas position X,Y), to try spans on one monitor; sets mode outputs unless mode is given. |

Example:

```json
{
  "name": "app_display",
  "arguments": {
    "screensaver": true,
    "profile": "Hodgepodge"
  }
}
```

## Resources

Resources are what a client can read without calling a tool. Clients that do not offer them can reach the same text through `help` and `reference`.

| URI | Type | What |
| --- | --- | --- |
| `jetcarp://reference/rules` | text/plain | Rules and rule notation: The named rules (name, notation, family, one line each) and how rule notation works. |
| `jetcarp://reference/tilings` | text/plain | Tilings: The tiling families with their neighbour counts, and the catalogue's tilings in summary. |
| `jetcarp://reference/palettes` | text/plain | Palettes: The built-in palettes with their background, stage and ramp colours. |
| `jetcarp://reference/script-api` | text/markdown | Scripting (Rhai) API: Every function a JETCARP script (run_script) can call. |
| `jetcarp://schema/scene.json` | application/schema+json | Scene file JSON Schema: The JSON Schema of a JETCARP scene file (world_status {"scene": true} returns one). |
| `jetcarp://schema/pack.json` | application/schema+json | Pack manifest JSON Schema: The JSON Schema of a pack version's manifest (pack.json in an export). |

The settings of 30 tool arguments, which `tools/list` only summarises (each argument names its resource, and its settings are listed with the tool above); `help {"schema": "search_start/hunt"}` returns the same JSON Schema:

- `world_read`: `cell` (`jetcarp://schema/world_read/<argument>`)
- `world_edit`: `cells`, `paste`, `reseed` (`jetcarp://schema/world_edit/<argument>`)
- `world_render`: `wallpaper` (`jetcarp://schema/world_render/<argument>`)
- `library_curate`: `changes`, `quickstart`, `compounds` (`jetcarp://schema/library_curate/<argument>`)
- `search_start`: `hunt`, `search`, `ants`, `census`, `mover`, `pile`, `collide`, `evolve`, `explore`, `budget` (`jetcarp://schema/search_start/<argument>`)
- `search_runs`: `sheet`, `scene`, `try`, `review` (`jetcarp://schema/search_runs/<argument>`)
- `pack`: `info`, `check`, `import`, `publish`, `export`, `site` (`jetcarp://schema/pack/<argument>`)
- `app_display`: `save_rotation`, `saver` (`jetcarp://schema/app_display/<argument>`)

And the 77 pages of this book, `jetcarp://docs/<page>` by their path without `.md` (`jetcarp://docs/guides/searching` is [Searching for patterns](../guides/searching.md)), as the server lists them:

<details><summary>The book's pages</summary>

- `jetcarp://docs/introduction`: Introduction
- `jetcarp://docs/getting-started/installing`: Installing JETCARP
- `jetcarp://docs/getting-started/first-run`: First run and setup
- `jetcarp://docs/getting-started/tour`: A tour of the app
- `jetcarp://docs/getting-started/core-ideas`: Core ideas: tilings, cell fields, rules, scenes
- `jetcarp://docs/tutorials/first-wallpaper`: Your first live wallpaper
- `jetcarp://docs/tutorials/life-on-a-new-tiling`: Life on a new tiling
- `jetcarp://docs/tutorials/hunting-spaceships`: 3. Hunting for spaceships
- `jetcarp://docs/tutorials/building-quick-start`: Building your own Quick Start
- `jetcarp://docs/tutorials/scripting-a-sweep`: 5. Scripting a rule sweep
- `jetcarp://docs/tutorials/mcp`: 6. Driving JETCARP from an AI assistant (MCP)
- `jetcarp://docs/guides/scenes`: Scenes: saving, sharing, reproducing
- `jetcarp://docs/guides/several-automata`: Several automata at once
- `jetcarp://docs/guides/drawing-tiles`: Drawing and editing tiles
- `jetcarp://docs/guides/palettes`: Palettes and colouring
- `jetcarp://docs/guides/reseeding`: Reseeding
- `jetcarp://docs/guides/wallpapers`: Wallpapers, screensavers and the background host
- `jetcarp://docs/guides/rotations`: Rotations
- `jetcarp://docs/guides/searching`: Searching for patterns
- `jetcarp://docs/guides/pattern-library`: The pattern library
- `jetcarp://docs/guides/packs`: Packs: importing, exporting, checking
- `jetcarp://docs/guides/pictures`: Pictures and animations
- `jetcarp://docs/guides/large-sandpiles`: Large sandpiles
- `jetcarp://docs/guides/hashlife`: HashLife and very long runs
- `jetcarp://docs/guides/aperiodic-patches`: Aperiodic patches and following movers
- `jetcarp://docs/guides/website-export`: Website export
- `jetcarp://docs/guides/uninstalling`: Uninstalling
- `jetcarp://docs/automata/generalising`: How JETCARP generalises automata to any tiling
- `jetcarp://docs/automata/life-like`: Life-like rules
- `jetcarp://docs/automata/generations`: Generations
- `jetcarp://docs/automata/larger-than-life`: Larger than Life
- `jetcarp://docs/automata/elementary`: Elementary (Wolfram) rules
- `jetcarp://docs/automata/hexagonal-rows`: Hexagonal row rules: HexW and HexT
- `jetcarp://docs/automata/layered`: Layered rules
- `jetcarp://docs/automata/jaca`: JACA: Jimmie's Annihilating Cellular Automata
- `jetcarp://docs/automata/turmites`: Langton's ant and turmites
- `jetcarp://docs/automata/wireworld`: WireWorld
- `jetcarp://docs/automata/cyclic`: Cyclic, Greenberg–Hastings and rock-paper-scissors
- `jetcarp://docs/automata/hodgepodge-forest-fire`: Hodgepodge and forest fire
- `jetcarp://docs/automata/margolus`: Margolus block rules
- `jetcarp://docs/automata/lenia`: Lenia
- `jetcarp://docs/automata/sandpile`: The Abelian sandpile
- `jetcarp://docs/automata/von-neumann`: Von Neumann's 29-state automaton
- `jetcarp://docs/automata/priority-rules`: Priority rules and the Spectre glider
- `jetcarp://docs/automata/rule-tables`: Rule tables
- `jetcarp://docs/automata/named-rules`: Named rule catalogue
- `jetcarp://docs/tilings/neighbour-graphs`: Tilings as neighbour graphs
- `jetcarp://docs/tilings/regular-archimedean`: Regular and Archimedean tilings
- `jetcarp://docs/tilings/k-uniform`: k-uniform tilings
- `jetcarp://docs/tilings/isohedral`: Isohedral tilings
- `jetcarp://docs/tilings/editable-families`: Editable tile families
- `jetcarp://docs/tilings/penrose`: Penrose tilings
- `jetcarp://docs/tilings/hat-spectre`: The hat and the spectre
- `jetcarp://docs/tilings/catalogue`: Tiling catalogue
- `jetcarp://docs/tilings/catalogue-kuniform`: k-uniform tilings, k = 4 to 6
- `jetcarp://docs/reference/cli`: Command line
- `jetcarp://docs/reference/rule-notation`: Rule notation
- `jetcarp://docs/reference/scene-format`: Scene file format
- `jetcarp://docs/reference/library-format`: Library format
- `jetcarp://docs/reference/search-runs`: Search runs
- `jetcarp://docs/reference/pack-format`: Pack format
- `jetcarp://docs/reference/scripting`: Scripting (Rhai) API
- `jetcarp://docs/reference/mcp`: MCP tools
- `jetcarp://docs/reference/palettes`: Palettes
- `jetcarp://docs/reference/folders`: Folders and environment variables
- `jetcarp://docs/reference/glossary`: Glossary
- `jetcarp://docs/research/reproducibility`: Reproducibility
- `jetcarp://docs/research/search-methods`: Search methods
- `jetcarp://docs/research/contributions`: What is new in JETCARP
- `jetcarp://docs/research/findings`: Findings
- `jetcarp://docs/research/bibliography`: Bibliography
- `jetcarp://docs/about/citing`: Citing JETCARP
- `jetcarp://docs/about/licence`: Licence
- `jetcarp://docs/about/credits`: Credits and acknowledgements
- `jetcarp://docs/about/how-made`: How JETCARP was made
- `jetcarp://docs/about/changelog`: Changelog
- `jetcarp://docs/about/contributing`: Contributing

</details>

Templates, for one record at a time:

| URI template | Type | What |
| --- | --- | --- |
| `jetcarp://docs/{page}` | text/markdown | A page of the JETCARP book: A page by its path without .md, e.g. jetcarp://docs/guides/searching (resources/list lists every page). |
| `jetcarp://schema/{tool}/{argument}` | application/schema+json | A tool argument's settings: The JSON Schema of a tool argument's settings, for the arguments tools/list only summarises, e.g. jetcarp://schema/search_start/hunt or jetcarp://schema/pack/publish (resources/list lists every one). |
| `jetcarp://job/{id}` | application/json | A library job: One search job's status and progress (as job_status reports it), by its id. |
| `jetcarp://pattern/{id}` | application/json | A library pattern: One pattern's record (rules, range, sightings, the cells of its phases), by id, unique prefix, or name. |
| `jetcarp://quickstart/{handle}` | application/json | A Quick Start entry: One Quick Start entry with its scene, by handle (life, find:<id>, atlas:<...>). |

<!-- END GENERATED: docs/tools/gen-mcp.py -->
