# Folders and environment variables

JETCARP keeps its files in three folders: one for settings, one for data and one for output. Each has
an environment variable that moves it, so tests, scripts and portable installs can keep JETCARP away
from your own folders. `jetcarp paths` prints the three folders as JETCARP sees them, with any
overrides applied:

```text
$ jetcarp paths
settings: /home/ada/.config/jetcarp
data:     /home/ada/.local/share/jetcarp
output:   /home/ada/Pictures/JETCARP
bundled:  /home/ada/.local/lib/jetcarp/packs
```

The last line is the folder of packs that came with the program, the starter pack, which JETCARP
reads and never writes; it says `(no packs came with the program)` when there is none, as after
`cargo install`. Where it is looked for is in
[Installing JETCARP](../getting-started/installing.md#the-starter-pack).

`jetcarp paths --json` prints the same as JSON (the last as `bundled_packs`, `null` when there is
none). `jetcarp doctor` adds the catalog file, how much data is in use, the starter pack's size (or
where it looked for one), and every `JETCARP_…` variable that is set, which makes it the thing to
paste into a bug report.

## The three folders

| Folder | Holds | Override | Linux | macOS | Windows |
| --- | --- | --- | --- | --- | --- |
| Settings | App and background-host settings, screensaver profiles, rotations | `JETCARP_CONFIG_DIR` | `~/.config/jetcarp` | `~/Library/Application Support/jetcarp` | `%APPDATA%\jetcarp` |
| Data | The pattern catalog with its media and Quick Start entries, search databases | `JETCARP_DATA_DIR`, or a folder chosen in setup (`paths.json`) | `~/.local/share/jetcarp` | `~/Library/Application Support/jetcarp/data` | `%LOCALAPPDATA%\jetcarp` |
| Output | Pictures, script runs, sweeps, sandpiles, wallpapers | `JETCARP_OUT` | `~/Pictures/JETCARP` | `~/Pictures/JETCARP` | your Pictures folder, `\JETCARP` |

The defaults are the operating system's own folders. On Linux they follow the XDG settings
(`$XDG_CONFIG_HOME`, `$XDG_DATA_HOME` and the Pictures folder in `user-dirs.dirs`), so the paths above
are only the usual case. On Windows the data goes in the local application-data folder, not the
roaming one, because search databases can be large. On macOS the settings and data folders would be
the same, so the data goes in a `data` folder inside it.

On Windows the output folder follows the Pictures folder wherever Windows keeps it. When OneDrive
backs up Pictures it moves that folder into OneDrive, so the output goes to
`%USERPROFILE%\OneDrive\Pictures\JETCARP` and OneDrive syncs it (large sandpiles and long sweeps
included). `jetcarp paths` shows where it is; set `JETCARP_OUT` to keep the output out of OneDrive.

**A data folder of your own.** A large library can live on another disk: the
[first-run setup](../getting-started/first-run.md) (its data folder page, or
`jetcarp setup --data-dir DIR`) records the folder in `paths.json` in the settings folder, as
`{"version": 1, "data_dir": "/media/ada/Big Disk/jetcarp"}`, and every part of JETCARP (the app,
the command line, the background host, the MCP server) uses it from then on. `JETCARP_DATA_DIR`
still wins over it while it is set, and `jetcarp setup --status` says where the data folder comes
from. Setup only accepts a folder that is empty or already holds a JETCARP library, and never moves
a library; deleting `paths.json` brings the usual folder back.

If the system has no such folder, JETCARP falls back: without a settings folder, settings are simply
not saved; without a data folder, it uses `jetcarp-data` in the current folder; without a Pictures
folder, `~/Pictures/JETCARP`, and failing that `jetcarp-out` in the current folder. An override set
to an empty value counts as not set.

## What lives in each folder

