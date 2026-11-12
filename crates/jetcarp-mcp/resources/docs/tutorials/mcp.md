# 6. Driving JETCARP from an AI assistant (MCP)

In this tutorial you connect an AI assistant to JETCARP and let it do the work: build a world, run
it, show you a picture, hunt for spaceships, and read what it found. JETCARP speaks the
[Model Context Protocol](https://modelcontextprotocol.io) (MCP), so any assistant that does
(Claude Code, Claude Desktop, LM Studio and others) can use it. It takes about fifteen minutes.

You need JETCARP installed ([Installing](../getting-started/installing.md)) and an assistant that
supports MCP. You talk to the assistant in plain words; it decides which tools to call. This page
shows the tool calls it made in a real session, with their answers trimmed, so you can see what
happens underneath and recognise it in your own.

## 1. Connect your assistant

The assistant starts JETCARP's server itself, as `jetcarp mcp`, and talks to it over standard
input and output for as long as the session lasts. There is nothing to keep running.

**Claude Code.** In a terminal:

```text
$ claude mcp add jetcarp -- jetcarp mcp
```

That adds JETCARP to the project you are in; add `--scope user` before `jetcarp` to have it in
every project. `claude mcp list` shows it, and in a session `/mcp` shows its tools.

**Claude Desktop.** Open *Settings → Developer → Edit Config*, add JETCARP to the
`claude_desktop_config.json` it opens, and restart Claude Desktop:

```json
{
  "mcpServers": {
    "jetcarp": {"command": "jetcarp", "args": ["mcp"]}
  }
}
```

If Claude Desktop says it cannot find the program, give its full path as `command`: `which jetcarp`
prints it (on Windows, `where jetcarp`).

**Other clients.** Most take the same JSON, or ask for a command (`jetcarp`) and its arguments
(`mcp`) in a form. The [MCP tools](../reference/mcp.md) reference lists the server's options.

To check the connection, ask the assistant *"What can the JETCARP tools do?"* A good assistant
calls `help` first: it explains how the tools fit together, with worked examples.

## 2. A world, a run and a picture

Ask: *"Run Brian's Brain on a 120 by 80 square grid for 200 generations and show me."* The
assistant builds the world, steps it, and renders it:

```json
{"name": "world_new", "arguments": {"tiling": "square", "width": 120, "height": 80,
  "boundary": "wrap", "cas": ["Brian's Brain"]}}
{"name": "world_step", "arguments": {"generations": 200}}
{"name": "world_render", "arguments": {"max_side": 640, "save_path": "brain.png"}}
```

```text
New world created.
{ "cas": [{"id": 1, "name": "Brian's Brain", "rule": "B2/S/C3", ...}], "cells": 9600, ... }

Ran 200 generation(s) in 0.04s.
Generation 200. Status: Running. Hash c38568895709dcc6 ...
field 0 [Brian's Brain (id 1)]: 690 live

640 x 427 px picture of generation 200 (Square (parallelogram), 9600 cells; hash c38568895709dcc6).
Saved to /home/ada/Pictures/JETCARP/brain.png.
```

The picture comes back to the assistant. Claude Desktop shows it to you in the conversation; a
terminal client such as Claude Code can't show pictures, so ask for it to be saved ("…and save the
picture as brain.png") and open the file from JETCARP's output folder:

![Brian's Brain after 200 generations, from the session](../images/tutorials/mcp-brians-brain.png)

A few things to notice:

- The world lives inside this server, private to the session; the app is not affected. It starts
  from the rule's default seed, a random soup, and the same arguments give the same world on any
  computer: your hash will match the one above.
- `save_path` is relative, so the picture went to JETCARP's output folder. The tools read and write
  only there, in the library's folder, and in folders you allow (see [Limits and
  safety](#5-limits-and-safety)).
- Had the assistant tried Brian's Brain on hexagons, it would have died out by generation 16 (each
  cell has only six neighbours there), and `world_step` would have said so and suggested another
  rule. Rules behave differently on every tiling; that is what JETCARP is for.

Then ask for something exact: *"Draw a glider in Life and tell me where it is after 8
generations."*

```json
{"name": "world_new", "arguments": {"tiling": "square", "width": 40, "height": 40,
  "cas": [{"rule": "Life", "seed": "empty"}]}}
{"name": "world_edit", "arguments": {"cells": {"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}}}
{"name": "world_step", "arguments": {"generations": 8}}
{"name": "world_read", "arguments": {}}
{"name": "world_read", "arguments": {"format": "objects"}}
```

```text
Set 5 cell(s) in CA 'Life' (id 1). Pattern box: x 5..7, y 5..7. Field population now 5. Generation 0.

CA 'Life' (id 1, B3/S23), field 0, generation 8.
Grid 40 x 40. Non-empty cells in the whole field: 5; bounding box x 7..9, y 7..9 (3 x 3).
x units 67890
y=6     .....
y=7     ..O..
y=8     ...O.
y=9     .OOO.
y=10    .....

Generation 8, CA 1 (B3/S23): 5 live cells. glider (c/4) x 1
    0  glider (c/4)                    5 cells  x 7..9, y 7..9
```

The glider moved two cells right and two down, as gliders do every eight generations. The
`objects` format runs JETCARP's object finder over the field, so the assistant does not have to
recognise patterns by eye.

## 3. A hunt and its finds

Ask: *"Run 20,000 Life soups and tell me which spaceships turned up."* Long searches are jobs:
`search_start` starts one in the background, and `wait_s` waits for it to end (up to ten minutes)
so the assistant needs no polling:

```json
{"name": "search_start", "arguments": {"hunt": {"rules": ["B3/S23"], "soups": 20000}, "wait_s": 120}}
```

```text
Hunt job 1 started (background).
hunt 1: 20,000/20,000 soups, 2816 soups/s, 430,384 objects (ships 38,188, osc 124,582,
still 267,614), 50 distinct, 50 new | newest: still life 12c #bf73c4 (B3/S23)
  status done, 7 s run, 19892 settled, 52 died, 56 exploded, 0 unsettled
  newest finds:
    bf73c46c186561ca3b75a3884d0b5ab5  still life 12c #bf73c4   B3/S23   Square soup 17973  (new to the catalog)
    ...
{"done":20000,"job":1,"kind":"hunt","status":"done","total":20000,"unit":"soups","waited_s":7.2, ...}
```

Every object the hunt saw was classed, and each distinct one was verified and stored in the
library. The assistant reads the spaceships back:

```json
{"name": "library_query", "arguments": {"class": "spaceship", "limit": 5}}
```

```text
{"matching": 4, "patterns": [
  {"id": "1376072ec0c084fc67a9b92d361379f3", "label": "HWSS", "speed": "c/2 orthogonal", "cells": 13, "seen": 4, ...},
  {"id": "5a080db389c0cab3caf884d2596a1e70", "label": "glider", "speed": "c/4 diagonal", "cells": 5, "seen": 38120, ...},
  {"id": "77364004b6d2c23cba74126b8aeb6ca3", "label": "MWSS", "speed": "c/2 orthogonal", "cells": 11, "seen": 13, ...},
  {"id": "6dd55e69689ba784aff448b7dbbc9328", "label": "LWSS", "speed": "c/2 orthogonal", "cells": 9, "seen": 51, ...}]}
```

All four of Life's common spaceships, best first. `library_query` with an `id` gives one record in
full; ids may be shortened to a unique prefix, or given as a name:

```json
{"name": "library_query", "arguments": {"id": "LWSS"}}
```

```text
LWSS (6dd55e69689ba784aff448b7dbbc9328)
verified in: B3/S23
works in: B3/S23 to B3468/S2378 (32 B/S rules)
sightings: B3/S23 x51 (job 1)
moves per period by paste orientation: 0: (-2, 0), 1: (2, 0), 2: (2, 0), 3: (0, 2), ...
phase 0:
OOOO.
O...O
O....
.O..O
```

`works in` is the range of B/S rules the LWSS runs identically in, 32 of them. To watch it fly, the assistant opens it on a grid sized for it, or pastes it into a world of its own
with `world_edit {"paste": {"id": "LWSS"}}`:

```json
{"name": "world_new", "arguments": {"pattern": "LWSS"}}
{"name": "world_step", "arguments": {"generations": 4}}
{"name": "world_read", "arguments": {}}
```

```text
Grid 48 x 32. Non-empty cells in the whole field: 9; bounding box x 19..23, y 14..17 (5 x 4).
y=14    .OOOO..
y=15    .O...O.
y=16    .O.....
y=17    ..O..O.
```

And to keep it where you can find it in the app, ask *"Add the LWSS to my Quick Start."*:

```json
{"name": "library_curate", "arguments": {"quickstart": {"pattern": "LWSS", "title": "Lightweight spaceship"}}}
```

```text
promoted: Lightweight spaceship
entry:    find:6dd55e69689ba784aff448b7dbbc9328 in /home/ada/.local/share/jetcarp/library.sqlite
It appears in the app's Quick Start under "My finds".
```

The library is the same one the app, the command line and your scripts use, so the hunt's finds and
the new Quick Start entry are there the next time you open the app. `library_curate` also renames,
tags, rates and hides records; an assistant that writes notes should mark them as its own.

The same tools reach every search JETCARP has: ant and turmite searches, censuses, collisions,
large sandpiles, movers on aperiodic tilings, the evolutionary glider search, and the broad explore
search, whose runs `search_runs` reads, draws on a contact sheet, opens and reviews. Ask the
assistant for `help {"topic": "searches"}` to see them all, or read
[Searching for patterns](../guides/searching.md).

## 4. In the app: AI control, and the background host

So far the assistant's world was private. To watch it work, let it drive the world in the app's
window instead:

1. In the app, open *Tools → AI control…* and tick *Allow AI control* (or start the app with
   `jetcarp --control`). The app now listens on `127.0.0.1` only, for programs that know the token
   it has just made.
2. Ask the assistant to use the app. It calls `use_app`, which finds the app's port and token in
   JETCARP's runtime folder and attaches; from then on the world tools act on the window you are
   watching, and every change shows up in its command log. `use_app {"attach": false}` goes back to
   the private world.

To start attached every time, register the server with `--app`
(`claude mcp add jetcarp-app -- jetcarp mcp --app`). Attached to the app, the assistant also gets
`app_display`: where the grid shows (window, full screen, a span of monitors), the screensaver and
its profiles, the cell size and keep-awake. Ask *"Show Hodgepodge as the screensaver"*, and it
calls `app_display {"screensaver": true, "profile": "Hodgepodge"}`.

The [background host](../guides/wallpapers.md), which keeps scenes running as wallpapers without
the app, has a tool of its own, `host_control`: its status, the next or previous item, pause and
resume, which rotation runs, and whether the scenes are the desktop background. The host must be
running already (`jetcarp host start`); the tool does not start it.

Switch AI control off when you are done: while it is on, any program running as you can read the
token and steer the app. See [`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md).

## 5. Limits and safety

The tools are made so that an assistant cannot do much harm by mistake, and cannot stall:

- **Folders.** The tools read and write files only in JETCARP's output folder, the library's
  folder, and folders you name with `jetcarp mcp --allow DIR`; they also read the packs that came
  with JETCARP. A path anywhere else is refused, so an assistant cannot save over your documents or
  read your files. Relative paths start in the output folder.
- **Time and size.** Worlds hold at most 4 million cells, and one `world_step` runs at most
  1,000,000 generations or 2 billion cell updates. `run_script` runs a Rhai script for 60 seconds
  by default and 600 at most, writes only inside the output folder and reads only there. Waits for
  a job last ten minutes at most. When the assistant cancels a request, the call stops soon.
- **Searches.** A search runs on a thread of the server and stops when the session ends; it stays
  resumable (`search_start {"resume": job}`), from the app, the command line or a later session.
  Explore searches, which run for hours, start in a process of their own that outlives the session
  (under `jetcarp mcp`); `job_stop` stops any job, wherever it runs.
- **What it cannot do.** It cannot make your pack-signing key or record anyone's consent to publish
  their ratings: those are for you, at a command line (`jetcarp pack key`, `jetcarp pack rater`).
  Removing records (`library_curate` with `remove`) is allowed; a careful assistant asks first.
- **AI control.** Switch it on only while you use it.

## Where next

- [MCP tools](../reference/mcp.md): every tool, its arguments and an example, and the resources
  (the pages of this book among them: an assistant can read any page as
  `jetcarp://docs/<page>`, or with `help {"page": ...}`).
- [Scripting a rule sweep](scripting-a-sweep.md): an assistant can write and run scripts like these
  with `run_script`.
- [Searching for patterns](../guides/searching.md) and [The pattern library](../guides/pattern-library.md).
