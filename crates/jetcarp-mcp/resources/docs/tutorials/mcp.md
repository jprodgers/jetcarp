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

```sh
claude mcp add --scope user jetcarp -- jetcarp mcp
```

`--scope user` adds JETCARP for every folder you start Claude Code in. Without it, Claude Code adds
it only for the folder you ran the command in, and a session started anywhere else does not have it.
To check, start a new session and type `/mcp`: JETCARP is listed, with its tools. `claude mcp list`
lists it too. To remove it again:

```sh
claude mcp remove jetcarp
```

**Claude Desktop.** Add JETCARP to its `claude_desktop_config.json` and restart Claude Desktop. The
file is in Claude's settings folder (`~/Library/Application Support/Claude/` on macOS,
`%APPDATA%\Claude\` on Windows); older versions open it from *Settings → Developer → Edit Config*,
and newer ones list the servers it starts under *Settings → Customize*:

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

**Clients that connect to a URL (HTTP).** A few clients don't start a program: they connect to a
server's address instead (Postman, web-based tools, remote setups). For them, `jetcarp mcp --http`
serves the same tools over MCP's *Streamable HTTP* transport, on your computer only. Start it in a
terminal and leave it running:

```sh
jetcarp mcp --http
```

```text
JETCARP's MCP server is answering HTTP clients (MCP Streamable HTTP) on this computer only.
  URL:    http://127.0.0.1:47812/mcp
  Header: Authorization: Bearer 3f9c0a5e2b7d41c8a6e09f1d2c4b8a73
It listens on 127.0.0.1, which other computers cannot reach. The token is new each time it starts, and is recorded in /run/user/1000/jetcarp/mcp-http-endpoint.json, which only you can read.
Each session has a private world of its own. Ctrl-C stops the server.
```

Give the client the URL and the header. A client that asks for a *bearer token* wants only the
token, the part after `Bearer`. Claude Code can connect this way too, though `jetcarp mcp` itself
suits it better:

```sh
claude mcp add --transport http jetcarp-http http://127.0.0.1:47812/mcp \
  --header "Authorization: Bearer 3f9c0a5e2b7d41c8a6e09f1d2c4b8a73"
```

The token is new each time the server starts, so the client needs the new one each time, unless
you ask for it to be kept: `jetcarp mcp --http --keep-token` keeps it for the next start, in the
same file, and `--new-token` replaces it with a new one when you want the old one to stop working.
Keeping it saves setting the client up again, but whoever learns it keeps their access until you
make a new one.

A client that runs in a web browser sends its page's origin, and the server refuses every origin
but its own unless you name it: `--allow-origin http://localhost:6274` lets that one page connect
(with the token, still), and you can give the option once for each. Name only pages you trust,
exactly; no wildcards are accepted.

To try the endpoint by hand, `curl` starts a session (give it your own token):

```sh
curl -i http://127.0.0.1:47812/mcp \
  -H 'Authorization: Bearer 3f9c0a5e2b7d41c8a6e09f1d2c4b8a73' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "curl", "version": "1"}}}'
```