**Settings.** `app.json` holds the app's layout, panes, tool windows and display settings, and
`host.json` the background host's. `setup.json` records the first-run setup (the profile chosen,
when, and the jobs it started; its being there is what tells the app that setup has run), and
`paths.json` a data folder chosen in setup. Screensaver profiles are in `screensavers/` and rotations in
`rotations/`, one JSON file each. The host records the command that starts it in `host-program`, for
GNOME's menu and the Windows screen saver; while it shows a still picture as your desktop background
it keeps the stills in `desktop-pictures/` and remembers your own picture in `desktop-picture.json`,
so that it can put it back. On macOS, installing the screen saver records the screen saver macOS had
chosen in `screen-saver-before.plist`, for removing it to put back (the file goes then), and removing
it keeps a copy of macOS's wallpaper store as it was in `wallpaper-store-before-saver-remove.plist`.
Files written by another settings version, or that cannot be read, give the defaults.

**Data.**

- `library.sqlite`, the pattern library: every verified pattern with the rules it works in and where
  it was seen, the jobs and their checkpoints, census results, every Quick Start entry (the
  built-in examples, entries from packs, the atlas and your own), ratings, and ant and turmite runs,
  each with its provenance and geometry pin (see
  [Library format](library-format.md)). Beside it, SQLite's own `-wal` and `-shm` files, a
  `library.sqlite-jobs/` folder of lock files (one per job, so two programs never run the same job),
  and `media/`, the frames, animations and thumbnails rendered for the library's records, each file
  stored once under the SHA-256 of its bytes as `media/ab/<sha256>.<ext>` (see
  [Library format](library-format.md#media-files)). `jetcarp media list KIND ID` shows a record's
  files, `jetcarp media export KIND ID --to DIR` copies them out under readable names, and
  `jetcarp media gc` deletes the files no record uses any more. `JETCARP_LIBRARY` moves the library
  (see below), and its media and lock files move with it.
- `searches/`, one folder per exploratory search, beside the library and moving with it:
  `searches/<uuid>/` holds the search's runs as Parquet files (`runs/part-000001.parquet`, a million
  runs each), a `manifest.json`, and its working database `search.sqlite` (its saved state and its
  newest runs); see [Search runs](search-runs.md). Reviews of a search's runs are kept in the
  library, so deleting a search folder keeps them, and so is the atlas that
  `jetcarp quickstart publish` picks from a search (see
  [Quick Start entries](library-format.md#quick-start-entries)).

**Output.** Everything JETCARP makes for you to look at: pictures, the folders of script runs and
sweeps (each with a `manifest.json`, and `favourites.json` for the runs you star), `piles/` for large
sandpiles, `wallpapers/`, `media/<name>` for a scene (or a built-in Quick Start entry) rendered with
`jetcarp media render`, under readable names, and the run folders of evolutionary searches. Script runs, sweeps, sandpiles and
evolutionary searches get folders named with the time they were made and a label, such as
`2026-11-05_14-30-02_my-sweep` or `piles/2026-11-05_14-30-02_hex-10000`. Scripts can write only
inside the output folder and folders they are explicitly allowed. Commands that write a single file,
such as `jetcarp run --png`, write where you tell them. The [MCP tools](mcp.md) read and write only
in the output folder, the library's folder and folders given with `jetcarp mcp --allow`, and
relative paths they are given start in the output folder.

**Runtime.** One more, small folder holds the files through which the running app and the background
host accept local control (their port and access token): `$XDG_RUNTIME_DIR/jetcarp`, else
`~/.cache/jetcarp`, on Linux and macOS, and `%LOCALAPPDATA%\jetcarp` on Windows. On Linux and macOS
the folder is kept readable by you only (mode 0700) and the files too (0600).

To remove what JETCARP set up outside these folders, and optionally the folders themselves, see
[Uninstalling](../guides/uninstalling.md).

## Environment variables

### For everyone

| Variable | Effect |
| --- | --- |
| `JETCARP_CONFIG_DIR` | The settings folder |
| `JETCARP_DATA_DIR` | The data folder |
| `JETCARP_OUT` | The output folder |
| `JETCARP_LIBRARY` | The pattern catalog: a `.sqlite` file, or a folder that holds (or will hold) `library.sqlite`. Default `<data>/library.sqlite`. Commands that use the catalog also take `--library` |
| `JETCARP_FINDER`, `JETCARP_ORCID` | Your name and ORCID iD, recorded as the finder in the [provenance](library-format.md#provenance) of the records you find or import (empty when unset) |
| `JETCARP_RUNTIME_DIR` | The runtime folder for the app's and host's control files |
| `JETCARP_BUNDLED_PACKS` | The folder of packs that came with the program, instead of `share/jetcarp/packs` beside the program's folder (when that folder is a `bin` folder, as in Homebrew and Python installs) or `packs` in it (see [The starter pack](../getting-started/installing.md#the-starter-pack)) |
| `JETCARP_GPU` | Set to anything but `0`, the app starts with GPU simulation switched on (where the graphics adapter allows) |
| `JETCARP_AI_CONTROL` | `1` or `on`: the app switches AI control on at launch, as its setting of the same name does |
| `JETCARP_HUNT_CACHE_MB` | Memory that a soup hunt's object caches may use together, in megabytes (default 1,536) |
| `JETCARP_QS_SEED` | A number: the seed of the Quick Start menu's *Featured* shuffle, so the menu comes up the same each time (default: from the clock) |
| `JETCARP_WINDOW` | The app's starting window size, such as `1920x1080` |

A safe way to try JETCARP without touching your own folders is to point the first four at scratch
folders. This is what JETCARP's continuous-integration tests do:

```bash
export JETCARP_CONFIG_DIR=/scratch/jetcarp/config
export JETCARP_DATA_DIR=/scratch/jetcarp/data
export JETCARP_OUT=/scratch/jetcarp/out
export JETCARP_LIBRARY=/scratch/jetcarp/library
jetcarp paths
```

### For development and debugging

These are read by JETCARP but meant for its developers: headless screenshot checks of the app, timing
traces, and test fixtures. They may change without notice.

| Variable | Used for |
| --- | --- |
| `JETCARP_SCREENSHOT` with `JETCARP_SCENE`, `JETCARP_FRAMES` and many others (`JETCARP_MENU`, `JETCARP_PANES`, `JETCARP_POPOUT`, `JETCARP_SPEED`, `JETCARP_PAUSED`, `JETCARP_WALLPAPER`, `JETCARP_SCRIPTS`, `JETCARP_DASHBOARD`, `JETCARP_RESEED`, `JETCARP_TOOL_PALETTE`, …) | Open the app on a scene, set up a window or tool, run some frames, save a screenshot and quit. A screenshot run neither reads nor writes `app.json` unless `JETCARP_CONFIG_DIR` is set |
| `JETCARP_FRAMETIME` | Print frame-time statistics when the app closes |
| `JETCARP_PRESENT` | The windows' present mode (`fifo`, `mailbox`, `immediate`, `novsync`), for frame-pacing checks |
| `JETCARP_ZOOM`, `JETCARP_HLVIEW`, `JETCARP_HOST_PANEL` | Zoom, HashLife view and host panel set-up for headless checks |
| `JETCARP_EXPLORE_TRACE` | Timing lines from the exploratory search on standard error |
| `JETCARP_HOST_NO_PAUSE_RULES` | The background host ignores its power rules (its own tests) |
| `JETCARP_PIN_WRITE` | Tests only: rewrite the geometry and dynamics pins after a deliberate change (see [Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)) |
| `JETCARP_WRITE_SCENES` | Tests only: rewrite von Neumann's checked scenes |
| `JETCARP_MEDIA_SAMPLES`, `JETCARP_ANT_MEDIA_OUT`, `JETCARP_PALETTE_SHEET`, `JETCARP_OFFSCREEN_OUT`, `JETCARP_TEST_OUT` | Tests only: keep the files a test renders for a look |
| `JETCARP_PROFILE_OUT`, `JETCARP_PROFILE_ONLY`, `JETCARP_PROFILE_GENS`, `JETCARP_PROFILE_RULE`, `JETCARP_PROFILE_DENSITY`, `JETCARP_PROFILE_RESEED`, `JETCARP_PROFILE_FAMILY`, `JETCARP_PRESET_OUT`, `JETCARP_PRESET_ONLY` | Tests only: surveys used to tune the built-in screensaver profiles and reseed presets |
