# Searching for patterns

JETCARP's searches look for spaceships, oscillators, still lifes, ant highways, interesting
rules and beautiful runs, and keep what they find in your [pattern library](pattern-library.md).
There are nine ways of searching, all started with `jetcarp search <strategy>`, and every one runs
as a *job*: it can be stopped at any time and carried on later, and the result does not depend on
how often it was stopped or how many threads ran it. This guide shows how to run them and how to
find what they found. How each strategy works, and how its finds are checked, is in
[Search methods](../research/search-methods.md).

## Your first hunt

The *soup hunt* drops small random soups on an unbounded plane, runs each until it settles, and
classes every object left behind. With no options it hunts Conway's Life on squares:

```text
$ jetcarp search hunt --soups 20000
hunt job 1 in /home/ada/.local/share/jetcarp/library.sqlite
hunt 1: 20,000/20,000 soups, 5954 soups/s, 430,384 objects (ships 38,188, osc 124,582, still 267,614), 50 distinct, 50 new | newest: still life 12c #bf73c4 (B3/S23)
  status done, 3 s run, 19892 settled, 52 died, 56 exploded, 0 unsettled
  B3/S23 on Square: 20000 soups, 430384 objects (oscillator 124582, spaceship 38188, still life 267614), 50 distinct, 50 new
  newest finds:
    bf73c46c186561ca3b75a3884d0b5ab5  still life 12c #bf73c4             B3/S23     Square soup 17973  (new to the catalog)
    …
```

Each soup is 16 × 16 cells at half density. Of 20,000 soups, 19,892 settled, 52 died out and 56
grew past the hunt's limit. The 430,384 objects they left are 50 distinct patterns. Every new one
is verified exactly before it is stored (run alone, it must come back to itself, moved by its
displacement, for many periods), and each copy found counts as a sighting.

