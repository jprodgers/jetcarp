# The pattern library

Everything JETCARP's searches find goes into your *pattern library*: still lifes, oscillators,
spaceships, puffers and guns, ant and turmite runs, and movers on aperiodic tilings. Each record has
been verified exactly, carries the range of rules it works in, counts every time a search saw it,
and remembers where and when it was found. You curate it (names, tags, notes, ratings), promote the
best to your Quick Start menu, and share it in [packs](packs.md) or as a
[website](website-export.md). The file itself is described in
[Library format](../reference/library-format.md).

The library is one file, `library.sqlite`, in JETCARP's data folder (see
[Folders](../reference/folders.md)). `--library FILE` on any command, or the environment variable
`JETCARP_LIBRARY`, uses another one.

```text
$ jetcarp library info
catalog:        /home/ada/.local/share/jetcarp/library.sqlite
schema version: 3
patterns 149, rules 1, pattern rules 149, sightings 149 (446483 copies), jobs 8, Quick Start 364, ratings 0, media 0, ants 98
```

## Browse

`jetcarp library list` shows patterns, the highest *interest score* first:

```text
$ jetcarp library list --works-in B36/S23 --class oscillator --limit 4
89afce3a64a4c19cb274f4f1e2bf254e  p26 oscillator 6c #89afce   B3/S23  Hat     p26   6c  seen 1    score 68.4  new
6d445584e8b5cb1686a80ec55492af9f  p4 oscillator 15c #6d4455   B3/S23  Hat     p4   15c  seen 1    score 47.9  new
45b218dae7cb76eb193a78cb14981562  p2 oscillator 3c #45b218    B3/S23  Hat     p2    3c  seen 22   score 26.1  new
299d09888b99cae4935e015f36b49976  beacon                      B3/S23  Square  p2    8c  seen 321  score 25.6  new
```

Each row gives the pattern's id, its name or label, the rule it was found in, the tiling, its
period and cells, how often searches have seen it, its score and its curation state. The score
favours larger periods, faster ships, more cells and rarity. The filters:

| Option | Patterns that… |
| --- | --- |
| `--class still_life\|oscillator\|spaceship` | are of that class |
| `--tiling ID` | are on a tiling: a family id (`hex`), a catalog id (`k2-001`) or part of its name |
| `--rule TEXT` | were verified in a rule whose notation contains the text |
| `--works-in RULE` | run in this rule: inside their rule range, or verified there |
| `--min-period N`, `--max-period N`, `--min-score X` | pass those bounds |
| `--state new\|reviewed\|promoted\|rejected`, `--tag TAG`, `--text TEXT` | carry that state, tag or text |
| `--by search\|script\|llm\|user` | were found that way |

`--order` sorts by `score` (the default), `seen`, `rare`, `newest`, `updated`, `cells`, `period`,
`rating` or `id`. `--limit` and `--offset` page through, `--ids` prints ids only, and `--json` whole
records.

The `--works-in` filter is where the rule ranges pay off: a pattern found in Life also runs in every
rule in its range, so the example above found oscillators from Life that run unchanged in HighLife
(`B36/S23`).

**One record.** `jetcarp library show ID` (any unique start of the id, or the pattern's name) prints
everything: the rule range, the displacement and speed, the cells over its period, every sighting,
how it was verified, where it was found, and a drawing of each phase. `jetcarp library render ID
--png FILE` draws the phases, and `jetcarp media render --pattern ID` makes its thumbnail, poster
and animation (see [Pictures and animations](pictures.md)).

## Curate

```text
$ jetcarp library update 47fc3f --name "kagome glider" --tags kagome,known --rating 5 --state reviewed
updated 47fc3f43aae9ab4683227d5fdb7cfde0 (kagome glider), state reviewed
```

- `--name`, `--tags` (or `--add-tag`), `--notes` and `--rating` (1 to 5 stars; 0 clears) are
  yours to set. They go into your packs as your annotations.
- The state marks where a pattern stands: `new`, `reviewed`, `promoted` or `rejected`. Only
  reviewed and promoted patterns go into a [website export](website-export.md), and a rejected one
  never does.
- `jetcarp library remove ID` deletes a pattern with its rules, sightings and Quick Start entry.

**Promote a find** to your Quick Start menu:

```text
$ jetcarp library promote 47fc3f
promoted: kagome glider
entry:    find:47fc3f43aae9ab4683227d5fdb7cfde0 in /home/ada/.local/share/jetcarp/library.sqlite
It appears in the app's Quick Start under "My finds".
```

The entry is a scene with the pattern in it, ready to open. `promote` also takes a run of an
explore search (`--search-run RUN --job JOB`) or any scene file (`--scene FILE`), and
`promote-run` takes a run of a script's sweep folder (see
[Scripting a rule sweep](../tutorials/scripting-a-sweep.md)). `jetcarp library quickstart` lists
the entries that aren't built in, and hides, shows or removes one.

## Ant runs and movers

Ant and turmite runs are kept apart from patterns, under `jetcarp library ants`:

```text
$ jetcarp library ants show 6ab5f5
ant-6ab5f538561ff2d8c7d09af7889d4ce4 L2L2L Hexagon: highway p1940 d(5,-6) from step 367643
  rule Turmite L2L2L (1 states, 3 colours), tiling Hexagon, start empty
  436940 steps, 3125 coloured cells, score 93.7, state new
  proved at step 435000: period 1940, d (5, -6), 297 cells compared
```

A highway is *proved*: JETCARP found a stretch of the ant's path that repeats exactly, moved by a
fixed displacement, and compared every cell it touches. `ants list`, `update`, `remove`, `scene`
and `promote` work as they do for patterns, and `ants run RULE` runs one rule by itself.

On aperiodic tilings nothing repeats by translation, so a glider there is a *mover*, followed over
growing patches by `jetcarp library movers track` and checked again by `movers verify` (see
[Aperiodic patches and following movers](aperiodic-patches.md)).

## Keep it honest

The library's records are only as good as the engine that checked them, so it can check them
again:

- **`jetcarp library verify`** runs stored patterns again with this build: by default every
  pattern off squares and hexagons, with `--all` every one, and `--drop` deletes those that fail.

  ```text
  $ jetcarp library verify
  149 patterns verified (50 of them on squares and hexagons) in 1 s: 0 failed
  ```

- **`jetcarp library reverify`** checks the records a newer JETCARP flagged: those whose tiling
  geometry or rule meaning changed since they were verified (see
  [Reproducibility](../research/reproducibility.md)). Those that pass are brought up to date and
  those that fail stay flagged. It runs as a job.
- **`jetcarp library compounds`** finds records that are really several independent objects side
  by side, which older searches stored as one. It lists them and writes a report, and with
  `--apply` stores their parts in their place (see [Compounds](../reference/library-format.md#compounds)).
- **`jetcarp library prune`** deletes the large still lifes and oscillators a hunt would no longer
  store, keeping any you curated, and makes the file smaller.

## In the app

*Tools → Pattern library…* browses the same library, with the same filters and sort orders. For
each pattern it shows the phases (choose a phase and an orientation), and offers *Paste into world*,
*Open in a new world* and *Promote to Quick Start*. Its name, tags, notes, rating and state can be
edited there, with *Undo*. Records a [pack](packs.md) retracted are marked, with who retracted them
and why. Removing a pattern is done from the command line (`jetcarp library remove`).