The answer's `Mcp-Session-Id` header names the session, and every later request sends it too. The
[MCP tools](../reference/mcp.md#over-http) reference has the rest: a tool call, how a session ends,
and what the server refuses. Only programs on your computer can connect, and only with the token.

To check the connection, ask the assistant *"What can the JETCARP tools do?"* A good assistant
calls `help` first: it explains how the tools fit together, with worked examples.

If it answers without calling a JETCARP tool (asked to "run Brian's Brain", Claude Desktop once
wrote a simulation of its own instead), say so in the request: *"Use the JETCARP tools to …"*, and
check that JETCARP is switched on in the chat's tools menu.

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
Generation 200. Status: Running. Hash fb9769dc44b916fb432c12e3bbf0f5a2420a8c9644b46044096e5bc642892ecb ...
field 0 [Brian's Brain (id 1)]: 690 live

640 x 427 px picture of generation 200 (Square (parallelogram), 9600 cells; hash fb9769dc44b916fb432c12e3bbf0f5a2420a8c9644b46044096e5bc642892ecb).
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
hunt 1: 20,000/20,000 soups, 2816 soups/s, 433,183 objects (ships 38,381, osc 125,411,
still 269,391), 50 distinct, 50 new | newest: still life 12c #bf73c4 (B3/S23)
  status done, 7 s run, 19946 settled, 52 died, 2 exploded, 0 unsettled
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
  {"id": "5a080db389c0cab3caf884d2596a1e70", "label": "glider", "speed": "c/4 diagonal", "cells": 5, "seen": 38312, ...},
  {"id": "77364004b6d2c23cba74126b8aeb6ca3", "label": "MWSS", "speed": "c/2 orthogonal", "cells": 11, "seen": 13, ...},
  {"id": "6dd55e69689ba784aff448b7dbbc9328", "label": "LWSS", "speed": "c/2 orthogonal", "cells": 9, "seen": 52, ...}]}
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
sightings: B3/S23 x52 (job 1)
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
   it has just made. The address the window shows is the private link through which JETCARP's MCP
   server reaches the app, not an MCP endpoint of its own: it speaks JETCARP's protocol, not HTTP,
   so point the assistant at `jetcarp mcp` (or `jetcarp mcp --app`, which the window shows with a
   *Copy* button), never at the address. (A client that connects to a URL has an endpoint of its
   own: see [Over HTTP](#over-http) below.)

   <img src="../images/tutorials/mcp-ai-control.webp" alt="The AI control window: a line on connecting an assistant to JETCARP's MCP server with the command below it, the command jetcarp mcp --app with a Copy button, Allow AI control (127.0.0.1 only) ticked, the address 127.0.0.1:47811, the start of the token with a Copy button, Open connections 0, Tool calls 0, and a note that the address is the private link that command uses to reach the window, not an MCP endpoint for HTTP clients; below it, Also serve MCP over HTTP (127.0.0.1 only) ticked, the URL http://127.0.0.1:47812/mcp, the whole token and the header Authorization: Bearer, each with a Copy button, Open connections 0, Sessions 0, a note on what to give the client, Keep this token for next time (not ticked), a New token button, and Web pages that may connect, listing http://localhost:6274 with a Remove button above a field for another origin and an Allow button" width="435">

2. Ask the assistant to use the app. It calls `use_app`, which finds the app's port and token in
   JETCARP's runtime folder and attaches; from then on the world tools act on the window you are
   watching, and every change shows up in its command log. `use_app {"attach": false}` goes back to
   the private world.

The app goes on answering while its window is minimised, covered or on another desktop, so you can
set the assistant a task and switch to something else.

To start attached every time, register the server with `--app` as well:

```sh
claude mcp add --scope user jetcarp-app -- jetcarp mcp --app
```

<a id="over-http"></a>**Over HTTP.** A client that connects to a URL can drive the app's world
without `jetcarp mcp`: with AI control on, tick *Also serve MCP over HTTP* in the same window. It
shows the URL (`http://127.0.0.1:47812/mcp`, or another port when that one is taken), the token and
the header, each with a *Copy* button, for the client's settings. Every session acts on the world
in the window, with the tools AI control offers (not `use_app` and `host_control`, which belong to
`jetcarp mcp`). Unticking it, or switching AI control off, closes it, and it is off again at the
next launch. `jetcarp mcp --http --app` does the same from a terminal, with `jetcarp mcp`'s tools.

Below the token, two choices, both off until you make them, are saved with the app's settings:
*Keep this token for next time* gives the client the same token the next time you tick the box,
and *New token* replaces it at once (the old one stops working, and its clients must connect
again). Under *Web pages that may connect*, type the exact origin of a browser-based client you
trust, such as `http://localhost:6274`, and press *Allow*; *Remove* takes it off the list again.

Attached to the app, the assistant also gets `app_display`: where the grid shows (window, full
screen, a span of monitors), the screensaver and its profiles, the cell size and keep-awake. Ask
*"Show Hodgepodge as the screensaver"*, and it calls
`app_display {"screensaver": true, "profile": "Hodgepodge"}`.

The [background host](../guides/wallpapers.md), which keeps scenes running as wallpapers without
the app, has a tool of its own, `host_control`: its status, the next or previous item, pause and
resume, which rotation runs, and whether the scenes are the desktop background. The host must be
running already (`jetcarp host start`); the tool does not start it. Until you have answered the
[photosensitive seizure warning](../getting-started/photosensitivity.md), in the app or with
`jetcarp setup`, it refuses to start or show scenes, with the warning; an assistant cannot answer
it for you.

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
- **Searches.** A search runs on a thread of the server; when the session ends, the server stops it
  at its next checkpoint and leaves it paused and resumable (`search_start {"resume": job}`), from
  the app, the command line or a later session.
  Explore searches, which run for hours, start in a process of their own that outlives the session
  (under `jetcarp mcp`); `job_stop` stops any job, wherever it runs.
- **What it cannot do.** It cannot make your pack-signing key or record anyone's consent to publish
  their ratings: those are for you, at a command line (`jetcarp pack key`, `jetcarp pack rater`).
  Removing records (`library_curate` with `remove`) is allowed; a careful assistant asks first.
- **AI control and HTTP.** Switch them on only while you use them. The HTTP endpoint, from
  `jetcarp mcp --http` or the app, answers only on `127.0.0.1` and only with its token, and refuses
  requests from web pages (an `Origin` other than its own) unless you have named their origin, so
  a website open in your browser cannot reach it. A kept token, and each origin you name, trade a
  little of that for convenience: [`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)
  says what each one gives up.

## Where next

- [MCP tools](../reference/mcp.md): every tool, its arguments and an example, and the resources
  (the pages of this book among them: an assistant can read any page as
  `jetcarp://docs/<page>`, or with `help {"page": ...}`).
- [Scripting a rule sweep](scripting-a-sweep.md): an assistant can write and run scripts like these
  with `run_script`.
- [Searching for patterns](../guides/searching.md) and [The pattern library](../guides/pattern-library.md).
