# Aperiodic patches and following movers

The [Penrose](../tilings/penrose.md), [hat and spectre](../tilings/hat-spectre.md) tilings never
repeat, so they cannot be wrapped into a torus the way a periodic grid can. JETCARP builds finite
*patches* of them instead, round pieces of the infinite tiling chosen by *level*, with dead edges. A
pattern that travels across one of these tilings, a *mover*, soon reaches the edge of any patch. To
see how far it really goes, JETCARP follows it on a *sliding window*: only the tiles near the live
cells exist, and the window is rebuilt ahead of the pattern as it moves, so the memory needed depends
on the window, not on the distance. This guide covers patches and their levels, running rules on
them, and following movers.

## Patches and levels

| Level | Hat, Penrose kites and darts, Penrose rhombs | Spectre |
| --- | --- | --- |
| 1 | 10 tiles | |
| 2 | 70 | |
| 3 | 500 | 559 |
| 4 | 4,000 | 4,401 |
| 5 | 35,000 | 34,649 |
| 6 | 280,000 | 272,791 |
| 7 | 2,240,000 | 2,147,679 |
| 8 | 17,920,000 | 16,908,641 |

For the hat and both Penrose tilings, a level is a size: the given number of tiles nearest a fixed
centre, so patches are round and each contains the smaller ones. For the spectre, a level is a
supertile level of its substitution. Levels 7 and 8 are built in exact integer coordinates, with every
neighbour found by an exact lookup; level 8 has 17 to 18 million tiles and needs more than a gigabyte
of memory with its neighbour graph. Each tiling's page in the encyclopedia says how it is built.

Patches always have dead edges: cells past the edge are missing and count as empty, so patterns near
the edge behave differently from patterns deep inside. The tiles have fixed shapes and cannot be
edited, and the number of neighbours varies from tile to tile (`jetcarp tilings` lists the ranges:
a spectre tile has 4 to 7 edge neighbours, a Penrose rhomb 7 to 11 touching ones).

**In the app**, choose the tiling under *Tiling → Family* (or in the *Tiling* pane), where the four
aperiodic tilings have a group of their own, then the level under *Size*, which lists levels 3 to 8
with their tile counts (*Level 4 (4,401 tiles)*). A new aperiodic world starts at level 3. The *Edges*
choice is greyed out.

**In a scene** the level is the size:

```json
"tiling": {"family": "spectre", "size": {"level": 5}}
```

and in a script, `world(#{ tiling: "hat", size: #{ level: 4 } })` or, for short,
`world(#{ tiling: "hat", level: 4 })`. Levels 1 to 8 can be used in both.

## Running rules on patches

Almost every rule family runs on the aperiodic tilings; only the few tied to a grid, such as the
elementary row rules and von Neumann's automaton, do not (see
[How JETCARP generalises automata](../automata/generalising.md)). The Quick Start has examples, such
as *B2/S34 on the hat*, *Sandpile on the Spectre*, *Spectre glider* and *Brian's Brain on Penrose
rhombs*. Searches work on patches too: the bounded soup search runs soups on a patch, and the
evolutionary glider search evolves rules that make gliders on the spectre or the hat (see
[Searching for patterns](searching.md) and [Search methods](../research/search-methods.md)). HashLife
does not, since these tilings have no lattice.

When you hover over the grid in the app, the bottom left corner shows the number of the cell under the
pointer and its state in each field (*cell 4071 · field 0: 1*). Cell numbers are the tile numbers of
the patch at that level, which is how the mover commands below name starting cells.

## Following a mover

A glider on an aperiodic tiling never repeats exactly, since the tiles it crosses never repeat, so it
cannot be proved to run for ever by finding its period. JETCARP measures instead how far it has been
seen to travel unchanged, and calls it *verified to a distance*: so many *rings* of tiles (edge steps
from the starting tiles to the nearest live tile), with the distance also given in neighbour steps and
in plane units. There are two ways to follow one.

**On a sliding window**, as a resumable job, with `jetcarp search mover`:

```text
$ jetcarp search mover --rule SpectreGlider --tiling spectre --rings 200 --store
mover job 1 in /home/ada/.local/share/jetcarp/catalog.sqlite
start: 4071:1,4097:3
mover 1: 200 rings of 200 (634 units) after 404 of 40000 generations, 11757 gens/s, 5821 rings/s, level 7, 603 tiles | verified to distance 200 edge rings (200 touching-neighbour steps, 633.6 units) over 404 generations on a sliding window of spectre (supertile level 7, at most 1009 tiles), population 2-12; stopped at the requested distance
library: 4b70f6b289a68075
```

