# Changelog

All notable changes to JETCARP are listed here, newest first, in the style of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before version 1.0, a release may still
change the scene format, the command line, the scripting API and the MCP tools; 1.0 is the release
that will declare them stable.

## [0.4.0] – unreleased

The first public release. JETCARP is CART, an earlier private project of Jimmie Rodgers's, renamed
and reworked for release in a new repository with a fresh history.

### For anyone who used CART

- **The name.** Every program, crate, identifier, environment variable and piece of interface text
  changed from CART to JETCARP. Environment variables now start with `JETCARP_`, the app's id is
  `com.jimmieprodgers.jetcarp`, the GNOME extension is `jetcarp@jimmieprodgers.com`, and sandpile files
  start with `JETCPILE`.
- **Folders.** JETCARP keeps its files in the operating system's own folders for settings, data and
  output (`~/Pictures/JETCARP` for output) instead of `~/Documents/cartout`, and each folder has an
  override; see [Folders and environment variables](../reference/folders.md).
- **One program.** `jetcarp` with no subcommand opens the app, `jetcarp host` controls the background
  host (`jetcarp host serve` runs it), and `jetcarp mcp` is the MCP server. On Windows, `jetcarpw.exe`
  starts the program without a console window.
- **Search commands.** `jetcarp search <strategy>` (`hunt`, `bounded`, `ants`, `census`, `mover`,
  `pile`, `collide`, `evolve`) and `jetcarp jobs` (`list`, `status`, `stop`, `resume`, `report`,
  `remove`) replace `jetcarp hunt`, `evolve`, `census`, `movers`, `ants` and `library search`,
  `library collide` and `library jobs`, which are gone, with no aliases. The commands for stored ant
  runs are now under `jetcarp library ants`, and those for movers over fixed patch levels under
  `jetcarp library movers`. `jetcarp pile` stays. The MCP tools kept their names.
- **JSON output.** `--json` on any command that reports a result prints it as JSON. The options that
  wrote a JSON report to a file (on the census, mover tracking, library search and collision
  commands) are now `--json-out FILE`.
- **Old files.** JETCARP does not read CART's files. CART's pattern catalog and its long exploratory
  search were brought into JETCARP once, before this release, and checked on it (see
  [Records from CART](../reference/library-format.md#records-from-cart) and
  [The search imported from CART](../reference/search-runs.md#the-search-imported-from-cart)).
- **Penrose rhombs.** The P3 (rhomb) construction was rewritten (see *Changed*), so `penrose-rhomb`
  lays out its tiles differently from CART's, and what ran on it in CART does not replay to the same
  hashes. The kites-and-darts tiling (P2) is unchanged.
- **Removed features.** The stage-1 settings search, its score models and the catalogue pipeline built
  on it are gone, with `jetcarp stage1`, `jetcarp pipeline`, `jetcarp media render --setting`, the MCP
  tool `stage1_start` and the matching script functions; so is the Quick Start approval queue. The
  stage-1 tables stay in old catalogues but are no longer read.

### Added

- `jetcarp paths`, which prints the settings, data and output folders; `jetcarp doctor`, which reports
  the version, folders, graphics adapters and desktop integration for bug reports; and
  `jetcarp uninstall`, which undoes what JETCARP set up outside its folders and, if asked, deletes its
  settings and data.
- A shared layer of 74 named operations that the command line, the scripts and the MCP server call,
  each with JSON Schemas for its input and output: `jetcarp api list`, `jetcarp api schema OP` and
  `jetcarp api call OP JSON`. Scripts can call any of them with `jetcarp(op, #{…})`.
- One job runner for every kind of search. Each job runs in steps of a few seconds with a checkpoint
  after each, can be stopped from any process and resumed, ends exactly as an uninterrupted run would,
  takes a lease so that two runners never own one job, and is seen as stalled, and resumable, if the
  process running it dies. Searches take `--minutes` and `--limit` budgets, `--label`, `--detach` and
  `--resume`, and `jetcarp jobs report` writes a Markdown summary of what a job searched and found.
- Dynamics pins: every built-in Quick Start entry must reach a recorded state hash (see
  [Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)).
- Rule-semantics versions: every rule family's meaning has a version (all 1), pinned by one scene per
  family; every library pattern carries its family's version, and one verified under another version
  is flagged for re-verification, as after a geometry change (see
  [Library format](../reference/library-format.md#rule-semantics-versions)).
- One spelling for a pattern id's tiling line: its values are RFC 8785 canonical JSON, so another
  program can write the same line and check an id (see
  [Library format](../reference/library-format.md#the-identity-text-of-a-pattern)). No id of a tiling
  at its default shape changed.
- Packs, format 1.0: `jetcarp pack publish` writes the library (or a selection) as a signed,
  content-addressed store of Parquet tables and canonical JSON that pyarrow, DuckDB or Polars read
  without JETCARP, `jetcarp pack export` lays a version out for a single download, and
  `jetcarp pack check` and `jetcarp pack import` check every hash, id and SSH signature (verified in
  pure Rust) before anything is stored. Ratings are published only with each rater's consent. See
  [Pack format](../reference/pack-format.md).
- Continuous integration that builds and tests on Linux, Windows and macOS, and builds for macOS on
  Intel and Linux on ARM.
- Licences: JETCARP's own code and data under the Unlicense or the MIT licence, notices for the work
  it contains, and citation metadata (`CITATION.cff`). See [Licence](licence.md) and
  [Citing JETCARP](citing.md).
- This book.

### Changed

- The named rules' descriptions are written in JETCARP's own words, and the people LifeWiki and other
  sources credit for each rule are shown with it.
- The Penrose rhomb construction and the GNOME extension's window hiding were rewritten clean-room
  after a provenance audit (see [How JETCARP was made](how-made.md#the-provenance-audit)).
- The evolutionary glider search runs as a checkpointed job that can be resumed.
- The Penrose JACA presets' descriptions now match what they show.

### Removed

- The commands, MCP tool and features listed under *For anyone who used CART*.
