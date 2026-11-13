# Changelog

All notable changes to JETCARP are listed here, newest first, in the style of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before version 1.0, a release may still
change the scene format, the command line, the scripting API and the MCP tools; 1.0 is the release
that will declare them stable.

## [Unreleased]

### Fixed

- **Popping a window out could crash the app.** The ↗ button in a tool window's title bar, or
  *Own window* for a pane, closed the whole app when the window held a table (the AI control
  window with AI control on, among others). Popped-out windows now draw as the main window does.
  ([#1](https://github.com/jprodgers/jetcarp/issues/1))
- **The AI control window opens on the Quick Start.** It showed only with a world open, so from
  the Quick Start, *Tools → AI control…* ticked in the menu but showed nothing, and AI control could
  not be switched on there. ([#1](https://github.com/jprodgers/jetcarp/issues/1))
- **A newer version of a pack replaces its Quick Start entries.** Importing one kept every entry
  the earlier version had brought, so a library that had a release candidate's starter pack showed
  both versions' entries, some of them twice. Entries the new version no longer has now leave the
  Quick Start, unless you hid or rated them. To tidy a library that already has both, import the
  starter pack again: `jetcarp pack import <folder>/jetcarp-starter`, where `jetcarp paths` names the
  folder as *bundled*.
- **Packs that came with a Homebrew or Scoop install are named by the link that stays.** `jetcarp
  paths` and the setup's import job named Homebrew's `Cellar/jetcarp/<version>` and Scoop's
  `apps\jetcarp\<version>`, folders the next upgrade deletes. They now name
  `/opt/homebrew/share/jetcarp/packs` and `apps\jetcarp\current\packs`.
- **`jetcarp jobs status` on a hunt that came with a pack** printed a hunt's empty counters ("0
  soups … status , 0 s run"). It now says the hunt was imported, as the Hunts window does, and
  names the pack rather than its `sha256:` hash, here and in `jobs list`. Packs imported from now
  on are named by their title (*JETCARP starter pack*) there and in the Hunts window.

### Changed

- **More atlas titles tell entries apart.** A JACA entry is titled by its rules up to 40 characters
  (*Battle JACA +B14/S124 -B12/S345 blast 1*, not a plain *Battle JACA*), and a Priority entry by its
  number of states (*Oscillating Priority 6*). Entries from earlier builds keep their titles.

## [0.4.0] – 2026-11-13

The first public release.

### What JETCARP 0.4.0 does

- **Tilings.** Squares, hexagons and triangles, all eleven Archimedean tilings, the 1,248 k-uniform
  tilings up to k = 6, the 81 isohedral tile types, tiles you draw yourself, and the aperiodic
  Penrose (kites and darts, and rhombs), hat and spectre tilings, built on exact patches.
- **Automata.** Life and the Life-like rules, Generations, Larger than Life, elementary rules on
  squares and hexagons, Langton's ant and the turmites, WireWorld, Greenberg–Hastings, the cyclic
  and rock-paper-scissors rules, hodgepodge, sandpiles, Lenia and other continuous rules, JACA
  (Jimmie Rodgers's annihilating two-sided Life), layered rules, Margolus block rules, rule tables
  and von Neumann's 29-state automaton. Several can run at once, on shared or separate cell fields.
- **The app.** A Quick Start menu of examples, an editor for worlds, tiles and unit cells, thirty
  palettes and your own, pictures and animations, and screensaver mode.
- **On the desktop.** A background host that runs scenes as a live wallpaper (Windows, macOS, and
  GNOME through its own extension), on the GNOME lock screen, as the system's screen saver (Windows
  and macOS), or as a still picture; see [Wallpapers](../guides/wallpapers.md).
- **One program for every front end.** `jetcarp` opens the app, and its subcommands are the command
  line, the background host (`jetcarp host`) and an MCP server for AI assistants (`jetcarp mcp`).
  Rhai scripts run worlds and parallel rule sweeps. All of them call one layer of 79 named
  operations, each with JSON Schemas for its input and output (`jetcarp api list`).
- **AI assistants.** `jetcarp mcp` gives an assistant such as Claude Code or Claude Desktop 21 tools
  over those operations (worlds, scripts, the library, searches, media and packs), the app's
  displays when it drives the app, and this book as resources, within folder and time limits; see
  [Driving JETCARP from an AI assistant](../tutorials/mcp.md).
- **Searches.** Nine search strategies (soup hunts on the plane and on bounded grids, a rule census,
  ants and turmites, movers on aperiodic tilings, large sandpiles, collisions, an evolutionary glider
  search, and an exploratory search over tilings, rule families and rules) run as resumable jobs
  whose results don't depend on the thread count; see [Search methods](../research/search-methods.md).
- **A verified pattern library.** Every catalogued pattern is verified exactly, named by a content
  id that any program can recompute, and pinned to its tiling's geometry and its rule family's
  meaning, so a change to either is caught; see [Library format](../reference/library-format.md).
- **Packs.** Format 1.0 for sharing data: signed, content-addressed stores of Parquet tables and
  canonical JSON that other tools read without JETCARP, checked hash by hash and id by id on import;
  ratings go in only with each rater's consent. See [Packs](../guides/packs.md) and
  [Pack format](../reference/pack-format.md).
- **A starter pack** beside the program: 79 Quick Start entries that Jimmie Rodgers picked (36 from
  the atlas of an exploratory search, 18 spaceships, 8 oscillators, 8 ant and turmite highways, 7
  sandpile identities and 2 hat gliders), with the 26 pattern records and 8 ant records they show.
  The 63 built-in examples picked with them are featured in the Quick Start menu. See
  [The starter pack](../getting-started/installing.md#the-starter-pack).
- **Reproducibility.** Every run is a scene that replays bit for bit to a recorded state hash, and
  every picture JETCARP saves carries its scene; see [Reproducibility](../research/reproducibility.md).
- **Installing.** Archives for Linux (x86_64 and ARM, glibc 2.28 or newer), Windows and macOS (Apple
  silicon and Intel), installers that check every download, Python wheels for pipx, uv and pip, a
  Homebrew tap, a Scoop bucket and crates.io, with a starter pack beside the program; see
  [Installing JETCARP](../getting-started/installing.md).
- **First-run setup.** On its first launch the app asks what JETCARP is for, in five profiles
  (Desktop art, Explorer, Researcher, Everything, Blank slate), and sets it up: a data folder of your
  choosing for big libraries, the starter pack and packs of your own imported, a Quick Start build
  that searches for so many minutes and adds its best runs to the Quick Start, and the live
  wallpaper and screen saver offered, never turned on unasked. The work runs as background jobs;
  `jetcarp setup` does the same in a terminal. See [First run and setup](../getting-started/first-run.md).
- **No network, no telemetry.** JETCARP makes no network connections; see `SECURITY.md`.
- **This book**, with a page on the history and credits of every automaton and tiling.

JETCARP grew out of CART, a private prototype Jimmie Rodgers built in early November 2026, and was
reworked for release in a new repository with a fresh history; see
[How JETCARP was made](how-made.md).