Objects that merely sit side by side are counted apart: a pulsar with four blinkers beside it
counts as one pulsar and four blinkers, and two gliders flying together as two gliders. Objects
that do act on each other, such as a pair of ships bound together, stay one object (see
[Compounds](../research/search-methods.md#compounds)).

Look at what it found:

```text
$ jetcarp library list --class spaceship
5a080db389c0cab3caf884d2596a1e70  glider   B3/S23  Square  p4   5c  seen 38120  score 74.5  new  c/4 diagonal
6dd55e69689ba784aff448b7dbbc9328  LWSS     B3/S23  Square  p4   9c  seen 51     …
77364004b6d2c23cba74126b8aeb6ca3  MWSS     B3/S23  Square  p4  11c  seen 13     …
1376072ec0c084fc67a9b92d361379f3  HWSS     B3/S23  Square  p4  13c  seen 4      …
```

Patterns with a well-known name (the glider, the pulsar, the block) are shown by it; others by
their class, cells and the start of their id. `jetcarp library show ID` prints a record, with a
text drawing of its phases, and `jetcarp library render ID` draws it. The
[pattern library](pattern-library.md) guide covers curating finds.

## Other rules and tilings

`--rules` takes one rule or several, in [rule notation](../reference/rule-notation.md), and
`--tilings` takes tiling ids such as `trihex` or `k2-001` (`catalog:k2-001` works too) or whole
sets: `archimedean`, `kuniform:2`, `isohedral`, `all-periodic`. A short hunt of Life on the kagome tiling:

```text
$ jetcarp search hunt --tilings trihex --soups 5000 --label kagome
  status done, 27 s run, 31 settled, 14 died, 4954 exploded, 1 unsettled
  B3/S23 on 3.6.3.6 trihexagonal: 5000 soups, 15192 objects (oscillator 4, spaceship 15140, still life 48), 24 distinct, 17 new
```

Nearly every soup explodes there, yet 15,140 spaceships escape, and all of them are one ship: the
five-cell c/4 glider of the kagome tiling, which
[Guttesen (2026)](../research/bibliography.md#guttesen2026game) described (see
[Findings](../research/findings.md)). A find is "new to the catalog" when *your* library had not
seen it, which says nothing about whether anyone has seen it before. Before calling a pattern new,
check [What is new in JETCARP](../research/contributions.md) and the sources it lists.

Useful hunt options:

| Option | Effect |
| --- | --- |
| `--soups N` | Soups per rule and tiling (default 100,000); `--forever` runs until stopped |
| `--rules-file FILE` | More rules, one per line (`#` starts a comment). Every rule is read before the job starts: one that cannot run stops the job from starting, and the error names its line |
| `--size WxH`, `--density P`, `--shape rect\|disc` | The soups: 16 × 16 unit cells at 0.5 by default |
| `--nb touch\|edge` | The neighbourhood for every rule (a `V` suffix in a rule means edge) |
| `--gens N`, `--max-period N` | How long a soup may run before it is classed as it is, and the longest period looked for |
| `--seed N` | The soups' random seed: the same seed gives the same soups |

## Watch, stop, carry on

Every search is a job in your library:

```text
$ jetcarp jobs list
    2  hunt     done     2026-11-12T07:02:17Z  hunt 2: 5,000/5,000 soups, 187 soups/s, 15,192 objects (ships 15,140, osc 4, still 48), 24 distinct, 17 new
    1  hunt     done     2026-11-12T07:02:04Z  hunt 1: 20,000/20,000 soups, 5954 soups/s, 430,384 objects …
```

- **Budgets.** `--minutes M` or `--until UNIX-TIME` stops a search, still resumable, at its next
  checkpoint after that time (the exploratory search ends at that time; see below), and `--limit N`
  after N items.
- **Memory.** `--max-rss-gb GB` pauses a hunt, a bounded search or an explore search, still
  resumable, once JETCARP uses more memory than that. The job keeps the cap, so a resumed run keeps
  to it too, and `jetcarp jobs status JOB` says why it paused.
- **Stopping.** Ctrl-C, or `jetcarp jobs stop JOB` from another terminal, stops it at its next
  checkpoint.
- **Carrying on.** `jetcarp jobs resume JOB` carries it on from there. It ends exactly where an
  uninterrupted run would have ended, at any thread count.
- **In the background.** `--detach` runs a search in a process of its own and returns at once;
  `jetcarp jobs status JOB` follows it, and `--label NAME` makes it easy to find.
- **Waiting.** `jetcarp jobs wait JOB` returns once a job stops running, wherever it runs, and
  `--timeout S` gives up after S seconds. Its exit status says how the job ended (see below).
- **Reports.** `jetcarp jobs report JOB` writes a Markdown report of what a job searched, for how
  long, and what it found: one table per hunted rule, with its spaceships, guns, puffers and
  longest oscillators.

`jetcarp jobs remove JOB` deletes a job that has finished or stopped, with its sightings;
`--delete-results` also deletes the patterns only it found.

### Waiting in a script

`jetcarp jobs wait` ends with one of these exit statuses:

| Exit status | The job |
| --- | --- |
| 0 | is done |
| 3 | is paused: stopped, at the end of its budget, or past its memory cap |
| 4 | failed |
| 5 | was still running at `--timeout` |
| 6 | is marked running, but no process runs it: its process crashed or was killed |

Exit status 1 means the wait itself failed: there is no such job, or the library cannot be read.
A crashed run is noticed within about a minute, once its heartbeat is late. `jetcarp jobs resume
JOB` carries such a job on from its last checkpoint (an explore search from its archive), so a
long search can be kept going through crashes until a deadline with a loop in the shell:

```sh
until jetcarp jobs resume 12 --until 1763200000; do sleep 15; done
```

`jobs resume` returns 0 when the search ends or pauses at the deadline, which ends the loop; a
crash returns something else, and the loop starts it again.

## The other strategies

| Strategy | Looks for | A first command |
| --- | --- | --- |
| `bounded` | Still lifes, oscillators and spaceships on a finite grid of any tiling, aperiodic patches included | `jetcarp search bounded --rules B3/S23 --tilings hat --size L4 --soups 32` |
| `census` | How whole rules behave: died, frozen, oscillating, active or exploded | `jetcarp search census --tilings hex --sample 50 --birth 2-3 --survive 1-4` |
| `ants` | Highways and other outcomes of ants and turmites | `jetcarp search ants --tilings hex --strings 2-3` |
| `mover` | How far a glider on an aperiodic tiling really travels | see [Aperiodic patches and following movers](aperiodic-patches.md) |
| `pile` | A large sandpile | see [Large sandpiles](large-sandpiles.md) |
| `collide` | What two patterns make when they meet, at every offset and phase | `jetcarp search collide A B --offsets -6..6 --sheet hits.png` |
| `evolve` | Gliders, by evolving rules (James Edmond's method) | `jetcarp search evolve --tiling spectre --gens 200` |
| `explore` | Interesting runs of any rule family on any tiling | below |

`jetcarp search STRATEGY --help` lists each one's options. A few examples of what comes back:

- **Bounded, 32 soups of Life on a hat patch** took two seconds and stored 82 still lifes and
  oscillators. Aperiodic tilings have no translation, so this is how to search them.
- **The census of 50 rules on hexagons** prints a table with the share of runs that died, froze,
  oscillated, stayed active or exploded under each rule, and names the rules it knows (`B3/S12`
  is *Flock*).
- **Ants with turn strings of 2 and 3 letters on hexagons** took one second for 98 rules and found
  19 highways, among them `L2L2L`, which builds a highway of period 1,940 only after 367,643
  steps. They are kept under `jetcarp library ants`.

## The exploratory search

`explore` searches across tilings, rule families, rules and seeds at once and keeps the most
interesting run of each kind it sees, scored for how lively, varied and long-lasting it is (the
method is quality-diversity search; see [Search methods](../research/search-methods.md#explore)).
It runs until stopped, keeps every run in the job's own folder, and is best given a long time and
the GPU:

```text
$ jetcarp search explore --gpu --minutes 60 --label first-look
```

A budget ends it on time: the GPU's last batch is sized to end before it (and given up if it runs
on), and runs still going on the processor then are given up, unrecorded. A run on the processor
stops at 8 seconds (a confirmation run at 32) and is kept as a run of the generations it reached.
Its progress (time and runs) is written every two seconds, and `jetcarp jobs stop` stops it within
seconds. Unless given `--threads`, it leaves two of the computer's cores free for other work, and a
third of the rest prepare the GPU's batches; `jetcarp jobs status` says how many. A search of a few
minutes sees only a handful of tilings. While it runs, or once it has stopped:

```text
$ jetcarp jobs runs 7 --limit 5
     1836   2.43  cycle   greenberg_hastings snubhex          GH 20/2
      984   2.37  cycle   hodgepodge         square           Hodgepodge 117/2/2/24
     3879   2.21  cycle   hodgepodge         hat              Hodgepodge 130/1/4/47
     …
```

- `jetcarp jobs sheet JOB --ids …` draws a contact sheet of chosen runs.
- `jetcarp jobs scene JOB RUN` gives a scene that replays a run, to open in the app.
- `jetcarp jobs clips JOB --out DIR` renders short clips of a sample of runs, to rate by eye.
  `jetcarp jobs review JOB RUN keep|maybe|drop` records a verdict, and
  `jetcarp jobs rescore JOB` scores the runs again after the score changes.
- `jetcarp quickstart publish JOB` puts the search's best confirmed runs, a few per category, rule
  family and tiling, into your Quick Start menu as its *atlas*.

Unlike the other strategies, `explore` is not deterministic: a stopped one carries on from its
saved archive rather than replaying run for run.

## In the app

*Tools → Hunts…* starts, watches, stops and resumes searches, and lists every job with its progress
and finds. *Tools → Ants and turmites…* and *Tools → Large sandpile…* do the same for their
strategies, *Tools → Catalog tools…* runs collisions and the evolutionary search, and
*Tools → Pattern library…* browses what all of them found. A job started in the app can be stopped
or resumed from the command line, and the other way round.
