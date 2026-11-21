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
classes every object left behind. This is the method of Adam P. Goucher's apgsearch and of his
Catagolue census, which has classed about 7.4 × 10¹⁵ objects from asymmetric soups of Life alone
([Goucher 2016](../research/bibliography.md#goucher2016apgsearch);
[Goucher 2015](../research/bibliography.md#goucher2015catagolue)), and the hunt's soups are
apgsearch's, so a hunt of Life on squares can be compared with Catagolue: it agrees
within chance (see [Soup hunt](../research/search-methods.md#soup-hunt), which also says how the two
differ). With no options it hunts Conway's Life on squares:

```sh
jetcarp search hunt --soups 20000
```

It prints:

```text
hunt job 1 in /home/ada/.local/share/jetcarp/library.sqlite
hunt 1: 20,000/20,000 soups, 5954 soups/s, 433,183 objects (ships 38,381, osc 125,411, still 269,391), 50 distinct, 50 new | newest: still life 12c #bf73c4 (B3/S23)
  status done, 3 s run, 19946 settled, 52 died, 2 exploded, 0 unsettled
  B3/S23 on Square: 20000 soups, 433183 objects (oscillator 125411, spaceship 38381, still life 269391), 50 distinct, 50 new
  newest finds:
    bf73c46c186561ca3b75a3884d0b5ab5  still life 12c #bf73c4             B3/S23     Square soup 17973  (new to the catalog)
    …
```

The outputs on this page come from Linux; on macOS and Windows the folders in them differ (see
[Folders](../reference/folders.md#the-three-folders)).

Each soup is 16 × 16 cells at half density. Of 20,000 soups, 19,946 settled, 52 died out and 2
grew past the hunt's limit. The 433,183 objects they left are 50 distinct patterns. Every new one
is verified exactly before it is stored (run alone, it must come back to itself, moved by its
displacement, for many periods), and each copy found counts as a sighting.

Objects that merely sit side by side are counted apart: a pulsar with four blinkers beside it
counts as one pulsar and four blinkers, and two gliders flying together as two gliders. Objects
that do act on each other, such as a pair of ships bound together, stay one object (see
[Compounds](../research/search-methods.md#compounds)).

Look at what it found:

```sh
jetcarp library list --class spaceship
```

It prints:

```text
1376072ec0c084fc67a9b92d361379f3  HWSS     B3/S23  Square  p4  13c  seen 4      score 74.6  new  c/2 orthogonal
5a080db389c0cab3caf884d2596a1e70  glider   B3/S23  Square  p4   5c  seen 38312  score 74.5  new  c/4 diagonal
77364004b6d2c23cba74126b8aeb6ca3  MWSS     B3/S23  Square  p4  11c  seen 13     …
6dd55e69689ba784aff448b7dbbc9328  LWSS     B3/S23  Square  p4   9c  seen 52     …
```

Patterns with a well-known name (the glider, the pulsar, the block) are shown by it; others by
their class, cells and the start of their id. `jetcarp library show ID` prints a record, with a
text drawing of its phases, and `jetcarp library render ID` draws it. The
[pattern library](pattern-library.md) guide covers curating finds.

## Other rules and tilings

`--rules` takes one rule or several, in [rule notation](../reference/rule-notation.md), and
`--tilings` takes tiling ids such as `trihex` or `k2-001` (`catalog:k2-001` works too) or whole
sets: `archimedean`, `kuniform:2`, `isohedral`, `all-periodic`. A short hunt of Life on the kagome tiling:

```sh
jetcarp search hunt --tilings trihex --soups 5000 --label kagome
```

It ends with:

```text
  status done, 27 s run, 52 settled, 19 died, 4929 exploded, 0 unsettled
  B3/S23 on 3.6.3.6 trihexagonal: 5000 soups, 22950 objects (oscillator 5, spaceship 22857, still life 88), 32 distinct, 22 new
```

Nearly every soup explodes there, yet 22,857 spaceships escape, and all of them are one ship: the
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

```sh
jetcarp jobs list
```

It prints:

```text
    2  hunt     done     2026-11-12T07:02:17Z  hunt 2: 5,000/5,000 soups, 187 soups/s, 22,950 objects (ships 22,857, osc 5, still 88), 32 distinct, 22 new
    1  hunt     done     2026-11-12T07:02:04Z  hunt 1: 20,000/20,000 soups, 5954 soups/s, 433,183 objects …
```

- **Progress.** In a terminal, a search's progress line is redrawn in place on standard error, cut
  to the terminal's width: its least important part, such as a hunt's *newest:*, goes first.
  Written to a file or a pipe, each line is whole.
- **Budgets.** `--minutes M` or `--until UNIX-TIME` stops a search, still resumable, at its next
  checkpoint after that time (the exploratory search ends at that time; see below), and `--limit N`
  after N items in that session (an explore search checks it between its steps of a few seconds, so
  it finishes the step it is in: `--limit 20` can give 240 runs, and `--minutes` suits it better).
  The job keeps its minutes and its end: `jetcarp jobs resume JOB` given no budget of its own runs
  the minutes left, counting the time it has run in every session, and keeps the same end, so a
  search started with `--minutes 120` and cut off after an hour runs one more hour. Once they are
  used up, a resume runs until it is stopped. Either way it says which.
- **Memory.** `--max-rss-gb GB` pauses a hunt, a bounded search or an explore search, still
  resumable, once JETCARP uses more memory than that. The job keeps the cap, so a resumed run keeps
  to it too, and `jetcarp jobs status JOB` says why it paused. An explore search reads its memory
  every half second, even inside a step of a minute; `jetcarp jobs status` shows that reading and
  the session's highest. Over the cap it first gives back what it can (the memory it has freed, and
  the grids it keeps for reuse but is not using) and reads again, and pauses only once its memory
  has stayed over the cap for 10 seconds, so a moment's peak does not pause it but a lasting one
  does. It keeps the grids it has built for reuse, up to about 3 GB, so over hours its memory grows
  towards that as it reaches more tilings; on Linux it gives the memory it has freed back to the
  system every 30 seconds. "Memory" is the physical footprint on macOS, the figure Activity
  Monitor shows, and the resident size elsewhere (the working set on Windows).
- **Stopping.** Ctrl-C, or `jetcarp jobs stop JOB` from another terminal, stops it at its next
  checkpoint.
- **Carrying on.** `jetcarp jobs resume JOB` carries it on from there. It ends exactly where an
  uninterrupted run would have ended, at any thread count.
- **In the background.** `--detach` runs a search in a process of its own and returns as soon as
  that process has taken the job (it says so if the process has not taken it within 30 seconds);
  `jetcarp jobs status JOB` follows it, and `--label NAME` makes it easy to find. `jetcarp jobs
  resume JOB --detach` waits for a run that is letting go after a stop, and is refused while
  another process still runs the job.
- **Waiting.** `jetcarp jobs wait JOB` returns once a job stops running, wherever it runs. A job
  asked to stop is marked paused at once but lets go at its next checkpoint, and `jobs wait` waits
  for that too, so a `jetcarp jobs resume` straight after it finds the job free. Meanwhile it shows
  the job's progress line each time the line changes (`--quiet` for none), and `--timeout S` gives
  up after S seconds. Its exit status says how the job ended (see below).
- **Reports.** `jetcarp jobs report JOB` writes a Markdown report of what a job searched, for how
  long, and what it found: one table per hunted rule, with its spaceships, guns, puffers and
  longest oscillators.

`jetcarp jobs remove JOB` deletes a job that has finished or stopped, or whose process has gone
(*stale*, below), with its sightings; `--delete-results` also deletes the patterns only it found.

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
A run whose process crashed or was killed is noticed at once: the system lets go of the job's lock
when the process ends. `jetcarp jobs status` and `jetcarp jobs list` call such a job *stale*
(`--status stale` lists them), and `jetcarp jobs resume JOB` carries it on from its last
checkpoint (an explore search from its archive), so a long search can be kept going through
crashes until a deadline with a loop in the shell:

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
It runs until stopped, keeps every run in the job's own folder, and is best given a long time. It
runs on the GPU when there is a usable one, as the [GPU setting](gpu.md) says; `--cpu` keeps a
search on the processor and `--gpu` on the GPU whatever the setting, in this and later runs:

```sh
jetcarp search explore --minutes 60 --label first-look
```

A budget ends it on time: the GPU's last batch is sized to end before it (and given up if it runs
on), and runs still going on the processor then are given up, unrecorded. A run on the processor
stops at 8 seconds (a confirmation run at 32) and is kept as a run of the generations it reached.
A screening run's grid has about 16,000 cells (`--screen-cells`), and an elite is confirmed on one
of about 64,000 (`--confirm-cells`). An aperiodic patch comes only in levels of about 500, 4,000,
35,000 and 280,000 tiles, so it is the level nearest: 4,000 tiles to screen and 35,000 to confirm
(a search made before 0.4.3 runs both on 35,000).
Its progress (time, runs and memory) is written every two seconds, with the runs that have ended
since, so a search that is killed loses at most those two seconds, and `jetcarp jobs stop` stops it
within seconds. Unless given `--threads`, it leaves two of the computer's cores free for other work, and a
third of the rest prepare the GPU's batches; `jetcarp jobs status` says how many. A search of a few
minutes sees only a handful of tilings. While it runs, or once it has stopped:

```sh
jetcarp jobs runs 7 --limit 5
```

It prints:

```text
     1836   2.43  cycle   greenberg_hastings snubhex          GH 20/2
      984   2.37  cycle   hodgepodge         square           Hodgepodge 117/2/2/24
     3879   2.21  cycle   hodgepodge         hat              Hodgepodge 130/1/4/47
     …
```

- `jetcarp jobs sheet JOB --ids …` draws a contact sheet of chosen runs.
- `jetcarp jobs scene JOB RUN` gives a scene that replays a run, to open in the app.
- `jetcarp jobs runs JOB --measures all --format csv --limit 0 > runs.csv` writes every run with
  every measure, for a spreadsheet or a notebook.
- `jetcarp jobs clips JOB --out DIR` renders short clips of a sample of runs, to rate by eye
  (`--sample N` of them: a run of each family and tiling first, then more runs of the same tilings,
  each with a rule of its own, so a search on a few tilings gives as many as asked for; the
  categories people seldom keep, *Frozen at once*, *Quick deaths*, *Static* and *Quiet rows*, get
  only a few slots each, a sixteenth of the sample and at least 2, while the others have runs to
  give, and the output says how many each category got);
  `--page` adds a page for rating them in a browser.
  `jetcarp jobs review JOB RUN keep|maybe|drop` records a verdict, `jetcarp jobs runs JOB
  --reviewed keep` lists the runs given one, with each run's verdicts and star (`[keep ★]`), and
  `jetcarp jobs rescore JOB` scores the runs again after the score changes. A rescore picks the
  elites again by the new scores, and the job's line counts how many of them are confirmed.
- `jetcarp jobs refit JOB --out taste.json` fits a score model to your verdicts and says how well
  it ranks runs it did not see. `jetcarp jobs rescore JOB --score taste.json` scores the job's runs
  with it, and `jetcarp search explore --score taste.json` searches with it. The job keeps the
  model, and `jetcarp jobs status` names it: `score: the score model 'Ada, jaca on explore job 2'
  (for jaca runs)`.
- `jetcarp quickstart publish JOB` puts the search's best confirmed runs, a few per category, rule
  family and tiling, into your Quick Start menu as its *atlas*.

Unlike the other strategies, `explore` is not deterministic: a stopped one carries on from its
saved archive rather than replaying run for run.

## In the app

*Tools → Hunts…* starts, watches, stops and resumes searches, and lists every job with its progress
and finds. *Tools → Ants and turmites…* and *Tools → Large sandpile…* do the same for their
strategies, *Tools → Catalog tools…* runs collisions and the evolutionary search, and
*Tools → Pattern library…* browses what all of them found. A job started in the app can be stopped
or resumed from the command line, and the other way round. In the *Hunts* window, a job whose
process has gone shows as *stale* at once, and *Render media (animations of these finds)*, beside
a job's finds, writes each find's animation, poster and thumbnail in the background (see
[Pictures and animations](pictures.md#animations)).