The window never held more than about a thousand tiles. This is how JETCARP followed James Edmond's
Spectre glider pair for 10 million rings of tiles, and the hat glider for a million (see
[Findings](../research/findings.md#the-hat-glider-is-edmonds-glider-b)).

**Over fixed patches of growing level**, with `jetcarp library movers track`, which runs the pattern on
each patch from `--level` up to `--max-level` (7 on the spectre by default, where 8 is on request; the
largest elsewhere) and checks that each level agrees with the one before:

```text
$ jetcarp library movers track --rule SpectreGlider --tiling spectre --gens 3000
start: 4071:1,4097:3
verified to distance 224 edge rings (224 touching-neighbour steps, 708.2 units) over 451 generations on spectre level 7 (2147679 tiles), population 2-12; stopped at the edge of the largest patch
  level 4 (4401 tiles): 35 generations, Edge, 16 rings, population 2-12, 0.0s
  level 5 (34649 tiles): 35 generations, Edge, 16 rings, population 2-12, agrees with the smaller level, 0.1s
  level 6 (272791 tiles): 268 generations, Edge, 132 rings, population 2-12, agrees with the smaller level, 2.0s
  level 7 (2147679 tiles): 451 generations, Edge, 224 rings, population 2-12, agrees with the smaller level, 7.2s
  group of 5 cells at 712.0 units, heading 107.9 degrees
  group of 6 cells at 712.0 units, heading -131.7 degrees
```

Both commands take the same description of the pattern:

| Option | Meaning |
| --- | --- |
| `--rule` | The rule: `SpectreGlider` (the default), a priority table (`"Priority 4: …"`) or a B/S rule |
| `--tiling` | `spectre` (the default), `hat`, `penrose-kd` or `penrose-rhomb` |
| `--level` | The patch level the starting cells are numbered on (default 4) |
| `--nb` | `touch` (the default) or `edge` |
| `--cells` | The starting cells as `CELL:STATE` pairs, such as `1203:1,1204:3` |
| `--states`, `--at` | Instead of `--cells`: put these states (such as `1,3`) on tile `--at` and its edge neighbours in order; with no `--at`, try pairs around the deepest tile of the patch until one travels |
| `--gens`, `--max-pop` | Stop after this many generations (default 20,000; with `--rings`, 100 more per ring), or when the pattern grows past this many live cells (default 2,000) |
| `--store`, `--json-out` | Add the mover to the [pattern library](pattern-library.md) once verified, with the distance; write the full report as JSON |

`jetcarp search mover` also takes `--rings` (stop, verified, at this distance), `--margin` (how much of
the tiling to keep around the live cells, in plane units, default 24) and `--fixed-levels` (on the
spectre, the window follows the same tiles as the fixed patches up to this level, default 8), and
the options every search job has: `--label`, `--minutes`, `--limit`, `--resume`, `--detach`,
`--threads` and `--library` (see [Search methods](../research/search-methods.md#jobs)). A run stops
when it reaches the distance asked for, the generation limit or the edge of the largest patch, or
early when the pattern dies, grows past the cell limit or repeats in place (then it is an oscillator,
not a mover).

The library keeps movers with the distance they were verified to. `jetcarp library movers list` lists
them, farthest first, and `jetcarp library movers verify ID` follows one again from where it was
stored (20,000 generations by default) and updates its record.

In a script, `track_mover(#{ rule: "SpectreGlider", tiling: "spectre", level: 4, cells: [[4071, 1],
[4097, 3]], window: true, rings: 100 })` does the same and returns the report; without `window` it
tracks over the fixed patches. `library().query(#{ class: "mover" })` lists the stored movers (see
[Scripting (Rhai) API](../reference/scripting.md#glider-search-and-movers)).

## How the window stays exact

Every tile of the window is placed by an exact rigid motion in integer coordinates, so tiles built at
different times match exactly and neighbours are found without rounding. Live cells are always kept at
least three neighbour steps from the window's open edge, so every cell that can change sees its true
neighbours, and the run equals the run on the whole infinite tiling. The method is described in full
in [Search methods](../research/search-methods.md#movers-on-a-sliding-window).
