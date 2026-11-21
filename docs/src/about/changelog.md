# Changelog

All notable changes to JETCARP are listed here, newest first, in the style of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before version 1.0, a release may still
change the scene format, the command line, the scripting API and the MCP tools; 1.0 is the release
that will declare them stable. A release's date here and in `CITATION.cff` is the day it was
published where the maintainer lives; GitHub, PyPI and Zenodo give the date in UTC, which can be a
day later (0.4.2: 17 November here, 18 November there).

## [0.4.3] – unreleased

### Added

- **Palettes chosen and checked for colour-vision deficiency.** Three new built-in palettes,
  Lantern for any rule and Cobalt and Saffron for JACA's +1 and −1 cells, keep neighbouring stops,
  and every stop and the background, at a CIEDE2000 difference of 10 or more for protanopia,
  deuteranopia, tritanopia and their milder forms (simulated with Machado, Oliveira and Fernandes'
  model), with lightness falling steadily along each ramp. The same check is run on every built-in
  palette: `jetcarp palettes` and `reference.palettes` give each one's result (Tide, Phosphor, Koi
  and Verdigris pass too), and the palettes reference marks them. The three join the pack
  format's frozen palette registry, so packs may name them. In the app, *Settings →
  Accessibility* makes them, or any palettes, the defaults for scenes that name none.
  ([Palettes](../reference/palettes.md#colour-vision-deficiency),
  [#217](https://github.com/jprodgers/jetcarp/issues/217))
- **The app follows the system's request for reduced motion.** It reads Windows' *Show animations
  in Windows*, macOS's *Reduce motion* and GNOME's *Reduce Animation* at start and every couple of
  seconds. When it is on, the Quick Start's cards and the small animations in the Hunts and Ants
  windows show stills until one is chosen, a scene opens paused with a note saying why, and the
  interface's own animations, spinners and progress bars stand still. *Settings → Accessibility*
  overrides the system either way; the strobe guard is unchanged.
  ([Accessibility settings](../guides/accessibility-settings.md#reduced-motion),
  [#218](https://github.com/jprodgers/jetcarp/issues/218))
- **A high-contrast theme for the app,** dark and light: text at 7:1 or more against its
  background, thicker outlines at 3:1 or more, and a thick ring round whatever has the keyboard.
  It follows Windows' contrast themes, macOS's *Increase contrast* and GNOME's *High Contrast*, and
  *Settings → Accessibility* overrides them, and chooses light or dark (on Linux, the way to a
  light app until [#137](https://github.com/jprodgers/jetcarp/issues/137) is fixed). A test
  computes every contrast the theme uses. The docked panes now take the theme's panel colour in
  the light and high-contrast themes, where they kept the grid's dark backdrop.
  ([Accessibility settings](../guides/accessibility-settings.md#high-contrast),
  [#220](https://github.com/jprodgers/jetcarp/issues/220))

- **WireWorld's `circuit` seed has a short form.** The command line, scripts and the MCP tools
  take `circuit` (40 regions), `circuit:N` (N regions, four signals each) and `circuit:N:S`, and
  `reference seeds` lists them; before, only a scene file could name a circuit.
  ([Scripting](../reference/scripting.md))
- **The strobe guard: JETCARP watches what it draws, and softens, slows or pauses a scene before
  it flashes more than three times a second or shows strong regular stripes.** Every frame is
  measured as WCAG 2.3.1 defines flashes and red flashes, with its area rule (a quarter of a 10°
  field, for a screen watched from 40 cm at 96 pixels an inch for each step of its scale factor),
  and the screen's mean as IRIS measures it; regular high-contrast stripes over a quarter of the
  screen are measured as ITU-R BT.1702 describes them. The guard first softens the tiles that flash
  (their brightness may change by at most 0.3 a second), then slows a scene that keeps flashing,
  down to half a generation a second, then pauses it with a note saying why; strong stripes are
  veiled. It is on for everyone, in the app's view, full screen, the outputs, the world AI control
  acts on, the desktop background on all three systems and the screen savers; it never changes what
  a world computes. Someone who answered the photosensitive seizure warning *I have, or may have, a
  seizure disorder* can't turn it off; someone who answered *I don't* can, in the app's new
  *Settings → Safety*, through a confirmation with an unticked box. The desktop background and the
  screen saver keep their own switch, on until turned off with the same confirmation, since people
  in the room never answered the warning. The switches are saved with the warning's answer;
  `jetcarp guard` shows them and `jetcarp guard --on` turns them back on, and no command, script or
  MCP tool turns them off. Measuring a 1080p frame takes 0.02 ms on a Radeon 8060S.
  ([The strobe guard](../guides/strobe-guard.md), [#97](https://github.com/jprodgers/jetcarp/issues/97))
- **Exported animations are checked for flashing and strong stripes before they are saved.** A GIF
  or WebP is measured at its own frame rate, looping as a player would. *File → Export animation…*
  holds one that flashes and offers a softened version (or, with the app's guard off, the
  animation as it is); `jetcarp media render` saves the softened version and says so, and the MCP
  tool `media_render` always does. The softened version is the animation shown more slowly when up
  to four times slower is enough (so an oscillator's GIF keeps its frames), and otherwise its
  flashing softened and strong stripes veiled. The saved file is read
  back and checked again. The Quick Start's live cards, the *Hunts* window's animations, the ants
  window's and an explore search's atlas clips are guarded too. Every animation's render key now
  includes the guard, so `jetcarp media render --missing` renders the old ones again.
  ([Exported animations](../guides/strobe-guard.md#exported-animations))
- **Importing a pack says who signed it, and says plainly when their key is new.** The setup
  window shows, under each pack before it is imported and under each import afterwards, the
  publisher's name and the start of their key, marked *New key: never seen in this library* when
  no pack signed by that key has been imported into the library before. `jetcarp pack import`,
  `pack check` and `pack info`, the MCP `pack` tool and `jetcarp setup` say the same, with the whole
  key; a known key, your own publisher key and the packs that came with JETCARP are each said as
  such. Before, the app showed only a pack's name and size, and the command line the key alone.
  ([Packs](../guides/packs.md#check-a-pack))

### Changed

- **The state hash is SHA-256 over the whole state, in all 64 hexadecimal digits.** Version 1, the
  64-bit FNV-1a hash JETCARP printed as 16 digits, left out part of what decides a world's future
  (the random-number generator, a turmite's internal state, a layered rule's layers, a forest fire's
  noise seed, a script's streak, the automata's rules and settings), so two different worlds could
  share it. Version 2 hashes every piece of state that decides what a world does next, and a digest
  of its tiling: changing any one of them changes the hash, and a world reopened from its saved scene
  at any generation hashes as the one that ran straight through. `jetcarp run` and `verify`, scene
  records, PNG metadata, `--json`, the MCP tools and *Copy state hash* carry all 64 digits; the app's
  menu bar shows the first 16. Records now say `"hash_version": 2`; those written before carry no
  version, and JETCARP checks them with version 1, kept unchanged, so old scenes, figures and packs
  still verify. `--expect` takes either version, by length (16 or 64 digits), and refuses anything
  else. The pinned hashes of every Quick Start entry, the rule-semantics scenes and the portable
  Lenia runs are now version 2; their version 1 hashes are the old pins, so no state changed.
  ([The state hash](../research/reproducibility.md#the-state-hash),
  [#173](https://github.com/jprodgers/jetcarp/issues/173))

### Fixed

- **The GPU tests fail, rather than pass, when they were meant to run and could not.** A GPU test
  with no adapter it can use (none, or only one the GPU self-test turns down) skips with a note;
  with `JETCARP_REQUIRE_GPU=1` it fails, naming the adapter and why. CI sets it on Linux (Mesa's
  lavapipe) and macOS (the runner's own GPU), and the release workflow's tests, which had left
  lavapipe unused, now run the GPU tests on it.
  ([Contributing](contributing.md), [Testing](testing.md))
- **A settled reseed policy fires at the same generation whatever else counts changes.** Its
  check for a steady cycle of changing cells read every generation whose changes had been counted,
  and which those were depended on whether the app's activity graph or the desktop host was
  counting them, or a script trigger, or the GPU: with them it could fire sooner, so a scene saved
  from the app could replay to another state. It now reads only the generations it measured itself,
  from the end of its cooldown. A scene in which something else measured activity before a policy's
  cooldown ended can reseed at another generation than before.
  ([Reseeding](../guides/reseeding.md))
- **A world with a B0 rule is said to repeat only when it does.** The period check compared the
  cells JETCARP keeps for a B0 rule, which are relative to its background, but not the
  background's phase, so a world whose kept cells came back with the background in another phase
  (on squares, after an odd number of generations) was reported as repeating, and a script
  waiting for a repeat stopped, although the next generations differed. The background's phase is
  now compared too, on the CPU and the GPU: such a world repeats with the true period (often
  twice the old one), and on an aperiodic patch whose background does not settle within a few
  thousand generations no repeat is reported. ([B0 rules](../automata/life-like.md#b0-rules),
  [#211](https://github.com/jprodgers/jetcarp/issues/211))
- **Life-like rules with birth on 0 give the right results on every tiling.** JETCARP ran a B0
  rule as two complementary rules alternating on even and odd generations, which is exact only
  where every tile has the same number of neighbours. On tilings whose tiles have different
  numbers, such as 4.8.8, 3.4.6.4, most k-uniform tilings and every aperiodic patch, a rule whose
  survival list held some of those numbers but not all, such as `B0/S8` on 4.8.8, went wrong from
  the third generation, on the CPU and the GPU alike. Each cell is now kept relative to the rule's
  run from an empty world, so empty space still stays dark and the result is exact on any tiling;
  where every tile has the same number of neighbours nothing changes. A B0 rule also counts the
  cells past a live edge as live now, as other B/S rules do (it ignored live edges), and HashLife
  runs B0 rules on wrap-edge lattices whatever their tiles' neighbour counts. B0 rules are now a
  [rule-semantics family](../reference/library-format.md#rule-semantics-versions) of their own,
  `lifelike_b0`, at version 2: scenes of such rules replay to other states, and a library flags
  any B0 pattern record for `jetcarp library reverify` (no search stores B0 patterns). Other B/S
  rules keep `lifelike` at version 1, so their records are not flagged.
  ([B0 rules](../automata/life-like.md#b0-rules),
  [#171](https://github.com/jprodgers/jetcarp/issues/171))
- **WireWorld can be added without a start on any tiling.** Its default start, the clock-loop
  demo, is drawn for the square grid, so everywhere else adding the rule alone was refused with
  "the WireWorld demo needs the square grid", from the command line, scripts and the MCP tools
  alike. Off the square grid it now starts with a `circuit`: wiring along the borders of random
  regions, about one region per 70 cells, with four signals in each. The square grid keeps the
  demo. ([WireWorld](../automata/wireworld.md#on-any-tiling),
  [#180](https://github.com/jprodgers/jetcarp/issues/180))
- **Larger than Life keeps every count a rule names.** A birth or survival count above the square
  grid's neighbourhood size (8 at range 1, 1,680 at range 20) was dropped without a word, so on
  tilings whose neighbourhoods hold more cells, such as triangles with 12 touch neighbours, a rule
  ran as another: `R1,C2,S0-12,B9-12` ran as `R1,C2,S0-8,B`. Counts up to 65,535 are now kept, a
  larger one is refused with a message, and the notation reads back as written, so Majority and
  Majorly show as `R4,C2,S40-80,B41-81` and `R7,C2,S112-224,B113-225`. On the square grid no rule
  runs differently, but those two rules' Quick Start entries have new handles
  (`rule:R4,C2,S40-80,B41-81`, `rule:R7,C2,S112-224,B113-225`), and so new random starts; the
  old handles still open them, so rotations and screensaver profiles naming them keep working. Larger
  than Life's [rule-semantics version](../reference/library-format.md#rule-semantics-versions) is
  now 2: scenes of such rules on other tilings replay to other states, and pattern records verified
  under version 1 are flagged for `jetcarp library reverify`.
  ([Larger than Life](../automata/larger-than-life.md#on-any-tiling),
  [#172](https://github.com/jprodgers/jetcarp/issues/172))
- **`jetcarp host status` no longer says "GPU none" for a host that hasn't opened its GPU yet.** A
  host started at login with nothing to show opens the GPU with its first window; until then the
  status says "GPU not opened yet (nothing shown yet)", and its screens that the GPU "is opened when
  the scene is first shown", where both said there was no GPU. A host that lost its GPU says that.
  ([#141](https://github.com/jprodgers/jetcarp/issues/141))
- **On GNOME, setup says plainly to log out once after turning the live wallpaper on.** GNOME loads
  JETCARP's extension only at login, and until then the scene shows in a window, which said nothing
  about it; the only word was small print in setup. Setup now ends with "Log out and back in once to
  see the live wallpaper on your desktop", the window's title says the same, and the host's note no
  longer says to install an extension that setup already installed.
  ([#139](https://github.com/jprodgers/jetcarp/issues/139))
- **The background host no longer steps and draws a screen nobody can see.** A screen kept for a
  monitor that wasn't connected when the host started, with the main monitor taken by another
  screen, had no window but went on stepping and drawing on the GPU. It now waits until its monitor
  comes back, and `jetcarp host status` says so. ([#130](https://github.com/jprodgers/jetcarp/issues/130))
- **The photosensitive seizure warning's example is Hodgepodge, not B0.** Since B0 rules are drawn
  relative to their background ([#171](https://github.com/jprodgers/jetcarp/issues/171)), a B0
  rule no longer turns the whole field over at each step on screen. The warning now gives
  Hodgepodge, which can flicker across the whole field at each step, and the book's warning page
  follows it: Hodgepodge as Quick Start shows it, run at 60 generations a second, flashes about
  four times a second as it settles, and the strobe guard catches it (a scene in its test corpus).
  ([Photosensitive seizure warning](../getting-started/photosensitivity.md))
- **Programs starting together no longer fail with "database is locked" on a new library.** When
  the app, the desktop host, the MCP server or a search job opened a library that did not exist
  yet at the same moment as another, one of them could be refused at once, although each waits
  up to 30 seconds for the others: SQLite does not wait while a new library is switched to its
  write-ahead log. JETCARP now waits for that too, for the library and for the explore runs'
  store. Programs opening one search from before 0.4 at the same moment no longer fail with
  "duplicate column name" while it is brought up to date: one does it, and the others wait. Nor is
  a library that another program is creating at that moment refused as having "no schema version"
  or as "not a JETCARP library": its version and mark are now read together.
- **`jetcarp_core::lenia::decode_rle` refuses a run-length text that would need more room than
  its limit allows, or that is malformed, instead of panicking or asking for more memory than the
  system has.** The limit is the caller's: the new `decode_rle_within` takes it, so it can follow
  a per-machine limit, and returns the pattern as one flat buffer that costs its values and
  nothing per row; `decode_rle` keeps the most cells a grid may have (`topology::MAX_CELLS`) and
  counts each of its rows at what the row costs. Sizes are checked before anything is allocated,
  empty rows take no memory, and a refusal says what the pattern needs and what the limit is. A
  count too long for a number and a malformed two-letter value are refused too. Found by fuzzing;
  JETCARP itself decodes only its built-in Orbium with it, which is unchanged.
  ([#194](https://github.com/jprodgers/jetcarp/issues/194))
- **Names that Windows treats specially are refused on every system.** The MCP tools, scripts, jobs
  and commands refuse a name with a `:` (on Windows it wrote a hidden data stream beside a file),
  Windows' device names such as `CON`, `nul.json` and `AUX/x.json` (on Windows they were saved as
  files other tools struggle to remove), and a name ending in a dot or a space, such as
  `scene.json.` (Windows drops those, so the name would change there), with an error that names it,
  so that folders and scenes can be copied between systems. A folder JETCARP names after a scene
  called, say, "Con" gets a `_` after the name ([Names](../reference/folders.md#names)).
- **JETCARP builds from source again with Rust 1.95 to 1.98.** 1.95 is the stated minimum, but
  the workspace's manifests needed 1.99 to load. CI now checks the whole workspace with 1.95.
  ([#175](https://github.com/jprodgers/jetcarp/issues/175))
- **Paths of 4,096 bytes or more are refused on every system.** The MCP tools, scripts and jobs
  refuse such a path at once, with an error that gives its length, as Linux always did; on
  Windows, with long paths on, they used to make the folders, and a save a few thousand folders
  deep held the MCP server for minutes. JETCARP also finds where a new path begins in a few
  lookups instead of one for each folder on the way. On macOS the system's own limit, 1,024 bytes,
  still comes first.
- **`jetcarp uninstall --data` and `--settings` delete their folders only when everything in each
  is JETCARP's.** A folder holding anything else, such as a folder of documents that
  `JETCARP_DATA_DIR`, `JETCARP_CONFIG_DIR` or a data folder chosen in setup names by mistake, is
  refused before anything is done, even with `--yes`, and the refusal names the folder, where its
  name came from and what in it is not JETCARP's. A library alone does not count, since JETCARP
  makes one in whatever folder it is given. A folder that is a link, or a junction on Windows, is
  removed as a link: nothing behind it is read, emptied or deleted, where before `--settings`
  emptied the folder behind it when it kept something there (the picture the desktop shows, or the
  data folder inside). A link inside either folder is removed as a link too. And `--settings` now
  keeps the publisher key and the rater book (`pack/`), saying where they are, unless
  `--publisher-key` is added: the key is the identity behind every pack its publisher signed, and
  the rater book holds raters' names and their consent; before, `--settings` deleted them.
  ([Uninstalling](../guides/uninstalling.md), [#195](https://github.com/jprodgers/jetcarp/issues/195))
- **`THIRD_PARTY_LICENSES.html` lists only the libraries a release contains.** It also listed 42
  that no release builds: a library's dependencies on one system, kept although JETCARP builds that
  library only for another (the GTK family, which the tray icon needs only on Linux, where JETCARP
  has no tray icon). The notices are now gathered for each release system on its own and put
  together, and CI checks that they name exactly the libraries in each system's build.
  ([#123](https://github.com/jprodgers/jetcarp/issues/123))
- **The Windows install and uninstall lines save the installer to the temp folder.** Every copy
  (the README, the book, the release notes, the installer's own help and `jetcarp uninstall`) saved
  `jetcarp-installer.ps1` into the current folder, which fails in a PowerShell window that opens in
  `C:\Windows\system32`; they now save it to and run it from `$env:TEMP`, and a check in CI keeps
  every copy so. ([Installing](../getting-started/installing.md), [#143](https://github.com/jprodgers/jetcarp/issues/143))
- **The Windows installer's last line says to open a new PowerShell window.** The window it runs from
  can't see the new `PATH`, so `jetcarp` typed there next wasn't found, and the only word of it was
  in the middle of the output. ([#144](https://github.com/jprodgers/jetcarp/issues/144))
- **A repeated `jetcarp uninstall --settings --data` says the folders are already gone,** instead of
  only "Nothing to undo", which read as if the options had been ignored.
  ([#144](https://github.com/jprodgers/jetcarp/issues/144))
- **The installers' `--uninstall` names each folder it keeps and what it holds,** instead of "kept
  your settings, data and pictures" when only the pictures were left.
  ([#142](https://github.com/jprodgers/jetcarp/issues/142))
- **The soup hunt cuts out gliders that fly away together.** A hunt erased an escaping ship only
  once it lay beyond everything else by a few cells, so two gliders flying the same way on lanes a
  few cells apart, or several staggered so that each stayed level with another, were never cut. They
  flew on until the soup passed the hunt's span limit and was dropped as exploded, with no census of
  what it had left, while the ships cut from it before were still counted. In Life that was about one
  soup in 370, nearly every "explosion"; on some tilings it was far more, and every one of the 703
  explosions and 152 unsettled soups of the k2-017 hunt in the
  [spaceship tutorial](../tutorials/hunting-spaceships.md) was such a soup. Far from the soup, groups
  that no gap separates are now taken together and cut as the ships they are, and a ship is also cut
  when every other group it is not clear of is a ship that cannot catch it: one with the same
  velocity, or one slower along the way it is leaving. A hunt's counts change: in 150,000 soups of
  Life the exploded soups fall from 405 to 21, most of them past the population cap, and the glider's share,
  0.7% above Catagolue's census before, is within chance of it. A hunt of Life runs about a tenth
  slower. ([Soup hunt](../research/search-methods.md#soup-hunt),
  [#182](https://github.com/jprodgers/jetcarp/issues/182))
- **The book credits apgsearch and Catagolue, and says how a hunt compares with them.** The soup
  hunt's defaults are those of Adam P. Goucher's apgsearch and of his Catagolue census, which the
  book did not mention; *Search methods*, *Searching for patterns* and *What is new in JETCARP* now
  credit them and set out how JETCARP's hunt differs, what its counts are and are not, and how they
  compare with Catagolue's. `docs/tools/check-catagolue.py`, run for each release candidate, compares
  the shares of the twelve commonest objects of a fixed hunt of Life with a pinned copy of the top of
  Catagolue's census. ([Soup hunt](../research/search-methods.md#soup-hunt),
  [#182](https://github.com/jprodgers/jetcarp/issues/182))
- **The research pages add up, and their search log is published.** The count of spaceship records
  in *Findings* is now a table that can be checked by subtraction (the 72 candidates came from 137
  records, 103 of CART's and 34 of fresh searches; seven records of two candidate ships flying
  together had not been said); the k-uniform chapter no longer says that no work had run cellular
  automata on 2- and 3-uniform tilings; and the research notes that the claims and the
  bibliography rest on, with every query and page their searches read, are in the repository's
  `research/` folder. The bibliography marks the 10 entries that rest on a secondary listing (it
  said every entry had been checked against a primary record), cites every entry it holds, and
  corrects four: the year of the Smith et al. monotile papers, the pages of Eppstein (2010) and
  Heudin (1996), and the translators of Kepler's *Harmony of the World*. *What is new in JETCARP*
  now weighs Marr and Hütt's outer-totalistic rules on graphs and Nochella's network rules against
  the layered rules and the one engine for every tiling.
  ([Findings](../research/findings.md#how-the-list-was-cut),
  [Bibliography](../research/bibliography.md), [#179](https://github.com/jprodgers/jetcarp/issues/179))

## [0.4.2] – 2026-11-17

### Security

- **Two low-severity security problems are fixed.** Both were handled privately, and each has an
  advisory with the details:
  [GHSA-88q3-9mmf-m2xw](https://github.com/jprodgers/jetcarp/security/advisories/GHSA-88q3-9mmf-m2xw)
  (in 0.4.1 and earlier) and
  [GHSA-78q7-m3j3-xg23](https://github.com/jprodgers/jetcarp/security/advisories/GHSA-78q7-m3j3-xg23)
  (in 0.4.0 and 0.4.1). Upgrading to 0.4.2 fixes both.

### Fixed

- **The live desktop works on GNOME 51 (Fedora 45).** The GNOME extension claimed only GNOME
  Shell 50, so GNOME 51 would not load it and the host waited for it with nothing on the desktop.
  ([#2](https://github.com/jprodgers/jetcarp/issues/2))
- **The GNOME live desktop stood still on the desktop and ran only behind other windows.** The
  extension took the host's window out of the window lists before hiding it, and GNOME will not
  minimize a window that is out of the window lists. The window stayed up, full screen, so GNOME
  counted the desktop as covered by a full screen app and the host paused whenever the desktop
  showed. The window is now hidden first. ([#2](https://github.com/jprodgers/jetcarp/issues/2))

  For both, log out and back in once after upgrading (see below).
- **A CPU-only explore search never confirmed its elites, and ran only about a quarter of the
  candidates it made.** The search works in steps of 2 seconds, and a round of runs on the
  processor nearly always outlasts one; the step's deadline was checked inside the round, so the
  candidates not yet started were dropped and the confirmation every fourth round never came.
  Searches run without a GPU (every family on a computer without one, or a search started without
  `--gpu`), and the families that only ever run on the processor (turmites, layered and elementary
  rules among them), now confirm their best runs on bigger grids, and run every candidate. A round under way now
  runs to its end; a stop still takes effect within moments.
  ([#3](https://github.com/jprodgers/jetcarp/issues/3))
- **The JACA page said JACA runs only on the CPU.** The explore search runs JACA on the GPU, in its
  batches, when given one, and a JACA world now runs on the GPU in the app and the background host
  too (see below); the page says so. ([#9](https://github.com/jprodgers/jetcarp/issues/9))
- **A stale example script is gone.** `explore-supervise.sh`, beside the example scripts in the
  scripting crate's source, called `jetcarp explore run`, a command of JETCARP's predecessor, so it
  failed at once. An explore search stops on time by itself and carries on from its archive after a
  crash, so keeping one going needs only the one-line loop around `jetcarp jobs resume` that the
  [searching guide](../guides/searching.md#waiting-in-a-script) now shows.
  ([#4](https://github.com/jprodgers/jetcarp/issues/4))

- **The two signs of JACA looked alike once their territories went still.** JACA colours each sign
  by age, and its default palettes, Tide and Ember, both end in nearly the same dark indigo, so old
  cells of either sign showed the same colour. The −1 cells now default to Koi, whose reds stay apart
  from Tide's blues at every age; so do the 13 Quick Start presets and the book's scenes that paired
  Tide with Ember. A test keeps every JACA palette pair apart as cells age.
  ([#6](https://github.com/jprodgers/jetcarp/issues/6))

- **An edge-neighbour rule was named as the plain rule.** A world given `B3/S23V` named its
  automaton "B3/S23", the touch-neighbour rule's notation, on cards, captions and status lines. The
  default name now keeps the `V` (and a JACA rule's `V`s), as the rule's notation does.
  ([#7](https://github.com/jprodgers/jetcarp/issues/7))
- **A hunt over many rules and tilings worded its progress oddly.** "100/100 x 1200 groups soups"
  now reads "100/100 soups in each of 1,200 groups", and the count of new patterns is grouped by
  thousands like the others. ([#8](https://github.com/jprodgers/jetcarp/issues/8))

- **A hunt asked for the GPU failed when there was none.** `jetcarp search hunt --gpu` on a computer
  without a usable GPU stopped with an error, and a pile asked for the GPU did the same in a program
  without a GPU toppler. Both now run on the processor, with the same results and one note in their
  log. ([#10](https://github.com/jprodgers/jetcarp/issues/10))
- **Each part of JETCARP chose its graphics adapter its own way.** The command line asked for the
  fastest adapter, the app took its toolkit's choice, the host's windows asked for high performance
  and its headless device for no preference, and only the command line refused software adapters. On
  a computer with two GPUs, the app and the command line could use different ones, and there was no
  way to pick one. They now all choose the same way (see [The GPU](../guides/gpu.md#which-adapter)),
  and `JETCARP_GPU` or the GPU setting can name the adapter.
  ([#11](https://github.com/jprodgers/jetcarp/issues/11))
- **The GPU parity tests never ran in CI.** CI installs Mesa's software Vulkan, but the GPU tests
  refused software adapters, so every one of them skipped. CI now sets `JETCARP_GPU=software`, and
  they run on lavapipe. ([#12](https://github.com/jprodgers/jetcarp/issues/12))
- **A GPU that failed part way through a run left the world ahead of its cells.** When a later
  batch of a run failed (a readback that failed, a lost device), the generations the earlier
  batches had stepped were counted and logged but their cells never read back, so the processor
  carried on from the wrong cells. The world is now put back as it was before the run, and the
  processor carries on from there to the same result. A failed readback no longer stops the program
  either. ([#13](https://github.com/jprodgers/jetcarp/issues/13))
- **An explore search's progress read 0/s most of the time on a GPU.** Its rate was measured over a
  few seconds, but the GPU's batches land a thousand runs at once, every 10 to 30 seconds on a
  laptop's GPU, so most lines saw none. It is now the rate over the last minute.
  ([#22](https://github.com/jprodgers/jetcarp/issues/22))
- **"N confirmed" in an explore search's progress read like a running total.** It counts the elites
  confirmed now, which falls when a better unconfirmed run takes a confirmed elite's niche. The line
  now says "1,204 elites in 980 niches (75 of them confirmed)".
  ([#24](https://github.com/jprodgers/jetcarp/issues/24))
- **A switched-off GNOME extension was said to be starting.** GNOME reports an extension it loaded
  at login but isn't enabled as state 6, which `jetcarp host gnome status` read as "it is
  starting"; it now says it is switched off. ([#19](https://github.com/jprodgers/jetcarp/issues/19))
- **The search runs reference said `cells` is about the grid's size.** It is the size asked for; an
  aperiodic patch, sized by level, can have half as many cells or twice as many. The reference says
  so. ([#27](https://github.com/jprodgers/jetcarp/issues/27))
- **`jobs clips --sample` gave one clip per tiling at most.** Its sample took one run per family
  and tiling, so a search on few tilings (a JACA search from parents gave 13 for any number asked)
  had too few clips to rate. It now takes more runs of the same tilings, each with a rule of its
  own, once every family and tiling has one. ([#29](https://github.com/jprodgers/jetcarp/issues/29))
- **`jobs runs --reviewed` didn't show the verdicts.** It now gives each listed run's verdicts and
  star (`[keep ★]`, `[maybe, drop]`; `review` in the JSON).
  ([#33](https://github.com/jprodgers/jetcarp/issues/33))
- **A pack of an explore search that was never sealed carried no runs.** A search seals its runs
  into segments every million runs (or with `jobs seal`), and a pack carries only sealed segments,
  so a smaller search was exported as an empty one, without a word. Publishing now seals a stopped
  search's newest runs first and says so; a running one is refused until it is stopped.
  ([#35](https://github.com/jprodgers/jetcarp/issues/35))
- **After a rescore, a job's status kept its confirmed count from before.** A rescore picks new
  elites by the new scores; it now counts how many of them are confirmed and rewrites the job's
  line. ([#36](https://github.com/jprodgers/jetcarp/issues/36))
- **An explore search imported from a pack said "cpu only"** though nothing had run in the
  importing library; it now says "not run here yet".
  ([#37](https://github.com/jprodgers/jetcarp/issues/37))
- **The desktop background's pane could not make a rotation, and the rotation editor's *Save* was
  out of sight.** A rotation was made in the *Screensaver* pane, with *Show* set to *A rotation*,
  where *Save* sat below the whole editor in a scrolling pane, and saving there chose the rotation
  for the screensaver, not the desktop. The *Desktop background* pane now has, under each screen's
  rotation, *Show just this scene*, which saves the scene open in the app as a profile and as a
  rotation of just that profile and chooses it for the screen (pressed again for the same scene, it
  updates them rather than adding more), and *Edit* and *New*, which open the rotation editor in the
  pane; its *Save* chooses the rotation for that screen and leaves the screensaver's as it was. In
  both panes the editor has *Save* and *Close* above it as well as below. A running host now shows
  a rotation or profile saved again under the same name, where it carried on with the old one.
  ([#41](https://github.com/jprodgers/jetcarp/issues/41))
- **Life's blinkers on the desktop background stood still, or blinked far too slowly.** The host
  drew 30 frames a second whatever the scene's speed, so a scene of 60 generations a second showed
  every second generation, and an oscillator of period 2 showed nearly the same phase in every
  frame; frames that came a little late made even a scene of 30 a second step two generations now
  and then. Each screen now draws as fast as its scene steps, one generation a frame, at least the
  frame rate and at most 60 frames a second, or the monitor's refresh rate if lower (a faster
  scene still draws several a frame; `jetcarp host max-fps refresh` lets it draw at a fast
  monitor's full rate), and each screen keeps its own rate. *Low power*, in the *Desktop background* pane's *Performance*
  group or `jetcarp host pacing low-power`, draws at the frame rate instead and caps each scene's
  speed there. `jetcarp host status` gives each screen's frame rate and speed (see
  [Frame rate and speed](../guides/wallpapers.md#frame-rate-and-speed)).
  ([#42](https://github.com/jprodgers/jetcarp/issues/42))
- **`--limit`'s help promised an exact stop that an explore search can't make.** An explore
  search checks the limit between its steps of a few seconds, so `--limit 20` could run 240 runs.
  The help for `search` and `jobs resume` now says so; use `--minutes` for an explore budget.
  ([#47](https://github.com/jprodgers/jetcarp/issues/47))
- **`jetcarp doctor` with `JETCARP_GPU` set tested adapters the setting didn't name.** Naming
  any adapter let every software and "other" adapter pass the requirements, so with
  `JETCARP_GPU=llvmpipe` doctor self-tested a Radeon's OpenGL adapter and showed its failure.
  The name now has to match, as it always did for the choice itself.
  ([#48](https://github.com/jprodgers/jetcarp/issues/48))
- **A long explore search's memory kept growing on Linux, to 7 GB or more after four hours.** What
  the search holds is capped, but each GPU batch frees a few hundred megabytes, which glibc kept
  for reuse rather than giving back. The search now gives its free memory back between steps, at
  most every 30 seconds. The grid cache's cap is about 3 GB, not the 2.2 GB its notes said: a grid
  costs about 90 bytes a cell. ([#53](https://github.com/jprodgers/jetcarp/issues/53))
- **A rule typed into the app's Rule box stayed there, looking applied, while the old rule ran.**
  The box applied a rule only on Enter, so a click elsewhere left the typed text in place, and a
  saved scene kept the old rule. The Rule box and JACA's two rule boxes now apply a rule when they
  lose focus, however that happens; Esc puts the current rule back.
  ([#61](https://github.com/jprodgers/jetcarp/issues/61))
- **The AI control window said "Connections 0" after a client had connected and left.** It counts
  the links open at the moment, and now says so: *Open connections*.
  ([#60](https://github.com/jprodgers/jetcarp/issues/60))
- **The background host's memory limit counted memory it had already freed.** On Linux, glibc keeps
  freed memory for reuse rather than giving it back. Before its memory limit (`max_memory_mb` in
  `host.json`) lets a scene go, the host now gives that memory back and looks again.
  ([#69](https://github.com/jprodgers/jetcarp/issues/69))
- **A rating round of clips gave the runs nobody keeps as many slots as the rest.** `jobs clips`
  shared its sample equally among the categories, so on a JACA search "Frozen at once", 12% of the
  runs, took 20 of 48 clips. The categories people seldom keep (*Frozen at once*, *Quick deaths*,
  *Static*, *Quiet rows*) now get a sixteenth of the sample each, at least 2, and the output says
  how many clips each category got. ([#56](https://github.com/jprodgers/jetcarp/issues/56))
- **doctor with the GPU simulation off still tested every adapter.** It said "not used, and not
  tested", but its list of graphics adapters ran the GPU self-test on each one and saved the
  results. With the GPU off for the app and the desktop background alike, doctor now checks each
  adapter against the requirements only. ([#84](https://github.com/jprodgers/jetcarp/issues/84))
- **`jetcarp paths` didn't name the runtime folder**, where the host's lock and the app's and
  host's control files live. It now prints a `runtime:` line (`runtime` in `--json`), and doctor's
  Folders section names it too. ([#85](https://github.com/jprodgers/jetcarp/issues/85))
- **On macOS, a locked Mac's sleeping display let the background host run.** Locked, the host
  held; but a few seconds after the display slept it let go and ran on the dark screen until the
  display woke: its holds asked for a wallpaper window being shown, and the host had none while
  the display slept. It now holds whenever a wallpaper is wanted and none of it can be seen: the
  session locked, every display asleep, or no wallpaper window at all.
  ([#95](https://github.com/jprodgers/jetcarp/issues/95))
- **On macOS the app stopped while its window was hidden.** Minimised, covered by another
  window or on another desktop, the app got no frames, and it answered AI control and stepped its
  world only in a frame: the world nearly stopped, and `jetcarp mcp --app` waited until the window
  showed again. Both now run in eframe's logic passes, which go on while the window is hidden, so
  tasks set up through AI control keep running when you switch away.
  ([#87](https://github.com/jprodgers/jetcarp/issues/87))
- **A rule typed in the Rule box could be saved unapplied.** The box applies a typed rule when it
  loses focus, but leaving the window, or saving, didn't take focus from it, so the scene could be
  saved with the old rule while the box showed the new one. Leaving the window now ends the edit,
  and Save applies a rule still being typed first.
  ([#90](https://github.com/jprodgers/jetcarp/issues/90))
- **On macOS, ⌘H (Quick Start) hid the app and F11 (Full screen) showed the desktop**, both taken
  by the system. Quick Start is now Ctrl+Shift+H (⇧⌘H) everywhere, Ctrl+H still working where the
  system allows, and Full screen is ⌃⌘F on macOS, as in other Mac apps, with F11 kept. ⌘M, which
  did nothing, now minimises the window.
  ([#91](https://github.com/jprodgers/jetcarp/issues/91), [#98](https://github.com/jprodgers/jetcarp/issues/98))
- **`host status` gave a screen's asked-for speed, not the speed it reached.** A world too big for
  its screen read as running at full speed (a 64-million-cell world at 3.9 generations a second
  said 30). Each screen now measures what it reaches, and the status says so when it falls short:
  `30 fps, 3.90 of 30 generations a second` (`measured_speed` in `--json`).
  ([#116](https://github.com/jprodgers/jetcarp/issues/116))
- **The app's Copy buttons and Paste did nothing.** JETCARP built its window toolkit without
  clipboard support, so copying a rule, an id, the world's hash or the AI control token, and pasting
  into a text box, never reached the system clipboard. The AI control window also now says how to
  connect an assistant (`jetcarp mcp --app`, with a Copy button), and that its address is the MCP
  server's private link to the app, not an HTTP endpoint.
  ([#114](https://github.com/jprodgers/jetcarp/issues/114))
- **Run → Jump's Generations field closed the menu when clicked**, so only the default 1,000
  generations could be jumped in place. The menu now closes only on a click outside it.
  ([#88](https://github.com/jprodgers/jetcarp/issues/88))
- **View → Keep awake did nothing on macOS** ("not available on this system yet"). It now takes a
  power assertion that the display may not sleep, as `caffeinate -d` does.
  ([#89](https://github.com/jprodgers/jetcarp/issues/89))
- **Every JACA run that didn't die was a "Battle", settled ones too.** A JACA run that had settled
  into a cycle, or froze late, was put with the battles still being fought, so a clip round on a
  JACA search was one category, full of runs that look static, and the caps for the categories
  people seldom keep (#56) never applied. A JACA run is now a Battle while it is still going, and
  *Static* once it has settled. ([#86](https://github.com/jprodgers/jetcarp/issues/86))
- **A GPU failing part way could crash the process on Windows.** After a failed batch, the
  stepper let go of its buffers while the GPU still had work in flight, and a readback waited for
  the GPU for ever. It now lets the GPU finish (or fail) before anything goes, a stepper does the
  same when it is dropped, and a wait for the GPU over 60 seconds counts as a failure, so the
  processor carries on. The GPU recovery test can still hang now and then on an Intel UHD under
  Windows (1 run in 30); that part is still open.
  ([#52](https://github.com/jprodgers/jetcarp/issues/52))
- **At login on Windows, JETCARP's launcher kept Windows waiting.** The launcher, `jetcarpw.exe`,
  waited for the host it started, so Windows, which runs the login entries one at a time and waits
  on each, waited on it too, which held up the entries after it. At login the launcher now starts
  the host and exits. ([#78](https://github.com/jprodgers/jetcarp/issues/78))
- **An explore search packed and imported had a few fewer confirmed elites than the search.** Runs
  often tie for a niche's last places (every run that dies at the same generation scores the same),
  and a tie went to the run that came first. A search takes in its runs as its GPU batches and CPU
  rounds finish, not in the order of their ids, while an import rebuilds the archive in id order, so
  0.2–0.5% of the elites were other, unconfirmed runs of the same score. A tie now goes to the run
  with the lower id, so the archive holds the same elites in whatever order its runs come. A search
  made before keeps its elites until `jetcarp jobs rescore` picks them again from its runs.
  ([#54](https://github.com/jprodgers/jetcarp/issues/54))
- **An export took a running explore search without a word when it had no runs to seal yet.**
  Publishing refused a running search only when it had runs not yet sealed, so one that had just
  started, or had sealed every run, was packed as it stood while it went on storing runs the pack
  left out. A running search is now refused whatever it has stored, and an export or publish that
  is refused no longer leaves its empty `--to` folder behind, nor an export the hidden store
  beside it. ([#57](https://github.com/jprodgers/jetcarp/issues/57))
- **A job whose process was killed read as running for up to five minutes.** `jetcarp jobs status`
  called a job stale only once its progress and heartbeat were five minutes old, `jobs wait` said it
  was gone after 45 seconds, and `jobs resume` could take it at once, so the three disagreed. A
  runner holds its job's lock and names itself in the job's row, and the system lets go of the lock
  when the process ends: a job that names a runner and whose lock is free is now stale at once, in
  `jobs status`, `jobs list`, `jobs wait`, `jobs remove` and the app's *Hunts…* window alike. `jobs status` says so in words (it printed `status running` and kept `stale` to its
  JSON), and so do the MCP server's `job_status` and its list. `--status running` leaves stale jobs
  out, and `--status stale` lists them. ([#49](https://github.com/jprodgers/jetcarp/issues/49),
  [#64](https://github.com/jprodgers/jetcarp/issues/64))
- **`jobs wait` straight after `jobs resume --detach` said the job was gone.** The new process had
  not taken the job yet, and the killed run's name was still in its row. `--detach` now clears it,
  and returns only once the new process holds the job (within 30 seconds, or it says so and puts
  the job back as it was). A `--detach` while another process still runs the job is refused, and
  one just after a stop waits for that run to let go; both used to start a process that failed
  unseen. ([#50](https://github.com/jprodgers/jetcarp/issues/50))
- **A search resumed without `--minutes` ran with no end, though it was started with them.**
  `--minutes` belonged to the session it was given to, so a search started with `--minutes 120`,
  cut off after an hour and resumed with a plain `jetcarp jobs resume`, ran until stopped by hand.
  A job now keeps the minutes and the `--until` it was started with, and a resume given no budget
  of its own runs the minutes left (counting the time it has run in every session) and keeps the
  same end, saying so; once they are used up, a resume runs until stopped, as before, and says
  that. `--limit` still counts one session. Scripts and the MCP server's `search_start` resume the
  same way. ([#51](https://github.com/jprodgers/jetcarp/issues/51))
- **A search on the MCP server's own thread was left running when the server exited.**
  `search_start` without `detach` runs a search on a thread of the server, and said it stops when
  the server exits; but when the client closed the server's input, the server exited at once and
  the job stayed marked running until it went stale. The server now stops its own searches at their
  next checkpoint, as `job_stop` does, waits for them (up to 30 seconds), and leaves them paused and
  resumable. ([#64](https://github.com/jprodgers/jetcarp/issues/64))
- **Two MCP answers misled about jobs that had stopped.** `search_start {"resume": N}` on a
  finished job answered with a note that the job ran in the background and how to stop it; it now
  says the job is already done, with nothing to carry on, and how to see its finds and report.
  `job_status` with `wait_s` after a stop answered as soon as the stop marked the job paused, with
  the counters of the step before the last; it now waits, as `jobs wait` does, until the run has
  written its last checkpoint and let go, so `jetcarp jobs status --wait` gives the final counters
  too. ([#68](https://github.com/jprodgers/jetcarp/issues/68))
- **The MCP tools resolve symbolic links before checking that a path is inside the folders they
  may use.** `files_list` also lists the folders given with `jetcarp mcp --allow`, as the other
  tools read and write there.
- **The folder checks cover each file JETCARP names itself inside a folder it was given, or beside
  it**, as well as the folder.
- **MCP `world_new` given width and height skipped its 4,000,000-cell limit.** It checked each
  side, not the cells the grid would have, so 4096 x 4096 (16.8 million cells, or 50 million on
  trihex) was built. The limit now counts the world's cells, several to a unit cell on trihex and
  the k-uniform tilings, for a new world, an opened scene and a script's world alike, and the
  refusal names both numbers and sides that fit: "4096 x 4096 on square is 16,777,216 cells, over
  the limit of 4,000,000 cells (try 2000 x 2000)".
  ([#63](https://github.com/jprodgers/jetcarp/issues/63))
- **With a read-only output folder, four MCP tools blamed the folder rules.** A wallpaper, a
  scene's media, a pack published and the website export each checked their folder by way of a
  file in it, `.keep`, and reported a folder they could not create as a path outside the folders
  they may use, naming that file. They now say "could not create folder …: Permission denied",
  and a refusal names the path that was asked for (a wallpaper's file, or the folder).
  ([#65](https://github.com/jprodgers/jetcarp/issues/65))
- **`pack info` called a pack changed after signing "not signed".** A version whose signature is
  there but does not verify now reads "signed, but a signature does not verify: the version was
  changed after it was signed, or the signature is damaged", with the reason. The refusal of an
  unsigned version names the option, `--allow-unsigned` on the command line and `allow_unsigned`
  through MCP, and says "check" when it was a check, not "import".
  ([#66](https://github.com/jprodgers/jetcarp/issues/66))
- **A malformed rule in a scene, and a read-only library, gave errors that didn't say what they
  were about.** A rule the parser could not read said only "Generations need a state count, like
  B2/S/C3"; it now quotes the rule and names the CA and the scene's file:
  `unknown-rule.json: CA 1 'Life': rule 'B3/S23/Q9 Frobnicate': Generations need a state count,
  like B2/S/C3`. A library whose file or folder cannot be written (read-only media, or a library
  shared to be read) could not even be listed ("attempt to write a readonly database"); it now
  opens to be read, and a command that would write to it says the library's file is read-only.
  ([#67](https://github.com/jprodgers/jetcarp/issues/67))
- **Some MCP argument errors named a Rust type, and some wrong values passed silently.** A
  wrong-typed argument now says which and what to give ("`width` must be a whole number (got
  "abc")", not "expected i64"), and a value of the wrong type that a tool would have ignored
  (`help {"topic": 5}`, `reference {"family": 7}`) or one below the schema's minimum
  (`"threads": -1`) is refused. `world_read`'s refusal of an unknown format names `objects` too,
  and publishing a pack no longer advises `jetcarp jobs seal`, which no MCP tool runs: it seals
  first, and a note now appears only if runs are still unsealed.
  ([#68](https://github.com/jprodgers/jetcarp/issues/68))
- **`host status --json` said `"paused": false` while `paused_why` gave a reason.** A hold (on
  battery, a busy processor, the desktop covered or dark) was listed in `paused_why`,
  though `paused` means paused by hand. `paused_why` now gives only why it is paused, and a hold's
  reasons are in a new `held_why`, beside `held`, the holds' names. The text status and the
  *Desktop background* pane say "held: the processor is busy (resumes by itself)".
  ([#76](https://github.com/jprodgers/jetcarp/issues/76))
- **`host status` kept its start-up GPU note after the GPU setting changed.** Turning the desktop
  background's GPU simulation off, on or back to follow, or the shared setting, took effect at once,
  but the status went on saying "GPU simulation auto (the setting in gpu.json)" and had no field for
  what each screen steps on. Each screen's line now ends with what its world steps on now and why
  (`simulation: GPU (Intel(R) UHD Graphics …)`, or `simulation: CPU: GPU simulation off, from the
  desktop background's own setting`), `--json` gives each screen's `simulation` (`on`, `adapter`
  and `why`), and the log's GPU line is replaced when the setting changes.
  ([#75](https://github.com/jprodgers/jetcarp/issues/75))
- **`host status` called a screen frozen by a hold *paused*, and doubled its reason** ("held: held
  by the desktop: windows cover it"). A screen frozen by a hold now says *held*, one paused by hand
  *paused*, and the reason reads "held: windows cover the desktop (resumes by itself)". The app's
  *Hunts…* window says *stale*, as the command line and the MCP server do, not *stalled*.
  ([#76](https://github.com/jprodgers/jetcarp/issues/76))
- **On macOS, JETCARP's memory readings counted gigabytes of memory already freed.** The resident
  size it read includes pages the allocator has freed but the system hasn't taken back yet: a long
  explore search showed 5.6 GB where Activity Monitor's figure, its physical footprint, was 533 MB.
  The status lines, `--max-rss-gb` and the background host's memory limit now use the physical
  footprint on macOS. ([#80](https://github.com/jprodgers/jetcarp/issues/80))
- **On macOS, after an upgrade, the desktop background didn't come back at login.** launchd keeps
  the code requirement of the program a login item runs, and refused the new `jetcarp` at the first
  login (`OS_REASON_CODESIGNING`), since each Homebrew build is signed on its own. The login item
  now records the version that registered it with launchd, and when another version's is found,
  the app (as it opens), `host start` and `host wallpaper on` write it again for this program and
  register it with launchd again, which stops a host launchd started from the old version and
  starts the new one. `host login on`, *Start at login* and setup's offer register it at once, so
  the host starts then rather than at the next login; a host already running holds its lock, so
  there is never a second one. Homebrew's notes after an upgrade say to open the app or run
  `jetcarp host start` once. Turning start at login off (or `jetcarp uninstall`) now also unloads
  the job from launchd, which had kept it until logout, unless the host it started is running.
  ([#99](https://github.com/jprodgers/jetcarp/issues/99))
- **The MCP reference lost its placeholders.** Where a tool's text named one outside a code span,
  such as `"pattern": "<id>"` or the size format `L<level>`, the book took it for an HTML tag and
  showed `"L"`. The page now shows them. ([#92](https://github.com/jprodgers/jetcarp/issues/92))
- **The book's first hour didn't work as written.** Its command blocks held a `$` prompt, the
  command and its output together, so copying a block, or pressing its copy button, pasted all
  three and nothing ran. Each command is now in a block of its own with no prompt, and its output in
  a separate block after it; pages whose output shows Linux folders say that the folders differ by
  system, and commands lost the trailing `#` comments that zsh, the Mac's shell, does not read as
  comments. *Installing* points testers to *Release candidates* near its top, where the example is
  a placeholder (`vX.Y.Z-rc.N`) and the page says where to find a candidate's tag, and it ends with
  how to open the app: `jetcarp`, on every system, or on Windows the Start menu entry Scoop adds.
  *First run* says so too, before the seizure warning and setup. The MCP tutorial's
  `claude mcp add` registered JETCARP for the folder it was run in only, Claude Code's default; it
  is now `claude mcp add --scope user jetcarp -- jetcarp mcp`, with `/mcp` to check it and
  `claude mcp remove jetcarp` to remove it.
  ([#100](https://github.com/jprodgers/jetcarp/issues/100),
  [#101](https://github.com/jprodgers/jetcarp/issues/101),
  [#102](https://github.com/jprodgers/jetcarp/issues/102))
- **Uninstalling read as not uninstalling, and failed a step on macOS.** `jetcarp uninstall` now
  says first what it does and doesn't: it undoes what JETCARP set up outside its folders, and the
  program stays, as do the settings and data unless asked. It ends with the one command that
  removes the program, told from where the program is (the installers' folder, Homebrew's `Cellar`,
  Scoop, winget, pipx, uv, another Python environment, cargo), rather than a list of eight, which
  stays for a program it can't place; and with a line on removing JETCARP from an AI assistant
  (`claude mcp remove jetcarp`, or Claude Desktop's config), which otherwise lists it as failing to
  connect. On macOS the data folder is inside the settings folder, so `--settings --data` deleted
  it with the settings and then reported "FAILED ... No such file or directory" for it: the data
  folder now goes first, a folder already gone counts as done, and `--settings` alone, which
  deleted the data folder with the settings, keeps it. The installers' `--uninstall` no longer says
  to run `jetcarp uninstall` first after the fact, or points to `jetcarp paths` once the program
  has gone: they name the folders kept instead. The [Uninstalling](../guides/uninstalling.md) page
  puts each command in a block of its own (a table cell held two, joined by ", or", and copied
  whole the installer refused `--uninstall,`), says plainly that `jetcarp uninstall` keeps the
  program and that any release's installer removes any version's install, and has a step for the
  assistant. ([#103](https://github.com/jprodgers/jetcarp/issues/103))
- **The Windows installer's refusals looked like a crash.** Each one-line refusal (a folder it
  didn't make, nothing to uninstall, a version that isn't one) was followed by PowerShell's error
  block, because the script stopped with `throw`. A refusal is now its one line and exit code 1. CI
  now parses the installer with Windows PowerShell and checks a refusal.
  ([#117](https://github.com/jprodgers/jetcarp/issues/117))
- **Small corrections from the book audit.** The AI control window named its command with literal
  backticks; it now points to the command's own row. *Help → Keyboard shortcuts* lists ⌘M on macOS.
  `jetcarp host login --help` says that on macOS `off` unloads launchd's job, and `setup --help`
  says that `--seizure-warning` without `--profile` saves only the answer when setup can't ask the
  rest (with `--yes`, or without a terminal).
  ([#118](https://github.com/jprodgers/jetcarp/issues/118))
- **`jetcarp host start` straight after `host stop` could fail on a busy Windows machine.** `host
  stop` returned once the old host's port had closed, but the old process could hold the host's
  single-instance lock a moment longer. The new host found it held and quit, and `host start` said
  only "the host did not start". `host stop` now also waits for the lock, and a new host waits up to
  2 seconds for one that is still closing.
  ([#122](https://github.com/jprodgers/jetcarp/issues/122))
- **Progress lines wider than the terminal left pieces behind.** A job's progress, redrawn in place
  on standard error, wrapped in a narrow window, and each redraw went back only to the start of its
  last row, leaving the rest ("newest: p3 oscillatohunt 1: 192/5,000 soups, ..."). A progress line
  in a terminal is now cut to its width, its least important part (a hunt's "newest: ...") going
  first; written to a file, lines stay whole. ([#104](https://github.com/jprodgers/jetcarp/issues/104))
- **In Windows' old console window, progress lines printed `←[2K` and left a row behind each
  redraw.** The line is erased with an escape code before each redraw, and the old console window
  (conhost, which cmd and PowerShell open unless Windows Terminal is the default, as on Windows 10)
  prints escape codes as text while they are off. JETCARP now turns them on for standard error at
  start-up, and where it can't, erases the line with spaces.
  ([#127](https://github.com/jprodgers/jetcarp/issues/127))

### Changed

- **The desktop background has its own pane.** *View → Desktop background settings…* (linked from
  the *Display* and *Screensaver* panes) holds everything about the background host that was in the
  *Screensaver* pane's *Background host* section, and more: each screen's rotation and monitor,
  *Add a screen*, the lock screen, the still picture, the frame rate, *GPU simulation* (see
  below), the caption, the pauses, start at login, GNOME's extension, and the new *Performance*
  group. The *Screensaver* pane keeps the screensaver's own settings, its profiles
  and rotations (which the desktop background shares), and the system screen saver's rotation.
  `host.json` and `app.json` are unchanged.
- **The still picture the host leaves carries its scene,** as every picture JETCARP writes does, so
  the desktop's picture can be opened in JETCARP and run again.

- **JACA worlds run on the GPU.** With *GPU simulation* on, the app and the background host (so
  live desktops, the lock screen and the screen saver) step JACA on the graphics card: lattices and
  aperiodic patches, both neighbourhoods, blast radii up to 64, every edge. The result is the CPU's
  to the bit, the annihilation glow and the counts on the card and in the status included. On an
  AMD Radeon 8060S a 1-million-cell JACA world runs about 48 times as fast as on 32 CPU threads.
- **B0 Life-like rules and Margolus blocks run on the GPU,** in the app and the background host,
  with the CPU's results. Both depend on the step count's parity, which the GPU now keeps.
- **Larger than Life runs on the GPU,** on lattices and on aperiodic patches, in the app and the
  background host, with the CPU's results. A range-5 rule on a million cells runs about 40 times as
  fast.
- **The forest fire runs on the GPU,** in the app and the background host. The GPU makes the CPU's
  64-bit random numbers and compares them with the probabilities as whole numbers, so every fire
  starts on the same tree at the same generation.
- **Sandpiles run on the GPU,** in the app and the background host, on lattices and aperiodic
  patches: the same grains, drops and losses as on the CPU, piles past 2^32 grains included. A script
  step that waits for a sandpile to be done still runs on the CPU.
- **Upgrading JETCARP upgrades its GNOME extension.** The background host, when it starts, and any
  `jetcarp host` command bring an installed extension up to the program's version (an extension you
  have switched off stays off). GNOME loads the new files at the next login, so log out and back in
  once after upgrading; until then `jetcarp host gnome status` says that an older version is running.
- **A `V` may follow the B or S counts of a rule.** `B3/S23V/C4` and `B47V/S13568/4` now mean the
  same as `B3/S23/C4V`, which is how JETCARP writes them. A `V` anywhere else is refused with the
  rule written the right way; the message used to name only the letter.
  ([#5](https://github.com/jprodgers/jetcarp/issues/5))
- **Hunts, bounded searches and censuses read every rule before they start.** A rule in `--rules`
  or `--rules-file` that cannot run stops the job from starting, and the error names each bad rule
  by its line in the file. A hunt used to start anyway and report the rule once per tiling, with no
  soups run. ([#5](https://github.com/jprodgers/jetcarp/issues/5))
- **A JACA run that lost a sign scores low and has a niche of its own.** A JACA run whose sign
  fell below 1% of live cells, at any point in the run, keeps 0.3 of its score, under the built-in
  score and fitted models alike, and a JACA run's niche now includes whether both signs held, so
  runs that turned into plain Life-like runs stop competing with battles for a place.
- **Run segments are schema 2** (`jetcarp-explore-runs 2`), with the sign columns. Segments of
  schema 1, from earlier searches and packs, still read, as runs without sign measures, and
  publishing and importing copy segments as they are. JETCARP 0.4.1 cannot read a segment of
  schema 2.

- **The GPU is the default wherever it gains, and the processor the fallback.** Under the new
  automatic GPU setting, the app's world, wallpapers and screen savers step on the GPU from 16,000
  cells; `jetcarp run`, `jetcarp wallpaper`, scripts (`w.step`, sweeps) and the MCP server's
  `world_step` and `world_render` from a million neighbour reads a generation; hunts and explore
  searches always; piles from 100,000 grains. The results are the same, bit for bit. The app's
  world used to start on the processor, and the command line's runs and searches stayed there unless
  given `--gpu`. See [The GPU](../guides/gpu.md).
- **`--cpu` joins `--gpu`** on `jetcarp run`, `wallpaper`, `search hunt`, `search explore`, `pile`,
  `jobs resume`, `jobs refit` and `jobs rescore`; either wins over the GPU setting. The operations'
  `gpu` options are now true, false or left out (the setting decides), and an explore search or pile
  made with `--gpu` or `--cpu` keeps that choice. `jetcarp setup --no-gpu` is unchanged.
- **Displays use the adapter driving the screen.** The app, the host, the lock screen and the screen
  savers prefer it (the integrated GPU on a laptop with two), so a wallpaper does not keep a
  discrete GPU awake; searches and runs use the fastest adapter.
- **One GPU setting, shared, with a choice of their own for the app and the host.** The shared
  setting (`gpu.json`: Automatic, On, Off) is *Shared setting* in the *Desktop background* pane.
  The app's *Run → GPU simulation* and the pane's *GPU simulation* each offer *Follow the shared
  setting* (the default), *On* or *Off*, saved in `app.json` and `host.json`; so do
  `jetcarp app display --gpu follow|on|off` and MCP `app_display {gpu}`. A `host.json` from before
  reads `true` as follow and `false` as off. First to last: `JETCARP_GPU` (when it names a mode, it
  now comes before a command's `--gpu` or `--cpu`), the command's own choice, the app's or host's
  own, the shared setting, automatic. `jetcarp doctor` says what the app and the host each do, and
  where that came from.
- **A wallpaper on the GPU costs the processor far less.** Reading a small world back from the GPU
  woke every worker thread each frame, about 4 ms of processor time; worlds under a million cells now
  do it on one thread, about 0.04 ms.

### Added

- **A photosensitive seizure warning, answered before JETCARP animates anything on screen.** Some
  rules flicker the whole screen many times a second (a B0 rule turns the whole field over every
  generation), which can trigger seizures in people with photosensitive epilepsy. The app now opens
  on the warning, before setup, and an existing user sees it at the next launch. To go on, you
  confirm that you don't have epilepsy or a seizure disorder; the other answer advises you not to
  use JETCARP, with *Quit JETCARP* as its main button and a smaller *Continue at my own risk*. The
  answer is saved in the settings folder (`photosensitivity.json`, with its date and version), so
  the planned strobe limiter ([#97](https://github.com/jprodgers/jetcarp/issues/97)) can switch
  itself on for those at risk. `jetcarp setup` asks it first in a terminal, and with `--yes` needs
  `--seizure-warning no-disorder` (or `at-risk`) the first time. Until it has been answered,
  `jetcarp host start`, `host wallpaper on`, `host login on` and `host saver install` refuse with the
  warning, a host started at login holds its screens still and says why in `host status`, the
  screen saver does not start, and the MCP tool `host_control` refuses to start showing scenes, as
  the command line does.
  Pictures, animations and other files are not held back. See
  [Photosensitive seizure warning](../getting-started/photosensitivity.md); the README and the book's
  introduction, installation and desktop pages carry the warning too.
  ([#96](https://github.com/jprodgers/jetcarp/issues/96))
- **`jetcarp host scene`** prints or saves (`--save FILE`) the scene a screen shows, its grid's size
  included, so a wallpaper replays exactly; `host_control` gives it as `{"action": "scene"}`.
  The host's status gives each screen's cell size and grid.

- **Export an animation of the world.** *File → Export animation (GIF or WebP)…* records the view
  and the generations after it, as a GIF or a lossless WebP: how many generations, a frame every how
  many and the longer side in pixels, rendered on a copy of the world in the background. The *Hunts*
  window's *Render media*, which makes animations of finds and picks their format itself, is now a
  full-size button, *Render media (animations of these finds)*.
  ([#62](https://github.com/jprodgers/jetcarp/issues/62))
- **Choose the GPU in the app.** *Run → GPU adapter* lists *Automatic*, *Discrete*, *Integrated* and
  each adapter found, and saves the choice as the shared setting's adapter (`gpu_adapter` in
  `gpu.json`, which could be set only by editing the file or with `JETCARP_GPU`), so a laptop's app
  can move from one GPU to the other. The app moves at its next start; the command line, the MCP
  server and the host at theirs. ([#59](https://github.com/jprodgers/jetcarp/issues/59))
- **A grid of a set width and height in the app.** The *Tiling* pane's *Size* has *Width × height*,
  in unit cells as `--size WxH` on the command line, so HashLife's jump in place, which needs a
  wrapping grid whose sides are powers of two, can be reached from the app; the jump's message says
  how to make such a grid. ([#58](https://github.com/jprodgers/jetcarp/issues/58))
- **Find a Quick Start entry for a rotation by name.** The rotation editor's *Add Quick Start* box
  searches the built-in entries by title, rule or handle as you type, and adds the one you click,
  where it took a handle only; a handle and Enter still work, for entries of your own.
  ([#74](https://github.com/jprodgers/jetcarp/issues/74))

- **A page for rating clips.** `jetcarp jobs clips --page` also writes `index.html` into the clips
  folder: the clips in batches, each with Keep, Maybe and Drop and a ★, with a progress bar, a
  tally, a note box, and the keyboard (← →, K, M, D, S). It works offline from the folder and
  follows the system's light or dark setting. The browser keeps the ratings as you go, and *Save
  ratings* downloads them as one file, which `jetcarp jobs review JOB --from FILE` records (with
  `--rater NAME` to name someone else). Stars are kept as five-star ratings of the runs, apart from
  the verdicts. Scripts and `jetcarp api call` reach it as `search.clips` with `page`.
- **`jetcarp library soup ID`** rebuilds the exact soup a hunt's find came from, checks with the
  hunt's own engine that it makes the find, and gives a scene that replays to it (`--save FILE`).
  `--best N` runs the hunt's soups again and gives the N that made the most copies of the find.
  Hunt finds now keep the soup's shape and the hunt's seed in their record, so their soups can be
  made again without the hunt's job; earlier finds' own soups are rebuilt too, but `--best` needs
  their job. Scripts reach it as the operation `library.soup`.
- **`--max-rss-gb` for hunts and bounded searches,** as explore searches have it: past that much
  memory the search pauses, resumable, and says why. The job keeps the cap, so a resumed run keeps
  to it too. Scripts and the MCP server's `search_start` take it as `max_rss_gb`.
- **`jetcarp jobs wait JOB`** waits until a job stops running, in this process or another, and its
  exit status says how it ended: 0 done, 3 paused, 4 failed, 5 still running at `--timeout S`, and
  6 when the job is marked running but its process crashed or was killed. Scripts call it as the
  operation `jobs.wait`. A search started or resumed with `--detach` is now marked running as the
  command returns, so a `jobs wait` or `jobs status` straight after sees it running.
- **Scripts can read a tiling's geometry.** `w.neighbours(i)` and `w.neighbours(i, "edge")` give a
  cell's touch or edge neighbours, `w.live_cells()` the cells that are not empty with their states,
  and `w.tile(i)` a tile's centre and corners, on any tiling, aperiodic patches too. `w.cas()` now
  says each CA's neighbourhood. A new example script, `jaca-territories`, uses them to follow a JACA
  world's sign balance, how sorted its territories are, its fronts, and the ground the signs trade.
- **JACA runs record how their two signs share the board.** An explore search measures five more
  things about each JACA run: which sign is ahead, how balanced the signs are, how well they sort
  into territories, how much territory changes hands late, and when a sign was lost. They are new
  columns of the run segments (null for other families) and show in `jetcarp jobs runs
  --measures`. In a JACA-only search, more than half of the best-scoring runs had lost a sign in
  the first half, which leaves a plain Life-like run; see
  [Search methods](../research/search-methods.md#explore).
- **Your taste as the score.** `jetcarp jobs refit JOB` fits a score model to the keep, maybe and
  drop verdicts on a job's runs, says how well it ranks runs it did not see, and with `--out`
  writes it as a file. `jetcarp search explore --score FILE` searches with it, and `jetcarp jobs
  rescore JOB --score FILE` rescores a job's runs with it; the job keeps the model, so resuming
  the search keeps it. Fitted to 227 ratings of JACA runs, a model ranked keeps above drops 90% of
  the time on runs held out of the fit, against 74% for the score they were stored with. Older
  runs replay exactly, so a refit runs again those that lack the sign measures, and `jetcarp jobs
  rescore --replay` does it for a whole search (`--gpu` on the GPU). The MCP `search_runs` tool
  can refit and rescore too, and `search_start` takes a model.
- **The explore search breeds JACA its own way.** Parents are chosen in proportion to quality, a
  fifth of the time at random, and for JACA only from runs whose signs both held. Strong JACA pairs
  are transplanted to tilings they have not run on (with their counts scaled when the neighbour
  counts differ), and pairs whose weaker sign is losing are tuned to help it, with 30% of JACA's
  candidates still fresh random draws. `jetcarp search explore --parents-from JOB` starts a search
  from another job's reviewed JACA runs (`--reviewed`, default keep and maybe), transplanting and
  tuning them first. See [Search methods](../research/search-methods.md#explore).
- **Every measure out.** `jetcarp jobs runs` takes `--measures all` (or a list), `--format
  csv|json`, `--since ID` for the runs after an id, `--limit 0` for every run, and `--out FILE`.
  The MCP `search_runs` tool writes a CSV file of every run with `csv` and limit 0.

- **A GPU setting shared by the app, the host, the command line and the MCP server**: `gpu.json`
  in the settings folder, with `gpu` (`auto`, `on`, `off`) and an optional `gpu_adapter`
  (`integrated`, `discrete`, `software`, or part of a name). `JETCARP_GPU` overrides it with the same
  words, so `JETCARP_GPU=UHD` uses an Intel UHD rather than the RTX beside it. Software adapters are
  used only when asked for, and Windows' WARP never.
- **A GPU self-test.** Before simulating on an adapter, JETCARP steps one rule of every family the
  GPU runs on two lattices and an aperiodic patch, and a small explore batch, and compares every hash
  with the processor's (about 0.1 seconds). An adapter whose results differ is not used. The result
  is kept in `gpu-checks.json` in the settings folder for that adapter, driver and version.
- **`jetcarp doctor` says which adapter each purpose uses and why**: the GPU setting and where it
  came from, and for displays and computing the adapter, how it was chosen, its limits, the
  self-test's result and whether the simulation uses it; `doctor --json` has it as `gpu_policy`.
- **`examples/bench.rs` in `jetcarp-gpu` times from 4,000 cells up**, with a `--paced` table of
  one generation a frame and the processor time each path takes, from which the automatic
  thresholds come.

- **Ways to take part.** On GitHub, new issues offer forms for a bug report, a feature request, a
  new rule or tiling, and a history or credit correction, a pull request starts from a checklist,
  and `CONTRIBUTING.md` and a code of conduct (the Contributor Covenant 2.1) join the repository.
  Security problems can be reported privately through GitHub's *Report a vulnerability* as well as
  by email. See [Contributing](contributing.md).

- **A page on how JETCARP is tested**, [How JETCARP is tested](testing.md): the automated tests and
  CI, the release candidates tested by hand on three machines with GPUs from four vendors (their
  specifications), how each problem is handled, who does what, a record of the 0.4.2 cycle, and what
  the testing does not cover. [How JETCARP was made](how-made.md) says more exactly who did what in
  testing.

### Removed

- **Fit to this computer, tried in the release candidates, is not in 0.4.2.** It showed each scene
  of the desktop background at a ladder of cell sizes, measured each, and recommended one, but a fit
  took from 45 seconds to 12 minutes, against a target of 10 to 30 seconds. A faster one is planned
  for 0.4.3 ([#113](https://github.com/jprodgers/jetcarp/issues/113)). Each scene shows at its own
  size, as in 0.4.1: the *Desktop background* pane has no *Fit to this computer…* or its cards and
  limits, `jetcarp host tune` refuses, the MCP tool `host_control` has no `tune`, and the host
  measures nothing when the monitors change. Sizes a release candidate's Fit saved stay in
  `host.json`, unused. ([#112](https://github.com/jprodgers/jetcarp/issues/112))

## [0.4.1] – 2026-11-13

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
- **Windows lists the screen saver as JETCARP,** not "Jetcarp": the programs now carry the name
  Windows reads for that list. `jetcarp host saver install` also says that Windows chooses JETCARP
  as Screen Saver Settings opens, so Cancel there keeps it chosen.
- **`jetcarp doctor` notes a data folder that is gone.** When the folder chosen in setup
  (`paths.json`) was moved or deleted, it said only "not created yet"; it now says so and how to
  name the new place.

### Changed

- **More atlas titles tell entries apart.** A JACA entry is titled by its rules up to 40 characters
  (*Battle JACA +B14/S124 -B12/S345 blast 1*, not a plain *Battle JACA*), and a Priority entry by its
  number of states (*Oscillating Priority 6*). Entries from earlier builds keep their titles.
- **One DOI to cite.** The citing page, the README and `CITATION.cff` give Zenodo's DOI for every
  version, [10.5281/zenodo.24191765](https://doi.org/10.5281/zenodo.24191765), which resolves to the
  newest release, and ask for the version you ran.

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
